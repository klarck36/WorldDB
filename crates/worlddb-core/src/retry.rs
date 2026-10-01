//! Bounded retries for internal replay-safe transactions only.

use crate::errors::CommitOutcome;

/// Maximum total attempts, including the original attempt.
pub(crate) const MAX_REPLAY_SAFE_ATTEMPTS: usize = 2;

/// Rebuilds and retries an internal write from the same authoritative input.
///
/// The callback must re-read current state and re-run validation on every
/// attempt. It may not perform external side effects. User transactions and
/// migrations do not call this helper and remain manual-retry operations.
pub(crate) fn run_internal_replay_safe<I, E>(
    authoritative_input: &I,
    mut attempt: impl FnMut(&I) -> Result<CommitOutcome, E>,
) -> Result<CommitOutcome, E> {
    let mut outcome = attempt(authoritative_input)?;
    for _ in 1..MAX_REPLAY_SAFE_ATTEMPTS {
        if matches!(outcome, CommitOutcome::Conflict(_)) {
            outcome = attempt(authoritative_input)?;
        } else {
            break;
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{MAX_REPLAY_SAFE_ATTEMPTS, run_internal_replay_safe};
    use crate::errors::{CommitOutcome, CommitReceipt, ConflictFact, ConflictReport};
    use crate::ids::{DomainId, OperationId, Revision};

    fn operation_id() -> Result<OperationId, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = 31;
        OperationId::try_from_bytes(bytes)
    }

    fn conflict() -> CommitOutcome {
        CommitOutcome::Conflict(ConflictReport::new(vec![
            ConflictFact::ReadDependencyChanged,
        ]))
    }

    #[test]
    fn replay_safe_retries_only_conflict_and_keeps_authoritative_input() -> Result<(), String> {
        let input = vec![1_u8, 2, 3];
        let mut attempts = 0;
        let outcome = run_internal_replay_safe(&input, |received| {
            assert_eq!(received, &input);
            attempts += 1;
            if attempts == 1 {
                Ok(conflict())
            } else {
                Ok(CommitOutcome::Committed(CommitReceipt::new(
                    operation_id().map_err(|e| e.to_string())?,
                    Revision::FIRST_COMMIT,
                )))
            }
        })
        .map_err(|error: String| error)?;
        assert_eq!(attempts, MAX_REPLAY_SAFE_ATTEMPTS);
        assert!(matches!(outcome, CommitOutcome::Committed(_)));
        Ok(())
    }

    #[test]
    fn persistent_conflict_returns_after_two_total_attempts() {
        let mut attempts = 0;
        let outcome = run_internal_replay_safe(&"same-input", |_| {
            attempts += 1;
            Ok::<_, ()>(conflict())
        });
        assert!(matches!(outcome, Ok(CommitOutcome::Conflict(_))));
        assert_eq!(attempts, MAX_REPLAY_SAFE_ATTEMPTS);
    }

    #[test]
    fn committed_outcome_does_not_retry() {
        let mut attempts = 0;
        let outcome = run_internal_replay_safe(&(), |_| {
            attempts += 1;
            Ok::<_, ()>(CommitOutcome::Committed(CommitReceipt::new(
                OperationId::try_from_bytes({
                    let mut bytes = [0_u8; 16];
                    bytes[6] = 0x70;
                    bytes[8] = 0x80;
                    bytes[15] = 32;
                    bytes
                })
                .map_err(|_| ())?,
                Revision::FIRST_COMMIT,
            )))
        });
        assert!(matches!(outcome, Ok(CommitOutcome::Committed(_))));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn non_conflict_error_is_returned_without_retry() {
        let mut attempts = 0;
        let result = run_internal_replay_safe(&(), |_| {
            attempts += 1;
            Err::<CommitOutcome, _>("validation failed")
        });
        assert_eq!(result, Err("validation failed"));
        assert_eq!(attempts, 1);
    }
}
