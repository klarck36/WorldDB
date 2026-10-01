use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{CHECKSUM_LEN, FrameHeader, encode_frame};
use worlddb_storage_file::{
    DatabaseLayout, FORMAT_FILE_KIND, FormatProbeError, StorageFileError, WriterLockError,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-02-{}-{sequence}", std::process::id()));
        DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fixed_layout_uses_root_local_staging_and_format_resave() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = DatabaseLayout::open(database.path()).map_err(|error| error.to_string())?;
    let canonical_staging =
        fs::canonicalize(layout.staging_directory()).map_err(|error| error.to_string())?;

    assert_eq!(
        layout.root(),
        fs::canonicalize(database.path()).map_err(|e| e.to_string())?
    );
    assert!(canonical_staging.starts_with(layout.root()));
    assert!(layout.wal_directory().is_dir());
    assert!(layout.segments_directory().is_dir());
    assert!(layout.manifests_directory().is_dir());
    assert!(layout.audit_wal_directory().is_dir());
    assert!(layout.audit_segments_directory().is_dir());
    assert!(layout.quarantine_directory().is_dir());

    layout.resave_format().map_err(|error| error.to_string())?;
    let reopened = DatabaseLayout::open(database.path()).map_err(|error| error.to_string())?;
    assert_eq!(reopened.format_capabilities().optional_flags(), 0);
    Ok(())
}

#[test]
fn unknown_optional_capability_survives_open_and_resave_unchanged() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let flags = 0x8000_0042_0000_0001;
    let frame = encode_frame(
        FrameHeader::new(FORMAT_FILE_KIND).with_optional_flags(flags),
        &[],
    )
    .map_err(|error| error.to_string())?;
    fs::write(database.path().join("FORMAT"), frame).map_err(|error| error.to_string())?;

    let layout = DatabaseLayout::open(database.path()).map_err(|error| error.to_string())?;
    assert_eq!(layout.format_capabilities().optional_flags(), flags);
    assert_eq!(layout.format_capabilities().unknown_optional_flags(), flags);
    layout.resave_format().map_err(|error| error.to_string())?;

    let reopened = DatabaseLayout::open(database.path()).map_err(|error| error.to_string())?;
    assert_eq!(reopened.format_capabilities().optional_flags(), flags);
    assert_eq!(
        reopened.format_capabilities().unknown_optional_flags(),
        flags
    );
    Ok(())
}

#[test]
fn unknown_required_capability_is_rejected_even_with_valid_checksum() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let mut frame =
        encode_frame(FrameHeader::new(FORMAT_FILE_KIND), &[]).map_err(|error| error.to_string())?;
    let required_flags = frame
        .get_mut(12..20)
        .ok_or_else(|| String::from("encoded frame omitted required-capability bytes"))?;
    required_flags.copy_from_slice(&1_u64.to_le_bytes());
    let checksum_start = frame
        .len()
        .checked_sub(CHECKSUM_LEN)
        .ok_or_else(|| String::from("encoded frame omitted checksum"))?;
    let (signed_bytes, checksum_bytes) = frame.split_at_mut(checksum_start);
    let checksum = blake3::hash(signed_bytes);
    checksum_bytes.copy_from_slice(checksum.as_bytes());
    fs::write(database.path().join("FORMAT"), frame).map_err(|error| error.to_string())?;

    assert!(matches!(
        DatabaseLayout::open(database.path()),
        Err(StorageFileError::Format(
            FormatProbeError::UnsupportedRequiredCapabilities { flags: 1 }
        ))
    ));
    Ok(())
}

#[test]
fn a_second_process_cannot_acquire_the_writer_lock() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = DatabaseLayout::open(database.path()).map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;

    let blocked = run_lock_probe(database.path(), "blocked")?;
    assert!(
        blocked.status.success(),
        "writer-lock child failed: {}",
        output_text(&blocked)
    );

    drop(lock);
    let available = run_lock_probe(database.path(), "available")?;
    assert!(
        available.status.success(),
        "writer-lock child failed after release: {}",
        output_text(&available)
    );
    Ok(())
}

#[test]
fn writer_lock_child_probe() {
    let Ok(root) = env::var("WORLDDB_M5_02_LOCK_ROOT") else {
        return;
    };
    let Ok(expected) = env::var("WORLDDB_M5_02_LOCK_EXPECTED") else {
        return;
    };
    let layout = DatabaseLayout::open(root);
    assert!(
        layout.is_ok(),
        "child could not open database layout: {layout:?}"
    );
    let Ok(layout) = layout else {
        return;
    };
    match expected.as_str() {
        "blocked" => {
            assert!(matches!(
                layout.try_writer_lock(),
                Err(WriterLockError::AlreadyHeld)
            ));
            assert!(matches!(
                layout.resave_format(),
                Err(StorageFileError::WriterLock(WriterLockError::AlreadyHeld))
            ));
        }
        "available" => assert!(layout.try_writer_lock().is_ok()),
        _ => assert_eq!(expected, "blocked"),
    }
}

fn run_lock_probe(root: &Path, expected: &str) -> Result<Output, String> {
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    Command::new(executable)
        .arg("writer_lock_child_probe")
        .arg("--nocapture")
        .env("WORLDDB_M5_02_LOCK_ROOT", root)
        .env("WORLDDB_M5_02_LOCK_EXPECTED", expected)
        .output()
        .map_err(|error| error.to_string())
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
