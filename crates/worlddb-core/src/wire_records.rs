//! Closed M1 record frames for project, schema, and migration values.

use std::fmt;

use crate::wire::{
    DecodeResource, DecoderLimits, FrameError, FrameHeader, TlvDecoder, TlvEncoder, WireError,
    decode_frame_with_limits, decode_id, encode_frame, encode_id,
};
use crate::{
    ArchiveTransition, Assertion, AssertionRetraction, AssertionValidityClosure, Entity,
    EntityRetirement, EntityTypeDefinition, Event, EventKindDefinition, EventMask,
    EventMaskRetraction, EventRelation, EventRelationRetraction, EventRetraction, EventSpanClosure,
    Evidence, EvidenceRetraction, HistorySpaceDefinition, LayerDefinition, LayerSchemaSnapshot,
    Mask, MaskRetraction, MaskValidityClosure, MigrationPlan, MigrationRun,
    MigrationStepCommitIdentity, PerspectiveDefinitionRevision, PerspectiveRetirement,
    PredicateDefinition, ProvenanceEdge, ProvenanceRetraction, RecordRef, RecordRefWireTag,
    ReplacementBoundary, ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure, Source,
    TransferLineage,
};

mod events;
mod lifecycle;
mod meta;
mod migrations;
mod projects;
mod schema;

// The deepest path in the closed, non-recursive 1.0 record grammar is seven.
const MAX_RECORD_NESTING_DEPTH: usize = 7;

/// Stable top-level frame kind assignments for the WorldDB record stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RecordKind {
    /// Immutable HistorySpace ancestry metadata.
    HistorySpaceDefinition = 0x1001,
    /// Immutable Entity identity and permanent type assignment.
    Entity = 0x1002,
    /// Immutable Entity retirement record.
    EntityRetirement = 0x1003,
    /// One revision of Perspective display metadata.
    PerspectiveDefinitionRevision = 0x1004,
    /// Immutable Perspective retirement record.
    PerspectiveRetirement = 0x1005,
    /// One revision of a project-wide layer definition.
    LayerDefinition = 0x1006,
    /// A complete layer schema state, including its explicit base layer.
    LayerSchemaSnapshot = 0x1007,
    /// One revision of an EntityType definition.
    EntityTypeDefinition = 0x1008,
    /// One revision of a Predicate definition.
    PredicateDefinition = 0x1009,
    /// One revision of an EventKind definition.
    EventKindDefinition = 0x100a,
    /// Immutable migration plan outline.
    MigrationPlan = 0x100b,
    /// Snapshot of one migration run state.
    MigrationRun = 0x100c,
    /// Identity binding for one migration step commit.
    MigrationStepCommitIdentity = 0x100d,
    /// Immutable assertion proposition and validity.
    Assertion = 0x1101,
    /// Assertion world-time validity closure.
    AssertionValidityClosure = 0x1102,
    /// Assertion Transaction-Time retraction.
    AssertionRetraction = 0x1103,
    /// Immutable assertion mask.
    Mask = 0x1104,
    /// Mask world-time validity closure.
    MaskValidityClosure = 0x1105,
    /// Mask Transaction-Time retraction.
    MaskRetraction = 0x1106,
    /// Immutable MultiValueReplace boundary.
    ReplacementBoundary = 0x1107,
    /// Replacement-boundary world-time validity closure.
    ReplacementBoundaryValidityClosure = 0x1108,
    /// Replacement-boundary Transaction-Time retraction.
    ReplacementBoundaryRetraction = 0x1109,
    /// Operational archive state transition.
    ArchiveTransition = 0x110a,
    /// Immutable typed event record.
    Event = 0x1201,
    /// Direct mask against one Event.
    EventMask = 0x1202,
    /// Event world-time span closure.
    EventSpanClosure = 0x1203,
    /// Event Transaction-Time retraction.
    EventRetraction = 0x1204,
    /// EventMask Transaction-Time retraction.
    EventMaskRetraction = 0x1205,
    /// Canonical persisted EventRelation.
    EventRelation = 0x1206,
    /// EventRelation Transaction-Time retraction.
    EventRelationRetraction = 0x1207,
    /// Immutable project-wide source metadata.
    Source = 0x1301,
    /// Immutable link from a Source to one typed domain record.
    Evidence = 0x1302,
    /// Immutable project-wide provenance edge.
    Provenance = 0x1303,
    /// Transaction-Time retraction of one Evidence edge.
    EvidenceRetraction = 0x1304,
    /// Transaction-Time retraction of one Provenance edge.
    ProvenanceRetraction = 0x1305,
    /// Dedicated lineage record for HistorySpace copies excluded from DerivedFrom.
    TransferLineage = 0x1306,
}

impl RecordKind {
    /// Returns the stable frame-kind number.
    #[must_use]
    pub const fn number(self) -> u32 {
        self as u32
    }

    fn from_number(number: u32) -> Option<Self> {
        match number {
            0x1001 => Some(Self::HistorySpaceDefinition),
            0x1002 => Some(Self::Entity),
            0x1003 => Some(Self::EntityRetirement),
            0x1004 => Some(Self::PerspectiveDefinitionRevision),
            0x1005 => Some(Self::PerspectiveRetirement),
            0x1006 => Some(Self::LayerDefinition),
            0x1007 => Some(Self::LayerSchemaSnapshot),
            0x1008 => Some(Self::EntityTypeDefinition),
            0x1009 => Some(Self::PredicateDefinition),
            0x100a => Some(Self::EventKindDefinition),
            0x100b => Some(Self::MigrationPlan),
            0x100c => Some(Self::MigrationRun),
            0x100d => Some(Self::MigrationStepCommitIdentity),
            0x1101 => Some(Self::Assertion),
            0x1102 => Some(Self::AssertionValidityClosure),
            0x1103 => Some(Self::AssertionRetraction),
            0x1104 => Some(Self::Mask),
            0x1105 => Some(Self::MaskValidityClosure),
            0x1106 => Some(Self::MaskRetraction),
            0x1107 => Some(Self::ReplacementBoundary),
            0x1108 => Some(Self::ReplacementBoundaryValidityClosure),
            0x1109 => Some(Self::ReplacementBoundaryRetraction),
            0x110a => Some(Self::ArchiveTransition),
            0x1201 => Some(Self::Event),
            0x1202 => Some(Self::EventMask),
            0x1203 => Some(Self::EventSpanClosure),
            0x1204 => Some(Self::EventRetraction),
            0x1205 => Some(Self::EventMaskRetraction),
            0x1206 => Some(Self::EventRelation),
            0x1207 => Some(Self::EventRelationRetraction),
            0x1301 => Some(Self::Source),
            0x1302 => Some(Self::Evidence),
            0x1303 => Some(Self::Provenance),
            0x1304 => Some(Self::EvidenceRetraction),
            0x1305 => Some(Self::ProvenanceRetraction),
            0x1306 => Some(Self::TransferLineage),
            _ => None,
        }
    }
}

/// The closed set of record payloads implemented by the M1 codecs.
#[derive(Clone, Debug)]
pub enum Record {
    /// Immutable HistorySpace ancestry metadata.
    HistorySpaceDefinition(HistorySpaceDefinition),
    /// Immutable Entity identity and permanent type assignment.
    Entity(Entity),
    /// Immutable Entity retirement.
    EntityRetirement(EntityRetirement),
    /// One Perspective display-metadata revision.
    PerspectiveDefinitionRevision(PerspectiveDefinitionRevision),
    /// Immutable Perspective retirement.
    PerspectiveRetirement(PerspectiveRetirement),
    /// One revision of a layer definition.
    LayerDefinition(LayerDefinition),
    /// One complete layer schema state with explicit base designation.
    LayerSchemaSnapshot(LayerSchemaSnapshot),
    /// One EntityType schema revision.
    EntityTypeDefinition(EntityTypeDefinition),
    /// One Predicate schema revision.
    PredicateDefinition(PredicateDefinition),
    /// One EventKind schema revision, including roles and attributes.
    EventKindDefinition(EventKindDefinition),
    /// Immutable migration plan outline.
    MigrationPlan(MigrationPlan),
    /// Snapshot of a concrete migration run state.
    MigrationRun(MigrationRun),
    /// Four identities for one migration step commit.
    MigrationStepCommitIdentity(MigrationStepCommitIdentity),
    /// Immutable assertion proposition and validity.
    Assertion(Assertion),
    /// Assertion world-time validity closure.
    AssertionValidityClosure(AssertionValidityClosure),
    /// Assertion Transaction-Time retraction.
    AssertionRetraction(AssertionRetraction),
    /// Immutable assertion mask.
    Mask(Mask),
    /// Mask world-time validity closure.
    MaskValidityClosure(MaskValidityClosure),
    /// Mask Transaction-Time retraction.
    MaskRetraction(MaskRetraction),
    /// Immutable MultiValueReplace boundary.
    ReplacementBoundary(ReplacementBoundary),
    /// Replacement-boundary world-time validity closure.
    ReplacementBoundaryValidityClosure(ReplacementBoundaryValidityClosure),
    /// Replacement-boundary Transaction-Time retraction.
    ReplacementBoundaryRetraction(ReplacementBoundaryRetraction),
    /// Operational archive state transition.
    ArchiveTransition(ArchiveTransition),
    /// One schema-typed immutable Event.
    Event(Event),
    /// One direct Event mask.
    EventMask(EventMask),
    /// One closure of an open Event span.
    EventSpanClosure(EventSpanClosure),
    /// One Transaction-Time retraction of an Event.
    EventRetraction(EventRetraction),
    /// One Transaction-Time retraction of an EventMask.
    EventMaskRetraction(EventMaskRetraction),
    /// One canonical EventRelation.
    EventRelation(EventRelation),
    /// One Transaction-Time retraction of an EventRelation.
    EventRelationRetraction(EventRelationRetraction),
    /// One project-wide immutable Source record.
    Source(Source),
    /// One project-wide Evidence link.
    Evidence(Evidence),
    /// One project-wide immutable Provenance edge.
    Provenance(ProvenanceEdge),
    /// One Transaction-Time retraction of an Evidence link.
    EvidenceRetraction(EvidenceRetraction),
    /// One Transaction-Time retraction of a Provenance edge.
    ProvenanceRetraction(ProvenanceRetraction),
    /// Dedicated record for the source and target of a HistorySpace copy.
    TransferLineage(TransferLineage),
}

/// A decoded record with optional frame capabilities retained for exact roundtrip.
#[derive(Clone, Debug)]
pub struct DecodedRecord {
    record: Record,
    optional_flags: u64,
}

impl DecodedRecord {
    /// Returns the typed record payload.
    #[must_use]
    pub const fn record(&self) -> &Record {
        &self.record
    }

    /// Returns all optional frame bits unchanged; this codec does not interpret them.
    #[must_use]
    pub const fn optional_flags(&self) -> u64 {
        self.optional_flags
    }

    /// Consumes the decoded wrapper and returns its typed payload.
    #[must_use]
    pub fn into_record(self) -> Record {
        self.record
    }
}

/// A malformed, noncanonical, or unsupported M1 record frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordCodecError {
    /// The frame header, version, required flags, or checksum is invalid.
    Frame(FrameError),
    /// A field or scalar encoding is malformed.
    Wire(WireError),
    /// No record codec is assigned to this frame kind.
    UnknownRecordKind { kind: u32 },
    /// A closed record contains a field not declared for its type.
    UnknownField { kind: u32, field: u32 },
    /// A required field is absent.
    MissingField { kind: u32, field: u32 },
    /// A field's bytes do not construct a valid domain value.
    InvalidFieldValue { kind: u32, field: u32 },
    /// Decoding succeeded but the bytes differ from the unique canonical encoding.
    NonCanonicalRecord,
    /// The RecordRef tag is not assigned by the closed registry.
    UnknownRecordRefTag { tag: u16 },
    /// The numeric RecordRef tag does not fit the registry's u16 domain.
    RecordRefTagOverflow { tag: u128 },
}

impl fmt::Display for RecordCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => write!(formatter, "record frame: {error}"),
            Self::Wire(error) => write!(formatter, "record wire value: {error}"),
            Self::UnknownRecordKind { kind } => {
                write!(formatter, "unknown record kind 0x{kind:08x}")
            }
            Self::UnknownField { kind, field } => write!(
                formatter,
                "record kind 0x{kind:08x} has unknown field {field}"
            ),
            Self::MissingField { kind, field } => write!(
                formatter,
                "record kind 0x{kind:08x} is missing required field {field}"
            ),
            Self::InvalidFieldValue { kind, field } => write!(
                formatter,
                "record kind 0x{kind:08x} has an invalid value in field {field}"
            ),
            Self::NonCanonicalRecord => {
                formatter.write_str("record is valid but is not canonically encoded")
            }
            Self::UnknownRecordRefTag { tag } => write!(formatter, "unknown RecordRef tag {tag}"),
            Self::RecordRefTagOverflow { tag } => {
                write!(formatter, "RecordRef tag {tag} exceeds u16")
            }
        }
    }
}

impl std::error::Error for RecordCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::UnknownRecordKind { .. }
            | Self::UnknownField { .. }
            | Self::MissingField { .. }
            | Self::InvalidFieldValue { .. }
            | Self::NonCanonicalRecord
            | Self::UnknownRecordRefTag { .. }
            | Self::RecordRefTagOverflow { .. } => None,
        }
    }
}

/// Encodes one typed WorldDB record with no optional capability bits.
pub fn encode_record(record: &Record) -> Result<Vec<u8>, RecordCodecError> {
    encode_record_with_flags(record, 0)
}

/// Encodes a record while preserving caller-supplied optional capability bits.
pub fn encode_record_with_flags(
    record: &Record,
    optional_flags: u64,
) -> Result<Vec<u8>, RecordCodecError> {
    let (kind, payload) = encode_payload(record)?;
    encode_frame(
        FrameHeader::new(kind.number()).with_optional_flags(optional_flags),
        &payload,
    )
    .map_err(RecordCodecError::Frame)
}

/// Verifies and decodes one known, closed WorldDB record frame.
pub fn decode_record(bytes: &[u8]) -> Result<DecodedRecord, RecordCodecError> {
    decode_record_with_limits(bytes, &DecoderLimits::process_default())
}

/// Decodes one known record under an explicit resource policy.
pub fn decode_record_with_limits(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<DecodedRecord, RecordCodecError> {
    limits
        .check_nesting_depth(MAX_RECORD_NESTING_DEPTH)
        .map_err(RecordCodecError::Wire)?;
    let frame = decode_frame_with_limits(bytes, limits).map_err(RecordCodecError::Frame)?;
    let kind = RecordKind::from_number(frame.header().kind()).ok_or(
        RecordCodecError::UnknownRecordKind {
            kind: frame.header().kind(),
        },
    )?;
    let record = decode_payload(kind, frame.payload(), limits)?;
    let decoded = DecodedRecord {
        record,
        optional_flags: frame.header().optional_flags(),
    };
    if encode_decoded_record(&decoded)? != bytes {
        return Err(RecordCodecError::NonCanonicalRecord);
    }
    Ok(decoded)
}

/// Decodes a borrowed list of frames as one batch after checking count and byte budgets.
pub fn decode_record_batch_with_limits(
    frames: &[&[u8]],
    limits: &DecoderLimits,
) -> Result<Vec<DecodedRecord>, RecordCodecError> {
    if frames.len() > limits.max_records_per_batch {
        return Err(RecordCodecError::Wire(WireError::ResourceLimitExceeded {
            resource: DecodeResource::BatchRecords,
            limit: limits.max_records_per_batch,
            actual: frames.len(),
        }));
    }
    let mut total_bytes = 0usize;
    for frame in frames {
        total_bytes = total_bytes
            .checked_add(frame.len())
            .ok_or(RecordCodecError::Wire(WireError::ResourceLimitExceeded {
                resource: DecodeResource::BatchBytes,
                limit: limits.max_batch_bytes,
                actual: usize::MAX,
            }))?;
        if total_bytes > limits.max_batch_bytes {
            return Err(RecordCodecError::Wire(WireError::ResourceLimitExceeded {
                resource: DecodeResource::BatchBytes,
                limit: limits.max_batch_bytes,
                actual: total_bytes,
            }));
        }
    }
    check_collection_size::<DecodedRecord>(frames.len(), limits).map_err(RecordCodecError::Wire)?;
    let mut decoded = Vec::new();
    decoded.try_reserve_exact(frames.len()).map_err(|_| {
        RecordCodecError::Wire(WireError::AllocationFailed {
            resource: DecodeResource::BatchRecords,
        })
    })?;
    for frame in frames {
        decoded.push(decode_record_with_limits(frame, limits)?);
    }
    Ok(decoded)
}

/// Re-encodes a decoded record without dropping unknown optional capability bits.
pub fn encode_decoded_record(record: &DecodedRecord) -> Result<Vec<u8>, RecordCodecError> {
    encode_record_with_flags(&record.record, record.optional_flags)
}

fn encode_payload(record: &Record) -> Result<(RecordKind, Vec<u8>), RecordCodecError> {
    match record {
        Record::HistorySpaceDefinition(value) => Ok((
            RecordKind::HistorySpaceDefinition,
            projects::encode_history_space(value)?,
        )),
        Record::Entity(value) => Ok((RecordKind::Entity, projects::encode_entity(value)?)),
        Record::EntityRetirement(value) => Ok((
            RecordKind::EntityRetirement,
            projects::encode_entity_retirement(value)?,
        )),
        Record::PerspectiveDefinitionRevision(value) => Ok((
            RecordKind::PerspectiveDefinitionRevision,
            projects::encode_perspective_definition(value)?,
        )),
        Record::PerspectiveRetirement(value) => Ok((
            RecordKind::PerspectiveRetirement,
            projects::encode_perspective_retirement(value)?,
        )),
        Record::LayerDefinition(value) => Ok((
            RecordKind::LayerDefinition,
            projects::encode_layer_definition(value)?,
        )),
        Record::LayerSchemaSnapshot(value) => Ok((
            RecordKind::LayerSchemaSnapshot,
            projects::encode_layer_snapshot(value)?,
        )),
        Record::EntityTypeDefinition(value) => Ok((
            RecordKind::EntityTypeDefinition,
            schema::encode_entity_type(value)?,
        )),
        Record::PredicateDefinition(value) => Ok((
            RecordKind::PredicateDefinition,
            schema::encode_predicate(value)?,
        )),
        Record::EventKindDefinition(value) => Ok((
            RecordKind::EventKindDefinition,
            schema::encode_event_kind(value)?,
        )),
        Record::MigrationPlan(value) => {
            Ok((RecordKind::MigrationPlan, migrations::encode_plan(value)?))
        }
        Record::MigrationRun(value) => {
            Ok((RecordKind::MigrationRun, migrations::encode_run(value)?))
        }
        Record::MigrationStepCommitIdentity(value) => Ok((
            RecordKind::MigrationStepCommitIdentity,
            migrations::encode_step_commit(value)?,
        )),
        Record::Assertion(value) => {
            Ok((RecordKind::Assertion, lifecycle::encode_assertion(value)?))
        }
        Record::AssertionValidityClosure(value) => Ok((
            RecordKind::AssertionValidityClosure,
            lifecycle::encode_assertion_validity_closure(value)?,
        )),
        Record::AssertionRetraction(value) => Ok((
            RecordKind::AssertionRetraction,
            lifecycle::encode_assertion_retraction(value)?,
        )),
        Record::Mask(value) => Ok((RecordKind::Mask, lifecycle::encode_mask(value)?)),
        Record::MaskValidityClosure(value) => Ok((
            RecordKind::MaskValidityClosure,
            lifecycle::encode_mask_validity_closure(value)?,
        )),
        Record::MaskRetraction(value) => Ok((
            RecordKind::MaskRetraction,
            lifecycle::encode_mask_retraction(value)?,
        )),
        Record::ReplacementBoundary(value) => Ok((
            RecordKind::ReplacementBoundary,
            lifecycle::encode_replacement_boundary(value)?,
        )),
        Record::ReplacementBoundaryValidityClosure(value) => Ok((
            RecordKind::ReplacementBoundaryValidityClosure,
            lifecycle::encode_boundary_validity_closure(value)?,
        )),
        Record::ReplacementBoundaryRetraction(value) => Ok((
            RecordKind::ReplacementBoundaryRetraction,
            lifecycle::encode_boundary_retraction(value)?,
        )),
        Record::ArchiveTransition(value) => Ok((
            RecordKind::ArchiveTransition,
            lifecycle::encode_archive_transition(value)?,
        )),
        Record::Event(value) => Ok((RecordKind::Event, events::encode_event(value)?)),
        Record::EventMask(value) => Ok((RecordKind::EventMask, events::encode_event_mask(*value)?)),
        Record::EventSpanClosure(value) => Ok((
            RecordKind::EventSpanClosure,
            events::encode_event_span_closure(*value)?,
        )),
        Record::EventRetraction(value) => Ok((
            RecordKind::EventRetraction,
            events::encode_event_retraction(value)?,
        )),
        Record::EventMaskRetraction(value) => Ok((
            RecordKind::EventMaskRetraction,
            events::encode_event_mask_retraction(value)?,
        )),
        Record::EventRelation(value) => Ok((
            RecordKind::EventRelation,
            events::encode_event_relation(*value)?,
        )),
        Record::EventRelationRetraction(value) => Ok((
            RecordKind::EventRelationRetraction,
            events::encode_event_relation_retraction(value)?,
        )),
        Record::Source(value) => Ok((RecordKind::Source, meta::encode_source(value)?)),
        Record::Evidence(value) => Ok((RecordKind::Evidence, meta::encode_evidence(value)?)),
        Record::Provenance(value) => Ok((RecordKind::Provenance, meta::encode_provenance(value)?)),
        Record::EvidenceRetraction(value) => Ok((
            RecordKind::EvidenceRetraction,
            meta::encode_evidence_retraction(value)?,
        )),
        Record::ProvenanceRetraction(value) => Ok((
            RecordKind::ProvenanceRetraction,
            meta::encode_provenance_retraction(value)?,
        )),
        Record::TransferLineage(value) => Ok((
            RecordKind::TransferLineage,
            meta::encode_transfer_lineage(*value)?,
        )),
    }
}

fn decode_payload(
    kind: RecordKind,
    payload: &[u8],
    limits: &DecoderLimits,
) -> Result<Record, RecordCodecError> {
    match kind {
        RecordKind::HistorySpaceDefinition => {
            projects::decode_history_space(payload, limits).map(Record::HistorySpaceDefinition)
        }
        RecordKind::Entity => projects::decode_entity(payload, limits).map(Record::Entity),
        RecordKind::EntityRetirement => {
            projects::decode_entity_retirement(payload, limits).map(Record::EntityRetirement)
        }
        RecordKind::PerspectiveDefinitionRevision => {
            projects::decode_perspective_definition(payload, limits)
                .map(Record::PerspectiveDefinitionRevision)
        }
        RecordKind::PerspectiveRetirement => {
            projects::decode_perspective_retirement(payload, limits)
                .map(Record::PerspectiveRetirement)
        }
        RecordKind::LayerDefinition => {
            projects::decode_layer_definition(payload, limits).map(Record::LayerDefinition)
        }
        RecordKind::LayerSchemaSnapshot => {
            projects::decode_layer_snapshot(payload, limits).map(Record::LayerSchemaSnapshot)
        }
        RecordKind::EntityTypeDefinition => {
            schema::decode_entity_type(payload, limits).map(Record::EntityTypeDefinition)
        }
        RecordKind::PredicateDefinition => {
            schema::decode_predicate(payload, limits).map(Record::PredicateDefinition)
        }
        RecordKind::EventKindDefinition => {
            schema::decode_event_kind(payload, limits).map(Record::EventKindDefinition)
        }
        RecordKind::MigrationPlan => {
            migrations::decode_plan(payload, limits).map(Record::MigrationPlan)
        }
        RecordKind::MigrationRun => {
            migrations::decode_run(payload, limits).map(Record::MigrationRun)
        }
        RecordKind::MigrationStepCommitIdentity => {
            migrations::decode_step_commit(payload, limits).map(Record::MigrationStepCommitIdentity)
        }
        RecordKind::Assertion => {
            lifecycle::decode_assertion(payload, limits).map(Record::Assertion)
        }
        RecordKind::AssertionValidityClosure => {
            lifecycle::decode_assertion_validity_closure(payload, limits)
                .map(Record::AssertionValidityClosure)
        }
        RecordKind::AssertionRetraction => {
            lifecycle::decode_assertion_retraction(payload, limits).map(Record::AssertionRetraction)
        }
        RecordKind::Mask => lifecycle::decode_mask(payload, limits).map(Record::Mask),
        RecordKind::MaskValidityClosure => lifecycle::decode_mask_validity_closure(payload, limits)
            .map(Record::MaskValidityClosure),
        RecordKind::MaskRetraction => {
            lifecycle::decode_mask_retraction(payload, limits).map(Record::MaskRetraction)
        }
        RecordKind::ReplacementBoundary => {
            lifecycle::decode_replacement_boundary(payload, limits).map(Record::ReplacementBoundary)
        }
        RecordKind::ReplacementBoundaryValidityClosure => {
            lifecycle::decode_boundary_validity_closure(payload, limits)
                .map(Record::ReplacementBoundaryValidityClosure)
        }
        RecordKind::ReplacementBoundaryRetraction => {
            lifecycle::decode_boundary_retraction(payload, limits)
                .map(Record::ReplacementBoundaryRetraction)
        }
        RecordKind::ArchiveTransition => {
            lifecycle::decode_archive_transition(payload, limits).map(Record::ArchiveTransition)
        }
        RecordKind::Event => events::decode_event(payload, limits).map(Record::Event),
        RecordKind::EventMask => events::decode_event_mask(payload, limits).map(Record::EventMask),
        RecordKind::EventSpanClosure => {
            events::decode_event_span_closure(payload, limits).map(Record::EventSpanClosure)
        }
        RecordKind::EventRetraction => {
            events::decode_event_retraction(payload, limits).map(Record::EventRetraction)
        }
        RecordKind::EventMaskRetraction => {
            events::decode_event_mask_retraction(payload, limits).map(Record::EventMaskRetraction)
        }
        RecordKind::EventRelation => {
            events::decode_event_relation(payload, limits).map(Record::EventRelation)
        }
        RecordKind::EventRelationRetraction => {
            events::decode_event_relation_retraction(payload, limits)
                .map(Record::EventRelationRetraction)
        }
        RecordKind::Source => meta::decode_source(payload, limits).map(Record::Source),
        RecordKind::Evidence => meta::decode_evidence(payload, limits).map(Record::Evidence),
        RecordKind::Provenance => meta::decode_provenance(payload, limits).map(Record::Provenance),
        RecordKind::EvidenceRetraction => {
            meta::decode_evidence_retraction(payload, limits).map(Record::EvidenceRetraction)
        }
        RecordKind::ProvenanceRetraction => {
            meta::decode_provenance_retraction(payload, limits).map(Record::ProvenanceRetraction)
        }
        RecordKind::TransferLineage => {
            meta::decode_transfer_lineage(payload, limits).map(Record::TransferLineage)
        }
    }
}

/// Encodes one of the 25 closed `RecordRef` variants as its
/// minimal unsigned LEB128 registry tag followed by the exact 16-byte identity.
pub fn encode_record_ref(reference: RecordRef) -> Result<Vec<u8>, RecordCodecError> {
    let tag = reference.wire_tag().value();
    let id = match reference {
        RecordRef::Assertion(id) => encode_id(id),
        RecordRef::Mask(id) => encode_id(id),
        RecordRef::ReplacementBoundary(id) => encode_id(id),
        RecordRef::Event(id) => encode_id(id),
        RecordRef::EventMask(id) => encode_id(id),
        RecordRef::EventRelation(id) => encode_id(id),
        RecordRef::Source(id) => encode_id(id),
        RecordRef::Evidence(id) => encode_id(id),
        RecordRef::Provenance(id) => encode_id(id),
        RecordRef::AssertionValidityClosure(id) => encode_id(id),
        RecordRef::AssertionRetraction(id) => encode_id(id),
        RecordRef::MaskValidityClosure(id) => encode_id(id),
        RecordRef::MaskRetraction(id) => encode_id(id),
        RecordRef::ReplacementBoundaryValidityClosure(id) => encode_id(id),
        RecordRef::ReplacementBoundaryRetraction(id) => encode_id(id),
        RecordRef::EventSpanClosure(id) => encode_id(id),
        RecordRef::EventRetraction(id) => encode_id(id),
        RecordRef::EventMaskRetraction(id) => encode_id(id),
        RecordRef::EventRelationRetraction(id) => encode_id(id),
        RecordRef::EvidenceRetraction(id) => encode_id(id),
        RecordRef::ProvenanceRetraction(id) => encode_id(id),
        RecordRef::EntityRetirement(id) => encode_id(id),
        RecordRef::PerspectiveRetirement(id) => encode_id(id),
        RecordRef::ArchiveTransition(id) => encode_id(id),
        RecordRef::TransferLineage(id) => encode_id(id),
    };
    let mut bytes = crate::numbers::encode_u128_varint(u128::from(tag));
    bytes.extend_from_slice(&id);
    Ok(bytes)
}

/// Decodes one of the 24 closed `RecordRef` scalar variants.
pub fn decode_record_ref(bytes: &[u8]) -> Result<RecordRef, RecordCodecError> {
    let (raw_tag, tag_len) = crate::numbers::decode_u128_varint_prefix(bytes)
        .map_err(|error| RecordCodecError::Wire(WireError::Varint { offset: 0, error }))?;
    let tag = u16::try_from(raw_tag)
        .map_err(|_| RecordCodecError::RecordRefTagOverflow { tag: raw_tag })?;
    let wire_tag = RecordRefWireTag::try_from(tag)
        .map_err(|_| RecordCodecError::UnknownRecordRefTag { tag })?;
    let id_bytes = bytes
        .get(tag_len..)
        .ok_or(RecordCodecError::Wire(WireError::Truncated {
            offset: tag_len,
        }))?;
    if id_bytes.len() != 16 {
        return Err(RecordCodecError::Wire(WireError::LengthMismatch {
            offset: bytes.len(),
        }));
    }
    match wire_tag {
        RecordRefWireTag::Assertion => decode_id(id_bytes)
            .map(RecordRef::Assertion)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::Mask => decode_id(id_bytes)
            .map(RecordRef::Mask)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::ReplacementBoundary => decode_id(id_bytes)
            .map(RecordRef::ReplacementBoundary)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::Event => decode_id(id_bytes)
            .map(RecordRef::Event)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EventMask => decode_id(id_bytes)
            .map(RecordRef::EventMask)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EventRelation => decode_id(id_bytes)
            .map(RecordRef::EventRelation)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::Source => decode_id(id_bytes)
            .map(RecordRef::Source)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::Evidence => decode_id(id_bytes)
            .map(RecordRef::Evidence)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::Provenance => decode_id(id_bytes)
            .map(RecordRef::Provenance)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::AssertionValidityClosure => decode_id(id_bytes)
            .map(RecordRef::AssertionValidityClosure)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::AssertionRetraction => decode_id(id_bytes)
            .map(RecordRef::AssertionRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::MaskValidityClosure => decode_id(id_bytes)
            .map(RecordRef::MaskValidityClosure)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::MaskRetraction => decode_id(id_bytes)
            .map(RecordRef::MaskRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::ReplacementBoundaryValidityClosure => decode_id(id_bytes)
            .map(RecordRef::ReplacementBoundaryValidityClosure)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::ReplacementBoundaryRetraction => decode_id(id_bytes)
            .map(RecordRef::ReplacementBoundaryRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EventSpanClosure => decode_id(id_bytes)
            .map(RecordRef::EventSpanClosure)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EventRetraction => decode_id(id_bytes)
            .map(RecordRef::EventRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EventMaskRetraction => decode_id(id_bytes)
            .map(RecordRef::EventMaskRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EventRelationRetraction => decode_id(id_bytes)
            .map(RecordRef::EventRelationRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EvidenceRetraction => decode_id(id_bytes)
            .map(RecordRef::EvidenceRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::ProvenanceRetraction => decode_id(id_bytes)
            .map(RecordRef::ProvenanceRetraction)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::EntityRetirement => decode_id(id_bytes)
            .map(RecordRef::EntityRetirement)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::PerspectiveRetirement => decode_id(id_bytes)
            .map(RecordRef::PerspectiveRetirement)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::ArchiveTransition => decode_id(id_bytes)
            .map(RecordRef::ArchiveTransition)
            .map_err(RecordCodecError::Wire),
        RecordRefWireTag::TransferLineage => decode_id(id_bytes)
            .map(RecordRef::TransferLineage)
            .map_err(RecordCodecError::Wire),
    }
}

pub(super) fn encode_fields(
    kind: RecordKind,
    values: Vec<(u32, Vec<u8>)>,
) -> Result<Vec<u8>, RecordCodecError> {
    let mut encoder = TlvEncoder::new();
    for (field, value) in values {
        encoder
            .push(field, &value)
            .map_err(RecordCodecError::Wire)?;
    }
    let _ = kind;
    Ok(encoder.finish())
}

#[cfg(test)]
pub(super) fn decode_fields<'a>(
    kind: RecordKind,
    payload: &'a [u8],
    allowed: &[u32],
) -> Result<Vec<(u32, &'a [u8])>, RecordCodecError> {
    decode_fields_with_limits(kind, payload, allowed, &DecoderLimits::process_default())
}

pub(super) fn decode_fields_with_limits<'a>(
    kind: RecordKind,
    payload: &'a [u8],
    allowed: &[u32],
    limits: &DecoderLimits,
) -> Result<Vec<(u32, &'a [u8])>, RecordCodecError> {
    let mut decoder = TlvDecoder::with_limits(payload, *limits);
    let mut fields = Vec::new();
    while let Some(field) = decoder.next_field().map_err(RecordCodecError::Wire)? {
        if !allowed.contains(&field.tag()) {
            return Err(RecordCodecError::UnknownField {
                kind: kind.number(),
                field: field.tag(),
            });
        }
        fields.try_reserve(1).map_err(|_| {
            RecordCodecError::Wire(WireError::AllocationFailed {
                resource: DecodeResource::FieldsPerRecord,
            })
        })?;
        fields.push((field.tag(), field.value()));
    }
    Ok(fields)
}

pub(super) fn required_field<'a>(
    kind: RecordKind,
    fields: &[(u32, &'a [u8])],
    tag: u32,
) -> Result<&'a [u8], RecordCodecError> {
    fields
        .iter()
        .find_map(|(field, value)| (*field == tag).then_some(*value))
        .ok_or(RecordCodecError::MissingField {
            kind: kind.number(),
            field: tag,
        })
}

pub(super) fn invalid_field(kind: RecordKind, field: u32) -> RecordCodecError {
    RecordCodecError::InvalidFieldValue {
        kind: kind.number(),
        field,
    }
}

pub(super) fn encode_array(items: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = crate::numbers::encode_u128_varint(items.len() as u128);
    for item in items {
        bytes.extend(crate::numbers::encode_u128_varint(item.len() as u128));
        bytes.extend_from_slice(item);
    }
    bytes
}

#[cfg(test)]
pub(super) fn decode_array(bytes: &[u8]) -> Result<Vec<&[u8]>, WireError> {
    decode_array_with_limits(bytes, &DecoderLimits::process_default())
}

pub(super) fn decode_array_with_limits<'a>(
    bytes: &'a [u8],
    limits: &DecoderLimits,
) -> Result<Vec<&'a [u8]>, WireError> {
    let (count, prefix_len) = crate::numbers::decode_u128_varint_prefix(bytes)
        .map_err(|error| WireError::Varint { offset: 0, error })?;
    if count > limits.max_array_items as u128 {
        return Err(WireError::ResourceLimitExceeded {
            resource: DecodeResource::ArrayItems,
            limit: limits.max_array_items,
            actual: limits.max_array_items.saturating_add(1),
        });
    }
    let count = usize::try_from(count).map_err(|_| WireError::LengthOverflow { offset: 0 })?;
    let mut cursor = prefix_len;
    if count > bytes.len().saturating_sub(cursor) {
        return Err(WireError::LengthMismatch { offset: cursor });
    }
    check_collection_size::<&[u8]>(count, limits)?;
    let mut items = Vec::new();
    items
        .try_reserve_exact(count)
        .map_err(|_| WireError::AllocationFailed {
            resource: DecodeResource::CollectionBytes,
        })?;
    for _ in 0..count {
        let remaining = bytes
            .get(cursor..)
            .ok_or(WireError::Truncated { offset: cursor })?;
        let (length, length_prefix_len) = crate::numbers::decode_u128_varint_prefix(remaining)
            .map_err(|error| WireError::Varint {
                offset: cursor,
                error,
            })?;
        let value_start = cursor
            .checked_add(length_prefix_len)
            .ok_or(WireError::LengthOverflow { offset: cursor })?;
        let available =
            remaining
                .len()
                .checked_sub(length_prefix_len)
                .ok_or(WireError::Truncated {
                    offset: value_start,
                })?;
        if length > available as u128 {
            return Err(WireError::Truncated {
                offset: value_start,
            });
        }
        let length =
            usize::try_from(length).map_err(|_| WireError::LengthOverflow { offset: cursor })?;
        cursor = value_start;
        let end = cursor
            .checked_add(length)
            .ok_or(WireError::LengthOverflow { offset: cursor })?;
        let item = bytes
            .get(cursor..end)
            .ok_or(WireError::Truncated { offset: cursor })?;
        items.push(item);
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err(WireError::TrailingBytes { offset: cursor });
    }
    Ok(items)
}

pub(super) fn check_collection_size<T>(
    count: usize,
    limits: &DecoderLimits,
) -> Result<(), WireError> {
    let bytes =
        count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(WireError::ResourceLimitExceeded {
                resource: DecodeResource::CollectionBytes,
                limit: limits.max_collection_bytes,
                actual: usize::MAX,
            })?;
    if bytes > limits.max_collection_bytes {
        return Err(WireError::ResourceLimitExceeded {
            resource: DecodeResource::CollectionBytes,
            limit: limits.max_collection_bytes,
            actual: bytes,
        });
    }
    Ok(())
}

pub(super) fn reserve_collection<T>(
    count: usize,
    limits: &DecoderLimits,
) -> Result<Vec<T>, WireError> {
    check_collection_size::<T>(count, limits)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| WireError::AllocationFailed {
            resource: DecodeResource::CollectionBytes,
        })?;
    Ok(values)
}

pub(super) fn collect_results_limited<T, I>(
    values: I,
    limits: &DecoderLimits,
) -> Result<Vec<T>, RecordCodecError>
where
    I: ExactSizeIterator<Item = Result<T, RecordCodecError>>,
{
    let count = values.len();
    let mut output = reserve_collection(count, limits).map_err(RecordCodecError::Wire)?;
    for value in values {
        output.push(value?);
    }
    Ok(output)
}

pub(super) fn encode_string(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut encoded = crate::numbers::encode_u128_varint(bytes.len() as u128);
    encoded.extend_from_slice(bytes);
    encoded
}

pub(super) fn decode_string_with_limits(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<String, WireError> {
    let (length, prefix_len) = crate::numbers::decode_u128_varint_prefix(bytes)
        .map_err(|error| WireError::Varint { offset: 0, error })?;
    if length > limits.max_string_or_bytes as u128 {
        return Err(WireError::ResourceLimitExceeded {
            resource: DecodeResource::StringOrBytes,
            limit: limits.max_string_or_bytes,
            actual: limits.max_string_or_bytes.saturating_add(1),
        });
    }
    let length = usize::try_from(length).map_err(|_| WireError::LengthOverflow { offset: 0 })?;
    let end = prefix_len
        .checked_add(length)
        .ok_or(WireError::LengthOverflow { offset: prefix_len })?;
    if end != bytes.len() {
        return if end < bytes.len() {
            Err(WireError::TrailingBytes { offset: end })
        } else {
            Err(WireError::Truncated { offset: prefix_len })
        };
    }
    let value = bytes
        .get(prefix_len..end)
        .ok_or(WireError::Truncated { offset: prefix_len })?;
    let text = std::str::from_utf8(value).map_err(|error| WireError::InvalidUtf8 {
        offset: prefix_len.saturating_add(error.valid_up_to()),
    })?;
    Ok(text.to_owned())
}

pub(super) fn encode_revision(value: crate::Revision) -> Vec<u8> {
    crate::numbers::encode_u128_varint(u128::from(value.value()))
}

pub(super) fn decode_revision(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<crate::Revision, RecordCodecError> {
    let value = crate::UInt::from_canonical_bytes(bytes)
        .map_err(|error| RecordCodecError::Wire(WireError::Integer(error)))?;
    u64::try_from(value.value())
        .map_err(|_| invalid_field(kind, field))
        .and_then(|number| {
            crate::Revision::try_from(number).map_err(|_| invalid_field(kind, field))
        })
}

pub(super) fn encode_schema_revision(value: crate::SchemaRevision) -> Vec<u8> {
    encode_revision(value.revision())
}

pub(super) fn decode_schema_revision(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<crate::SchemaRevision, RecordCodecError> {
    decode_revision(kind, field, bytes).map(crate::SchemaRevision::from_published_revision)
}

#[cfg(test)]
mod tests;
