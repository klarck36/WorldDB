use std::env;
use std::fs;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOperationId, AuditOutcome,
    AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordId, AuditRecordIdentity,
    AuditRetentionPolicy, AuditSequence, Bytes, DomainId, OperationId, Principal, PrincipalId,
    Revision, SecurityEpoch, SecurityPolicyChange, SecurityPolicyRecord, SecurityPolicyRecordId,
    SecurityPolicySnapshot, SecurityPolicyVersion, encode_audit_record,
};
use worlddb_storage_file::{
    DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference, RecoveryDisposition,
    RecoveryFinding, RecoveryManager, RecoveryScanner, RequiredAuditError,
    SecurityPolicyHistoryStore, WalError, WalOperationStatus, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-17-{}-{sequence}", std::process::id()));
        DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn layout(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::open(&self.0).map_err(|error| error.to_string())
    }
}

impl Drop for TempDatabase {
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

fn revision(value: u64) -> Result<Revision, String> {
    Revision::try_from(value).map_err(|error| error.to_string())
}

fn policy(include_registered_principal: bool) -> Result<SecurityPolicySnapshot, String> {
    let mut principals = vec![Principal::new(id::<PrincipalId>(1)?)];
    if include_registered_principal {
        principals.push(Principal::new(id::<PrincipalId>(2)?));
    }
    SecurityPolicySnapshot::new(principals, vec![], vec![], vec![])
        .map_err(|error| error.to_string())
}

fn audit(
    record_tail: u8,
    sequence: u64,
    actor: PrincipalId,
    action: AuditAction,
    object_class: AuditObjectClass,
    revision: Revision,
    operation_id: OperationId,
) -> Result<AuditRecord, String> {
    Ok(AuditRecord::new(
        AuditRecordIdentity {
            record_id: id::<AuditRecordId>(record_tail)?,
            sequence: AuditSequence::new(sequence),
            audit_operation_id: id::<AuditOperationId>(record_tail.wrapping_add(40))?,
        },
        AuditRecordDetails {
            actor,
            action,
            object_class,
            outcome: AuditOutcome::Succeeded,
            commit_context: AuditCommitContext::Committed {
                revision,
                operation_id,
            },
            security_epoch: SecurityEpoch::INITIAL,
            policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(vec![0x31, record_tail]))
                .map_err(|error| error.to_string())?,
        },
    ))
}

fn policy_version(
    revision: Revision,
    epoch: SecurityEpoch,
    include_registered_principal: bool,
) -> Result<SecurityPolicyVersion, String> {
    Ok(SecurityPolicyVersion::new(
        revision,
        epoch,
        policy(include_registered_principal)?,
    ))
}

fn reference(
    receipt: &worlddb_storage_file::SecurityPolicySegmentReceipt,
) -> ManifestSegmentReference {
    ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        receipt.id(),
        receipt.content_digest(),
        receipt.revision(),
    )
}

#[test]
fn generic_required_audit_commits_with_action_and_rejects_bad_binding_or_sequence()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let operation = id::<OperationId>(3)?;
    let record = audit(
        4,
        10,
        id::<PrincipalId>(1)?,
        AuditAction::SecurityPolicyChange,
        AuditObjectClass::SecurityPolicy,
        Revision::FIRST_COMMIT,
        operation,
    )?;
    let receipt = wal
        .commit_required_audit(&lock, operation, b"canonical-policy-action", &record)
        .map_err(|error| error.to_string())?;
    assert_eq!(receipt.revision(), Revision::FIRST_COMMIT);

    let committed = wal
        .committed_required_audit_records(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(committed.len(), 1);
    let committed_record = committed
        .first()
        .ok_or_else(|| String::from("committed audit record is missing"))?;
    assert_eq!(committed_record.revision(), receipt.revision());
    assert_eq!(committed_record.operation_id(), operation);
    assert_eq!(committed_record.record(), &record);

    // Reusing the committed OperationId with the same action and audit is idempotent.
    assert_eq!(
        wal.commit_required_audit(&lock, operation, b"canonical-policy-action", &record)
            .map_err(|error| error.to_string())?,
        receipt
    );

    let wrong_operation = id::<OperationId>(5)?;
    let wrong_binding = audit(
        6,
        11,
        id::<PrincipalId>(1)?,
        AuditAction::AuditConfigurationChange,
        AuditObjectClass::AuditConfiguration,
        revision(2)?,
        id::<OperationId>(26)?,
    )?;
    assert!(matches!(
        wal.commit_required_audit(&lock, wrong_operation, b"retention-change", &wrong_binding),
        Err(RequiredAuditError::InvalidCommitBinding)
    ));

    let repeated_sequence_operation = id::<OperationId>(7)?;
    let repeated_sequence = audit(
        8,
        10,
        id::<PrincipalId>(1)?,
        AuditAction::AuditConfigurationChange,
        AuditObjectClass::AuditConfiguration,
        revision(2)?,
        repeated_sequence_operation,
    )?;
    assert!(matches!(
        wal.commit_required_audit(
            &lock,
            repeated_sequence_operation,
            b"retention-change",
            &repeated_sequence,
        ),
        Err(RequiredAuditError::AuditSequenceNotIncreasing { .. })
    ));
    assert_eq!(
        wal.commit_head(&lock)
            .map_err(|error| error.to_string())?
            .revision(),
        receipt.revision()
    );
    assert_eq!(
        wal.committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn process_exit_after_required_audit_commit_recovers_action_and_record() -> Result<(), String> {
    const ROOT_ENV: &str = "WORLDDB_M5_22_REQUIRED_AUDIT_CRASH_ROOT";
    const TEST_NAME: &str = "process_exit_after_required_audit_commit_recovers_action_and_record";
    const ACTION: &[u8] = b"m5-22-atomic-policy-action";
    let operation = id::<OperationId>(90)?;
    let record = audit(
        91,
        1,
        id::<PrincipalId>(1)?,
        AuditAction::AuditConfigurationChange,
        AuditObjectClass::AuditConfiguration,
        Revision::FIRST_COMMIT,
        operation,
    )?;

    if let Ok(root) = env::var(ROOT_ENV) {
        let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let receipt = WalPrepareLog::new(&layout)
            .commit_required_audit(&lock, operation, ACTION, &record)
            .map_err(|error| error.to_string())?;
        if receipt.revision() != Revision::FIRST_COMMIT {
            return Err("required audit child committed an unexpected revision".to_owned());
        }
        std::process::exit(86);
    }

    let database = TempDatabase::create()?;
    let status = Command::new(env::current_exe().map_err(|error| error.to_string())?)
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(ROOT_ENV, &database.0)
        .status()
        .map_err(|error| error.to_string())?;
    if status.code() != Some(86) {
        return Err(format!(
            "required-audit child exited with {:?}, expected process exit code 86",
            status.code()
        ));
    }

    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let recovered = RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    assert!(recovered.report().is_clean());
    assert_eq!(recovered.report().safe_revision(), Revision::FIRST_COMMIT);
    let wal = WalPrepareLog::new(&layout);
    let audit_records = wal
        .committed_required_audit_records(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(audit_records.len(), 1);
    let committed = audit_records
        .first()
        .ok_or_else(|| "committed Required Audit Record is missing".to_owned())?;
    assert_eq!(committed.revision(), Revision::FIRST_COMMIT);
    assert_eq!(committed.operation_id(), operation);
    assert_eq!(committed.record(), &record);
    assert!(matches!(
        wal.operation_status(&lock, operation)
            .map_err(|error| error.to_string())?,
        WalOperationStatus::Committed(receipt)
            if receipt.revision() == Revision::FIRST_COMMIT
    ));
    Ok(())
}

#[test]
fn policy_and_audit_configuration_changes_recover_with_their_required_audits() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = SecurityPolicyHistoryStore::new(layout.clone());
    let actor = id::<PrincipalId>(1)?;
    let first_operation = id::<OperationId>(10)?;
    let retention_before =
        AuditRetentionPolicy::new(86_400_000, 2_048).map_err(|error| error.to_string())?;
    let retention_after =
        AuditRetentionPolicy::new(172_800_000, 1_024).map_err(|error| error.to_string())?;

    let initial_receipt = store
        .stage_version(
            &lock,
            &policy_version(Revision::GENESIS, SecurityEpoch::INITIAL, false)?,
            None,
            Some(retention_before),
        )
        .map_err(|error| error.to_string())?;
    let policy_change_record = SecurityPolicyRecord::new(
        id::<SecurityPolicyRecordId>(11)?,
        Revision::FIRST_COMMIT,
        actor,
        SecurityEpoch::new(1),
        vec![SecurityPolicyChange::PrincipalRegistered {
            principal_id: id::<PrincipalId>(2)?,
        }],
    )
    .map_err(|error| error.to_string())?;
    let policy_receipt = store
        .stage_version(
            &lock,
            &policy_version(Revision::FIRST_COMMIT, SecurityEpoch::new(1), true)?,
            Some(&policy_change_record),
            Some(retention_before),
        )
        .map_err(|error| error.to_string())?;
    let initial_reference = reference(&initial_receipt);
    let policy_reference = reference(&policy_receipt);
    let policy_audit = audit(
        12,
        1,
        actor,
        AuditAction::SecurityPolicyChange,
        AuditObjectClass::SecurityPolicy,
        Revision::FIRST_COMMIT,
        first_operation,
    )?;
    let first_commit = wal
        .commit_audited_manifest_snapshot(
            &lock,
            first_operation,
            vec![initial_reference, policy_reference],
            &[initial_reference, policy_reference],
            &policy_audit,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(first_commit.revision(), Revision::FIRST_COMMIT);
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        wal.commit_audited_manifest_snapshot(
            &lock,
            first_operation,
            vec![initial_reference, policy_reference],
            &[initial_reference, policy_reference],
            &policy_audit,
        )
        .map_err(|error| error.to_string())?,
        first_commit
    );

    let config_operation = id::<OperationId>(13)?;
    let config_receipt = store
        .stage_version(
            &lock,
            &policy_version(revision(2)?, SecurityEpoch::new(1), true)?,
            None,
            Some(retention_after),
        )
        .map_err(|error| error.to_string())?;
    let config_reference = reference(&config_receipt);
    let config_audit = audit(
        14,
        2,
        actor,
        AuditAction::AuditConfigurationChange,
        AuditObjectClass::AuditConfiguration,
        revision(2)?,
        config_operation,
    )?;
    let second_commit = wal
        .commit_audited_manifest_snapshot(
            &lock,
            config_operation,
            vec![initial_reference, policy_reference, config_reference],
            &[config_reference],
            &config_audit,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(second_commit.revision(), revision(2)?);
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    let restored = store
        .load_history(
            revision(2)?,
            &[
                initial_receipt.id(),
                policy_receipt.id(),
                config_receipt.id(),
            ],
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(restored.policy().versions().len(), 3);
    assert_eq!(
        restored
            .policy()
            .version_at(Revision::FIRST_COMMIT)
            .map_err(|error| error.to_string())?
            .snapshot()
            .principals()
            .len(),
        2
    );
    assert_eq!(
        restored
            .audit_retention_at(Revision::FIRST_COMMIT)
            .map_err(|error| error.to_string())?,
        Some(retention_before)
    );
    assert_eq!(
        restored
            .audit_retention_at(revision(2)?)
            .map_err(|error| error.to_string())?,
        Some(retention_after)
    );

    let records = wal
        .committed_required_audit_records(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(records.len(), 2);
    let policy_record = records
        .first()
        .ok_or_else(|| String::from("policy audit record is missing"))?;
    let config_record = records
        .get(1)
        .ok_or_else(|| String::from("audit configuration record is missing"))?;
    assert_eq!(policy_record.record(), &policy_audit);
    assert_eq!(policy_record.revision(), first_commit.revision());
    assert_eq!(config_record.record(), &config_audit);
    assert_eq!(config_record.revision(), second_commit.revision());
    Ok(())
}

#[test]
fn wrong_required_audit_binding_does_not_publish_staged_policy_action() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = SecurityPolicyHistoryStore::new(layout.clone());
    let staged = store
        .stage_version(
            &lock,
            &policy_version(Revision::FIRST_COMMIT, SecurityEpoch::new(1), true)?,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
    let staged_reference = reference(&staged);
    let operation = id::<OperationId>(20)?;
    let mismatched_audit = audit(
        21,
        1,
        id::<PrincipalId>(1)?,
        AuditAction::SecurityPolicyChange,
        AuditObjectClass::SecurityPolicy,
        revision(2)?,
        operation,
    )?;

    assert!(matches!(
        wal.commit_audited_manifest_snapshot(
            &lock,
            operation,
            vec![staged_reference],
            &[staged_reference],
            &mismatched_audit,
        ),
        Err(worlddb_storage_file::SnapshotCommitError::RequiredAudit(
            RequiredAuditError::InvalidCommitBinding
        ))
    ));
    assert_eq!(
        wal.commit_head(&lock)
            .map_err(|error| error.to_string())?
            .revision(),
        Revision::GENESIS
    );
    assert!(
        wal.committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    assert!(
        RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?
            .report()
            .current_manifest()
            .eq(&worlddb_storage_file::CurrentManifestState::Missing)
    );
    Ok(())
}

#[test]
fn checksum_valid_but_mismatched_audit_record_quarantines_the_committed_prefix()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let operation = id::<OperationId>(31)?;
    let mismatched_audit = audit(
        32,
        1,
        id::<PrincipalId>(1)?,
        AuditAction::SecurityPolicyChange,
        AuditObjectClass::SecurityPolicy,
        Revision::FIRST_COMMIT,
        id::<OperationId>(33)?,
    )?;
    let audit_bytes = encode_audit_record(&mismatched_audit).map_err(|error| error.to_string())?;
    let action = b"durable-policy-action";
    let mut payload = b"WDBAUD\0\x01".to_vec();
    payload.extend_from_slice(
        &u32::try_from(action.len())
            .map_err(|error| error.to_string())?
            .to_le_bytes(),
    );
    payload.extend_from_slice(
        &u32::try_from(audit_bytes.len())
            .map_err(|error| error.to_string())?
            .to_le_bytes(),
    );
    payload.extend_from_slice(action);
    payload.extend_from_slice(&audit_bytes);
    wal.commit_operation(&lock, operation, &payload)
        .map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        report.disposition(),
        RecoveryDisposition::QuarantinedReadOnly
    );
    assert!(report.findings().iter().any(|finding| matches!(
        finding,
        RecoveryFinding::CommittedReplayPayloadCorrupt { revision }
            if *revision == Revision::FIRST_COMMIT
    )));
    assert!(matches!(
        wal.committed_required_audit_records(&lock),
        Err(RequiredAuditError::InvalidCommitBinding)
    ));
    assert!(matches!(
        wal.commit_operation(&lock, id::<OperationId>(34)?, b"another mutation"),
        Err(WalError::RecoveryRequired)
    ));
    Ok(())
}
