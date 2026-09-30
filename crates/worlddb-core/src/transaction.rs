//! Typed transaction metadata and runtime state values.

use crate::ids::{OperationId, TransactionId};

/// Runtime state of one write transaction.
///
/// UnknownOutcome describes the client's knowledge after losing a response; it
/// is not a server-side commit result.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TransactionState {
    /// The caller may add operations, register reads, or explicitly abort.
    Open,
    /// Validation against the base snapshot completed; commit revalidation remains.
    Validated,
    /// The writer owns the operation and is approaching or passing the commitpoint.
    Committing,
    /// The durable commitpoint was reached.
    Committed,
    /// The transaction ended without publishing a commit.
    Aborted,
    /// The client lost the response and must resolve it through OperationId.
    UnknownOutcome,
}

/// The distinct identities for one concrete attempt and its logical commit intent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransactionIdentity {
    transaction_id: TransactionId,
    operation_id: OperationId,
}

impl TransactionIdentity {
    /// Associates a concrete transaction attempt with a logical operation.
    #[must_use]
    pub const fn new(transaction_id: TransactionId, operation_id: OperationId) -> Self {
        Self {
            transaction_id,
            operation_id,
        }
    }

    /// Returns the identity of this concrete execution attempt.
    #[must_use]
    pub const fn transaction_id(self) -> TransactionId {
        self.transaction_id
    }

    /// Returns the idempotency identity that remains stable across retries.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }
}

/// A value snapshot of transaction identifiers and state.
///
/// This copyable DTO is diagnostic metadata, not a transaction handle and does
/// not own a writer, staging area, snapshot lease, or commit capability.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransactionDescriptor {
    identity: TransactionIdentity,
    state: TransactionState,
}

impl TransactionDescriptor {
    /// Creates a diagnostic descriptor from typed identity and runtime state.
    #[must_use]
    pub const fn new(identity: TransactionIdentity, state: TransactionState) -> Self {
        Self { identity, state }
    }

    /// Returns the transaction and logical operation identities.
    #[must_use]
    pub const fn identity(self) -> TransactionIdentity {
        self.identity
    }

    /// Returns the observed transaction state.
    #[must_use]
    pub const fn state(self) -> TransactionState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::{TransactionDescriptor, TransactionIdentity, TransactionState};
    use crate::ids::{DomainId, IdValidationError, OperationId, TransactionId};

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    #[test]
    fn transaction_attempt_and_logical_operation_remain_distinct() -> Result<(), IdValidationError>
    {
        let transaction_id = uuid::<TransactionId>(1)?;
        let operation_id = uuid::<OperationId>(2)?;
        let identity = TransactionIdentity::new(transaction_id, operation_id);
        let descriptor = TransactionDescriptor::new(identity, TransactionState::Open);
        assert_eq!(descriptor.identity().transaction_id(), transaction_id);
        assert_eq!(descriptor.identity().operation_id(), operation_id);
        assert_eq!(descriptor.state(), TransactionState::Open);
        Ok(())
    }

    #[test]
    fn transaction_states_are_explicit_runtime_values() {
        let states = [
            TransactionState::Open,
            TransactionState::Validated,
            TransactionState::Committing,
            TransactionState::Committed,
            TransactionState::Aborted,
            TransactionState::UnknownOutcome,
        ];
        assert_eq!(states.len(), 6);
        assert_ne!(TransactionState::UnknownOutcome, TransactionState::Aborted);
        assert_ne!(
            TransactionState::UnknownOutcome,
            TransactionState::Committed
        );
    }
}
