use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOperationId, AuditOutcome,
    AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordId, AuditRecordIdentity,
    AuditSequence, AuthorizationMode, Bytes, Capability, CapabilityGrant, CapabilityRule,
    DatabaseId, DomainId, Entity, EntityId, EntityTypeDefinition, EntityTypeId, GrantEffect,
    HistorySpaceDefinition, HistorySpaceId, JobBudget, Lifecycle, MigrationCategory,
    MigrationDryRun, MigrationId, MigrationPlan, MigrationPlanSpec, MigrationRunId,
    MigrationRunJournalState, MigrationStepId, MigrationStepInput, MigrationStepTargetSchema,
    MigrationTargetSchema, MigrationTransformer, MigrationTransformerVersion, OperationId,
    PerspectiveDefinitionRevision, PerspectiveId, PolicyRuleId, PolicyScope, PolicySubject,
    PolicyTarget, PredicateId, Principal, PrincipalId, Record, RecordKind, Revision,
    SchemaDefinitionId, SchemaIdentityTransition, SchemaRevision, SecurityEpoch,
    SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion, SourceSchemaPrecondition,
    ValidatedMigrationDecisions, decode_record, encode_decoded_record, encode_record,
};
use worlddb_storage_file::{
    BackupProfile, DatabaseLayout, ExactBackupManager, FileStoreGuardedMigrationRun,
    HistorySegmentStore, LogicalExport, LogicalExportManager, LogicalExportScope,
    ManifestSegmentKind, ManifestSegmentReference, ManifestStore, RecoveryManager, RestoreManager,
    SecurityPolicyHistoryStore, SharingExport, SharingExportManager, SharingExportScope,
    StorageVerifier, WalPrepareLog, verify_exact_backup,
};

const MANIFEST_BLAKE3: &str = "e055937e2cc6a7613fc9d8c40a5a2e6ea9e18910c727d4eb6b3fcaa99cadea33";
const CAPTURE_ENV: &str = "WORLDDB_M7_16H_CAPTURE_FIXTURES";
const REFRESH_MANIFEST_ENV: &str = "WORLDDB_M7_16H_REFRESH_MANIFEST";
const FIXTURE_MANIFEST_HEADER: &str = "worlddb-m7-16h-fixtures-v2";
const FIXTURE_N1_STATUS: &str = "n1-applicability=not-applicable-before-first-alpha";
const STORAGE_PREFIX: &str = "storage/";
const BACKUP_PREFIX: &str = "exact-backup/";

static NEXT_AREA: AtomicU64 = AtomicU64::new(0);

struct TestArea(PathBuf);

impl TestArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_AREA.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "worlddb-m7-16h-fixture-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug)]
struct FixtureFile {
    path: String,
    length: usize,
    blake3: String,
}

#[test]
fn versioned_pre_alpha_fixture_baseline() -> Result<(), String> {
    if env::var_os(CAPTURE_ENV).as_deref() == Some(std::ffi::OsStr::new("1")) {
        capture_fixture_assets()?;
        return Ok(());
    }
    if env::var_os(REFRESH_MANIFEST_ENV).as_deref() == Some(std::ffi::OsStr::new("1")) {
        let root = fixture_root();
        write_fixture_manifest(&root)?;
        let manifest = fs::read(root.join("manifest.tsv")).map_err(|error| error.to_string())?;
        println!(
            "M7-16h fixture manifest BLAKE3: {}",
            blake3::hash(&manifest).to_hex()
        );
        return Ok(());
    }

    let entries = load_and_verify_fixture_manifest()?;
    let area = TestArea::create()?;
    let fixture_root = fixture_root();
    let actor = id::<PrincipalId>(1)?;
    let (permissions, policy_snapshot) = build_permissions()?;
    let history_space = id::<HistorySpaceId>(4)?;
    let source_revision = Revision::new(2).map_err(|error| error.to_string())?;

    let source =
        materialize_database_fixture(&fixture_root, &entries, STORAGE_PREFIX, area.path("source"))
            .map_err(|error| format!("materialize initial storage: {error}"))?;
    verify_clean_database(&source, source_revision)
        .map_err(|error| format!("verify initial storage: {error}"))?;
    let source_database_id = source
        .database_id()
        .ok_or_else(|| String::from("storage fixture has no DatabaseId"))?;
    if source_database_id != id::<DatabaseId>(1)? {
        return Err(String::from(
            "storage fixture DatabaseId differs from its pinned value",
        ));
    }

    let expected_logical = fs::read(fixture_root.join("exports/logical-export.wdbx"))
        .map_err(|error| error.to_string())?;
    let decoded_logical =
        LogicalExport::decode(&expected_logical).map_err(|error| error.to_string())?;
    if decoded_logical.manifest().database_id() != source_database_id
        || decoded_logical.manifest().snapshot_revision() != source_revision
        || decoded_logical.records().is_empty()
    {
        return Err(String::from(
            "logical-export fixture metadata or records differ",
        ));
    }
    let logical_view = policy_view(&permissions, actor)?;
    let source_scope = logical_scope(history_space, source_revision)?;
    let actual_logical = LogicalExportManager::new(source.clone())
        .export(source_scope, logical_view)
        .and_then(|export| export.encode())
        .map_err(|error| format!("export fixture source logically: {error}"))?;
    if actual_logical != expected_logical {
        return Err(String::from("logical-export fixture is not reproducible"));
    }

    let backup_root = materialize_file_group(
        &fixture_root,
        &entries,
        BACKUP_PREFIX,
        area.path("exact-backup"),
    )?;
    let backup = verify_exact_backup(&backup_root, None)
        .map_err(|error| format!("verify exact backup fixture: {error}"))?;
    if backup.profile() != BackupProfile::ExactDatabase
        || backup.database_id() != source_database_id
        || backup.revision() != source_revision
        || !backup.storage_report().is_clean()
    {
        return Err(String::from(
            "ExactDatabaseBackup fixture verification differs",
        ));
    }
    let restore_root = area.path("restored");
    let restored = RestoreManager::new()
        .restore_clone(
            &backup_root,
            &restore_root,
            None,
            policy_view(&permissions, actor)?,
            PolicyTarget::default(),
            policy_fingerprint()?,
        )
        .map_err(|error| format!("restore exact backup fixture: {error}"))?;
    if restored.source_database_id() != source_database_id
        || restored.restored_database_id() == source_database_id
        || restored.profile() != BackupProfile::ExactDatabase
    {
        return Err(String::from("restored fixture identity or profile differs"));
    }
    let restored_layout = DatabaseLayout::open(&restore_root)
        .map_err(|error| format!("reopen restored fixture: {error}"))?;
    verify_clean_database(
        &restored_layout,
        source_revision
            .next_commit()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("verify restored fixture: {error}"))?;
    let source_logical =
        LogicalExport::decode(&expected_logical).map_err(|error| error.to_string())?;
    let restored_logical = LogicalExportManager::new(restored_layout.clone())
        .export(
            logical_scope(history_space, source_revision)?,
            policy_view(&permissions, actor)?,
        )
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    let restored_logical =
        LogicalExport::decode(&restored_logical).map_err(|error| error.to_string())?;
    if record_frames(&source_logical)? != record_frames(&restored_logical)? {
        return Err(String::from(
            "restored fixture records differ from the source",
        ));
    }
    let restored_lock = restored_layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let restore_audits = WalPrepareLog::new(&restored_layout)
        .committed_required_audit_records(&restored_lock)
        .map_err(|error| error.to_string())?;
    if !restore_audits.iter().any(|entry| {
        entry.record().action() == AuditAction::RestorePublication
            && entry.record().record_id() == restored.audit_record_id()
            && entry.operation_id() == restored.operation_id()
            && entry.revision() == restored.restored_revision()
    }) {
        return Err(String::from(
            "restored fixture lacks its bound RestorePublication audit",
        ));
    }
    drop(restored_lock);

    let expected_sharing = fs::read(fixture_root.join("exports/sharing-export.wdbs"))
        .map_err(|error| error.to_string())?;
    let decoded_sharing =
        SharingExport::decode(&expected_sharing).map_err(|error| error.to_string())?;
    if decoded_sharing.from_revision() != Revision::GENESIS
        || decoded_sharing.through_revision() != source_revision
        || decoded_sharing.history_spaces() != [history_space]
        || decoded_sharing.records().is_empty()
    {
        return Err(String::from(
            "sharing-export fixture scope or records differ",
        ));
    }
    let sharing_source = copy_database_tree(&source, area.path("sharing-source"))?;
    let actual_sharing = SharingExportManager::new(sharing_source)
        .export(
            SharingExportScope::new(
                Revision::GENESIS,
                source_revision,
                vec![history_space],
                standard_record_kinds(),
            )
            .map_err(|error| error.to_string())?,
            policy_view(&permissions, actor)?,
        )
        .and_then(|export| export.encode())
        .map_err(|error| format!("export fixture for sharing: {error}"))?;
    if actual_sharing != expected_sharing {
        return Err(String::from("sharing-export fixture is not reproducible"));
    }

    let migration_source = materialize_database_fixture(
        &fixture_root,
        &entries,
        STORAGE_PREFIX,
        area.path("migration-source"),
    )?;
    let original_format_probe =
        fs::read(fixture_root.join("storage/FORMAT")).map_err(|error| error.to_string())?;
    let plan_bytes =
        fs::read(fixture_root.join("migration/plan.record")).map_err(|error| error.to_string())?;
    let plan_decoded = decode_record(&plan_bytes).map_err(|error| error.to_string())?;
    let Record::MigrationPlan(plan) = plan_decoded.record() else {
        return Err(String::from(
            "migration fixture plan is not a MigrationPlan record",
        ));
    };
    let plan = plan.clone();
    if plan.source_schema_precondition().revision().revision() != source_revision
        || plan.category() != MigrationCategory::Restrictive
        || plan.steps().len() != 2
    {
        return Err(String::from("migration fixture plan precondition differs"));
    }
    let final_revision = execute_migration_fixture(
        &migration_source,
        &fixture_root,
        &plan,
        &policy_snapshot,
        actor,
    )
    .map_err(|error| format!("execute guarded migration fixture: {error}"))?;
    if final_revision != Revision::new(4).map_err(|error| error.to_string())?
        || fs::read(migration_source.root().join("FORMAT")).map_err(|error| error.to_string())?
            != original_format_probe
    {
        return Err(String::from(
            "schema migration changed the storage format or final revision",
        ));
    }
    let expected_migrated_export =
        fs::read(fixture_root.join("migration/expected-logical-export.wdbx"))
            .map_err(|error| error.to_string())?;
    let actual_migrated_export = LogicalExportManager::new(migration_source.clone())
        .export(
            logical_scope(history_space, final_revision)?,
            policy_view(&permissions, actor)?,
        )
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    if actual_migrated_export != expected_migrated_export {
        return Err(String::from(
            "guarded migration fixture output differs from its golden",
        ));
    }
    verify_migration_audits(&migration_source, &plan)?;

    Ok(())
}

fn capture_fixture_assets() -> Result<(), String> {
    let fixture_root = fixture_root();
    fs::create_dir_all(&fixture_root).map_err(|error| error.to_string())?;
    let area = TestArea::create()?;
    let (source, permissions, space, entity_type) = seed_storage_database(area.path("source"))?;
    copy_data_tree(source.root(), &fixture_root.join("storage"))?;

    let backup_root = area.path("exact-backup");
    ExactBackupManager::new(source.clone())
        .create_exact_backup(&backup_root, None)
        .map_err(|error| error.to_string())?;
    copy_data_tree(&backup_root, &fixture_root.join("exact-backup"))?;

    let actor = id::<PrincipalId>(1)?;
    let source_revision = Revision::new(2).map_err(|error| error.to_string())?;
    let logical_bytes = LogicalExportManager::new(source.clone())
        .export(
            logical_scope(space, source_revision)?,
            policy_view(&permissions, actor)?,
        )
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    write_fixture_file(
        &fixture_root.join("exports/logical-export.wdbx"),
        &logical_bytes,
    )?;

    let sharing_source = copy_database_tree(&source, area.path("sharing-source"))?;
    let sharing_bytes = SharingExportManager::new(sharing_source)
        .export(
            SharingExportScope::new(
                Revision::GENESIS,
                source_revision,
                vec![space],
                standard_record_kinds(),
            )
            .map_err(|error| error.to_string())?,
            policy_view(&permissions, actor)?,
        )
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    write_fixture_file(
        &fixture_root.join("exports/sharing-export.wdbs"),
        &sharing_bytes,
    )?;

    let plan = migration_fixture_plan(source_revision)?;
    let plan_bytes =
        encode_record(&Record::MigrationPlan(plan.clone())).map_err(|error| error.to_string())?;
    write_fixture_file(&fixture_root.join("migration/plan.record"), &plan_bytes)?;
    let step_records = migration_fixture_records(entity_type)?;
    for (index, record) in step_records.iter().enumerate() {
        let bytes = encode_record(record).map_err(|error| error.to_string())?;
        let filename = if index == 0 {
            "migration/step-1.record"
        } else {
            "migration/step-2.record"
        };
        write_fixture_file(&fixture_root.join(filename), &bytes)?;
    }
    let (policy_history, policy_snapshot) = build_permissions()?;
    let migration_revision =
        execute_migration_fixture(&source, &fixture_root, &plan, &policy_snapshot, actor)?;
    let migrated_logical = LogicalExportManager::new(source.clone())
        .export(
            logical_scope(space, migration_revision)?,
            policy_view(&policy_history, actor)?,
        )
        .and_then(|export| export.encode())
        .map_err(|error| error.to_string())?;
    write_fixture_file(
        &fixture_root.join("migration/expected-logical-export.wdbx"),
        &migrated_logical,
    )?;

    write_fixture_manifest(&fixture_root)?;
    let manifest =
        fs::read(fixture_root.join("manifest.tsv")).map_err(|error| error.to_string())?;
    println!(
        "M7-16h fixture manifest BLAKE3: {}",
        blake3::hash(&manifest).to_hex()
    );
    Ok(())
}

fn seed_storage_database(
    root: PathBuf,
) -> Result<
    (
        DatabaseLayout,
        SecurityPolicyHistory,
        HistorySpaceId,
        EntityTypeId,
    ),
    String,
> {
    let created = DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
    fs::write(created.database_id_file(), id::<DatabaseId>(1)?.to_bytes())
        .map_err(|error| error.to_string())?;
    drop(created);
    let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
    let permissions = build_permissions()?.0;
    let space = id::<HistorySpaceId>(4)?;
    let entity_type = id::<EntityTypeId>(5)?;
    let entity = id::<EntityId>(6)?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;

    let records = [
        Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(space, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ),
        Record::EntityTypeDefinition(EntityTypeDefinition::new(
            entity_type,
            worlddb_core::Symbol::new("fixture_entity").map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            Revision::FIRST_COMMIT,
        )),
        Record::Entity(Entity::new(entity, entity_type, Revision::FIRST_COMMIT)),
    ];
    let history = HistorySegmentStore::new(layout.clone())
        .write_segment(&lock, &records)
        .map_err(|error| error.to_string())?;
    let policy_version = permissions
        .versions()
        .first()
        .ok_or_else(|| String::from("fixture policy has no initial version"))?;
    let security = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(&lock, policy_version, None, None)
        .map_err(|error| error.to_string())?;
    let mut references = vec![
        ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history.id(),
            history.content_digest(),
            Revision::FIRST_COMMIT,
        ),
        ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            security.id(),
            security.content_digest(),
            Revision::GENESIS,
        ),
    ];
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, id::<OperationId>(20)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    let second_revision = Revision::new(2).map_err(|error| error.to_string())?;
    let perspective = Record::PerspectiveDefinitionRevision(
        PerspectiveDefinitionRevision::new(
            id::<PerspectiveId>(9)?,
            Some(String::from("Versioned fixture")),
            Some(String::from("Pre-Alpha compatibility baseline")),
            second_revision,
        )
        .map_err(|error| error.to_string())?,
    );
    let second_history = HistorySegmentStore::new(layout.clone())
        .write_segment(&lock, &[perspective])
        .map_err(|error| error.to_string())?;
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("fixture first manifest is missing"))?;
    references = current.segments().to_vec();
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        second_history.id(),
        second_history.content_digest(),
        second_revision,
    ));
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, id::<OperationId>(21)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    let verification = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    if !verification.is_clean() {
        return Err(format!(
            "seeded storage fixture is not clean: {verification}"
        ));
    }
    drop(lock);
    Ok((layout, permissions, space, entity_type))
}

fn build_permissions() -> Result<(SecurityPolicyHistory, SecurityPolicySnapshot), String> {
    let actor = id::<PrincipalId>(1)?;
    let rules = Capability::ALL
        .into_iter()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            let tail = u8::try_from(index + 1).map_err(|error| error.to_string())?;
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(tail)?,
                PolicySubject::Principal(actor),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot = SecurityPolicySnapshot::new(vec![Principal::new(actor)], vec![], vec![], rules)
        .map_err(|error| error.to_string())?;
    let history = SecurityPolicyHistory::new(
        Revision::GENESIS,
        vec![SecurityPolicyVersion::new(
            Revision::GENESIS,
            SecurityEpoch::INITIAL,
            snapshot.clone(),
        )],
    )
    .map_err(|error| error.to_string())?;
    Ok((history, snapshot))
}

fn policy_view(
    permissions: &SecurityPolicyHistory,
    actor: PrincipalId,
) -> Result<worlddb_core::SecurityPolicyView<'_>, String> {
    permissions
        .select(AuthorizationMode::Now, actor, Revision::GENESIS)
        .map_err(|error| error.to_string())
}

fn policy_fingerprint() -> Result<AuditPolicyFingerprint, String> {
    AuditPolicyFingerprint::new(Bytes::new(vec![0x4d, 0x37, 0x16]))
        .map_err(|error| error.to_string())
}

fn logical_scope(
    history_space: HistorySpaceId,
    through_revision: Revision,
) -> Result<LogicalExportScope, String> {
    LogicalExportScope::new(
        Revision::GENESIS,
        through_revision,
        vec![history_space],
        standard_record_kinds(),
    )
    .map_err(|error| error.to_string())
}

fn standard_record_kinds() -> Vec<RecordKind> {
    RecordKind::ALL
        .into_iter()
        .filter(|kind| {
            !matches!(
                kind,
                RecordKind::MigrationPlan
                    | RecordKind::MigrationRun
                    | RecordKind::MigrationStepCommitIdentity
            )
        })
        .collect()
}

fn migration_fixture_plan(source_revision: Revision) -> Result<MigrationPlan, String> {
    let first_revision = source_revision
        .next_commit()
        .map_err(|error| error.to_string())?;
    let second_revision = first_revision
        .next_commit()
        .map_err(|error| error.to_string())?;
    let first_step = id::<MigrationStepId>(22)?;
    let second_step = id::<MigrationStepId>(23)?;
    let first_target = MigrationTargetSchema::new(
        SchemaRevision::from_published_revision(first_revision),
        [0x21; 32],
    );
    let second_target = MigrationTargetSchema::new(
        SchemaRevision::from_published_revision(second_revision),
        [0x22; 32],
    );
    let predicate = id::<PredicateId>(11)?;
    MigrationPlan::new(MigrationPlanSpec {
        migration_id: id::<MigrationId>(10)?,
        category: MigrationCategory::Restrictive,
        source_schema: SourceSchemaPrecondition::new(
            SchemaRevision::from_published_revision(source_revision),
            [0x11; 32],
        ),
        target_schema: second_target,
        steps: vec![first_step, second_step],
        step_targets: Some(vec![
            MigrationStepTargetSchema::new(first_step, first_target),
            MigrationStepTargetSchema::new(second_step, second_target),
        ]),
        schema_changes: vec![
            SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(predicate)),
                Some(SchemaDefinitionId::Predicate(predicate)),
                MigrationCategory::Restrictive,
            )
            .map_err(|error| error.to_string())?,
        ],
        transformer_version: MigrationTransformerVersion::new(1)
            .map_err(|error| error.to_string())?,
        calendar_shift: None,
        budget: JobBudget::new(100, 1024 * 1024).map_err(|error| error.to_string())?,
    })
    .map_err(|error| error.to_string())
}

fn migration_fixture_records(entity_type: EntityTypeId) -> Result<Vec<Record>, String> {
    let first = Record::Entity(Entity::new(
        id::<EntityId>(50)?,
        entity_type,
        Revision::new(3).map_err(|error| error.to_string())?,
    ));
    let second = Record::PerspectiveDefinitionRevision(
        PerspectiveDefinitionRevision::new(
            id::<PerspectiveId>(51)?,
            Some(String::from("Migrated fixture")),
            Some(String::from("Guarded migration result")),
            Revision::new(4).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?,
    );
    Ok(vec![first, second])
}

fn migration_inputs(
    fixture_root: &Path,
    plan: &MigrationPlan,
) -> Result<Vec<MigrationStepInput>, String> {
    let first_step = plan
        .steps()
        .first()
        .copied()
        .ok_or_else(|| String::from("migration fixture has no first step"))?;
    let second_step = plan
        .steps()
        .get(1)
        .copied()
        .ok_or_else(|| String::from("migration fixture has no second step"))?;
    let first = fs::read(fixture_root.join("migration/step-1.record"))
        .map_err(|error| error.to_string())?;
    let second = fs::read(fixture_root.join("migration/step-2.record"))
        .map_err(|error| error.to_string())?;
    decode_record(&first).map_err(|error| error.to_string())?;
    decode_record(&second).map_err(|error| error.to_string())?;
    Ok(vec![
        MigrationStepInput::new(first_step, id::<OperationId>(30)?, vec![first]),
        MigrationStepInput::new(second_step, id::<OperationId>(31)?, vec![second]),
    ])
}

fn migration_audits(
    plan: &MigrationPlan,
    inputs: &[MigrationStepInput],
    policy: &SecurityPolicySnapshot,
    actor: PrincipalId,
) -> Result<Vec<AuditRecord>, String> {
    let policy_fingerprint =
        policy.effective_capability_fingerprint(actor, PolicyTarget::default());
    inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let target = plan
                .step_targets()
                .and_then(|targets| targets.get(index))
                .ok_or_else(|| String::from("migration fixture step target is missing"))?;
            let index = u8::try_from(index).map_err(|error| error.to_string())?;
            let sequence = u64::from(index) + 1;
            Ok(AuditRecord::new(
                AuditRecordIdentity {
                    record_id: id::<AuditRecordId>(60 + index)?,
                    sequence: AuditSequence::new(sequence),
                    audit_operation_id: id::<AuditOperationId>(70 + index)?,
                },
                AuditRecordDetails {
                    actor,
                    action: AuditAction::Migration,
                    object_class: AuditObjectClass::Migration,
                    outcome: AuditOutcome::Succeeded,
                    commit_context: AuditCommitContext::Committed {
                        revision: target.schema().revision().revision(),
                        operation_id: input.operation_id(),
                    },
                    security_epoch: SecurityEpoch::INITIAL,
                    policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                        policy_fingerprint.to_vec(),
                    ))
                    .map_err(|error| error.to_string())?,
                },
            ))
        })
        .collect()
}

fn execute_migration_fixture(
    layout: &DatabaseLayout,
    fixture_root: &Path,
    plan: &MigrationPlan,
    policy: &SecurityPolicySnapshot,
    actor: PrincipalId,
) -> Result<Revision, String> {
    let source_revision = plan.source_schema_precondition().revision();
    let inputs = migration_inputs(fixture_root, plan)?;
    let records = inputs
        .iter()
        .flat_map(|input| input.records().iter().cloned())
        .collect::<Vec<_>>();
    let dry_run = MigrationDryRun::run(plan, source_revision, [0x11; 32], &records);
    let decisions = ValidatedMigrationDecisions::validate(
        plan,
        &dry_run,
        Vec::new(),
        policy,
        actor,
        PolicyTarget::default(),
    )
    .map_err(|error| error.to_string())?;
    let audits = migration_audits(plan, &inputs, policy, actor)?;
    let database_id = layout
        .database_id()
        .ok_or_else(|| String::from("migration fixture source has no DatabaseId"))?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let mut run = FileStoreGuardedMigrationRun::open(layout.clone(), &lock)
        .map_err(|error| error.to_string())?;
    let result = run
        .execute(
            plan,
            id::<MigrationRunId>(40)?,
            database_id,
            [0x11; 32],
            MigrationTransformer::for_version(plan.transformer_version())
                .map_err(|error| error.to_string())?,
            inputs,
            decisions,
            actor,
            policy,
            PolicyTarget::default(),
            SecurityEpoch::INITIAL,
            None,
            None,
            audits,
            |_, _, _, _| Ok::<(), String>(()),
        )
        .map_err(|error| error.to_string())?;
    let final_revision = result.final_revision();
    if final_revision
        != source_revision
            .revision()
            .next_commit()
            .and_then(Revision::next_commit)
            .map_err(|error| error.to_string())?
    {
        return Err(String::from(
            "migration fixture ended at an unexpected revision",
        ));
    }
    drop(run);
    drop(lock);

    let reopened = DatabaseLayout::open(layout.root())
        .map_err(|error| format!("reopen migrated fixture: {error}"))?;
    verify_clean_database(&reopened, final_revision)?;
    let reopened_lock = reopened
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let reopened_run = FileStoreGuardedMigrationRun::open(reopened.clone(), &reopened_lock)
        .map_err(|error| error.to_string())?;
    let journal = reopened_run
        .load_journal_status(id::<MigrationRunId>(40)?)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("migration fixture journal is missing after reopen"))?;
    if journal.state() != MigrationRunJournalState::Completed || journal.steps().len() != 2 {
        return Err(String::from(
            "migration fixture journal did not reopen Completed",
        ));
    }
    drop(reopened_lock);
    verify_migration_audits(&reopened, plan)?;
    Ok(final_revision)
}

fn verify_migration_audits(layout: &DatabaseLayout, plan: &MigrationPlan) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let audits = WalPrepareLog::new(layout)
        .committed_required_audit_records(&lock)
        .map_err(|error| error.to_string())?;
    let migration_audits = audits
        .iter()
        .filter(|entry| entry.record().action() == AuditAction::Migration)
        .collect::<Vec<_>>();
    if migration_audits.len() != plan.steps().len() {
        return Err(String::from(
            "migration fixture Required Audit count differs",
        ));
    }
    for (index, audit) in migration_audits.iter().enumerate() {
        let index = u8::try_from(index).map_err(|error| error.to_string())?;
        if audit.record().record_id() != id::<AuditRecordId>(60 + index)?
            || audit.record().sequence() != AuditSequence::new(u64::from(index) + 1)
            || audit.record().action() != AuditAction::Migration
            || audit.operation_id() != id::<OperationId>(30 + index)?
        {
            return Err(String::from(
                "migration fixture Required Audit binding differs",
            ));
        }
    }
    drop(lock);
    Ok(())
}

fn materialize_database_fixture(
    fixture_root: &Path,
    entries: &[FixtureFile],
    prefix: &str,
    destination: PathBuf,
) -> Result<DatabaseLayout, String> {
    DatabaseLayout::create(&destination).map_err(|error| {
        format!(
            "create materialized database fixture {}: {error}",
            destination.display()
        )
    })?;
    copy_manifest_group(fixture_root, entries, prefix, &destination)?;
    DatabaseLayout::open(&destination).map_err(|error| {
        format!(
            "open materialized database fixture {}: {error}",
            destination.display()
        )
    })
}

fn materialize_file_group(
    fixture_root: &Path,
    entries: &[FixtureFile],
    prefix: &str,
    destination: PathBuf,
) -> Result<PathBuf, String> {
    DatabaseLayout::create(&destination).map_err(|error| {
        format!(
            "create materialized file-group layout {}: {error}",
            destination.display()
        )
    })?;
    copy_manifest_group(fixture_root, entries, prefix, &destination)?;
    Ok(destination)
}

fn copy_manifest_group(
    fixture_root: &Path,
    entries: &[FixtureFile],
    prefix: &str,
    destination: &Path,
) -> Result<(), String> {
    let mut copied = 0_usize;
    for entry in entries
        .iter()
        .filter(|entry| entry.path.starts_with(prefix))
    {
        let relative = entry
            .path
            .strip_prefix(prefix)
            .ok_or_else(|| String::from("fixture prefix changed while copying"))?;
        if relative.is_empty() {
            return Err(String::from(
                "fixture manifest contains an empty artifact path",
            ));
        }
        let source = fixture_root.join(&entry.path);
        let target = destination.join(relative);
        let parent = target
            .parent()
            .ok_or_else(|| String::from("fixture target has no parent"))?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        fs::copy(source, target).map_err(|error| error.to_string())?;
        copied = copied
            .checked_add(1)
            .ok_or_else(|| String::from("fixture file count overflow"))?;
    }
    if copied == 0 {
        return Err(format!("fixture manifest contains no files below {prefix}"));
    }
    Ok(())
}

fn copy_database_tree(
    source: &DatabaseLayout,
    destination: PathBuf,
) -> Result<DatabaseLayout, String> {
    copy_data_tree(source.root(), &destination)?;
    DatabaseLayout::open(&destination).map_err(|error| {
        format!(
            "open copied database tree {}: {error}",
            destination.display()
        )
    })
}

fn copy_data_tree(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err(format!(
            "fixture capture target already exists: {}",
            destination.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    copy_data_tree_inner(source, source, destination)
}

fn copy_data_tree_inner(source: &Path, root: &Path, destination: &Path) -> Result<(), String> {
    for item in fs::read_dir(source).map_err(|error| error.to_string())? {
        let item = item.map_err(|error| error.to_string())?;
        let path = item.path();
        let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
        if relative == Path::new("LOCK") {
            continue;
        }
        let kind = item.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "fixture source contains a symlink: {}",
                path.display()
            ));
        }
        let target = destination.join(relative);
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            if relative == Path::new("staging") {
                continue;
            }
            copy_data_tree_inner(&path, root, destination)?;
        } else if kind.is_file() {
            let parent = target
                .parent()
                .ok_or_else(|| String::from("fixture copy target has no parent"))?;
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            fs::copy(path, target).map_err(|error| error.to_string())?;
        } else {
            return Err(format!(
                "fixture source contains a special file: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn verify_clean_database(
    layout: &DatabaseLayout,
    expected_revision: Revision,
) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let report = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    if !report.is_clean() || report.safe_revision() != expected_revision {
        return Err(format!("fixture storage verification differs: {report}"));
    }
    let head = WalPrepareLog::new(layout)
        .commit_head(&lock)
        .map_err(|error| error.to_string())?;
    if head.revision() != expected_revision {
        return Err(String::from(
            "fixture WAL head differs from its expected revision",
        ));
    }
    drop(lock);
    Ok(())
}

fn record_frames(export: &LogicalExport) -> Result<Vec<Vec<u8>>, String> {
    export
        .records()
        .iter()
        .map(|entry| encode_decoded_record(entry.record()).map_err(|error| error.to_string()))
        .collect()
}

fn id<T: DomainId>(tail: u8) -> Result<T, String> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).map_err(|error| error.to_string())
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m7-16h")
}

fn write_fixture_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| String::from("fixture file has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn write_fixture_manifest(root: &Path) -> Result<(), String> {
    let mut paths = Vec::new();
    collect_fixture_paths(root, root, &mut paths)?;
    paths.sort();
    let mut manifest = format!("{FIXTURE_MANIFEST_HEADER}\n{FIXTURE_N1_STATUS}\n");
    for path in paths {
        let bytes = fs::read(root.join(&path)).map_err(|error| error.to_string())?;
        manifest.push_str(&path);
        manifest.push('\t');
        manifest.push_str(&bytes.len().to_string());
        manifest.push('\t');
        let digest = blake3::hash(&bytes).to_hex();
        manifest.push_str(digest.as_ref());
        manifest.push('\n');
    }
    fs::write(root.join("manifest.tsv"), manifest).map_err(|error| error.to_string())
}

fn load_and_verify_fixture_manifest() -> Result<Vec<FixtureFile>, String> {
    let root = fixture_root();
    let manifest_bytes = fs::read(root.join("manifest.tsv")).map_err(|error| error.to_string())?;
    let manifest_digest = blake3::hash(&manifest_bytes).to_hex().to_string();
    if manifest_digest != MANIFEST_BLAKE3 {
        return Err(format!(
            "fixture manifest hash changed: expected {MANIFEST_BLAKE3}, found {manifest_digest}"
        ));
    }
    let manifest = std::str::from_utf8(&manifest_bytes).map_err(|error| error.to_string())?;
    let mut lines = manifest.lines();
    if lines.next() != Some(FIXTURE_MANIFEST_HEADER) || lines.next() != Some(FIXTURE_N1_STATUS) {
        return Err(String::from(
            "fixture manifest version or N-1 status differs",
        ));
    }
    let mut entries = Vec::new();
    let mut previous_path = None::<String>;
    for line in lines {
        let mut fields = line.split('\t');
        let path = fields
            .next()
            .ok_or_else(|| String::from("fixture manifest path is missing"))?;
        let length = fields
            .next()
            .ok_or_else(|| String::from("fixture manifest length is missing"))?
            .parse::<usize>()
            .map_err(|error| error.to_string())?;
        let digest = fields
            .next()
            .ok_or_else(|| String::from("fixture manifest digest is missing"))?;
        if fields.next().is_some() || path.is_empty() || digest.len() != 64 {
            return Err(String::from("fixture manifest row is malformed"));
        }
        validate_fixture_relative_path(path)?;
        if previous_path
            .as_deref()
            .is_some_and(|previous| previous >= path)
        {
            return Err(String::from(
                "fixture manifest paths are not strictly sorted",
            ));
        }
        previous_path = Some(path.to_owned());
        let bytes = fs::read(root.join(path)).map_err(|error| error.to_string())?;
        if bytes.len() != length || blake3::hash(&bytes).to_hex().as_str() != digest {
            return Err(format!("fixture bytes differ from manifest: {path}"));
        }
        entries.push(FixtureFile {
            path: path.to_owned(),
            length,
            blake3: digest.to_owned(),
        });
    }
    if entries.is_empty() {
        return Err(String::from("fixture manifest has no file entries"));
    }
    let mut actual_paths = Vec::new();
    collect_fixture_paths(&root, &root, &mut actual_paths)?;
    actual_paths.sort();
    let expected_paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    if actual_paths != expected_paths {
        return Err(String::from(
            "fixture files differ from the closed manifest inventory",
        ));
    }
    let _total_bytes = entries
        .iter()
        .try_fold(0_usize, |total, entry| total.checked_add(entry.length));
    if _total_bytes.is_none() {
        return Err(String::from("fixture total size overflows usize"));
    }
    let _unique_paths = entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    if _unique_paths.len() != entries.len() {
        return Err(String::from("fixture manifest repeats a path"));
    }
    let _all_hashes_well_formed = entries.iter().all(|entry| {
        entry.blake3.len() == 64 && entry.blake3.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if !_all_hashes_well_formed {
        return Err(String::from(
            "fixture manifest contains a malformed BLAKE3 hash",
        ));
    }
    Ok(entries)
}

fn validate_fixture_relative_path(path: &str) -> Result<(), String> {
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("fixture path is not a safe relative path: {path}"));
    }
    Ok(())
}

fn collect_fixture_paths(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> Result<(), String> {
    for item in fs::read_dir(current).map_err(|error| error.to_string())? {
        let item = item.map_err(|error| error.to_string())?;
        let path = item.path();
        let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
        if relative == Path::new("manifest.tsv") || relative == Path::new("README.md") {
            continue;
        }
        let kind = item.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "fixture inventory contains a symlink: {}",
                path.display()
            ));
        }
        if kind.is_dir() {
            collect_fixture_paths(root, &path, output)?;
        } else if kind.is_file() {
            output.push(relative.to_string_lossy().replace('\\', "/"));
        } else {
            return Err(format!(
                "fixture inventory contains a special file: {}",
                path.display()
            ));
        }
    }
    Ok(())
}
