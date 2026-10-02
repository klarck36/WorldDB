use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    Capability, CapabilityGrant, CapabilityRule, DomainId, GrantEffect, JobBudget,
    MigrationCategory, MigrationId, MigrationPlan, MigrationPlanSpec, MigrationStepId,
    MigrationStepTargetSchema, MigrationTargetSchema, MigrationTransformerVersion, PolicyRuleId,
    PolicyScope, PolicySubject, PredicateId, Principal, PrincipalId, Revision, SchemaDefinitionId,
    SchemaIdentityTransition, SchemaRevision, SecurityEpoch, SecurityPolicyHistory,
    SecurityPolicySnapshot, SecurityPolicyVersion, SourceSchemaPrecondition,
};
use worlddb_storage_file::{
    DatabaseLayout, ExactBackupManager, HistorySegmentStore, ManifestSegmentKind,
    ManifestSegmentReference, ManifestStore, MigrationRestorePointError, RecoveryManager,
    WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("worlddb-m7-10a-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempArea {
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

fn permissions() -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(1)?;
    let rules = [
        Capability::MigrationExecute,
        Capability::BackupCreate,
        Capability::BackupRestore,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, capability)| -> Result<_, String> {
        Ok(CapabilityRule::new(
            id::<PolicyRuleId>(u8::try_from(index + 1).map_err(|error| error.to_string())?)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(capability, GrantEffect::Allow),
            PolicyScope::project(),
        ))
    })
    .collect::<Result<Vec<_>, String>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
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

fn breaking_plan(source_revision: Revision) -> Result<(MigrationPlan, MigrationStepId), String> {
    let migration_id = id::<MigrationId>(10)?;
    let step_id = id::<MigrationStepId>(11)?;
    let source_predicate = id::<PredicateId>(12)?;
    let target_predicate = id::<PredicateId>(13)?;
    let source_schema_revision = SchemaRevision::from_published_revision(source_revision);
    let target_revision = source_revision
        .next_commit()
        .map_err(|error| error.to_string())?;
    let target_schema = MigrationTargetSchema::new(
        SchemaRevision::from_published_revision(target_revision),
        [0x22; 32],
    );
    let plan = MigrationPlan::new(MigrationPlanSpec {
        migration_id,
        category: MigrationCategory::Breaking,
        source_schema: SourceSchemaPrecondition::new(source_schema_revision, [0x11; 32]),
        target_schema,
        steps: vec![step_id],
        step_targets: Some(vec![MigrationStepTargetSchema::new(step_id, target_schema)]),
        schema_changes: vec![
            SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(source_predicate)),
                Some(SchemaDefinitionId::Predicate(target_predicate)),
                MigrationCategory::Breaking,
            )
            .map_err(|error| error.to_string())?,
        ],
        transformer_version: MigrationTransformerVersion::new(1)
            .map_err(|error| error.to_string())?,
        calendar_shift: None,
        budget: JobBudget::new(100, 1024 * 1024).map_err(|error| error.to_string())?,
    })
    .map_err(|error| error.to_string())?;
    Ok((plan, step_id))
}

fn install_history_snapshot(layout: &DatabaseLayout) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let history_record = worlddb_core::Record::HistorySpaceDefinition(
        worlddb_core::HistorySpaceDefinition::new(
            id::<worlddb_core::HistorySpaceId>(20)?,
            None,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?,
    );
    let bytes = worlddb_core::encode_record(&history_record).map_err(|error| error.to_string())?;
    let decoded = worlddb_core::decode_record(&bytes).map_err(|error| error.to_string())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[decoded])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(
            &lock,
            id::<worlddb_core::OperationId>(21)?,
            vec![reference],
            &[],
        )
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    let report = worlddb_storage_file::StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    assert!(report.is_clean(), "{report}");
    assert_eq!(
        ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or("source manifest missing")?
            .revision(),
        Revision::FIRST_COMMIT
    );
    Ok(())
}

#[test]
fn breaking_migration_restore_point_requires_exact_backup_and_real_verified_clone()
-> Result<(), String> {
    let area = TempArea::create()?;
    let source = DatabaseLayout::create(area.path("source")).map_err(|error| error.to_string())?;
    install_history_snapshot(&source)?;
    let source_id = source
        .database_id()
        .ok_or("source database identity missing")?;
    let (plan, _) = breaking_plan(Revision::FIRST_COMMIT)?;
    let history = permissions()?;
    let view = history
        .select(
            worlddb_core::AuthorizationMode::Now,
            id::<PrincipalId>(1)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let backup_path = area.path("exact-backup");
    let restore_path = area.path("restore-test-clone");

    let proof = ExactBackupManager::new(source)
        .create_migration_safe_restore_point(
            &plan,
            &backup_path,
            &restore_path,
            None,
            view,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error: MigrationRestorePointError| error.to_string())?;

    assert_eq!(proof.plan_fingerprint(), plan.fingerprint());
    assert_eq!(proof.source_database_id(), source_id);
    assert_eq!(proof.source_revision(), Revision::FIRST_COMMIT);
    assert_ne!(proof.restored_database_id(), source_id);
    assert!(proof.restored_revision() > proof.source_revision());
    assert_ne!(proof.proof_fingerprint(), &[0; 32]);
    assert!(backup_path.join("EXACT_BACKUP").is_file());
    assert!(DatabaseLayout::open(&restore_path).is_ok());
    Ok(())
}
