//! Schema-checked immutable Events, EventMasks, and their lifecycle records.

use std::cmp::Ordering;
use std::fmt;

use crate::ids::{
    EntityId, EventAttributeId, EventId, EventKindId, EventMaskId, EventMaskRetractionId,
    EventRetractionId, EventRoleId, EventSpanClosureId, HistorySpaceId, LayerId, Revision,
};
use crate::schema::{
    EventAttributeDefinition, EventKindDefinition, EventTimeForm, RoleCardinality, ValueKind,
};
use crate::temporal::{EventTime, TemporalError, WorldTime};
use crate::values::Value;

/// One entity assigned to a role in an Event.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventParticipant {
    role_id: EventRoleId,
    entity_id: EntityId,
}

impl EventParticipant {
    /// Creates a typed role/entity pair; EventKind validation occurs in [`Participants::new`].
    #[must_use]
    pub const fn new(role_id: EventRoleId, entity_id: EntityId) -> Self {
        Self { role_id, entity_id }
    }

    /// Returns the schema-defined role identity.
    #[must_use]
    pub const fn role_id(self) -> EventRoleId {
        self.role_id
    }

    /// Returns the referenced entity identity.
    #[must_use]
    pub const fn entity_id(self) -> EntityId {
        self.entity_id
    }
}

/// Canonically ordered Event participant pairs validated against one EventKind.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Participants(Vec<EventParticipant>);

impl Participants {
    /// Validates allowed roles, uniqueness, required roles, and cardinalities.
    pub fn new(
        mut participants: Vec<EventParticipant>,
        event_kind: &EventKindDefinition,
    ) -> Result<Self, EventRecordError> {
        participants.sort();
        if let Some(duplicate) = participants
            .windows(2)
            .find(|pair| matches!(pair, [left, right] if left == right))
            .and_then(|pair| pair.first())
        {
            return Err(EventRecordError::DuplicateParticipant {
                role_id: duplicate.role_id(),
                entity_id: duplicate.entity_id(),
            });
        }

        for participant in &participants {
            if !event_kind
                .roles()
                .iter()
                .any(|role| role.event_role_id() == participant.role_id())
            {
                return Err(EventRecordError::UnknownParticipantRole {
                    role_id: participant.role_id(),
                });
            }
        }

        for role in event_kind.roles() {
            let actual = participants
                .iter()
                .filter(|participant| participant.role_id() == role.event_role_id())
                .count();
            let actual =
                u32::try_from(actual).map_err(|_| EventRecordError::ParticipantCountOverflow)?;
            validate_role_cardinality(role.event_role_id(), role.cardinality(), actual)?;
        }

        Ok(Self(participants))
    }

    /// Returns the canonical `(role_id, entity_id)` ordering.
    #[must_use]
    pub fn as_slice(&self) -> &[EventParticipant] {
        &self.0
    }

    /// Reconstructs a canonical participant list after the wire decoder has
    /// checked its ordering; EventKind membership is checked during import.
    pub(crate) fn from_wire_fields(participants: Vec<EventParticipant>) -> Self {
        Self(participants)
    }
}

/// One typed value supplied for an Event attribute.
#[derive(Clone, Debug)]
pub struct EventAttributeValue {
    attribute_id: EventAttributeId,
    value: Value,
}

impl EventAttributeValue {
    /// Creates a typed attribute payload; the EventKind validates its identity and value kind.
    #[must_use]
    pub const fn new(attribute_id: EventAttributeId, value: Value) -> Self {
        Self {
            attribute_id,
            value,
        }
    }

    /// Returns the stable schema attribute identity.
    #[must_use]
    pub const fn attribute_id(&self) -> EventAttributeId {
        self.attribute_id
    }

    /// Returns the typed core value.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }
}

/// Canonically ordered Event attributes validated against one EventKind.
#[derive(Clone, Debug)]
pub struct EventAttributes(Vec<EventAttributeValue>);

impl EventAttributes {
    /// Validates declared attributes, required attributes, uniqueness, and exact ValueKind.
    pub fn new(
        mut attributes: Vec<EventAttributeValue>,
        event_kind: &EventKindDefinition,
    ) -> Result<Self, EventRecordError> {
        attributes.sort_by_key(EventAttributeValue::attribute_id);
        for pair in attributes.windows(2) {
            if let [left, right] = pair {
                if left.attribute_id() == right.attribute_id() {
                    return Err(EventRecordError::DuplicateAttribute {
                        attribute_id: left.attribute_id(),
                    });
                }
            }
        }

        for attribute in &attributes {
            let Some(definition) = event_kind
                .attributes()
                .iter()
                .find(|definition| definition.event_attribute_id() == attribute.attribute_id())
            else {
                return Err(EventRecordError::UnknownAttribute {
                    attribute_id: attribute.attribute_id(),
                });
            };
            validate_attribute_kind(definition, attribute)?;
        }

        for definition in event_kind.attributes() {
            if definition.required()
                && !attributes
                    .iter()
                    .any(|attribute| attribute.attribute_id() == definition.event_attribute_id())
            {
                return Err(EventRecordError::MissingRequiredAttribute {
                    attribute_id: definition.event_attribute_id(),
                });
            }
        }

        Ok(Self(attributes))
    }

    /// Returns attributes in stable `EventAttributeId` order.
    #[must_use]
    pub fn as_slice(&self) -> &[EventAttributeValue] {
        &self.0
    }

    /// Reconstructs a canonical attribute list after the wire decoder has
    /// checked its ordering; EventKind membership and value kinds are checked
    /// during import.
    pub(crate) fn from_wire_fields(attributes: Vec<EventAttributeValue>) -> Self {
        Self(attributes)
    }
}

/// Complete typed Event input before the engine assigns record identity and revision.
#[derive(Clone, Debug)]
pub struct EventDraft {
    history_space_id: HistorySpaceId,
    layer_id: LayerId,
    event_kind_id: EventKindId,
    event_kind_revision: Revision,
    participants: Participants,
    attributes: EventAttributes,
    event_time: EventTime,
}

impl EventDraft {
    /// Validates complete Event content against one concrete EventKind definition.
    pub fn new(
        history_space_id: HistorySpaceId,
        layer_id: LayerId,
        event_kind: &EventKindDefinition,
        participants: Vec<EventParticipant>,
        attributes: Vec<EventAttributeValue>,
        event_time: EventTime,
    ) -> Result<Self, EventRecordError> {
        let participants = Participants::new(participants, event_kind)?;
        let attributes = EventAttributes::new(attributes, event_kind)?;
        validate_event_time(event_time, event_kind.event_time_constraint().form())?;
        Ok(Self {
            history_space_id,
            layer_id,
            event_kind_id: event_kind.event_kind_id(),
            event_kind_revision: event_kind.created_revision(),
            participants,
            attributes,
            event_time,
        })
    }

    /// Returns the HistorySpace that will own the Event.
    #[must_use]
    pub const fn history_space_id(&self) -> HistorySpaceId {
        self.history_space_id
    }

    /// Returns the explicit Layer that will own the Event.
    #[must_use]
    pub const fn layer_id(&self) -> LayerId {
        self.layer_id
    }

    /// Returns the schema EventKind identity used to validate this draft.
    #[must_use]
    pub const fn event_kind_id(&self) -> EventKindId {
        self.event_kind_id
    }

    /// Returns the EventKind schema revision used to validate this draft.
    #[must_use]
    pub const fn event_kind_revision(&self) -> Revision {
        self.event_kind_revision
    }

    /// Returns the canonical, already shape-validated participants.
    #[must_use]
    pub const fn participants(&self) -> &Participants {
        &self.participants
    }

    /// Returns the canonical, already shape-validated attributes.
    #[must_use]
    pub const fn attributes(&self) -> &EventAttributes {
        &self.attributes
    }

    /// Returns the validated event instant or span.
    #[must_use]
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }
}

/// One immutable event history record.
#[derive(Clone, Debug)]
pub struct Event {
    id: EventId,
    history_space_id: HistorySpaceId,
    layer_id: LayerId,
    event_kind_id: EventKindId,
    participants: Participants,
    attributes: EventAttributes,
    event_time: EventTime,
    created_revision: Revision,
}

/// Locally decoded Event fields before the creation revision is attached.
pub(crate) struct EventWireFields {
    history_space_id: HistorySpaceId,
    layer_id: LayerId,
    event_kind_id: EventKindId,
    participants: Participants,
    attributes: EventAttributes,
    event_time: EventTime,
}

impl EventWireFields {
    /// Groups the decoded Event fields for the wire reconstruction boundary.
    pub(crate) fn new(
        history_space_id: HistorySpaceId,
        layer_id: LayerId,
        event_kind_id: EventKindId,
        participants: Participants,
        attributes: EventAttributes,
        event_time: EventTime,
    ) -> Self {
        Self {
            history_space_id,
            layer_id,
            event_kind_id,
            participants,
            attributes,
            event_time,
        }
    }
}

impl Event {
    /// Creates an immutable Event at or after the schema revision used by its draft.
    pub fn new(
        id: EventId,
        draft: EventDraft,
        created_revision: Revision,
    ) -> Result<Self, EventRecordError> {
        if draft.event_kind_revision > created_revision {
            return Err(EventRecordError::EventKindNotYetPublished {
                event_kind_revision: draft.event_kind_revision,
                event_revision: created_revision,
            });
        }
        Ok(Self {
            id,
            history_space_id: draft.history_space_id,
            layer_id: draft.layer_id,
            event_kind_id: draft.event_kind_id,
            participants: draft.participants,
            attributes: draft.attributes,
            event_time: draft.event_time,
            created_revision,
        })
    }

    /// Reconstructs persisted fields after local wire-shape validation.
    /// EventKind, entity, schema revision, and history checks happen on import.
    pub(crate) fn from_wire_fields(
        id: EventId,
        fields: EventWireFields,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            history_space_id: fields.history_space_id,
            layer_id: fields.layer_id,
            event_kind_id: fields.event_kind_id,
            participants: fields.participants,
            attributes: fields.attributes,
            event_time: fields.event_time,
            created_revision,
        }
    }

    /// Returns the stable event identity.
    #[must_use]
    pub const fn id(&self) -> EventId {
        self.id
    }

    /// Returns the HistorySpace that owns this event.
    #[must_use]
    pub const fn history_space_id(&self) -> HistorySpaceId {
        self.history_space_id
    }

    /// Returns the explicit Layer identity.
    #[must_use]
    pub const fn layer_id(&self) -> LayerId {
        self.layer_id
    }

    /// Returns the schema EventKind identity.
    #[must_use]
    pub const fn event_kind_id(&self) -> EventKindId {
        self.event_kind_id
    }

    /// Returns canonical role/entity participant pairs.
    #[must_use]
    pub const fn participants(&self) -> &Participants {
        &self.participants
    }

    /// Returns canonical, typed attributes.
    #[must_use]
    pub const fn attributes(&self) -> &EventAttributes {
        &self.attributes
    }

    /// Returns the validated instant or span.
    #[must_use]
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }

    /// Returns the Transaction-Time revision that created this event.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// One immutable direct mask against a concrete Event.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EventMask {
    id: EventMaskId,
    history_space_id: HistorySpaceId,
    layer_id: LayerId,
    target_event: EventId,
    created_revision: Revision,
}

impl EventMask {
    /// Creates a direct Event mask. Existence and strict precedence are checked by the query/commit context.
    #[must_use]
    pub const fn new(
        id: EventMaskId,
        history_space_id: HistorySpaceId,
        layer_id: LayerId,
        target_event: EventId,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            history_space_id,
            layer_id,
            target_event,
            created_revision,
        }
    }

    /// Returns the stable EventMask identity.
    #[must_use]
    pub const fn id(self) -> EventMaskId {
        self.id
    }

    /// Returns the owning HistorySpace.
    #[must_use]
    pub const fn history_space_id(self) -> HistorySpaceId {
        self.history_space_id
    }

    /// Returns the explicit Layer.
    #[must_use]
    pub const fn layer_id(self) -> LayerId {
        self.layer_id
    }

    /// Returns the concrete target Event.
    #[must_use]
    pub const fn target_event(self) -> EventId {
        self.target_event
    }

    /// Returns the Transaction-Time revision that created this record.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One immutable closure of an Event span that was created open.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EventSpanClosure {
    id: EventSpanClosureId,
    event_id: EventId,
    close_at_event_time: WorldTime,
    created_revision: Revision,
}

impl EventSpanClosure {
    /// Closes an open Event span at a later point on its original timeline.
    pub fn new(
        id: EventSpanClosureId,
        event: &Event,
        close_at_event_time: WorldTime,
        created_revision: Revision,
    ) -> Result<Self, EventRecordError> {
        validate_lifecycle_revision(event.created_revision(), created_revision)?;
        let EventTime::Span { start, end: None } = event.event_time() else {
            return Err(EventRecordError::SpanClosureRequiresOpenSpan);
        };
        if start.timeline().id() != close_at_event_time.timeline().id() {
            return Err(EventRecordError::CloseTime(
                TemporalError::IncomparableTimelines {
                    expected: start.timeline().id(),
                    actual: close_at_event_time.timeline().id(),
                },
            ));
        }
        if close_at_event_time
            .checked_cmp(start)
            .map_err(EventRecordError::CloseTime)?
            != Ordering::Greater
        {
            return Err(EventRecordError::CloseTime(
                TemporalError::NonPositiveEventSpan,
            ));
        }
        Ok(Self {
            id,
            event_id: event.id(),
            close_at_event_time,
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target is open and that the close coordinate is later.
    pub(crate) const fn from_wire_fields(
        id: EventSpanClosureId,
        event_id: EventId,
        close_at_event_time: WorldTime,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            event_id,
            close_at_event_time,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(self) -> EventSpanClosureId {
        self.id
    }

    /// Returns the target Event identity.
    #[must_use]
    pub const fn event_id(self) -> EventId {
        self.event_id
    }

    /// Returns the exclusive event-time end introduced by this record.
    #[must_use]
    pub const fn close_at_event_time(self) -> WorldTime {
        self.close_at_event_time
    }

    /// Returns the Transaction-Time revision that created this closure.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One immutable Transaction-Time retraction of an Event.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EventRetraction {
    id: EventRetractionId,
    event_id: EventId,
    reason: String,
    created_revision: Revision,
}

impl EventRetraction {
    /// Creates a retraction after its target Event.
    pub fn new(
        id: EventRetractionId,
        event: &Event,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, EventRecordError> {
        validate_lifecycle_revision(event.created_revision(), created_revision)?;
        Ok(Self {
            id,
            event_id: event.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: EventRetractionId,
        event_id: EventId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            event_id,
            reason,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(&self) -> EventRetractionId {
        self.id
    }

    /// Returns the target Event identity.
    #[must_use]
    pub const fn event_id(&self) -> EventId {
        self.event_id
    }

    /// Returns the required retraction reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time revision at which the retraction applies.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// One immutable Transaction-Time retraction of an EventMask.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EventMaskRetraction {
    id: EventMaskRetractionId,
    event_mask_id: EventMaskId,
    reason: String,
    created_revision: Revision,
}

impl EventMaskRetraction {
    /// Creates a retraction after its target EventMask.
    pub fn new(
        id: EventMaskRetractionId,
        event_mask: &EventMask,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, EventRecordError> {
        validate_lifecycle_revision(event_mask.created_revision(), created_revision)?;
        Ok(Self {
            id,
            event_mask_id: event_mask.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: EventMaskRetractionId,
        event_mask_id: EventMaskId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            event_mask_id,
            reason,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(&self) -> EventMaskRetractionId {
        self.id
    }

    /// Returns the target EventMask identity.
    #[must_use]
    pub const fn event_mask_id(&self) -> EventMaskId {
        self.event_mask_id
    }

    /// Returns the required retraction reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time revision at which the retraction applies.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

fn validate_role_cardinality(
    role_id: EventRoleId,
    cardinality: RoleCardinality,
    actual: u32,
) -> Result<(), EventRecordError> {
    if actual < cardinality.min() || cardinality.max().is_some_and(|max| actual > max) {
        return Err(EventRecordError::RoleCardinalityViolation {
            role_id,
            actual,
            min: cardinality.min(),
            max: cardinality.max(),
        });
    }
    Ok(())
}

fn validate_attribute_kind(
    definition: &EventAttributeDefinition,
    attribute: &EventAttributeValue,
) -> Result<(), EventRecordError> {
    let actual = ValueKind::of(attribute.value());
    if actual != definition.value_kind() {
        return Err(EventRecordError::AttributeValueKindMismatch {
            attribute_id: definition.event_attribute_id(),
            expected: definition.value_kind(),
            actual,
        });
    }
    Ok(())
}

fn validate_event_time(event_time: EventTime, form: EventTimeForm) -> Result<(), EventRecordError> {
    match (event_time, form) {
        (EventTime::Instant(_), EventTimeForm::InstantOnly | EventTimeForm::InstantOrSpan)
        | (EventTime::Instant(_), EventTimeForm::OpenSpanAllowed) => Ok(()),
        (EventTime::Span { end: Some(_), .. }, EventTimeForm::InstantOnly) => {
            Err(EventRecordError::EventTimeFormMismatch { expected: form })
        }
        (EventTime::Span { end: Some(_), .. }, _) => validate_closed_span(event_time),
        (EventTime::Span { end: None, .. }, EventTimeForm::OpenSpanAllowed) => Ok(()),
        (EventTime::Span { end: None, .. }, _) => {
            Err(EventRecordError::EventTimeFormMismatch { expected: form })
        }
        (EventTime::Instant(_), EventTimeForm::SpanOnly) => {
            Err(EventRecordError::EventTimeFormMismatch { expected: form })
        }
    }
}

fn validate_closed_span(event_time: EventTime) -> Result<(), EventRecordError> {
    if let EventTime::Span {
        start,
        end: Some(end),
    } = event_time
    {
        let order = start
            .checked_cmp(end)
            .map_err(EventRecordError::EventTime)?;
        if order != Ordering::Less {
            return Err(EventRecordError::EventTime(
                TemporalError::NonPositiveEventSpan,
            ));
        }
    }
    Ok(())
}

fn validate_lifecycle_revision(
    event_revision: Revision,
    lifecycle_revision: Revision,
) -> Result<(), EventRecordError> {
    if lifecycle_revision <= event_revision {
        return Err(EventRecordError::LifecycleRevisionNotAfterTarget {
            target_revision: event_revision,
            lifecycle_revision,
        });
    }
    Ok(())
}

/// A rejected Event/EventMask shape, schema reference, or lifecycle value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventRecordError {
    /// A participant references a role absent from the selected EventKind.
    UnknownParticipantRole {
        /// The missing EventRole identity.
        role_id: EventRoleId,
    },
    /// The same role/entity pair was provided more than once.
    DuplicateParticipant {
        /// The repeated role identity.
        role_id: EventRoleId,
        /// The repeated entity identity.
        entity_id: EntityId,
    },
    /// The role's minimum or maximum participant count was violated.
    RoleCardinalityViolation {
        /// The affected role identity.
        role_id: EventRoleId,
        /// Number of supplied participants.
        actual: u32,
        /// Required minimum count.
        min: u32,
        /// Optional maximum count.
        max: Option<u32>,
    },
    /// Participant count could not be represented by the schema's u32 bounds.
    ParticipantCountOverflow,
    /// An attribute references an identity absent from the selected EventKind.
    UnknownAttribute {
        /// The missing EventAttribute identity.
        attribute_id: EventAttributeId,
    },
    /// The same EventAttribute identity was supplied more than once.
    DuplicateAttribute {
        /// The repeated EventAttribute identity.
        attribute_id: EventAttributeId,
    },
    /// A declared required attribute was omitted.
    MissingRequiredAttribute {
        /// The missing EventAttribute identity.
        attribute_id: EventAttributeId,
    },
    /// An attribute's Value variant differs from its schema ValueKind.
    AttributeValueKindMismatch {
        /// The schema attribute identity.
        attribute_id: EventAttributeId,
        /// The schema-required exact value family.
        expected: ValueKind,
        /// The supplied exact value family.
        actual: ValueKind,
    },
    /// EventTime form was not accepted by the selected EventKind.
    EventTimeFormMismatch {
        /// The EventKind's allowed form.
        expected: EventTimeForm,
    },
    /// EventTime points to a different timeline or has a non-positive span.
    EventTime(TemporalError),
    /// The EventKind schema definition is newer than the Event record revision.
    EventKindNotYetPublished {
        /// Revision that introduced the EventKind definition.
        event_kind_revision: Revision,
        /// Proposed Event creation revision.
        event_revision: Revision,
    },
    /// EventSpanClosure can target only a span created without an end.
    SpanClosureRequiresOpenSpan,
    /// An Event/EventMask lifecycle record must follow its target on Transaction Time.
    LifecycleRevisionNotAfterTarget {
        /// Target record creation revision.
        target_revision: Revision,
        /// Proposed lifecycle-record creation revision.
        lifecycle_revision: Revision,
    },
    /// EventSpanClosure time is invalid for the open target span.
    CloseTime(TemporalError),
}

impl fmt::Display for EventRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownParticipantRole { role_id } => {
                write!(formatter, "EventKind does not declare role {role_id}")
            }
            Self::DuplicateParticipant { role_id, entity_id } => {
                write!(
                    formatter,
                    "duplicate participant pair {role_id}/{entity_id}"
                )
            }
            Self::RoleCardinalityViolation {
                role_id,
                actual,
                min,
                max,
            } => write!(
                formatter,
                "role {role_id} has {actual} participants; expected {min}..={max:?}"
            ),
            Self::ParticipantCountOverflow => {
                formatter.write_str("participant count exceeds schema cardinality range")
            }
            Self::UnknownAttribute { attribute_id } => {
                write!(
                    formatter,
                    "EventKind does not declare attribute {attribute_id}"
                )
            }
            Self::DuplicateAttribute { attribute_id } => {
                write!(formatter, "duplicate EventAttribute {attribute_id}")
            }
            Self::MissingRequiredAttribute { attribute_id } => {
                write!(
                    formatter,
                    "required EventAttribute {attribute_id} is absent"
                )
            }
            Self::AttributeValueKindMismatch {
                attribute_id,
                expected,
                actual,
            } => write!(
                formatter,
                "EventAttribute {attribute_id} expects {expected:?}, got {actual:?}"
            ),
            Self::EventTimeFormMismatch { expected } => {
                write!(formatter, "EventTime is not allowed by {expected:?}")
            }
            Self::EventTime(error) => write!(formatter, "invalid EventTime: {error}"),
            Self::EventKindNotYetPublished {
                event_kind_revision,
                event_revision,
            } => write!(
                formatter,
                "EventKind revision {event_kind_revision} is after Event revision {event_revision}"
            ),
            Self::SpanClosureRequiresOpenSpan => {
                formatter.write_str("EventSpanClosure requires an Event created with an open span")
            }
            Self::LifecycleRevisionNotAfterTarget {
                target_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "Event lifecycle revision {lifecycle_revision} must be after target revision {target_revision}"
            ),
            Self::CloseTime(error) => write!(formatter, "invalid EventSpanClosure time: {error}"),
        }
    }
}

impl std::error::Error for EventRecordError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        Event, EventAttributeValue, EventAttributes, EventDraft, EventMask, EventMaskRetraction,
        EventParticipant, EventRecordError, EventRetraction, EventSpanClosure, EventWireFields,
        Participants,
    };
    use crate::UInt;
    use crate::archive::ArchiveTargetRef;
    use crate::archive_projection::{ArchiveHistoryReferenceModel, ArchiveTargetRecord};
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition};
    use crate::event_projection::{
        EventCandidateQuery, EventHistory, EventProjectionError, EventTimeFilter,
        event_is_authorized, full_scan_event_candidates, prepare_event_correction,
    };
    use crate::ids::{
        DomainId, EntityId, EventAttributeId, EventId, EventKindId as SchemaEventKindId,
        EventMaskId, EventMaskRetractionId, EventRetractionId, EventRoleId, EventSpanClosureId,
        HistorySpaceId, IdValidationError, LayerId, ProvenanceId, Revision, RevisionError,
        SchemaRevision, TimelineId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
    use crate::schema::{
        ConstraintSet, EntityTypeConstraint, EventAttributeDefinition, EventKindDefinition,
        EventRoleDefinition, EventTimeConstraint, EventTimeForm, Lifecycle, RoleCardinality,
        SchemaDefinitionError, ValueKind,
    };
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, FieldSelector, GrantEffect, PolicyBundle,
        PolicyScope, PolicySubject, Principal, RoleAssignment, RoleDefinition,
        SecurityPolicySnapshot,
    };
    use crate::temporal::{EventTime, RecordedAsOf, TemporalError, Timeline, WorldTime};
    use crate::values::{Symbol, SymbolError, Value};

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Event(EventRecordError),
        Id(IdValidationError),
        Revision(RevisionError),
        Schema(SchemaDefinitionError),
        Symbol(SymbolError),
        Temporal(TemporalError),
        EventProjection(EventProjectionError),
        Archive(crate::archive_projection::ArchiveProjectionError),
        History(crate::catalog::HistorySpaceError),
        Layer(LayerSchemaError),
        Policy(crate::security::SecurityPolicyError),
        Role(crate::security::RoleDefinitionError),
        Bundle(crate::security::PolicyBundleError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Event(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Schema(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
                Self::Temporal(error) => write!(formatter, "{error}"),
                Self::EventProjection(error) => write!(formatter, "{error}"),
                Self::Archive(error) => write!(formatter, "{error}"),
                Self::History(error) => write!(formatter, "{error}"),
                Self::Layer(error) => write!(formatter, "{error}"),
                Self::Policy(error) => write!(formatter, "{error}"),
                Self::Role(error) => write!(formatter, "{error}"),
                Self::Bundle(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl Error for TestError {}

    macro_rules! error_conversion {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    error_conversion!(EventRecordError, Event);
    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);
    error_conversion!(SchemaDefinitionError, Schema);
    error_conversion!(SymbolError, Symbol);
    error_conversion!(TemporalError, Temporal);
    error_conversion!(EventProjectionError, EventProjection);
    error_conversion!(crate::archive_projection::ArchiveProjectionError, Archive);
    error_conversion!(crate::catalog::HistorySpaceError, History);
    error_conversion!(LayerSchemaError, Layer);
    error_conversion!(crate::security::SecurityPolicyError, Policy);
    error_conversion!(crate::security::RoleDefinitionError, Role);
    error_conversion!(crate::security::PolicyBundleError, Bundle);

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn uuid<T: DomainId>(byte: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn event_kind(
        form: EventTimeForm,
        minimum: u32,
        maximum: Option<u32>,
        created_revision: Revision,
    ) -> Result<EventKindDefinition, TestError> {
        let role_id = value!(uuid::<EventRoleId>(1));
        let title_id = value!(uuid::<EventAttributeId>(2));
        let count_id = value!(uuid::<EventAttributeId>(3));
        let role = EventRoleDefinition::new(
            role_id,
            value!(Symbol::new("actor")),
            EntityTypeConstraint::AnyEntity,
            value!(RoleCardinality::new(minimum, maximum)),
        );
        let title = value!(EventAttributeDefinition::new(
            title_id,
            value!(Symbol::new("title")),
            ValueKind::String,
            None,
            ConstraintSet::unconstrained(),
            None,
            true,
        ));
        let count = value!(EventAttributeDefinition::new(
            count_id,
            value!(Symbol::new("count")),
            ValueKind::UInt,
            None,
            ConstraintSet::unconstrained(),
            None,
            false,
        ));
        let time_constraint = value!(EventTimeConstraint::new(form, None));
        Ok(value!(EventKindDefinition::new(
            value!(uuid::<SchemaEventKindId>(4)),
            value!(Symbol::new("event_kind")),
            vec![role],
            vec![title, count],
            time_constraint,
            Lifecycle::Active,
            created_revision,
        )))
    }

    fn participants(entity_bytes: &[u8]) -> Result<Vec<EventParticipant>, TestError> {
        let role_id = value!(uuid::<EventRoleId>(1));
        let mut result = Vec::new();
        for byte in entity_bytes {
            result.push(EventParticipant::new(
                role_id,
                value!(uuid::<EntityId>(*byte)),
            ));
        }
        Ok(result)
    }

    fn attributes() -> Result<Vec<EventAttributeValue>, TestError> {
        Ok(vec![EventAttributeValue::new(
            value!(uuid::<EventAttributeId>(2)),
            Value::String(String::from("arrival")),
        )])
    }

    fn draft(
        event_kind: &EventKindDefinition,
        event_time: EventTime,
    ) -> Result<EventDraft, TestError> {
        Ok(value!(EventDraft::new(
            value!(uuid::<HistorySpaceId>(5)),
            value!(uuid::<LayerId>(6)),
            event_kind,
            participants(&[8])?,
            attributes()?,
            event_time,
        )))
    }

    fn projection_layers() -> Result<(LayerSchemaSnapshot, crate::ids::LayerId), TestError> {
        let layer = value!(uuid::<LayerId>(6));
        let revision = SchemaRevision::from_published_revision(Revision::GENESIS);
        let snapshot = LayerSchemaSnapshot::new(
            revision,
            vec![LayerDefinition::new(
                layer,
                value!(Symbol::new("base")),
                None,
                0,
                Lifecycle::Active,
                revision,
            )],
            layer,
        )?;
        Ok((snapshot, layer))
    }

    fn archive_events(events: &[Event]) -> Result<ArchiveHistoryReferenceModel, TestError> {
        let targets = events
            .iter()
            .map(|event| {
                Ok(ArchiveTargetRecord::new(
                    ArchiveTargetRef::Event(event.id()),
                    event.created_revision(),
                ))
            })
            .collect::<Result<Vec<_>, TestError>>()?;
        Ok(ArchiveHistoryReferenceModel::new(targets, Vec::new())?)
    }

    #[test]
    fn event_is_hidden_when_a_candidate_field_is_denied() -> TestResult {
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        let event = value!(Event::new(
            value!(uuid::<EventId>(85)),
            draft(
                &kind,
                EventTime::Instant(WorldTime::from_nanoseconds(
                    crate::temporal::Timeline::new(value!(uuid::<TimelineId>(86))),
                    10,
                ))
            )?,
            Revision::GENESIS,
        ));
        let principal = value!(uuid::<crate::ids::PrincipalId>(87));
        let role_id = value!(uuid::<crate::ids::RoleId>(88));
        let assignment_id = value!(uuid::<crate::ids::RoleAssignmentId>(89));
        let role = RoleDefinition::new(
            role_id,
            "event_reader",
            PolicyBundle::from_grants([
                CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::LayerRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::EventRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::FieldRead, GrantEffect::Allow),
            ])?,
        )?;
        let assignment =
            RoleAssignment::new(assignment_id, principal, role_id, PolicyScope::project());
        let allow = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role.clone()],
            vec![assignment],
            Vec::new(),
        )?;
        assert!(event_is_authorized(&allow, principal, &event));
        let deny_participant = CapabilityRule::new(
            value!(uuid::<crate::ids::PolicyRuleId>(90)),
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
            PolicyScope::new(
                Some(event.history_space_id()),
                Some(event.layer_id()),
                Some(crate::RecordRef::Event(event.id())),
                Some(FieldSelector::EventParticipant(
                    kind.event_kind_id(),
                    value!(uuid::<EventRoleId>(1)),
                )),
                None,
            ),
        );
        let denied = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role],
            vec![assignment],
            vec![deny_participant],
        )?;
        assert!(!event_is_authorized(&denied, principal, &event));
        Ok(())
    }

    #[test]
    fn participants_are_canonical_unique_and_cardinality_checked() -> TestResult {
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        let role_id = value!(uuid::<EventRoleId>(1));
        let first_entity = value!(uuid::<EntityId>(8));
        let second_entity = value!(uuid::<EntityId>(9));
        let first = EventParticipant::new(role_id, first_entity);
        let second = EventParticipant::new(role_id, second_entity);
        let canonical = value!(Participants::new(vec![second, first], &kind));
        assert_eq!(canonical.as_slice().first().copied(), Some(first));
        assert_eq!(canonical.as_slice().get(1).copied(), Some(second));

        assert_eq!(
            Participants::new(vec![first, first], &kind).err(),
            Some(EventRecordError::DuplicateParticipant {
                role_id,
                entity_id: first_entity,
            })
        );
        assert_eq!(
            Participants::new(Vec::new(), &kind).err(),
            Some(EventRecordError::RoleCardinalityViolation {
                role_id,
                actual: 0,
                min: 1,
                max: Some(2),
            })
        );
        assert_eq!(
            Participants::new(
                vec![
                    first,
                    second,
                    EventParticipant::new(role_id, value!(uuid::<EntityId>(10))),
                ],
                &kind,
            )
            .err(),
            Some(EventRecordError::RoleCardinalityViolation {
                role_id,
                actual: 3,
                min: 1,
                max: Some(2),
            })
        );
        assert!(matches!(
            Participants::new(
                vec![EventParticipant::new(
                    value!(uuid::<EventRoleId>(11)),
                    first_entity,
                )],
                &kind,
            ),
            Err(EventRecordError::UnknownParticipantRole { .. })
        ));
        Ok(())
    }

    #[test]
    fn attributes_are_sorted_unique_required_and_exactly_typed() -> TestResult {
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        let title_id = value!(uuid::<EventAttributeId>(2));
        let count_id = value!(uuid::<EventAttributeId>(3));
        let ordered = super::EventAttributes::new(
            vec![
                EventAttributeValue::new(count_id, Value::UInt(UInt::new(2))),
                EventAttributeValue::new(title_id, Value::String(String::from("arrival"))),
            ],
            &kind,
        )?;
        assert_eq!(
            ordered
                .as_slice()
                .first()
                .map(EventAttributeValue::attribute_id),
            Some(title_id)
        );

        assert_eq!(
            super::EventAttributes::new(
                vec![EventAttributeValue::new(
                    count_id,
                    Value::UInt(UInt::new(1)),
                )],
                &kind,
            )
            .err(),
            Some(EventRecordError::MissingRequiredAttribute {
                attribute_id: title_id,
            })
        );
        assert_eq!(
            super::EventAttributes::new(
                vec![
                    EventAttributeValue::new(title_id, Value::String(String::from("first")),),
                    EventAttributeValue::new(title_id, Value::String(String::from("second")),),
                ],
                &kind,
            )
            .err(),
            Some(EventRecordError::DuplicateAttribute {
                attribute_id: title_id,
            })
        );
        assert_eq!(
            super::EventAttributes::new(
                vec![EventAttributeValue::new(
                    title_id,
                    Value::UInt(UInt::new(1)),
                )],
                &kind,
            )
            .err(),
            Some(EventRecordError::AttributeValueKindMismatch {
                attribute_id: title_id,
                expected: ValueKind::String,
                actual: ValueKind::UInt,
            })
        );
        assert_eq!(
            super::EventAttributes::new(
                vec![
                    EventAttributeValue::new(title_id, Value::String(String::from("arrival")),),
                    EventAttributeValue::new(
                        value!(uuid::<EventAttributeId>(12)),
                        Value::Bool(true),
                    ),
                ],
                &kind,
            )
            .err(),
            Some(EventRecordError::UnknownAttribute {
                attribute_id: value!(uuid::<EventAttributeId>(12)),
            })
        );
        Ok(())
    }

    #[test]
    fn event_kind_time_form_and_span_shape_are_enforced() -> TestResult {
        let timeline = Timeline::new(value!(uuid::<TimelineId>(13)));
        let start = WorldTime::from_nanoseconds(timeline, 10);
        let end = WorldTime::from_nanoseconds(timeline, 20);
        let instant_kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        assert!(draft(&instant_kind, EventTime::Instant(start)).is_ok());
        assert!(matches!(
            draft(&instant_kind, EventTime::span(start, Some(end))?),
            Err(TestError::Event(EventRecordError::EventTimeFormMismatch {
                expected: EventTimeForm::InstantOnly,
            }))
        ));

        let span_kind = event_kind(EventTimeForm::SpanOnly, 1, Some(2), Revision::GENESIS)?;
        assert!(draft(&span_kind, EventTime::span(start, Some(end))?).is_ok());
        assert!(matches!(
            draft(&span_kind, EventTime::Instant(start)),
            Err(TestError::Event(EventRecordError::EventTimeFormMismatch {
                expected: EventTimeForm::SpanOnly,
            }))
        ));
        assert!(matches!(
            draft(&span_kind, EventTime::Span { start, end: None }),
            Err(TestError::Event(EventRecordError::EventTimeFormMismatch {
                expected: EventTimeForm::SpanOnly,
            }))
        ));

        let open_kind = event_kind(
            EventTimeForm::OpenSpanAllowed,
            1,
            Some(2),
            Revision::GENESIS,
        )?;
        assert!(draft(&open_kind, EventTime::Span { start, end: None }).is_ok());
        assert!(matches!(
            draft(
                &open_kind,
                EventTime::Span {
                    start: end,
                    end: Some(start),
                },
            ),
            Err(TestError::Event(EventRecordError::EventTime(
                TemporalError::NonPositiveEventSpan,
            )))
        ));
        Ok(())
    }

    #[test]
    fn event_kind_must_be_published_before_the_event_revision() -> TestResult {
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), value!(revision(4)))?;
        let timeline = Timeline::new(value!(uuid::<TimelineId>(30)));
        let draft = draft(
            &kind,
            EventTime::Instant(WorldTime::from_nanoseconds(timeline, 10)),
        )?;
        assert_eq!(
            Event::new(value!(uuid::<EventId>(31)), draft, value!(revision(3))).err(),
            Some(EventRecordError::EventKindNotYetPublished {
                event_kind_revision: value!(revision(4)),
                event_revision: value!(revision(3)),
            })
        );
        Ok(())
    }

    #[test]
    fn event_mask_and_event_lifecycle_records_use_distinct_ids_and_revisions() -> TestResult {
        let timeline = Timeline::new(value!(uuid::<TimelineId>(14)));
        let start = WorldTime::from_nanoseconds(timeline, 10);
        let kind = event_kind(
            EventTimeForm::OpenSpanAllowed,
            1,
            Some(2),
            Revision::GENESIS,
        )?;
        let event = value!(Event::new(
            value!(uuid::<EventId>(15)),
            draft(&kind, EventTime::Span { start, end: None })?,
            value!(revision(3)),
        ));
        let event_mask = EventMask::new(
            value!(uuid::<EventMaskId>(16)),
            value!(uuid::<HistorySpaceId>(17)),
            value!(uuid::<LayerId>(18)),
            event.id(),
            value!(revision(4)),
        );
        let mask_retraction = value!(EventMaskRetraction::new(
            value!(uuid::<EventMaskRetractionId>(19)),
            &event_mask,
            "mask correction",
            value!(revision(5)),
        ));
        let event_retraction = value!(EventRetraction::new(
            value!(uuid::<EventRetractionId>(20)),
            &event,
            "event correction",
            value!(revision(5)),
        ));
        let closure = value!(EventSpanClosure::new(
            value!(uuid::<EventSpanClosureId>(21)),
            &event,
            WorldTime::from_nanoseconds(timeline, 20),
            value!(revision(5)),
        ));

        assert_eq!(event_mask.target_event(), event.id());
        assert_eq!(mask_retraction.event_mask_id(), event_mask.id());
        assert_eq!(event_retraction.event_id(), event.id());
        assert_eq!(closure.event_id(), event.id());
        assert_eq!(event_retraction.reason(), "event correction");
        assert_eq!(
            EventRetraction::new(
                value!(uuid::<EventRetractionId>(22)),
                &event,
                "too early",
                value!(revision(3)),
            )
            .err(),
            Some(EventRecordError::LifecycleRevisionNotAfterTarget {
                target_revision: value!(revision(3)),
                lifecycle_revision: value!(revision(3)),
            })
        );
        Ok(())
    }

    #[test]
    fn event_span_closure_rejects_closed_targets_and_invalid_endpoints() -> TestResult {
        let timeline = Timeline::new(value!(uuid::<TimelineId>(23)));
        let start = WorldTime::from_nanoseconds(timeline, 10);
        let kind = event_kind(
            EventTimeForm::OpenSpanAllowed,
            1,
            Some(2),
            Revision::GENESIS,
        )?;
        let closed_event = value!(Event::new(
            value!(uuid::<EventId>(24)),
            draft(
                &kind,
                EventTime::span(start, Some(WorldTime::from_nanoseconds(timeline, 20)))?
            )?,
            value!(revision(3)),
        ));
        assert_eq!(
            EventSpanClosure::new(
                value!(uuid::<EventSpanClosureId>(25)),
                &closed_event,
                WorldTime::from_nanoseconds(timeline, 30),
                value!(revision(4)),
            )
            .err(),
            Some(EventRecordError::SpanClosureRequiresOpenSpan)
        );

        let open_event = value!(Event::new(
            value!(uuid::<EventId>(26)),
            draft(&kind, EventTime::Span { start, end: None })?,
            value!(revision(3)),
        ));
        assert_eq!(
            EventSpanClosure::new(
                value!(uuid::<EventSpanClosureId>(27)),
                &open_event,
                start,
                value!(revision(4)),
            )
            .err(),
            Some(EventRecordError::CloseTime(
                TemporalError::NonPositiveEventSpan,
            ))
        );
        assert_eq!(
            EventSpanClosure::new(
                value!(uuid::<EventSpanClosureId>(28)),
                &open_event,
                WorldTime::from_nanoseconds(Timeline::new(value!(uuid::<TimelineId>(29))), 30,),
                value!(revision(4)),
            )
            .err(),
            Some(EventRecordError::CloseTime(
                TemporalError::IncomparableTimelines {
                    expected: timeline.id(),
                    actual: value!(uuid::<TimelineId>(29)),
                }
            ))
        );
        Ok(())
    }

    #[test]
    fn event_projection_respects_schema_time_closure_retraction_and_half_open_filters() -> TestResult
    {
        let root = value!(uuid::<HistorySpaceId>(5));
        let (layer_snapshot, _) = projection_layers()?;
        let history_spaces = value!(HistorySpaceCatalog::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?]));
        let timeline = Timeline::new(value!(uuid::<TimelineId>(30)));
        let kind = event_kind(
            EventTimeForm::OpenSpanAllowed,
            1,
            Some(2),
            Revision::GENESIS,
        )?;
        let closed_span = value!(Event::new(
            value!(uuid::<EventId>(31)),
            draft(
                &kind,
                EventTime::Span {
                    start: WorldTime::from_nanoseconds(timeline, 10),
                    end: None
                }
            )?,
            value!(revision(2)),
        ));
        let closure = value!(EventSpanClosure::new(
            value!(uuid::<EventSpanClosureId>(32)),
            &closed_span,
            WorldTime::from_nanoseconds(timeline, 20),
            value!(revision(4)),
        ));
        let instant_at_end = value!(Event::new(
            value!(uuid::<EventId>(33)),
            draft(
                &kind,
                EventTime::Instant(WorldTime::from_nanoseconds(timeline, 20))
            )?,
            value!(revision(2)),
        ));
        let still_open = value!(Event::new(
            value!(uuid::<EventId>(34)),
            draft(
                &kind,
                EventTime::Span {
                    start: WorldTime::from_nanoseconds(timeline, 25),
                    end: None
                }
            )?,
            value!(revision(2)),
        ));
        let later_retracted = value!(Event::new(
            value!(uuid::<EventId>(35)),
            draft(
                &kind,
                EventTime::Instant(WorldTime::from_nanoseconds(timeline, 15))
            )?,
            value!(revision(2)),
        ));
        let retraction = value!(EventRetraction::new(
            value!(uuid::<EventRetractionId>(36)),
            &later_retracted,
            "event withdrawn",
            value!(revision(5)),
        ));
        let events = vec![
            closed_span.clone(),
            instant_at_end.clone(),
            still_open.clone(),
            later_retracted.clone(),
        ];
        let archive = archive_events(&events)?;
        let query_before_closure = EventCandidateQuery::new(
            root,
            LayerSelection::BaseOnly,
            RecordedAsOf::from_published_revision(value!(revision(3))),
            EventTimeFilter::Overlaps {
                start: WorldTime::from_nanoseconds(timeline, 15),
                end: WorldTime::from_nanoseconds(timeline, 20),
            },
        );
        let as_of_before_closure = full_scan_event_candidates(
            EventHistory::new(
                &events,
                std::slice::from_ref(&closure),
                std::slice::from_ref(&retraction),
                &archive,
                std::slice::from_ref(&kind),
            ),
            &history_spaces,
            &layer_snapshot,
            &query_before_closure,
        )?;
        assert_eq!(
            as_of_before_closure
                .iter()
                .map(|candidate| candidate.event().id())
                .collect::<Vec<_>>(),
            vec![closed_span.id(), later_retracted.id()]
        );

        let query_at_end = EventCandidateQuery::new(
            root,
            LayerSelection::BaseOnly,
            RecordedAsOf::from_published_revision(value!(revision(4))),
            EventTimeFilter::At(WorldTime::from_nanoseconds(timeline, 20)),
        );
        let at_exclusive_end = full_scan_event_candidates(
            EventHistory::new(
                &events,
                std::slice::from_ref(&closure),
                std::slice::from_ref(&retraction),
                &archive,
                std::slice::from_ref(&kind),
            ),
            &history_spaces,
            &layer_snapshot,
            &query_at_end,
        )?;
        assert_eq!(
            at_exclusive_end
                .iter()
                .map(|candidate| candidate.event().id())
                .collect::<Vec<_>>(),
            vec![instant_at_end.id()]
        );

        let query_after_retraction = EventCandidateQuery::new(
            root,
            LayerSelection::BaseOnly,
            RecordedAsOf::from_published_revision(value!(revision(5))),
            EventTimeFilter::At(WorldTime::from_nanoseconds(timeline, 15)),
        );
        let after_retraction = full_scan_event_candidates(
            EventHistory::new(
                &events,
                std::slice::from_ref(&closure),
                std::slice::from_ref(&retraction),
                &archive,
                std::slice::from_ref(&kind),
            ),
            &history_spaces,
            &layer_snapshot,
            &query_after_retraction,
        )?;
        assert_eq!(
            after_retraction
                .iter()
                .map(|candidate| candidate.event().id())
                .collect::<Vec<_>>(),
            vec![closed_span.id()]
        );
        Ok(())
    }

    #[test]
    fn event_mask_requires_strict_precedence_and_retracts_independently() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(5));
        let child = value!(uuid::<HistorySpaceId>(51));
        let history_spaces = value!(HistorySpaceCatalog::new(vec![
            HistorySpaceDefinition::new(root, None, Revision::GENESIS)?,
            HistorySpaceDefinition::new(child, Some(root), value!(revision(2)))?,
        ]));
        let (layer_snapshot, layer) = projection_layers()?;
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        let event = value!(Event::new(
            value!(uuid::<EventId>(52)),
            draft(
                &kind,
                EventTime::Instant(WorldTime::from_nanoseconds(
                    Timeline::new(value!(uuid::<TimelineId>(53))),
                    12,
                )),
            )?,
            value!(revision(2)),
        ));
        let mask = EventMask::new(
            value!(uuid::<EventMaskId>(54)),
            child,
            layer,
            event.id(),
            value!(revision(3)),
        );
        let mask_retraction = value!(EventMaskRetraction::new(
            value!(uuid::<EventMaskRetractionId>(55)),
            &mask,
            "mask superseded",
            value!(revision(5)),
        ));
        let event_retraction = value!(EventRetraction::new(
            value!(uuid::<EventRetractionId>(57)),
            &event,
            "event independently withdrawn",
            value!(revision(6)),
        ));
        let archive = value!(ArchiveHistoryReferenceModel::new(
            vec![
                ArchiveTargetRecord::new(
                    ArchiveTargetRef::Event(event.id()),
                    event.created_revision()
                ),
                ArchiveTargetRecord::new(
                    ArchiveTargetRef::EventMask(mask.id()),
                    mask.created_revision()
                ),
            ],
            Vec::new(),
        ));
        let events = [event.clone()];
        let masks = [mask];
        let retractions = [mask_retraction];
        let query = |revision_value| {
            EventCandidateQuery::new(
                child,
                LayerSelection::BaseOnly,
                RecordedAsOf::from_published_revision(revision_value),
                EventTimeFilter::Any,
            )
        };

        let before_mask = full_scan_event_candidates(
            EventHistory::new(&events, &[], &[], &archive, std::slice::from_ref(&kind))
                .with_masks(&masks, &retractions),
            &history_spaces,
            &layer_snapshot,
            &query(value!(revision(2))),
        )?;
        assert_eq!(before_mask.len(), 1);

        let hidden = full_scan_event_candidates(
            EventHistory::new(&events, &[], &[], &archive, std::slice::from_ref(&kind))
                .with_masks(&masks, &retractions),
            &history_spaces,
            &layer_snapshot,
            &query(value!(revision(4))),
        )?;
        assert!(hidden.is_empty());

        let visible_after_mask_retraction = full_scan_event_candidates(
            EventHistory::new(&events, &[], &[], &archive, std::slice::from_ref(&kind))
                .with_masks(&masks, &retractions),
            &history_spaces,
            &layer_snapshot,
            &query(value!(revision(5))),
        )?;
        assert_eq!(
            visible_after_mask_retraction
                .iter()
                .map(|candidate| candidate.event().id())
                .collect::<Vec<_>>(),
            vec![event.id()]
        );

        let independently_retracted = full_scan_event_candidates(
            EventHistory::new(
                &events,
                &[],
                std::slice::from_ref(&event_retraction),
                &archive,
                std::slice::from_ref(&kind),
            )
            .with_masks(&[], &[]),
            &history_spaces,
            &layer_snapshot,
            &query(value!(revision(6))),
        )?;
        assert!(independently_retracted.is_empty());

        let same_precedence_mask = [EventMask::new(
            value!(uuid::<EventMaskId>(56)),
            root,
            layer,
            event.id(),
            value!(revision(3)),
        )];
        let same_precedence_archive = value!(ArchiveHistoryReferenceModel::new(
            vec![
                ArchiveTargetRecord::new(
                    ArchiveTargetRef::Event(event.id()),
                    event.created_revision()
                ),
                ArchiveTargetRecord::new(
                    ArchiveTargetRef::EventMask(same_precedence_mask[0].id()),
                    same_precedence_mask[0].created_revision(),
                ),
            ],
            Vec::new(),
        ));
        let equal_precedence_result = full_scan_event_candidates(
            EventHistory::new(
                &events,
                &[],
                &[],
                &same_precedence_archive,
                std::slice::from_ref(&kind),
            )
            .with_masks(&same_precedence_mask, &[]),
            &history_spaces,
            &layer_snapshot,
            &query(value!(revision(4))),
        )?;
        assert_eq!(equal_precedence_result.len(), 1);
        Ok(())
    }

    #[test]
    fn event_projection_revalidates_decoded_role_attribute_and_time_shape() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(5));
        let (layer_snapshot, layer) = projection_layers()?;
        let history_spaces = value!(HistorySpaceCatalog::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?]));
        let timeline = Timeline::new(value!(uuid::<TimelineId>(48)));
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        let malformed = Event::from_wire_fields(
            value!(uuid::<EventId>(49)),
            EventWireFields::new(
                root,
                layer,
                kind.event_kind_id(),
                Participants::from_wire_fields(Vec::new()),
                EventAttributes::from_wire_fields(Vec::new()),
                EventTime::Span {
                    start: WorldTime::from_nanoseconds(timeline, 10),
                    end: None,
                },
            ),
            value!(revision(2)),
        );
        let archive = archive_events(std::slice::from_ref(&malformed))?;
        let query = EventCandidateQuery::new(
            root,
            LayerSelection::BaseOnly,
            RecordedAsOf::from_published_revision(value!(revision(2))),
            EventTimeFilter::Any,
        );
        assert!(matches!(
            full_scan_event_candidates(
                EventHistory::new(
                    std::slice::from_ref(&malformed),
                    &[],
                    &[],
                    &archive,
                    std::slice::from_ref(&kind),
                ),
                &history_spaces,
                &layer_snapshot,
                &query,
            ),
            Err(EventProjectionError::Event(
                EventRecordError::RoleCardinalityViolation { .. }
            ))
        ));
        let malformed_time = Event::from_wire_fields(
            value!(uuid::<EventId>(50)),
            EventWireFields::new(
                root,
                layer,
                kind.event_kind_id(),
                Participants::from_wire_fields(participants(&[8])?),
                EventAttributes::from_wire_fields(attributes()?),
                EventTime::Span {
                    start: WorldTime::from_nanoseconds(timeline, 10),
                    end: None,
                },
            ),
            value!(revision(2)),
        );
        let time_archive = archive_events(std::slice::from_ref(&malformed_time))?;
        assert!(matches!(
            full_scan_event_candidates(
                EventHistory::new(
                    std::slice::from_ref(&malformed_time),
                    &[],
                    &[],
                    &time_archive,
                    std::slice::from_ref(&kind),
                ),
                &history_spaces,
                &layer_snapshot,
                &query,
            ),
            Err(EventProjectionError::Event(
                EventRecordError::EventTimeFormMismatch {
                    expected: EventTimeForm::InstantOnly
                }
            ))
        ));
        Ok(())
    }

    #[test]
    fn event_correction_adds_new_event_and_corrects_edge_but_keeps_original_active() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(5));
        let (layer_snapshot, _) = projection_layers()?;
        let history_spaces = value!(HistorySpaceCatalog::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?]));
        let timeline = Timeline::new(value!(uuid::<TimelineId>(37)));
        let kind = event_kind(EventTimeForm::InstantOnly, 1, Some(2), Revision::GENESIS)?;
        let original = value!(Event::new(
            value!(uuid::<EventId>(38)),
            draft(
                &kind,
                EventTime::Instant(WorldTime::from_nanoseconds(timeline, 10))
            )?,
            value!(revision(2)),
        ));
        let (replacement, corrects, correction) = prepare_event_correction(
            &original,
            value!(uuid::<EventId>(39)),
            draft(
                &kind,
                EventTime::Instant(WorldTime::from_nanoseconds(timeline, 10)),
            )?,
            value!(uuid::<ProvenanceId>(40)),
            value!(revision(3)),
            |old_kind, new_kind| old_kind == new_kind,
        )?;
        assert_eq!(correction.replacement_id(), replacement.id());
        assert_eq!(replacement.event_time(), original.event_time());
        assert_eq!(
            corrects.from(),
            crate::ProvenanceEndpointRef::Event(replacement.id())
        );
        assert_eq!(
            corrects.to(),
            crate::ProvenanceEndpointRef::Event(original.id())
        );
        assert_eq!(corrects.relation(), crate::ProvenanceRelation::Corrects);
        assert_eq!(corrects.created_revision(), replacement.created_revision());

        let events = vec![original.clone(), replacement.clone()];
        let archive = archive_events(&events)?;
        let query = EventCandidateQuery::new(
            root,
            LayerSelection::BaseOnly,
            RecordedAsOf::from_published_revision(value!(revision(3))),
            EventTimeFilter::Any,
        );
        let visible = full_scan_event_candidates(
            EventHistory::new(&events, &[], &[], &archive, std::slice::from_ref(&kind)),
            &history_spaces,
            &layer_snapshot,
            &query,
        )?;
        assert_eq!(
            visible
                .iter()
                .map(|candidate| candidate.event().id())
                .collect::<Vec<_>>(),
            vec![original.id(), replacement.id()]
        );
        assert!(matches!(
            prepare_event_correction(
                &original,
                value!(uuid::<EventId>(65)),
                draft(
                    &kind,
                    EventTime::Instant(WorldTime::from_nanoseconds(timeline, 10)),
                )?,
                value!(uuid::<ProvenanceId>(66)),
                value!(revision(3)),
                |_, _| false,
            ),
            Err(EventProjectionError::IncompatibleCorrectionEventKind { .. })
        ));
        Ok(())
    }
}
