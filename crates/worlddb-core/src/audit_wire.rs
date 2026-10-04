//! Separate canonical frames for the append-only audit subsystem.

use std::fmt;

use crate::Bytes;
use crate::audit::{
    AuditAction, AuditCommitContext, AuditFingerprintError, AuditObjectClass, AuditOutcome,
    AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
    AuditScopeFingerprint, AuditSequence, PageOrdinal, RawReadAttempt, RawReadAttemptIdentity,
    RawReadAttemptScope,
};
use crate::ids::{Revision, SecurityEpoch};
use crate::numbers::{decode_u128_varint_prefix, encode_u128_varint};
#[cfg(test)]
use crate::wire::decode_frame;
use crate::wire::{
    DecodeResource, DecoderLimits, FrameError, FrameHeader, TlvDecoder, TlvEncoder, WireError,
    decode_frame_with_limits, decode_id, encode_frame, encode_id,
};

/// Distinct frame kinds for audit records; these are not WorldDB `RecordKind`s.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AuditRecordKind {
    /// Safe audit facts associated with an ordinary data commit or rejected action.
    AuditRecord = 0xa001,
    /// Durable page-attempt facts for a raw administrative read.
    RawReadAttempt = 0xa002,
}

impl AuditRecordKind {
    /// Returns the stable audit frame-kind number.
    #[must_use]
    pub const fn number(self) -> u32 {
        self as u32
    }
}

/// A malformed or noncanonical audit frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditCodecError {
    /// The shared frame header, version, or checksum is invalid.
    Frame(FrameError),
    /// A TLV, scalar, or typed identity is malformed.
    Wire(WireError),
    /// Audit frames have no optional capability flags in version 1.0.
    OptionalFlagsNotSupported { flags: u64 },
    /// The frame kind is not part of the separate audit stream.
    UnknownAuditKind { kind: u32 },
    /// The closed audit record contains an unknown field.
    UnknownField { kind: u32, field: u32 },
    /// A required audit field is absent.
    MissingField { kind: u32, field: u32 },
    /// A field is not a valid value for its declared audit type.
    InvalidFieldValue { kind: u32, field: u32 },
    /// A valid payload differs from its unique canonical encoding.
    NonCanonicalRecord,
}

impl fmt::Display for AuditCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => write!(formatter, "audit frame: {error}"),
            Self::Wire(error) => write!(formatter, "audit wire value: {error}"),
            Self::OptionalFlagsNotSupported { flags } => {
                write!(
                    formatter,
                    "audit frame has unsupported optional flags 0x{flags:016x}"
                )
            }
            Self::UnknownAuditKind { kind } => {
                write!(formatter, "unknown audit frame kind 0x{kind:08x}")
            }
            Self::UnknownField { kind, field } => write!(
                formatter,
                "audit frame kind 0x{kind:08x} has unknown field {field}"
            ),
            Self::MissingField { kind, field } => write!(
                formatter,
                "audit frame kind 0x{kind:08x} is missing required field {field}"
            ),
            Self::InvalidFieldValue { kind, field } => write!(
                formatter,
                "audit frame kind 0x{kind:08x} has an invalid value in field {field}"
            ),
            Self::NonCanonicalRecord => {
                formatter.write_str("audit record is valid but not canonically encoded")
            }
        }
    }
}

impl std::error::Error for AuditCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::OptionalFlagsNotSupported { .. }
            | Self::UnknownAuditKind { .. }
            | Self::UnknownField { .. }
            | Self::MissingField { .. }
            | Self::InvalidFieldValue { .. }
            | Self::NonCanonicalRecord => None,
        }
    }
}

fn encode_fields(values: Vec<(u32, Vec<u8>)>) -> Result<Vec<u8>, AuditCodecError> {
    let mut encoder = TlvEncoder::new();
    for (field, value) in values {
        encoder.push(field, &value).map_err(AuditCodecError::Wire)?;
    }
    Ok(encoder.finish())
}

#[cfg(test)]
fn decode_fields<'a>(
    kind: AuditRecordKind,
    payload: &'a [u8],
    allowed: &[u32],
) -> Result<Vec<(u32, &'a [u8])>, AuditCodecError> {
    decode_fields_with_limits(kind, payload, allowed, &DecoderLimits::process_default())
}

fn decode_fields_with_limits<'a>(
    kind: AuditRecordKind,
    payload: &'a [u8],
    allowed: &[u32],
    limits: &DecoderLimits,
) -> Result<Vec<(u32, &'a [u8])>, AuditCodecError> {
    let mut decoder = TlvDecoder::with_limits(payload, *limits);
    let mut fields = Vec::new();
    while let Some(field) = decoder.next_field().map_err(AuditCodecError::Wire)? {
        if !allowed.contains(&field.tag()) {
            return Err(AuditCodecError::UnknownField {
                kind: kind.number(),
                field: field.tag(),
            });
        }
        fields.try_reserve(1).map_err(|_| {
            AuditCodecError::Wire(WireError::AllocationFailed {
                resource: DecodeResource::FieldsPerRecord,
            })
        })?;
        fields.push((field.tag(), field.value()));
    }
    Ok(fields)
}

fn required_field<'a>(
    kind: AuditRecordKind,
    fields: &[(u32, &'a [u8])],
    tag: u32,
) -> Result<&'a [u8], AuditCodecError> {
    fields
        .iter()
        .find_map(|(actual, value)| (*actual == tag).then_some(*value))
        .ok_or(AuditCodecError::MissingField {
            kind: kind.number(),
            field: tag,
        })
}

fn invalid(kind: AuditRecordKind, field: u32) -> AuditCodecError {
    AuditCodecError::InvalidFieldValue {
        kind: kind.number(),
        field,
    }
}

fn encode_u64(value: u64) -> Vec<u8> {
    encode_u128_varint(u128::from(value))
}

fn decode_u64(kind: AuditRecordKind, field: u32, bytes: &[u8]) -> Result<u64, AuditCodecError> {
    let (value, length) = decode_u128_varint_prefix(bytes)
        .map_err(|error| AuditCodecError::Wire(WireError::Varint { offset: 0, error }))?;
    if length != bytes.len() {
        return Err(invalid(kind, field));
    }
    u64::try_from(value).map_err(|_| invalid(kind, field))
}

fn encode_commit_context(value: AuditCommitContext) -> Result<Vec<u8>, AuditCodecError> {
    match value {
        AuditCommitContext::NotCommitted => Ok(vec![1]),
        AuditCommitContext::Committed {
            revision,
            operation_id,
        } => {
            let fields = encode_fields(vec![
                (1, encode_u64(revision.value())),
                (2, encode_id(operation_id).to_vec()),
            ])?;
            let mut bytes = vec![2];
            bytes.extend(fields);
            Ok(bytes)
        }
    }
}

fn decode_commit_context(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<AuditCommitContext, AuditCodecError> {
    let kind = AuditRecordKind::AuditRecord;
    match bytes {
        [1] => Ok(AuditCommitContext::NotCommitted),
        [2, payload @ ..] => {
            let fields = decode_nested_commit_fields(payload, limits)?;
            let revision =
                Revision::try_from(decode_u64(kind, 8, required_field(kind, &fields, 1)?)?)
                    .map_err(|_| invalid(kind, 8))?;
            let operation_id =
                decode_id(required_field(kind, &fields, 2)?).map_err(AuditCodecError::Wire)?;
            Ok(AuditCommitContext::Committed {
                revision,
                operation_id,
            })
        }
        _ => Err(invalid(kind, 8)),
    }
}

fn decode_nested_commit_fields<'a>(
    payload: &'a [u8],
    limits: &DecoderLimits,
) -> Result<Vec<(u32, &'a [u8])>, AuditCodecError> {
    let kind = AuditRecordKind::AuditRecord;
    let mut decoder = TlvDecoder::with_limits(payload, *limits);
    let mut fields = Vec::new();
    while let Some(field) = decoder.next_field().map_err(AuditCodecError::Wire)? {
        if ![1, 2].contains(&field.tag()) {
            return Err(invalid(kind, 8));
        }
        fields.try_reserve(1).map_err(|_| {
            AuditCodecError::Wire(WireError::AllocationFailed {
                resource: DecodeResource::FieldsPerRecord,
            })
        })?;
        fields.push((field.tag(), field.value()));
    }
    Ok(fields)
}

fn encode_action(value: AuditAction) -> u8 {
    match value {
        AuditAction::SecurityPolicyChange => 1,
        AuditAction::RawReadAttempt => 2,
        AuditAction::ExportAuthorization => 3,
        AuditAction::ExportCompletion => 4,
        AuditAction::Migration => 5,
        AuditAction::Backup => 6,
        AuditAction::RestorePublication => 7,
        AuditAction::PurgePublication => 8,
        AuditAction::AuditConfigurationChange => 9,
        AuditAction::SchemaManagement => 10,
        AuditAction::EntityCreation => 11,
        AuditAction::EntityRetirement => 12,
        AuditAction::HistorySpaceTransfer => 13,
        AuditAction::FactualRecordWrite => 14,
    }
}

fn decode_action(bytes: &[u8]) -> Result<AuditAction, AuditCodecError> {
    match bytes {
        [1] => Ok(AuditAction::SecurityPolicyChange),
        [2] => Ok(AuditAction::RawReadAttempt),
        [3] => Ok(AuditAction::ExportAuthorization),
        [4] => Ok(AuditAction::ExportCompletion),
        [5] => Ok(AuditAction::Migration),
        [6] => Ok(AuditAction::Backup),
        [7] => Ok(AuditAction::RestorePublication),
        [8] => Ok(AuditAction::PurgePublication),
        [9] => Ok(AuditAction::AuditConfigurationChange),
        [10] => Ok(AuditAction::SchemaManagement),
        [11] => Ok(AuditAction::EntityCreation),
        [12] => Ok(AuditAction::EntityRetirement),
        [13] => Ok(AuditAction::HistorySpaceTransfer),
        [14] => Ok(AuditAction::FactualRecordWrite),
        _ => Err(invalid(AuditRecordKind::AuditRecord, 5)),
    }
}

fn encode_object_class(value: AuditObjectClass) -> u8 {
    match value {
        AuditObjectClass::Database => 1,
        AuditObjectClass::SecurityPolicy => 2,
        AuditObjectClass::Migration => 3,
        AuditObjectClass::Job => 4,
        AuditObjectClass::Backup => 5,
        AuditObjectClass::Export => 6,
        AuditObjectClass::RawReadScope => 7,
        AuditObjectClass::AuditConfiguration => 8,
        AuditObjectClass::SchemaDefinition => 9,
        AuditObjectClass::EntityCatalog => 10,
        AuditObjectClass::FactualRecord => 11,
    }
}

fn decode_object_class(bytes: &[u8]) -> Result<AuditObjectClass, AuditCodecError> {
    match bytes {
        [1] => Ok(AuditObjectClass::Database),
        [2] => Ok(AuditObjectClass::SecurityPolicy),
        [3] => Ok(AuditObjectClass::Migration),
        [4] => Ok(AuditObjectClass::Job),
        [5] => Ok(AuditObjectClass::Backup),
        [6] => Ok(AuditObjectClass::Export),
        [7] => Ok(AuditObjectClass::RawReadScope),
        [8] => Ok(AuditObjectClass::AuditConfiguration),
        [9] => Ok(AuditObjectClass::SchemaDefinition),
        [10] => Ok(AuditObjectClass::EntityCatalog),
        [11] => Ok(AuditObjectClass::FactualRecord),
        _ => Err(invalid(AuditRecordKind::AuditRecord, 6)),
    }
}

fn encode_outcome(value: AuditOutcome) -> u8 {
    match value {
        AuditOutcome::Succeeded => 1,
        AuditOutcome::Denied => 2,
        AuditOutcome::Failed => 3,
        AuditOutcome::Indeterminate => 4,
    }
}

fn decode_outcome(bytes: &[u8]) -> Result<AuditOutcome, AuditCodecError> {
    match bytes {
        [1] => Ok(AuditOutcome::Succeeded),
        [2] => Ok(AuditOutcome::Denied),
        [3] => Ok(AuditOutcome::Failed),
        [4] => Ok(AuditOutcome::Indeterminate),
        _ => Err(invalid(AuditRecordKind::AuditRecord, 7)),
    }
}

fn encode_frame_for(kind: AuditRecordKind, payload: &[u8]) -> Result<Vec<u8>, AuditCodecError> {
    encode_frame(FrameHeader::new(kind.number()), payload).map_err(AuditCodecError::Frame)
}

/// Encodes a safe audit record in its separate audit frame namespace.
pub fn encode_audit_record(value: &AuditRecord) -> Result<Vec<u8>, AuditCodecError> {
    let payload = encode_fields(vec![
        (1, encode_id(value.record_id()).to_vec()),
        (2, encode_u64(value.sequence().value())),
        (3, encode_id(value.audit_operation_id()).to_vec()),
        (4, encode_id(value.actor()).to_vec()),
        (5, vec![encode_action(value.action())]),
        (6, vec![encode_object_class(value.object_class())]),
        (7, vec![encode_outcome(value.outcome())]),
        (8, encode_commit_context(value.commit_context())?),
        (9, encode_u64(value.security_epoch().value())),
        (10, value.policy_fingerprint().as_bytes().to_vec()),
    ])?;
    encode_frame_for(AuditRecordKind::AuditRecord, &payload)
}

/// Decodes exactly one canonical safe audit record frame.
pub fn decode_audit_record(bytes: &[u8]) -> Result<AuditRecord, AuditCodecError> {
    decode_audit_record_with_limits(bytes, &DecoderLimits::process_default())
}

/// Decodes one canonical safe audit record under an explicit resource policy.
pub fn decode_audit_record_with_limits(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<AuditRecord, AuditCodecError> {
    limits
        .check_nesting_depth(3)
        .map_err(AuditCodecError::Wire)?;
    let frame = decode_frame_with_limits(bytes, limits).map_err(AuditCodecError::Frame)?;
    let kind = AuditRecordKind::AuditRecord;
    if frame.header().kind() != kind.number() {
        return Err(AuditCodecError::UnknownAuditKind {
            kind: frame.header().kind(),
        });
    }
    if frame.header().optional_flags() != 0 {
        return Err(AuditCodecError::OptionalFlagsNotSupported {
            flags: frame.header().optional_flags(),
        });
    }
    let fields = decode_fields_with_limits(
        kind,
        frame.payload(),
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        limits,
    )?;
    let record_id = decode_id(required_field(kind, &fields, 1)?).map_err(AuditCodecError::Wire)?;
    let sequence = AuditSequence::new(decode_u64(kind, 2, required_field(kind, &fields, 2)?)?);
    let audit_operation_id =
        decode_id(required_field(kind, &fields, 3)?).map_err(AuditCodecError::Wire)?;
    let actor = decode_id(required_field(kind, &fields, 4)?).map_err(AuditCodecError::Wire)?;
    let action = decode_action(required_field(kind, &fields, 5)?)?;
    let object_class = decode_object_class(required_field(kind, &fields, 6)?)?;
    let outcome = decode_outcome(required_field(kind, &fields, 7)?)?;
    let commit_context = decode_commit_context(required_field(kind, &fields, 8)?, limits)?;
    let security_epoch =
        SecurityEpoch::new(decode_u64(kind, 9, required_field(kind, &fields, 9)?)?);
    let policy_bytes = required_field(kind, &fields, 10)?;
    if policy_bytes.len() > limits.max_string_or_bytes {
        return Err(AuditCodecError::Wire(WireError::ResourceLimitExceeded {
            resource: DecodeResource::StringOrBytes,
            limit: limits.max_string_or_bytes,
            actual: policy_bytes.len(),
        }));
    }
    let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(policy_bytes.to_vec()))
        .map_err(|_: AuditFingerprintError| invalid(kind, 10))?;
    let record = AuditRecord::new(
        AuditRecordIdentity {
            record_id,
            sequence,
            audit_operation_id,
        },
        AuditRecordDetails {
            actor,
            action,
            object_class,
            outcome,
            commit_context,
            security_epoch,
            policy_fingerprint,
        },
    );
    if encode_audit_record(&record)? != bytes {
        return Err(AuditCodecError::NonCanonicalRecord);
    }
    Ok(record)
}

/// Encodes one raw-read page attempt without any read payload or WorldDB revision.
pub fn encode_raw_read_attempt(value: &RawReadAttempt) -> Result<Vec<u8>, AuditCodecError> {
    let payload = encode_fields(vec![
        (1, encode_id(value.record_id()).to_vec()),
        (2, encode_u64(value.sequence().value())),
        (3, encode_id(value.audit_operation_id()).to_vec()),
        (4, encode_id(value.client_request_id()).to_vec()),
        (5, encode_id(value.principal_id()).to_vec()),
        (6, value.scope_fingerprint().as_bytes().to_vec()),
        (7, encode_id(value.snapshot_id()).to_vec()),
        (8, encode_u64(value.security_epoch().value())),
        (9, encode_u64(value.page_ordinal().value())),
    ])?;
    encode_frame_for(AuditRecordKind::RawReadAttempt, &payload)
}

/// Decodes exactly one canonical raw-read-attempt frame.
pub fn decode_raw_read_attempt(bytes: &[u8]) -> Result<RawReadAttempt, AuditCodecError> {
    decode_raw_read_attempt_with_limits(bytes, &DecoderLimits::process_default())
}

/// Decodes one canonical raw-read attempt under an explicit resource policy.
pub fn decode_raw_read_attempt_with_limits(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<RawReadAttempt, AuditCodecError> {
    limits
        .check_nesting_depth(2)
        .map_err(AuditCodecError::Wire)?;
    let frame = decode_frame_with_limits(bytes, limits).map_err(AuditCodecError::Frame)?;
    let kind = AuditRecordKind::RawReadAttempt;
    if frame.header().kind() != kind.number() {
        return Err(AuditCodecError::UnknownAuditKind {
            kind: frame.header().kind(),
        });
    }
    if frame.header().optional_flags() != 0 {
        return Err(AuditCodecError::OptionalFlagsNotSupported {
            flags: frame.header().optional_flags(),
        });
    }
    let fields =
        decode_fields_with_limits(kind, frame.payload(), &[1, 2, 3, 4, 5, 6, 7, 8, 9], limits)?;
    let record_id = decode_id(required_field(kind, &fields, 1)?).map_err(AuditCodecError::Wire)?;
    let sequence = AuditSequence::new(decode_u64(kind, 2, required_field(kind, &fields, 2)?)?);
    let audit_operation_id =
        decode_id(required_field(kind, &fields, 3)?).map_err(AuditCodecError::Wire)?;
    let client_request_id =
        decode_id(required_field(kind, &fields, 4)?).map_err(AuditCodecError::Wire)?;
    let principal_id =
        decode_id(required_field(kind, &fields, 5)?).map_err(AuditCodecError::Wire)?;
    let scope_bytes = required_field(kind, &fields, 6)?;
    if scope_bytes.len() > limits.max_string_or_bytes {
        return Err(AuditCodecError::Wire(WireError::ResourceLimitExceeded {
            resource: DecodeResource::StringOrBytes,
            limit: limits.max_string_or_bytes,
            actual: scope_bytes.len(),
        }));
    }
    let scope_fingerprint = AuditScopeFingerprint::new(Bytes::new(scope_bytes.to_vec()))
        .map_err(|_: AuditFingerprintError| invalid(kind, 6))?;
    let snapshot_id =
        decode_id(required_field(kind, &fields, 7)?).map_err(AuditCodecError::Wire)?;
    let security_epoch =
        SecurityEpoch::new(decode_u64(kind, 8, required_field(kind, &fields, 8)?)?);
    let page_ordinal = PageOrdinal::new(decode_u64(kind, 9, required_field(kind, &fields, 9)?)?);
    let attempt = RawReadAttempt::new(
        RawReadAttemptIdentity {
            record_id,
            sequence,
            audit_operation_id,
            client_request_id,
        },
        RawReadAttemptScope {
            principal_id,
            scope_fingerprint,
            snapshot_id,
            security_epoch,
            page_ordinal,
        },
    );
    if encode_raw_read_attempt(&attempt)? != bytes {
        return Err(AuditCodecError::NonCanonicalRecord);
    }
    Ok(attempt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DomainId, IdValidationError};

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum AuditFixture {
        Record(AuditRecord),
        RawRead(RawReadAttempt),
    }

    impl AuditFixture {
        fn encode(&self) -> Result<Vec<u8>, AuditCodecError> {
            match self {
                Self::Record(value) => encode_audit_record(value),
                Self::RawRead(value) => encode_raw_read_attempt(value),
            }
        }

        fn decode(bytes: &[u8]) -> Result<Self, AuditCodecError> {
            let frame = decode_frame(bytes).map_err(AuditCodecError::Frame)?;
            match frame.header().kind() {
                value if value == AuditRecordKind::AuditRecord.number() => {
                    decode_audit_record(bytes).map(Self::Record)
                }
                value if value == AuditRecordKind::RawReadAttempt.number() => {
                    decode_raw_read_attempt(bytes).map(Self::RawRead)
                }
                kind => Err(AuditCodecError::UnknownAuditKind { kind }),
            }
        }
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn fixtures() -> Option<Vec<(&'static str, AuditFixture)>> {
        let revision = Revision::new(9).ok()?;
        let committed = AuditRecord::new(
            AuditRecordIdentity {
                record_id: id(101).ok()?,
                sequence: AuditSequence::new(3),
                audit_operation_id: id(102).ok()?,
            },
            AuditRecordDetails {
                actor: id(103).ok()?,
                action: AuditAction::AuditConfigurationChange,
                object_class: AuditObjectClass::AuditConfiguration,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision,
                    operation_id: id(104).ok()?,
                },
                security_epoch: SecurityEpoch::new(4),
                policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(vec![0x11, 0x22, 0x33]))
                    .ok()?,
            },
        );
        let rejected = AuditRecord::new(
            AuditRecordIdentity {
                record_id: id(105).ok()?,
                sequence: AuditSequence::new(0),
                audit_operation_id: id(106).ok()?,
            },
            AuditRecordDetails {
                actor: id(107).ok()?,
                action: AuditAction::RawReadAttempt,
                object_class: AuditObjectClass::RawReadScope,
                outcome: AuditOutcome::Denied,
                commit_context: AuditCommitContext::NotCommitted,
                security_epoch: SecurityEpoch::new(5),
                policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(vec![0x44, 0x55]))
                    .ok()?,
            },
        );
        let raw_read = RawReadAttempt::new(
            RawReadAttemptIdentity {
                record_id: id(111).ok()?,
                sequence: AuditSequence::new(7),
                audit_operation_id: id(112).ok()?,
                client_request_id: id(113).ok()?,
            },
            RawReadAttemptScope {
                principal_id: id(114).ok()?,
                scope_fingerprint: AuditScopeFingerprint::new(Bytes::new(vec![0x66; 32])).ok()?,
                snapshot_id: id(115).ok()?,
                security_epoch: SecurityEpoch::new(6),
                page_ordinal: PageOrdinal::new(0),
            },
        );
        Some(vec![
            ("audit_record_committed", AuditFixture::Record(committed)),
            ("audit_record_uncommitted", AuditFixture::Record(rejected)),
            (
                "raw_read_attempt_page_zero",
                AuditFixture::RawRead(raw_read),
            ),
        ])
    }

    fn hex(bytes: &[u8]) -> String {
        let mut output = String::with_capacity(bytes.len().saturating_mul(2));
        for byte in bytes {
            output.push_str(&format!("{byte:02x}"));
        }
        output
    }

    fn unhex(value: &str) -> Option<Vec<u8>> {
        if value.len() % 2 != 0 {
            return None;
        }
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).ok()?;
                u8::from_str_radix(text, 16).ok()
            })
            .collect()
    }

    #[test]
    fn audit_frames_match_fixed_golden_vectors_and_round_trip() {
        let mut expected = std::collections::BTreeMap::new();
        for (line_number, line) in include_str!("../tests/data/audit-v1.0-golden.tsv")
            .lines()
            .enumerate()
        {
            if line_number == 0 {
                continue;
            }
            let columns = line.split('\t').collect::<Vec<_>>();
            assert_eq!(columns.len(), 3, "audit golden line {line_number}");
            if let [name, kind, frame] = columns.as_slice() {
                assert!(expected.insert(*name, (*kind, *frame)).is_none());
            }
        }
        let values = fixtures();
        assert!(values.is_some(), "audit fixtures must be valid");
        let Some(values) = values else {
            return;
        };
        assert_eq!(expected.len(), values.len());
        for (name, fixture) in values {
            let golden = expected.get(name);
            assert!(golden.is_some(), "missing audit golden vector {name}");
            let Some((raw_kind, raw_frame)) = golden else {
                continue;
            };
            let expected_kind = u32::from_str_radix(raw_kind, 16);
            assert!(expected_kind.is_ok(), "invalid audit kind for {name}");
            let expected_frame = unhex(raw_frame);
            assert!(expected_frame.is_some(), "invalid audit frame for {name}");
            let encoded = fixture.encode();
            assert!(encoded.is_ok(), "could not encode {name}: {encoded:?}");
            let (Some(expected_kind), Some(expected_frame), Ok(encoded)) =
                (expected_kind.ok(), expected_frame, encoded)
            else {
                continue;
            };
            assert_eq!(hex(&encoded), hex(&expected_frame), "audit golden {name}");
            let frame = decode_frame(&encoded);
            assert!(frame.is_ok(), "invalid audit frame {name}: {frame:?}");
            if let Ok(frame) = frame {
                assert_eq!(frame.header().kind(), expected_kind, "audit kind {name}");
                assert_eq!(frame.header().optional_flags(), 0, "audit flags {name}");
            }
            let decoded = AuditFixture::decode(&encoded);
            assert_eq!(decoded, Ok(fixture), "audit round trip {name}");
        }
    }

    #[test]
    fn audit_enums_have_closed_stable_codes() {
        for (value, code) in [
            (AuditAction::SecurityPolicyChange, 1),
            (AuditAction::RawReadAttempt, 2),
            (AuditAction::ExportAuthorization, 3),
            (AuditAction::ExportCompletion, 4),
            (AuditAction::Migration, 5),
            (AuditAction::Backup, 6),
            (AuditAction::RestorePublication, 7),
            (AuditAction::PurgePublication, 8),
            (AuditAction::AuditConfigurationChange, 9),
            (AuditAction::SchemaManagement, 10),
            (AuditAction::EntityCreation, 11),
            (AuditAction::EntityRetirement, 12),
            (AuditAction::HistorySpaceTransfer, 13),
            (AuditAction::FactualRecordWrite, 14),
        ] {
            assert_eq!(encode_action(value), code);
            assert_eq!(decode_action(&[code]), Ok(value));
        }
        assert!(decode_action(&[0]).is_err());
        assert!(decode_action(&[15]).is_err());

        for (value, code) in [
            (AuditObjectClass::Database, 1),
            (AuditObjectClass::SecurityPolicy, 2),
            (AuditObjectClass::Migration, 3),
            (AuditObjectClass::Job, 4),
            (AuditObjectClass::Backup, 5),
            (AuditObjectClass::Export, 6),
            (AuditObjectClass::RawReadScope, 7),
            (AuditObjectClass::AuditConfiguration, 8),
            (AuditObjectClass::SchemaDefinition, 9),
            (AuditObjectClass::EntityCatalog, 10),
            (AuditObjectClass::FactualRecord, 11),
        ] {
            assert_eq!(encode_object_class(value), code);
            assert_eq!(decode_object_class(&[code]), Ok(value));
        }
        assert!(decode_object_class(&[0]).is_err());
        assert!(decode_object_class(&[12]).is_err());

        for (value, code) in [
            (AuditOutcome::Succeeded, 1),
            (AuditOutcome::Denied, 2),
            (AuditOutcome::Failed, 3),
            (AuditOutcome::Indeterminate, 4),
        ] {
            assert_eq!(encode_outcome(value), code);
            assert_eq!(decode_outcome(&[code]), Ok(value));
        }
        assert!(decode_outcome(&[0]).is_err());
        assert!(decode_outcome(&[5]).is_err());
    }

    #[test]
    fn audit_frame_kinds_match_the_separate_closed_policy_registry() {
        let mut entries = std::collections::BTreeMap::new();
        for (line_number, line) in include_str!("../../../policy/audit-wire-kinds.tsv")
            .lines()
            .enumerate()
        {
            if line_number == 0 {
                continue;
            }
            let columns = line.split('\t').collect::<Vec<_>>();
            assert_eq!(columns.len(), 4, "audit wire registry line {line_number}");
            let [raw_kind, name, extensibility, fields] = columns.as_slice() else {
                continue;
            };
            assert_eq!(*extensibility, "closed", "audit kind {name}");
            let kind = u32::from_str_radix(raw_kind, 16);
            assert!(kind.is_ok(), "invalid audit kind {name}");
            if let Ok(kind) = kind {
                assert!(entries.insert(kind, (*name, *fields)).is_none());
            }
        }
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries
                .get(&AuditRecordKind::AuditRecord.number())
                .map(|entry| entry.0),
            Some("AuditRecord")
        );
        assert_eq!(
            entries
                .get(&AuditRecordKind::RawReadAttempt.number())
                .map(|entry| entry.0),
            Some("RawReadAttempt")
        );
        assert!(entries.contains_key(&0xa001));
        assert!(entries.contains_key(&0xa002));
    }

    #[test]
    fn audit_enum_codes_match_the_separate_policy_registry() {
        let expected = [
            ("action", 1, "SecurityPolicyChange"),
            ("action", 2, "RawReadAttempt"),
            ("action", 3, "ExportAuthorization"),
            ("action", 4, "ExportCompletion"),
            ("action", 5, "Migration"),
            ("action", 6, "Backup"),
            ("action", 7, "RestorePublication"),
            ("action", 8, "PurgePublication"),
            ("action", 9, "AuditConfigurationChange"),
            ("action", 10, "SchemaManagement"),
            ("action", 11, "EntityCreation"),
            ("action", 12, "EntityRetirement"),
            ("action", 13, "HistorySpaceTransfer"),
            ("action", 14, "FactualRecordWrite"),
            ("object_class", 1, "Database"),
            ("object_class", 2, "SecurityPolicy"),
            ("object_class", 3, "Migration"),
            ("object_class", 4, "Job"),
            ("object_class", 5, "Backup"),
            ("object_class", 6, "Export"),
            ("object_class", 7, "RawReadScope"),
            ("object_class", 8, "AuditConfiguration"),
            ("object_class", 9, "SchemaDefinition"),
            ("object_class", 10, "EntityCatalog"),
            ("object_class", 11, "FactualRecord"),
            ("outcome", 1, "Succeeded"),
            ("outcome", 2, "Denied"),
            ("outcome", 3, "Failed"),
            ("outcome", 4, "Indeterminate"),
            ("commit_context", 1, "NotCommitted"),
            ("commit_context", 2, "Committed"),
        ];
        let mut actual = Vec::new();
        for (line_number, line) in include_str!("../../../policy/audit-wire-values.tsv")
            .lines()
            .enumerate()
        {
            if line_number == 0 {
                continue;
            }
            let columns = line.split('\t').collect::<Vec<_>>();
            assert_eq!(columns.len(), 3, "audit value registry line {line_number}");
            let [family, raw_code, value] = columns.as_slice() else {
                continue;
            };
            let code = raw_code.parse::<u8>();
            assert!(code.is_ok(), "invalid audit value code for {value}");
            if let Ok(code) = code {
                actual.push((*family, code, *value));
            }
        }
        assert_eq!(actual.len(), expected.len());
        for row in expected {
            assert!(
                actual.contains(&row),
                "missing audit value registry row {row:?}"
            );
        }
    }

    #[test]
    fn audit_codecs_reject_unknown_missing_invalid_and_cross_namespace_fields() {
        let values = fixtures();
        assert!(values.is_some());
        let Some(values) = values else {
            return;
        };
        let record = values.iter().find_map(|(name, fixture)| {
            if *name == "audit_record_committed" {
                if let AuditFixture::Record(record) = fixture {
                    return Some(record);
                }
            }
            None
        });
        assert!(record.is_some(), "committed audit fixture is missing");
        let Some(record) = record else {
            return;
        };
        let encoded = encode_audit_record(record);
        assert!(encoded.is_ok());
        let Some(encoded) = encoded.ok() else {
            return;
        };
        let frame = decode_frame(&encoded);
        assert!(frame.is_ok());
        let Some(payload) = frame.ok().map(|frame| frame.payload()) else {
            return;
        };
        let fields = decode_fields(
            AuditRecordKind::AuditRecord,
            payload,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        );
        assert!(fields.is_ok());
        let Some(fields) = fields.ok() else {
            return;
        };
        let fields = fields
            .into_iter()
            .map(|(tag, value)| (tag, value.to_vec()))
            .collect::<Vec<_>>();

        let mut unknown = fields.clone();
        unknown.push((99, vec![1]));
        let payload = encode_fields(unknown);
        assert!(payload.is_ok());
        if let Ok(payload) = payload {
            let frame = encode_frame_for(AuditRecordKind::AuditRecord, &payload);
            assert!(matches!(
                frame.and_then(|frame| decode_audit_record(&frame)),
                Err(AuditCodecError::UnknownField {
                    kind: 0xa001,
                    field: 99
                })
            ));
        }

        let mut missing = fields.clone();
        missing.retain(|(tag, _)| *tag != 4);
        let payload = encode_fields(missing);
        assert!(payload.is_ok());
        if let Ok(payload) = payload {
            let frame = encode_frame_for(AuditRecordKind::AuditRecord, &payload);
            assert!(matches!(
                frame.and_then(|frame| decode_audit_record(&frame)),
                Err(AuditCodecError::MissingField {
                    kind: 0xa001,
                    field: 4
                })
            ));
        }

        let mut invalid_enum = fields.clone();
        if let Some(field) = invalid_enum.iter_mut().find(|(tag, _)| *tag == 5) {
            field.1 = vec![0xff];
        }
        let payload = encode_fields(invalid_enum);
        assert!(payload.is_ok());
        if let Ok(payload) = payload {
            let frame = encode_frame_for(AuditRecordKind::AuditRecord, &payload);
            assert!(matches!(
                frame.and_then(|frame| decode_audit_record(&frame)),
                Err(AuditCodecError::InvalidFieldValue {
                    kind: 0xa001,
                    field: 5
                })
            ));
        }

        assert!(matches!(
            crate::decode_record(&encoded),
            Err(crate::RecordCodecError::UnknownRecordKind { kind: 0xa001 })
        ));
        assert!(matches!(
            decode_raw_read_attempt(&encoded),
            Err(AuditCodecError::UnknownAuditKind { kind: 0xa001 })
        ));

        let Ok(encoded_frame) = decode_frame(&encoded) else {
            return;
        };
        let flagged = encode_frame(
            FrameHeader::new(AuditRecordKind::AuditRecord.number()).with_optional_flags(1),
            encoded_frame.payload(),
        );
        assert!(flagged.is_ok());
        if let Ok(flagged) = flagged {
            assert!(matches!(
                decode_audit_record(&flagged),
                Err(AuditCodecError::OptionalFlagsNotSupported { flags: 1 })
            ));
        }

        let unknown_kind = encode_frame(FrameHeader::new(0xa003), &[]);
        assert!(unknown_kind.is_ok());
        if let Ok(unknown_kind) = unknown_kind {
            assert!(matches!(
                decode_audit_record(&unknown_kind),
                Err(AuditCodecError::UnknownAuditKind { kind: 0xa003 })
            ));
        }
    }

    #[test]
    fn audit_decoders_enforce_frame_field_value_and_depth_budgets() {
        let values = fixtures();
        assert!(values.is_some(), "audit fixtures must be valid");
        let Some(values) = values else { return };
        let matching = values
            .iter()
            .find(|(name, _)| *name == "audit_record_committed");
        assert!(
            matching.is_some(),
            "committed audit fixture must be present"
        );
        let Some((_, AuditFixture::Record(record))) = matching else {
            return;
        };
        let encoded = encode_audit_record(record);
        assert!(encoded.is_ok());
        let Ok(encoded) = encoded else { return };

        let frame_limit = DecoderLimits {
            max_frame_bytes: encoded.len() - 1,
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            decode_audit_record_with_limits(&encoded, &frame_limit),
            Err(AuditCodecError::Frame(FrameError::FrameTooLarge { .. }))
        ));

        let field_limit = DecoderLimits {
            max_fields_per_record: 1,
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            decode_audit_record_with_limits(&encoded, &field_limit),
            Err(AuditCodecError::Wire(WireError::ResourceLimitExceeded {
                resource: DecodeResource::FieldsPerRecord,
                ..
            }))
        ));

        let value_limit = DecoderLimits {
            max_string_or_bytes: 2,
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            decode_audit_record_with_limits(&encoded, &value_limit),
            Err(AuditCodecError::Wire(WireError::ResourceLimitExceeded {
                resource: DecodeResource::StringOrBytes,
                ..
            }))
        ));

        let shallow = DecoderLimits {
            max_nesting_depth: 2,
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            decode_audit_record_with_limits(&encoded, &shallow),
            Err(AuditCodecError::Wire(WireError::ResourceLimitExceeded {
                resource: DecodeResource::NestingDepth,
                limit: 2,
                actual: 3,
            }))
        ));
    }
}
