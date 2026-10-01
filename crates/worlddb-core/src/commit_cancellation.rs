//! Linearized cancellation at the transaction publication boundary.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const READY: u8 = 0;
const CANCELLATION_REQUESTED: u8 = 1;
const COMMITPOINT_PASSED: u8 = 2;
const COMMITTED: u8 = 3;
const NOT_COMMITTED: u8 = 4;
const UNKNOWN_OUTCOME: u8 = 5;

/// Shared cancellation control for one transaction commit attempt.
///
/// Cancellation and entry into publication race through one atomic state
/// transition. Whichever transition wins defines the outcome: cancellation
/// before the commitpoint prevents publication; cancellation after it returns
/// [`CancellationRequestDisposition::TooLate`] while publication continues.
#[derive(Clone, Debug)]
pub struct CommitCancellation(Arc<AtomicU8>);

impl Default for CommitCancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitCancellation {
    /// Creates a fresh control for one transaction attempt.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(AtomicU8::new(READY)))
    }

    /// Requests cancellation and reports which side of the commitpoint won.
    pub fn request_cancellation(&self) -> CancellationRequestDisposition {
        loop {
            match self.0.load(Ordering::Acquire) {
                READY => {
                    if self
                        .0
                        .compare_exchange(
                            READY,
                            CANCELLATION_REQUESTED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return CancellationRequestDisposition::Signalled;
                    }
                }
                CANCELLATION_REQUESTED => {
                    return CancellationRequestDisposition::AlreadySignalled;
                }
                COMMITPOINT_PASSED => return CancellationRequestDisposition::TooLate,
                COMMITTED | NOT_COMMITTED => {
                    return CancellationRequestDisposition::AlreadyTerminal;
                }
                UNKNOWN_OUTCOME => return CancellationRequestDisposition::OutcomeUnknown,
                _ => return CancellationRequestDisposition::OutcomeUnknown,
            }
        }
    }

    /// Current commit/cancellation phase, including an interrupted unknown outcome.
    #[must_use]
    pub fn state(&self) -> CommitCancellationState {
        match self.0.load(Ordering::Acquire) {
            READY => CommitCancellationState::Ready,
            CANCELLATION_REQUESTED => CommitCancellationState::CancellationRequested,
            COMMITPOINT_PASSED => CommitCancellationState::CommitpointPassed,
            COMMITTED => CommitCancellationState::Committed,
            NOT_COMMITTED => CommitCancellationState::NotCommitted,
            UNKNOWN_OUTCOME => CommitCancellationState::UnknownOutcome,
            _ => CommitCancellationState::UnknownOutcome,
        }
    }

    /// Wins the cancellation race immediately before irreversible publication.
    ///
    /// Backend implementations must do reversible preparation first, call this
    /// method at their actual publication boundary, then finish publication
    /// without polling cancellation again.
    pub fn begin_commitpoint(&self) -> Result<CommitpointPermit, CommitpointError> {
        match self.0.compare_exchange(
            READY,
            COMMITPOINT_PASSED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(CommitpointPermit {
                state: Arc::clone(&self.0),
                finished: false,
            }),
            Err(CANCELLATION_REQUESTED) => Err(CommitpointError::CancelledBeforeCommitpoint),
            Err(COMMITPOINT_PASSED) => Err(CommitpointError::AlreadyStarted),
            Err(COMMITTED | NOT_COMMITTED) => Err(CommitpointError::AlreadyTerminal),
            Err(UNKNOWN_OUTCOME) => Err(CommitpointError::OutcomeUnknown),
            Err(_) => Err(CommitpointError::OutcomeUnknown),
        }
    }
}

/// Result of a cancellation request racing with one commit attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationRequestDisposition {
    /// Cancellation won before publication began.
    Signalled,
    /// An earlier cancellation request already won.
    AlreadySignalled,
    /// The commitpoint won; the caller must let publication finish.
    TooLate,
    /// The attempt has already completed.
    AlreadyTerminal,
    /// The attempt was interrupted after the commitpoint and needs reconciliation.
    OutcomeUnknown,
}

/// Observable phase of one cancellable commit attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitCancellationState {
    Ready,
    CancellationRequested,
    CommitpointPassed,
    Committed,
    NotCommitted,
    UnknownOutcome,
}

/// Why the validated transaction could not enter its commitpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitpointError {
    CancelledBeforeCommitpoint,
    AlreadyStarted,
    AlreadyTerminal,
    OutcomeUnknown,
}

impl fmt::Display for CommitpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CancelledBeforeCommitpoint => "commit cancelled before publication",
            Self::AlreadyStarted => "commitpoint was already entered",
            Self::AlreadyTerminal => "commit attempt is already terminal",
            Self::OutcomeUnknown => "commit attempt outcome requires reconciliation",
        })
    }
}

impl std::error::Error for CommitpointError {}

/// Exclusive authority to finish one publication after its commitpoint.
pub struct CommitpointPermit {
    state: Arc<AtomicU8>,
    finished: bool,
}

impl CommitpointPermit {
    /// Records that the complete batch became visible.
    pub fn committed(mut self) {
        self.state.store(COMMITTED, Ordering::Release);
        self.finished = true;
    }

    /// Records that the backend atomically rejected the batch without effects.
    pub fn not_committed(mut self) {
        self.state.store(NOT_COMMITTED, Ordering::Release);
        self.finished = true;
    }
}

impl Drop for CommitpointPermit {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.state.compare_exchange(
                COMMITPOINT_PASSED,
                UNKNOWN_OUTCOME,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::thread;

    use super::{
        CancellationRequestDisposition, CommitCancellation, CommitCancellationState,
        CommitpointError,
    };

    #[test]
    fn cancellation_before_commitpoint_prevents_entry() {
        let cancellation = CommitCancellation::new();
        assert_eq!(
            cancellation.request_cancellation(),
            CancellationRequestDisposition::Signalled
        );
        assert_eq!(
            cancellation.begin_commitpoint().err(),
            Some(CommitpointError::CancelledBeforeCommitpoint)
        );
        assert_eq!(
            cancellation.state(),
            CommitCancellationState::CancellationRequested
        );
    }

    #[test]
    fn cancellation_after_commitpoint_is_too_late_and_commit_can_finish() -> Result<(), String> {
        let cancellation = CommitCancellation::new();
        let permit = cancellation
            .begin_commitpoint()
            .map_err(|error| error.to_string())?;
        assert_eq!(
            cancellation.request_cancellation(),
            CancellationRequestDisposition::TooLate
        );
        permit.committed();
        assert_eq!(cancellation.state(), CommitCancellationState::Committed);
        assert_eq!(
            cancellation.request_cancellation(),
            CancellationRequestDisposition::AlreadyTerminal
        );
        Ok(())
    }

    #[test]
    fn abandoned_in_flight_permit_records_unknown_outcome() -> Result<(), String> {
        let cancellation = CommitCancellation::new();
        let permit = cancellation
            .begin_commitpoint()
            .map_err(|error| error.to_string())?;
        drop(permit);
        assert_eq!(
            cancellation.state(),
            CommitCancellationState::UnknownOutcome
        );
        assert_eq!(
            cancellation.request_cancellation(),
            CancellationRequestDisposition::OutcomeUnknown
        );
        Ok(())
    }

    #[test]
    fn cancellation_and_commitpoint_race_has_one_linearized_winner() -> Result<(), String> {
        for _ in 0..128 {
            let cancellation = CommitCancellation::new();
            let barrier = std::sync::Arc::new(Barrier::new(3));
            let settled = std::sync::Arc::new(Barrier::new(3));
            let cancel_control = cancellation.clone();
            let cancel_barrier = std::sync::Arc::clone(&barrier);
            let cancel_settled = std::sync::Arc::clone(&settled);
            let cancel_thread = thread::spawn(move || {
                cancel_barrier.wait();
                let disposition = cancel_control.request_cancellation();
                cancel_settled.wait();
                disposition
            });
            let commit_control = cancellation.clone();
            let commit_barrier = std::sync::Arc::clone(&barrier);
            let commit_settled = std::sync::Arc::clone(&settled);
            let commit_thread = thread::spawn(move || {
                commit_barrier.wait();
                let permit = commit_control.begin_commitpoint();
                commit_settled.wait();
                match permit {
                    Ok(permit) => {
                        permit.not_committed();
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            });
            barrier.wait();
            settled.wait();
            let cancel = cancel_thread
                .join()
                .map_err(|_| "cancellation racer panicked".to_owned())?;
            let commit = commit_thread
                .join()
                .map_err(|_| "commitpoint racer panicked".to_owned())?;
            match (cancel, commit) {
                (
                    CancellationRequestDisposition::Signalled,
                    Err(CommitpointError::CancelledBeforeCommitpoint),
                )
                | (CancellationRequestDisposition::TooLate, Ok(())) => {}
                (cancel, commit) => {
                    return Err(format!(
                        "race had no single winner: cancellation={cancel:?}, commit={commit:?}"
                    ));
                }
            }
        }
        Ok(())
    }
}
