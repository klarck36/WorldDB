use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditPolicyFingerprint, AuthorizationMode, Bytes, Capability, CapabilityGrant, CapabilityRule,
    DomainId, Entity, EntityId, EntityRetirement, EntityRetirementId, EntityTypeDefinition,
    EntityTypeId, GrantEffect, HistorySpaceDefinition, HistorySpaceId, Lifecycle, OperationId,
    PerspectiveDefinitionRevision, PerspectiveId, PolicyRuleId, PolicyScope, PolicySubject,
    Principal, PrincipalId, Record, RecordKind, Revision, SecurityEpoch, SecurityPolicyHistory,
    SecurityPolicySnapshot, SecurityPolicyVersion, Symbol,
};
use worlddb_storage_file::{
    HistorySegmentStore, IndexGenerationStore, LogicalExportManager, LogicalExportScope,
    ManifestSegmentKind, ManifestSegmentReference, PurgeApproval, PurgeCascadePlan,
    PurgePlanManager, PurgeRewriteError, PurgeRewriteManager, PurgeRewriteRequest,
    PurgeSidecarInventory, RecoveryManager, SecurityPolicyHistoryStore, StorageVerifier,
    WalPrepareLog,
};

static NEXT_AREA: AtomicU64 = AtomicU64::new(0);

struct TestArea(PathBuf);

impl TestArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_AREA.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "worlddb-m7-15-purge-e2e-{}-{sequence}",
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

fn policy() -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(6)?;
    let rules = [Capability::Purge, Capability::DataExport]
        .into_iter()
        .enumerate()
        .map(|(index, capability)| {
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(7 + u8::try_from(index).map_err(|e| e.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(|error| error.to_string())?;
    let version = SecurityPolicyVersion::new(Revision::GENESIS, SecurityEpoch::INITIAL, snapshot);
    SecurityPolicyHistory::new(Revision::GENESIS, vec![version]).map_err(|error| error.to_string())
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
    let permissions = policy()?;
    let policy_version = permissions
        .versions()
        .first()
        .ok_or_else(|| String::from("initial policy missing"))?;
    let policy_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(&lock, policy_version, None, None)
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
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, id::<OperationId>(5)?, references, &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    let second_revision = Revision::new(2).map_err(|error| error.to_string())?;
    let perspective = Record::PerspectiveDefinitionRevision(
        PerspectiveDefinitionRevision::new(
            id::<PerspectiveId>(9)?,
            Some(String::from("retained")),
            Some(String::from("revision two survives the rewrite")),
            second_revision,
        )
        .map_err(|error| error.to_string())?,
    );
    let second_history = HistorySegmentStore::new(layout.clone())
        .write_segment(&lock, &[perspective])
        .map_err(|error| error.to_string())?;
    let current = worlddb_storage_file::ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("manifest missing after first commit"))?;
    let mut second_references = current.segments().to_vec();
    second_references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        second_history.id(),
        second_history.content_digest(),
        second_revision,
    ));
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, id::<OperationId>(10)?, second_references, &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok((history_space, entity))
}

fn scope(history_space: HistorySpaceId) -> Result<LogicalExportScope, String> {
    let record_kinds = RecordKind::ALL
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
    LogicalExportScope::new(
        Revision::GENESIS,
        Revision::new(2).map_err(|error| error.to_string())?,
        vec![history_space],
        record_kinds,
    )
    .map_err(|error| error.to_string())
}

fn fingerprint() -> Result<AuditPolicyFingerprint, String> {
    AuditPolicyFingerprint::new(Bytes::new(vec![0x4d, 0x37, 0x15]))
        .map_err(|error| error.to_string())
}

#[test]
fn offline_rewrite_publishes_a_new_verified_database_with_required_audit_and_report()
-> Result<(), String> {
    let area = TestArea::create()?;
    let source = worlddb_storage_file::DatabaseLayout::create(area.0.join("source"))
        .map_err(|error| error.to_string())?;
    let (history_space, entity) = install_fixture(&source)?;
    let perspective_id = id::<PerspectiveId>(9)?;
    let permissions = policy()?;
    let view = permissions
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(6)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let source_artifact = LogicalExportManager::new(source.clone())
        .export(scope(history_space)?, view)
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    let index_inventory =
        IndexGenerationStore::inventory_all(&source).map_err(|error| error.to_string())?;
    let sidecars = PurgeSidecarInventory::new(index_inventory, Vec::new(), true)
        .map_err(|error| error.to_string())?;
    let preview = PurgePlanManager::preview(
        &source_artifact,
        vec![worlddb_storage_file::LogicalImportIdentity::Entity(entity)],
        sidecars,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(preview.dependants().len(), 1);
    let cascade =
        PurgeCascadePlan::new(preview.dependants().to_vec()).map_err(|error| error.to_string())?;
    let plan = preview
        .approve_cascade(&cascade)
        .map_err(|error| error.to_string())?;
    assert_eq!(plan.approval(), Some(PurgeApproval::CascadePlan));

    let destination = area.0.join("purged");
    let receipt = PurgeRewriteManager::new()
        .rewrite(PurgeRewriteRequest {
            source_root: source.root(),
            source_artifact: &source_artifact,
            plan: &plan,
            known_external_artifacts: &[],
            external_inventory_complete: true,
            destination: &destination,
            policy: view,
            policy_target: worlddb_core::PolicyTarget::default(),
            policy_fingerprint: fingerprint()?,
        })
        .map_err(|error| error.to_string())?;

    let report = receipt.report();
    assert_ne!(
        report.source_database_id(),
        report.destination_database_id()
    );
    assert_eq!(
        report.source_revision(),
        Revision::new(2).map_err(|e| e.to_string())?
    );
    assert_eq!(
        report.destination_revision(),
        Revision::new(3).map_err(|e| e.to_string())?
    );
    assert_eq!(report.removed_records().len(), 2);
    assert_eq!(report.retained_external_artifacts().len(), 0);
    assert_eq!(report.index_families_to_rebuild().len(), 11);
    assert!(!report.secure_erase_claimed());
    let encoded_report = report.encode().map_err(|error| error.to_string())?;
    let secure_erase_claim_index = encoded_report
        .len()
        .checked_sub(33)
        .ok_or_else(|| String::from("encoded PurgeReport is shorter than its fixed trailer"))?;
    assert_eq!(encoded_report.get(secure_erase_claim_index), Some(&0));
    assert!(report.id_mappings().iter().any(|mapping| {
        mapping.source() == worlddb_storage_file::LogicalImportIdentity::Entity(entity)
            && mapping.destination().is_none()
    }));

    let rewritten = worlddb_storage_file::DatabaseLayout::open(&destination)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        rewritten.database_id(),
        Some(report.destination_database_id())
    );
    let lock = rewritten
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let verify = StorageVerifier::new(rewritten.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    assert!(verify.is_clean());
    let audits = WalPrepareLog::new(&rewritten)
        .committed_required_audit_records(&lock)
        .map_err(|error| error.to_string())?;
    assert!(audits.iter().any(|entry| {
        entry.operation_id() == report.operation_id()
            && entry.record().record_id() == report.audit_record_id()
            && entry.record().action() == worlddb_core::AuditAction::PurgePublication
            && entry.revision() == report.destination_revision()
    }));
    drop(lock);
    assert_eq!(
        fs::read(destination.join("PURGE_REPORT")).map_err(|error| error.to_string())?,
        report.encode().map_err(|error| error.to_string())?
    );
    let target_export = LogicalExportManager::new(rewritten)
        .export(scope(history_space)?, view)
        .map_err(|error| error.to_string())?;
    assert!(target_export
        .records()
        .iter()
        .all(|entry| !matches!(entry.record().record(), Record::Entity(value) if value.entity_id() == entity)));
    assert!(target_export.records().iter().any(|entry| {
        matches!(entry.record().record(), Record::PerspectiveDefinitionRevision(value) if value.perspective_id() == perspective_id)
    }));

    let source_layout = worlddb_storage_file::DatabaseLayout::open(source.root())
        .map_err(|error| error.to_string())?;
    let source_lock = source_layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    assert_eq!(
        WalPrepareLog::new(&source_layout)
            .commit_head(&source_lock)
            .map_err(|error| error.to_string())?
            .revision(),
        Revision::new(2).map_err(|error| error.to_string())?
    );
    drop(source_lock);
    let source_after = LogicalExportManager::new(source_layout)
        .export(scope(history_space)?, view)
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    assert_eq!(source_after, source_artifact);
    Ok(())
}

#[test]
fn purge_rewrite_fails_closed_without_approval_or_current_purge_permission() -> Result<(), String> {
    let area = TestArea::create()?;
    let source = worlddb_storage_file::DatabaseLayout::create(area.0.join("source"))
        .map_err(|error| error.to_string())?;
    let (history_space, entity) = install_fixture(&source)?;
    let permissions = policy()?;
    let view = permissions
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(6)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let source_artifact = LogicalExportManager::new(source.clone())
        .export(scope(history_space)?, view)
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    let inventory =
        IndexGenerationStore::inventory_all(&source).map_err(|error| error.to_string())?;
    let sidecars = PurgeSidecarInventory::new(inventory, Vec::new(), true)
        .map_err(|error| error.to_string())?;
    let plan = PurgePlanManager::preview(
        &source_artifact,
        vec![worlddb_storage_file::LogicalImportIdentity::Entity(entity)],
        sidecars,
    )
    .map_err(|error| error.to_string())?;
    let destination = area.0.join("unpublished");
    let no_purge = export_only_policy()?;
    let no_purge_view = no_purge
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(6)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let approved = plan
        .clone()
        .approve_cascade(
            &PurgeCascadePlan::new(plan.dependants().to_vec())
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        PurgeRewriteManager::new().rewrite(PurgeRewriteRequest {
            source_root: source.root(),
            source_artifact: &source_artifact,
            plan: &approved,
            known_external_artifacts: &[],
            external_inventory_complete: true,
            destination: &destination,
            policy: no_purge_view,
            policy_target: worlddb_core::PolicyTarget::default(),
            policy_fingerprint: fingerprint()?,
        }),
        Err(PurgeRewriteError::AuthorizationDenied)
    ));
    assert!(!destination.exists());
    assert!(matches!(
        PurgeRewriteManager::new().rewrite(PurgeRewriteRequest {
            source_root: source.root(),
            source_artifact: &source_artifact,
            plan: &plan,
            known_external_artifacts: &[],
            external_inventory_complete: true,
            destination: &destination,
            policy: view,
            policy_target: worlddb_core::PolicyTarget::default(),
            policy_fingerprint: fingerprint()?,
        }),
        Err(PurgeRewriteError::ApprovalMissing)
    ));
    assert!(!destination.exists());
    Ok(())
}

fn export_only_policy() -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(6)?;
    let rule = CapabilityRule::new(
        id::<PolicyRuleId>(7)?,
        PolicySubject::Principal(principal),
        CapabilityGrant::new(Capability::DataExport, GrantEffect::Allow),
        PolicyScope::project(),
    );
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], vec![rule])
            .map_err(|error| error.to_string())?;
    SecurityPolicyHistory::new(
        Revision::GENESIS,
        vec![SecurityPolicyVersion::new(
            Revision::GENESIS,
            SecurityEpoch::INITIAL,
            snapshot,
        )],
    )
    .map_err(|error| error.to_string())
}
