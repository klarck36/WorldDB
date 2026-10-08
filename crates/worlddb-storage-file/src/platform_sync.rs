//! Platform-specific synchronization for durable regular-file writes.

use std::fs::File;
use std::io;

/// Flushes file contents using the strongest operation required by the local profile.
///
/// macOS uses `F_FULLFSYNC`, because `File::sync_all` only requests `fsync` and
/// does not provide the APFS machine-durability operation selected by ODE-006.
/// Errors are returned directly so unsupported full-sync operations fail closed.
#[allow(unsafe_code)] // The OS call is isolated here because Rust exposes no safe F_FULLFSYNC wrapper.
pub(crate) fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        sync_file_with(file, |descriptor| {
            let result = unsafe { libc::fcntl(descriptor, libc::F_FULLFSYNC) };
            if result == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

#[cfg(target_os = "macos")]
fn sync_file_with(
    file: &File,
    sync: impl FnOnce(std::os::fd::RawFd) -> io::Result<()>,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    sync(file.as_raw_fd())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{sync_file, sync_file_with};
    use std::fs::OpenOptions;
    use std::io;
    use std::io::Write;
    use std::path::PathBuf;

    struct RemoveFile(PathBuf);

    impl Drop for RemoveFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn apfs_regular_file_full_sync_succeeds() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "worlddb-apfs-full-sync-{}-{:?}.tmp",
            std::process::id(),
            std::thread::current().id()
        ));
        let _cleanup = RemoveFile(path.clone());
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(b"worlddb apfs full sync probe")?;
        sync_file(&file)?;
        Ok(())
    }

    #[test]
    fn full_sync_error_is_returned_without_fallback() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "worlddb-apfs-full-sync-error-{}-{:?}.tmp",
            std::process::id(),
            std::thread::current().id()
        ));
        let _cleanup = RemoveFile(path.clone());
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let result = sync_file_with(&file, |_| {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "injected F_FULLFSYNC unsupported result",
            ))
        });
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        Ok(())
    }
}
