//! Narrow Windows NTFS file-replacement boundary.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

/// Publishes one same-volume file with write-through semantics.
///
/// `replace` is false for immutable manifest generation names and true only
/// for the single mutable `CURRENT` pointer.
pub(crate) fn move_file(stage: &Path, target: &Path, replace: bool) -> io::Result<()> {
    let stage = encode_local_path(stage)?;
    let target = encode_local_path(target)?;
    let mut flags = MOVEFILE_WRITE_THROUGH;
    if replace {
        flags |= MOVEFILE_REPLACE_EXISTING;
    }

    let succeeded = call_move_file(&stage, &target, flags);
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Publishes a complete same-volume directory without replacing any target.
pub(crate) fn move_directory(stage: &Path, target: &Path) -> io::Result<()> {
    let stage = encode_local_path(stage)?;
    let target = encode_local_path(target)?;
    let succeeded = call_move_file(&stage, &target, MOVEFILE_WRITE_THROUGH);
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[allow(unsafe_code, reason = "WDB-EXC-0002")]
fn call_move_file(stage: &[u16], target: &[u16], flags: u32) -> windows_sys::core::BOOL {
    // SAFETY: both paths are absolute, NUL-terminated UTF-16 buffers which remain alive for the
    // call; this function controls the exact same-volume staging and destination paths.
    // TEST: Windows M5-09 tests exercise new immutable publication, pointer replacement, and a
    // held-handle sharing violation that must leave the old CURRENT target in place.
    // REVIEW: WDB-EXC-0002 confines the FFI to this adapter; MoveFileExW flags are checked against
    // the Microsoft Win32 documentation linked from docs/M5-09-verification.md.
    unsafe { MoveFileExW(stage.as_ptr(), target.as_ptr(), flags) }
}

fn encode_local_path(path: &Path) -> io::Result<Vec<u16>> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let encoded: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    if encoded.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows publication path contains a NUL character",
        ));
    }
    const EXTENDED_PREFIX: &[u16] = &[92, 92, 63, 92];
    const EXTENDED_UNC_PREFIX: &[u16] = &[92, 92, 63, 92, 85, 78, 67, 92];
    if encoded.get(..EXTENDED_UNC_PREFIX.len()) == Some(EXTENDED_UNC_PREFIX) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "network paths are not enabled for Machine-durable publication",
        ));
    }
    if encoded.get(..EXTENDED_PREFIX.len()) == Some(EXTENDED_PREFIX) {
        return nul_terminated(encoded);
    }
    if encoded.get(..2) == Some(&[92, 92]) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "network paths are not enabled for Machine-durable publication",
        ));
    }

    let mut extended = Vec::new();
    extended.extend([b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]);
    extended.extend(encoded);
    nul_terminated(extended)
}

fn nul_terminated(mut path: Vec<u16>) -> io::Result<Vec<u16>> {
    if path.last() == Some(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows publication path must not include a trailing NUL",
        ));
    }
    path.push(0);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::encode_local_path;
    use std::io;
    use std::path::Path;

    #[test]
    fn absolute_local_paths_use_the_extended_length_prefix() -> Result<(), String> {
        let path = encode_local_path(Path::new(r"C:\worlddb\manifests\manifest.wdbm"))
            .map_err(|error| error.to_string())?;
        assert!(
            path.get(..4)
                .is_some_and(|prefix| prefix == [92_u16, 92, 63, 92])
        );
        assert_eq!(path.last(), Some(&0));
        Ok(())
    }

    #[test]
    fn network_paths_fail_closed_before_publication() {
        assert_eq!(
            encode_local_path(Path::new(r"\\server\share\worlddb\CURRENT"))
                .err()
                .map(|error| error.kind()),
            Some(io::ErrorKind::Unsupported)
        );
        assert_eq!(
            encode_local_path(Path::new(r"\\?\UNC\server\share\worlddb\CURRENT"))
                .err()
                .map(|error| error.kind()),
            Some(io::ErrorKind::Unsupported)
        );
    }
}
