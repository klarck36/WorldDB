//! Event, EventMask, EventRelation, and event lifecycle record payloads.

use crate::events::{EventAttributes, EventWireFields, Participants};
use crate::wire::{DecoderLimits, decode_id, decode_value_with_limits, encode_id, encode_value};
use crate::{
    Event, EventAttributeValue, EventMask, EventMaskRetraction, EventParticipant, EventRelation,
    EventRelationInputKind, EventRelationRetraction, EventRetraction, EventSpanClosure, EventTime,
};

use super::{
    RecordCodecError, RecordKind, decode_array_with_limits, decode_revision,
    decode_string_with_limits, encode_array, encode_fields, encode_revision, encode_string,
    invalid_field, lifecycle, required_field, reserve_collection,
};

fn encode_event_time(value: EventTime) -> Result<Vec<u8>, RecordCodecError> {
    match value {
        EventTime::Instant(time) => {
            let mut bytes = vec![1];
            bytes.extend(lifecycle::encode_world_time(time)?);
            Ok(bytes)
        }
        EventTime::Span { start, end } => {
            let fields = vec![
                (1, lifecycle::encode_world_time(start)?),
                (
                    2,
                    lifecycle::optional(end.map(lifecycle::encode_world_time).transpose()?),
                ),
            ];
            let mut bytes = vec![2];
            bytes.extend(lifecycle::nested(fields)?);
            Ok(bytes)
        }
    }
}

pub(super) fn decode_event_time(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventTime, RecordCodecError> {
    let (tag, body) = bytes
        .split_first()
        .ok_or_else(|| invalid_field(kind, field))?;
    match tag {
        1 => lifecycle::decode_world_time(kind, field, body, limits).map(EventTime::Instant),
        2 => {
            let fields = lifecycle::nested_fields_with_limits(kind, field, body, &[1, 2], limits)?;
            let start = lifecycle::decode_world_time(
                kind,
                field,
                lifecycle::nested_required(kind, field, &fields, 1)?,
                limits,
            )?;
            let end = lifecycle::read_optional(
                kind,
                field,
                lifecycle::nested_required(kind, field, &fields, 2)?,
            )?
            .map(|bytes| lifecycle::decode_world_time(kind, field, bytes, limits))
            .transpose()?;
            match end {
                Some(end) => {
                    EventTime::span(start, Some(end)).map_err(|_| invalid_field(kind, field))
                }
                None => Ok(EventTime::Span { start, end: None }),
            }
        }
        _ => Err(invalid_field(kind, field)),
    }
}

fn encode_participants(value: &Participants) -> Result<Vec<u8>, RecordCodecError> {
    let mut items = Vec::new();
    for participant in value.as_slice() {
        items.push(lifecycle::nested(vec![
            (1, encode_id(participant.role_id()).to_vec()),
            (2, encode_id(participant.entity_id()).to_vec()),
        ])?);
    }
    Ok(encode_array(&items))
}

pub(super) fn decode_participants(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Participants, RecordCodecError> {
    let items = decode_array_with_limits(bytes, limits).map_err(RecordCodecError::Wire)?;
    let mut participants =
        reserve_collection(items.len(), limits).map_err(RecordCodecError::Wire)?;
    for item in items {
        let fields = lifecycle::nested_fields_with_limits(kind, field, item, &[1, 2], limits)?;
        let role_id = decode_id(lifecycle::nested_required(kind, field, &fields, 1)?)
            .map_err(RecordCodecError::Wire)?;
        let entity_id = decode_id(lifecycle::nested_required(kind, field, &fields, 2)?)
            .map_err(RecordCodecError::Wire)?;
        participants.push(EventParticipant::new(role_id, entity_id));
    }
    if participants
        .windows(2)
        .any(|pair| matches!(pair, [left, right] if left >= right))
    {
        return Err(RecordCodecError::NonCanonicalRecord);
    }
    Ok(Participants::from_wire_fields(participants))
}

fn encode_attributes(value: &EventAttributes) -> Result<Vec<u8>, RecordCodecError> {
    let mut items = Vec::new();
    for attribute in value.as_slice() {
        items.push(lifecycle::nested(vec![
            (1, encode_id(attribute.attribute_id()).to_vec()),
            (2, encode_value(attribute.value())),
        ])?);
    }
    Ok(encode_array(&items))
}

pub(super) fn decode_attributes(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventAttributes, RecordCodecError> {
    let items = decode_array_with_limits(bytes, limits).map_err(RecordCodecError::Wire)?;
    let mut attributes = reserve_collection(items.len(), limits).map_err(RecordCodecError::Wire)?;
    for item in items {
        let fields = lifecycle::nested_fields_with_limits(kind, field, item, &[1, 2], limits)?;
        let attribute_id = decode_id(lifecycle::nested_required(kind, field, &fields, 1)?)
            .map_err(RecordCodecError::Wire)?;
        let value =
            decode_value_with_limits(lifecycle::nested_required(kind, field, &fields, 2)?, limits)
                .map_err(RecordCodecError::Wire)?;
        attributes.push(EventAttributeValue::new(attribute_id, value));
    }
    if attributes
        .windows(2)
        .any(|pair| matches!(pair, [left, right] if left.attribute_id() >= right.attribute_id()))
    {
        return Err(RecordCodecError::NonCanonicalRecord);
    }
    Ok(EventAttributes::from_wire_fields(attributes))
}

pub(super) fn encode_event(value: &Event) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Event,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.history_space_id()).to_vec()),
            (3, encode_id(value.layer_id()).to_vec()),
            (4, encode_id(value.event_kind_id()).to_vec()),
            (5, encode_participants(value.participants())?),
            (6, encode_attributes(value.attributes())?),
            (7, encode_event_time(value.event_time())?),
            (8, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Event, RecordCodecError> {
    let kind = RecordKind::Event;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6, 7, 8], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let history_space_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let layer_id = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let event_kind_id =
        decode_id(required_field(kind, &fields, 4)?).map_err(RecordCodecError::Wire)?;
    let participants = decode_participants(kind, 5, required_field(kind, &fields, 5)?, limits)?;
    let attributes = decode_attributes(kind, 6, required_field(kind, &fields, 6)?, limits)?;
    let event_time = decode_event_time(kind, 7, required_field(kind, &fields, 7)?, limits)?;
    let created_revision = decode_revision(kind, 8, required_field(kind, &fields, 8)?)?;
    Ok(Event::from_wire_fields(
        id,
        EventWireFields::new(
            history_space_id,
            layer_id,
            event_kind_id,
            participants,
            attributes,
            event_time,
        ),
        created_revision,
    ))
}

pub(super) fn encode_event_mask(value: EventMask) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EventMask,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.history_space_id()).to_vec()),
            (3, encode_id(value.layer_id()).to_vec()),
            (4, encode_id(value.target_event()).to_vec()),
            (5, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_mask(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventMask, RecordCodecError> {
    let kind = RecordKind::EventMask;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let history_space_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let layer_id = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let target_event =
        decode_id(required_field(kind, &fields, 4)?).map_err(RecordCodecError::Wire)?;
    let created_revision = decode_revision(kind, 5, required_field(kind, &fields, 5)?)?;
    Ok(EventMask::new(
        id,
        history_space_id,
        layer_id,
        target_event,
        created_revision,
    ))
}

pub(super) fn encode_event_span_closure(
    value: EventSpanClosure,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EventSpanClosure,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.event_id()).to_vec()),
            (
                3,
                lifecycle::encode_world_time(value.close_at_event_time())?,
            ),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_span_closure(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventSpanClosure, RecordCodecError> {
    let kind = RecordKind::EventSpanClosure;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let event_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let close_at =
        lifecycle::decode_world_time(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let created_revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(EventSpanClosure::from_wire_fields(
        id,
        event_id,
        close_at,
        created_revision,
    ))
}

pub(super) fn encode_event_retraction(
    value: &EventRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EventRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.event_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventRetraction, RecordCodecError> {
    let kind = RecordKind::EventRetraction;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let event_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(|_| invalid_field(kind, 3))?;
    let created_revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(EventRetraction::from_wire_fields(
        id,
        event_id,
        reason,
        created_revision,
    ))
}

pub(super) fn encode_event_mask_retraction(
    value: &EventMaskRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EventMaskRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.event_mask_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_mask_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventMaskRetraction, RecordCodecError> {
    let kind = RecordKind::EventMaskRetraction;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let event_mask_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(|_| invalid_field(kind, 3))?;
    let created_revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(EventMaskRetraction::from_wire_fields(
        id,
        event_mask_id,
        reason,
        created_revision,
    ))
}

pub(super) fn encode_event_relation(value: EventRelation) -> Result<Vec<u8>, RecordCodecError> {
    let relation_kind = match value.kind() {
        crate::EventRelationKind::Before => 1,
        crate::EventRelationKind::SameTime => 2,
        crate::EventRelationKind::Causes => 3,
    };
    encode_fields(
        RecordKind::EventRelation,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.from_event()).to_vec()),
            (3, encode_id(value.to_event()).to_vec()),
            (4, vec![relation_kind]),
            (5, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_relation(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventRelation, RecordCodecError> {
    let kind = RecordKind::EventRelation;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let from_event =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let to_event = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let input_kind = match required_field(kind, &fields, 4)? {
        [1] => EventRelationInputKind::Before,
        [2] => EventRelationInputKind::SameTime,
        [3] => EventRelationInputKind::Causes,
        _ => return Err(invalid_field(kind, 4)),
    };
    let created_revision = decode_revision(kind, 5, required_field(kind, &fields, 5)?)?;
    EventRelation::new(id, from_event, to_event, input_kind, created_revision)
        .map_err(|_| invalid_field(kind, 2))
}

pub(super) fn encode_event_relation_retraction(
    value: &EventRelationRetraction,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EventRelationRetraction,
        vec![
            (1, encode_id(value.id()).to_vec()),
            (2, encode_id(value.event_relation_id()).to_vec()),
            (3, encode_string(value.reason())),
            (4, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_relation_retraction(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventRelationRetraction, RecordCodecError> {
    let kind = RecordKind::EventRelationRetraction;
    let fields = super::decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let relation_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let reason = decode_string_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(|_| invalid_field(kind, 3))?;
    let created_revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    Ok(EventRelationRetraction::from_wire_fields(
        id,
        relation_id,
        reason,
        created_revision,
    ))
}
