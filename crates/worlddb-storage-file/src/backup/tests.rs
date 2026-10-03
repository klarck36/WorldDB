use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use worlddb_core::{
    DecodedRecord, DomainId, HistorySpaceDefinition, HistorySpaceId, OperationId, Record, Revision,
    decode_record, encode_record_with_flags,
};

use crate::{
    CompactionManager, DatabaseLayout, HistorySegmentStore, ManifestSegmentKind,
    ManifestSegmentReference, RecoveryManager, StorageVerifier, WalPrepareLog,
};

use super::{
    BACKUP_INCOMPLETE_FILE, BackupCheckpoint, BackupError, ExactBackupManager, verify_exact_backup,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const CRASH_EXIT_CODE: i32 = 86;
const SOURCE_ENV: &str = "WORLDDB_M7_16A_BACKUP_CRASH_SOURCE";
const TARGET_ENV: &str = "WORLDDB_M7_16A_BACKUP_CRASH_TARGET";
const CHECKPOINT_ENV: &str = "WORLDDB_M7_16A_BACKUP_CRASH_CHECKPOINT";
const READY_ENV: &str = "WORLDDB_M7_16A_BACKUP_CRASH_READY";
const CONTINUE_ENV: &str = "WORLDDB_M7_16A_BACKUP_CRASH_CONTINUE";
const CRASH_TEST_NAME: &str = "backup::tests::process_crash_at_each_backup_boundary_leaves_only_incomplete_or_verified_target";

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root =
            env::temp_dir().join(format!("worlddb-m7-16a-{}-{sequence}", std::process::id()));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn database(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::create(self.0.join("source")).map_err(|error| error.to_string())
    }

    fn target(&self, name: &str) -> PathBuf {
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

fn operation_id(tail: u8) -> Result<OperationId, String> {
    OperationId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        tail,
    ])
    .map_err(|error| error.to_string())
}

fn history_record(tail: u8) -> Result<DecodedRecord, String> {
    let record = Record::HistorySpaceDefinition(
        HistorySpaceDefinition::new(id::<HistorySpaceId>(tail)?, None, Revision::GENESIS)
            .map_err(|error| error.to_string())?,
    );
    let encoded = encode_record_with_flags(&record, 0).map_err(|error| error.to_string())?;
    decode_record(&encoded).map_err(|error| error.to_string())
}

fn install_snapshot(layout: &DatabaseLayout) -> Result<ManifestSegmentReference, String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[history_record(1)?])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, operation_id(1)?, vec![reference], &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok(reference)
}

fn replace_snapshot_inventory(layout: &DatabaseLayout) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[history_record(2)?])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, operation_id(2)?, vec![reference], &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn checkpoint_name(checkpoint: BackupCheckpoint) -> String {
    match checkpoint {
        BackupCheckpoint::TargetLayoutCreated => "target_layout_created".to_owned(),
        BackupCheckpoint::IncompleteMarkerWritten => "incomplete_marker_written".to_owned(),
        BackupCheckpoint::IncompleteMarkerSynced => "incomplete_marker_synced".to_owned(),
        BackupCheckpoint::ItemCreated(ordinal) => format!("item_created_{ordinal}"),
        BackupCheckpoint::ItemFirstChunkWritten(ordinal) => {
            format!("item_first_chunk_written_{ordinal}")
        }
        BackupCheckpoint::ItemSynced(ordinal) => format!("item_synced_{ordinal}"),
        BackupCheckpoint::ManifestFileWritten => "manifest_file_written".to_owned(),
        BackupCheckpoint::ManifestFileSynced => "manifest_file_synced".to_owned(),
        BackupCheckpoint::ManifestDirectorySynced => "manifest_directory_synced".to_owned(),
        BackupCheckpoint::PrepublicationVerified => "prepublication_verified".to_owned(),
        BackupCheckpoint::CompletionMarkerRemoved => "completion_marker_removed".to_owned(),
        BackupCheckpoint::CompletionDirectorySynced => "completion_directory_synced".to_owned(),
        BackupCheckpoint::FinalVerified => "final_verified".to_owned(),
    }
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn process_crash_at_each_backup_boundary_leaves_only_incomplete_or_verified_target()
-> Result<(), String> {
    if env::var_os(SOURCE_ENV).is_some()
        || env::var_os(TARGET_ENV).is_some()
        || env::var_os(CHECKPOINT_ENV).is_some()
    {
        let source_root = env::var_os(SOURCE_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| format!("child environment is missing {SOURCE_ENV}"))?;
        let target_root = env::var_os(TARGET_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| format!("child environment is missing {TARGET_ENV}"))?;
        let wanted = env::var(CHECKPOINT_ENV)
            .map_err(|error| format!("child environment is missing {CHECKPOINT_ENV}: {error}"))?;
        let source = DatabaseLayout::open(source_root).map_err(|error| error.to_string())?;
        let result = ExactBackupManager::new(source).create_backup_with_checkpoint_and_buffer(
            target_root,
            None,
            None,
            8,
            |_| {},
            |checkpoint| {
                if checkpoint_name(checkpoint) == wanted {
                    if let Some(ready_path) = env::var_os(READY_ENV) {
                        let continue_path = env::var_os(CONTINUE_ENV)
                            .map(PathBuf::from)
                            .unwrap_or_else(|| {
                                std::process::exit(87);
                            });
                        if fs::write(ready_path, b"ready").is_err() {
                            std::process::exit(87);
                        }
                        let start = Instant::now();
                        while !continue_path.exists() {
                            if start.elapsed() > Duration::from_secs(20) {
                                std::process::exit(87);
                            }
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    std::process::exit(CRASH_EXIT_CODE);
                }
            },
        );
        return Err(format!(
            "child did not exit at {wanted}; backup returned {result:?}"
        ));
    }

    let area = TempArea::create()?;
    let source = area.database()?;
    let history_reference = install_snapshot(&source)?;
    let baseline_target = area.target("baseline");
    let baseline = ExactBackupManager::new(source.clone())
        .create_exact_backup(&baseline_target, None)
        .map_err(|error| format!("baseline backup: {error}"))?;
    let item_count = baseline.item_count();
    assert!(item_count > 0, "fixture must contain copied backup items");

    let mut checkpoints = vec![
        "target_layout_created".to_owned(),
        "incomplete_marker_written".to_owned(),
        "incomplete_marker_synced".to_owned(),
        "item_created_1".to_owned(),
        "item_first_chunk_written_1".to_owned(),
    ];
    checkpoints.extend((1..=item_count).map(|ordinal| format!("item_synced_{ordinal}")));
    checkpoints.extend([
        "manifest_file_written".to_owned(),
        "manifest_file_synced".to_owned(),
        "manifest_directory_synced".to_owned(),
        "prepublication_verified".to_owned(),
        "completion_marker_removed".to_owned(),
        "completion_directory_synced".to_owned(),
        "final_verified".to_owned(),
    ]);

    let expected_database_id = source
        .database_id()
        .ok_or_else(|| "source database identity missing".to_owned())?;
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        let target = area.target(&format!("crash-{index}"));
        let output = Command::new(&executable)
            .args(["--exact", CRASH_TEST_NAME, "--nocapture"])
            .env(SOURCE_ENV, source.root())
            .env(TARGET_ENV, &target)
            .env(CHECKPOINT_ENV, checkpoint)
            .output()
            .map_err(|error| format!("spawn child for {checkpoint}: {error}"))?;
        if output.status.code() != Some(CRASH_EXIT_CODE) {
            return Err(format!(
                "child for {checkpoint} exited with {:?}, expected process exit {CRASH_EXIT_CODE}; {}",
                output.status.code(),
                output_text(&output)
            ));
        }

        assert!(
            target.is_dir(),
            "target layout should exist after {checkpoint}"
        );
        let expect_complete = matches!(
            checkpoint.as_str(),
            "completion_marker_removed" | "completion_directory_synced" | "final_verified"
        );
        match verify_exact_backup(&target, None) {
            Ok(verification) => {
                assert!(
                    expect_complete,
                    "backup unexpectedly verified after {checkpoint}"
                );
                assert_eq!(verification.item_count(), item_count);
                assert_eq!(verification.database_id(), expected_database_id);
                assert_eq!(verification.revision(), Revision::FIRST_COMMIT);
                assert!(verification.storage_report().is_clean());
                assert!(!target.join(BACKUP_INCOMPLETE_FILE).exists());
            }
            Err(BackupError::IncompleteTarget) => {
                assert!(
                    !expect_complete,
                    "complete backup was not verifiable after {checkpoint}"
                );
            }
            Err(error) => {
                return Err(format!(
                    "target after {checkpoint} was neither incomplete nor valid: {error}"
                ));
            }
        }
        assert!(
            matches!(
                ExactBackupManager::new(source.clone()).create_exact_backup(&target, None),
                Err(BackupError::TargetAlreadyExists)
            ),
            "retry must never overwrite the target after {checkpoint}"
        );

        let lock = source
            .try_writer_lock()
            .map_err(|error| format!("reopen source after {checkpoint}: {error}"))?;
        let source_report = StorageVerifier::new(source.clone())
            .verify(&lock)
            .map_err(|error| format!("verify source after {checkpoint}: {error}"))?;
        assert!(source_report.is_clean());
        assert_eq!(source_report.safe_revision(), Revision::FIRST_COMMIT);
        let reclamation = CompactionManager::new(source.clone())
            .reclaim_retired(&lock, &[history_reference])
            .map_err(|error| format!("reconcile backup pin after {checkpoint}: {error}"))?;
        assert_eq!(reclamation.still_referenced(), &[history_reference]);
        let pin_directory = source.staging_directory().join("backup-pins");
        let remaining_pins = fs::read_dir(pin_directory)
            .map_err(|error| format!("list backup pins after {checkpoint}: {error}"))?
            .count();
        assert_eq!(remaining_pins, 0, "stale backup pin survived {checkpoint}");
    }
    Ok(())
}

#[test]
fn process_crash_releases_stale_pin_after_reopen_without_reclaiming_live_snapshot_segment()
-> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.database()?;
    let history_reference = install_snapshot(&source)?;
    let target = area.target("crash-with-live-pin");
    let ready = area.target("child-ready");
    let resume = area.target("child-resume");
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let mut child = Command::new(executable)
        .args(["--exact", CRASH_TEST_NAME, "--nocapture"])
        .env(SOURCE_ENV, source.root())
        .env(TARGET_ENV, &target)
        .env(CHECKPOINT_ENV, "item_created_1")
        .env(READY_ENV, &ready)
        .env(CONTINUE_ENV, &resume)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("spawn backup child: {error}"))?;

    let start = Instant::now();
    while !ready.is_file() {
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("backup child did not reach the live-pin checkpoint".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    }

    replace_snapshot_inventory(&source)?;
    {
        let lock = source
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let outcome = CompactionManager::new(source.clone())
            .reclaim_retired(&lock, &[history_reference])
            .map_err(|error| format!("inspect live backup pin: {error}"))?;
        assert_eq!(outcome.retained_by_pin(), &[history_reference]);
        let old_segment = source.segments_directory().join(format!(
            "segment-{}.wdbseg",
            history_reference.id().to_canonical_string()
        ));
        assert!(
            old_segment.is_file(),
            "live backup pin protects its segment"
        );
    }

    fs::write(&resume, b"resume").map_err(|error| error.to_string())?;
    let status = child
        .wait()
        .map_err(|error| format!("wait for crashed backup child: {error}"))?;
    if status.code() != Some(CRASH_EXIT_CODE) {
        return Err(format!(
            "backup child exited with {:?}, expected process exit {CRASH_EXIT_CODE}",
            status.code()
        ));
    }

    let lock = source
        .try_writer_lock()
        .map_err(|error| format!("reopen source after backup crash: {error}"))?;
    let report = StorageVerifier::new(source.clone())
        .verify(&lock)
        .map_err(|error| format!("verify source after backup crash: {error}"))?;
    assert!(report.is_clean());
    assert_eq!(
        report.safe_revision(),
        Revision::try_from(2).map_err(|e| e.to_string())?
    );
    let outcome = CompactionManager::new(source.clone())
        .reclaim_retired(&lock, &[history_reference])
        .map_err(|error| format!("reconcile stale backup pin: {error}"))?;
    assert_eq!(outcome.reclaimed(), &[history_reference]);
    let old_segment = source.segments_directory().join(format!(
        "segment-{}.wdbseg",
        history_reference.id().to_canonical_string()
    ));
    assert!(
        !old_segment.exists(),
        "stale pin is removed after child exit"
    );
    let pin_count = fs::read_dir(source.staging_directory().join("backup-pins"))
        .map_err(|error| error.to_string())?
        .count();
    assert_eq!(pin_count, 0);
    assert!(matches!(
        verify_exact_backup(&target, None),
        Err(BackupError::IncompleteTarget)
    ));
    Ok(())
}
