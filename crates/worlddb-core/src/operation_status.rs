//! Payload-bound OperationId deduplication and commit-status reconciliation.

use std::collections::BTreeMap;
use std::fmt;

use crate::errors::CommitReceipt;
use crate::ids::OperationId;

/// Status returned by commit-status lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    /// This OperationId has a known committed receipt.
    Committed(CommitReceipt),
    /// The journal proves that no commit occurred, so the same payload may retry.
    NotCommitted,
    /// A commit may have occurred; reconcile before any new attempt.
    Indeterminate,
}

/// Result of registering a logical commit attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationBeginOutcome {
    /// Caller owns the only in-memory attempt for this OperationId and payload.
    Execute(OperationAttempt),
    /// The same operation is already committed or has an unresolved outcome.
    Existing(OperationStatus),
}

/// Payload-bound proof required to update one journal entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationAttempt {
    operation_id: OperationId,
    payload_fingerprint: [u8; 32],
}

impl OperationAttempt {
    /// Logical operation identity.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JournalEntry {
    payload_fingerprint: [u8; 32],
    status: OperationStatus,
}

/// In-memory model of the persistent OperationId index.
///
/// A durable backend must store each state transition with its commit marker
/// and recover unresolved entries as `Indeterminate` until reconciliation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OperationStatusJournal {
    entries: BTreeMap<OperationId, JournalEntry>,
}

impl OperationStatusJournal {
    /// Creates an empty operation-status journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks up an OperationId; absence proves NotCommitted in this journal view.
    #[must_use]
    pub fn status(&self, operation_id: OperationId) -> OperationStatus {
        self.entries
            .get(&operation_id)
            .map_or(OperationStatus::NotCommitted, |entry| entry.status.clone())
    }

    /// Binds an OperationId to a payload and reserves one execution attempt.
    ///
    /// Repeating a committed request returns the existing receipt. Repeating an
    /// unresolved request returns `Indeterminate` without executing it again.
    /// A request known not to have committed may begin again with the same bytes.
    pub fn begin(
        &mut self,
        operation_id: OperationId,
        canonical_payload: &[u8],
    ) -> Result<OperationBeginOutcome, OperationStatusError> {
        let fingerprint = *blake3::hash(canonical_payload).as_bytes();
        let attempt = OperationAttempt {
            operation_id,
            payload_fingerprint: fingerprint,
        };
        match self.entries.get_mut(&operation_id) {
            Some(entry) if entry.payload_fingerprint != fingerprint => {
                Err(OperationStatusError::IdempotencyMismatch)
            }
            Some(entry) => match &entry.status {
                OperationStatus::Committed(receipt) => Ok(OperationBeginOutcome::Existing(
                    OperationStatus::Committed(receipt.clone()),
                )),
                OperationStatus::Indeterminate => Ok(OperationBeginOutcome::Existing(
                    OperationStatus::Indeterminate,
                )),
                OperationStatus::NotCommitted => {
                    entry.status = OperationStatus::Indeterminate;
                    Ok(OperationBeginOutcome::Execute(attempt))
                }
            },
            None => {
                self.entries.insert(
                    operation_id,
                    JournalEntry {
                        payload_fingerprint: fingerprint,
                        status: OperationStatus::Indeterminate,
                    },
                );
                Ok(OperationBeginOutcome::Execute(attempt))
            }
        }
    }

    /// Records that validation or another pre-commit failure proved no commit occurred.
    pub fn mark_not_committed(
        &mut self,
        attempt: OperationAttempt,
    ) -> Result<(), OperationStatusError> {
        let entry = self.entry_for_attempt_mut(attempt)?;
        if entry.status != OperationStatus::Indeterminate {
            return Err(OperationStatusError::AttemptNotActive);
        }
        entry.status = OperationStatus::NotCommitted;
        Ok(())
    }

    /// Records the receipt after the commitpoint is known to have succeeded.
    pub fn mark_committed(
        &mut self,
        attempt: OperationAttempt,
        receipt: CommitReceipt,
    ) -> Result<(), OperationStatusError> {
        if receipt.operation_id() != attempt.operation_id {
            return Err(OperationStatusError::ReceiptOperationMismatch);
        }
        let entry = self.entry_for_attempt_mut(attempt)?;
        if entry.status != OperationStatus::Indeterminate {
            return Err(OperationStatusError::AttemptNotActive);
        }
        entry.status = OperationStatus::Committed(receipt);
        Ok(())
    }

    fn entry_for_attempt_mut(
        &mut self,
        attempt: OperationAttempt,
    ) -> Result<&mut JournalEntry, OperationStatusError> {
        let entry = self
            .entries
            .get_mut(&attempt.operation_id)
            .ok_or(OperationStatusError::UnknownAttempt)?;
        if entry.payload_fingerprint != attempt.payload_fingerprint {
            return Err(OperationStatusError::IdempotencyMismatch);
        }
        Ok(entry)
    }
}

/// Invalid payload binding or operation-status transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationStatusError {
    /// An OperationId was reused with different canonical payload bytes.
    IdempotencyMismatch,
    /// The supplied receipt names another OperationId.
    ReceiptOperationMismatch,
    /// The attempt is absent from this journal view.
    UnknownAttempt,
    /// This attempt is no longer in an unresolved active state.
    AttemptNotActive,
}

impl fmt::Display for OperationStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdempotencyMismatch => formatter.write_str("OperationId payload does not match"),
            Self::ReceiptOperationMismatch => {
                formatter.write_str("receipt OperationId does not match attempt")
            }
            Self::UnknownAttempt => {
                formatter.write_str("OperationId attempt is absent from the journal")
            }
            Self::AttemptNotActive => formatter.write_str("OperationId attempt is not unresolved"),
        }
    }
}

impl std::error::Error for OperationStatusError {}

#[cfg(test)]
mod tests {
    use super::{
        OperationBeginOutcome, OperationStatus, OperationStatusError, OperationStatusJournal,
    };
    use crate::errors::CommitReceipt;
    use crate::ids::{DomainId, OperationId, Revision};

    fn id(tail: u8) -> Result<OperationId, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        OperationId::try_from_bytes(bytes)
    }

    #[test]
    fn same_payload_returns_committed_receipt_without_reexecution() -> Result<(), String> {
        let operation_id = id(1).map_err(|e| e.to_string())?;
        let mut journal = OperationStatusJournal::new();
        let attempt = match journal
            .begin(operation_id, b"canonical request")
            .map_err(|e| e.to_string())?
        {
            OperationBeginOutcome::Execute(attempt) => attempt,
            OperationBeginOutcome::Existing(_) => {
                return Err("new operation was not reserved".into());
            }
        };
        let receipt = CommitReceipt::new(operation_id, Revision::FIRST_COMMIT);
        journal
            .mark_committed(attempt, receipt.clone())
            .map_err(|e| e.to_string())?;
        assert_eq!(
            journal.status(operation_id),
            OperationStatus::Committed(receipt.clone())
        );
        assert_eq!(
            journal
                .begin(operation_id, b"canonical request")
                .map_err(|e| e.to_string())?,
            OperationBeginOutcome::Existing(OperationStatus::Committed(receipt))
        );
        Ok(())
    }

    #[test]
    fn changed_payload_with_same_id_is_rejected() -> Result<(), String> {
        let operation_id = id(2).map_err(|e| e.to_string())?;
        let mut journal = OperationStatusJournal::new();
        let _attempt = journal
            .begin(operation_id, b"first payload")
            .map_err(|e| e.to_string())?;
        assert_eq!(
            journal.begin(operation_id, b"different payload"),
            Err(OperationStatusError::IdempotencyMismatch)
        );
        Ok(())
    }

    #[test]
    fn absent_operation_is_proven_not_committed_in_this_journal_view() -> Result<(), String> {
        let operation_id = id(5).map_err(|e| e.to_string())?;
        let journal = OperationStatusJournal::new();

        assert_eq!(journal.status(operation_id), OperationStatus::NotCommitted);
        Ok(())
    }

    #[test]
    fn unresolved_attempt_is_not_executed_twice_until_reconciled() -> Result<(), String> {
        let operation_id = id(3).map_err(|e| e.to_string())?;
        let mut journal = OperationStatusJournal::new();
        let _attempt = journal
            .begin(operation_id, b"same payload")
            .map_err(|e| e.to_string())?;
        assert_eq!(journal.status(operation_id), OperationStatus::Indeterminate);
        assert_eq!(
            journal
                .begin(operation_id, b"same payload")
                .map_err(|e| e.to_string())?,
            OperationBeginOutcome::Existing(OperationStatus::Indeterminate)
        );
        Ok(())
    }

    #[test]
    fn proven_not_committed_allows_same_payload_retry() -> Result<(), String> {
        let operation_id = id(4).map_err(|e| e.to_string())?;
        let mut journal = OperationStatusJournal::new();
        let attempt = match journal
            .begin(operation_id, b"same payload")
            .map_err(|e| e.to_string())?
        {
            OperationBeginOutcome::Execute(attempt) => attempt,
            OperationBeginOutcome::Existing(_) => {
                return Err("new operation was not reserved".into());
            }
        };
        journal
            .mark_not_committed(attempt)
            .map_err(|e| e.to_string())?;
        assert_eq!(journal.status(operation_id), OperationStatus::NotCommitted);
        assert!(matches!(
            journal
                .begin(operation_id, b"same payload")
                .map_err(|e| e.to_string())?,
            OperationBeginOutcome::Execute(_)
        ));
        Ok(())
    }
}
