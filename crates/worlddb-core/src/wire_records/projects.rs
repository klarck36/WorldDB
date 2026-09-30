//! Project and layer record wire payloads.

use crate::wire::DecoderLimits;
use crate::wire::{decode_id, encode_id};
use crate::{
    Entity, EntityRetirement, HistorySpaceDefinition, HistorySpaceError, LayerDefinition,
    LayerSchemaSnapshot, Lifecycle, PerspectiveCatalogError, PerspectiveDefinitionRevision,
    PerspectiveRetirement, RecordCodecError, RecordKind, Symbol,
};

use super::{
    collect_results_limited, decode_fields_with_limits, decode_revision, decode_schema_revision,
    decode_string_with_limits, encode_array, encode_fields, encode_revision,
    encode_schema_revision, encode_string, invalid_field, required_field,
};

fn encode_optional_text(value: Option<&str>) -> Vec<u8> {
    match value {
        None => vec![0],
        Some(value) => {
            let mut bytes = vec![1];
            bytes.extend(encode_string(value));
            bytes
        }
    }
}

fn decode_optional_text(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Option<String>, RecordCodecError> {
    match bytes {
        [0] => Ok(None),
        [1, rest @ ..] => decode_string_with_limits(rest, limits)
            .map(Some)
            .map_err(|_| invalid_field(kind, field)),
        _ => Err(invalid_field(kind, field)),
    }
}

fn encode_lifecycle(value: Lifecycle) -> Vec<u8> {
    vec![match value {
        Lifecycle::Active => 1,
        Lifecycle::Deprecated => 2,
        Lifecycle::Retired => 3,
    }]
}

fn decode_lifecycle(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    _limits: &DecoderLimits,
) -> Result<Lifecycle, RecordCodecError> {
    match bytes {
        [1] => Ok(Lifecycle::Active),
        [2] => Ok(Lifecycle::Deprecated),
        [3] => Ok(Lifecycle::Retired),
        _ => Err(invalid_field(kind, field)),
    }
}

pub(super) fn encode_history_space(
    value: &HistorySpaceDefinition,
) -> Result<Vec<u8>, RecordCodecError> {
    let parent = value
        .parent_history_space_id()
        .map(encode_id)
        .map_or_else(Vec::new, |id| id.to_vec());
    encode_fields(
        RecordKind::HistorySpaceDefinition,
        vec![
            (1, encode_id(value.history_space_id()).to_vec()),
            (2, parent),
            (3, encode_revision(value.base_revision())),
        ],
    )
}

pub(super) fn decode_history_space(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<HistorySpaceDefinition, RecordCodecError> {
    let kind = RecordKind::HistorySpaceDefinition;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let parent_bytes = required_field(kind, &fields, 2)?;
    let parent = if parent_bytes.is_empty() {
        None
    } else {
        Some(decode_id(parent_bytes).map_err(RecordCodecError::Wire)?)
    };
    let revision = decode_revision(kind, 3, required_field(kind, &fields, 3)?)?;
    HistorySpaceDefinition::new(id, parent, revision).map_err(|error| match error {
        HistorySpaceError::RootBaseRevisionNotGenesis => invalid_field(kind, 3),
        HistorySpaceError::SelfParent => invalid_field(kind, 2),
        _ => invalid_field(kind, 1),
    })
}

pub(super) fn encode_entity(value: &Entity) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::Entity,
        vec![
            (1, encode_id(value.entity_id()).to_vec()),
            (2, encode_id(value.entity_type_id()).to_vec()),
            (3, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_entity(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Entity, RecordCodecError> {
    let kind = RecordKind::Entity;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let entity_type =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let revision = decode_revision(kind, 3, required_field(kind, &fields, 3)?)?;
    Ok(Entity::new(id, entity_type, revision))
}

pub(super) fn encode_entity_retirement(
    value: &EntityRetirement,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EntityRetirement,
        vec![
            (1, encode_id(value.entity_retirement_id()).to_vec()),
            (2, encode_id(value.entity_id()).to_vec()),
            (3, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_entity_retirement(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EntityRetirement, RecordCodecError> {
    let kind = RecordKind::EntityRetirement;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let entity = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let revision = decode_revision(kind, 3, required_field(kind, &fields, 3)?)?;
    Ok(EntityRetirement::new(id, entity, revision))
}

pub(super) fn encode_perspective_definition(
    value: &PerspectiveDefinitionRevision,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::PerspectiveDefinitionRevision,
        vec![
            (1, encode_id(value.perspective_id()).to_vec()),
            (2, encode_optional_text(value.display_name())),
            (3, encode_optional_text(value.description())),
            (4, encode_revision(value.recorded_revision())),
        ],
    )
}

pub(super) fn decode_perspective_definition(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<PerspectiveDefinitionRevision, RecordCodecError> {
    let kind = RecordKind::PerspectiveDefinitionRevision;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let name = decode_optional_text(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let description = decode_optional_text(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let revision = decode_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    PerspectiveDefinitionRevision::new(id, name, description, revision).map_err(|error| match error
    {
        PerspectiveCatalogError::EmptyDisplayName => invalid_field(kind, 2),
        PerspectiveCatalogError::EmptyDescription => invalid_field(kind, 3),
        _ => invalid_field(kind, 1),
    })
}

pub(super) fn encode_perspective_retirement(
    value: &PerspectiveRetirement,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::PerspectiveRetirement,
        vec![
            (1, encode_id(value.perspective_retirement_id()).to_vec()),
            (2, encode_id(value.perspective_id()).to_vec()),
            (3, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_perspective_retirement(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<PerspectiveRetirement, RecordCodecError> {
    let kind = RecordKind::PerspectiveRetirement;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let perspective =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let revision = decode_revision(kind, 3, required_field(kind, &fields, 3)?)?;
    Ok(PerspectiveRetirement::new(id, perspective, revision))
}

pub(super) fn encode_layer_definition(
    value: &LayerDefinition,
) -> Result<Vec<u8>, RecordCodecError> {
    let rank = crate::Int::new(i128::from(value.precedence_rank())).to_canonical_bytes();
    encode_fields(
        RecordKind::LayerDefinition,
        vec![
            (1, encode_id(value.layer_id()).to_vec()),
            (2, encode_string(value.symbol().as_str())),
            (3, encode_optional_text(value.description())),
            (4, rank),
            (5, encode_lifecycle(value.lifecycle())),
            (6, encode_schema_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_layer_definition(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<LayerDefinition, RecordCodecError> {
    let kind = RecordKind::LayerDefinition;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let symbol_text = decode_string_with_limits(required_field(kind, &fields, 2)?, limits)
        .map_err(|_| invalid_field(kind, 2))?;
    let symbol = Symbol::new(symbol_text).map_err(|_| invalid_field(kind, 2))?;
    let description = decode_optional_text(kind, 3, required_field(kind, &fields, 3)?, limits)?;
    let rank = crate::Int::from_canonical_bytes(required_field(kind, &fields, 4)?)
        .map_err(|_| invalid_field(kind, 4))?
        .value();
    let rank = i32::try_from(rank).map_err(|_| invalid_field(kind, 4))?;
    let lifecycle = decode_lifecycle(kind, 5, required_field(kind, &fields, 5)?, limits)?;
    let revision = decode_schema_revision(kind, 6, required_field(kind, &fields, 6)?)?;
    Ok(LayerDefinition::new(
        id,
        symbol,
        description,
        rank,
        lifecycle,
        revision,
    ))
}

pub(super) fn encode_layer_snapshot(
    value: &LayerSchemaSnapshot,
) -> Result<Vec<u8>, RecordCodecError> {
    let definitions = value
        .definitions()
        .iter()
        .map(encode_layer_definition)
        .collect::<Result<Vec<_>, _>>()?;
    encode_fields(
        RecordKind::LayerSchemaSnapshot,
        vec![
            (1, encode_schema_revision(value.revision())),
            (2, encode_id(value.base_layer_id()).to_vec()),
            (3, encode_array(&definitions)),
        ],
    )
}

pub(super) fn decode_layer_snapshot(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<LayerSchemaSnapshot, RecordCodecError> {
    let kind = RecordKind::LayerSchemaSnapshot;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let revision = decode_schema_revision(kind, 1, required_field(kind, &fields, 1)?)?;
    let base = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let items = super::decode_array_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let definitions = collect_results_limited(
        items
            .into_iter()
            .map(|bytes| decode_layer_definition(bytes, limits)),
        limits,
    )?;
    LayerSchemaSnapshot::new(revision, definitions, base).map_err(|_| invalid_field(kind, 3))
}
