//! Advisory, process-wide writer lock for one database directory.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

// Unit tests spawn the test binary to model process crashes. Wait until all
// test WriterLocks are closed before spawning so fork cannot copy live locks.
#[cfg(test)]
struct TestLockRegistry {
    active_locks: usize,
    active_by_thread: std::collections::HashMap<std::thread::ThreadId, usize>,
    waiting_spawns: usize,
}

#[cfg(test)]
static TEST_CHILD_PROCESS_LOCK: std::sync::LazyLock<std::sync::Mutex<TestLockRegistry>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(TestLockRegistry {
            active_locks: 0,
            active_by_thread: std::collections::HashMap::new(),
            waiting_spawns: 0,
        })
    });

#[cfg(test)]
static TEST_CHILD_PROCESS_CONDITION: std::sync::Condvar = std::sync::Condvar::new();

#[cfg(test)]
struct TestLockRegistration {
    owner: std::thread::ThreadId,
}

#[cfg(test)]
impl TestLockRegistry {
    fn register_lock(&mut self) -> TestLockRegistration {
        let owner = std::thread::current().id();
        self.active_locks += 1;
        *self.active_by_thread.entry(owner).or_default() += 1;
        TestLockRegistration { owner }
    }
}

#[cfg(test)]
impl Drop for TestLockRegistration {
    fn drop(&mut self) {
        let mut registry = TEST_CHILD_PROCESS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        registry.active_locks -= 1;
        if let Some(count) = registry.active_by_thread.get_mut(&self.owner) {
            *count -= 1;
            if *count == 0 {
                registry.active_by_thread.remove(&self.owner);
            }
        }
        TEST_CHILD_PROCESS_CONDITION.notify_all();
    }
}

#[cfg(test)]
fn test_child_process_guard() -> std::sync::MutexGuard<'static, TestLockRegistry> {
    let owner = std::thread::current().id();
    let mut registry = TEST_CHILD_PROCESS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while registry.waiting_spawns > 0 && !registry.active_by_thread.contains_key(&owner) {
        registry = TEST_CHILD_PROCESS_CONDITION
            .wait(registry)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    registry
}

#[cfg(test)]
pub(crate) fn test_command_spawn(
    command: &mut std::process::Command,
) -> io::Result<std::process::Child> {
    let owner = std::thread::current().id();
    let mut registry = TEST_CHILD_PROCESS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if registry.active_by_thread.contains_key(&owner) {
        return Err(io::Error::other(
            "test subprocess must not start while its thread holds a database writer lock",
        ));
    }
    registry.waiting_spawns += 1;
    while registry.active_locks > 0 {
        registry = TEST_CHILD_PROCESS_CONDITION
            .wait(registry)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    registry.waiting_spawns -= 1;
    let child = command.spawn();
    TEST_CHILD_PROCESS_CONDITION.notify_all();
    child
}

#[cfg(test)]
pub(crate) fn test_command_status(
    command: &mut std::process::Command,
) -> io::Result<std::process::ExitStatus> {
    test_command_spawn(command)?.wait()
}

#[cfg(test)]
pub(crate) fn test_command_output(
    command: &mut std::process::Command,
) -> io::Result<std::process::Output> {
    use std::process::Stdio;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    test_command_spawn(command)?.wait_with_output()
}

use crate::recovery::RecoveryDisposition;

const MODE_UNVERIFIED: u8 = 0;
const MODE_WRITABLE: u8 = 1;
const MODE_RECOVERY_REQUIRED: u8 = 2;
const MODE_RECOVERY_RUNNING: u8 = 3;
const MODE_READ_ONLY: u8 = 4;

/// Why a database writer lock could not be acquired.
#[derive(Debug)]
pub enum WriterLockError {
    /// Another cooperating process currently owns the lock.
    AlreadyHeld,
    /// A read-only lock was requested, but the stable lock file does not exist.
    LockFileMissing,
    /// The lock file could not be opened or locked for an operating-system reason.
    Io(io::Error),
}

impl fmt::Display for WriterLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyHeld => formatter.write_str("database writer lock is already held"),
            Self::LockFileMissing => {
                formatter.write_str("database lock file is required for read-only access")
            }
            Self::Io(error) => write!(formatter, "database writer lock failed: {error}"),
        }
    }
}

impl std::error::Error for WriterLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::AlreadyHeld | Self::LockFileMissing => None,
        }
    }
}

/// Shared or exclusive database lock held by an operating-system file handle.
///
/// The lock file itself remains in the database directory after drop. The
/// operating system releases ownership when the underlying file handle closes.
/// Shared locks enter read-only mode and cannot be promoted to recovery access.
pub struct WriterLock {
    _file: File,
    database_root: PathBuf,
    write_mode: AtomicU8,
    #[cfg(test)]
    _test_lock_registration: TestLockRegistration,
}

impl WriterLock {
    /// Opens the stable lock file and attempts to acquire its exclusive lock
    /// without waiting for another process.
    pub(crate) fn try_acquire(database_root: &Path, path: &Path) -> Result<Self, WriterLockError> {
        #[cfg(test)]
        let mut test_child_process_guard = test_child_process_guard();

        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(WriterLockError::Io(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "database writer-lock entry is not a regular file",
                )));
            }
        }
        let file = open_lock_file(path).map_err(WriterLockError::Io)?;
        let canonical_root = fs::canonicalize(database_root).map_err(WriterLockError::Io)?;
        let canonical_lock = fs::canonicalize(path).map_err(WriterLockError::Io)?;
        if canonical_lock.parent() != Some(canonical_root.as_path()) {
            return Err(WriterLockError::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "database writer-lock file resolves outside its root",
            )));
        }
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(Self {
                _file: file,
                database_root: canonical_root,
                write_mode: AtomicU8::new(MODE_UNVERIFIED),
                #[cfg(test)]
                _test_lock_registration: test_child_process_guard.register_lock(),
            }),
            Err(fs4::TryLockError::WouldBlock) => Err(WriterLockError::AlreadyHeld),
            Err(fs4::TryLockError::Error(error)) => Err(WriterLockError::Io(error)),
        }
    }

    /// Opens an existing stable lock file and attempts a non-blocking shared lock.
    ///
    /// This path never creates or writes a file. The read-only mode is retained
    /// through recovery scans and cannot be promoted to recovery/write access.
    pub(crate) fn try_acquire_read_only(
        database_root: &Path,
        path: &Path,
    ) -> Result<Self, WriterLockError> {
        #[cfg(test)]
        let mut test_child_process_guard = test_child_process_guard();

        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(WriterLockError::Io(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "database lock entry is not a regular file",
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(WriterLockError::LockFileMissing);
            }
            Err(error) => return Err(WriterLockError::Io(error)),
        }
        let file = open_existing_lock_file_read_only(path).map_err(WriterLockError::Io)?;
        let canonical_root = fs::canonicalize(database_root).map_err(WriterLockError::Io)?;
        let canonical_lock = fs::canonicalize(path).map_err(WriterLockError::Io)?;
        if canonical_lock.parent() != Some(canonical_root.as_path()) {
            return Err(WriterLockError::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "database lock file resolves outside its root",
            )));
        }
        match fs4::FileExt::try_lock_shared(&file) {
            Ok(()) => Ok(Self {
                _file: file,
                database_root: canonical_root,
                write_mode: AtomicU8::new(MODE_READ_ONLY),
                #[cfg(test)]
                _test_lock_registration: test_child_process_guard.register_lock(),
            }),
            Err(fs4::TryLockError::WouldBlock) => Err(WriterLockError::AlreadyHeld),
            Err(fs4::TryLockError::Error(error)) => Err(WriterLockError::Io(error)),
        }
    }

    pub(crate) fn belongs_to_database_root(&self, root: &Path) -> bool {
        self.database_root == root
    }

    pub(crate) fn require_write_access(&self) -> bool {
        matches!(
            self.write_mode.load(Ordering::Acquire),
            MODE_WRITABLE | MODE_RECOVERY_RUNNING
        )
    }

    pub(crate) fn note_recovery_disposition(&self, disposition: RecoveryDisposition) {
        let current = self.write_mode.load(Ordering::Acquire);
        if current == MODE_READ_ONLY {
            return;
        }
        if current == MODE_RECOVERY_RUNNING
            && disposition != RecoveryDisposition::QuarantinedReadOnly
        {
            return;
        }
        let next = match disposition {
            RecoveryDisposition::Clean => MODE_WRITABLE,
            RecoveryDisposition::RecoveryRequired => MODE_RECOVERY_REQUIRED,
            RecoveryDisposition::QuarantinedReadOnly => MODE_READ_ONLY,
        };
        self.write_mode.store(next, Ordering::Release);
    }

    pub(crate) fn recovery_scan_failed(&self) {
        let current = self.write_mode.load(Ordering::Acquire);
        if current != MODE_READ_ONLY {
            self.write_mode
                .store(MODE_RECOVERY_REQUIRED, Ordering::Release);
        }
    }

    pub(crate) fn begin_recovery(&self) -> Result<RecoveryWriteGuard<'_>, ()> {
        loop {
            let current = self.write_mode.load(Ordering::Acquire);
            if matches!(
                current,
                MODE_READ_ONLY | MODE_UNVERIFIED | MODE_RECOVERY_RUNNING
            ) {
                return Err(());
            }
            if self
                .write_mode
                .compare_exchange(
                    current,
                    MODE_RECOVERY_RUNNING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Ok(RecoveryWriteGuard {
                    lock: self,
                    finished: false,
                });
            }
        }
    }
}

pub(crate) struct RecoveryWriteGuard<'a> {
    lock: &'a WriterLock,
    finished: bool,
}

impl RecoveryWriteGuard<'_> {
    pub(crate) fn finish(mut self) {
        if self.lock.write_mode.load(Ordering::Acquire) == MODE_RECOVERY_RUNNING {
            self.lock.write_mode.store(MODE_WRITABLE, Ordering::Release);
        }
        self.finished = true;
    }
}

impl Drop for RecoveryWriteGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.lock.write_mode.compare_exchange(
                MODE_RECOVERY_RUNNING,
                MODE_RECOVERY_REQUIRED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    options.open(path)
}

fn open_existing_lock_file_read_only(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    options.open(path)
}
