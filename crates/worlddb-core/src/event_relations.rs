//! Canonical immutable EventRelation records and transaction-time retractions.

use std::fmt;

use crate::ids::{EventId, EventRelationId, EventRelationRetractionId, Revision};

/// A persisted EventRelation kind.
///
/// `After` is accepted only as [`EventRelationInputKind::After`] and is stored
/// as the inverse `Before` edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EventRelationKind {
    /// The source Event precedes the destination Event.
    Before,
    /// Both Events belong to one explicit same-time equivalence relation.
    SameTime,
    /// The source Event causes the destination Event.
    Causes,
}

/// An API/import spelling that is normalized before constructing a record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventRelationInputKind {
    /// The source Event precedes the destination Event.
    Before,
    /// Alias for `Before(to_event, from_event)`; never persisted.
    After,
    /// Symmetric same-time input; endpoint order is canonicalized.
    SameTime,
    /// A directed causal relation, independent from temporal order.
    Causes,
}

/// The canonical logical key of a relation, excluding record identity/revision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventRelationKey {
    kind: EventRelationKind,
    from_event: EventId,
    to_event: EventId,
}

impl EventRelationKey {
    /// Returns the persisted canonical relation kind.
    #[must_use]
    pub const fn kind(self) -> EventRelationKind {
        self.kind
    }

    /// Returns the canonical source endpoint.
    #[must_use]
    pub const fn from_event(self) -> EventId {
        self.from_event
    }

    /// Returns the canonical destination endpoint.
    #[must_use]
    pub const fn to_event(self) -> EventId {
        self.to_event
    }
}

/// One immutable canonical EventRelation record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EventRelation {
    id: EventRelationId,
    key: EventRelationKey,
    created_revision: Revision,
}

impl EventRelation {
    /// Creates a relation after normalizing aliases and rejecting self-edges.
    pub fn new(
        id: EventRelationId,
        from_event: EventId,
        to_event: EventId,
        input_kind: EventRelationInputKind,
        created_revision: Revision,
    ) -> Result<Self, EventRelationError> {
        if from_event == to_event {
            return Err(EventRelationError::SelfRelation {
                event_id: from_event,
            });
        }

        let key = match input_kind {
            EventRelationInputKind::Before => EventRelationKey {
                kind: EventRelationKind::Before,
                from_event,
                to_event,
            },
            EventRelationInputKind::After => EventRelationKey {
                kind: EventRelationKind::Before,
                from_event: to_event,
                to_event: from_event,
            },
            EventRelationInputKind::SameTime => {
                let (from_event, to_event) = if from_event < to_event {
                    (from_event, to_event)
                } else {
                    (to_event, from_event)
                };
                EventRelationKey {
                    kind: EventRelationKind::SameTime,
                    from_event,
                    to_event,
                }
            }
            EventRelationInputKind::Causes => EventRelationKey {
                kind: EventRelationKind::Causes,
                from_event,
                to_event,
            },
        };

        Ok(Self {
            id,
            key,
            created_revision,
        })
    }

    /// Returns the stable EventRelation identity.
    #[must_use]
    pub const fn id(self) -> EventRelationId {
        self.id
    }

    /// Returns the stored canonical relation kind; it can never be `After`.
    #[must_use]
    pub const fn kind(self) -> EventRelationKind {
        self.key.kind()
    }

    /// Returns the canonical source endpoint.
    #[must_use]
    pub const fn from_event(self) -> EventId {
        self.key.from_event()
    }

    /// Returns the canonical destination endpoint.
    #[must_use]
    pub const fn to_event(self) -> EventId {
        self.key.to_event()
    }

    /// Returns the logical key used for normalized equality and duplicate checks.
    #[must_use]
    pub const fn key(self) -> EventRelationKey {
        self.key
    }

    /// Returns the Transaction-Time revision that created this relation.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// A canonical batch of new relations with no normalized duplicate keys.
///
/// This validates duplicates within the proposed batch. Checking conflicts
/// against already active history requires the transaction's retraction and
/// graph snapshot and belongs to the later engine validation path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRelationBatch(Vec<EventRelation>);

impl EventRelationBatch {
    /// Sorts by logical key and rejects duplicate canonical relations.
    pub fn new(mut relations: Vec<EventRelation>) -> Result<Self, EventRelationError> {
        relations.sort_by_key(|relation| relation.key());
        for pair in relations.windows(2) {
            if let [left, right] = pair {
                if left.key() == right.key() {
                    return Err(EventRelationError::DuplicateRelation { key: left.key() });
                }
            }
        }
        Ok(Self(relations))
    }

    /// Returns relations in canonical key order.
    #[must_use]
    pub fn as_slice(&self) -> &[EventRelation] {
        &self.0
    }
}

/// One immutable Transaction-Time retraction of an EventRelation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EventRelationRetraction {
    id: EventRelationRetractionId,
    event_relation_id: EventRelationId,
    reason: String,
    created_revision: Revision,
}

impl EventRelationRetraction {
    /// Creates a retraction strictly after its EventRelation.
    pub fn new(
        id: EventRelationRetractionId,
        event_relation: &EventRelation,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, EventRelationError> {
        if created_revision <= event_relation.created_revision() {
            return Err(EventRelationError::LifecycleRevisionNotAfterTarget {
                target_revision: event_relation.created_revision(),
                lifecycle_revision: created_revision,
            });
        }
        Ok(Self {
            id,
            event_relation_id: event_relation.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: EventRelationRetractionId,
        event_relation_id: EventRelationId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            event_relation_id,
            reason,
            created_revision,
        }
    }

    /// Returns the concrete retraction identity.
    #[must_use]
    pub const fn id(&self) -> EventRelationRetractionId {
        self.id
    }

    /// Returns the target EventRelation identity.
    #[must_use]
    pub const fn event_relation_id(&self) -> EventRelationId {
        self.event_relation_id
    }

    /// Returns the required reason for retracting this relation.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time revision at which retraction applies.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// A rejected EventRelation shape or lifecycle value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventRelationError {
    /// An EventRelation cannot refer from an Event to itself.
    SelfRelation {
        /// The repeated Event identity.
        event_id: EventId,
    },
    /// A batch contains the same normalized logical edge more than once.
    DuplicateRelation {
        /// The duplicated canonical logical key.
        key: EventRelationKey,
    },
    /// A lifecycle record must be later than its target on Transaction Time.
    LifecycleRevisionNotAfterTarget {
        /// The target EventRelation revision.
        target_revision: Revision,
        /// The proposed retraction revision.
        lifecycle_revision: Revision,
    },
}

impl fmt::Display for EventRelationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfRelation { event_id } => {
                write!(
                    formatter,
                    "EventRelation cannot target Event {event_id} from itself"
                )
            }
            Self::DuplicateRelation { key } => write!(
                formatter,
                "duplicate EventRelation {:?}({},{})",
                key.kind(),
                key.from_event(),
                key.to_event()
            ),
            Self::LifecycleRevisionNotAfterTarget {
                target_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "EventRelationRetraction revision {lifecycle_revision} must be after target revision {target_revision}"
            ),
        }
    }
}

impl std::error::Error for EventRelationError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        EventRelation, EventRelationBatch, EventRelationError, EventRelationInputKind,
        EventRelationKind, EventRelationRetraction,
    };
    use crate::ids::{
        DomainId, EventId, EventRelationId, EventRelationRetractionId, IdValidationError, Revision,
        RevisionError,
    };

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Relation(EventRelationError),
        Id(IdValidationError),
        Revision(RevisionError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Relation(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
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

    error_conversion!(EventRelationError, Relation);
    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);

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

    fn relation(
        id: u8,
        from: u8,
        to: u8,
        kind: EventRelationInputKind,
    ) -> Result<EventRelation, TestError> {
        Ok(value!(EventRelation::new(
            value!(uuid::<EventRelationId>(id)),
            value!(uuid::<EventId>(from)),
            value!(uuid::<EventId>(to)),
            kind,
            value!(revision(3)),
        )))
    }

    #[test]
    fn after_is_stored_as_the_inverse_before_edge() -> TestResult {
        let after = relation(1, 8, 9, EventRelationInputKind::After)?;
        let before = relation(1, 9, 8, EventRelationInputKind::Before)?;
        assert_eq!(after, before);
        assert_eq!(after.kind(), EventRelationKind::Before);
        assert_eq!(after.from_event(), value!(uuid::<EventId>(9)));
        assert_eq!(after.to_event(), value!(uuid::<EventId>(8)));
        Ok(())
    }

    #[test]
    fn same_time_is_symmetric_and_canonically_ordered() -> TestResult {
        let forward = relation(2, 8, 9, EventRelationInputKind::SameTime)?;
        let reverse = relation(2, 9, 8, EventRelationInputKind::SameTime)?;
        assert_eq!(forward, reverse);
        assert_eq!(forward.kind(), EventRelationKind::SameTime);
        assert!(forward.from_event() < forward.to_event());
        Ok(())
    }

    #[test]
    fn self_relations_are_rejected_for_every_input_kind() -> TestResult {
        let event_id = value!(uuid::<EventId>(10));
        for kind in [
            EventRelationInputKind::Before,
            EventRelationInputKind::After,
            EventRelationInputKind::SameTime,
            EventRelationInputKind::Causes,
        ] {
            assert_eq!(
                EventRelation::new(
                    value!(uuid::<EventRelationId>(3)),
                    event_id,
                    event_id,
                    kind,
                    Revision::GENESIS,
                )
                .err(),
                Some(EventRelationError::SelfRelation { event_id })
            );
        }
        Ok(())
    }

    #[test]
    fn candidate_batch_rejects_normalized_before_after_and_sametime_duplicates() -> TestResult {
        let before = relation(4, 8, 9, EventRelationInputKind::Before)?;
        let after = relation(5, 9, 8, EventRelationInputKind::After)?;
        assert_eq!(
            EventRelationBatch::new(vec![before, after]).err(),
            Some(EventRelationError::DuplicateRelation { key: before.key() })
        );

        let same_time = relation(6, 8, 9, EventRelationInputKind::SameTime)?;
        let inverse_same_time = relation(7, 9, 8, EventRelationInputKind::SameTime)?;
        assert_eq!(
            EventRelationBatch::new(vec![same_time, inverse_same_time]).err(),
            Some(EventRelationError::DuplicateRelation {
                key: same_time.key(),
            })
        );
        Ok(())
    }

    #[test]
    fn candidate_batch_is_ordered_by_canonical_relation_key() -> TestResult {
        let causes = relation(8, 9, 10, EventRelationInputKind::Causes)?;
        let before = relation(9, 8, 10, EventRelationInputKind::Before)?;
        let batch = value!(EventRelationBatch::new(vec![causes, before]));
        assert_eq!(batch.as_slice(), &[before, causes]);
        Ok(())
    }

    #[test]
    fn relation_retraction_is_a_separate_later_lifecycle_record() -> TestResult {
        let relation = relation(11, 8, 9, EventRelationInputKind::Causes)?;
        let retraction = value!(EventRelationRetraction::new(
            value!(uuid::<EventRelationRetractionId>(12)),
            &relation,
            "relation correction",
            value!(revision(4)),
        ));
        assert_eq!(retraction.event_relation_id(), relation.id());
        assert_eq!(retraction.reason(), "relation correction");
        assert_eq!(
            EventRelationRetraction::new(
                value!(uuid::<EventRelationRetractionId>(13)),
                &relation,
                "too early",
                value!(revision(3)),
            )
            .err(),
            Some(EventRelationError::LifecycleRevisionNotAfterTarget {
                target_revision: value!(revision(3)),
                lifecycle_revision: value!(revision(3)),
            })
        );
        Ok(())
    }
}
