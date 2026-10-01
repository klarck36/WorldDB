//! Referential and context validation for atomic assertion/event write batches.

use std::collections::BTreeMap;
use std::fmt;

use crate::assertions::AssertionDraft;
use crate::catalog::{EntityCatalogSnapshot, HistorySpaceCatalog, PerspectiveCatalogSnapshot};
use crate::context::PerspectiveScope;
use crate::events::EventDraft;
use crate::ids::{
    EntityId, EventKindId, HistorySpaceId, LayerId, PerspectiveId, PredicateId, TimelineId,
};
use crate::layers::LayerSchemaSnapshot;
use crate::schema::{EntityTypeConstraint, EventKindDefinition, Lifecycle, PredicateDefinition};
use crate::schema_history::{SchemaDefinition, SchemaSnapshot};
use crate::temporal::EventTime;
use crate::values::{Symbol, Value};

/// Pinned project catalogs used by one write validation attempt.
///
/// The caller supplies every view from the same post-transaction snapshot.
/// Timeline lifecycle is passed separately until timeline definitions join the
/// public schema record model.
pub struct WriteReferenceSnapshot<'a> {
    /// Complete retained HistorySpace ancestry at the selected snapshot.
    pub history_spaces: &'a HistorySpaceCatalog,
    /// Complete layer state at the selected schema revision.
    pub layers: &'a LayerSchemaSnapshot,
    /// Entity identities and retirements at the selected shared revision.
    pub entities: &'a EntityCatalogSnapshot,
    /// Perspective metadata and retirements at the selected shared revision.
    pub perspectives: &'a PerspectiveCatalogSnapshot,
    /// Schema definitions at the selected post-transaction revision.
    pub schema: &'a SchemaSnapshot,
    /// Lifecycle states for registered timelines in that same schema view.
    pub timelines: &'a BTreeMap<TimelineId, Lifecycle>,
    /// Lifecycle states for registered time units in that same schema view.
    pub time_units: &'a BTreeMap<Symbol, Lifecycle>,
}

/// Assertion and Event drafts whose project references and contexts resolve.
pub struct ValidatedWriteReferenceBatch {
    assertions: Vec<AssertionDraft>,
    events: Vec<EventDraft>,
}

impl ValidatedWriteReferenceBatch {
    /// Returns all assertion drafts after whole-batch reference validation.
    #[must_use]
    pub fn assertions(&self) -> &[AssertionDraft] {
        &self.assertions
    }

    /// Returns all event drafts after whole-batch reference validation.
    #[must_use]
    pub fn events(&self) -> &[EventDraft] {
        &self.events
    }

    /// Consumes the validated batch into its two record families.
    #[must_use]
    pub fn into_parts(self) -> (Vec<AssertionDraft>, Vec<EventDraft>) {
        (self.assertions, self.events)
    }
}

/// Validates all assertion and event references before any batch can publish.
///
/// It checks HistorySpace, Layer, Perspective, Entity, EntityType, Predicate,
/// EventKind, participant-role, and Timeline references. Retired referenced
/// definitions are rejected. Drafts are returned together only if every check
/// succeeds; errors never expose a partially accepted batch.
pub fn validate_write_references(
    assertions: Vec<AssertionDraft>,
    events: Vec<EventDraft>,
    snapshot: &WriteReferenceSnapshot<'_>,
) -> Result<ValidatedWriteReferenceBatch, WriteReferenceValidationError> {
    let selected_revision = snapshot.schema.schema_revision().revision();
    for (catalog, revision) in [
        (
            ReferenceCatalog::Layers,
            snapshot.layers.revision().revision(),
        ),
        (ReferenceCatalog::Entities, snapshot.entities.revision()),
        (
            ReferenceCatalog::Perspectives,
            snapshot.perspectives.revision(),
        ),
    ] {
        if revision != selected_revision {
            return Err(WriteReferenceValidationError::CatalogRevisionMismatch {
                catalog,
                expected: selected_revision,
                actual: revision,
            });
        }
    }

    for assertion in &assertions {
        let context = assertion.context();
        require_history_space(snapshot, context.history_space_id())?;
        require_layer(snapshot, context.layer_id())?;
        if let PerspectiveScope::Perspective(id) = context.perspective_scope() {
            require_perspective(snapshot, id)?;
        }

        let subject = assertion.subject().entity_id();
        let entity = require_entity(snapshot, subject)?;
        let predicate = find_predicate(snapshot.schema, assertion.predicate_id())?;
        require_active_entity_type(snapshot, entity.entity_type_id())?;
        require_entity_type(
            subject,
            entity.entity_type_id(),
            predicate.subject_constraint(),
        )?;
        if let Value::Entity(target) = assertion.value() {
            let target_entity = require_entity(snapshot, *target)?;
            require_active_entity_type(snapshot, target_entity.entity_type_id())?;
            let constraint = predicate.object_constraint().ok_or(
                WriteReferenceValidationError::MissingEntityConstraint(predicate.predicate_id()),
            )?;
            require_entity_type(*target, target_entity.entity_type_id(), constraint)?;
        }
        if let Value::Time(time) = assertion.value() {
            require_timeline_id(snapshot, time.timeline_id())?;
            match snapshot.time_units.get(time.unit()) {
                None => {
                    return Err(WriteReferenceValidationError::UnknownTimeUnit(
                        time.unit().clone(),
                    ));
                }
                Some(Lifecycle::Active) => {}
                Some(Lifecycle::Deprecated) => {
                    return Err(WriteReferenceValidationError::DeprecatedTimeUnit(
                        time.unit().clone(),
                    ));
                }
                Some(Lifecycle::Retired) => {
                    return Err(WriteReferenceValidationError::RetiredTimeUnit(
                        time.unit().clone(),
                    ));
                }
            }
        }
        require_timeline_id(snapshot, assertion.validity().interval().timeline().id())?;
    }

    for event in &events {
        require_history_space(snapshot, event.history_space_id())?;
        require_layer(snapshot, event.layer_id())?;
        let kind = find_event_kind(snapshot.schema, event.event_kind_id())?;
        if kind.created_revision() != event.event_kind_revision() {
            return Err(WriteReferenceValidationError::EventKindRevisionMismatch {
                event_kind_id: event.event_kind_id(),
                draft_revision: event.event_kind_revision(),
                schema_revision: kind.created_revision(),
            });
        }
        if kind.lifecycle() == Lifecycle::Retired {
            return Err(WriteReferenceValidationError::RetiredEventKind(
                kind.event_kind_id(),
            ));
        }
        for participant in event.participants().as_slice() {
            let entity = require_entity(snapshot, participant.entity_id())?;
            require_active_entity_type(snapshot, entity.entity_type_id())?;
            let role = kind
                .roles()
                .iter()
                .find(|role| role.event_role_id() == participant.role_id())
                .ok_or(WriteReferenceValidationError::UnknownEventRole {
                    event_kind_id: kind.event_kind_id(),
                    role_id: participant.role_id(),
                })?;
            require_entity_type(
                participant.entity_id(),
                entity.entity_type_id(),
                role.entity_constraint(),
            )?;
        }
        for attribute in event.attributes().as_slice() {
            if let Value::Entity(target) = attribute.value() {
                let entity = require_entity(snapshot, *target)?;
                require_active_entity_type(snapshot, entity.entity_type_id())?;
                let definition = kind
                    .attributes()
                    .iter()
                    .find(|definition| definition.event_attribute_id() == attribute.attribute_id())
                    .ok_or(WriteReferenceValidationError::UnknownEventAttribute {
                        event_kind_id: kind.event_kind_id(),
                        attribute_id: attribute.attribute_id(),
                    })?;
                let constraint = definition.object_constraint().ok_or(
                    WriteReferenceValidationError::MissingEventEntityConstraint {
                        event_kind_id: kind.event_kind_id(),
                        attribute_id: attribute.attribute_id(),
                    },
                )?;
                require_entity_type(*target, entity.entity_type_id(), constraint)?;
            }
        }
        for timeline_id in event_timeline_ids(event.event_time()) {
            require_timeline_id(snapshot, timeline_id)?;
        }
    }

    Ok(ValidatedWriteReferenceBatch { assertions, events })
}

fn require_history_space(
    snapshot: &WriteReferenceSnapshot<'_>,
    id: HistorySpaceId,
) -> Result<(), WriteReferenceValidationError> {
    snapshot
        .history_spaces
        .definition(id)
        .map(|_| ())
        .ok_or(WriteReferenceValidationError::UnknownHistorySpace(id))
}

fn require_layer(
    snapshot: &WriteReferenceSnapshot<'_>,
    id: LayerId,
) -> Result<(), WriteReferenceValidationError> {
    let definition = snapshot
        .layers
        .definition(id)
        .ok_or(WriteReferenceValidationError::UnknownLayer(id))?;
    if definition.lifecycle() == Lifecycle::Retired {
        return Err(WriteReferenceValidationError::RetiredLayer(id));
    }
    Ok(())
}

fn require_perspective(
    snapshot: &WriteReferenceSnapshot<'_>,
    id: PerspectiveId,
) -> Result<(), WriteReferenceValidationError> {
    if snapshot.perspectives.latest_definition(id).is_none() {
        return Err(WriteReferenceValidationError::UnknownPerspective(id));
    }
    if snapshot.perspectives.is_retired(id) {
        return Err(WriteReferenceValidationError::RetiredPerspective(id));
    }
    Ok(())
}

fn require_entity(
    snapshot: &WriteReferenceSnapshot<'_>,
    id: EntityId,
) -> Result<crate::catalog::Entity, WriteReferenceValidationError> {
    let entity = snapshot
        .entities
        .entity(id)
        .ok_or(WriteReferenceValidationError::UnknownEntity(id))?;
    if snapshot.entities.is_retired(id) {
        return Err(WriteReferenceValidationError::RetiredEntity(id));
    }
    Ok(entity)
}

fn require_active_entity_type(
    snapshot: &WriteReferenceSnapshot<'_>,
    id: crate::ids::EntityTypeId,
) -> Result<(), WriteReferenceValidationError> {
    let definition = snapshot
        .schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::EntityType(value) if value.entity_type_id() == id => Some(value),
            _ => None,
        })
        .ok_or(WriteReferenceValidationError::UnknownEntityType(id))?;
    if definition.lifecycle() == Lifecycle::Retired {
        return Err(WriteReferenceValidationError::RetiredEntityType(id));
    }
    Ok(())
}

fn require_entity_type(
    entity_id: EntityId,
    actual: crate::ids::EntityTypeId,
    constraint: EntityTypeConstraint,
) -> Result<(), WriteReferenceValidationError> {
    if let EntityTypeConstraint::Exact(expected) = constraint {
        if actual != expected {
            return Err(WriteReferenceValidationError::EntityTypeMismatch {
                entity_id,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

fn find_predicate(
    schema: &SchemaSnapshot,
    id: PredicateId,
) -> Result<&PredicateDefinition, WriteReferenceValidationError> {
    schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::Predicate(value) if value.predicate_id() == id => Some(value),
            _ => None,
        })
        .ok_or(WriteReferenceValidationError::UnknownPredicate(id))
}

fn find_event_kind(
    schema: &SchemaSnapshot,
    id: EventKindId,
) -> Result<&EventKindDefinition, WriteReferenceValidationError> {
    schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::EventKind(value) if value.event_kind_id() == id => Some(value),
            _ => None,
        })
        .ok_or(WriteReferenceValidationError::UnknownEventKind(id))
}

fn require_timeline_id(
    snapshot: &WriteReferenceSnapshot<'_>,
    id: TimelineId,
) -> Result<(), WriteReferenceValidationError> {
    match snapshot.timelines.get(&id) {
        None => Err(WriteReferenceValidationError::UnknownTimeline(id)),
        Some(Lifecycle::Retired) => Err(WriteReferenceValidationError::RetiredTimeline(id)),
        Some(Lifecycle::Active) => Ok(()),
        Some(Lifecycle::Deprecated) => {
            Err(WriteReferenceValidationError::DeprecatedTimelineOptInRequired(id))
        }
    }
}

fn event_timeline_ids(event_time: EventTime) -> Vec<TimelineId> {
    match event_time {
        EventTime::Instant(time) => vec![time.timeline().id()],
        EventTime::Span { start, end } => {
            let mut timelines = vec![start.timeline().id()];
            if let Some(end) = end {
                timelines.push(end.timeline().id());
            }
            timelines
        }
    }
}

/// A closed project-reference or cross-record context failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriteReferenceValidationError {
    /// One catalog does not belong to the selected post-transaction revision.
    CatalogRevisionMismatch {
        /// Catalog with the mismatched revision.
        catalog: ReferenceCatalog,
        /// Revision selected by the schema snapshot.
        expected: crate::ids::Revision,
        /// Revision carried by the catalog.
        actual: crate::ids::Revision,
    },
    /// A draft names a HistorySpace absent from the selected catalog.
    UnknownHistorySpace(HistorySpaceId),
    /// A draft names a Layer absent from the selected schema.
    UnknownLayer(LayerId),
    /// A retired Layer cannot receive a new record.
    RetiredLayer(LayerId),
    /// A draft names a Perspective absent from the selected catalog.
    UnknownPerspective(PerspectiveId),
    /// A retired Perspective cannot receive a new assertion.
    RetiredPerspective(PerspectiveId),
    /// A referenced Entity is absent from the selected catalog.
    UnknownEntity(EntityId),
    /// A retired Entity cannot be used in a new record.
    RetiredEntity(EntityId),
    /// A referenced EntityType is absent from the selected schema.
    UnknownEntityType(crate::ids::EntityTypeId),
    /// A retired EntityType cannot be used by a new record.
    RetiredEntityType(crate::ids::EntityTypeId),
    /// A typed Entity reference violates its schema constraint.
    EntityTypeMismatch {
        /// Referenced Entity.
        entity_id: EntityId,
        /// Required type.
        expected: crate::ids::EntityTypeId,
        /// Actual permanent type.
        actual: crate::ids::EntityTypeId,
    },
    /// An Assertion names a Predicate absent from the selected schema.
    UnknownPredicate(PredicateId),
    /// An Event names an EventKind absent from the selected schema.
    UnknownEventKind(EventKindId),
    /// The Event draft was built against a different EventKind revision.
    EventKindRevisionMismatch {
        /// EventKind identity.
        event_kind_id: EventKindId,
        /// Revision used when the draft was constructed.
        draft_revision: crate::ids::Revision,
        /// Revision in the selected schema snapshot.
        schema_revision: crate::ids::Revision,
    },
    /// A retired EventKind cannot receive new Events.
    RetiredEventKind(EventKindId),
    /// An Event participant role is not in the selected EventKind.
    UnknownEventRole {
        /// EventKind identity.
        event_kind_id: EventKindId,
        /// Participant role identity.
        role_id: crate::ids::EventRoleId,
    },
    /// An entity-valued Predicate lacks its mandatory entity constraint.
    MissingEntityConstraint(PredicateId),
    /// An Event attribute is absent from the selected EventKind.
    UnknownEventAttribute {
        /// EventKind identity.
        event_kind_id: EventKindId,
        /// Attribute identity.
        attribute_id: crate::ids::EventAttributeId,
    },
    /// An entity-valued Event attribute lacks its mandatory constraint.
    MissingEventEntityConstraint {
        /// EventKind identity.
        event_kind_id: EventKindId,
        /// Attribute identity.
        attribute_id: crate::ids::EventAttributeId,
    },
    /// A time endpoint names no Timeline in the selected schema state.
    UnknownTimeline(TimelineId),
    /// A retired Timeline cannot be used for new time values.
    RetiredTimeline(TimelineId),
    /// A deprecated Timeline needs the explicit time-write authorization path.
    DeprecatedTimelineOptInRequired(TimelineId),
    /// A time value names no registered TimeUnit symbol.
    UnknownTimeUnit(Symbol),
    /// A deprecated TimeUnit needs an explicit write authorization path.
    DeprecatedTimeUnit(Symbol),
    /// A retired TimeUnit cannot be used for a new time value.
    RetiredTimeUnit(Symbol),
}

/// Versioned catalog family checked for snapshot alignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceCatalog {
    /// Layer schema state.
    Layers,
    /// Entity identities and retirement state.
    Entities,
    /// Perspective metadata and retirement state.
    Perspectives,
}

impl fmt::Display for WriteReferenceValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "write reference validation failed: {self:?}")
    }
}

impl std::error::Error for WriteReferenceValidationError {}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::error::Error;
    use std::fmt;

    use super::{
        ReferenceCatalog, WriteReferenceSnapshot, WriteReferenceValidationError,
        validate_write_references,
    };
    use crate::assertions::{AssertionDraft, Polarity, Subject};
    use crate::catalog::{
        Entity, EntityCatalogSnapshot, HistorySpaceCatalog, HistorySpaceDefinition,
        PerspectiveCatalogSnapshot,
    };
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::events::{EventDraft, EventParticipant};
    use crate::ids::{
        DomainId, EntityId, EntityTypeId, EventKindId, EventRoleId, HistorySpaceId, LayerId,
        PerspectiveId, PolicyRuleId, PredicateId, PrincipalId, Revision, SchemaRevision,
        TimelineId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
    use crate::revision_backend::RevisionBackend;
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, EntityTypeDefinition,
        EventKindDefinition, EventRoleDefinition, EventTimeConstraint, EventTimeForm, Lifecycle,
        PredicateDefinition, PredicateDefinitionSpec, ResolutionPolicy, RoleCardinality, ValueKind,
    };
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, PolicyScope, PolicySubject,
        SecurityPolicySnapshot,
    };
    use crate::temporal::{AssertionValidity, EventTime, TimeInterval, Timeline, WorldTime};
    use crate::values::{Symbol, Value};
    use crate::write_authorization::{WriteAuthorizationError, authorize_validated_write_batch};

    #[derive(Debug)]
    enum TestError {
        Id(crate::IdValidationError),
        Catalog(crate::catalog::HistorySpaceError),
        Entity(crate::catalog::EntityCatalogError),
        Perspective(crate::catalog::PerspectiveCatalogError),
        Layer(crate::layers::LayerSchemaError),
        Context(crate::context::ContextError),
        Schema(crate::schema_history::SchemaHistoryError),
        Definition(crate::schema::SchemaDefinitionError),
        Symbol(crate::values::SymbolError),
        Temporal(crate::TemporalError),
        Event(crate::EventRecordError),
        EventRelation(crate::EventRelationError),
        Security(crate::SecurityPolicyError),
        Validation(WriteReferenceValidationError),
        SchemaWrite(crate::SchemaWriteValidationError),
        CrossRecord(crate::WriteCrossRecordValidationError),
        Message(String),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Catalog(error) => write!(formatter, "{error}"),
                Self::Entity(error) => write!(formatter, "{error}"),
                Self::Perspective(error) => write!(formatter, "{error}"),
                Self::Layer(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
                Self::Schema(error) => write!(formatter, "{error}"),
                Self::Definition(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
                Self::Temporal(error) => write!(formatter, "{error}"),
                Self::Event(error) => write!(formatter, "{error}"),
                Self::EventRelation(error) => write!(formatter, "{error}"),
                Self::Security(error) => write!(formatter, "{error}"),
                Self::Validation(error) => write!(formatter, "{error}"),
                Self::SchemaWrite(error) => write!(formatter, "{error}"),
                Self::CrossRecord(error) => write!(formatter, "{error}"),
                Self::Message(message) => formatter.write_str(message),
            }
        }
    }

    impl Error for TestError {}

    macro_rules! take {
        ($value:expr) => {
            match $value {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    macro_rules! conversion {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    conversion!(crate::IdValidationError, Id);
    conversion!(crate::catalog::HistorySpaceError, Catalog);
    conversion!(crate::catalog::EntityCatalogError, Entity);
    conversion!(crate::catalog::PerspectiveCatalogError, Perspective);
    conversion!(crate::layers::LayerSchemaError, Layer);
    conversion!(crate::context::ContextError, Context);
    conversion!(crate::schema_history::SchemaHistoryError, Schema);
    conversion!(crate::schema::SchemaDefinitionError, Definition);
    conversion!(crate::values::SymbolError, Symbol);
    conversion!(crate::TemporalError, Temporal);
    conversion!(crate::EventRecordError, Event);
    conversion!(crate::EventRelationError, EventRelation);
    conversion!(crate::SecurityPolicyError, Security);
    conversion!(WriteReferenceValidationError, Validation);
    conversion!(crate::SchemaWriteValidationError, SchemaWrite);
    conversion!(crate::WriteCrossRecordValidationError, CrossRecord);

    struct Fixture {
        history_spaces: HistorySpaceCatalog,
        layers: LayerSchemaSnapshot,
        entities: EntityCatalogSnapshot,
        perspectives: PerspectiveCatalogSnapshot,
        schema: crate::schema_history::SchemaSnapshot,
        timelines: BTreeMap<TimelineId, Lifecycle>,
        time_units: BTreeMap<Symbol, Lifecycle>,
        history_space_id: HistorySpaceId,
        perspective_id: PerspectiveId,
        layer_id: LayerId,
        retired_layer_id: LayerId,
        entity_id: EntityId,
        predicate_id: PredicateId,
        timeline_id: TimelineId,
        event_kind: EventKindDefinition,
        event_role_id: EventRoleId,
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn fixture() -> Result<Fixture, TestError> {
        fixture_with_predicate_lifecycle(Lifecycle::Active)
    }

    fn fixture_with_predicate_lifecycle(lifecycle: Lifecycle) -> Result<Fixture, TestError> {
        let revision = Revision::FIRST_COMMIT;
        let schema_revision = SchemaRevision::from_published_revision(revision);
        let history_space_id = take!(id::<HistorySpaceId>(1));
        let layer_id = take!(id::<LayerId>(2));
        let retired_layer_id = take!(id::<LayerId>(3));
        let entity_id = take!(id::<EntityId>(4));
        let entity_type_id = take!(id::<EntityTypeId>(5));
        let predicate_id = take!(id::<PredicateId>(6));
        let timeline_id = take!(id::<TimelineId>(7));
        let event_kind_id = take!(id::<EventKindId>(8));
        let event_role_id = take!(id::<EventRoleId>(9));
        let perspective_id = take!(id::<PerspectiveId>(10));

        let history_spaces = take!(HistorySpaceCatalog::new(vec![take!(
            HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)
        )]));
        let layers = take!(LayerSchemaSnapshot::new(
            schema_revision,
            vec![
                LayerDefinition::new(
                    layer_id,
                    take!(Symbol::new("base")),
                    None,
                    0,
                    Lifecycle::Active,
                    schema_revision,
                ),
                LayerDefinition::new(
                    retired_layer_id,
                    take!(Symbol::new("old")),
                    None,
                    1,
                    Lifecycle::Retired,
                    schema_revision,
                ),
            ],
            layer_id,
        ));
        let entities = take!(EntityCatalogSnapshot::new(
            revision,
            vec![Entity::new(entity_id, entity_type_id, revision)],
            vec![],
        ));
        let perspective = take!(crate::catalog::PerspectiveDefinitionRevision::new(
            perspective_id,
            Some("observer".to_owned()),
            None,
            revision,
        ));
        let perspectives = take!(PerspectiveCatalogSnapshot::new(
            revision,
            vec![perspective],
            vec![],
        ));

        let entity_type = EntityTypeDefinition::new(
            entity_type_id,
            take!(Symbol::new("person")),
            None,
            Lifecycle::Active,
            revision,
        );
        let predicate = take!(PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: take!(Symbol::new("active")),
            subject_constraint: EntityTypeConstraint::Exact(entity_type_id),
            value_kind: ValueKind::Bool,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints: ConstraintSet::unconstrained(),
            decimal_metadata: None,
            lifecycle,
            created_revision: revision,
        }));
        let event_kind = take!(EventKindDefinition::new(
            event_kind_id,
            take!(Symbol::new("visit")),
            vec![EventRoleDefinition::new(
                event_role_id,
                take!(Symbol::new("visitor")),
                EntityTypeConstraint::Exact(entity_type_id),
                take!(RoleCardinality::new(1, Some(1))),
            )],
            vec![],
            take!(EventTimeConstraint::new(EventTimeForm::InstantOnly, None)),
            Lifecycle::Active,
            revision,
        ));
        let mut history = SchemaHistoryReferenceModel::new();
        history.publish(
            revision,
            vec![
                SchemaDefinition::EntityType(entity_type),
                SchemaDefinition::Predicate(predicate),
                SchemaDefinition::EventKind(event_kind.clone()),
            ],
        )?;
        let schema = take!(history.schema_at(SchemaMode::Current, revision));
        let mut timelines = BTreeMap::new();
        timelines.insert(timeline_id, Lifecycle::Active);
        let mut time_units = BTreeMap::new();
        time_units.insert(take!(Symbol::new("tick")), Lifecycle::Active);

        Ok(Fixture {
            history_spaces,
            layers,
            entities,
            perspectives,
            schema,
            timelines,
            time_units,
            history_space_id,
            perspective_id,
            layer_id,
            retired_layer_id,
            entity_id,
            predicate_id,
            timeline_id,
            event_kind,
            event_role_id,
        })
    }

    impl Fixture {
        fn snapshot(&self) -> WriteReferenceSnapshot<'_> {
            self.snapshot_with_schema(&self.schema)
        }

        fn snapshot_with_schema<'a>(
            &'a self,
            schema: &'a crate::schema_history::SchemaSnapshot,
        ) -> WriteReferenceSnapshot<'a> {
            WriteReferenceSnapshot {
                history_spaces: &self.history_spaces,
                layers: &self.layers,
                entities: &self.entities,
                perspectives: &self.perspectives,
                schema,
                timelines: &self.timelines,
                time_units: &self.time_units,
            }
        }

        fn assertion(&self, layer_id: LayerId) -> Result<AssertionDraft, TestError> {
            self.assertion_with_value(layer_id, Value::Bool(true))
        }

        fn assertion_with_value(
            &self,
            layer_id: LayerId,
            value: Value,
        ) -> Result<AssertionDraft, TestError> {
            self.assertion_in_scope(layer_id, PerspectiveScope::World, value)
        }

        fn assertion_in_scope(
            &self,
            layer_id: LayerId,
            perspective_scope: PerspectiveScope,
            value: Value,
        ) -> Result<AssertionDraft, TestError> {
            let epistemic_mode = match perspective_scope {
                PerspectiveScope::World => EpistemicMode::WorldState,
                PerspectiveScope::Perspective(_) => EpistemicMode::Knows,
            };
            let context = take!(ContextKey::new(
                self.history_space_id,
                layer_id,
                perspective_scope,
                epistemic_mode,
            ));
            let timeline = Timeline::new(self.timeline_id);
            let start = WorldTime::from_nanoseconds(timeline, 0);
            let end = WorldTime::from_nanoseconds(timeline, 1);
            let interval = take!(TimeInterval::new(timeline, Some(start), Some(end)));
            Ok(AssertionDraft::new(
                context,
                Subject::new(self.entity_id),
                self.predicate_id,
                value,
                Polarity::Positive,
                AssertionValidity::new(interval),
            ))
        }

        fn event(&self) -> Result<EventDraft, TestError> {
            let time = EventTime::Instant(WorldTime::from_nanoseconds(
                Timeline::new(self.timeline_id),
                10,
            ));
            EventDraft::new(
                self.history_space_id,
                self.layer_id,
                &self.event_kind,
                vec![EventParticipant::new(self.event_role_id, self.entity_id)],
                vec![],
                time,
            )
            .map_err(TestError::Event)
        }
    }

    #[test]
    fn complete_assertion_references_produce_a_validated_batch() -> Result<(), TestError> {
        let fixture = fixture()?;
        let assertion = fixture.assertion(fixture.layer_id)?;
        let validated = take!(validate_write_references(
            vec![assertion],
            vec![],
            &fixture.snapshot()
        ));
        assert_eq!(validated.assertions().len(), 1);
        assert!(validated.events().is_empty());
        Ok(())
    }

    #[test]
    fn event_references_check_event_kind_role_entity_and_time() -> Result<(), TestError> {
        let fixture = fixture()?;
        let event = fixture.event()?;
        let validated = take!(validate_write_references(
            vec![],
            vec![event],
            &fixture.snapshot()
        ));
        assert!(validated.assertions().is_empty());
        assert_eq!(validated.events().len(), 1);
        Ok(())
    }

    #[test]
    fn event_reference_rejects_role_outside_selected_post_transaction_schema()
    -> Result<(), TestError> {
        let fixture = fixture()?;
        let alternate_role_id = take!(id::<EventRoleId>(11));
        let selected_role = fixture
            .event_kind
            .roles()
            .first()
            .ok_or_else(|| TestError::Message("fixture EventKind has no role".to_owned()))?;
        let stale_event_kind = take!(EventKindDefinition::new(
            fixture.event_kind.event_kind_id(),
            fixture.event_kind.symbol().clone(),
            vec![EventRoleDefinition::new(
                alternate_role_id,
                take!(Symbol::new("alternate_visitor")),
                selected_role.entity_constraint(),
                selected_role.cardinality(),
            )],
            fixture.event_kind.attributes().to_vec(),
            fixture.event_kind.event_time_constraint(),
            fixture.event_kind.lifecycle(),
            fixture.event_kind.created_revision(),
        ));
        let event = take!(EventDraft::new(
            fixture.history_space_id,
            fixture.layer_id,
            &stale_event_kind,
            vec![EventParticipant::new(alternate_role_id, fixture.entity_id)],
            vec![],
            EventTime::Instant(WorldTime::from_nanoseconds(
                Timeline::new(fixture.timeline_id),
                10,
            )),
        ));

        assert!(matches!(
            validate_write_references(vec![], vec![event], &fixture.snapshot()),
            Err(WriteReferenceValidationError::UnknownEventRole {
                event_kind_id,
                role_id,
            }) if event_kind_id == fixture.event_kind.event_kind_id()
                && role_id == alternate_role_id
        ));
        Ok(())
    }

    #[test]
    fn assertion_perspective_must_resolve_in_the_selected_catalog() -> Result<(), TestError> {
        let fixture = fixture()?;
        let assertion = fixture.assertion_in_scope(
            fixture.layer_id,
            PerspectiveScope::Perspective(fixture.perspective_id),
            Value::Bool(true),
        )?;
        let validated = take!(validate_write_references(
            vec![assertion],
            vec![],
            &fixture.snapshot()
        ));
        assert_eq!(validated.assertions().len(), 1);
        Ok(())
    }

    #[test]
    fn catalogs_from_different_revisions_are_rejected() -> Result<(), TestError> {
        let mut fixture = fixture()?;
        fixture.perspectives = take!(PerspectiveCatalogSnapshot::new(
            Revision::GENESIS,
            vec![],
            vec![]
        ));
        let assertion = fixture.assertion(fixture.layer_id)?;
        assert!(matches!(
            validate_write_references(vec![assertion], vec![], &fixture.snapshot()),
            Err(WriteReferenceValidationError::CatalogRevisionMismatch {
                catalog: ReferenceCatalog::Perspectives,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn write_authorization_requires_operation_layer_entity_and_field_grants()
    -> Result<(), TestError> {
        let fixture = fixture()?;
        let assertion = fixture.assertion(fixture.layer_id)?;
        let event = fixture.event()?;
        let batch = take!(validate_write_references(
            vec![assertion],
            vec![event],
            &fixture.snapshot()
        ));
        let principal = take!(id::<PrincipalId>(11));
        let denied = take!(SecurityPolicySnapshot::new(vec![], vec![], vec![], vec![]));
        assert!(matches!(
            authorize_validated_write_batch(&batch, &denied, principal),
            Err(WriteAuthorizationError::Denied {
                capability: Capability::AssertionCreate,
                ..
            })
        ));

        let capabilities = [
            (20, Capability::AssertionCreate),
            (21, Capability::EventCreate),
            (22, Capability::LayerWrite),
            (23, Capability::EntityReference),
            (24, Capability::FieldWrite),
        ];
        let mut rules = Vec::new();
        for (tail, capability) in capabilities {
            rules.push(CapabilityRule::new(
                take!(id::<PolicyRuleId>(tail)),
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, crate::GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        let allowed = take!(SecurityPolicySnapshot::new(
            vec![crate::Principal::new(principal)],
            vec![],
            vec![],
            rules,
        ));
        assert_eq!(
            authorize_validated_write_batch(&batch, &allowed, principal),
            Ok(())
        );

        let mut rules_without_field_write = Vec::new();
        for (tail, capability) in &capabilities[..4] {
            rules_without_field_write.push(CapabilityRule::new(
                take!(id::<PolicyRuleId>(*tail + 10)),
                PolicySubject::Principal(principal),
                CapabilityGrant::new(*capability, crate::GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        let missing_field_grant = take!(SecurityPolicySnapshot::new(
            vec![crate::Principal::new(principal)],
            vec![],
            vec![],
            rules_without_field_write,
        ));
        assert!(matches!(
            authorize_validated_write_batch(&batch, &missing_field_grant, principal),
            Err(WriteAuthorizationError::Denied {
                capability: Capability::FieldWrite,
                target,
            }) if target.field() == Some(crate::FieldSelector::AssertionSubject)
        ));
        Ok(())
    }

    #[test]
    fn deprecated_schema_warning_is_rechecked_against_current_policy() -> Result<(), TestError> {
        let fixture = fixture_with_predicate_lifecycle(Lifecycle::Deprecated)?;
        let assertion = fixture.assertion(fixture.layer_id)?;
        let principal = take!(id::<PrincipalId>(50));
        let field = crate::FieldSelector::AssertionValue(fixture.predicate_id);
        let mut initial_rules = Vec::new();
        for (tail, capability) in [
            (51, Capability::LayerWrite),
            (52, Capability::EntityReference),
            (53, Capability::FieldWrite),
        ] {
            initial_rules.push(CapabilityRule::new(
                take!(id::<PolicyRuleId>(tail)),
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, crate::GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        initial_rules.push(CapabilityRule::new(
            take!(id::<PolicyRuleId>(54)),
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::AssertionCreate, crate::GrantEffect::Allow),
            PolicyScope::new(
                Some(fixture.history_space_id),
                Some(fixture.layer_id),
                None,
                Some(field),
                None,
            ),
        ));
        let initial_policy = take!(SecurityPolicySnapshot::new(
            vec![crate::Principal::new(principal)],
            vec![],
            vec![],
            initial_rules,
        ));
        let schema_assertions = take!(crate::validate_assertion_batch(
            vec![assertion.clone()],
            &fixture.schema,
            &initial_policy,
            principal,
            true,
            crate::DecoderLimits::default(),
            |_| {
                Ok(crate::WorldTime::from_nanoseconds(
                    crate::Timeline::new(fixture.timeline_id),
                    0,
                ))
            },
        ));
        assert_eq!(schema_assertions.warnings().len(), 1);

        let references = take!(validate_write_references(
            vec![assertion],
            vec![],
            &fixture.snapshot(),
        ));
        assert_eq!(
            crate::authorize_deprecated_schema_warnings(
                &references,
                &schema_assertions,
                &initial_policy,
                principal,
            ),
            Ok(())
        );

        let mut revoked_rules = Vec::new();
        for (tail, capability) in [
            (61, Capability::LayerWrite),
            (62, Capability::EntityReference),
            (63, Capability::FieldWrite),
        ] {
            revoked_rules.push(CapabilityRule::new(
                take!(id::<PolicyRuleId>(tail)),
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, crate::GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        let current_policy = take!(SecurityPolicySnapshot::new(
            vec![crate::Principal::new(principal)],
            vec![],
            vec![],
            revoked_rules,
        ));
        assert!(matches!(
            crate::authorize_deprecated_schema_warnings(
                &references,
                &schema_assertions,
                &current_policy,
                principal,
            ),
            Err(crate::WriteAuthorizationError::Denied {
                capability: Capability::AssertionCreate,
                target,
            }) if target.field() == Some(field)
        ));
        Ok(())
    }

    #[test]
    fn retired_layer_and_unknown_timeline_fail_before_a_batch_is_returned() -> Result<(), TestError>
    {
        let mut fixture = fixture()?;
        let assertion = fixture.assertion(fixture.retired_layer_id)?;
        assert!(matches!(
            validate_write_references(vec![assertion], vec![], &fixture.snapshot()),
            Err(WriteReferenceValidationError::RetiredLayer(_))
        ));

        fixture.timelines.clear();
        let assertion = fixture.assertion(fixture.layer_id)?;
        assert!(matches!(
            validate_write_references(vec![assertion], vec![], &fixture.snapshot()),
            Err(WriteReferenceValidationError::UnknownTimeline(_))
        ));
        Ok(())
    }

    #[test]
    fn complete_cross_record_boundary_accepts_an_empty_atomic_batch() -> Result<(), TestError> {
        let fixture = fixture()?;
        let references = take!(validate_write_references(
            vec![],
            vec![],
            &fixture.snapshot(),
        ));
        let principal = take!(id::<PrincipalId>(40));
        let policy = SecurityPolicySnapshot::default();
        let schema_assertions = take!(crate::validate_assertion_batch(
            vec![],
            &fixture.schema,
            &policy,
            principal,
            false,
            crate::DecoderLimits::default(),
            |_| {
                Ok(crate::WorldTime::from_nanoseconds(
                    crate::Timeline::new(fixture.timeline_id),
                    0,
                ))
            },
        ));
        let event_relations = take!(crate::EventRelationBatch::new(vec![]));
        let provenance_endpoints = BTreeSet::new();

        let validated = take!(crate::validate_write_cross_record_state(
            &references,
            &policy,
            principal,
            crate::WriteCrossRecordCandidate {
                schema_assertions: &schema_assertions,
                event_ids: &[],
                event_relation_history: crate::EventRelationHistory::new(&[], &[]),
                recorded_as_of: crate::RecordedAsOf::from_published_revision(Revision::GENESIS),
                commit_revision: Revision::FIRST_COMMIT,
                event_relation_retractions: &[],
                event_relation_additions: &event_relations,
                provenance_history: crate::ProvenanceEdgeHistory::new(&[], &[]),
                provenance_retractions: &[],
                provenance_additions: &[],
                provenance_endpoints_after: &provenance_endpoints,
                graph_budget: crate::GraphValidationBudget::new(100),
                source_evidence_history: crate::SourceEvidenceHistory::new(&[], &[], &[], &[]),
                history_spaces: &fixture.history_spaces,
                evidence_history_space: fixture.history_space_id,
            },
        ));

        assert!(validated.references().assertions().is_empty());
        assert!(validated.references().events().is_empty());
        assert!(validated.event_graph().active_relations().is_empty());
        assert!(validated.provenance_graph().active_edges().is_empty());
        assert!(validated.source_evidence().evidence().is_empty());
        Ok(())
    }

    #[test]
    fn mixed_schema_domain_event_evidence_and_provenance_publish_once() -> Result<(), TestError> {
        let fixture = fixture()?;
        let commit_revision = Revision::FIRST_COMMIT;
        let assertion_id = take!(id::<crate::AssertionId>(70));
        let event_id = take!(id::<crate::EventId>(71));
        let source_id = take!(id::<crate::SourceId>(72));
        let evidence_id = take!(id::<crate::EvidenceId>(73));
        let provenance_id = take!(id::<crate::ProvenanceId>(74));
        let principal = take!(id::<PrincipalId>(75));
        let assertion_draft = fixture.assertion(fixture.layer_id)?;
        let event_draft = fixture.event()?;
        let references = take!(validate_write_references(
            vec![assertion_draft.clone()],
            vec![event_draft.clone()],
            &fixture.snapshot(),
        ));

        let mut rules = Vec::new();
        for (tail, capability) in [
            (76, Capability::AssertionCreate),
            (77, Capability::EventCreate),
            (78, Capability::LayerWrite),
            (79, Capability::EntityReference),
            (80, Capability::FieldWrite),
        ] {
            rules.push(CapabilityRule::new(
                take!(id::<PolicyRuleId>(tail)),
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, crate::GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        let policy = take!(SecurityPolicySnapshot::new(
            vec![crate::Principal::new(principal)],
            vec![],
            vec![],
            rules,
        ));
        let assertion_draft_for_record = references
            .assertions()
            .first()
            .cloned()
            .ok_or_else(|| TestError::Message("validated assertion was missing".to_owned()))?;
        let assertion =
            crate::Assertion::new(assertion_id, assertion_draft_for_record, commit_revision);
        let event = take!(crate::Event::new(
            event_id,
            event_draft.clone(),
            commit_revision
        ));
        let source = crate::Source::new(
            source_id,
            take!(Symbol::new("web")),
            None,
            None,
            crate::SourceMetadata::default(),
            commit_revision,
        );
        let evidence = crate::Evidence::new(
            evidence_id,
            source_id,
            crate::EvidenceTargetRef::Assertion(assertion_id),
            crate::EvidenceRelation::Documents,
            commit_revision,
        );
        let provenance = crate::ProvenanceEdge::new(
            provenance_id,
            crate::ProvenanceEndpointRef::Assertion(assertion_id),
            crate::ProvenanceEndpointRef::Event(event_id),
            crate::ProvenanceRelation::ResultedFrom,
            commit_revision,
        )
        .map_err(|error| TestError::Message(error.to_string()))?;
        let evidence_targets = [crate::EvidenceTargetHistoryEntry::new(
            crate::EvidenceTargetRef::Assertion(assertion_id),
            commit_revision,
            Some(fixture.history_space_id),
        )];
        let sources = [source.clone()];
        let evidence_records = [evidence];
        let provenance_edges = [provenance];
        let event_relations = take!(crate::EventRelationBatch::new(vec![]));
        let event_ids = [event_id];
        let provenance_endpoints = BTreeSet::from([
            crate::ProvenanceEndpointRef::Assertion(assertion_id),
            crate::ProvenanceEndpointRef::Event(event_id),
        ]);

        let mut records = Vec::new();
        for definition in fixture.schema.definitions() {
            records.push(match definition {
                SchemaDefinition::Layer(value) => crate::Record::LayerDefinition(value.clone()),
                SchemaDefinition::LayerSnapshot(value) => {
                    crate::Record::LayerSchemaSnapshot(value.clone())
                }
                SchemaDefinition::EntityType(value) => {
                    crate::Record::EntityTypeDefinition(value.clone())
                }
                SchemaDefinition::Predicate(value) => {
                    crate::Record::PredicateDefinition(value.clone())
                }
                SchemaDefinition::EventKind(value) => {
                    crate::Record::EventKindDefinition(value.clone())
                }
            });
        }
        records.extend([
            crate::Record::Assertion(assertion),
            crate::Record::Event(event),
            crate::Record::Source(source),
            crate::Record::Evidence(evidence),
            crate::Record::Provenance(provenance),
        ]);
        let rejected_records = records.clone();

        let mut backend = crate::InMemoryRevisionBackend::<crate::Record>::new();
        let mut transaction = crate::OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| TestError::Message(error.to_string()))?;
        for record in records {
            transaction.stage(record);
        }
        let committed = crate::commit_mixed_record_batch(transaction, |base, entries| {
            if base != Revision::GENESIS || entries.len() != 8 {
                return Err("mixed candidate was not complete".to_owned());
            }
            let mut family_counts = [0_usize; 6];
            for record in entries {
                match record {
                    crate::Record::EntityTypeDefinition(_)
                    | crate::Record::PredicateDefinition(_)
                    | crate::Record::EventKindDefinition(_) => family_counts[0] += 1,
                    crate::Record::Assertion(_) => family_counts[1] += 1,
                    crate::Record::Event(_) => family_counts[2] += 1,
                    crate::Record::Source(_) => family_counts[3] += 1,
                    crate::Record::Evidence(_) => family_counts[4] += 1,
                    crate::Record::Provenance(_) => family_counts[5] += 1,
                    _ => {}
                }
            }
            if family_counts != [3, 1, 1, 1, 1, 1] {
                return Err("mixed batch did not contain each required record family".to_owned());
            }
            let staged_schema_definitions = entries
                .iter()
                .filter_map(|record| match record {
                    crate::Record::LayerDefinition(value) => {
                        Some(SchemaDefinition::Layer(value.clone()))
                    }
                    crate::Record::LayerSchemaSnapshot(value) => {
                        Some(SchemaDefinition::LayerSnapshot(value.clone()))
                    }
                    crate::Record::EntityTypeDefinition(value) => {
                        Some(SchemaDefinition::EntityType(value.clone()))
                    }
                    crate::Record::PredicateDefinition(value) => {
                        Some(SchemaDefinition::Predicate(value.clone()))
                    }
                    crate::Record::EventKindDefinition(value) => {
                        Some(SchemaDefinition::EventKind(value.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let mut schema_history = SchemaHistoryReferenceModel::new();
            schema_history
                .publish(commit_revision, staged_schema_definitions)
                .map_err(|error| error.to_string())?;
            let post_transaction_schema = schema_history
                .schema_at(SchemaMode::Current, commit_revision)
                .map_err(|error| error.to_string())?;
            if post_transaction_schema.fingerprint() != fixture.schema.fingerprint() {
                return Err("staged schema records differ from the selected post-state".to_owned());
            }
            let post_transaction_snapshot = fixture.snapshot_with_schema(&post_transaction_schema);
            let staged_assertion = references
                .assertions()
                .first()
                .cloned()
                .ok_or_else(|| "validated assertion was missing".to_owned())?;
            let staged_event = references
                .events()
                .first()
                .cloned()
                .ok_or_else(|| "validated event was missing".to_owned())?;
            let post_schema_assertions = crate::validate_assertion_batch(
                vec![staged_assertion.clone()],
                &post_transaction_schema,
                &policy,
                principal,
                false,
                crate::DecoderLimits::default(),
                |_| {
                    Ok(crate::WorldTime::from_nanoseconds(
                        crate::Timeline::new(fixture.timeline_id),
                        0,
                    ))
                },
            )
            .map_err(|error| error.to_string())?;
            let post_snapshot_references = validate_write_references(
                vec![staged_assertion],
                vec![staged_event],
                &post_transaction_snapshot,
            )
            .map_err(|error| error.to_string())?;
            let cross_record = crate::validate_write_cross_record_state(
                &post_snapshot_references,
                &policy,
                principal,
                crate::WriteCrossRecordCandidate {
                    schema_assertions: &post_schema_assertions,
                    event_ids: &event_ids,
                    event_relation_history: crate::EventRelationHistory::new(&[], &[]),
                    recorded_as_of: crate::RecordedAsOf::from_published_revision(Revision::GENESIS),
                    commit_revision,
                    event_relation_retractions: &[],
                    event_relation_additions: &event_relations,
                    provenance_history: crate::ProvenanceEdgeHistory::new(&[], &[]),
                    provenance_retractions: &[],
                    provenance_additions: &provenance_edges,
                    provenance_endpoints_after: &provenance_endpoints,
                    graph_budget: crate::GraphValidationBudget::new(100),
                    source_evidence_history: crate::SourceEvidenceHistory::new(
                        &sources,
                        &evidence_records,
                        &[],
                        &evidence_targets,
                    ),
                    history_spaces: &fixture.history_spaces,
                    evidence_history_space: fixture.history_space_id,
                },
            )
            .map_err(|error| error.to_string())?;
            if cross_record.source_evidence().evidence().len() != 1
                || cross_record.provenance_graph().active_edges().len() != 1
            {
                return Err("post-transaction relationship projection was incomplete".to_owned());
            }
            Ok(())
        });
        let revision = committed.map_err(|error| TestError::Message(error.to_string()))?;

        assert_eq!(revision, commit_revision);
        let committed_records = backend
            .read_at(revision)
            .map_err(|error| TestError::Message(error.to_string()))?
            .collect::<Vec<_>>();
        assert_eq!(committed_records.len(), 8);
        assert!(
            committed_records
                .iter()
                .all(|(record_revision, _)| *record_revision == revision)
        );

        let invalid_context = take!(ContextKey::new(
            fixture.history_space_id,
            fixture.layer_id,
            crate::PerspectiveScope::World,
            crate::EpistemicMode::WorldState,
        ));
        let invalid_start = WorldTime::from_nanoseconds(Timeline::new(fixture.timeline_id), 0);
        let invalid_end = WorldTime::from_nanoseconds(Timeline::new(fixture.timeline_id), 1);
        let invalid_validity = take!(TimeInterval::new(
            Timeline::new(fixture.timeline_id),
            Some(invalid_start),
            Some(invalid_end),
        ));
        let invalid_draft = AssertionDraft::new(
            invalid_context,
            Subject::new(fixture.entity_id),
            take!(id::<PredicateId>(90)),
            Value::Bool(true),
            Polarity::Positive,
            AssertionValidity::new(invalid_validity),
        );
        let mut invalid_records = rejected_records.clone();
        let invalid_assertion = crate::Record::Assertion(crate::Assertion::new(
            assertion_id,
            invalid_draft.clone(),
            commit_revision,
        ));
        let assertion_position = invalid_records
            .iter()
            .position(|record| matches!(record, crate::Record::Assertion(_)))
            .ok_or_else(|| TestError::Message("mixed batch had no Assertion".to_owned()))?;
        let assertion_slot = invalid_records.get_mut(assertion_position).ok_or_else(|| {
            TestError::Message("mixed batch Assertion slot disappeared".to_owned())
        })?;
        *assertion_slot = invalid_assertion;

        let mut invalid_backend = crate::InMemoryRevisionBackend::<crate::Record>::new();
        let mut invalid_transaction =
            crate::OpenTransaction::begin(&mut invalid_backend, Revision::GENESIS)
                .map_err(|error| TestError::Message(error.to_string()))?;
        for record in invalid_records {
            invalid_transaction.stage(record);
        }
        let invalid_candidate =
            crate::commit_mixed_record_batch(invalid_transaction, |_, entries| {
                let staged_schema_definitions = entries
                    .iter()
                    .filter_map(|record| match record {
                        crate::Record::LayerDefinition(value) => {
                            Some(SchemaDefinition::Layer(value.clone()))
                        }
                        crate::Record::LayerSchemaSnapshot(value) => {
                            Some(SchemaDefinition::LayerSnapshot(value.clone()))
                        }
                        crate::Record::EntityTypeDefinition(value) => {
                            Some(SchemaDefinition::EntityType(value.clone()))
                        }
                        crate::Record::PredicateDefinition(value) => {
                            Some(SchemaDefinition::Predicate(value.clone()))
                        }
                        crate::Record::EventKindDefinition(value) => {
                            Some(SchemaDefinition::EventKind(value.clone()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let mut schema_history = SchemaHistoryReferenceModel::new();
                schema_history
                    .publish(commit_revision, staged_schema_definitions)
                    .map_err(|error| error.to_string())?;
                let post_transaction_schema = schema_history
                    .schema_at(SchemaMode::Current, commit_revision)
                    .map_err(|error| error.to_string())?;
                let post_transaction_snapshot =
                    fixture.snapshot_with_schema(&post_transaction_schema);
                validate_write_references(
                    vec![invalid_draft.clone()],
                    vec![event_draft.clone()],
                    &post_transaction_snapshot,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            });
        assert!(invalid_candidate.is_err());
        assert_eq!(invalid_backend.latest_published(), Revision::GENESIS);
        assert!(invalid_backend.commits().is_empty());

        let denied = SecurityPolicySnapshot::default();
        let mut rejected_backend = crate::InMemoryRevisionBackend::<crate::Record>::new();
        let mut rejected_transaction =
            crate::OpenTransaction::begin(&mut rejected_backend, Revision::GENESIS)
                .map_err(|error| TestError::Message(error.to_string()))?;
        for record in rejected_records {
            rejected_transaction.stage(record);
        }
        let rejected = crate::commit_mixed_record_batch(rejected_transaction, |_, _| {
            authorize_validated_write_batch(&references, &denied, principal)
                .map_err(|error| error.to_string())
        });
        assert!(rejected.is_err());
        assert_eq!(rejected_backend.latest_published(), Revision::GENESIS);
        assert!(rejected_backend.commits().is_empty());
        Ok(())
    }

    #[test]
    fn time_values_require_active_registered_timeline_and_unit() -> Result<(), TestError> {
        let mut fixture = fixture()?;
        fixture.time_units.clear();
        let assertion = fixture.assertion_with_value(
            fixture.layer_id,
            Value::Time(crate::Time::new(
                fixture.timeline_id,
                5,
                take!(Symbol::new("tick")),
            )),
        )?;
        assert!(matches!(
            validate_write_references(vec![assertion], vec![], &fixture.snapshot()),
            Err(WriteReferenceValidationError::UnknownTimeUnit(_))
        ));

        fixture
            .time_units
            .insert(take!(Symbol::new("tick")), Lifecycle::Deprecated);
        let assertion = fixture.assertion_with_value(
            fixture.layer_id,
            Value::Time(crate::Time::new(
                fixture.timeline_id,
                5,
                take!(Symbol::new("tick")),
            )),
        )?;
        assert!(matches!(
            validate_write_references(vec![assertion], vec![], &fixture.snapshot()),
            Err(WriteReferenceValidationError::DeprecatedTimeUnit(_))
        ));
        Ok(())
    }
}
