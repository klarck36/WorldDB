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
use worlddb_storage_file::{
    DatabaseLayout, ExactBackupManager, ManifestSnapshot, ManifestStore, RawReadAuditWal,
    WalPrepareLog, WriterLockError,
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
    let rules = [Capability::AuditRead, Capability::AuditExport]
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
    let layout = DatabaseLayout::create(&source).map_err(|error| error.to_string())?;
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
    let backup = area.path("backup-path-canary");
    let destination = area.path("restore-target");
    let backup_arg = backup.to_string_lossy().into_owned();
    let destination_arg = destination.to_string_lossy().into_owned();
    let output = run(&[
        "--format=jsonl",
        "v1",
        "restore",
        "clone",
        &backup_arg,
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
