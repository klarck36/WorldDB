use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::FrameError;
use worlddb_storage_file::{DatabaseLayout, FormatProbeError, StorageFileError};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m7-07-{}-{sequence}", std::process::id()));
        DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn format_bytes(&self) -> Result<Vec<u8>, String> {
        fs::read(self.0.join("FORMAT")).map_err(|error| error.to_string())
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn initial_v1_format_fixture_opens_without_rewriting_the_probe() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let fixture = include_bytes!("fixtures/m7-07/format-v1.0.bin");
    fs::write(database.path().join("FORMAT"), fixture).map_err(|error| error.to_string())?;
    let before_open = database.format_bytes()?;

    let layout = DatabaseLayout::open(database.path()).map_err(|error| error.to_string())?;
    assert_eq!(layout.format_capabilities().required_flags(), 0);
    assert_eq!(layout.format_capabilities().optional_flags(), 0);
    assert_eq!(database.format_bytes()?, before_open);
    Ok(())
}

#[test]
fn unsupported_future_format_is_rejected_without_rewriting_the_probe() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let fixture = include_bytes!("fixtures/m7-07/format-v2.0-unsupported.bin");
    fs::write(database.path().join("FORMAT"), fixture).map_err(|error| error.to_string())?;
    let before_open = database.format_bytes()?;

    assert!(matches!(
        DatabaseLayout::open(database.path()),
        Err(StorageFileError::Format(FormatProbeError::Frame(
            FrameError::UnsupportedVersion { major: 2, minor: 0 }
        )))
    ));
    assert_eq!(database.format_bytes()?, before_open);
    Ok(())
}

#[test]
fn unsupported_required_capability_is_rejected_without_rewriting_the_probe() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let fixture = include_bytes!("fixtures/m7-07/format-v1.0-unknown-required-capability.bin");
    fs::write(database.path().join("FORMAT"), fixture).map_err(|error| error.to_string())?;
    let before_open = database.format_bytes()?;

    assert!(matches!(
        DatabaseLayout::open(database.path()),
        Err(StorageFileError::Format(
            FormatProbeError::UnsupportedRequiredCapabilities { flags: 1 }
        ))
    ));
    assert_eq!(database.format_bytes()?, before_open);
    Ok(())
}
