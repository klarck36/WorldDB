//! Typed schema-management commands used by the desktop host and sidecar.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    CalendarPeriod, Cardinality, ConstraintSet, Decimal, DecimalFieldMetadata, DomainId, Duration,
    EntityTypeConstraint, EntityTypeDefinition, EntityTypeId, EventAttributeDefinition,
    EventAttributeId, EventKindDefinition, EventKindId, EventRoleDefinition, EventRoleId,
    EventTimeConstraint, EventTimeForm, InclusiveRange, Int, Lifecycle, NonEmptySet,
    PredicateDefinition, PredicateDefinitionSpec, PredicateId, Record, ResolutionPolicy, Revision,
    RoleCardinality, SchemaDefinition, SchemaMode, SchemaRevision, SchemaSnapshot, Symbol, Time,
    TimeRange, TimeUnitDefinition, TimelineCalendarProfile, TimelineDefinition, TimelineId, UInt,
    ValueConstraint, ValueKind,
};
use worlddb_storage_file::FileSchemaManager;

use crate::{EngineError, EngineHost};

/// Schema operation sent from a host window to its authenticated engine.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchemaCommand {
    /// Reads the selected immutable schema view.
    Snapshot { mode: SchemaModeInput },
    /// Creates one schema definition.
    Create {
        expected_base_revision: u64,
        definition: SchemaDefinitionDraft,
    },
    /// Appends a legal lifecycle-only revision.
    SetLifecycle {
        expected_base_revision: u64,
        family: SchemaFamily,
        identity: String,
        lifecycle: SchemaLifecycle,
    },
    /// Appends several lifecycle-only revisions as one validated schema commit.
    SetLifecycleBatch {
        expected_base_revision: u64,
        updates: Vec<SchemaLifecycleUpdateDraft>,
    },
}

/// Historical, current, or explicit schema selection.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchemaModeInput {
    /// Schema effective at the requested transaction-time revision.
    Historical { recorded_as_of: u64 },
    /// Latest schema at the live database head.
    Current,
    /// Exact previously committed schema revision.
    Explicit { revision: u64 },
}

/// Schema family selected for a lifecycle update.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaFamily {
    EntityType,
    Predicate,
    EventKind,
    Timeline,
    TimeUnit,
}

/// Monotonic schema lifecycle states.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaLifecycle {
    Active,
    Deprecated,
    Retired,
}

/// One stable schema identity and its requested forward lifecycle state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaLifecycleUpdateDraft {
    pub family: SchemaFamily,
    pub identity: String,
    pub lifecycle: SchemaLifecycle,
}

impl From<SchemaLifecycle> for Lifecycle {
    fn from(value: SchemaLifecycle) -> Self {
        match value {
            SchemaLifecycle::Active => Self::Active,
            SchemaLifecycle::Deprecated => Self::Deprecated,
            SchemaLifecycle::Retired => Self::Retired,
        }
    }
}

/// Complete typed input for one new schema definition.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "family", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchemaDefinitionDraft {
    EntityType {
        symbol: String,
        #[serde(default)]
        description: Option<String>,
    },
    Predicate {
        symbol: String,
        #[serde(default)]
        subject_constraint: EntityConstraintDraft,
        value_kind: ValueKindDraft,
        #[serde(default)]
        object_constraint: Option<EntityConstraintDraft>,
        cardinality: CardinalityDraft,
        resolution_policy: ResolutionPolicyDraft,
        #[serde(default)]
        constraints: Vec<ConstraintDraft>,
        #[serde(default)]
        decimal_metadata: Option<DecimalMetadataDraft>,
    },
    EventKind {
        symbol: String,
        #[serde(default)]
        roles: Vec<EventRoleDraft>,
        #[serde(default)]
        attributes: Vec<EventAttributeDraft>,
        event_time: EventTimeDraft,
    },
    Timeline {
        symbol: String,
        calendar_profile: TimelineCalendarProfileDraft,
    },
    TimeUnit {
        symbol: String,
        /// Decimal string, kept exact across JavaScript and the signed IPC envelope.
        nanoseconds_per_tick: String,
    },
}

/// Closed calendar mapping input for a Timeline definition.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "profile", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineCalendarProfileDraft {
    None,
    ProlepticGregorianUtc {
        /// Signed decimal nanoseconds from the Unix epoch, parsed as i128.
        epoch_unix_nanoseconds: String,
    },
}

/// `AnyEntity` or an exact stable EntityType identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityConstraintDraft {
    AnyEntity,
    Exact { entity_type_id: String },
}

impl Default for EntityConstraintDraft {
    fn default() -> Self {
        Self::AnyEntity
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKindDraft {
    Bool,
    Int,
    UInt,
    Decimal,
    String,
    Symbol,
    Entity,
    Time,
    Duration,
    Bytes,
}

impl From<ValueKindDraft> for ValueKind {
    fn from(value: ValueKindDraft) -> Self {
        match value {
            ValueKindDraft::Bool => Self::Bool,
            ValueKindDraft::Int => Self::Int,
            ValueKindDraft::UInt => Self::UInt,
            ValueKindDraft::Decimal => Self::Decimal,
            ValueKindDraft::String => Self::String,
            ValueKindDraft::Symbol => Self::Symbol,
            ValueKindDraft::Entity => Self::Entity,
            ValueKindDraft::Time => Self::Time,
            ValueKindDraft::Duration => Self::Duration,
            ValueKindDraft::Bytes => Self::Bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardinalityDraft {
    Single,
    Multi,
}

impl From<CardinalityDraft> for Cardinality {
    fn from(value: CardinalityDraft) -> Self {
        match value {
            CardinalityDraft::Single => Self::Single,
            CardinalityDraft::Multi => Self::Multi,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionPolicyDraft {
    SingleValueReplace,
    MultiValueOverlay,
    MultiValueReplace,
}

impl From<ResolutionPolicyDraft> for ResolutionPolicy {
    fn from(value: ResolutionPolicyDraft) -> Self {
        match value {
            ResolutionPolicyDraft::SingleValueReplace => Self::SingleValueReplace,
            ResolutionPolicyDraft::MultiValueOverlay => Self::MultiValueOverlay,
            ResolutionPolicyDraft::MultiValueReplace => Self::MultiValueReplace,
        }
    }
}

/// Closed typed constraint input. Numeric values are decimal strings to preserve 128-bit range.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConstraintDraft {
    BoolSet {
        values: Vec<bool>,
    },
    IntRange {
        min: Option<String>,
        max: Option<String>,
    },
    UIntRange {
        min: Option<String>,
        max: Option<String>,
    },
    DecimalRange {
        min: Option<String>,
        max: Option<String>,
    },
    StringByteLength {
        min: Option<String>,
        max: Option<String>,
    },
    SymbolSet {
        values: Vec<String>,
    },
    TimeRange {
        min: Option<TimeBoundDraft>,
        max: Option<TimeBoundDraft>,
    },
    DurationRange {
        min_ns: Option<String>,
        max_ns: Option<String>,
    },
    BytesLength {
        min: Option<String>,
        max: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeBoundDraft {
    pub timeline_id: String,
    pub ticks: String,
    pub unit: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecimalMetadataDraft {
    pub display_precision: Option<u32>,
    pub measurement_precision: Option<u32>,
    pub currency_scale: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTimeFormDraft {
    InstantOnly,
    SpanOnly,
    InstantOrSpan,
    OpenSpanAllowed,
}

impl From<EventTimeFormDraft> for EventTimeForm {
    fn from(value: EventTimeFormDraft) -> Self {
        match value {
            EventTimeFormDraft::InstantOnly => Self::InstantOnly,
            EventTimeFormDraft::SpanOnly => Self::SpanOnly,
            EventTimeFormDraft::InstantOrSpan => Self::InstantOrSpan,
            EventTimeFormDraft::OpenSpanAllowed => Self::OpenSpanAllowed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarPeriodDraft {
    pub years: u32,
    pub months: u8,
    pub days: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventTimeDraft {
    pub form: EventTimeFormDraft,
    #[serde(default)]
    pub max_calendar_span: Option<CalendarPeriodDraft>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventRoleDraft {
    pub symbol: String,
    pub entity_constraint: EntityConstraintDraft,
    pub min_participants: u32,
    pub max_participants: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventAttributeDraft {
    pub symbol: String,
    pub value_kind: ValueKindDraft,
    #[serde(default)]
    pub object_constraint: Option<EntityConstraintDraft>,
    #[serde(default)]
    pub constraints: Vec<ConstraintDraft>,
    #[serde(default)]
    pub decimal_metadata: Option<DecimalMetadataDraft>,
    #[serde(default)]
    pub required: bool,
}

/// Result of a typed schema command.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaResponse {
    Snapshot(SchemaSnapshotView),
    Published(SchemaPublicationView),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SchemaSnapshotView {
    pub revision: u64,
    pub fingerprint: String,
    pub definitions: Vec<SchemaDefinitionView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SchemaPublicationView {
    pub operation_id: String,
    pub revision: u64,
    pub fingerprint: String,
    pub definition_count: usize,
    pub definitions: Vec<SchemaDefinitionView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SchemaDefinitionView {
    pub family: String,
    pub identity: String,
    pub symbol: String,
    pub lifecycle: String,
    pub lifecycle_help: String,
    pub created_revision: u64,
    pub description: Option<String>,
    pub details: serde_json::Value,
    pub canonical_record_hex: String,
}

impl EngineHost {
    /// Executes a host-authenticated schema query or mutation.
    pub fn schema(&self, command: SchemaCommand) -> Result<SchemaResponse, EngineError> {
        let _guard = self
            .schema_management
            .lock()
            .map_err(|_| EngineError::Schema("schema manager state is unavailable".to_owned()))?;
        let principal = self.principal_id.ok_or_else(|| {
            EngineError::Schema("an authenticated project session is required".to_owned())
        })?;
        let mut manager =
            FileSchemaManager::open(self._layout.clone(), &self._writer_lock, principal)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
        match command {
            SchemaCommand::Snapshot { mode } => {
                let (mode, recorded_as_of) = schema_mode(mode)?;
                let snapshot = manager
                    .schema_at(mode, recorded_as_of)
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                Ok(SchemaResponse::Snapshot(schema_snapshot_view(&snapshot)?))
            }
            SchemaCommand::Create {
                expected_base_revision,
                definition,
            } => {
                let base = revision_from_u64(expected_base_revision)?;
                let target = manager
                    .next_revision()
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let record = build_schema_record(definition, target)?;
                let operation_id = crate::requested_operation_id_or(
                    worlddb_core::storage_internal::generate_schema_management_operation_id,
                )
                .map_err(|error| EngineError::Schema(error.to_string()))?;
                let receipt = manager
                    .publish(base, operation_id, vec![record])
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let snapshot = manager
                    .schema_at(SchemaMode::Current, receipt.revision())
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let view = schema_snapshot_view(&snapshot)?;
                Ok(SchemaResponse::Published(SchemaPublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    fingerprint: fingerprint_hex(&receipt.fingerprint()),
                    definition_count: receipt.definition_count(),
                    definitions: view.definitions,
                }))
            }
            SchemaCommand::SetLifecycle {
                expected_base_revision,
                family,
                identity,
                lifecycle,
            } => {
                let base = revision_from_u64(expected_base_revision)?;
                let target = manager
                    .next_revision()
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let snapshot = manager
                    .schema_at(SchemaMode::Current, manager.revision())
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let record =
                    revise_schema_lifecycle(&snapshot, family, &identity, lifecycle, target)?;
                let operation_id = crate::requested_operation_id_or(
                    worlddb_core::storage_internal::generate_schema_management_operation_id,
                )
                .map_err(|error| EngineError::Schema(error.to_string()))?;
                let receipt = manager
                    .publish(base, operation_id, vec![record])
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let snapshot = manager
                    .schema_at(SchemaMode::Current, receipt.revision())
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let view = schema_snapshot_view(&snapshot)?;
                Ok(SchemaResponse::Published(SchemaPublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    fingerprint: fingerprint_hex(&receipt.fingerprint()),
                    definition_count: receipt.definition_count(),
                    definitions: view.definitions,
                }))
            }
            SchemaCommand::SetLifecycleBatch {
                expected_base_revision,
                updates,
            } => {
                if updates.is_empty() {
                    return Err(EngineError::Schema(
                        "at least one lifecycle update is required".to_owned(),
                    ));
                }
                let base = revision_from_u64(expected_base_revision)?;
                let target = manager
                    .next_revision()
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let snapshot = manager
                    .schema_at(SchemaMode::Current, manager.revision())
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let mut seen = Vec::new();
                let records = updates
                    .into_iter()
                    .map(|update| {
                        if seen.iter().any(|(family, identity)| {
                            *family == update.family && identity == &update.identity
                        }) {
                            return Err(EngineError::Schema(
                                "schema identity is repeated in the lifecycle batch".to_owned(),
                            ));
                        }
                        seen.push((update.family, update.identity.clone()));
                        revise_schema_lifecycle(
                            &snapshot,
                            update.family,
                            &update.identity,
                            update.lifecycle,
                            target,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let operation_id = crate::requested_operation_id_or(
                    worlddb_core::storage_internal::generate_schema_management_operation_id,
                )
                .map_err(|error| EngineError::Schema(error.to_string()))?;
                let receipt = manager
                    .publish(base, operation_id, records)
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let snapshot = manager
                    .schema_at(SchemaMode::Current, receipt.revision())
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
                let view = schema_snapshot_view(&snapshot)?;
                Ok(SchemaResponse::Published(SchemaPublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    fingerprint: fingerprint_hex(&receipt.fingerprint()),
                    definition_count: receipt.definition_count(),
                    definitions: view.definitions,
                }))
            }
        }
    }
}

fn schema_mode(input: SchemaModeInput) -> Result<(SchemaMode, Revision), EngineError> {
    match input {
        SchemaModeInput::Historical { recorded_as_of } => {
            Ok((SchemaMode::Historical, revision_from_u64(recorded_as_of)?))
        }
        SchemaModeInput::Current => Ok((SchemaMode::Current, Revision::GENESIS)),
        SchemaModeInput::Explicit { revision } => Ok((
            SchemaMode::Explicit(SchemaRevision::from_published_revision(revision_from_u64(
                revision,
            )?)),
            Revision::GENESIS,
        )),
    }
}

fn build_schema_record(
    draft: SchemaDefinitionDraft,
    revision: Revision,
) -> Result<Record, EngineError> {
    let record = match draft {
        SchemaDefinitionDraft::EntityType {
            symbol,
            description,
        } => {
            let id = generate_id::<EntityTypeId>()?;
            let symbol = parse_symbol(&symbol)?;
            Record::EntityTypeDefinition(EntityTypeDefinition::new(
                id,
                symbol,
                description,
                Lifecycle::Active,
                revision,
            ))
        }
        SchemaDefinitionDraft::Predicate {
            symbol,
            subject_constraint,
            value_kind,
            object_constraint,
            cardinality,
            resolution_policy,
            constraints,
            decimal_metadata,
        } => {
            let id = generate_id::<PredicateId>()?;
            let definition = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id: id,
                symbol: parse_symbol(&symbol)?,
                subject_constraint: entity_constraint(subject_constraint)?,
                value_kind: value_kind.into(),
                object_constraint: object_constraint.map(entity_constraint).transpose()?,
                cardinality: cardinality.into(),
                resolution_policy: resolution_policy.into(),
                constraints: constraint_set(constraints)?,
                decimal_metadata: decimal_metadata.map(decimal_metadata_value).transpose()?,
                lifecycle: Lifecycle::Active,
                created_revision: revision,
            })
            .map_err(|error| EngineError::Schema(error.to_string()))?;
            Record::PredicateDefinition(definition)
        }
        SchemaDefinitionDraft::EventKind {
            symbol,
            roles,
            attributes,
            event_time,
        } => {
            let id = generate_id::<EventKindId>()?;
            let roles = roles
                .into_iter()
                .map(|role| {
                    Ok(EventRoleDefinition::new(
                        generate_id::<EventRoleId>()?,
                        parse_symbol(&role.symbol)?,
                        entity_constraint(role.entity_constraint)?,
                        RoleCardinality::new(role.min_participants, role.max_participants)
                            .map_err(|error| EngineError::Schema(error.to_string()))?,
                    ))
                })
                .collect::<Result<Vec<_>, EngineError>>()?;
            let attributes = attributes
                .into_iter()
                .map(|attribute| {
                    EventAttributeDefinition::new(
                        generate_id::<EventAttributeId>()?,
                        parse_symbol(&attribute.symbol)?,
                        attribute.value_kind.into(),
                        attribute
                            .object_constraint
                            .map(entity_constraint)
                            .transpose()?,
                        constraint_set(attribute.constraints)?,
                        attribute
                            .decimal_metadata
                            .map(decimal_metadata_value)
                            .transpose()?,
                        attribute.required,
                    )
                    .map_err(|error| EngineError::Schema(error.to_string()))
                })
                .collect::<Result<Vec<_>, EngineError>>()?;
            let event_time_constraint = EventTimeConstraint::new(
                event_time.form.into(),
                event_time
                    .max_calendar_span
                    .map(|period| CalendarPeriod::new(period.years, period.months, period.days))
                    .transpose()
                    .map_err(|error| EngineError::Schema(error.to_string()))?,
            )
            .map_err(|error| EngineError::Schema(error.to_string()))?;
            let definition = EventKindDefinition::new(
                id,
                parse_symbol(&symbol)?,
                roles,
                attributes,
                event_time_constraint,
                Lifecycle::Active,
                revision,
            )
            .map_err(|error| EngineError::Schema(error.to_string()))?;
            Record::EventKindDefinition(definition)
        }
        SchemaDefinitionDraft::Timeline {
            symbol,
            calendar_profile,
        } => {
            let calendar = match calendar_profile {
                TimelineCalendarProfileDraft::None => TimelineCalendarProfile::None,
                TimelineCalendarProfileDraft::ProlepticGregorianUtc {
                    epoch_unix_nanoseconds,
                } => TimelineCalendarProfile::ProlepticGregorianUtc {
                    epoch_unix_nanoseconds: parse_i128(&epoch_unix_nanoseconds)?,
                },
            };
            Record::TimelineDefinition(TimelineDefinition::new(
                generate_id::<TimelineId>()?,
                parse_symbol(&symbol)?,
                calendar,
                Lifecycle::Active,
                revision,
            ))
        }
        SchemaDefinitionDraft::TimeUnit {
            symbol,
            nanoseconds_per_tick,
        } => {
            let scale = nanoseconds_per_tick.parse::<u64>().map_err(|_| {
                EngineError::Schema("invalid unsigned 64-bit nanosecond scale".to_owned())
            })?;
            let definition =
                TimeUnitDefinition::new(parse_symbol(&symbol)?, scale, Lifecycle::Active, revision)
                    .map_err(|error| EngineError::Schema(error.to_string()))?;
            Record::TimeUnitDefinition(definition)
        }
    };
    Ok(record)
}

fn revise_schema_lifecycle(
    snapshot: &SchemaSnapshot,
    family: SchemaFamily,
    identity: &str,
    lifecycle: SchemaLifecycle,
    revision: Revision,
) -> Result<Record, EngineError> {
    match family {
        SchemaFamily::EntityType => {
            let id = EntityTypeId::from_str(identity)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            let definition = snapshot
                .definitions()
                .iter()
                .find_map(|item| match item {
                    SchemaDefinition::EntityType(value) if value.entity_type_id() == id => {
                        Some(value)
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    EngineError::Schema("EntityType was not found in this schema view".to_owned())
                })?;
            let revised = definition
                .revise_lifecycle(lifecycle.into(), revision)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            Ok(Record::EntityTypeDefinition(revised))
        }
        SchemaFamily::Predicate => {
            let id = PredicateId::from_str(identity)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            let definition = snapshot
                .definitions()
                .iter()
                .find_map(|item| match item {
                    SchemaDefinition::Predicate(value) if value.predicate_id() == id => Some(value),
                    _ => None,
                })
                .ok_or_else(|| {
                    EngineError::Schema("Predicate was not found in this schema view".to_owned())
                })?;
            let revised = definition
                .revise_lifecycle(lifecycle.into(), revision)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            Ok(Record::PredicateDefinition(revised))
        }
        SchemaFamily::EventKind => {
            let id = EventKindId::from_str(identity)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            let definition = snapshot
                .definitions()
                .iter()
                .find_map(|item| match item {
                    SchemaDefinition::EventKind(value) if value.event_kind_id() == id => {
                        Some(value)
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    EngineError::Schema("EventKind was not found in this schema view".to_owned())
                })?;
            let revised = definition
                .revise_lifecycle(lifecycle.into(), revision)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            Ok(Record::EventKindDefinition(revised))
        }
        SchemaFamily::Timeline => {
            let id = TimelineId::from_str(identity)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            let definition = snapshot
                .definitions()
                .iter()
                .find_map(|item| match item {
                    SchemaDefinition::Timeline(value) if value.timeline_id() == id => Some(value),
                    _ => None,
                })
                .ok_or_else(|| {
                    EngineError::Schema("Timeline was not found in this schema view".to_owned())
                })?;
            let revised = definition
                .revise_lifecycle(lifecycle.into(), revision)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            Ok(Record::TimelineDefinition(revised))
        }
        SchemaFamily::TimeUnit => {
            let symbol = parse_symbol(identity)?;
            let definition = snapshot
                .definitions()
                .iter()
                .find_map(|item| match item {
                    SchemaDefinition::TimeUnit(value) if value.symbol() == &symbol => Some(value),
                    _ => None,
                })
                .ok_or_else(|| {
                    EngineError::Schema("TimeUnit was not found in this schema view".to_owned())
                })?;
            let revised = definition
                .revise_lifecycle(lifecycle.into(), revision)
                .map_err(|error| EngineError::Schema(error.to_string()))?;
            Ok(Record::TimeUnitDefinition(revised))
        }
    }
}

fn entity_constraint(draft: EntityConstraintDraft) -> Result<EntityTypeConstraint, EngineError> {
    match draft {
        EntityConstraintDraft::AnyEntity => Ok(EntityTypeConstraint::AnyEntity),
        EntityConstraintDraft::Exact { entity_type_id } => EntityTypeId::from_str(&entity_type_id)
            .map(EntityTypeConstraint::Exact)
            .map_err(|error| EngineError::Schema(error.to_string())),
    }
}

fn constraint_set(drafts: Vec<ConstraintDraft>) -> Result<ConstraintSet, EngineError> {
    let constraints = drafts
        .into_iter()
        .map(constraint)
        .collect::<Result<Vec<_>, _>>()?;
    ConstraintSet::new(constraints).map_err(|error| EngineError::Schema(error.to_string()))
}

fn constraint(draft: ConstraintDraft) -> Result<ValueConstraint, EngineError> {
    let parse_int = |value: Option<String>| {
        value
            .map(|value| {
                Int::from_str(&value).map_err(|error| EngineError::Schema(error.to_string()))
            })
            .transpose()
    };
    let parse_uint = |value: Option<String>| {
        value
            .map(|value| {
                UInt::from_str(&value).map_err(|error| EngineError::Schema(error.to_string()))
            })
            .transpose()
    };
    let parse_decimal = |value: Option<String>| {
        value
            .map(|value| {
                Decimal::from_canonical_string(&value)
                    .map_err(|error| EngineError::Schema(error.to_string()))
            })
            .transpose()
    };
    let result = match draft {
        ConstraintDraft::BoolSet { values } => ValueConstraint::BoolSet(
            NonEmptySet::new(values).map_err(|error| EngineError::Schema(error.to_string()))?,
        ),
        ConstraintDraft::IntRange { min, max } => ValueConstraint::IntRange(
            InclusiveRange::new(parse_int(min)?, parse_int(max)?)
                .map_err(|error| EngineError::Schema(error.to_string()))?,
        ),
        ConstraintDraft::UIntRange { min, max } => ValueConstraint::UIntRange(
            InclusiveRange::new(parse_uint(min)?, parse_uint(max)?)
                .map_err(|error| EngineError::Schema(error.to_string()))?,
        ),
        ConstraintDraft::DecimalRange { min, max } => ValueConstraint::DecimalRange(
            InclusiveRange::new(parse_decimal(min)?, parse_decimal(max)?)
                .map_err(|error| EngineError::Schema(error.to_string()))?,
        ),
        ConstraintDraft::StringByteLength { min, max } => ValueConstraint::StringByteLength(
            InclusiveRange::new(parse_uint(min)?, parse_uint(max)?)
                .map_err(|error| EngineError::Schema(error.to_string()))?,
        ),
        ConstraintDraft::SymbolSet { values } => {
            let values = values
                .into_iter()
                .map(|value| parse_symbol(&value))
                .collect::<Result<Vec<_>, _>>()?;
            ValueConstraint::SymbolSet(
                NonEmptySet::new(values).map_err(|error| EngineError::Schema(error.to_string()))?,
            )
        }
        ConstraintDraft::TimeRange { min, max } => {
            let min = min.map(time_bound).transpose()?;
            let max = max.map(time_bound).transpose()?;
            ValueConstraint::TimeRange(
                TimeRange::new(min, max).map_err(|error| EngineError::Schema(error.to_string()))?,
            )
        }
        ConstraintDraft::DurationRange { min_ns, max_ns } => {
            let min = min_ns
                .map(|value| parse_i128(&value))
                .transpose()?
                .map(Duration::from_nanoseconds);
            let max = max_ns
                .map(|value| parse_i128(&value))
                .transpose()?
                .map(Duration::from_nanoseconds);
            ValueConstraint::DurationRange(
                InclusiveRange::new(min, max)
                    .map_err(|error| EngineError::Schema(error.to_string()))?,
            )
        }
        ConstraintDraft::BytesLength { min, max } => ValueConstraint::BytesLength(
            InclusiveRange::new(parse_uint(min)?, parse_uint(max)?)
                .map_err(|error| EngineError::Schema(error.to_string()))?,
        ),
    };
    Ok(result)
}

fn time_bound(bound: TimeBoundDraft) -> Result<Time, EngineError> {
    let timeline_id = TimelineId::from_str(&bound.timeline_id)
        .map_err(|error| EngineError::Schema(error.to_string()))?;
    let ticks = parse_i128(&bound.ticks)?;
    Ok(Time::new(timeline_id, ticks, parse_symbol(&bound.unit)?))
}

fn parse_i128(value: &str) -> Result<i128, EngineError> {
    value
        .parse::<i128>()
        .map_err(|_| EngineError::Schema("invalid signed 128-bit integer".to_owned()))
}

fn decimal_metadata_value(
    metadata: DecimalMetadataDraft,
) -> Result<DecimalFieldMetadata, EngineError> {
    DecimalFieldMetadata::new(
        metadata.display_precision,
        metadata.measurement_precision,
        metadata.currency_scale,
    )
    .map_err(|error| EngineError::Schema(error.to_string()))
}

fn parse_symbol(value: &str) -> Result<Symbol, EngineError> {
    Symbol::new(value.to_owned()).map_err(|error| EngineError::Schema(error.to_string()))
}

fn generate_id<T: DomainId>() -> Result<T, EngineError> {
    worlddb_core::storage_internal::generate_project_bootstrap_id::<T>()
        .map_err(|error| EngineError::Schema(error.to_string()))
}

fn revision_from_u64(value: u64) -> Result<Revision, EngineError> {
    Revision::new(value).map_err(|error| EngineError::Schema(error.to_string()))
}

fn schema_snapshot_view(snapshot: &SchemaSnapshot) -> Result<SchemaSnapshotView, EngineError> {
    let definitions = snapshot
        .definitions()
        .iter()
        .map(schema_definition_view)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SchemaSnapshotView {
        revision: snapshot.schema_revision().revision().value(),
        fingerprint: fingerprint_hex(&snapshot.fingerprint()),
        definitions,
    })
}

fn schema_definition_view(
    definition: &SchemaDefinition,
) -> Result<SchemaDefinitionView, EngineError> {
    let (family, identity, symbol, lifecycle, revision, description, details, record) =
        match definition {
            SchemaDefinition::EntityType(value) => (
                "entity_type",
                value.entity_type_id().to_string(),
                value.symbol().as_str().to_owned(),
                lifecycle_word(value.lifecycle()),
                value.created_revision(),
                value.description().map(String::from),
                serde_json::json!({ "description": value.description() }),
                Record::EntityTypeDefinition(value.clone()),
            ),
            SchemaDefinition::Predicate(value) => (
                "predicate",
                value.predicate_id().to_string(),
                value.symbol().as_str().to_owned(),
                lifecycle_word(value.lifecycle()),
                value.created_revision(),
                None,
                serde_json::json!({
                    "subject_constraint": entity_constraint_text(value.subject_constraint()),
                    "value_kind": value_kind_word(value.value_kind()),
                    "object_constraint": value.object_constraint().map(entity_constraint_text),
                    "cardinality": cardinality_word(value.cardinality()),
                    "resolution_policy": resolution_word(value.resolution_policy()),
                    "constraints": value.constraints().rules().iter().map(constraint_text).collect::<Vec<_>>(),
                    "decimal_metadata": decimal_metadata_json(value.decimal_metadata()),
                }),
                Record::PredicateDefinition(value.clone()),
            ),
            SchemaDefinition::EventKind(value) => (
                "event_kind",
                value.event_kind_id().to_string(),
                value.symbol().as_str().to_owned(),
                lifecycle_word(value.lifecycle()),
                value.created_revision(),
                None,
                serde_json::json!({
                    "roles": value.roles().iter().map(|role| serde_json::json!({
                        "identity": role.event_role_id().to_string(),
                        "symbol": role.symbol().as_str(),
                        "entity_constraint": entity_constraint_text(role.entity_constraint()),
                        "min_participants": role.cardinality().min(),
                        "max_participants": role.cardinality().max(),
                    })).collect::<Vec<_>>(),
                    "attributes": value.attributes().iter().map(|attribute| serde_json::json!({
                        "identity": attribute.event_attribute_id().to_string(),
                        "symbol": attribute.symbol().as_str(),
                        "value_kind": value_kind_word(attribute.value_kind()),
                        "object_constraint": attribute.object_constraint().map(entity_constraint_text),
                        "constraints": attribute.constraints().rules().iter().map(constraint_text).collect::<Vec<_>>(),
                        "decimal_metadata": decimal_metadata_json(attribute.decimal_metadata()),
                        "required": attribute.required(),
                    })).collect::<Vec<_>>(),
                    "event_time": event_time_text(value.event_time_constraint()),
                }),
                Record::EventKindDefinition(value.clone()),
            ),
            SchemaDefinition::Timeline(value) => {
                let details = match value.calendar() {
                    TimelineCalendarProfile::None => serde_json::json!({
                        "calendar_profile": "none",
                        "epoch_unix_nanoseconds": null,
                    }),
                    TimelineCalendarProfile::ProlepticGregorianUtc {
                        epoch_unix_nanoseconds,
                    } => serde_json::json!({
                        "calendar_profile": "proleptic_gregorian_utc",
                        "epoch_unix_nanoseconds": epoch_unix_nanoseconds.to_string(),
                    }),
                };
                (
                    "timeline",
                    value.timeline_id().to_string(),
                    value.symbol().as_str().to_owned(),
                    lifecycle_word(value.lifecycle()),
                    value.created_revision(),
                    None,
                    details,
                    Record::TimelineDefinition(value.clone()),
                )
            }
            SchemaDefinition::TimeUnit(value) => (
                "time_unit",
                value.symbol().as_str().to_owned(),
                value.symbol().as_str().to_owned(),
                lifecycle_word(value.lifecycle()),
                value.created_revision(),
                None,
                serde_json::json!({
                    "nanoseconds_per_tick": value.nanoseconds_per_tick().get().to_string(),
                }),
                Record::TimeUnitDefinition(value.clone()),
            ),
            SchemaDefinition::Layer(value) => (
                "layer",
                value.layer_id().to_string(),
                value.symbol().as_str().to_owned(),
                lifecycle_word(value.lifecycle()),
                value.created_revision().revision(),
                value.description().map(String::from),
                serde_json::json!({ "precedence_rank": value.precedence_rank() }),
                Record::LayerDefinition(value.clone()),
            ),
            SchemaDefinition::LayerSnapshot(value) => (
                "layer_snapshot",
                format!("base:{}", value.base_layer_id()),
                "Layers".to_owned(),
                "active",
                value.revision().revision(),
                None,
                serde_json::json!({
                    "base_layer_id": value.base_layer_id().to_string(),
                    "layers": value.definitions().iter().map(|layer| serde_json::json!({
                        "identity": layer.layer_id().to_string(),
                        "symbol": layer.symbol().as_str(),
                        "description": layer.description(),
                        "precedence_rank": layer.precedence_rank(),
                        "lifecycle": lifecycle_word(layer.lifecycle()),
                        "created_revision": layer.created_revision().revision().value(),
                    })).collect::<Vec<_>>(),
                }),
                Record::LayerSchemaSnapshot(value.clone()),
            ),
        };
    let encoded = worlddb_core::encode_record(&record)
        .map_err(|error| EngineError::Schema(error.to_string()))?;
    Ok(SchemaDefinitionView {
        family: family.to_owned(),
        identity,
        symbol,
        lifecycle: lifecycle.to_owned(),
        lifecycle_help: match lifecycle {
            "deprecated" => {
                "New writes require opt-in, SchemaDeprecatedWrite permission, and a typed warning."
            }
            "retired" => "New writes are blocked; historical reads remain available.",
            _ => "New references and writes are allowed by schema lifecycle.",
        }
        .to_owned(),
        created_revision: revision.value(),
        description,
        details,
        canonical_record_hex: hex(&encoded),
    })
}

fn lifecycle_word(value: Lifecycle) -> &'static str {
    match value {
        Lifecycle::Active => "active",
        Lifecycle::Deprecated => "deprecated",
        Lifecycle::Retired => "retired",
    }
}

fn value_kind_word(value: ValueKind) -> &'static str {
    match value {
        ValueKind::Bool => "bool",
        ValueKind::Int => "int",
        ValueKind::UInt => "uint",
        ValueKind::Decimal => "decimal",
        ValueKind::String => "string",
        ValueKind::Symbol => "symbol",
        ValueKind::Entity => "entity",
        ValueKind::Time => "time",
        ValueKind::Duration => "duration",
        ValueKind::Bytes => "bytes",
    }
}

fn cardinality_word(value: Cardinality) -> &'static str {
    match value {
        Cardinality::Single => "single",
        Cardinality::Multi => "multi",
    }
}

fn resolution_word(value: ResolutionPolicy) -> &'static str {
    match value {
        ResolutionPolicy::SingleValueReplace => "single_value_replace",
        ResolutionPolicy::MultiValueOverlay => "multi_value_overlay",
        ResolutionPolicy::MultiValueReplace => "multi_value_replace",
    }
}

fn entity_constraint_text(value: EntityTypeConstraint) -> String {
    match value {
        EntityTypeConstraint::AnyEntity => "any_entity".to_owned(),
        EntityTypeConstraint::Exact(id) => format!("entity_type:{id}"),
    }
}

fn decimal_metadata_json(value: Option<DecimalFieldMetadata>) -> serde_json::Value {
    match value {
        Some(value) => serde_json::json!({
            "display_precision": value.display_precision(),
            "measurement_precision": value.measurement_precision(),
            "currency_scale": value.currency_scale(),
        }),
        None => serde_json::Value::Null,
    }
}

fn event_time_text(value: EventTimeConstraint) -> serde_json::Value {
    let form = match value.form() {
        EventTimeForm::InstantOnly => "instant_only",
        EventTimeForm::SpanOnly => "span_only",
        EventTimeForm::InstantOrSpan => "instant_or_span",
        EventTimeForm::OpenSpanAllowed => "open_span_allowed",
    };
    let period = value.max_calendar_span().map(|period| {
        serde_json::json!({ "years": period.years(), "months": period.months(), "days": period.days() })
    });
    serde_json::json!({ "form": form, "max_calendar_span": period })
}

fn constraint_text(value: &ValueConstraint) -> String {
    match value {
        ValueConstraint::BoolSet(values) => format!("bool_set:{:?}", values.as_slice()),
        ValueConstraint::IntRange(range) => format!(
            "int_range:{}..={}",
            range
                .min()
                .map_or_else(|| "-∞".to_owned(), ToString::to_string),
            range
                .max()
                .map_or_else(|| "+∞".to_owned(), ToString::to_string)
        ),
        ValueConstraint::UIntRange(range) => format!(
            "uint_range:{}..={}",
            range
                .min()
                .map_or_else(|| "0".to_owned(), ToString::to_string),
            range
                .max()
                .map_or_else(|| "+∞".to_owned(), ToString::to_string)
        ),
        ValueConstraint::DecimalRange(range) => format!(
            "decimal_range:{}..={}",
            range.min().map_or_else(|| "-∞".to_owned(), decimal_text),
            range.max().map_or_else(|| "+∞".to_owned(), decimal_text)
        ),
        ValueConstraint::StringByteLength(range) => format!(
            "string_byte_length:{}..={}",
            range
                .min()
                .map_or_else(|| "0".to_owned(), ToString::to_string),
            range
                .max()
                .map_or_else(|| "+∞".to_owned(), ToString::to_string)
        ),
        ValueConstraint::SymbolSet(values) => format!(
            "symbol_set:{}",
            values
                .as_slice()
                .iter()
                .map(Symbol::as_str)
                .collect::<Vec<_>>()
                .join(",")
        ),
        ValueConstraint::TimeRange(range) => format!(
            "time_range:{}..={}",
            range.min().map_or_else(|| "-∞".to_owned(), time_text),
            range.max().map_or_else(|| "+∞".to_owned(), time_text)
        ),
        ValueConstraint::DurationRange(range) => format!(
            "duration_ns:{}..={}",
            range
                .min()
                .map_or_else(|| "-∞".to_owned(), |value| value.nanoseconds().to_string()),
            range
                .max()
                .map_or_else(|| "+∞".to_owned(), |value| value.nanoseconds().to_string())
        ),
        ValueConstraint::BytesLength(range) => format!(
            "bytes_length:{}..={}",
            range
                .min()
                .map_or_else(|| "0".to_owned(), ToString::to_string),
            range
                .max()
                .map_or_else(|| "+∞".to_owned(), ToString::to_string)
        ),
    }
}

fn decimal_text(value: &Decimal) -> String {
    value
        .to_canonical_string(256)
        .unwrap_or_else(|_| "invalid_decimal".to_owned())
}

fn time_text(value: &Time) -> String {
    format!("{}:{} {}", value.timeline_id(), value.ticks(), value.unit())
}

fn fingerprint_hex(value: &[u8; 32]) -> String {
    hex(value)
}

fn hex(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len().saturating_mul(2));
    for byte in value {
        let high = usize::from(byte >> 4);
        let low = usize::from(byte & 0x0f);
        if let Some(character) = DIGITS.get(high) {
            output.push(char::from(*character));
        }
        if let Some(character) = DIGITS.get(low) {
            output.push(char::from(*character));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ConstraintDraft, EventTimeDraft, EventTimeFormDraft, SchemaCommand, SchemaDefinitionDraft,
        SchemaFamily, SchemaLifecycle, SchemaLifecycleUpdateDraft, SchemaModeInput, SchemaResponse,
        SchemaSnapshotView, TimeBoundDraft, TimelineCalendarProfileDraft, build_schema_record,
        constraint, schema_definition_view,
    };
    use crate::{Request, Response};
    use worlddb_core::{
        Lifecycle, Record, Revision, SchemaDefinition, TimelineCalendarProfile, ValueConstraint,
    };

    #[test]
    fn every_typed_constraint_draft_preserves_its_closed_value_kind() {
        let time_bound = TimeBoundDraft {
            timeline_id: "018f0000-0000-7000-8000-000000000001".to_owned(),
            ticks: "-170141183460469231731687303715884105728".to_owned(),
            unit: "nanosecond".to_owned(),
        };
        let cases = vec![
            (
                ConstraintDraft::BoolSet {
                    values: vec![false, true],
                },
                "bool_set",
            ),
            (
                ConstraintDraft::IntRange {
                    min: Some("-170141183460469231731687303715884105728".to_owned()),
                    max: Some("170141183460469231731687303715884105727".to_owned()),
                },
                "int_range",
            ),
            (
                ConstraintDraft::UIntRange {
                    min: Some("0".to_owned()),
                    max: Some("340282366920938463463374607431768211455".to_owned()),
                },
                "uint_range",
            ),
            (
                ConstraintDraft::DecimalRange {
                    min: Some("0".to_owned()),
                    max: Some("1.25".to_owned()),
                },
                "decimal_range",
            ),
            (
                ConstraintDraft::StringByteLength {
                    min: Some("1".to_owned()),
                    max: Some("4096".to_owned()),
                },
                "string_byte_length",
            ),
            (
                ConstraintDraft::SymbolSet {
                    values: vec!["alpha".to_owned(), "beta_2".to_owned()],
                },
                "symbol_set",
            ),
            (
                ConstraintDraft::TimeRange {
                    min: Some(time_bound.clone()),
                    max: Some(TimeBoundDraft {
                        ticks: "170141183460469231731687303715884105727".to_owned(),
                        ..time_bound
                    }),
                },
                "time_range",
            ),
            (
                ConstraintDraft::DurationRange {
                    min_ns: Some("-170141183460469231731687303715884105728".to_owned()),
                    max_ns: Some("170141183460469231731687303715884105727".to_owned()),
                },
                "duration_range",
            ),
            (
                ConstraintDraft::BytesLength {
                    min: Some("0".to_owned()),
                    max: Some("1048576".to_owned()),
                },
                "bytes_length",
            ),
        ];
        for (draft, expected) in cases {
            let value = constraint(draft).expect("valid typed constraint draft");
            let actual = match value {
                ValueConstraint::BoolSet(_) => "bool_set",
                ValueConstraint::IntRange(_) => "int_range",
                ValueConstraint::UIntRange(_) => "uint_range",
                ValueConstraint::DecimalRange(_) => "decimal_range",
                ValueConstraint::StringByteLength(_) => "string_byte_length",
                ValueConstraint::SymbolSet(_) => "symbol_set",
                ValueConstraint::TimeRange(_) => "time_range",
                ValueConstraint::DurationRange(_) => "duration_range",
                ValueConstraint::BytesLength(_) => "bytes_length",
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn schema_commands_have_stable_closed_json_shapes() {
        let snapshot: SchemaCommand = serde_json::from_value(json!({
            "command": "snapshot",
            "mode": { "mode": "historical", "recorded_as_of": 12 }
        }))
        .expect("historical snapshot command");
        assert!(matches!(
            snapshot,
            SchemaCommand::Snapshot {
                mode: SchemaModeInput::Historical { recorded_as_of: 12 }
            }
        ));

        let batch = SchemaCommand::SetLifecycleBatch {
            expected_base_revision: 12,
            updates: vec![SchemaLifecycleUpdateDraft {
                family: SchemaFamily::EntityType,
                identity: "018f0000-0000-7000-8000-000000000001".to_owned(),
                lifecycle: SchemaLifecycle::Retired,
            }],
        };
        let encoded = serde_json::to_value(batch.clone()).expect("serialize lifecycle batch");
        assert_eq!(encoded["command"], "set_lifecycle_batch");
        assert_eq!(encoded["updates"][0]["family"], "entity_type");
        assert_eq!(encoded["updates"][0]["lifecycle"], "retired");

        let request = Request::Schema {
            operation_id: Some("00000000-0000-7000-8000-000000000001".to_owned()),
            command: batch,
        };
        let request_wire = serde_json::to_value(&request).expect("serialize sidecar request");
        assert_eq!(request_wire["operation"], "schema");
        assert_eq!(
            request_wire["operation_id"],
            "00000000-0000-7000-8000-000000000001"
        );
        let decoded_request: Request =
            serde_json::from_value(request_wire).expect("decode sidecar request");
        assert!(matches!(decoded_request, Request::Schema { .. }));

        let response = Response::Schema {
            result: SchemaResponse::Snapshot(SchemaSnapshotView {
                revision: 12,
                fingerprint: "00".repeat(32),
                definitions: Vec::new(),
            }),
        };
        let response_wire = serde_json::to_value(&response).expect("serialize sidecar response");
        assert_eq!(response_wire["kind"], "schema");
        let decoded_response: Response =
            serde_json::from_value(response_wire).expect("decode sidecar response");
        assert!(matches!(decoded_response, Response::Schema { .. }));

        let malformed = serde_json::from_value::<SchemaCommand>(json!({
            "command": "create",
            "expected_base_revision": 12,
            "definition": { "family": "entity_type", "symbol": "person", "unknown": true }
        }));
        assert!(
            malformed.is_err(),
            "unknown request fields must fail closed"
        );

        let event_time = EventTimeDraft {
            form: EventTimeFormDraft::InstantOnly,
            max_calendar_span: None,
        };
        let event_time_wire = serde_json::to_value(event_time).expect("serialize event time");
        assert_eq!(event_time_wire["form"], "instant_only");

        let timeline = SchemaCommand::Create {
            expected_base_revision: 12,
            definition: SchemaDefinitionDraft::Timeline {
                symbol: "gregorian_utc".to_owned(),
                calendar_profile: TimelineCalendarProfileDraft::ProlepticGregorianUtc {
                    epoch_unix_nanoseconds: "-1".to_owned(),
                },
            },
        };
        let timeline_wire = serde_json::to_value(timeline).expect("serialize timeline draft");
        assert_eq!(timeline_wire["definition"]["family"], "timeline");
        assert_eq!(
            timeline_wire["definition"]["calendar_profile"]["profile"],
            "proleptic_gregorian_utc"
        );
        assert_eq!(
            timeline_wire["definition"]["calendar_profile"]["epoch_unix_nanoseconds"],
            "-1"
        );

        let time_unit: SchemaCommand = serde_json::from_value(json!({
            "command": "create",
            "expected_base_revision": 12,
            "definition": {
                "family": "time_unit",
                "symbol": "nanosecond",
                "nanoseconds_per_tick": "18446744073709551615"
            }
        }))
        .expect("deserialize exact time-unit draft");
        let encoded = serde_json::to_value(time_unit).expect("serialize time-unit draft");
        assert_eq!(
            encoded["definition"]["nanoseconds_per_tick"],
            "18446744073709551615"
        );
        assert!(
            serde_json::from_value::<SchemaCommand>(json!({
                "command": "create",
                "expected_base_revision": 12,
                "definition": {
                    "family": "time_unit",
                    "symbol": "nanosecond",
                    "nanoseconds_per_tick": "1",
                    "extra": true
                }
            }))
            .is_err()
        );
    }

    #[test]
    fn timeline_and_time_unit_drafts_validate_and_publish_exact_views() {
        let revision = Revision::new(12).expect("non-genesis revision");
        let timeline = build_schema_record(
            SchemaDefinitionDraft::Timeline {
                symbol: "gregorian_utc".to_owned(),
                calendar_profile: TimelineCalendarProfileDraft::ProlepticGregorianUtc {
                    epoch_unix_nanoseconds: "-170141183460469231731687303715884105728".to_owned(),
                },
            },
            revision,
        )
        .expect("valid Gregorian timeline");
        let Record::TimelineDefinition(timeline) = timeline else {
            panic!("timeline draft creates a timeline record");
        };
        assert_eq!(
            timeline.calendar(),
            TimelineCalendarProfile::ProlepticGregorianUtc {
                epoch_unix_nanoseconds: i128::MIN,
            }
        );
        let timeline_view = schema_definition_view(&SchemaDefinition::Timeline(timeline.clone()))
            .expect("timeline snapshot view");
        assert_eq!(timeline_view.family, "timeline");
        assert_eq!(timeline_view.symbol, "gregorian_utc");
        assert_eq!(
            timeline_view.details["epoch_unix_nanoseconds"],
            i128::MIN.to_string()
        );

        let uncalendared = build_schema_record(
            SchemaDefinitionDraft::Timeline {
                symbol: "linear_ticks".to_owned(),
                calendar_profile: TimelineCalendarProfileDraft::None,
            },
            revision,
        )
        .expect("timeline without a civil calendar");
        let Record::TimelineDefinition(uncalendared) = uncalendared else {
            panic!("uncalendared draft creates a timeline record");
        };
        let uncalendared_view = schema_definition_view(&SchemaDefinition::Timeline(uncalendared))
            .expect("uncalendared timeline view");
        assert_eq!(uncalendared_view.details["calendar_profile"], "none");
        assert!(uncalendared_view.details["epoch_unix_nanoseconds"].is_null());

        let time_unit = build_schema_record(
            SchemaDefinitionDraft::TimeUnit {
                symbol: "max_scale".to_owned(),
                nanoseconds_per_tick: u64::MAX.to_string(),
            },
            revision,
        )
        .expect("maximum exact u64 scale");
        let Record::TimeUnitDefinition(time_unit) = time_unit else {
            panic!("time-unit draft creates a time-unit record");
        };
        assert_eq!(time_unit.nanoseconds_per_tick().get(), u64::MAX);
        let time_unit_view = schema_definition_view(&SchemaDefinition::TimeUnit(time_unit.clone()))
            .expect("time-unit snapshot view");
        assert_eq!(time_unit_view.family, "time_unit");
        assert_eq!(time_unit_view.identity, "max_scale");
        assert_eq!(
            time_unit_view.details["nanoseconds_per_tick"],
            u64::MAX.to_string()
        );

        assert!(
            build_schema_record(
                SchemaDefinitionDraft::Timeline {
                    symbol: "Bad_symbol".to_owned(),
                    calendar_profile: TimelineCalendarProfileDraft::ProlepticGregorianUtc {
                        epoch_unix_nanoseconds: "0".to_owned(),
                    },
                },
                revision,
            )
            .is_err()
        );
        assert!(
            build_schema_record(
                SchemaDefinitionDraft::Timeline {
                    symbol: "out_of_range_epoch".to_owned(),
                    calendar_profile: TimelineCalendarProfileDraft::ProlepticGregorianUtc {
                        epoch_unix_nanoseconds: "170141183460469231731687303715884105728"
                            .to_owned(),
                    },
                },
                revision,
            )
            .is_err()
        );
        assert!(
            build_schema_record(
                SchemaDefinitionDraft::TimeUnit {
                    symbol: "zero_scale".to_owned(),
                    nanoseconds_per_tick: "0".to_owned(),
                },
                revision,
            )
            .is_err()
        );
        assert!(
            build_schema_record(
                SchemaDefinitionDraft::TimeUnit {
                    symbol: "overflow_scale".to_owned(),
                    nanoseconds_per_tick: "18446744073709551616".to_owned(),
                },
                revision,
            )
            .is_err()
        );

        let deprecated = timeline
            .revise_lifecycle(Lifecycle::Deprecated, Revision::new(13).expect("revision"))
            .expect("timeline lifecycle can advance");
        assert_eq!(deprecated.lifecycle(), Lifecycle::Deprecated);
    }
}
