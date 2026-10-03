use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId};
use worlddb_storage_file::{
    DatabaseLayout, ManifestSnapshot, ManifestStore, WalPrepareLog, WriterLockError,
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
