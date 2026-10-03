use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuthorizationMode, Capability, CapabilityGrant, CapabilityRule, DomainId, Entity, EntityId,
    EntityRetirement, EntityRetirementId, EntityTypeDefinition, EntityTypeId, GrantEffect,
    HistorySpaceDefinition, HistorySpaceId, Lifecycle, OperationId, PolicyRuleId, PolicyScope,
    PolicySubject, Principal, PrincipalId, Record, RecordKind, Revision, SecurityEpoch,
    SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion, Symbol,
};
use worlddb_storage_file::{
    HistorySegmentStore, IndexGenerationStore, LogicalExportManager, LogicalExportScope,
    ManifestSegmentKind, ManifestSegmentReference, ManifestSnapshot, PurgeApproval,
    PurgeCascadePlan, PurgeError, PurgeExternalArtifact, PurgeExternalArtifactKind,
    PurgePlanManager, PurgeSidecarInventory, RecoveryManager, SecurityPolicyHistoryStore,
    WalPrepareLog,
};

static NEXT_AREA: AtomicU64 = AtomicU64::new(0);

struct TestArea(PathBuf);

impl TestArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_AREA.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "worlddb-m7-14-purge-e2e-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for TestArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn id<T: DomainId>(tail: u8) -> Result<T, String> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).map_err(|error| error.to_string())
}

fn install_fixture(
    layout: &worlddb_storage_file::DatabaseLayout,
) -> Result<(HistorySpaceId, EntityId), String> {
    let history_space = id::<HistorySpaceId>(1)?;
    let entity_type = id::<EntityTypeId>(2)?;
    let entity = id::<EntityId>(3)?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let records = [
        Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(history_space, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ),
        Record::EntityTypeDefinition(EntityTypeDefinition::new(
            entity_type,
            Symbol::new("purge_fixture").map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            Revision::FIRST_COMMIT,
        )),
        Record::Entity(Entity::new(entity, entity_type, Revision::FIRST_COMMIT)),
        Record::EntityRetirement(EntityRetirement::new(
            id::<EntityRetirementId>(4)?,
            entity,
            Revision::FIRST_COMMIT,
        )),
    ];
    let history_receipt = HistorySegmentStore::new(layout.clone())
        .write_segment(&lock, &records)
        .map_err(|error| error.to_string())?;
    let policy_version = SecurityPolicyVersion::new(
        Revision::GENESIS,
        SecurityEpoch::INITIAL,
        SecurityPolicySnapshot::default(),
    );
    let policy_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(&lock, &policy_version, None, None)
        .map_err(|error| error.to_string())?;
    let references = vec![
        ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            Revision::FIRST_COMMIT,
        ),
        ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            policy_receipt.id(),
            policy_receipt.content_digest(),
            Revision::GENESIS,
        ),
    ];
    ManifestSnapshot::new(Revision::FIRST_COMMIT, references.clone())
        .map_err(|error| error.to_string())?;
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, id::<OperationId>(5)?, references, &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok((history_space, entity))
}

fn export_policy() -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(6)?;
    let grant = CapabilityGrant::new(Capability::DataExport, GrantEffect::Allow);
    let rule = CapabilityRule::new(
        id::<PolicyRuleId>(7)?,
        PolicySubject::Principal(principal),
        grant,
        PolicyScope::project(),
    );
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], vec![rule])
            .map_err(|error| error.to_string())?;
    let version = SecurityPolicyVersion::new(Revision::GENESIS, SecurityEpoch::INITIAL, snapshot);
    SecurityPolicyHistory::new(Revision::GENESIS, vec![version]).map_err(|error| error.to_string())
}

#[test]
fn purge_plan_reports_references_and_requires_the_exact_cascade_without_writing()
-> Result<(), String> {
    let area = TestArea::create()?;
    let layout = worlddb_storage_file::DatabaseLayout::create(area.0.join("database"))
        .map_err(|error| error.to_string())?;
    let (history_space, entity) = install_fixture(&layout)?;
    let permissions = export_policy()?;
    let view = permissions
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(6)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let selected_kinds = RecordKind::ALL
        .into_iter()
        .filter(|kind| {
            !matches!(
                kind,
                RecordKind::MigrationPlan
                    | RecordKind::MigrationRun
                    | RecordKind::MigrationStepCommitIdentity
            )
        })
        .collect();
    let source_artifact = LogicalExportManager::new(layout.clone())
        .export(
            LogicalExportScope::new(
                Revision::GENESIS,
                Revision::FIRST_COMMIT,
                vec![history_space],
                selected_kinds,
            )
            .map_err(|error| error.to_string())?,
            view,
        )
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;

    let source_database_id = layout
        .database_id()
        .ok_or_else(|| String::from("database identity missing"))?;
    let index_inventory =
        IndexGenerationStore::inventory_all(&layout).map_err(|error| error.to_string())?;
    let sidecars = PurgeSidecarInventory::new(
        index_inventory,
        vec![PurgeExternalArtifact::new(
            PurgeExternalArtifactKind::ExactBackup,
            [8; 32],
        )],
        false,
    )
    .map_err(|error| error.to_string())?;
    let target = worlddb_storage_file::LogicalImportIdentity::Entity(entity);
    let preview = PurgePlanManager::preview(&source_artifact, vec![target], sidecars.clone())
        .map_err(|error| error.to_string())?;
    assert_eq!(preview.source_database_id(), source_database_id);
    assert_eq!(preview.target_records().len(), 1);
    assert_eq!(preview.dependants().len(), 1);
    assert!(!preview.external_inventory_complete());
    assert_eq!(preview.retained_external_artifacts().len(), 1);
    assert!(matches!(
        preview.clone().approve_reject_if_referenced(),
        Err(PurgeError::ReferencesRemain)
    ));
    assert!(matches!(
        preview
            .clone()
            .approve_cascade(&PurgeCascadePlan::new(Vec::new()).map_err(|e| e.to_string())?),
        Err(PurgeError::CascadePlanMismatch)
    ));
    let cascade =
        PurgeCascadePlan::new(preview.dependants().to_vec()).map_err(|error| error.to_string())?;
    let approved = preview
        .approve_cascade(&cascade)
        .map_err(|error| error.to_string())?;
    assert_eq!(approved.approval(), Some(PurgeApproval::CascadePlan));

    let reopened = worlddb_storage_file::DatabaseLayout::open(layout.root())
        .map_err(|error| error.to_string())?;
    assert_eq!(reopened.database_id(), Some(source_database_id));
    let lock = reopened
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    assert_eq!(
        WalPrepareLog::new(&reopened)
            .commit_head(&lock)
            .map_err(|error| error.to_string())?
            .revision(),
        Revision::FIRST_COMMIT
    );
    Ok(())
}
