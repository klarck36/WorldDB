use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditScopeFingerprint, AuthorizationMode, Capability, CapabilityGrant, CapabilityRule,
    ClientRequestId, DomainId, GrantEffect, OperationId, PageOrdinal, PolicyRuleId, PolicyScope,
    PolicySubject, Principal, PrincipalId, Revision, SecurityEpoch, SecurityPolicyHistory,
    SecurityPolicySnapshot, SecurityPolicyVersion, SnapshotId,
};
#[cfg(windows)]
use worlddb_core::{
    HistorySpaceDefinition, HistorySpaceId, JobBudget, MigrationCategory, MigrationId,
    MigrationPlan, MigrationPlanSpec, MigrationRunId, MigrationStepId, MigrationStepTargetSchema,
    MigrationTargetSchema, MigrationTransformerVersion, PredicateId, Record, SchemaDefinitionId,
    SchemaHistoryReferenceModel, SchemaIdentityTransition, SchemaMode, SchemaRevision, Source,
    SourceLocator, SourceMetadata, SourceSchemaPrecondition, Symbol, encode_record,
};
use worlddb_storage_file::{
    DatabaseLayout, ExactBackupManager, ManifestSnapshot, ManifestStore, RawReadAuditWal,
    WalPrepareLog, WriterLockError,
};

#[cfg(windows)]
use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence, Bytes,
    MigrationRunJournalState, PolicyTarget, SecurityPolicyChange, SecurityPolicyRecord,
};
#[cfg(windows)]
use worlddb_storage_file::{
    FileStoreGuardedMigrationRun, HistorySegmentStore, LogicalExport, ManifestSegmentKind,
    ManifestSegmentReference, RecoveryManager, SecurityPolicyHistoryStore, SharingExport,
    StorageVerifier,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "worlddb-m8-03-cli-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
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

fn run(arguments: &[&str]) -> Option<Output> {
    Command::new(env!("CARGO_BIN_EXE_worlddb-cli"))
        .args(arguments)
        .output()
        .ok()
}

fn snapshot_tree(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf();
                files.insert(relative, fs::read(path).map_err(|error| error.to_string())?);
            } else {
                return Err("fixture contains an unexpected non-file entry".to_owned());
            }
        }
    }
    Ok(files)
}

fn operation_id(tail: u8) -> Result<OperationId, String> {
    OperationId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        tail,
    ])
    .map_err(|error| error.to_string())
}

fn domain_id<T: DomainId>(tail: u8) -> Result<T, String> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).map_err(|error| error.to_string())
}

fn create_audit_complete_backup(source: &Path, target: &Path) -> Result<(), String> {
    let layout = DatabaseLayout::open(source).map_err(|error| error.to_string())?;
    let audit_writer = RawReadAuditWal::new(layout.clone())
        .try_writer()
        .map_err(|error| error.to_string())?;
    audit_writer
        .append_attempt(
            worlddb_core::RawReadAttemptScope {
                principal_id: domain_id::<PrincipalId>(1)?,
                scope_fingerprint: AuditScopeFingerprint::new(worlddb_core::Bytes::new(vec![
                    0x31, 0x01,
                ]))
                .map_err(|error| error.to_string())?,
                snapshot_id: domain_id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(1),
            },
            domain_id::<ClientRequestId>(3)?,
        )
        .map_err(|error| error.to_string())?;

    let principal_id = domain_id::<PrincipalId>(1)?;
    let rules = [
        Capability::ProjectRead,
        Capability::BackupCreate,
        Capability::AuditRead,
        Capability::AuditExport,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, capability)| -> Result<_, String> {
        let tail = u8::try_from(index + 1).map_err(|error| error.to_string())?;
        Ok(CapabilityRule::new(
            domain_id::<PolicyRuleId>(tail)?,
            PolicySubject::Principal(principal_id),
            CapabilityGrant::new(capability, GrantEffect::Allow),
            PolicyScope::project(),
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal_id)], vec![], vec![], rules)
            .map_err(|error| error.to_string())?;
    let policy_history = SecurityPolicyHistory::new(
        Revision::GENESIS,
        vec![SecurityPolicyVersion::new(
            Revision::GENESIS,
            SecurityEpoch::INITIAL,
            snapshot,
        )],
    )
    .map_err(|error| error.to_string())?;
    let policy = policy_history
        .select(AuthorizationMode::Now, principal_id, Revision::GENESIS)
        .map_err(|error| error.to_string())?;

    ExactBackupManager::new(layout)
        .create_audit_complete_backup(
            target,
            None,
            &audit_writer,
            policy,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn create_host_policy_project(
    root: &Path,
    capabilities: &[Capability],
) -> Result<PrincipalId, String> {
    let identity = worlddb_process_adapter::current_process_identity_bytes()
        .map_err(|error| format!("process identity unavailable: {error:?}"))?;
    let principal = worlddb_core::derive_host_account_principal(&identity)
        .map_err(|error| error.to_string())?;
    let layout = DatabaseLayout::create(root).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;

    let genesis = SecurityPolicyVersion::new(
        Revision::GENESIS,
        SecurityEpoch::INITIAL,
        SecurityPolicySnapshot::default(),
    );
    let genesis_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .stage_version(&lock, &genesis, None, None)
        .map_err(|error| error.to_string())?;

    let rules = capabilities
        .iter()
        .copied()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            let tail = u8::try_from(index + 1).map_err(|error| error.to_string())?;
            let rule_id = domain_id::<PolicyRuleId>(tail)?;
            Ok(CapabilityRule::new(
                rule_id,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let active_snapshot = SecurityPolicySnapshot::new(
        vec![Principal::new(principal)],
        vec![],
        vec![],
        rules.clone(),
    )
    .map_err(|error| error.to_string())?;
    let active_epoch = SecurityEpoch::INITIAL
        .next()
        .map_err(|error| error.to_string())?;
    let policy_record = SecurityPolicyRecord::new(
        domain_id::<worlddb_core::SecurityPolicyRecordId>(10)?,
        Revision::FIRST_COMMIT,
        principal,
        active_epoch,
        std::iter::once(SecurityPolicyChange::PrincipalRegistered {
            principal_id: principal,
        })
        .chain(
            rules
                .iter()
                .map(|rule| SecurityPolicyChange::CapabilityRuleAdded {
                    rule_id: rule.id(),
                    subject: rule.subject(),
                    capability: rule.grant().capability(),
                    effect: rule.grant().effect(),
                    scope: rule.scope(),
                }),
        )
        .collect(),
    )
    .map_err(|error| error.to_string())?;
    let active = SecurityPolicyVersion::new(
        Revision::FIRST_COMMIT,
        active_epoch,
        active_snapshot.clone(),
    );
    let active_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .stage_version(&lock, &active, Some(&policy_record), None)
        .map_err(|error| error.to_string())?;

    let genesis_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        genesis_receipt.id(),
        genesis_receipt.content_digest(),
        Revision::GENESIS,
    );
    let active_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        active_receipt.id(),
        active_receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    let operation = operation_id(11)?;
    let audit_record = AuditRecord::new(
        AuditRecordIdentity {
            record_id: domain_id::<worlddb_core::AuditRecordId>(12)?,
            sequence: AuditSequence::new(1),
            audit_operation_id: domain_id::<worlddb_core::AuditOperationId>(13)?,
        },
        AuditRecordDetails {
            actor: principal,
            action: AuditAction::SecurityPolicyChange,
            object_class: AuditObjectClass::SecurityPolicy,
            outcome: AuditOutcome::Succeeded,
            commit_context: AuditCommitContext::Committed {
                revision: Revision::FIRST_COMMIT,
                operation_id: operation,
            },
            security_epoch: SecurityEpoch::INITIAL,
            policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                SecurityPolicySnapshot::default()
                    .effective_capability_fingerprint(principal, PolicyTarget::default())
                    .to_vec(),
            ))
            .map_err(|error| error.to_string())?,
        },
    );
    WalPrepareLog::new(&layout)
        .commit_audited_manifest_snapshot(
            &lock,
            operation,
            vec![genesis_reference, active_reference],
            &[genesis_reference, active_reference],
            &audit_record,
        )
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    let report = StorageVerifier::new(layout)
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    if !report.is_clean() || report.safe_revision() != Revision::FIRST_COMMIT {
        return Err("host-policy project fixture did not verify cleanly".to_owned());
    }
    drop(lock);
    Ok(principal)
}

#[cfg(windows)]
fn install_cli_export_history(root: &Path) -> Result<HistorySpaceId, String> {
    let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("export fixture is missing CURRENT"))?;
    let policy_segment_ids = current
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|reference| reference.id())
        .collect::<Vec<_>>();
    let policy_history = SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(current.revision(), &policy_segment_ids)
        .map_err(|error| error.to_string())?;
    let current_policy_version = policy_history
        .policy()
        .versions()
        .last()
        .cloned()
        .ok_or_else(|| String::from("export fixture is missing a current policy version"))?;
    let current_audit_retention = policy_history
        .audit_retention_at(current.revision())
        .map_err(|error| error.to_string())?;
    let space = domain_id::<HistorySpaceId>(90)?;
    let source_id = domain_id::<worlddb_core::SourceId>(91)?;
    let revision = Revision::new(2).map_err(|error| error.to_string())?;
    let records = [
        Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(space, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ),
        Record::Source(Source::new(
            source_id,
            Symbol::new("fixture").map_err(|error| error.to_string())?,
            Some(SourceLocator::new("hidden-locator-value").map_err(|error| error.to_string())?),
            None,
            SourceMetadata::default(),
            revision,
        )),
    ];
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_segment(&lock, &records)
        .map_err(|error| error.to_string())?;
    let continued_policy = SecurityPolicyVersion::new(
        revision,
        current_policy_version.epoch(),
        current_policy_version.snapshot().clone(),
    );
    let continued_policy_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(&lock, &continued_policy, None, current_audit_retention)
        .map_err(|error| error.to_string())?;
    let mut references = current.segments().to_vec();
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        revision,
    ));
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        continued_policy_receipt.id(),
        continued_policy_receipt.content_digest(),
        revision,
    ));
    ManifestSnapshot::new(revision, references.clone()).map_err(|error| error.to_string())?;
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(92)?, references, &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    let report = StorageVerifier::new(layout)
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    if !report.is_clean() || report.safe_revision() != revision {
        return Err("export fixture did not verify cleanly".to_owned());
    }
    drop(lock);
    Ok(space)
}

#[cfg(windows)]
fn install_cli_purge_history(root: &Path) -> Result<(HistorySpaceId, HistorySpaceId), String> {
    let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("purge fixture is missing CURRENT"))?;
    let policy_segment_ids = current
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|reference| reference.id())
        .collect::<Vec<_>>();
    let policy_history = SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(current.revision(), &policy_segment_ids)
        .map_err(|error| error.to_string())?;
    let current_policy_version = policy_history
        .policy()
        .versions()
        .last()
        .cloned()
        .ok_or_else(|| String::from("purge fixture has no current policy version"))?;
    let current_audit_retention = policy_history
        .audit_retention_at(current.revision())
        .map_err(|error| error.to_string())?;
    let root_space = domain_id::<HistorySpaceId>(90)?;
    let target_space = domain_id::<HistorySpaceId>(91)?;
    let dependant_space = domain_id::<HistorySpaceId>(92)?;
    let revision = Revision::new(2).map_err(|error| error.to_string())?;
    let records = [
        Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(root_space, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ),
        Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(target_space, Some(root_space), Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ),
        Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(dependant_space, Some(target_space), Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ),
    ];
    let history_receipt = HistorySegmentStore::new(layout.clone())
        .write_segment(&lock, &records)
        .map_err(|error| error.to_string())?;
    let continued_policy = SecurityPolicyVersion::new(
        revision,
        current_policy_version.epoch(),
        current_policy_version.snapshot().clone(),
    );
    let policy_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(&lock, &continued_policy, None, current_audit_retention)
        .map_err(|error| error.to_string())?;
    let mut references = current.segments().to_vec();
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        history_receipt.id(),
        history_receipt.content_digest(),
        revision,
    ));
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        policy_receipt.id(),
        policy_receipt.content_digest(),
        revision,
    ));
    ManifestSnapshot::new(revision, references.clone()).map_err(|error| error.to_string())?;
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(93)?, references, &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    let report = StorageVerifier::new(layout)
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    if !report.is_clean() || report.safe_revision() != revision {
        return Err("purge fixture did not verify cleanly".to_owned());
    }
    drop(lock);
    Ok((target_space, dependant_space))
}

#[cfg(windows)]
fn breaking_migration_plan_frame() -> Result<(Vec<u8>, MigrationStepId), String> {
    let step_id = domain_id::<MigrationStepId>(41)?;
    let source_predicate = domain_id::<PredicateId>(42)?;
    let target_predicate = domain_id::<PredicateId>(43)?;
    let source_fingerprint = SchemaHistoryReferenceModel::new()
        .schema_at(SchemaMode::Current, Revision::FIRST_COMMIT)
        .map_err(|error| error.to_string())?
        .fingerprint();
    let target_revision = Revision::new(2).map_err(|error| error.to_string())?;
    let target_schema = MigrationTargetSchema::new(
        SchemaRevision::from_published_revision(target_revision),
        source_fingerprint,
    );
    let transition = SchemaIdentityTransition::new(
        Some(SchemaDefinitionId::Predicate(source_predicate)),
        Some(SchemaDefinitionId::Predicate(target_predicate)),
        MigrationCategory::Breaking,
    )
    .map_err(|error| error.to_string())?;
    let plan = MigrationPlan::new(MigrationPlanSpec {
        migration_id: domain_id::<MigrationId>(40)?,
        category: MigrationCategory::Breaking,
        source_schema: SourceSchemaPrecondition::new(
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            source_fingerprint,
        ),
        target_schema,
        steps: vec![step_id],
        step_targets: Some(vec![MigrationStepTargetSchema::new(step_id, target_schema)]),
        schema_changes: vec![transition],
        transformer_version: MigrationTransformerVersion::new(1)
            .map_err(|error| error.to_string())?,
        calendar_shift: None,
        budget: JobBudget::new(10, 1024 * 1024).map_err(|error| error.to_string())?,
    })
    .map_err(|error| error.to_string())?;
    let frame = encode_record(&Record::MigrationPlan(plan)).map_err(|error| error.to_string())?;
    Ok((frame, step_id))
}

#[cfg(windows)]
#[test]
fn cli_export_import_roundtrip_applies_scope_manifest_and_sharing_rights() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("export-source");
    let _principal = create_host_policy_project(
        &source,
        &[
            Capability::DataExport,
            Capability::ProjectRead,
            Capability::HistorySpaceRead,
        ],
    )?;
    let space = install_cli_export_history(&source)?;
    let destination = area.path("import-destination");
    let _destination_principal = create_host_policy_project(
        &destination,
        &[Capability::ProjectRead, Capability::DataImport],
    )?;

    let source_arg = source.to_string_lossy().into_owned();
    let destination_arg = destination.to_string_lossy().into_owned();
    let logical_path = area.path("logical-export.bin");
    let logical_arg = logical_path.to_string_lossy().into_owned();
    let space_arg = space.to_string();
    let logical = run(&[
        "--format=jsonl",
        "v1",
        "export",
        "logical",
        &source_arg,
        "--output",
        &logical_arg,
        "--from",
        "0",
        "--through",
        "2",
        "--history-space",
        &space_arg,
        "--class",
        "HistorySpaceDefinition",
        "--class",
        "Source",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(
        logical.status.success(),
        "logical export failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&logical.stdout),
        String::from_utf8_lossy(&logical.stderr)
    );
    let logical_text = String::from_utf8_lossy(&logical.stdout);
    assert!(logical_text.contains("\"type\":\"logical_export\""));
    assert!(logical_text.contains("\"omission_manifest\":{\"complete\":true"));
    assert!(logical_text.contains(&format!("\"history_spaces\":[\"{space_arg}\"]")));
    assert!(logical_text.contains("\"record_classes\":[\"HistorySpaceDefinition\",\"Source\"]"));
    assert!(!logical_text.contains(&source_arg));
    assert!(!logical_text.contains(&logical_arg));
    let logical_bytes = fs::read(&logical_path).map_err(|error| error.to_string())?;
    let decoded_logical =
        LogicalExport::decode(&logical_bytes).map_err(|error| error.to_string())?;
    assert_eq!(decoded_logical.records().len(), 2);
    assert_eq!(
        decoded_logical.manifest().selected_history_spaces(),
        &[space]
    );
    assert_eq!(decoded_logical.manifest().selected_record_kinds().len(), 2);
    assert_eq!(
        decoded_logical.manifest().omitted_storage_classes().len(),
        5
    );

    let sharing_path = area.path("sharing-export.bin");
    let sharing_arg = sharing_path.to_string_lossy().into_owned();
    let sharing = run(&[
        "--format=jsonl",
        "v1",
        "export",
        "share",
        &source_arg,
        "--output",
        &sharing_arg,
        "--from",
        "0",
        "--through",
        "2",
        "--history-space",
        &space_arg,
        "--class",
        "HistorySpaceDefinition",
        "--class",
        "Source",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(sharing.status.success());
    let sharing_text = String::from_utf8_lossy(&sharing.stdout);
    assert!(sharing_text.contains("\"type\":\"sharing_export\""));
    assert!(sharing_text.contains("\"omission_counts_disclosed\":false"));
    assert!(sharing_text.contains(&format!("\"history_spaces\":[\"{space_arg}\"]")));
    assert!(sharing_text.contains("\"record_classes\":[\"HistorySpaceDefinition\",\"Source\"]"));
    assert!(!sharing_text.contains(&source_arg));
    assert!(!sharing_text.contains(&sharing_arg));
    let sharing_bytes = fs::read(&sharing_path).map_err(|error| error.to_string())?;
    assert!(
        !sharing_bytes
            .windows(b"hidden-locator-value".len())
            .any(|window| window == b"hidden-locator-value")
    );
    let decoded_sharing =
        SharingExport::decode(&sharing_bytes).map_err(|error| error.to_string())?;
    assert!(decoded_sharing.records().is_empty());

    let target_space = domain_id::<HistorySpaceId>(93)?.to_string();
    let mapping = format!("history-space:{space}=history-space:{target_space}");
    let plan_path = area.path("logical-import-plan.bin");
    let plan_arg = plan_path.to_string_lossy().into_owned();
    let plan = run(&[
        "--format=jsonl",
        "v1",
        "import",
        "plan",
        &destination_arg,
        "--input",
        &logical_arg,
        "--output",
        &plan_arg,
        "--map",
        &mapping,
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(plan.status.success());
    let plan_text = String::from_utf8_lossy(&plan.stdout);
    assert!(plan_text.contains("\"type\":\"import_plan\""));
    assert!(plan_text.contains("\"mapping_count\":\"1\""));
    assert!(!plan_text.contains(&destination_arg));
    assert!(!plan_text.contains(&plan_arg));

    let prepared = run(&[
        "--format=jsonl",
        "v1",
        "import",
        "prepare",
        &destination_arg,
        "--input",
        &logical_arg,
        "--plan-file",
        &plan_arg,
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(prepared.status.success());
    let prepared_text = String::from_utf8_lossy(&prepared.stdout);
    assert!(prepared_text.contains("\"type\":\"import_prepare\""));
    assert!(prepared_text.contains("\"mapping_count\":\"1\""));
    assert!(prepared_text.contains("\"writes_database\":false"));
    assert!(prepared_text.contains("\"record_count\":\"2\""));
    assert!(!prepared_text.contains(&destination_arg));
    assert!(!prepared_text.contains(&plan_arg));
    Ok(())
}

#[cfg(windows)]
#[test]
fn breaking_migration_without_explicit_confirmation_does_not_create_restore_artifacts()
-> Result<(), String> {
    let area = TempArea::create()?;
    let database_path = area.path("migration-source");
    let _principal = create_host_policy_project(&database_path, &[Capability::MigrationExecute])?;
    let (plan_frame, step_id) = breaking_migration_plan_frame()?;
    let plan_path = area.path("breaking-plan.bin");
    fs::write(&plan_path, plan_frame).map_err(|error| error.to_string())?;
    let run_id = domain_id::<MigrationRunId>(44)?.to_string();
    let operation_id = operation_id(45)?.to_string();
    let backup_path = area.path("migration-backup");
    let restore_path = area.path("migration-restore");
    let database_text = database_path
        .to_str()
        .ok_or_else(|| String::from("database path is not valid UTF-8"))?;
    let plan_text = plan_path
        .to_str()
        .ok_or_else(|| String::from("plan path is not valid UTF-8"))?;
    let run_text = run_id.as_str();
    let step_text = step_id.to_string();
    let operation_text = operation_id.as_str();
    let backup_text = backup_path
        .to_str()
        .ok_or_else(|| String::from("backup path is not valid UTF-8"))?;
    let restore_text = restore_path
        .to_str()
        .ok_or_else(|| String::from("restore path is not valid UTF-8"))?;
    let arguments = [
        "--format=jsonl",
        "v1",
        "migration",
        "run",
        database_text,
        "--plan-file",
        plan_text,
        "--run-id",
        run_text,
        "--step",
        &step_text,
        "--operation-id",
        operation_text,
        "--backup",
        backup_text,
        "--restore",
        restore_text,
    ];
    let output = run(&arguments).ok_or_else(|| String::from("CLI process failed to start"))?;
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"InvalidRequest\""));
    assert!(!stdout.contains(backup_text));
    assert!(!stdout.contains(restore_text));
    assert!(!backup_path.exists());
    assert!(!restore_path.exists());

    let layout = DatabaseLayout::open(&database_path).map_err(|error| error.to_string())?;
    let manifest = ManifestStore::new(layout)
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("migration source manifest disappeared"))?;
    assert_eq!(manifest.revision(), Revision::FIRST_COMMIT);
    Ok(())
}

#[cfg(windows)]
#[test]
fn migration_plan_dry_run_breaking_run_and_resume_guards_are_end_to_end() -> Result<(), String> {
    let area = TempArea::create()?;
    let database_path = area.path("migration-source");
    let _principal = create_host_policy_project(
        &database_path,
        &[
            Capability::ProjectRead,
            Capability::MigrationPlan,
            Capability::MigrationExecute,
            Capability::BackupCreate,
            Capability::BackupRestore,
        ],
    )?;
    let (plan_frame, step_id) = breaking_migration_plan_frame()?;
    let plan_path = area.path("breaking-plan.bin");
    fs::write(&plan_path, plan_frame).map_err(|error| error.to_string())?;
    let run_id = domain_id::<MigrationRunId>(46)?.to_string();
    let operation_id = operation_id(47)?.to_string();
    let backup_path = area.path("migration-backup");
    let restore_path = area.path("migration-restore");
    let resume_restore_path = area.path("migration-resume-restore");
    let database_text = database_path
        .to_str()
        .ok_or_else(|| String::from("database path is not valid UTF-8"))?;
    let plan_text = plan_path
        .to_str()
        .ok_or_else(|| String::from("plan path is not valid UTF-8"))?;
    let run_text = run_id.as_str();
    let step_text = step_id.to_string();
    let operation_text = operation_id.as_str();
    let backup_text = backup_path
        .to_str()
        .ok_or_else(|| String::from("backup path is not valid UTF-8"))?;
    let restore_text = restore_path
        .to_str()
        .ok_or_else(|| String::from("restore path is not valid UTF-8"))?;
    let resume_restore_text = resume_restore_path
        .to_str()
        .ok_or_else(|| String::from("resume restore path is not valid UTF-8"))?;

    let original_tree = snapshot_tree(&database_path)?;
    let plan_output = run(&[
        "--format=jsonl",
        "v1",
        "migration",
        "plan",
        database_text,
        "--plan-file",
        plan_text,
    ])
    .ok_or_else(|| String::from("CLI process failed to start"))?;
    assert!(plan_output.status.success());
    assert!(String::from_utf8_lossy(&plan_output.stdout).contains("\"status\":\"planned\""));
    assert!(!String::from_utf8_lossy(&plan_output.stdout).contains(database_text));
    assert_eq!(snapshot_tree(&database_path)?, original_tree);

    let dry_run_output = run(&[
        "--format=jsonl",
        "v1",
        "migration",
        "dry-run",
        database_text,
        "--plan-file",
        plan_text,
        "--step",
        &step_text,
    ])
    .ok_or_else(|| String::from("CLI process failed to start"))?;
    assert!(dry_run_output.status.success());
    let dry_run_text = String::from_utf8_lossy(&dry_run_output.stdout);
    assert!(dry_run_text.contains("\"status\":\"previewed\""));
    assert!(!dry_run_text.contains(database_text));
    assert_eq!(snapshot_tree(&database_path)?, original_tree);

    let missing_resume_output = run(&[
        "--format=jsonl",
        "v1",
        "migration",
        "resume",
        database_text,
        "--plan-file",
        plan_text,
        "--run-id",
        run_text,
        "--step",
        &step_text,
        "--operation-id",
        operation_text,
        "--backup",
        backup_text,
        "--restore",
        resume_restore_text,
        "--confirm-breaking",
    ])
    .ok_or_else(|| String::from("CLI process failed to start"))?;
    assert_eq!(missing_resume_output.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&missing_resume_output.stdout).contains("\"code\":\"NotFound\"")
    );
    assert!(!backup_path.exists());
    assert!(!resume_restore_path.exists());

    let run_output = run(&[
        "--format=jsonl",
        "v1",
        "migration",
        "run",
        database_text,
        "--plan-file",
        plan_text,
        "--run-id",
        run_text,
        "--step",
        &step_text,
        "--operation-id",
        operation_text,
        "--backup",
        backup_text,
        "--restore",
        restore_text,
        "--confirm-breaking",
    ])
    .ok_or_else(|| String::from("CLI process failed to start"))?;
    assert!(
        run_output.status.success(),
        "{}",
        String::from_utf8_lossy(&run_output.stdout)
    );
    let run_text_out = String::from_utf8_lossy(&run_output.stdout);
    assert!(run_text_out.contains("\"status\":\"completed\""));
    assert!(run_text_out.contains("\"final_revision\":\"2\""));
    assert!(!run_text_out.contains(database_text));
    assert!(!run_text_out.contains(backup_text));
    assert!(!run_text_out.contains(restore_text));
    assert!(backup_path.is_dir());
    assert!(restore_path.is_dir());

    let layout = DatabaseLayout::open(&database_path).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let migration_run = FileStoreGuardedMigrationRun::open(layout, &lock)
        .map_err(|error| format!("could not open migration run: {error}"))?;
    let snapshot = migration_run
        .load_journal_status(domain_id::<MigrationRunId>(46)?)
        .map_err(|error| format!("could not read migration run journal: {error}"))?
        .ok_or_else(|| String::from("migration run journal disappeared"))?;
    assert_eq!(snapshot.state(), MigrationRunJournalState::Completed);
    drop(lock);

    let resume_output = run(&[
        "--format=jsonl",
        "v1",
        "migration",
        "resume",
        database_text,
        "--plan-file",
        plan_text,
        "--run-id",
        run_text,
        "--step",
        &step_text,
        "--operation-id",
        operation_text,
        "--backup",
        backup_text,
        "--restore",
        resume_restore_text,
        "--confirm-breaking",
    ])
    .ok_or_else(|| String::from("CLI process failed to start"))?;
    assert_eq!(
        resume_output.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&resume_output.stdout),
        String::from_utf8_lossy(&resume_output.stderr)
    );
    let resume_text_out = String::from_utf8_lossy(&resume_output.stdout);
    assert!(resume_text_out.contains("\"code\":\"InvalidRequest\""));
    assert!(!resume_text_out.contains(database_text));
    assert!(!resume_text_out.contains(backup_text));
    assert!(!resume_text_out.contains(resume_restore_text));
    assert!(!resume_restore_path.exists());

    let layout = DatabaseLayout::open(&database_path).map_err(|error| error.to_string())?;
    let manifest = ManifestStore::new(layout)
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("migration source manifest disappeared"))?;
    assert_eq!(
        manifest.revision(),
        Revision::new(2).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(windows)]
fn append_host_raw_read_attempt(source: &Path, principal: PrincipalId) -> Result<(), String> {
    let layout = DatabaseLayout::open(source).map_err(|error| error.to_string())?;
    let writer = RawReadAuditWal::new(layout)
        .try_writer()
        .map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            worlddb_core::RawReadAttemptScope {
                principal_id: principal,
                scope_fingerprint: AuditScopeFingerprint::new(Bytes::new(vec![0x41, 0x01]))
                    .map_err(|error| error.to_string())?,
                snapshot_id: domain_id::<SnapshotId>(14)?,
                security_epoch: SecurityEpoch::new(1),
                page_ordinal: PageOrdinal::new(1),
            },
            domain_id::<ClientRequestId>(15)?,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn machine_help_and_version_are_single_versioned_json_lines() {
    let help = run(&["--format=jsonl", "v1", "--help"]);
    assert!(help.is_some());
    let Some(help) = help else {
        return;
    };
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.ends_with('\n'));
    assert_eq!(help_text.lines().count(), 1);
    assert!(help_text.contains("\"cli_protocol\":{\"major\":1,\"minor\":0}"));
    assert!(help_text.contains("\"type\":\"help\""));
    assert!(help_text.contains("\"scope\":\"root\""));
    assert!(help_text.contains("\"request_id\":\""));

    let version = run(&["--format", "jsonl", "v1", "version"]);
    assert!(version.is_some());
    let Some(version) = version else {
        return;
    };
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    let version_text = String::from_utf8_lossy(&version.stdout);
    assert_eq!(version_text.lines().count(), 1);
    assert!(version_text.contains("\"type\":\"version\""));
    assert!(version_text.contains("\"protocol\":{\"major\":1,\"minor\":0}"));
}

#[test]
fn unsupported_commands_use_public_code_and_do_not_echo_arguments() {
    let secret = "WDB_SECRET_ARGUMENT_CANARY";
    let output = run(&["--format=jsonl", "v1", "not-a-command", secret]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"type\":\"error\""));
    assert!(stdout.contains("\"code\":\"UnsupportedOperation\""));
    assert!(!stdout.contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[test]
fn adapter_io_errors_use_public_code_without_exposing_paths_or_causes() {
    let secret = "WDB_SECRET_PATH_CANARY";
    let output = run(&[
        "--format=jsonl",
        "v1",
        "adapter",
        "run",
        "--manifest",
        secret,
        "--input",
        "WDB_SECRET_INPUT_CANARY",
        "--output",
        "WDB_SECRET_OUTPUT_CANARY",
        "--",
        "WDB_SECRET_ADAPTER_CANARY",
    ]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(8));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"code\":\"StorageRead\""));
    for canary in [
        secret,
        "WDB_SECRET_INPUT_CANARY",
        "WDB_SECRET_OUTPUT_CANARY",
        "WDB_SECRET_ADAPTER_CANARY",
    ] {
        assert!(!stdout.contains(canary));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(canary));
    }
}

#[test]
fn human_errors_are_stderr_only_and_contain_no_user_arguments() {
    let secret = "WDB_SECRET_ARGUMENT_CANARY";
    let output = run(&["v1", "not-a-command", secret]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[UnsupportedOperation]"));
    assert!(!stderr.contains(secret));
}

#[test]
fn unsupported_cli_versions_have_a_distinct_public_code() {
    let output = run(&["--format=jsonl", "v2", "help"]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"UnsupportedProtocolVersion\""));
}

#[test]
fn read_only_open_verify_recovery_inspect_and_salvage_preserve_the_source() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("source");
    let layout = DatabaseLayout::create(&source).map_err(|error| error.to_string())?;

    // Read-only access must fail closed before any normal writer has established LOCK.
    let initial_tree = snapshot_tree(&source)?;
    let source_arg = source.to_string_lossy().into_owned();
    let missing_lock = run(&["--format=jsonl", "v1", "verify", &source_arg]);
    assert!(missing_lock.is_some());
    let Some(missing_lock) = missing_lock else {
        return Err("CLI process could not be started".to_owned());
    };
    assert_eq!(missing_lock.status.code(), Some(8));
    assert!(String::from_utf8_lossy(&missing_lock.stdout).contains("StorageRead"));
    assert!(!layout.writer_lock_file().exists());
    assert_eq!(snapshot_tree(&source)?, initial_tree);

    // Establish the stable lock file once, then prove a shared reader blocks writers.
    drop(
        layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?,
    );
    let read_lock = layout
        .try_read_only_lock()
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        layout.try_writer_lock(),
        Err(WriterLockError::AlreadyHeld)
    ));
    drop(read_lock);

    let before = snapshot_tree(&source)?;
    for (arguments, outcome_type) in [
        (
            vec!["--format=jsonl", "v1", "verify", &source_arg],
            "verify",
        ),
        (
            vec!["--format=jsonl", "v1", "recovery", "inspect", &source_arg],
            "recovery_inspect",
        ),
        (
            vec!["--format=jsonl", "v1", "open", "--read-only", &source_arg],
            "open_read_only",
        ),
    ] {
        let output = run(&arguments);
        assert!(output.is_some());
        let Some(output) = output else {
            return Err("CLI process could not be started".to_owned());
        };
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8_lossy(&output.stdout);
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains(&format!("\"type\":\"{outcome_type}\"")));
        assert!(text.contains("\"safe_revision\":\"0\""));
        assert!(text.contains("\"source_modified\":false"));
        assert!(!text.contains(&source_arg));
    }

    let target = area.path("salvage");
    let target_arg = target.to_string_lossy().into_owned();
    let output = run(&[
        "--format=jsonl",
        "v1",
        "salvage",
        &source_arg,
        "--output",
        &target_arg,
    ]);
    assert!(output.is_some());
    let Some(output) = output else {
        return Err("CLI process could not be started".to_owned());
    };
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("\"type\":\"salvage\""));
    assert!(text.contains("\"archive_created\":true"));
    assert!(text.contains("\"source_modified\":false"));
    assert!(!text.contains(&source_arg));
    assert!(!text.contains(&target_arg));
    assert!(target.join("SALVAGE").is_file());
    assert_eq!(snapshot_tree(&source)?, before);
    Ok(())
}

#[test]
fn verify_reports_truncation_and_original_protection_without_repairing() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("damaged-source");
    let layout = DatabaseLayout::create(&source).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let committed = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    let pending = wal
        .append_prepare(&lock, operation_id(2)?, b"uncommitted")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(committed.revision(), vec![])
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let tail = layout
        .wal_directory()
        .join(format!("segment-{:020}.wal", pending.segment_sequence()));
    OpenOptions::new()
        .append(true)
        .open(tail)
        .map_err(|error| error.to_string())?
        .write_all(&[0x57, 0x44, 0x42, 0x43, 0x01])
        .map_err(|error| error.to_string())?;
    drop(lock);

    let before = snapshot_tree(&source)?;
    let source_arg = source.to_string_lossy().into_owned();
    let output = run(&["--format=jsonl", "v1", "verify", &source_arg]);
    assert!(output.is_some());
    let Some(output) = output else {
        return Err("CLI process could not be started".to_owned());
    };
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("\"safe_revision\":\"1\""));
    assert!(text.contains("\"Truncation\":\""));
    assert!(!text.contains("\"Truncation\":\"0\""));
    assert!(text.contains("\"PreserveOriginal\""));
    assert!(text.contains("\"RunJournaledTailRecovery\""));
    assert!(text.contains("\"source_modified\":false"));
    assert_eq!(snapshot_tree(&source)?, before);
    Ok(())
}

#[test]
fn recovery_apply_is_explicit_and_reports_its_effect() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("recovery-source");
    let layout = DatabaseLayout::create(&source).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let committed = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    let pending = wal
        .append_prepare(&lock, operation_id(2)?, b"uncommitted")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(committed.revision(), vec![])
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let tail = layout
        .wal_directory()
        .join(format!("segment-{:020}.wal", pending.segment_sequence()));
    OpenOptions::new()
        .append(true)
        .open(tail)
        .map_err(|error| error.to_string())?
        .write_all(&[0x57, 0x44, 0x42, 0x43, 0x01])
        .map_err(|error| error.to_string())?;
    drop(lock);
    let source_arg = source.to_string_lossy().into_owned();

    let implicit = run(&["--format=jsonl", "v1", "recovery", "run", &source_arg]);
    assert!(implicit.is_some());
    let Some(implicit) = implicit else {
        return Err("CLI process could not be started".to_owned());
    };
    assert_eq!(implicit.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&implicit.stdout).contains("InvalidRequest"));

    let applied = run(&[
        "--format=jsonl",
        "v1",
        "recovery",
        "run",
        "--apply",
        &source_arg,
    ]);
    assert!(applied.is_some());
    let Some(applied) = applied else {
        return Err("CLI process could not be started".to_owned());
    };
    assert!(applied.status.success());
    let text = String::from_utf8_lossy(&applied.stdout);
    assert!(text.contains("\"type\":\"recovery\""));
    assert!(text.contains("\"status\":\"applied\""));
    assert!(text.contains("\"safe_revision\":\"1\""));
    assert!(text.contains("\"disposition\":\"Clean\""));
    assert!(text.contains("\"quarantined_tails\":\"1\""));
    assert!(text.contains("\"source_modified\":true"));

    let verified = run(&["--format=jsonl", "v1", "verify", &source_arg]);
    assert!(verified.is_some());
    let Some(verified) = verified else {
        return Err("CLI process could not be started".to_owned());
    };
    assert!(verified.status.success());
    let verified_text = String::from_utf8_lossy(&verified.stdout);
    assert!(verified_text.contains("\"disposition\":\"Clean\""));
    assert!(verified_text.contains("\"Truncation\":\"0\""));
    Ok(())
}

#[test]
fn backup_verify_reports_profile_scope_and_target_verification() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("backup-source");
    let layout = DatabaseLayout::create(&source).map_err(|error| error.to_string())?;
    drop(
        layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?,
    );
    let backup = area.path("exact-backup");
    ExactBackupManager::new(layout)
        .create_exact_backup(&backup, None)
        .map_err(|error| error.to_string())?;
    let source_arg = source.to_string_lossy().into_owned();
    let backup_arg = backup.to_string_lossy().into_owned();

    let verified = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "verify",
        &backup_arg,
        "--profile",
        "exact",
        "--audit-scope",
        "excluded",
    ]);
    assert!(verified.is_some());
    let Some(verified) = verified else {
        return Err("CLI process could not be started".to_owned());
    };
    assert!(verified.status.success());
    assert!(verified.stderr.is_empty());
    let verified_text = String::from_utf8_lossy(&verified.stdout);
    assert!(verified_text.contains("\"status\":\"verified\""));
    assert!(verified_text.contains("\"profile\":\"ExactDatabase\""));
    assert!(verified_text.contains("\"audit_scope\":\"Excluded\""));
    assert!(verified_text.contains("\"target_verified\":true"));
    assert!(verified_text.contains("\"source_modified\":false"));
    assert!(verified_text.contains("\"audit_safe_sequence\":null"));
    assert!(!verified_text.contains(&backup_arg));
    assert!(!verified_text.contains(&source_arg));

    let audit_source = area.path("audit-backup-source");
    DatabaseLayout::create(&audit_source).map_err(|error| error.to_string())?;
    let audit_backup = area.path("audit-complete-backup");
    create_audit_complete_backup(&audit_source, &audit_backup)?;
    let audit_source_arg = audit_source.to_string_lossy().into_owned();
    let audit_backup_arg = audit_backup.to_string_lossy().into_owned();
    let audit_verified = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "verify",
        &audit_backup_arg,
        "--profile",
        "audit-complete",
        "--audit-scope",
        "included",
    ]);
    assert!(audit_verified.is_some());
    let Some(audit_verified) = audit_verified else {
        return Err("CLI process could not be started".to_owned());
    };
    assert!(audit_verified.status.success());
    assert!(audit_verified.stderr.is_empty());
    let audit_text = String::from_utf8_lossy(&audit_verified.stdout);
    assert!(audit_text.contains("\"status\":\"verified\""));
    assert!(audit_text.contains("\"profile\":\"AuditComplete\""));
    assert!(audit_text.contains("\"audit_scope\":\"Included\""));
    assert!(audit_text.contains("\"audit_safe_sequence\":\"1\""));
    assert!(audit_text.contains("\"target_verified\":true"));
    assert!(audit_text.contains("\"source_modified\":false"));
    assert!(!audit_text.contains(&audit_source_arg));
    assert!(!audit_text.contains(&audit_backup_arg));
    Ok(())
}

#[test]
fn backup_creation_requires_trusted_policy_and_scope_mismatch_is_rejected() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("backup-source");
    #[cfg(windows)]
    create_host_policy_project(&source, &[])?;
    #[cfg(not(windows))]
    let layout = DatabaseLayout::create(&source).map_err(|error| error.to_string())?;
    #[cfg(not(windows))]
    drop(
        layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?,
    );
    let source_before = snapshot_tree(&source)?;
    let source_arg = source.to_string_lossy().into_owned();
    let exact_target = area.path("exact-backup");
    let exact_target_arg = exact_target.to_string_lossy().into_owned();
    let exact_unauthorized = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "create",
        &source_arg,
        "--output",
        &exact_target_arg,
        "--profile",
        "exact",
        "--audit-scope",
        "excluded",
    ]);
    assert!(exact_unauthorized.is_some());
    let Some(exact_unauthorized) = exact_unauthorized else {
        return Err("CLI process could not be started".to_owned());
    };
    assert_eq!(exact_unauthorized.status.code(), Some(4));
    assert!(exact_unauthorized.stderr.is_empty());
    assert!(
        String::from_utf8_lossy(&exact_unauthorized.stdout).contains("\"code\":\"Unauthorized\"")
    );
    assert!(!exact_target.exists());
    assert_eq!(snapshot_tree(&source)?, source_before);

    let audit_target = area.path("audit-backup");
    let audit_target_arg = audit_target.to_string_lossy().into_owned();

    let unauthorized = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "create",
        &source_arg,
        "--output",
        &audit_target_arg,
        "--profile",
        "audit-complete",
        "--audit-scope",
        "included",
    ]);
    assert!(unauthorized.is_some());
    let Some(unauthorized) = unauthorized else {
        return Err("CLI process could not be started".to_owned());
    };
    assert_eq!(unauthorized.status.code(), Some(4));
    assert!(unauthorized.stderr.is_empty());
    assert!(String::from_utf8_lossy(&unauthorized.stdout).contains("\"code\":\"Unauthorized\""));
    assert!(!audit_target.exists());
    assert_eq!(snapshot_tree(&source)?, source_before);

    let mismatched_scope = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "create",
        &source_arg,
        "--output",
        &audit_target_arg,
        "--profile",
        "audit-complete",
        "--audit-scope",
        "excluded",
    ]);
    assert!(mismatched_scope.is_some());
    let Some(mismatched_scope) = mismatched_scope else {
        return Err("CLI process could not be started".to_owned());
    };
    assert_eq!(mismatched_scope.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&mismatched_scope.stdout).contains("\"code\":\"InvalidRequest\"")
    );
    assert!(!audit_target.exists());
    assert_eq!(snapshot_tree(&source)?, source_before);
    Ok(())
}

#[test]
fn restore_clone_requires_trusted_policy_before_reading_backup_or_touching_target()
-> Result<(), String> {
    let area = TempArea::create()?;
    #[cfg(windows)]
    let authorization_project = area.path("authorization-project");
    #[cfg(windows)]
    create_host_policy_project(&authorization_project, &[])?;
    #[cfg(not(windows))]
    let authorization_project = area.path("nonexistent-authorization-project");
    let backup = area.path("backup-path-canary");
    let destination = area.path("restore-target");
    let authorization_arg = authorization_project.to_string_lossy().into_owned();
    let backup_arg = backup.to_string_lossy().into_owned();
    let destination_arg = destination.to_string_lossy().into_owned();
    let output = run(&[
        "--format=jsonl",
        "v1",
        "restore",
        "clone",
        &backup_arg,
        "--authorize-with",
        &authorization_arg,
        "--output",
        &destination_arg,
        "--profile",
        "exact",
        "--audit-scope",
        "excluded",
    ]);
    assert!(output.is_some());
    let Some(output) = output else {
        return Err("CLI process could not be started".to_owned());
    };
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("\"code\":\"Unauthorized\""));
    assert!(!text.contains(&backup_arg));
    assert!(!text.contains(&destination_arg));
    assert!(!destination.exists());
    Ok(())
}

#[cfg(windows)]
#[test]
fn authorized_cli_backup_and_restore_clone_cover_both_profiles() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("authorized-project");
    let principal = create_host_policy_project(
        &source,
        &[
            Capability::ProjectRead,
            Capability::BackupCreate,
            Capability::BackupRestore,
            Capability::AuditRead,
            Capability::AuditExport,
        ],
    )?;
    let source_arg = source.to_string_lossy().into_owned();

    let exact_backup = area.path("exact-backup");
    let exact_backup_arg = exact_backup.to_string_lossy().into_owned();
    let exact_created = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "create",
        &source_arg,
        "--output",
        &exact_backup_arg,
        "--profile",
        "exact",
        "--audit-scope",
        "excluded",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(exact_created.status.success());
    let exact_created_text = String::from_utf8_lossy(&exact_created.stdout);
    assert!(exact_created_text.contains("\"status\":\"created\""));
    assert!(exact_created_text.contains("\"profile\":\"ExactDatabase\""));
    assert!(!exact_created_text.contains(&source_arg));
    assert!(!exact_created_text.contains(&exact_backup_arg));

    let nested_destination = source.join("nested-restore");
    let nested_arg = nested_destination.to_string_lossy().into_owned();
    let nested_restore = run(&[
        "--format=jsonl",
        "v1",
        "restore",
        "clone",
        &area.path("backup-path-canary").to_string_lossy(),
        "--authorize-with",
        &source_arg,
        "--output",
        &nested_arg,
        "--profile",
        "exact",
        "--audit-scope",
        "excluded",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert_eq!(nested_restore.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&nested_restore.stdout).contains("\"code\":\"InvalidRequest\"")
    );
    assert!(!nested_destination.exists());

    let exact_destination = area.path("exact-clone");
    let exact_destination_arg = exact_destination.to_string_lossy().into_owned();
    let exact_restored = run(&[
        "--format=jsonl",
        "v1",
        "restore",
        "clone",
        &exact_backup_arg,
        "--authorize-with",
        &source_arg,
        "--output",
        &exact_destination_arg,
        "--profile",
        "exact",
        "--audit-scope",
        "excluded",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(exact_restored.status.success());
    let exact_restored_text = String::from_utf8_lossy(&exact_restored.stdout);
    assert!(exact_restored_text.contains("\"type\":\"restore_clone\""));
    assert!(exact_restored_text.contains("\"profile\":\"ExactDatabase\""));
    assert!(exact_restored_text.contains("\"target_verified\":true"));
    assert!(!exact_restored_text.contains(&source_arg));
    assert!(!exact_restored_text.contains(&exact_destination_arg));

    append_host_raw_read_attempt(&source, principal)?;
    let audit_backup = area.path("audit-complete-backup");
    let audit_backup_arg = audit_backup.to_string_lossy().into_owned();
    let audit_created = run(&[
        "--format=jsonl",
        "v1",
        "backup",
        "create",
        &source_arg,
        "--output",
        &audit_backup_arg,
        "--profile",
        "audit-complete",
        "--audit-scope",
        "included",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(audit_created.status.success());
    let audit_created_text = String::from_utf8_lossy(&audit_created.stdout);
    assert!(audit_created_text.contains("\"status\":\"created\""));
    assert!(audit_created_text.contains("\"profile\":\"AuditComplete\""));
    assert!(audit_created_text.contains("\"audit_safe_sequence\":\"1\""));
    assert!(!audit_created_text.contains(&source_arg));
    assert!(!audit_created_text.contains(&audit_backup_arg));

    let audit_destination = area.path("audit-complete-clone");
    let audit_destination_arg = audit_destination.to_string_lossy().into_owned();
    let audit_restored = run(&[
        "--format=jsonl",
        "v1",
        "restore",
        "clone",
        &audit_backup_arg,
        "--authorize-with",
        &source_arg,
        "--output",
        &audit_destination_arg,
        "--profile",
        "audit-complete",
        "--audit-scope",
        "included",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(audit_restored.status.success());
    let audit_restored_text = String::from_utf8_lossy(&audit_restored.stdout);
    assert!(audit_restored_text.contains("\"profile\":\"AuditComplete\""));
    assert!(audit_restored_text.contains("\"audit_safe_sequence\":\"1\""));
    assert!(audit_restored_text.contains("\"target_verified\":true"));
    assert!(!audit_restored_text.contains(&source_arg));
    assert!(!audit_restored_text.contains(&audit_destination_arg));
    Ok(())
}

#[cfg(windows)]
#[test]
fn cli_purge_plan_requires_explicit_cascade_confirmation_and_preserves_source() -> Result<(), String>
{
    let area = TempArea::create()?;
    let source = area.path("purge-source");
    create_host_policy_project(
        &source,
        &[
            Capability::DataExport,
            Capability::ProjectRead,
            Capability::Purge,
        ],
    )?;
    let (target_space, dependant_space) = install_cli_purge_history(&source)?;
    let source_arg = source.to_string_lossy().into_owned();
    let target_identity = format!("history-space:{target_space}");
    let destination = area.path("purge-destination");
    let destination_arg = destination.to_string_lossy().into_owned();
    let external_copy = format!("exact-backup:{}", "11".repeat(32));

    let rejected_report = area.path("purge-reject-plan.json");
    let rejected_report_arg = rejected_report.to_string_lossy().into_owned();
    let rejected_plan = run(&[
        "v1",
        "purge",
        "plan",
        &source_arg,
        "--destination",
        &destination_arg,
        "--report",
        &rejected_report_arg,
        "--target",
        &target_identity,
        "--mode",
        "reject-if-referenced",
        "--external-inventory",
        "incomplete",
        "--known-copy",
        &external_copy,
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(rejected_plan.status.success());
    let rejected_summary = String::from_utf8_lossy(&rejected_plan.stdout);
    assert!(rejected_summary.contains("approval_possible=false"));
    assert!(rejected_summary.contains("dependants=1"));
    let rejected_fingerprint = rejected_summary
        .split("plan_fingerprint=")
        .nth(1)
        .and_then(|tail| tail.split(',').next())
        .ok_or_else(|| String::from("reject plan did not report its fingerprint"))?;
    let rejected_report_bytes = fs::read(&rejected_report).map_err(|error| error.to_string())?;
    let rejected_report_text = String::from_utf8_lossy(&rejected_report_bytes);
    assert!(rejected_report_text.contains("\"approval_possible\":false"));
    assert!(rejected_report_text.contains(&format!("history-space:{dependant_space}")));
    assert!(rejected_report_text.contains("\"kind\":\"exact-backup\""));
    assert!(!rejected_report_text.contains(&source_arg));
    assert!(!rejected_report_text.contains(&destination_arg));
    assert!(!rejected_report_text.contains(&rejected_report_arg));
    assert!(!destination.exists());

    let reject_run = run(&[
        "v1",
        "purge",
        "run",
        &source_arg,
        "--destination",
        &destination_arg,
        "--confirm-plan",
        rejected_fingerprint,
        "--target",
        &target_identity,
        "--mode",
        "reject-if-referenced",
        "--external-inventory",
        "incomplete",
        "--known-copy",
        &external_copy,
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(!reject_run.status.success());
    assert!(String::from_utf8_lossy(&reject_run.stderr).contains("InvalidRequest"));
    assert!(!destination.exists());

    let cascade_report = area.path("purge-cascade-plan.json");
    let cascade_report_arg = cascade_report.to_string_lossy().into_owned();
    let cascade_plan = run(&[
        "v1",
        "purge",
        "plan",
        &source_arg,
        "--destination",
        &destination_arg,
        "--report",
        &cascade_report_arg,
        "--target",
        &target_identity,
        "--mode",
        "cascade",
        "--external-inventory",
        "incomplete",
        "--known-copy",
        &external_copy,
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(cascade_plan.status.success());
    let cascade_summary = String::from_utf8_lossy(&cascade_plan.stdout);
    assert!(cascade_summary.contains("approval_possible=true"));
    assert!(cascade_summary.contains("dependants=1"));
    let fingerprint = cascade_summary
        .split("plan_fingerprint=")
        .nth(1)
        .and_then(|tail| tail.split(',').next())
        .ok_or_else(|| String::from("cascade plan did not report its fingerprint"))?
        .to_owned();
    let cascade_report_bytes = fs::read(&cascade_report).map_err(|error| error.to_string())?;
    let cascade_report_text = String::from_utf8_lossy(&cascade_report_bytes);
    assert!(cascade_report_text.contains("\"approval_possible\":true"));
    assert!(cascade_report_text.contains(&format!("\"plan_fingerprint\":\"{fingerprint}\"")));
    assert!(cascade_report_text.contains(&format!("history-space:{target_space}")));
    assert!(cascade_report_text.contains(&format!("history-space:{dependant_space}")));
    assert!(cascade_report_text.contains("\"index_families_to_rebuild\":["));
    assert!(cascade_report_text.contains("\"secure_erase_claimed\":false"));

    let before_source = snapshot_tree(&source)?;
    let source_layout = DatabaseLayout::open(&source).map_err(|error| error.to_string())?;
    let source_database_id = source_layout
        .database_id()
        .ok_or_else(|| String::from("purge source DatabaseId is missing"))?;
    let source_revision = ManifestStore::new(source_layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("purge source CURRENT is missing"))?
        .revision();

    let run = run(&[
        "--format=jsonl",
        "v1",
        "purge",
        "run",
        &source_arg,
        "--destination",
        &destination_arg,
        "--confirm-plan",
        &fingerprint,
        "--target",
        &target_identity,
        "--mode",
        "cascade",
        "--external-inventory",
        "incomplete",
        "--known-copy",
        &external_copy,
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(
        run.status.success(),
        "purge run failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let run_text = String::from_utf8_lossy(&run.stdout);
    assert!(run_text.contains("\"type\":\"purge_run\""));
    assert!(run_text.contains(&format!("\"plan_fingerprint\":\"{fingerprint}\"")));
    assert!(run_text.contains("\"source_modified\":false"));
    assert!(run_text.contains("\"secure_erase_claimed\":false"));
    assert!(!run_text.contains(&source_arg));
    assert!(!run_text.contains(&destination_arg));
    assert_eq!(snapshot_tree(&source)?, before_source);

    let destination_layout =
        DatabaseLayout::open(&destination).map_err(|error| error.to_string())?;
    let destination_database_id = destination_layout
        .database_id()
        .ok_or_else(|| String::from("purge destination DatabaseId is missing"))?;
    assert_ne!(destination_database_id, source_database_id);
    let destination_lock = destination_layout
        .try_read_only_lock()
        .map_err(|error| error.to_string())?;
    let verify = StorageVerifier::new(destination_layout.clone())
        .verify(&destination_lock)
        .map_err(|error| error.to_string())?;
    assert!(verify.is_clean());
    assert_eq!(verify.safe_revision().value(), source_revision.value() + 1);
    let purge_report =
        fs::read(destination.join("PURGE_REPORT")).map_err(|error| error.to_string())?;
    assert!(purge_report.starts_with(b"WDBPRG\0\x01"));
    let report_digest = blake3::hash(&purge_report);
    assert!(run_text.contains(&format!("\"report_digest\":\"{}\"", report_digest.to_hex())));
    Ok(())
}

#[cfg(windows)]
#[test]
fn cli_purge_requires_current_purge_permission_and_rejects_in_place_destination()
-> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.path("purge-unauthorized-source");
    create_host_policy_project(&source, &[Capability::DataExport, Capability::ProjectRead])?;
    install_cli_purge_history(&source)?;
    let source_arg = source.to_string_lossy().into_owned();
    let destination = area.path("purge-unauthorized-destination");
    let destination_arg = destination.to_string_lossy().into_owned();
    let report = area.path("purge-unauthorized-plan.json");
    let report_arg = report.to_string_lossy().into_owned();
    let target = format!("history-space:{}", domain_id::<HistorySpaceId>(91)?);
    let before_source = snapshot_tree(&source)?;
    let denied = run(&[
        "v1",
        "purge",
        "plan",
        &source_arg,
        "--destination",
        &destination_arg,
        "--report",
        &report_arg,
        "--target",
        &target,
        "--mode",
        "cascade",
        "--external-inventory",
        "incomplete",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("Unauthorized"));
    assert!(!report.exists());
    assert!(!destination.exists());

    let in_place = run(&[
        "v1",
        "purge",
        "plan",
        &source_arg,
        "--destination",
        &source_arg,
        "--report",
        &report_arg,
        "--target",
        &target,
        "--mode",
        "cascade",
        "--external-inventory",
        "incomplete",
    ])
    .ok_or_else(|| "CLI process could not be started".to_owned())?;
    assert!(!in_place.status.success());
    assert!(String::from_utf8_lossy(&in_place.stderr).contains("InvalidRequest"));
    assert!(!report.exists());
    assert_eq!(snapshot_tree(&source)?, before_source);
    Ok(())
}
