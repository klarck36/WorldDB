//! Assertion, mask, boundary, archive, and lifecycle record payloads.

use crate::ids::*;
use crate::wire::{
    DecoderLimits, TlvDecoder, TlvEncoder, WireError, decode_id, decode_value_with_limits,
    encode_id, encode_value,
};
use crate::{
    ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition, Assertion, AssertionDraft,
    AssertionRetraction, AssertionValidity, AssertionValidityClosure, ContextKey, EpistemicMode,
    Mask, MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure, PerspectiveScope,
    Polarity, PropositionKey, RecordRefWireTag, ReplacementBoundary, ReplacementBoundaryRetraction,
    ReplacementBoundaryValidityClosure, Subject, TimeInterval, Timeline, WorldTime,
};

use super::{
    RecordCodecError, RecordKind, decode_fields_with_limits, decode_revision,
    decode_string_with_limits, encode_fields, encode_revision, encode_string, invalid_field,
    required_field,
};

pub(super) fn nested(fields: Vec<(u32, Vec<u8>)>) -> Result<Vec<u8>, RecordCodecError> {
    let mut encoder = TlvEncoder::new();
    for (tag, value) in fields {
        encoder.push(tag, &value).map_err(RecordCodecError::Wire)?;
    }
    Ok(encoder.finish())
}

pub(super) fn nested_fields_with_limits<'a>(
    kind: RecordKind,
    parent_field: u32,
    bytes: &'a [u8],
    allowed: &[u32],
    limits: &DecoderLimits,
) -> Result<Vec<(u32, &'a [u8])>, RecordCodecError> {
    let mut decoder = TlvDecoder::with_limits(bytes, *limits);
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
                resource: crate::wire::DecodeResource::FieldsPerRecord,
            })
        })?;
        fields.push((field.tag(), field.value()));
    }
    if fields.is_empty() {
        return Err(invalid_field(kind, parent_field));
    }
    Ok(fields)
}

pub(super) fn nested_required<'a>(
    kind: RecordKind,
    parent_field: u32,
    fields: &[(u32, &'a [u8])],
    tag: u32,
) -> Result<&'a [u8], RecordCodecError> {
    fields
        .iter()
        .find_map(|(actual, value)| (*actual == tag).then_some(*value))
        .ok_or_else(|| invalid_field(kind, parent_field))
}

pub(super) fn optional(value: Option<Vec<u8>>) -> Vec<u8> {
    match value {
        None => vec![0],
        Some(value) => {
            let mut bytes = vec![1];
            bytes.extend(value);
            bytes
        }
    }
}

pub(super) fn read_optional(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<Option<&[u8]>, RecordCodecError> {
    match bytes {
        [0] => Ok(None),
        [1, value @ ..] => Ok(Some(value)),
        _ => Err(invalid_field(kind, field)),
    }
}

fn scope(value: PerspectiveScope) -> Vec<u8> {
    match value {
        PerspectiveScope::World => vec![1],
        PerspectiveScope::Perspective(id) => {
            let mut bytes = vec![2];
            bytes.extend(encode_id(id));
            bytes
        }
    }
}

fn read_scope(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<PerspectiveScope, RecordCodecError> {
    match bytes {
        [1] => Ok(PerspectiveScope::World),
        [2, id @ ..] => decode_id(id)
            .map(PerspectiveScope::Perspective)
            .map_err(|_| invalid_field(kind, field)),
        _ => Err(invalid_field(kind, field)),
    }
}

fn epistemic(value: EpistemicMode) -> u8 {
    match value {
        EpistemicMode::WorldState => 1,
        EpistemicMode::Knows => 2,
        EpistemicMode::Believes => 3,
        EpistemicMode::Claims => 4,
    }
}

fn read_epistemic(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<EpistemicMode, RecordCodecError> {
    match bytes {
        [1] => Ok(EpistemicMode::WorldState),
        [2] => Ok(EpistemicMode::Knows),
        [3] => Ok(EpistemicMode::Believes),
        [4] => Ok(EpistemicMode::Claims),
        _ => Err(invalid_field(kind, field)),
    }
}

fn encode_context(context: ContextKey) -> Result<Vec<u8>, RecordCodecError> {
    nested(vec![
        (1, encode_id(context.history_space_id()).to_vec()),
        (2, encode_id(context.layer_id()).to_vec()),
        (3, scope(context.perspective_scope())),
        (4, vec![epistemic(context.epistemic_mode())]),
    ])
}

fn decode_context(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ContextKey, RecordCodecError> {
    let fields = nested_fields_with_limits(kind, field, bytes, &[1, 2, 3, 4], limits)?;
    let history_space_id = decode_id(nested_required(kind, field, &fields, 1)?)
        .map_err(|_| invalid_field(kind, field))?;
    let layer_id = decode_id(nested_required(kind, field, &fields, 2)?)
        .map_err(|_| invalid_field(kind, field))?;
    let perspective_scope = read_scope(kind, field, nested_required(kind, field, &fields, 3)?)?;
    let epistemic_mode = read_epistemic(kind, field, nested_required(kind, field, &fields, 4)?)?;
    ContextKey::new(
        history_space_id,
        layer_id,
        perspective_scope,
        epistemic_mode,
    )
    .map_err(|_| invalid_field(kind, field))
}

pub(super) fn encode_world_time(value: WorldTime) -> Result<Vec<u8>, RecordCodecError> {
    nested(vec![
        (1, encode_id(value.timeline().id()).to_vec()),
        (2, crate::Int::new(value.nanoseconds()).to_canonical_bytes()),
    ])
}

pub(super) fn decode_world_time(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<WorldTime, RecordCodecError> {
    let fields = nested_fields_with_limits(kind, field, bytes, &[1, 2], limits)?;
    let timeline_id = decode_id(nested_required(kind, field, &fields, 1)?)
        .map_err(|_| invalid_field(kind, field))?;
    let nanoseconds = crate::Int::from_canonical_bytes(nested_required(kind, field, &fields, 2)?)
        .map_err(|_| invalid_field(kind, field))?
        .value();
    Ok(WorldTime::from_nanoseconds(
        Timeline::new(timeline_id),
        nanoseconds,
    ))
}

fn encode_validity(value: AssertionValidity) -> Result<Vec<u8>, RecordCodecError> {
    let interval = value.interval();
    nested(vec![
        (1, encode_id(interval.timeline().id()).to_vec()),
        (
            2,
            optional(interval.start().map(encode_world_time).transpose()?),
        ),
        (
            3,
            optional(interval.end().map(encode_world_time).transpose()?),
        ),
    ])
}

fn decode_validity(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<AssertionValidity, RecordCodecError> {
    let fields = nested_fields_with_limits(kind, field, bytes, &[1, 2, 3], limits)?;
    let timeline_id = decode_id(nested_required(kind, field, &fields, 1)?)
        .map_err(|_| invalid_field(kind, field))?;
    let start = read_optional(kind, field, nested_required(kind, field, &fields, 2)?)?
        .map(|bytes| decode_world_time(kind, field, bytes, limits))
        .transpose()?;
    let end = read_optional(kind, field, nested_required(kind, field, &fields, 3)?)?
        .map(|bytes| decode_world_time(kind, field, bytes, limits))
        .transpose()?;
    let interval = TimeInterval::new(Timeline::new(timeline_id), start, end)
        .map_err(|_| invalid_field(kind, field))?;
    Ok(AssertionValidity::new(interval))
}

fn polarity(value: Polarity) -> u8 {
    match value {
        Polarity::Positive => 1,
        Polarity::Negative => 2,
    }
}

fn read_polarity(kind: RecordKind, field: u32, bytes: &[u8]) -> Result<Polarity, RecordCodecError> {
    match bytes {
        [1] => Ok(Polarity::Positive),
        [2] => Ok(Polarity::Negative),
        _ => Err(invalid_field(kind, field)),
    }
}

fn encode_optional_validity(value: Option<AssertionValidity>) -> Result<Vec<u8>, RecordCodecError> {
    Ok(optional(value.map(encode_validity).transpose()?))
}

fn decode_optional_validity(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Option<AssertionValidity>, RecordCodecError> {
    read_optional(kind, field, bytes)?
        .map(|bytes| decode_validity(kind, field, bytes, limits))
        .transpose()
}

fn encode_scope_slot(slot: MaskSlotSelector) -> Result<Vec<u8>, RecordCodecError> {
    nested(vec![
        (1, encode_id(slot.subject().entity_id()).to_vec()),
        (2, encode_id(slot.predicate_id()).to_vec()),
        (3, scope(slot.perspective_scope())),
        (4, vec![epistemic(slot.epistemic_mode())]),
    ])
}

fn encode_selector(selector: &MaskSelector) -> Result<Vec<u8>, RecordCodecError> {
    match selector {
        MaskSelector::ExactAssertion(id) => {
            let mut bytes = vec![1];
            bytes.extend(encode_id(*id));
            Ok(bytes)
        }
        MaskSelector::Proposition(key) => {
            let body = nested(vec![
                (1, encode_id(key.subject().entity_id()).to_vec()),
                (2, encode_id(key.predicate_id()).to_vec()),
                (3, encode_value(key.value())),
                (4, vec![polarity(key.polarity())]),
            ])?;
            let mut bytes = vec![2];
            bytes.extend(body);
            Ok(bytes)
        }
        MaskSelector::Slot(slot) => {
            let mut bytes = vec![3];
            bytes.extend(encode_scope_slot(*slot)?);
            Ok(bytes)
        }
    }
}

pub(super) fn decode_selector(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MaskSelector, RecordCodecError> {
    let (tag, body) = bytes
        .split_first()
        .ok_or_else(|| invalid_field(kind, field))?;
    match tag {
        1 => decode_id(body)
            .map(MaskSelector::ExactAssertion)
            .map_err(|_| invalid_field(kind, field)),
        2 => {
            let fields = nested_fields_with_limits(kind, field, body, &[1, 2, 3, 4], limits)?;
            let subject = decode_id(nested_required(kind, field, &fields, 1)?)
                .map_err(|_| invalid_field(kind, field))?;
            let predicate_id = decode_id(nested_required(kind, field, &fields, 2)?)
                .map_err(|_| invalid_field(kind, field))?;
            let value = decode_value_with_limits(nested_required(kind, field, &fields, 3)?, limits)
                .map_err(RecordCodecError::Wire)?;
            let polarity = read_polarity(kind, field, nested_required(kind, field, &fields, 4)?)?;
            Ok(MaskSelector::Proposition(PropositionKey::new(
                Subject::new(subject),
                predicate_id,
                value,
                polarity,
            )))
        }
        3 => {
            let fields = nested_fields_with_limits(kind, field, body, &[1, 2, 3, 4], limits)?;
            let subject = decode_id(nested_required(kind, field, &fields, 1)?)
                .map_err(|_| invalid_field(kind, field))?;
            let predicate_id = decode_id(nested_required(kind, field, &fields, 2)?)
                .map_err(|_| invalid_field(kind, field))?;
            let perspective_scope =
                read_scope(kind, field, nested_required(kind, field, &fields, 3)?)?;
            let epistemic_mode =
                read_epistemic(kind, field, nested_required(kind, field, &fields, 4)?)?;
            let slot = MaskSlotSelector::new(
                Subject::new(subject),
                predicate_id,
                perspective_scope,
                epistemic_mode,
            )
            .map_err(|_| invalid_field(kind, field))?;
            Ok(MaskSelector::Slot(slot))
        }
        _ => Err(invalid_field(kind, field)),
    }
}

pub(super) fn encode_assertion(value: &Assertion) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Assertion,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_context(value.context())?),
            (3, encode_id(value.subject().entity_id()).to_vec()),
            (4, encode_id(value.predicate_id()).to_vec()),
            (5, encode_value(value.value())),
            (6, vec![polarity(value.polarity())]),
            (7, encode_validity(value.validity())?),
            (8, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_assertion(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Assertion, RecordCodecError> {
    let kind = RecordKind::Assertion;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6, 7, 8], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let context = decode_context(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let subject = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let predicate_id =
        decode_id(required_field(kind, &fields, 4)?).map_err(RecordCodecError::Wire)?;
    let value = decode_value_with_limits(required_field(kind, &fields, 5)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let polarity = read_polarity(kind, 6, required_field(kind, &fields, 6)?)?;
    let validity = decode_validity(kind, 7, required_field(kind, &fields, 7)?, limits)?;
    let revision = decode_revision(kind, 8, required_field(kind, &fields, 8)?)?;
    Ok(Assertion::new(
        id,
        AssertionDraft::new(
            context,
            Subject::new(subject),
            predicate_id,
            value,
            polarity,
            validity,
        ),
        revision,
    ))
}

pub(super) fn encode_assertion_validity_closure(
    value: &AssertionValidityClosure,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::AssertionValidityClosure,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.assertion_id()).to_vec()),
            (3, encode_world_time(value.close_at_world_time())?),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_assertion_validity_closure(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<AssertionValidityClosure, RecordCodecError> {
    let kind = RecordKind::AssertionValidityClosure;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let assertion_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let close_at = decode_world_time(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(AssertionValidityClosure::from_wire_fields(
        id,
        assertion_id,
        close_at,
        revision,
    ))
}

pub(super) fn encode_assertion_retraction(
    value: &AssertionRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::AssertionRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.assertion_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_assertion_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<AssertionRetraction, RecordCodecError> {
    let kind = RecordKind::AssertionRetraction;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let assertion_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(|_| invalid_field(kind, 3))?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(AssertionRetraction::from_wire_fields(
        id,
        assertion_id,
        reason,
        revision,
    ))
}

pub(super) fn encode_mask(value: &Mask) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Mask,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_context(value.context())?),
            (3, encode_selector(value.selector())?),
            (4, encode_optional_validity(value.validity())?),
            (5, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_mask(bytes: &[u8], limits: &DecoderLimits) -> Result<Mask, RecordCodecError> {
    let kind = RecordKind::Mask;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let context = decode_context(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let selector = decode_selector(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let validity = decode_optional_validity(kind, 4, required_field(kind, &fields, 4)?, limits)?;
    let revision = decode_revision(kind, 5, required_field(kind, &fields, 5)?)?;
    Mask::new(id, context, selector, validity, revision).map_err(|_| invalid_field(kind, 3))
}

pub(super) fn encode_mask_validity_closure(
    value: &MaskValidityClosure,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::MaskValidityClosure,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.mask_id()).to_vec()),
            (3, encode_world_time(value.close_at_world_time())?),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_mask_validity_closure(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MaskValidityClosure, RecordCodecError> {
    let kind = RecordKind::MaskValidityClosure;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let mask_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let close_at = decode_world_time(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(MaskValidityClosure::from_wire_fields(
        id, mask_id, close_at, revision,
    ))
}

pub(super) fn encode_mask_retraction(value: &MaskRetraction) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::MaskRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.mask_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_mask_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MaskRetraction, RecordCodecError> {
    let kind = RecordKind::MaskRetraction;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let mask_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(|_| invalid_field(kind, 3))?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(MaskRetraction::from_wire_fields(
        id, mask_id, reason, revision,
    ))
}

pub(super) fn encode_replacement_boundary(
    value: &ReplacementBoundary,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::ReplacementBoundary,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_context(value.context())?),
            (3, encode_id(value.subject().entity_id()).to_vec()),
            (4, encode_id(value.predicate_id()).to_vec()),
            (5, encode_optional_validity(value.validity())?),
            (6, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_replacement_boundary(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ReplacementBoundary, RecordCodecError> {
    let kind = RecordKind::ReplacementBoundary;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let context = decode_context(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let subject = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let predicate_id =
        decode_id(required_field(kind, &fields, 4)?).map_err(RecordCodecError::Wire)?;
    let validity = decode_optional_validity(kind, 5, required_field(kind, &fields, 5)?, limits)?;
    let revision = decode_revision(kind, 6, required_field(kind, &fields, 6)?)?;
    Ok(ReplacementBoundary::from_wire_fields(
        id,
        context,
        Subject::new(subject),
        predicate_id,
        validity,
        revision,
    ))
}

pub(super) fn encode_boundary_validity_closure(
    value: &ReplacementBoundaryValidityClosure,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::ReplacementBoundaryValidityClosure,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.replacement_boundary_id()).to_vec()),
            (3, encode_world_time(value.close_at_world_time())?),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_boundary_validity_closure(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ReplacementBoundaryValidityClosure, RecordCodecError> {
    let kind = RecordKind::ReplacementBoundaryValidityClosure;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let boundary_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let close_at = decode_world_time(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(ReplacementBoundaryValidityClosure::from_wire_fields(
        id,
        boundary_id,
        close_at,
        revision,
    ))
}

pub(super) fn encode_boundary_retraction(
    value: &ReplacementBoundaryRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::ReplacementBoundaryRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.replacement_boundary_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_boundary_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ReplacementBoundaryRetraction, RecordCodecError> {
    let kind = RecordKind::ReplacementBoundaryRetraction;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let boundary_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(|_| invalid_field(kind, 3))?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(ReplacementBoundaryRetraction::from_wire_fields(
        id,
        boundary_id,
        reason,
        revision,
    ))
}

fn archive_target_id(target: ArchiveTargetRef) -> (u16, [u8; 16]) {
    match target {
        ArchiveTargetRef::Assertion(id) => (1, encode_id(id)),
        ArchiveTargetRef::Mask(id) => (2, encode_id(id)),
        ArchiveTargetRef::ReplacementBoundary(id) => (3, encode_id(id)),
        ArchiveTargetRef::Event(id) => (4, encode_id(id)),
        ArchiveTargetRef::EventMask(id) => (5, encode_id(id)),
        ArchiveTargetRef::EventRelation(id) => (6, encode_id(id)),
        ArchiveTargetRef::Source(id) => (7, encode_id(id)),
        ArchiveTargetRef::Evidence(id) => (8, encode_id(id)),
        ArchiveTargetRef::Provenance(id) => (9, encode_id(id)),
        ArchiveTargetRef::AssertionValidityClosure(id) => (10, encode_id(id)),
        ArchiveTargetRef::AssertionRetraction(id) => (11, encode_id(id)),
        ArchiveTargetRef::MaskValidityClosure(id) => (12, encode_id(id)),
        ArchiveTargetRef::MaskRetraction(id) => (13, encode_id(id)),
        ArchiveTargetRef::ReplacementBoundaryValidityClosure(id) => (14, encode_id(id)),
        ArchiveTargetRef::ReplacementBoundaryRetraction(id) => (15, encode_id(id)),
        ArchiveTargetRef::EventSpanClosure(id) => (16, encode_id(id)),
        ArchiveTargetRef::EventRetraction(id) => (17, encode_id(id)),
        ArchiveTargetRef::EventMaskRetraction(id) => (18, encode_id(id)),
        ArchiveTargetRef::EventRelationRetraction(id) => (19, encode_id(id)),
        ArchiveTargetRef::EvidenceRetraction(id) => (20, encode_id(id)),
        ArchiveTargetRef::ProvenanceRetraction(id) => (21, encode_id(id)),
        ArchiveTargetRef::EntityRetirement(id) => (22, encode_id(id)),
        ArchiveTargetRef::PerspectiveRetirement(id) => (23, encode_id(id)),
    }
}

pub(super) fn encode_archive_target(target: ArchiveTargetRef) -> Vec<u8> {
    let (tag, id) = archive_target_id(target);
    let mut bytes = crate::numbers::encode_u128_varint(u128::from(tag));
    bytes.extend_from_slice(&id);
    bytes
}

pub(super) fn decode_archive_target(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    _limits: &DecoderLimits,
) -> Result<ArchiveTargetRef, RecordCodecError> {
    let (raw_tag, tag_len) = crate::numbers::decode_u128_varint_prefix(bytes)
        .map_err(|error| RecordCodecError::Wire(WireError::Varint { offset: 0, error }))?;
    let tag = u16::try_from(raw_tag).map_err(|_| invalid_field(kind, field))?;
    let reference_tag = RecordRefWireTag::try_from(tag)
        .map_err(|_| RecordCodecError::UnknownRecordRefTag { tag })?;
    let id_bytes = bytes
        .get(tag_len..)
        .ok_or(RecordCodecError::Wire(WireError::Truncated {
            offset: tag_len,
        }))?;
    if id_bytes.len() != 16 {
        return Err(invalid_field(kind, field));
    }
    macro_rules! typed_target {
        ($id:ty, $variant:ident) => {
            decode_id::<$id>(id_bytes)
                .map(ArchiveTargetRef::$variant)
                .map_err(|_| invalid_field(kind, field))
        };
    }
    match reference_tag {
        RecordRefWireTag::Assertion => typed_target!(AssertionId, Assertion),
        RecordRefWireTag::Mask => typed_target!(MaskId, Mask),
        RecordRefWireTag::ReplacementBoundary => {
            typed_target!(ReplacementBoundaryId, ReplacementBoundary)
        }
        RecordRefWireTag::Event => typed_target!(EventId, Event),
        RecordRefWireTag::EventMask => typed_target!(EventMaskId, EventMask),
        RecordRefWireTag::EventRelation => typed_target!(EventRelationId, EventRelation),
        RecordRefWireTag::Source => typed_target!(SourceId, Source),
        RecordRefWireTag::Evidence => typed_target!(EvidenceId, Evidence),
        RecordRefWireTag::Provenance => typed_target!(ProvenanceId, Provenance),
        RecordRefWireTag::AssertionValidityClosure => {
            typed_target!(AssertionValidityClosureId, AssertionValidityClosure)
        }
        RecordRefWireTag::AssertionRetraction => {
            typed_target!(AssertionRetractionId, AssertionRetraction)
        }
        RecordRefWireTag::MaskValidityClosure => {
            typed_target!(MaskValidityClosureId, MaskValidityClosure)
        }
        RecordRefWireTag::MaskRetraction => typed_target!(MaskRetractionId, MaskRetraction),
        RecordRefWireTag::ReplacementBoundaryValidityClosure => {
            typed_target!(
                ReplacementBoundaryValidityClosureId,
                ReplacementBoundaryValidityClosure
            )
        }
        RecordRefWireTag::ReplacementBoundaryRetraction => {
            typed_target!(
                ReplacementBoundaryRetractionId,
                ReplacementBoundaryRetraction
            )
        }
        RecordRefWireTag::EventSpanClosure => {
            typed_target!(EventSpanClosureId, EventSpanClosure)
        }
        RecordRefWireTag::EventRetraction => typed_target!(EventRetractionId, EventRetraction),
        RecordRefWireTag::EventMaskRetraction => {
            typed_target!(EventMaskRetractionId, EventMaskRetraction)
        }
        RecordRefWireTag::EventRelationRetraction => {
            typed_target!(EventRelationRetractionId, EventRelationRetraction)
        }
        RecordRefWireTag::EvidenceRetraction => {
            typed_target!(EvidenceRetractionId, EvidenceRetraction)
        }
        RecordRefWireTag::ProvenanceRetraction => {
            typed_target!(ProvenanceRetractionId, ProvenanceRetraction)
        }
        RecordRefWireTag::EntityRetirement => {
            typed_target!(EntityRetirementId, EntityRetirement)
        }
        RecordRefWireTag::PerspectiveRetirement => {
            typed_target!(PerspectiveRetirementId, PerspectiveRetirement)
        }
        RecordRefWireTag::ArchiveTransition => Err(invalid_field(kind, field)),
    }
}

pub(super) fn encode_archive_transition(
    value: &ArchiveTransition,
) -> Result<Vec<u8>, RecordCodecError> {
    let action = match value.action() {
        ArchiveAction::Archive => 1,
        ArchiveAction::Unarchive => 2,
    };
    encode_fields(
        RecordKind::ArchiveTransition,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_archive_target(value.target())),
            (3, vec![action]),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_archive_transition(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ArchiveTransition, RecordCodecError> {
    let kind = RecordKind::ArchiveTransition;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let target = decode_archive_target(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let action = match required_field(kind, &fields, 3)? {
        [1] => ArchiveAction::Archive,
        [2] => ArchiveAction::Unarchive,
        _ => return Err(invalid_field(kind, 3)),
    };
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    let prior_state = match action {
        ArchiveAction::Archive => ArchiveState::Unarchived,
        ArchiveAction::Unarchive => ArchiveState::Archived,
    };
    ArchiveTransition::new(id, target, action, prior_state, revision)
        .map_err(|_| invalid_field(kind, 3))
}
