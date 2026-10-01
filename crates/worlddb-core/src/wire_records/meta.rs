//! Project-wide Source, Evidence, and Provenance record payloads.

use crate::ids::TransferLineageId;
use crate::source_provenance::{
    Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, ProvenanceEdge,
    ProvenanceEndpointRef, ProvenanceRelation, ProvenanceRetraction, Source, SourceContentDigest,
    SourceLocator, SourceMetadata, SourceMetadataEntry,
};
use crate::wire::{
    DecodeResource, DecoderLimits, WireError, decode_id, decode_value_with_limits, encode_id,
    encode_value,
};
use crate::{Bytes, RecordRef, Symbol};
use crate::{HistorySpaceContentRef, TransferLineage};

use super::lifecycle::{optional, read_optional};
use super::{
    RecordCodecError, RecordKind, decode_array_with_limits, decode_fields_with_limits,
    decode_revision, decode_string_with_limits, encode_array, encode_fields, encode_revision,
    encode_string, invalid_field, required_field, reserve_collection,
};

fn encode_symbol(value: &Symbol) -> Vec<u8> {
    encode_string(value.as_str())
}

fn decode_symbol(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Symbol, RecordCodecError> {
    let value = decode_string_with_limits(bytes, limits).map_err(RecordCodecError::Wire)?;
    Symbol::new(value).map_err(|_| invalid_field(kind, field))
}

fn encode_metadata(value: &SourceMetadata) -> Result<Vec<u8>, RecordCodecError> {
    let items = value
        .as_slice()
        .iter()
        .map(|entry| {
            let encoded_value = encode_value(entry.value());
            encode_fields(
                RecordKind::Source,
                vec![(1, encode_symbol(entry.key())), (2, encoded_value)],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(encode_array(&items))
}

fn decode_metadata(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<SourceMetadata, RecordCodecError> {
    let kind = RecordKind::Source;
    let items = decode_array_with_limits(bytes, limits).map_err(RecordCodecError::Wire)?;
    let mut entries = reserve_collection(items.len(), limits).map_err(RecordCodecError::Wire)?;
    let mut previous: Option<Symbol> = None;
    for item in items {
        let fields = decode_fields_with_limits(kind, item, &[1, 2], limits)?;
        let key = decode_symbol(kind, 5, required_field(kind, &fields, 1)?, limits)?;
        if previous.as_ref().is_some_and(|prior| prior >= &key) {
            return Err(invalid_field(kind, 5));
        }
        let value = decode_value_with_limits(required_field(kind, &fields, 2)?, limits)
            .map_err(RecordCodecError::Wire)?;
        previous = Some(key.clone());
        entries.push(SourceMetadataEntry::new(key, value));
    }
    SourceMetadata::new(entries).map_err(|_| invalid_field(kind, 5))
}

fn encode_evidence_target(value: EvidenceTargetRef) -> Result<Vec<u8>, RecordCodecError> {
    crate::wire_records::encode_record_ref(evidence_target_to_record_ref(value))
}

fn evidence_target_to_record_ref(value: EvidenceTargetRef) -> RecordRef {
    match value {
        EvidenceTargetRef::Assertion(id) => RecordRef::Assertion(id),
        EvidenceTargetRef::Mask(id) => RecordRef::Mask(id),
        EvidenceTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        EvidenceTargetRef::Event(id) => RecordRef::Event(id),
        EvidenceTargetRef::EventMask(id) => RecordRef::EventMask(id),
        EvidenceTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        EvidenceTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        EvidenceTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        EvidenceTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        EvidenceTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        EvidenceTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        EvidenceTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        EvidenceTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        EvidenceTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        EvidenceTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        EvidenceTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        EvidenceTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        EvidenceTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        EvidenceTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        EvidenceTargetRef::Provenance(id) => RecordRef::Provenance(id),
        EvidenceTargetRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn provenance_endpoint_to_record_ref(value: ProvenanceEndpointRef) -> RecordRef {
    match value {
        ProvenanceEndpointRef::Assertion(id) => RecordRef::Assertion(id),
        ProvenanceEndpointRef::Mask(id) => RecordRef::Mask(id),
        ProvenanceEndpointRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ProvenanceEndpointRef::Event(id) => RecordRef::Event(id),
        ProvenanceEndpointRef::EventMask(id) => RecordRef::EventMask(id),
        ProvenanceEndpointRef::Source(id) => RecordRef::Source(id),
        ProvenanceEndpointRef::Evidence(id) => RecordRef::Evidence(id),
        ProvenanceEndpointRef::Provenance(id) => RecordRef::Provenance(id),
        ProvenanceEndpointRef::AssertionValidityClosure(id) => {
            RecordRef::AssertionValidityClosure(id)
        }
        ProvenanceEndpointRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ProvenanceEndpointRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ProvenanceEndpointRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ProvenanceEndpointRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ProvenanceEndpointRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ProvenanceEndpointRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ProvenanceEndpointRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ProvenanceEndpointRef::EventRelationRetraction(id) => {
            RecordRef::EventRelationRetraction(id)
        }
        ProvenanceEndpointRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ProvenanceEndpointRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ProvenanceEndpointRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ProvenanceEndpointRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ProvenanceEndpointRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn decode_evidence_target(
    bytes: &[u8],
    _limits: &DecoderLimits,
) -> Result<EvidenceTargetRef, RecordCodecError> {
    let reference = crate::wire_records::decode_record_ref(bytes)?;
    EvidenceTargetRef::try_from(reference).map_err(|_| invalid_field(RecordKind::Evidence, 3))
}

fn decode_provenance_endpoint(
    field: u32,
    bytes: &[u8],
    _limits: &DecoderLimits,
) -> Result<ProvenanceEndpointRef, RecordCodecError> {
    let reference = crate::wire_records::decode_record_ref(bytes)?;
    ProvenanceEndpointRef::try_from(reference)
        .map_err(|_| invalid_field(RecordKind::Provenance, field))
}

pub(super) fn encode_source(value: &Source) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Source,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_symbol(value.source_kind())),
            (
                3,
                optional(
                    value
                        .locator()
                        .map(|locator| encode_string(locator.as_str())),
                ),
            ),
            (
                4,
                optional(
                    value
                        .content_digest()
                        .map(|digest| digest.as_bytes().as_slice().to_vec()),
                ),
            ),
            (5, encode_metadata(value.metadata())?),
            (6, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_source(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Source, RecordCodecError> {
    let kind = RecordKind::Source;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let source_kind = decode_symbol(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let locator = read_optional(kind, 3, required_field(kind, &fields, 3)?)?
        .map(|bytes| {
            let value = decode_string_with_limits(bytes, limits).map_err(RecordCodecError::Wire)?;
            SourceLocator::new(value).map_err(|_| invalid_field(kind, 3))
        })
        .transpose()?;
    let content_digest = read_optional(kind, 4, required_field(kind, &fields, 4)?)?
        .map(|bytes| {
            if bytes.len() > limits.max_string_or_bytes {
                return Err(RecordCodecError::Wire(WireError::ResourceLimitExceeded {
                    resource: DecodeResource::StringOrBytes,
                    limit: limits.max_string_or_bytes,
                    actual: bytes.len(),
                }));
            }
            SourceContentDigest::new(Bytes::new(bytes.to_vec())).map_err(|_| invalid_field(kind, 4))
        })
        .transpose()?;
    let metadata = decode_metadata(required_field(kind, &fields, 5)?, limits)?;
    let created_revision = decode_revision(kind, 6, required_field(kind, &fields, 6)?)?;
    Ok(Source::new(
        id,
        source_kind,
        locator,
        content_digest,
        metadata,
        created_revision,
    ))
}

fn evidence_relation(value: EvidenceRelation) -> u8 {
    match value {
        EvidenceRelation::Supports => 1,
        EvidenceRelation::Contradicts => 2,
        EvidenceRelation::Documents => 3,
    }
}

fn decode_evidence_relation(
    bytes: &[u8],
    _limits: &DecoderLimits,
) -> Result<EvidenceRelation, RecordCodecError> {
    match bytes {
        [1] => Ok(EvidenceRelation::Supports),
        [2] => Ok(EvidenceRelation::Contradicts),
        [3] => Ok(EvidenceRelation::Documents),
        _ => Err(invalid_field(RecordKind::Evidence, 4)),
    }
}

pub(super) fn encode_evidence(value: &Evidence) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Evidence,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.source_id()).to_vec()),
            (3, encode_evidence_target(value.target())?),
            (4, vec![evidence_relation(value.relation())]),
            (5, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_evidence(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Evidence, RecordCodecError> {
    let kind = RecordKind::Evidence;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let source_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let target = decode_evidence_target(required_field(kind, &fields, 3)?, limits)?;
    let relation = decode_evidence_relation(required_field(kind, &fields, 4)?, limits)?;
    let created_revision = decode_revision(kind, 5, required_field(kind, &fields, 5)?)?;
    Ok(Evidence::new(
        id,
        source_id,
        target,
        relation,
        created_revision,
    ))
}

fn provenance_relation(value: ProvenanceRelation) -> u8 {
    match value {
        ProvenanceRelation::Corrects => 1,
        ProvenanceRelation::DerivedFrom => 2,
        ProvenanceRelation::ResultedFrom => 3,
    }
}

fn decode_provenance_relation(
    bytes: &[u8],
    _limits: &DecoderLimits,
) -> Result<ProvenanceRelation, RecordCodecError> {
    match bytes {
        [1] => Ok(ProvenanceRelation::Corrects),
        [2] => Ok(ProvenanceRelation::DerivedFrom),
        [3] => Ok(ProvenanceRelation::ResultedFrom),
        _ => Err(invalid_field(RecordKind::Provenance, 4)),
    }
}

pub(super) fn encode_provenance(value: &ProvenanceEdge) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Provenance,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (
                2,
                crate::wire_records::encode_record_ref(provenance_endpoint_to_record_ref(
                    value.from(),
                ))?,
            ),
            (
                3,
                crate::wire_records::encode_record_ref(provenance_endpoint_to_record_ref(
                    value.to(),
                ))?,
            ),
            (4, vec![provenance_relation(value.relation())]),
            (5, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_provenance(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ProvenanceEdge, RecordCodecError> {
    let kind = RecordKind::Provenance;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let from = decode_provenance_endpoint(2, required_field(kind, &fields, 2)?, limits)?;
    let to = decode_provenance_endpoint(3, required_field(kind, &fields, 3)?, limits)?;
    let relation = decode_provenance_relation(required_field(kind, &fields, 4)?, limits)?;
    let created_revision = decode_revision(kind, 5, required_field(kind, &fields, 5)?)?;
    ProvenanceEdge::new(id, from, to, relation, created_revision)
        .map_err(|_| invalid_field(kind, 4))
}

pub(super) fn encode_evidence_retraction(
    value: &EvidenceRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EvidenceRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.evidence_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_evidence_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EvidenceRetraction, RecordCodecError> {
    let kind = RecordKind::EvidenceRetraction;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let evidence_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let created_revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(EvidenceRetraction::from_wire_fields(
        id,
        evidence_id,
        reason,
        created_revision,
    ))
}

pub(super) fn encode_provenance_retraction(
    value: &ProvenanceRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::ProvenanceRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.provenance_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_provenance_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ProvenanceRetraction, RecordCodecError> {
    let kind = RecordKind::ProvenanceRetraction;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let provenance_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let created_revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(ProvenanceRetraction::from_wire_fields(
        id,
        provenance_id,
        reason,
        created_revision,
    ))
}

pub(super) fn encode_transfer_lineage(value: TransferLineage) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::TransferLineage,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.source_history_space_id()).to_vec()),
            (3, encode_id(value.target_history_space_id()).to_vec()),
            (
                4,
                crate::wire_records::encode_record_ref(value.source().record_ref())?,
            ),
            (
                5,
                crate::wire_records::encode_record_ref(value.target().record_ref())?,
            ),
            (6, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_transfer_lineage(
    payload: &[u8],
    limits: &DecoderLimits,
) -> Result<TransferLineage, RecordCodecError> {
    let kind = RecordKind::TransferLineage;
    let fields = decode_fields_with_limits(kind, payload, &[1, 2, 3, 4, 5, 6], limits)?;
    let id = decode_id::<TransferLineageId>(required_field(kind, &fields, 1)?)
        .map_err(RecordCodecError::Wire)?;
    let source_space =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let target_space =
        decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let source_ref = crate::wire_records::decode_record_ref(required_field(kind, &fields, 4)?)?;
    let target_ref = crate::wire_records::decode_record_ref(required_field(kind, &fields, 5)?)?;
    let source =
        HistorySpaceContentRef::try_from(source_ref).map_err(|_| invalid_field(kind, 4))?;
    let target =
        HistorySpaceContentRef::try_from(target_ref).map_err(|_| invalid_field(kind, 5))?;
    let revision = decode_revision(kind, 6, required_field(kind, &fields, 6)?)?;
    TransferLineage::new(id, source_space, target_space, source, target, revision)
        .map_err(|_| invalid_field(kind, 5))
}
