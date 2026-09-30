//! Schema record wire payloads and their closed nested value formats.

use crate::wire::{
    DecoderLimits, TlvDecoder, TlvEncoder, decode_id, decode_value_with_limits, encode_id,
    encode_value,
};
use crate::{
    CalendarPeriod, Cardinality, ConstraintSet, DecimalFieldMetadata, EntityTypeConstraint,
    EntityTypeDefinition, EventAttributeDefinition, EventKindDefinition, EventRoleDefinition,
    EventTimeConstraint, EventTimeForm, InclusiveRange, Lifecycle, NonEmptySet,
    PredicateDefinition, PredicateDefinitionSpec, RecordCodecError, RecordKind, ResolutionPolicy,
    RoleCardinality, SchemaDefinitionError, Symbol, TimeRange, UInt, Value, ValueConstraint,
    ValueKind,
};

use super::{
    collect_results_limited, decode_array_with_limits, decode_fields_with_limits, decode_revision,
    decode_string_with_limits, encode_array, encode_fields, encode_revision, encode_string,
    invalid_field, required_field,
};

fn uleb(value: u128) -> Vec<u8> {
    crate::numbers::encode_u128_varint(value)
}

fn read_uleb(kind: RecordKind, field: u32, bytes: &[u8]) -> Result<u128, RecordCodecError> {
    UInt::from_canonical_bytes(bytes)
        .map(UInt::value)
        .map_err(|_| invalid_field(kind, field))
}

fn lifecycle(value: Lifecycle) -> u8 {
    match value {
        Lifecycle::Active => 1,
        Lifecycle::Deprecated => 2,
        Lifecycle::Retired => 3,
    }
}

fn read_lifecycle(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<Lifecycle, RecordCodecError> {
    match bytes {
        [1] => Ok(Lifecycle::Active),
        [2] => Ok(Lifecycle::Deprecated),
        [3] => Ok(Lifecycle::Retired),
        _ => Err(invalid_field(kind, field)),
    }
}

fn symbol_bytes(value: &Symbol) -> Vec<u8> {
    encode_string(value.as_str())
}

fn read_symbol(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Symbol, RecordCodecError> {
    let value = decode_string_with_limits(bytes, limits).map_err(|_| invalid_field(kind, field))?;
    Symbol::new(value).map_err(|_| invalid_field(kind, field))
}

fn optional_payload(value: Option<Vec<u8>>) -> Vec<u8> {
    match value {
        None => vec![0],
        Some(value) => {
            let mut bytes = vec![1];
            bytes.extend(value);
            bytes
        }
    }
}

fn read_optional_payload(
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

fn nest(fields: Vec<(u32, Vec<u8>)>) -> Result<Vec<u8>, RecordCodecError> {
    let mut encoder = TlvEncoder::new();
    for (tag, value) in fields {
        encoder.push(tag, &value).map_err(RecordCodecError::Wire)?;
    }
    Ok(encoder.finish())
}

fn unnest<'a>(
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
            RecordCodecError::Wire(crate::wire::WireError::AllocationFailed {
                resource: crate::wire::DecodeResource::FieldsPerRecord,
            })
        })?;
        fields.push((field.tag(), field.value()));
    }
    if fields.is_empty() && !allowed.is_empty() {
        return Err(invalid_field(kind, parent_field));
    }
    Ok(fields)
}

fn field<'a>(
    kind: RecordKind,
    parent: u32,
    fields: &[(u32, &'a [u8])],
    tag: u32,
) -> Result<&'a [u8], RecordCodecError> {
    fields
        .iter()
        .find_map(|(found, value)| (*found == tag).then_some(*value))
        .ok_or_else(|| invalid_field(kind, parent))
}

fn entity_constraint(value: EntityTypeConstraint) -> Vec<u8> {
    match value {
        EntityTypeConstraint::AnyEntity => vec![1],
        EntityTypeConstraint::Exact(id) => {
            let mut bytes = vec![2];
            bytes.extend(encode_id(id));
            bytes
        }
    }
}

fn read_entity_constraint(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<EntityTypeConstraint, RecordCodecError> {
    match bytes {
        [1] => Ok(EntityTypeConstraint::AnyEntity),
        [2, id @ ..] => decode_id(id)
            .map(EntityTypeConstraint::Exact)
            .map_err(|_| invalid_field(kind, field)),
        _ => Err(invalid_field(kind, field)),
    }
}

fn value_kind(value: ValueKind) -> u8 {
    match value {
        ValueKind::Bool => 1,
        ValueKind::Int => 2,
        ValueKind::UInt => 3,
        ValueKind::Decimal => 4,
        ValueKind::String => 5,
        ValueKind::Symbol => 6,
        ValueKind::Entity => 7,
        ValueKind::Time => 8,
        ValueKind::Duration => 9,
        ValueKind::Bytes => 10,
    }
}

fn read_value_kind(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<ValueKind, RecordCodecError> {
    match bytes {
        [1] => Ok(ValueKind::Bool),
        [2] => Ok(ValueKind::Int),
        [3] => Ok(ValueKind::UInt),
        [4] => Ok(ValueKind::Decimal),
        [5] => Ok(ValueKind::String),
        [6] => Ok(ValueKind::Symbol),
        [7] => Ok(ValueKind::Entity),
        [8] => Ok(ValueKind::Time),
        [9] => Ok(ValueKind::Duration),
        [10] => Ok(ValueKind::Bytes),
        _ => Err(invalid_field(kind, field)),
    }
}

fn optional_object(value: Option<EntityTypeConstraint>) -> Vec<u8> {
    optional_payload(value.map(entity_constraint))
}

fn read_optional_object(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<Option<EntityTypeConstraint>, RecordCodecError> {
    read_optional_payload(kind, field, bytes)?
        .map(|value| read_entity_constraint(kind, field, value))
        .transpose()
}

fn metadata(value: DecimalFieldMetadata) -> Result<Vec<u8>, RecordCodecError> {
    let opt = |value: Option<u32>| {
        value.map_or_else(
            || vec![0],
            |number| {
                let mut bytes = vec![1];
                bytes.extend(uleb(u128::from(number)));
                bytes
            },
        )
    };
    nest(vec![
        (1, opt(value.display_precision())),
        (2, opt(value.measurement_precision())),
        (3, opt(value.currency_scale())),
    ])
}

fn read_metadata(
    kind: RecordKind,
    parent: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<DecimalFieldMetadata, RecordCodecError> {
    let fields = unnest(kind, parent, bytes, &[1, 2, 3], limits)?;
    let opt = |tag| -> Result<Option<u32>, RecordCodecError> {
        match field(kind, parent, &fields, tag)? {
            [0] => Ok(None),
            [1, value @ ..] => {
                let value = read_uleb(kind, parent, value)?;
                u32::try_from(value)
                    .map(Some)
                    .map_err(|_| invalid_field(kind, parent))
            }
            _ => Err(invalid_field(kind, parent)),
        }
    };
    DecimalFieldMetadata::new(opt(1)?, opt(2)?, opt(3)?).map_err(|_| invalid_field(kind, parent))
}

fn optional_metadata(value: Option<DecimalFieldMetadata>) -> Result<Vec<u8>, RecordCodecError> {
    value.map(metadata).transpose().map(optional_payload)
}

fn read_optional_metadata(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Option<DecimalFieldMetadata>, RecordCodecError> {
    read_optional_payload(kind, field, bytes)?
        .map(|value| read_metadata(kind, field, value, limits))
        .transpose()
}

pub(super) fn encode_entity_type(
    value: &EntityTypeDefinition,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::EntityTypeDefinition,
        vec![
            (1, encode_id(value.entity_type_id()).to_vec()),
            (2, symbol_bytes(value.symbol())),
            (3, optional_payload(value.description().map(encode_string))),
            (4, vec![lifecycle(value.lifecycle())]),
            (5, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_entity_type(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EntityTypeDefinition, RecordCodecError> {
    let kind = RecordKind::EntityTypeDefinition;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let symbol = read_symbol(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let description = read_optional_payload(kind, 3, required_field(kind, &fields, 3)?)?
        .map(|bytes| decode_string_with_limits(bytes, limits))
        .transpose()
        .map_err(|_| invalid_field(kind, 3))?;
    let lifecycle = read_lifecycle(kind, 4, required_field(kind, &fields, 4)?)?;
    let revision = decode_revision(kind, 5, required_field(kind, &fields, 5)?)?;
    Ok(EntityTypeDefinition::new(
        id,
        symbol,
        description,
        lifecycle,
        revision,
    ))
}

pub(super) fn encode_predicate(value: &PredicateDefinition) -> Result<Vec<u8>, RecordCodecError> {
    let cardinality = match value.cardinality() {
        Cardinality::Single => 1,
        Cardinality::Multi => 2,
    };
    let policy = match value.resolution_policy() {
        ResolutionPolicy::SingleValueReplace => 1,
        ResolutionPolicy::MultiValueOverlay => 2,
        ResolutionPolicy::MultiValueReplace => 3,
    };
    let constraints = value
        .constraints()
        .rules()
        .iter()
        .map(encode_constraint)
        .collect::<Result<Vec<_>, _>>()?;
    encode_fields(
        RecordKind::PredicateDefinition,
        vec![
            (1, encode_id(value.predicate_id()).to_vec()),
            (2, symbol_bytes(value.symbol())),
            (3, entity_constraint(value.subject_constraint())),
            (4, vec![value_kind(value.value_kind())]),
            (5, optional_object(value.object_constraint())),
            (6, vec![cardinality]),
            (7, vec![policy]),
            (8, encode_array(&constraints)),
            (9, optional_metadata(value.decimal_metadata())?),
            (10, vec![lifecycle(value.lifecycle())]),
            (11, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_predicate(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<PredicateDefinition, RecordCodecError> {
    let kind = RecordKind::PredicateDefinition;
    let fields =
        decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], limits)?;
    let predicate_id =
        decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let symbol = read_symbol(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let subject_constraint = read_entity_constraint(kind, 3, required_field(kind, &fields, 3)?)?;
    let value_kind = read_value_kind(kind, 4, required_field(kind, &fields, 4)?)?;
    let object_constraint = read_optional_object(kind, 5, required_field(kind, &fields, 5)?)?;
    let cardinality = match required_field(kind, &fields, 6)? {
        [1] => Cardinality::Single,
        [2] => Cardinality::Multi,
        _ => return Err(invalid_field(kind, 6)),
    };
    let resolution_policy = match required_field(kind, &fields, 7)? {
        [1] => ResolutionPolicy::SingleValueReplace,
        [2] => ResolutionPolicy::MultiValueOverlay,
        [3] => ResolutionPolicy::MultiValueReplace,
        _ => return Err(invalid_field(kind, 7)),
    };
    let constraint_bytes = decode_array_with_limits(required_field(kind, &fields, 8)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let constraints = collect_results_limited(
        constraint_bytes
            .into_iter()
            .map(|bytes| decode_constraint(kind, 8, bytes, limits)),
        limits,
    )?;
    let constraints = ConstraintSet::new(constraints).map_err(|_| invalid_field(kind, 8))?;
    let decimal_metadata =
        read_optional_metadata(kind, 9, required_field(kind, &fields, 9)?, limits)?;
    let lifecycle = read_lifecycle(kind, 10, required_field(kind, &fields, 10)?)?;
    let created_revision = decode_revision(kind, 11, required_field(kind, &fields, 11)?)?;
    PredicateDefinition::new(PredicateDefinitionSpec {
        predicate_id,
        symbol,
        subject_constraint,
        value_kind,
        object_constraint,
        cardinality,
        resolution_policy,
        constraints,
        decimal_metadata,
        lifecycle,
        created_revision,
    })
    .map_err(|error| invalid_field(kind, predicate_error_field(error)))
}

fn predicate_error_field(error: SchemaDefinitionError) -> u32 {
    match error {
        SchemaDefinitionError::DecimalMetadataForNonDecimal
        | SchemaDefinitionError::EmptyDecimalMetadata => 9,
        SchemaDefinitionError::MissingEntityObjectConstraint
        | SchemaDefinitionError::ObjectConstraintForNonEntity => 5,
        SchemaDefinitionError::CardinalityPolicyMismatch => 7,
        SchemaDefinitionError::DuplicateConstraint(_)
        | SchemaDefinitionError::ConstraintValueKindMismatch { .. } => 8,
        _ => 4,
    }
}

pub(super) fn encode_constraint(value: &ValueConstraint) -> Result<Vec<u8>, RecordCodecError> {
    let (tag, body) = match value {
        ValueConstraint::BoolSet(set) => (
            1,
            encode_array(
                &set.as_slice()
                    .iter()
                    .map(|value| encode_value(&Value::Bool(*value)))
                    .collect::<Vec<_>>(),
            ),
        ),
        ValueConstraint::IntRange(range) => (
            2,
            encode_range(range.min(), range.max(), |value| Value::Int(*value))?,
        ),
        ValueConstraint::UIntRange(range) => (
            3,
            encode_range(range.min(), range.max(), |value| Value::UInt(*value))?,
        ),
        ValueConstraint::DecimalRange(range) => (
            4,
            encode_range(range.min(), range.max(), |value| Value::Decimal(*value))?,
        ),
        ValueConstraint::StringByteLength(range) => (
            5,
            encode_range(range.min(), range.max(), |value| Value::UInt(*value))?,
        ),
        ValueConstraint::SymbolSet(set) => (
            6,
            encode_array(
                &set.as_slice()
                    .iter()
                    .map(|value| encode_value(&Value::Symbol(value.clone())))
                    .collect::<Vec<_>>(),
            ),
        ),
        ValueConstraint::TimeRange(range) => (
            7,
            encode_range(range.min(), range.max(), |value| Value::Time(value.clone()))?,
        ),
        ValueConstraint::DurationRange(range) => (
            8,
            encode_range(range.min(), range.max(), |value| Value::Duration(*value))?,
        ),
        ValueConstraint::BytesLength(range) => (
            9,
            encode_range(range.min(), range.max(), |value| Value::UInt(*value))?,
        ),
    };
    nest(vec![(1, vec![tag]), (2, body)])
}

fn encode_range<T>(
    min: Option<&T>,
    max: Option<&T>,
    as_value: impl Fn(&T) -> Value,
) -> Result<Vec<u8>, RecordCodecError> {
    let bound =
        |value: Option<&T>| optional_payload(value.map(|value| encode_value(&as_value(value))));
    nest(vec![(1, bound(min)), (2, bound(max))])
}

fn read_value(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Value, RecordCodecError> {
    decode_value_with_limits(bytes, limits).map_err(|_| invalid_field(kind, field))
}

fn read_bound(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<Option<Value>, RecordCodecError> {
    read_optional_payload(kind, field, bytes)?
        .map(|value| read_value(kind, field, value, limits))
        .transpose()
}

fn decode_range_values(
    kind: RecordKind,
    parent: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<(Option<Value>, Option<Value>), RecordCodecError> {
    let fields = unnest(kind, parent, bytes, &[1, 2], limits)?;
    Ok((
        read_bound(kind, parent, field(kind, parent, &fields, 1)?, limits)?,
        read_bound(kind, parent, field(kind, parent, &fields, 2)?, limits)?,
    ))
}

pub(super) fn decode_constraint(
    kind: RecordKind,
    parent: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<ValueConstraint, RecordCodecError> {
    let fields = unnest(kind, parent, bytes, &[1, 2], limits)?;
    let tag = field(kind, parent, &fields, 1)?;
    let body = field(kind, parent, &fields, 2)?;
    let invalid = || invalid_field(kind, parent);
    match tag {
        [1] => {
            let values = decode_array_with_limits(body, limits).map_err(RecordCodecError::Wire)?;
            let values = collect_results_limited(
                values
                    .into_iter()
                    .map(|bytes| match read_value(kind, parent, bytes, limits)? {
                        Value::Bool(value) => Ok(value),
                        _ => Err(invalid()),
                    }),
                limits,
            )?;
            NonEmptySet::new(values)
                .map(ValueConstraint::BoolSet)
                .map_err(|_| invalid())
        }
        [2] => {
            let (min, max) = decode_range_values(kind, parent, body, limits)?;
            let min = expect_value(
                min,
                |value| {
                    if let Value::Int(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            let max = expect_value(
                max,
                |value| {
                    if let Value::Int(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            InclusiveRange::new(min, max)
                .map(ValueConstraint::IntRange)
                .map_err(|_| invalid())
        }
        [3] | [5] | [9] => {
            let tag_value = tag.first().copied().ok_or_else(invalid)?;
            let (min, max) = decode_range_values(kind, parent, body, limits)?;
            let min = expect_value(
                min,
                |value| {
                    if let Value::UInt(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            let max = expect_value(
                max,
                |value| {
                    if let Value::UInt(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            let range = InclusiveRange::new(min, max).map_err(|_| invalid())?;
            match tag_value {
                3 => Ok(ValueConstraint::UIntRange(range)),
                5 => Ok(ValueConstraint::StringByteLength(range)),
                _ => Ok(ValueConstraint::BytesLength(range)),
            }
        }
        [4] => {
            let (min, max) = decode_range_values(kind, parent, body, limits)?;
            let min = expect_value(
                min,
                |value| {
                    if let Value::Decimal(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            let max = expect_value(
                max,
                |value| {
                    if let Value::Decimal(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            InclusiveRange::new(min, max)
                .map(ValueConstraint::DecimalRange)
                .map_err(|_| invalid())
        }
        [6] => {
            let values = decode_array_with_limits(body, limits).map_err(RecordCodecError::Wire)?;
            let values = collect_results_limited(
                values
                    .into_iter()
                    .map(|bytes| match read_value(kind, parent, bytes, limits)? {
                        Value::Symbol(value) => Ok(value),
                        _ => Err(invalid()),
                    }),
                limits,
            )?;
            NonEmptySet::new(values)
                .map(ValueConstraint::SymbolSet)
                .map_err(|_| invalid())
        }
        [7] => {
            let (min, max) = decode_range_values(kind, parent, body, limits)?;
            let min = expect_value(
                min,
                |value| {
                    if let Value::Time(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            let max = expect_value(
                max,
                |value| {
                    if let Value::Time(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            TimeRange::new(min, max)
                .map(ValueConstraint::TimeRange)
                .map_err(|_| invalid())
        }
        [8] => {
            let (min, max) = decode_range_values(kind, parent, body, limits)?;
            let min = expect_value(
                min,
                |value| {
                    if let Value::Duration(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            let max = expect_value(
                max,
                |value| {
                    if let Value::Duration(value) = value {
                        Some(value)
                    } else {
                        None
                    }
                },
                invalid,
            )?;
            InclusiveRange::new(min, max)
                .map(ValueConstraint::DurationRange)
                .map_err(|_| invalid())
        }
        _ => Err(invalid()),
    }
}

fn expect_value<T>(
    value: Option<Value>,
    extract: impl FnOnce(Value) -> Option<T>,
    invalid: impl FnOnce() -> RecordCodecError,
) -> Result<Option<T>, RecordCodecError> {
    value
        .map(|value| extract(value).ok_or_else(invalid))
        .transpose()
}

pub(super) fn encode_event_kind(value: &EventKindDefinition) -> Result<Vec<u8>, RecordCodecError> {
    let roles = value
        .roles()
        .iter()
        .map(encode_role)
        .collect::<Result<Vec<_>, _>>()?;
    let attributes = value
        .attributes()
        .iter()
        .map(encode_attribute)
        .collect::<Result<Vec<_>, _>>()?;
    encode_fields(
        RecordKind::EventKindDefinition,
        vec![
            (1, encode_id(value.event_kind_id()).to_vec()),
            (2, symbol_bytes(value.symbol())),
            (3, encode_array(&roles)),
            (4, encode_array(&attributes)),
            (
                5,
                encode_event_time_constraint(value.event_time_constraint())?,
            ),
            (6, vec![lifecycle(value.lifecycle())]),
            (7, encode_revision(value.created_revision())),
        ],
    )
}

pub(super) fn decode_event_kind(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventKindDefinition, RecordCodecError> {
    let kind = RecordKind::EventKindDefinition;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6, 7], limits)?;
    let id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let symbol = read_symbol(kind, 2, required_field(kind, &fields, 2)?, limits)?;
    let role_bytes = decode_array_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let roles = collect_results_limited(
        role_bytes
            .into_iter()
            .map(|bytes| decode_role(kind, bytes, limits)),
        limits,
    )?;
    let attribute_bytes = decode_array_with_limits(required_field(kind, &fields, 4)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let attributes = collect_results_limited(
        attribute_bytes
            .into_iter()
            .map(|bytes| decode_attribute(kind, bytes, limits)),
        limits,
    )?;
    let time = decode_event_time_constraint(kind, 5, required_field(kind, &fields, 5)?, limits)?;
    let lifecycle = read_lifecycle(kind, 6, required_field(kind, &fields, 6)?)?;
    let revision = decode_revision(kind, 7, required_field(kind, &fields, 7)?)?;
    EventKindDefinition::new(id, symbol, roles, attributes, time, lifecycle, revision).map_err(
        |error| match error {
            SchemaDefinitionError::DuplicateEventRoleId
            | SchemaDefinitionError::DuplicateEventRoleSymbol => invalid_field(kind, 3),
            SchemaDefinitionError::DuplicateEventAttributeId
            | SchemaDefinitionError::DuplicateEventAttributeSymbol => invalid_field(kind, 4),
            _ => invalid_field(kind, 5),
        },
    )
}

fn encode_role(value: &EventRoleDefinition) -> Result<Vec<u8>, RecordCodecError> {
    nest(vec![
        (1, encode_id(value.event_role_id()).to_vec()),
        (2, symbol_bytes(value.symbol())),
        (3, entity_constraint(value.entity_constraint())),
        (
            4,
            nest(vec![
                (1, uleb(u128::from(value.cardinality().min()))),
                (
                    2,
                    value.cardinality().max().map_or_else(
                        || vec![0],
                        |max| {
                            let mut bytes = vec![1];
                            bytes.extend(uleb(u128::from(max)));
                            bytes
                        },
                    ),
                ),
            ])?,
        ),
    ])
}

fn decode_role(
    kind: RecordKind,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventRoleDefinition, RecordCodecError> {
    let fields = unnest(kind, 3, bytes, &[1, 2, 3, 4], limits)?;
    let id = decode_id(field(kind, 3, &fields, 1)?).map_err(|_| invalid_field(kind, 3))?;
    let symbol = read_symbol(kind, 3, field(kind, 3, &fields, 2)?, limits)?;
    let constraint = read_entity_constraint(kind, 3, field(kind, 3, &fields, 3)?)?;
    let cardinality_fields = unnest(kind, 3, field(kind, 3, &fields, 4)?, &[1, 2], limits)?;
    let minimum = read_uleb(kind, 3, field(kind, 3, &cardinality_fields, 1)?)?;
    let minimum = u32::try_from(minimum).map_err(|_| invalid_field(kind, 3))?;
    let maximum = match field(kind, 3, &cardinality_fields, 2)? {
        [0] => None,
        [1, value @ ..] => {
            Some(u32::try_from(read_uleb(kind, 3, value)?).map_err(|_| invalid_field(kind, 3))?)
        }
        _ => return Err(invalid_field(kind, 3)),
    };
    let cardinality = RoleCardinality::new(minimum, maximum).map_err(|_| invalid_field(kind, 3))?;
    Ok(EventRoleDefinition::new(
        id,
        symbol,
        constraint,
        cardinality,
    ))
}

fn encode_attribute(value: &EventAttributeDefinition) -> Result<Vec<u8>, RecordCodecError> {
    let constraints = value
        .constraints()
        .rules()
        .iter()
        .map(encode_constraint)
        .collect::<Result<Vec<_>, _>>()?;
    nest(vec![
        (1, encode_id(value.event_attribute_id()).to_vec()),
        (2, symbol_bytes(value.symbol())),
        (3, vec![value_kind(value.value_kind())]),
        (4, optional_object(value.object_constraint())),
        (5, encode_array(&constraints)),
        (6, optional_metadata(value.decimal_metadata())?),
        (7, vec![u8::from(value.required())]),
    ])
}

fn decode_attribute(
    kind: RecordKind,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventAttributeDefinition, RecordCodecError> {
    let fields = unnest(kind, 4, bytes, &[1, 2, 3, 4, 5, 6, 7], limits)?;
    let id = decode_id(field(kind, 4, &fields, 1)?).map_err(|_| invalid_field(kind, 4))?;
    let symbol = read_symbol(kind, 4, field(kind, 4, &fields, 2)?, limits)?;
    let kind_value = read_value_kind(kind, 4, field(kind, 4, &fields, 3)?)?;
    let object = read_optional_object(kind, 4, field(kind, 4, &fields, 4)?)?;
    let raw_constraints = decode_array_with_limits(field(kind, 4, &fields, 5)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let constraints = collect_results_limited(
        raw_constraints
            .into_iter()
            .map(|bytes| decode_constraint(kind, 4, bytes, limits)),
        limits,
    )?;
    let constraints = ConstraintSet::new(constraints).map_err(|_| invalid_field(kind, 4))?;
    let metadata = read_optional_metadata(kind, 4, field(kind, 4, &fields, 6)?, limits)?;
    let required = match field(kind, 4, &fields, 7)? {
        [0] => false,
        [1] => true,
        _ => return Err(invalid_field(kind, 4)),
    };
    EventAttributeDefinition::new(
        id,
        symbol,
        kind_value,
        object,
        constraints,
        metadata,
        required,
    )
    .map_err(|_| invalid_field(kind, 4))
}

fn encode_event_time_constraint(value: EventTimeConstraint) -> Result<Vec<u8>, RecordCodecError> {
    let form = match value.form() {
        EventTimeForm::InstantOnly => 1,
        EventTimeForm::SpanOnly => 2,
        EventTimeForm::InstantOrSpan => 3,
        EventTimeForm::OpenSpanAllowed => 4,
    };
    let period = value
        .max_calendar_span()
        .map(|period| {
            nest(vec![
                (1, uleb(u128::from(period.years()))),
                (2, uleb(u128::from(period.months()))),
                (3, uleb(u128::from(period.days()))),
            ])
        })
        .transpose()?;
    nest(vec![(1, vec![form]), (2, optional_payload(period))])
}

fn decode_event_time_constraint(
    kind: RecordKind,
    parent: u32,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<EventTimeConstraint, RecordCodecError> {
    let fields = unnest(kind, parent, bytes, &[1, 2], limits)?;
    let form = match field(kind, parent, &fields, 1)? {
        [1] => EventTimeForm::InstantOnly,
        [2] => EventTimeForm::SpanOnly,
        [3] => EventTimeForm::InstantOrSpan,
        [4] => EventTimeForm::OpenSpanAllowed,
        _ => return Err(invalid_field(kind, parent)),
    };
    let span = read_optional_payload(kind, parent, field(kind, parent, &fields, 2)?)?
        .map(|bytes| {
            let fields = unnest(kind, parent, bytes, &[1, 2, 3], limits)?;
            let years = u32::try_from(read_uleb(kind, parent, field(kind, parent, &fields, 1)?)?)
                .map_err(|_| invalid_field(kind, parent))?;
            let months = u8::try_from(read_uleb(kind, parent, field(kind, parent, &fields, 2)?)?)
                .map_err(|_| invalid_field(kind, parent))?;
            let days = u32::try_from(read_uleb(kind, parent, field(kind, parent, &fields, 3)?)?)
                .map_err(|_| invalid_field(kind, parent))?;
            CalendarPeriod::new(years, months, days).map_err(|_| invalid_field(kind, parent))
        })
        .transpose()?;
    EventTimeConstraint::new(form, span).map_err(|_| invalid_field(kind, parent))
}
