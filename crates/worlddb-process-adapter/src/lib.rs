#![deny(unsafe_code)]

//! OS process containment for untrusted WorldDB import/export adapters.

use std::io;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};

#[cfg(windows)]
mod windows;

/// Why the current operating-system process identity is unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessIdentityError {
    /// The platform has no verified identity reader in this build.
    UnsupportedPlatform,
    /// The operating system did not return a valid process-token identity.
    OperatingSystemFailure,
}

/// Reads opaque identity bytes from the current process token.
///
/// On Windows this returns the binary user SID. Other platforms fail closed until M9-07.
pub fn current_process_identity_bytes() -> Result<Vec<u8>, ProcessIdentityError> {
    #[cfg(windows)]
    {
        windows::current_process_identity_bytes()
            .map_err(|_| ProcessIdentityError::OperatingSystemFailure)
    }

    #[cfg(not(windows))]
    {
        Err(ProcessIdentityError::UnsupportedPlatform)
    }
}

/// A child process contained by an OS job with a hard committed-memory limit.
///
/// Windows applies the limit to the child process tree. Other platforms fail
/// closed until their process-limit adapter is implemented and verified.
pub struct IsolatedChild {
    child: Child,
    #[cfg(windows)]
    _job: windows::JobObject,
}

impl IsolatedChild {
    /// Starts a child suspended, assigns its process tree to a memory-limited
    /// OS job, and only then allows the child to run.
    pub fn spawn(command: &mut Command, memory_limit_bytes: u64) -> io::Result<Self> {
        if memory_limit_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "process memory limit must be positive",
            ));
        }

        #[cfg(windows)]
        {
            windows::spawn(command, memory_limit_bytes)
        }

        #[cfg(not(windows))]
        {
            let _ = command;
            let _ = memory_limit_bytes;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "hard process memory limits are not implemented on this platform",
            ))
        }
    }

    /// Returns the operating-system process identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Takes the child's standard input pipe once.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// Takes the child's standard output pipe once.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Takes the child's standard error pipe once.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// Checks whether the child has exited.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Terminates the direct child. Dropping this wrapper also terminates any
    /// descendants still attached to the Windows job object.
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    /// Waits for the direct child to exit.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    /// Stops every process remaining in the operating-system job.
    pub fn terminate_process_tree(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            windows::terminate_process_tree(&self._job)
        }

        #[cfg(not(windows))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process-tree termination is not implemented on this platform",
            ))
        }
    }
}

#[cfg(all(test, windows))]
mod process_identity_tests {
    #[test]
    fn current_process_identity_is_a_bounded_binary_sid() {
        let identity = super::current_process_identity_bytes();
        assert!(identity.is_ok());
        if let Ok(identity) = identity {
            assert!(!identity.is_empty());
            assert!(identity.len() <= 1024);
        }
    }
}
