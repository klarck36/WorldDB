//! Index-free reference model for project-wide schema history.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{
    EntityTypeId, EventKindId, LayerId, PredicateId, Revision, SchemaRevision, TimelineId,
};
use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
use crate::schema::{
    EntityTypeDefinition, EventKindDefinition, Lifecycle, PredicateDefinition, TimeUnitDefinition,
    TimelineDefinition,
};
use crate::temporal::{Timeline, WorldTime};
use crate::values::{Symbol, Time};
use crate::wire_records::{Record, RecordCodecError, encode_record};

/// Requested interpretation of the project-wide schema history.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SchemaMode {
    /// Use the schema effective at the query's `RecordedAsOf` revision.
    Historical,
    /// Use the schema at the model's latest published shared revision.
    Current,
    /// Use the exact explicitly selected schema revision.
    Explicit(SchemaRevision),
}

impl Default for SchemaMode {
    fn default() -> Self {
        Self::Historical
    }
}

/// One closed record family admitted to the schema reference model.
#[derive(Clone, Debug)]
pub enum SchemaDefinition {
    /// One revision of a layer definition.
    Layer(LayerDefinition),
    /// One complete layer state and explicit base layer.
    LayerSnapshot(LayerSchemaSnapshot),
    /// One revision of an EntityType definition.
    EntityType(EntityTypeDefinition),
    /// One revision of a Predicate definition.
    Predicate(PredicateDefinition),
    /// One revision of an EventKind definition.
    EventKind(EventKindDefinition),
    /// One revision of a project-wide Timeline definition.
    Timeline(TimelineDefinition),
    /// One revision of a project-wide TimeUnit definition.
    TimeUnit(TimeUnitDefinition),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SchemaDefinitionKey {
    Layer(LayerId),
    LayerSnapshot,
    EntityType(EntityTypeId),
    Predicate(PredicateId),
    EventKind(EventKindId),
    Timeline(TimelineId),
    TimeUnit(Symbol),
}

impl SchemaDefinition {
    fn key(&self) -> SchemaDefinitionKey {
        match self {
            Self::Layer(value) => SchemaDefinitionKey::Layer(value.layer_id()),
            Self::LayerSnapshot(_) => SchemaDefinitionKey::LayerSnapshot,
            Self::EntityType(value) => SchemaDefinitionKey::EntityType(value.entity_type_id()),
            Self::Predicate(value) => SchemaDefinitionKey::Predicate(value.predicate_id()),
            Self::EventKind(value) => SchemaDefinitionKey::EventKind(value.event_kind_id()),
            Self::Timeline(value) => SchemaDefinitionKey::Timeline(value.timeline_id()),
            Self::TimeUnit(value) => SchemaDefinitionKey::TimeUnit(value.symbol().clone()),
        }
    }

    fn created_revision(&self) -> Revision {
        match self {
            Self::Layer(value) => value.created_revision().revision(),
            Self::LayerSnapshot(value) => value.revision().revision(),
            Self::EntityType(value) => value.created_revision(),
            Self::Predicate(value) => value.created_revision(),
            Self::EventKind(value) => value.created_revision(),
            Self::Timeline(value) => value.created_revision(),
            Self::TimeUnit(value) => value.created_revision(),
        }
    }

    fn encode(&self) -> Result<Vec<u8>, RecordCodecError> {
        let record = match self {
            Self::Layer(value) => Record::LayerDefinition(value.clone()),
            Self::LayerSnapshot(value) => Record::LayerSchemaSnapshot(value.clone()),
            Self::EntityType(value) => Record::EntityTypeDefinition(value.clone()),
            Self::Predicate(value) => Record::PredicateDefinition(value.clone()),
            Self::EventKind(value) => Record::EventKindDefinition(value.clone()),
            Self::Timeline(value) => Record::TimelineDefinition(value.clone()),
            Self::TimeUnit(value) => Record::TimeUnitDefinition(value.clone()),
        };
        encode_record(&record)
    }

    fn symbol_key(&self) -> Option<(u8, &str)> {
        match self {
            Self::Layer(value) => Some((0, value.symbol().as_str())),
            Self::EntityType(value) => Some((1, value.symbol().as_str())),
            Self::Predicate(value) => Some((2, value.symbol().as_str())),
            Self::EventKind(value) => Some((3, value.symbol().as_str())),
            Self::Timeline(value) => Some((4, value.symbol().as_str())),
            Self::TimeUnit(value) => Some((5, value.symbol().as_str())),
            Self::LayerSnapshot(_) => None,
        }
    }
}

/// Immutable, canonical materialized view of project schema at one revision.
#[derive(Clone, Debug)]
pub struct SchemaSnapshot {
    schema_revision: SchemaRevision,
    definitions: Vec<SchemaDefinition>,
    fingerprint: [u8; 32],
}

impl SchemaSnapshot {
    fn new(
        schema_revision: SchemaRevision,
        definitions: Vec<SchemaDefinition>,
    ) -> Result<Self, SchemaHistoryError> {
        let mut symbols = BTreeSet::new();
        for definition in &definitions {
            if let Some((family, symbol)) = definition.symbol_key() {
                if !symbols.insert((family, symbol.to_owned())) {
                    return Err(SchemaHistoryError::DuplicateSymbol {
                        family,
                        symbol: symbol.to_owned(),
                    });
                }
            }
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"WorldDB.SchemaSnapshot.v1\0");
        for definition in &definitions {
            let encoded = definition.encode().map_err(SchemaHistoryError::Codec)?;
            let length = u64::try_from(encoded.len())
                .map_err(|_| SchemaHistoryError::FingerprintInputTooLarge)?;
            hasher.update(&length.to_be_bytes());
            hasher.update(&encoded);
        }
        Ok(Self {
            schema_revision,
            definitions,
            fingerprint: *hasher.finalize().as_bytes(),
        })
    }

    /// Returns the shared database revision represented by this view.
    #[must_use]
    pub const fn schema_revision(&self) -> SchemaRevision {
        self.schema_revision
    }

    /// Returns the stable BLAKE3 fingerprint of the canonical encoded definitions.
    #[must_use]
    pub const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Returns the canonical definitions in stable family-and-ID order.
    #[must_use]
    pub fn definitions(&self) -> &[SchemaDefinition] {
        &self.definitions
    }

    /// Returns the Timeline revision effective in this snapshot, if registered.
    #[must_use]
    pub fn timeline(&self, id: TimelineId) -> Option<&TimelineDefinition> {
        self.definitions
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::Timeline(value) if value.timeline_id() == id => Some(value),
                _ => None,
            })
    }

    /// Returns the TimeUnit revision effective in this snapshot, if registered.
    #[must_use]
    pub fn time_unit(&self, symbol: &Symbol) -> Option<&TimeUnitDefinition> {
        self.definitions
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::TimeUnit(value) if value.symbol() == symbol => Some(value),
                _ => None,
            })
    }

    /// Resolves a Time representation through this exact schema snapshot.
    ///
    /// The returned coordinate is the checked `ticks * nanoseconds_per_tick`
    /// value on the same Timeline. Deprecated definitions are rejected unless
    /// the caller has already applied the explicit deprecated-use policy.
    pub fn resolve_time(
        &self,
        value: &Time,
        allow_deprecated: bool,
    ) -> Result<WorldTime, TimeResolutionError> {
        let timeline = self
            .timeline(value.timeline_id())
            .ok_or(TimeResolutionError::UnknownTimeline(value.timeline_id()))?;
        match timeline.lifecycle() {
            Lifecycle::Active => {}
            Lifecycle::Deprecated if allow_deprecated => {}
            Lifecycle::Deprecated => {
                return Err(TimeResolutionError::DeprecatedTimeline(value.timeline_id()));
            }
            Lifecycle::Retired => {
                return Err(TimeResolutionError::RetiredTimeline(value.timeline_id()));
            }
        }

        let unit = self
            .time_unit(value.unit())
            .ok_or_else(|| TimeResolutionError::UnknownTimeUnit(value.unit().clone()))?;
        match unit.lifecycle() {
            Lifecycle::Active => {}
            Lifecycle::Deprecated if allow_deprecated => {}
            Lifecycle::Deprecated => {
                return Err(TimeResolutionError::DeprecatedTimeUnit(
                    value.unit().clone(),
                ));
            }
            Lifecycle::Retired => {
                return Err(TimeResolutionError::RetiredTimeUnit(value.unit().clone()));
            }
        }

        let nanoseconds = value
            .ticks()
            .checked_mul(i128::from(unit.nanoseconds_per_tick().get()))
            .ok_or(TimeResolutionError::Overflow)?;
        Ok(WorldTime::from_nanoseconds(
            Timeline::new(value.timeline_id()),
            nanoseconds,
        ))
    }

    /// Fails closed when a persisted/cached fingerprint does not match this view.
    pub fn verify_fingerprint(&self, expected: [u8; 32]) -> Result<(), SchemaHistoryError> {
        if self.fingerprint != expected {
            return Err(SchemaHistoryError::SchemaFingerprintMismatch {
                schema_revision: self.schema_revision.revision(),
            });
        }
        Ok(())
    }
}

/// Failure while resolving one stored Time through a selected schema snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimeResolutionError {
    /// The Time names no registered Timeline.
    UnknownTimeline(TimelineId),
    /// The Time names a retired Timeline.
    RetiredTimeline(TimelineId),
    /// The Time names a deprecated Timeline without the explicit opt-in path.
    DeprecatedTimeline(TimelineId),
    /// The Time names no registered TimeUnit symbol.
    UnknownTimeUnit(Symbol),
    /// The Time names a retired TimeUnit.
    RetiredTimeUnit(Symbol),
    /// The Time names a deprecated TimeUnit without the explicit opt-in path.
    DeprecatedTimeUnit(Symbol),
    /// Tick multiplication exceeded signed i128 nanoseconds.
    Overflow,
}

impl fmt::Display for TimeResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTimeline(id) => write!(formatter, "unknown Timeline {id}"),
            Self::RetiredTimeline(id) => write!(formatter, "Timeline {id} is retired"),
            Self::DeprecatedTimeline(id) => {
                write!(formatter, "Timeline {id} requires deprecated-use opt-in")
            }
            Self::UnknownTimeUnit(symbol) => write!(formatter, "unknown TimeUnit {symbol}"),
            Self::RetiredTimeUnit(symbol) => write!(formatter, "TimeUnit {symbol} is retired"),
            Self::DeprecatedTimeUnit(symbol) => {
                write!(
                    formatter,
                    "TimeUnit {symbol} requires deprecated-use opt-in"
                )
            }
            Self::Overflow => formatter.write_str("Time nanosecond normalization overflowed"),
        }
    }
}

impl std::error::Error for TimeResolutionError {}

/// Index-free immutable schema history over the shared Revision axis.
pub struct SchemaHistoryReferenceModel {
    latest_published: Revision,
    history: BTreeMap<Revision, Vec<SchemaDefinition>>,
    corrupt_revisions: BTreeSet<Revision>,
}

impl SchemaHistoryReferenceModel {
    /// Creates an empty project schema history at Genesis.
    #[must_use]
    pub fn new() -> Self {
        Self {
            latest_published: Revision::GENESIS,
            history: BTreeMap::new(),
            corrupt_revisions: BTreeSet::new(),
        }
    }

    /// Creates a reference model with definitions that are part of the immutable Genesis schema.
    ///
    /// Genesis definitions establish the initial effective schema and are not
    /// published as a later revision. Subsequent batches still use
    /// [`Self::publish`] and must advance the shared revision strictly.
    pub fn with_genesis(definitions: Vec<SchemaDefinition>) -> Result<Self, SchemaHistoryError> {
        let mut batch_keys = BTreeSet::new();
        for definition in &definitions {
            if definition.created_revision() != Revision::GENESIS {
                return Err(SchemaHistoryError::DefinitionRevisionMismatch {
                    expected: Revision::GENESIS,
                    actual: definition.created_revision(),
                });
            }
            match definition {
                SchemaDefinition::Timeline(value) if value.lifecycle() != Lifecycle::Active => {
                    return Err(SchemaHistoryError::InvalidTimeRegistryLifecycle);
                }
                SchemaDefinition::TimeUnit(value) if value.lifecycle() != Lifecycle::Active => {
                    return Err(SchemaHistoryError::InvalidTimeRegistryLifecycle);
                }
                _ => {}
            }
            if !batch_keys.insert(definition.key()) {
                return Err(SchemaHistoryError::DuplicateDefinition {
                    revision: Revision::GENESIS,
                });
            }
        }
        let history = BTreeMap::from([(Revision::GENESIS, definitions)]);
        let _ = materialize(&history, Revision::GENESIS)?;
        Ok(Self {
            latest_published: Revision::GENESIS,
            history,
            corrupt_revisions: BTreeSet::new(),
        })
    }

    /// Returns the latest shared revision known to this reference model.
    #[must_use]
    pub const fn latest_published(&self) -> Revision {
        self.latest_published
    }

    /// Advances the shared revision when a committed batch contains no schema changes.
    ///
    /// This keeps `Current` schema snapshots aligned with the database head while
    /// preserving the last effective definitions and all earlier historical views.
    pub fn advance_to(&mut self, revision: Revision) -> Result<(), SchemaHistoryError> {
        if revision <= self.latest_published {
            return Err(SchemaHistoryError::RevisionNotIncreasing {
                previous: self.latest_published,
                requested: revision,
            });
        }
        self.latest_published = revision;
        Ok(())
    }

    /// Publishes one immutable schema batch at a later shared revision.
    pub fn publish(
        &mut self,
        revision: Revision,
        definitions: Vec<SchemaDefinition>,
    ) -> Result<(), SchemaHistoryError> {
        if revision <= self.latest_published {
            return Err(SchemaHistoryError::RevisionNotIncreasing {
                previous: self.latest_published,
                requested: revision,
            });
        }
        let mut batch_keys = BTreeSet::new();
        for definition in &definitions {
            if definition.created_revision() != revision {
                return Err(SchemaHistoryError::DefinitionRevisionMismatch {
                    expected: revision,
                    actual: definition.created_revision(),
                });
            }
            if !batch_keys.insert(definition.key()) {
                return Err(SchemaHistoryError::DuplicateDefinition { revision });
            }
        }
        let previous = materialize(&self.history, self.latest_published)?;
        validate_time_registry_transitions(&previous, &definitions, revision)?;
        let mut staged = self.history.clone();
        staged.insert(revision, definitions);
        let latest_snapshot = materialize(&staged, revision)?;
        let _ = latest_snapshot;
        self.history = staged;
        self.latest_published = revision;
        Ok(())
    }

    /// Resolves Historical, Current, or Explicit schema without fallback.
    pub fn schema_at(
        &self,
        mode: SchemaMode,
        recorded_as_of: Revision,
    ) -> Result<SchemaSnapshot, SchemaHistoryError> {
        let requested = match mode {
            SchemaMode::Historical => recorded_as_of,
            SchemaMode::Current => self.latest_published,
            SchemaMode::Explicit(revision) => revision.revision(),
        };
        if requested > self.latest_published {
            return Err(SchemaHistoryError::RevisionNotPublished {
                requested,
                published: self.latest_published,
            });
        }
        if let Some(corrupt_revision) = self.corrupt_revisions.range(..=requested).next().copied() {
            return Err(SchemaHistoryError::HistoricalSchemaCorrupt {
                requested,
                corrupt_revision,
            });
        }
        materialize(&self.history, requested)
    }

    #[cfg(test)]
    fn mark_corrupt(&mut self, revision: Revision) {
        self.corrupt_revisions.insert(revision);
    }
}

impl Default for SchemaHistoryReferenceModel {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_time_registry_transitions(
    previous: &SchemaSnapshot,
    definitions: &[SchemaDefinition],
    revision: Revision,
) -> Result<(), SchemaHistoryError> {
    for next in definitions {
        match next {
            SchemaDefinition::Timeline(next) => {
                if let Some(prior) = previous.timeline(next.timeline_id()) {
                    let expected = prior
                        .revise_lifecycle(next.lifecycle(), revision)
                        .map_err(|_| SchemaHistoryError::InvalidTimeRegistryLifecycle)?;
                    if prior.lifecycle() == next.lifecycle() || expected != *next {
                        return Err(SchemaHistoryError::ImmutableTimelineMapping(
                            next.timeline_id(),
                        ));
                    }
                } else if next.lifecycle() != Lifecycle::Active {
                    return Err(SchemaHistoryError::InvalidTimeRegistryLifecycle);
                }
            }
            SchemaDefinition::TimeUnit(next) => {
                if let Some(prior) = previous.time_unit(next.symbol()) {
                    let expected = prior
                        .revise_lifecycle(next.lifecycle(), revision)
                        .map_err(|_| SchemaHistoryError::InvalidTimeRegistryLifecycle)?;
                    if prior.lifecycle() == next.lifecycle() || expected != *next {
                        return Err(SchemaHistoryError::ImmutableTimeUnitScale(
                            next.symbol().clone(),
                        ));
                    }
                } else if next.lifecycle() != Lifecycle::Active {
                    return Err(SchemaHistoryError::InvalidTimeRegistryLifecycle);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn materialize(
    history: &BTreeMap<Revision, Vec<SchemaDefinition>>,
    requested: Revision,
) -> Result<SchemaSnapshot, SchemaHistoryError> {
    let mut effective = BTreeMap::<SchemaDefinitionKey, SchemaDefinition>::new();
    for (revision, definitions) in history.range(..=requested) {
        for definition in definitions {
            if definition.created_revision() != *revision {
                return Err(SchemaHistoryError::HistoricalSchemaCorrupt {
                    requested,
                    corrupt_revision: *revision,
                });
            }
            effective.insert(definition.key(), definition.clone());
        }
    }
    SchemaSnapshot::new(
        SchemaRevision::from_published_revision(requested),
        effective.into_values().collect(),
    )
}

/// Invalid schema publication, historical materialization, or fingerprint verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaHistoryError {
    /// Schema records must use exactly the publication's shared revision.
    DefinitionRevisionMismatch {
        expected: Revision,
        actual: Revision,
    },
    /// One batch repeats the same typed schema definition identity.
    DuplicateDefinition { revision: Revision },
    /// A Timeline identity cannot change its symbol or calendar mapping.
    ImmutableTimelineMapping(TimelineId),
    /// A TimeUnit symbol cannot change its nanosecond scale.
    ImmutableTimeUnitScale(Symbol),
    /// A new time registry entry must be Active and lifecycle revisions must advance legally.
    InvalidTimeRegistryLifecycle,
    /// Two effective definitions in one schema family use one symbol.
    DuplicateSymbol { family: u8, symbol: String },
    /// The requested revision is later than the latest published shared revision.
    RevisionNotPublished {
        requested: Revision,
        published: Revision,
    },
    /// Schema history commits must use a strictly increasing shared revision.
    RevisionNotIncreasing {
        previous: Revision,
        requested: Revision,
    },
    /// A required historical schema entry is damaged; no fallback is permitted.
    HistoricalSchemaCorrupt {
        requested: Revision,
        corrupt_revision: Revision,
    },
    /// A persisted fingerprint disagrees with the canonical snapshot.
    SchemaFingerprintMismatch { schema_revision: Revision },
    /// A canonical schema record could not be encoded for fingerprinting.
    Codec(RecordCodecError),
    /// The canonical fingerprint input length cannot be represented.
    FingerprintInputTooLarge,
}

impl fmt::Display for SchemaHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefinitionRevisionMismatch { expected, actual } => write!(
                formatter,
                "schema definition uses revision {actual}; publication is {expected}"
            ),
            Self::DuplicateDefinition { revision } => write!(
                formatter,
                "schema batch at {revision} repeats one definition identity"
            ),
            Self::ImmutableTimelineMapping(id) => {
                write!(
                    formatter,
                    "Timeline {id} identity and calendar mapping are immutable"
                )
            }
            Self::ImmutableTimeUnitScale(symbol) => {
                write!(formatter, "TimeUnit {symbol} scale is immutable")
            }
            Self::InvalidTimeRegistryLifecycle => {
                formatter.write_str("time registry lifecycle transition is invalid")
            }
            Self::DuplicateSymbol { family, symbol } => {
                write!(formatter, "schema family {family} repeats symbol {symbol}")
            }
            Self::RevisionNotPublished {
                requested,
                published,
            } => write!(
                formatter,
                "schema revision {requested} is later than published revision {published}"
            ),
            Self::RevisionNotIncreasing {
                previous,
                requested,
            } => write!(
                formatter,
                "schema revision {requested} does not follow {previous}"
            ),
            Self::HistoricalSchemaCorrupt {
                requested,
                corrupt_revision,
            } => write!(
                formatter,
                "historical schema at {requested} requires corrupt entry {corrupt_revision}"
            ),
            Self::SchemaFingerprintMismatch { schema_revision } => write!(
                formatter,
                "schema fingerprint does not match canonical snapshot at {schema_revision}"
            ),
            Self::Codec(error) => write!(formatter, "schema fingerprint encoding failed: {error}"),
            Self::FingerprintInputTooLarge => {
                formatter.write_str("schema fingerprint input is too large")
            }
        }
    }
}

impl std::error::Error for SchemaHistoryError {}

#[cfg(test)]
mod tests {
    use super::{
        SchemaDefinition, SchemaHistoryError, SchemaHistoryReferenceModel, SchemaMode,
        TimeResolutionError,
    };
    use crate::ids::{DomainId, EntityTypeId, PredicateId, Revision, TimelineId};
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, EntityTypeDefinition, Lifecycle,
        PredicateDefinition, PredicateDefinitionSpec, ResolutionPolicy, SchemaDefinitionError,
        TimeUnitDefinition, TimelineCalendarProfile, TimelineDefinition, ValueKind,
    };
    use crate::{Symbol, Time, WorldTime};

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn entity_type(id: EntityTypeId, symbol: &str, revision: Revision) -> Option<SchemaDefinition> {
        Some(SchemaDefinition::EntityType(EntityTypeDefinition::new(
            id,
            Symbol::new(symbol).ok()?,
            None,
            Lifecycle::Active,
            revision,
        )))
    }

    fn predicate(
        id: PredicateId,
        revision: Revision,
        resolution_policy: ResolutionPolicy,
    ) -> Option<SchemaDefinition> {
        PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: id,
            symbol: Symbol::new("items").ok()?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy,
            constraints: ConstraintSet::unconstrained(),
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: revision,
        })
        .ok()
        .map(SchemaDefinition::Predicate)
    }

    #[test]
    fn timeline_and_time_unit_records_resolve_exact_checked_nanoseconds() {
        let (Ok(timeline_id), Ok(second_unit), Ok(double_unit), Ok(project_clock)) = (
            id::<TimelineId>(51),
            Symbol::new("second"),
            Symbol::new("double"),
            Symbol::new("project_clock"),
        ) else {
            return;
        };
        let first = Revision::FIRST_COMMIT;
        let timeline = TimelineDefinition::new(
            timeline_id,
            project_clock,
            TimelineCalendarProfile::ProlepticGregorianUtc {
                epoch_unix_nanoseconds: 123,
            },
            Lifecycle::Active,
            first,
        );
        let (Ok(seconds), Ok(doubles)) = (
            TimeUnitDefinition::new(second_unit.clone(), 1_000_000_000, Lifecycle::Active, first),
            TimeUnitDefinition::new(double_unit.clone(), 2, Lifecycle::Active, first),
        ) else {
            return;
        };
        let mut history = SchemaHistoryReferenceModel::new();
        let published = history.publish(
            first,
            vec![
                SchemaDefinition::Timeline(timeline),
                SchemaDefinition::TimeUnit(seconds),
                SchemaDefinition::TimeUnit(doubles),
            ],
        );
        assert!(published.is_ok());
        let snapshot = history.schema_at(SchemaMode::Current, first);
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            let second_time = Time::new(timeline_id, -3, second_unit);
            let normalized = snapshot.resolve_time(&second_time, false);
            assert_eq!(normalized.map(WorldTime::nanoseconds), Ok(-3_000_000_000));

            let overflowing_time = Time::new(timeline_id, i128::MAX, double_unit);
            assert_eq!(
                snapshot.resolve_time(&overflowing_time, false).err(),
                Some(TimeResolutionError::Overflow)
            );
        }
    }

    #[test]
    fn time_registry_mappings_are_immutable_and_lifecycle_is_monotone() {
        let (Ok(timeline_id), Ok(clock), Ok(unit)) = (
            id::<TimelineId>(52),
            Symbol::new("clock"),
            Symbol::new("tick"),
        ) else {
            return;
        };
        let first = Revision::FIRST_COMMIT;
        let Ok(revision) = first.next_commit() else {
            return;
        };
        let Ok(third_revision) = revision.next_commit() else {
            return;
        };
        let timeline = TimelineDefinition::new(
            timeline_id,
            clock.clone(),
            TimelineCalendarProfile::None,
            Lifecycle::Active,
            first,
        );
        let Ok(active_unit) = TimeUnitDefinition::new(unit.clone(), 10, Lifecycle::Active, first)
        else {
            return;
        };
        let mut history = SchemaHistoryReferenceModel::new();
        assert!(
            history
                .publish(
                    first,
                    vec![
                        SchemaDefinition::Timeline(timeline.clone()),
                        SchemaDefinition::TimeUnit(active_unit.clone()),
                    ],
                )
                .is_ok()
        );

        let changed_timeline = TimelineDefinition::new(
            timeline_id,
            clock,
            TimelineCalendarProfile::ProlepticGregorianUtc {
                epoch_unix_nanoseconds: 1,
            },
            Lifecycle::Deprecated,
            revision,
        );
        assert!(matches!(
            history.publish(revision, vec![SchemaDefinition::Timeline(changed_timeline)]),
            Err(SchemaHistoryError::ImmutableTimelineMapping(id)) if id == timeline_id
        ));

        let changed_unit =
            TimeUnitDefinition::new(unit.clone(), 11, Lifecycle::Deprecated, revision);
        assert!(changed_unit.is_ok());
        if let Ok(changed_unit) = changed_unit {
            assert!(matches!(
                history.publish(revision, vec![SchemaDefinition::TimeUnit(changed_unit)]),
                Err(SchemaHistoryError::ImmutableTimeUnitScale(symbol)) if symbol == unit
            ));
        }

        let deprecated_unit = active_unit.revise_lifecycle(Lifecycle::Deprecated, revision);
        assert!(deprecated_unit.is_ok());
        if let Ok(deprecated_unit) = deprecated_unit {
            let changed = history.publish(
                revision,
                vec![SchemaDefinition::TimeUnit(deprecated_unit.clone())],
            );
            assert!(changed.is_ok());
            let snapshot = history.schema_at(SchemaMode::Current, revision);
            assert!(snapshot.is_ok());
            if let Ok(snapshot) = snapshot {
                let value = Time::new(timeline_id, 3, unit);
                assert!(matches!(
                    snapshot.resolve_time(&value, false),
                    Err(TimeResolutionError::DeprecatedTimeUnit(_))
                ));
                assert_eq!(
                    snapshot
                        .resolve_time(&value, true)
                        .map(WorldTime::nanoseconds),
                    Ok(30)
                );
            }
            assert_eq!(
                deprecated_unit
                    .revise_lifecycle(Lifecycle::Active, third_revision)
                    .err(),
                Some(SchemaDefinitionError::LifecycleRegression)
            );
        }
    }

    #[test]
    fn historical_predicate_resolution_policy_is_preserved_by_schema_revision() {
        let (Ok(predicate_id), Ok(second_revision)) = (id(4), Revision::FIRST_COMMIT.next_commit())
        else {
            return;
        };
        let (Some(first), Some(second)) = (
            predicate(
                predicate_id,
                Revision::FIRST_COMMIT,
                ResolutionPolicy::MultiValueOverlay,
            ),
            predicate(
                predicate_id,
                second_revision,
                ResolutionPolicy::MultiValueReplace,
            ),
        ) else {
            return;
        };
        let mut model = SchemaHistoryReferenceModel::new();
        assert!(model.publish(Revision::FIRST_COMMIT, vec![first]).is_ok());
        assert!(model.publish(second_revision, vec![second]).is_ok());
        let historical = model.schema_at(SchemaMode::Historical, Revision::FIRST_COMMIT);
        let current = model.schema_at(SchemaMode::Current, Revision::FIRST_COMMIT);
        let policy = |snapshot: &super::SchemaSnapshot| {
            snapshot
                .definitions()
                .iter()
                .find_map(|definition| match definition {
                    SchemaDefinition::Predicate(value) => Some(value.resolution_policy()),
                    _ => None,
                })
        };
        assert_eq!(
            historical.ok().as_ref().and_then(policy),
            Some(ResolutionPolicy::MultiValueOverlay)
        );
        assert_eq!(
            current.ok().as_ref().and_then(policy),
            Some(ResolutionPolicy::MultiValueReplace)
        );
    }

    #[test]
    fn historical_current_and_explicit_modes_select_one_shared_history() {
        let (Ok(first_id), Ok(second_id), Ok(second_revision)) =
            (id(1), id(2), Revision::FIRST_COMMIT.next_commit())
        else {
            return;
        };
        let (Some(first), Some(second)) = (
            entity_type(first_id, "person", Revision::FIRST_COMMIT),
            entity_type(second_id, "place", second_revision),
        ) else {
            return;
        };
        let mut model = SchemaHistoryReferenceModel::new();
        assert!(model.publish(Revision::FIRST_COMMIT, vec![first]).is_ok());
        assert!(model.publish(second_revision, vec![second]).is_ok());
        let historical = model.schema_at(SchemaMode::Historical, Revision::FIRST_COMMIT);
        let current = model.schema_at(SchemaMode::Current, Revision::FIRST_COMMIT);
        let explicit = model.schema_at(
            SchemaMode::Explicit(crate::SchemaRevision::from_published_revision(
                Revision::FIRST_COMMIT,
            )),
            second_revision,
        );
        assert!(historical.is_ok());
        assert!(current.is_ok());
        assert!(explicit.is_ok());
        assert_eq!(
            historical.ok().map(|value| value.definitions().len()),
            Some(1)
        );
        assert_eq!(current.ok().map(|value| value.definitions().len()), Some(2));
        assert_eq!(
            explicit.ok().map(|value| value.definitions().len()),
            Some(1)
        );
    }

    #[test]
    fn current_schema_tracks_data_only_shared_revisions_without_losing_history() {
        let (Ok(entity_type_id), Ok(data_revision)) = (
            id(8),
            Revision::FIRST_COMMIT
                .next_commit()
                .and_then(Revision::next_commit),
        ) else {
            return;
        };
        let Some(definition) = entity_type(entity_type_id, "person", Revision::FIRST_COMMIT) else {
            return;
        };
        let mut model = SchemaHistoryReferenceModel::new();
        assert!(
            model
                .publish(Revision::FIRST_COMMIT, vec![definition])
                .is_ok()
        );
        assert!(model.advance_to(data_revision).is_ok());

        let current = model.schema_at(SchemaMode::Current, Revision::GENESIS);
        let historical = model.schema_at(SchemaMode::Historical, data_revision);
        assert_eq!(
            current
                .as_ref()
                .ok()
                .map(|snapshot| snapshot.schema_revision().revision()),
            Some(data_revision)
        );
        assert_eq!(
            historical
                .as_ref()
                .ok()
                .map(|snapshot| snapshot.definitions().len()),
            Some(1)
        );
        assert!(matches!(
            model.advance_to(data_revision),
            Err(SchemaHistoryError::RevisionNotIncreasing { .. })
        ));
    }

    #[test]
    fn fingerprints_are_stable_and_sensitive_to_canonical_schema_contents() {
        let (Ok(first_id), Ok(second_id)) = (id(3), id(3)) else {
            return;
        };
        let (Some(first), Some(second)) = (
            entity_type(first_id, "person", Revision::FIRST_COMMIT),
            entity_type(second_id, "person", Revision::FIRST_COMMIT),
        ) else {
            return;
        };
        let mut left = SchemaHistoryReferenceModel::new();
        let mut right = SchemaHistoryReferenceModel::new();
        let revision = Revision::FIRST_COMMIT;
        assert!(left.publish(revision, vec![first]).is_ok());
        assert!(right.publish(revision, vec![second]).is_ok());
        let Ok(left_snapshot) = left.schema_at(SchemaMode::Current, Revision::GENESIS) else {
            return;
        };
        let Ok(right_snapshot) = right.schema_at(SchemaMode::Current, Revision::GENESIS) else {
            return;
        };
        assert_eq!(left_snapshot.fingerprint(), right_snapshot.fingerprint());
        assert!(
            left_snapshot
                .verify_fingerprint(right_snapshot.fingerprint())
                .is_ok()
        );
        let mut corrupted = right_snapshot.fingerprint();
        corrupted[0] ^= 1;
        assert!(matches!(
            left_snapshot.verify_fingerprint(corrupted),
            Err(SchemaHistoryError::SchemaFingerprintMismatch { .. })
        ));
    }

    #[test]
    fn corrupt_required_history_fails_without_historical_fallback() {
        let revision = Revision::FIRST_COMMIT;
        let (Ok(first_id), Ok(second_id), Ok(later)) =
            (id(4), id(5), Revision::FIRST_COMMIT.next_commit())
        else {
            return;
        };
        let (Some(first), Some(second)) = (
            entity_type(first_id, "person", revision),
            entity_type(second_id, "place", later),
        ) else {
            return;
        };
        let mut model = SchemaHistoryReferenceModel::new();
        assert!(model.publish(revision, vec![first]).is_ok());
        assert!(model.publish(later, vec![second]).is_ok());
        model.mark_corrupt(revision);
        assert!(matches!(
            model.schema_at(SchemaMode::Historical, later),
            Err(SchemaHistoryError::HistoricalSchemaCorrupt { requested, corrupt_revision })
                if requested == later && corrupt_revision == revision
        ));
        assert!(matches!(
            model.schema_at(
                SchemaMode::Explicit(crate::SchemaRevision::from_published_revision(revision)),
                later
            ),
            Err(SchemaHistoryError::HistoricalSchemaCorrupt { .. })
        ));
    }

    #[test]
    fn historical_is_the_default_and_invalid_schema_batches_publish_nothing() {
        assert_eq!(SchemaMode::default(), SchemaMode::Historical);
        let (Ok(first_id), Ok(second_id), Ok(next_revision)) =
            (id(6), id(7), Revision::FIRST_COMMIT.next_commit())
        else {
            return;
        };
        let (Some(first), Some(duplicate_symbol)) = (
            entity_type(first_id, "person", Revision::FIRST_COMMIT),
            entity_type(second_id, "person", next_revision),
        ) else {
            return;
        };
        let mut model = SchemaHistoryReferenceModel::new();
        assert!(model.publish(Revision::FIRST_COMMIT, vec![first]).is_ok());
        assert!(matches!(
            model.publish(next_revision, vec![duplicate_symbol]),
            Err(SchemaHistoryError::DuplicateSymbol { .. })
        ));
        assert_eq!(model.latest_published(), Revision::FIRST_COMMIT);
        assert_eq!(
            model
                .schema_at(SchemaMode::Current, Revision::GENESIS)
                .ok()
                .map(|snapshot| snapshot.definitions().len()),
            Some(1)
        );
    }
}
