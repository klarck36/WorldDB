//! Required audit records bound to the same WAL prepare as a domain action.

use std::fmt;

use worlddb_core::{
    AuditCommitContext, AuditOutcome, AuditRecord, AuditSequence, OperationId, Revision,
    decode_audit_record, encode_audit_record,
};

use crate::wal::{WalCommittedFrame, WalError, WalPrepareLog};
use crate::writer_lock::WriterLock;

const REQUIRED_AUDIT_MAGIC: [u8; 8] = *b"WDBAUD\0\x01";
const HEADER_BYTES: usize = REQUIRED_AUDIT_MAGIC.len() + 8;

/// A required audit entry that has reached the same verified commit marker as its action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedRequiredAuditRecord {
    revision: Revision,
    operation_id: OperationId,
    record: AuditRecord,
}

impl CommittedRequiredAuditRecord {
    /// Shared database revision covered by the WAL commit marker.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Operation identity covered by the WAL commit marker.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Safe audit facts committed with the action.
    #[must_use]
    pub const fn record(&self) -> &AuditRecord {
        &self.record
    }
}

/// Failure to commit, validate, or read a required audit envelope.
#[derive(Debug)]
pub enum RequiredAuditError {
    /// The WAL could not prove a complete commit prefix or publish the operation.
    Wal(WalError),
    /// The encoded audit record failed its canonical wire contract.
    AuditCodec(worlddb_core::AuditCodecError),
    /// A required action must carry a non-empty canonical payload.
    EmptyActionPayload,
    /// The audit record did not describe a successful commit at this exact revision/OperationId.
    InvalidCommitBinding,
    /// The envelope length, allocation, or WAL payload size exceeded a supported limit.
    PayloadTooLarge,
    /// An existing committed audit sequence is not lower than the next sequence.
    AuditSequenceNotIncreasing {
        /// Previous required sequence in the verified WAL prefix.
        previous: AuditSequence,
        /// Repeated or regressed sequence from the candidate record.
        actual: AuditSequence,
    },
    /// A WAL payload starts with the audit-envelope marker but is malformed.
    MalformedEnvelope,
}

impl fmt::Display for RequiredAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wal(error) => write!(formatter, "required-audit WAL operation failed: {error}"),
            Self::AuditCodec(error) => {
                write!(formatter, "required audit record is invalid: {error}")
            }
            Self::EmptyActionPayload => {
                formatter.write_str("required-audit action payload is empty")
            }
            Self::InvalidCommitBinding => formatter.write_str(
                "required audit record must describe this successful WAL revision and OperationId",
            ),
            Self::PayloadTooLarge => {
                formatter.write_str("required-audit WAL payload exceeds its limit")
            }
            Self::AuditSequenceNotIncreasing { previous, actual } => write!(
                formatter,
                "required audit sequence {actual} does not follow prior sequence {previous}"
            ),
            Self::MalformedEnvelope => {
                formatter.write_str("required-audit WAL envelope is malformed")
            }
        }
    }
}

impl std::error::Error for RequiredAuditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wal(error) => Some(error),
            Self::AuditCodec(error) => Some(error),
            Self::EmptyActionPayload
            | Self::InvalidCommitBinding
            | Self::PayloadTooLarge
            | Self::AuditSequenceNotIncreasing { .. }
            | Self::MalformedEnvelope => None,
        }
    }
}

/// Borrowed action bytes and the decoded Required Audit Record from one WAL prepare.
pub(crate) struct DecodedRequiredAuditPayload<'a> {
    pub(crate) action_payload: &'a [u8],
    pub(crate) record: AuditRecord,
}

/// Encodes action bytes and safe audit facts into one canonical WAL prepare payload.
pub(crate) fn encode_required_audit_payload(
    action_payload: &[u8],
    record: &AuditRecord,
) -> Result<Vec<u8>, RequiredAuditError> {
    if action_payload.is_empty() {
        return Err(RequiredAuditError::EmptyActionPayload);
    }
    let audit_bytes = encode_audit_record(record).map_err(RequiredAuditError::AuditCodec)?;
    let action_length =
        u32::try_from(action_payload.len()).map_err(|_| RequiredAuditError::PayloadTooLarge)?;
    let audit_length =
        u32::try_from(audit_bytes.len()).map_err(|_| RequiredAuditError::PayloadTooLarge)?;
    let capacity = HEADER_BYTES
        .checked_add(action_payload.len())
        .and_then(|value| value.checked_add(audit_bytes.len()))
        .ok_or(RequiredAuditError::PayloadTooLarge)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| RequiredAuditError::PayloadTooLarge)?;
    bytes.extend_from_slice(&REQUIRED_AUDIT_MAGIC);
    bytes.extend_from_slice(&action_length.to_le_bytes());
    bytes.extend_from_slice(&audit_length.to_le_bytes());
    bytes.extend_from_slice(action_payload);
    bytes.extend_from_slice(&audit_bytes);
    Ok(bytes)
}

/// Decodes and checks the commit binding of an audited operation payload.
pub(crate) fn decode_required_audit_payload(
    payload: &[u8],
    revision: Revision,
    operation_id: OperationId,
) -> Result<Option<DecodedRequiredAuditPayload<'_>>, RequiredAuditError> {
    if payload.get(..REQUIRED_AUDIT_MAGIC.len()) != Some(REQUIRED_AUDIT_MAGIC.as_slice()) {
        return Ok(None);
    }
    if payload.len() < HEADER_BYTES {
        return Err(RequiredAuditError::MalformedEnvelope);
    }
    let action_length = u32::from_le_bytes(
        payload
            .get(REQUIRED_AUDIT_MAGIC.len()..REQUIRED_AUDIT_MAGIC.len() + 4)
            .ok_or(RequiredAuditError::MalformedEnvelope)?
            .try_into()
            .map_err(|_| RequiredAuditError::MalformedEnvelope)?,
    ) as usize;
    let audit_length = u32::from_le_bytes(
        payload
            .get(REQUIRED_AUDIT_MAGIC.len() + 4..HEADER_BYTES)
            .ok_or(RequiredAuditError::MalformedEnvelope)?
            .try_into()
            .map_err(|_| RequiredAuditError::MalformedEnvelope)?,
    ) as usize;
    if action_length == 0 {
        return Err(RequiredAuditError::EmptyActionPayload);
    }
    let action_start = HEADER_BYTES;
    let audit_start = action_start
        .checked_add(action_length)
        .ok_or(RequiredAuditError::MalformedEnvelope)?;
    let end = audit_start
        .checked_add(audit_length)
        .ok_or(RequiredAuditError::MalformedEnvelope)?;
    if end != payload.len() || audit_length == 0 {
        return Err(RequiredAuditError::MalformedEnvelope);
    }
    let audit_bytes = payload
        .get(audit_start..end)
        .ok_or(RequiredAuditError::MalformedEnvelope)?;
    let record = decode_audit_record(audit_bytes).map_err(RequiredAuditError::AuditCodec)?;
    validate_commit_binding(&record, revision, operation_id)?;
    Ok(Some(DecodedRequiredAuditPayload {
        action_payload: payload
            .get(action_start..audit_start)
            .ok_or(RequiredAuditError::MalformedEnvelope)?,
        record,
    }))
}

pub(crate) fn validate_commit_binding(
    record: &AuditRecord,
    revision: Revision,
    operation_id: OperationId,
) -> Result<(), RequiredAuditError> {
    if record.outcome() != AuditOutcome::Succeeded
        || record.commit_context()
            != (AuditCommitContext::Committed {
                revision,
                operation_id,
            })
    {
        return Err(RequiredAuditError::InvalidCommitBinding);
    }
    Ok(())
}

pub(crate) fn collect_committed_required_audits(
    frames: &[WalCommittedFrame],
) -> Result<Vec<CommittedRequiredAuditRecord>, RequiredAuditError> {
    let mut records = Vec::new();
    let mut previous_sequence = None;
    for frame in frames {
        let receipt = frame.receipt();
        let operation_id = frame.prepare().reference().operation_id();
        let Some(decoded) = decode_required_audit_payload(
            frame.prepare().payload(),
            receipt.revision(),
            operation_id,
        )?
        else {
            continue;
        };
        if let Some(previous) = previous_sequence {
            if decoded.record.sequence() <= previous {
                return Err(RequiredAuditError::AuditSequenceNotIncreasing {
                    previous,
                    actual: decoded.record.sequence(),
                });
            }
        }
        previous_sequence = Some(decoded.record.sequence());
        records
            .try_reserve(1)
            .map_err(|_| RequiredAuditError::PayloadTooLarge)?;
        records.push(CommittedRequiredAuditRecord {
            revision: receipt.revision(),
            operation_id,
            record: decoded.record,
        });
    }
    Ok(records)
}

pub(crate) fn validate_sequence_after(
    record: &AuditRecord,
    previous: Option<AuditSequence>,
) -> Result<(), RequiredAuditError> {
    if let Some(previous) = previous {
        if record.sequence() <= previous {
            return Err(RequiredAuditError::AuditSequenceNotIncreasing {
                previous,
                actual: record.sequence(),
            });
        }
    }
    Ok(())
}

pub(crate) fn latest_sequence(records: &[CommittedRequiredAuditRecord]) -> Option<AuditSequence> {
    records.last().map(|record| record.record.sequence())
}

/// Parses every required audit record in a verified committed WAL prefix.
pub(crate) fn records_for_log(
    log: &WalPrepareLog,
    lock: &WriterLock,
) -> Result<Vec<CommittedRequiredAuditRecord>, RequiredAuditError> {
    let prefix = log
        .scan_recovery_prefix(lock)
        .map_err(RequiredAuditError::Wal)?;
    if prefix.finding.is_some() {
        return Err(RequiredAuditError::Wal(WalError::RecoveryRequired));
    }
    collect_committed_required_audits(&prefix.committed_frames)
}
