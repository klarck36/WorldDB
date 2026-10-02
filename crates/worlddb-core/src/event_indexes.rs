//! Candidate indexes for Event records, time, masks, and explicit relations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::event_projection::EventTimeFilter;
use crate::event_relations::{EventRelation, EventRelationKind};
use crate::events::{Event, EventMask};
use crate::ids::{
    EntityId, EventId, EventKindId, EventMaskId, EventRelationId, EventRoleId, TimelineId,
};
use crate::temporal::{EventTime, TemporalError, TimeInterval};

/// Kind and participant candidate postings for schema-validated Event records.
#[derive(Clone, Debug, Default)]
pub struct EventSearchIndex {
    event_ids: BTreeSet<EventId>,
    by_kind: BTreeMap<EventKindId, BTreeSet<EventId>>,
    by_role: BTreeMap<EventRoleId, BTreeSet<EventId>>,
    by_role_entity: BTreeMap<(EventRoleId, EntityId), BTreeSet<EventId>>,
}

impl EventSearchIndex {
    /// Builds exact EventKind, participant-role, and role/entity postings.
    pub fn build(events: &[Event]) -> Result<Self, EventIndexError> {
        let mut index = Self::default();
        for event in events {
            if !index.event_ids.insert(event.id()) {
                return Err(EventIndexError::DuplicateEventId(event.id()));
            }
            index
                .by_kind
                .entry(event.event_kind_id())
                .or_default()
                .insert(event.id());
            for participant in event.participants().as_slice() {
                index
                    .by_role
                    .entry(participant.role_id())
                    .or_default()
                    .insert(event.id());
                index
                    .by_role_entity
                    .entry((participant.role_id(), participant.entity_id()))
                    .or_default()
                    .insert(event.id());
            }
        }
        Ok(index)
    }

    /// Returns the indexed identity set for one exact EventKind.
    #[must_use]
    pub fn for_kind(&self, event_kind_id: EventKindId) -> BTreeSet<EventId> {
        self.by_kind
            .get(&event_kind_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns Events that explicitly contain at least one participant in this role.
    #[must_use]
    pub fn for_role(&self, role_id: EventRoleId) -> BTreeSet<EventId> {
        self.by_role.get(&role_id).cloned().unwrap_or_default()
    }

    /// Returns Events with this exact role/entity participant pair.
    #[must_use]
    pub fn for_participant(&self, role_id: EventRoleId, entity_id: EntityId) -> BTreeSet<EventId> {
        self.by_role_entity
            .get(&(role_id, entity_id))
            .cloned()
            .unwrap_or_default()
    }

    /// Returns every indexed Event identity in canonical order.
    #[must_use]
    pub fn event_ids(&self) -> &BTreeSet<EventId> {
        &self.event_ids
    }
}

#[derive(Clone, Copy, Debug)]
struct TimePosting {
    event_id: EventId,
    event_time: EventTime,
}

/// EventTime candidate index partitioned by Timeline and inclusive start.
///
/// Open spans remain open in this immutable index. If an `EventSpanClosure`
/// shortens one at the query's RecordedAsOf point, the normal Event projection
/// remains the final filter; the index may return a harmless extra candidate.
#[derive(Clone, Debug, Default)]
pub struct EventTimeIndex {
    event_ids: BTreeSet<EventId>,
    by_timeline_start: BTreeMap<TimelineId, BTreeMap<i128, Vec<TimePosting>>>,
}

impl EventTimeIndex {
    /// Builds temporal candidate postings from stored EventTime values.
    pub fn build(events: &[Event]) -> Result<Self, EventIndexError> {
        let mut index = Self::default();
        for event in events {
            if !index.event_ids.insert(event.id()) {
                return Err(EventIndexError::DuplicateEventId(event.id()));
            }
            let event_time = event.event_time();
            let start = match event_time {
                EventTime::Instant(point) => point,
                EventTime::Span { start, .. } => start,
            };
            index
                .by_timeline_start
                .entry(start.timeline().id())
                .or_default()
                .entry(start.nanoseconds())
                .or_default()
                .push(TimePosting {
                    event_id: event.id(),
                    event_time,
                });
        }
        for starts in index.by_timeline_start.values_mut() {
            for postings in starts.values_mut() {
                postings.sort_unstable_by_key(|posting| posting.event_id);
            }
        }
        Ok(index)
    }

    /// Returns same-Timeline Event identities whose stored instant/span matches this filter.
    ///
    /// For a closed `EventTime` this is exact. An open span can be closed later
    /// by an as-of-visible lifecycle record, so callers still pass candidates
    /// through the reference Event projection before returning search results.
    /// Events on other Timelines are retained as candidates so the authoritative
    /// projection can report an incomparable-time error when those records are
    /// otherwise visible to the query.
    pub fn candidates(
        &self,
        filter: EventTimeFilter,
    ) -> Result<BTreeSet<EventId>, EventIndexError> {
        match filter {
            EventTimeFilter::Any => Ok(self.event_ids.clone()),
            EventTimeFilter::At(point) => {
                let mut matches = BTreeSet::new();
                if let Some(starts) = self.by_timeline_start.get(&point.timeline().id()) {
                    for posting in starts
                        .range(..=point.nanoseconds())
                        .flat_map(|(_, postings)| postings)
                    {
                        if stored_time_matches_at(posting.event_time, point) {
                            matches.insert(posting.event_id);
                        }
                    }
                }
                self.include_other_timelines(point.timeline().id(), &mut matches);
                Ok(matches)
            }
            EventTimeFilter::Overlaps { start, end } => {
                let interval = TimeInterval::new(start.timeline(), Some(start), Some(end))
                    .map_err(EventIndexError::InvalidEventTimeFilter)?;
                let mut matches = BTreeSet::new();
                if let Some(starts) = self.by_timeline_start.get(&start.timeline().id()) {
                    for posting in starts
                        .range(..end.nanoseconds())
                        .flat_map(|(_, postings)| postings)
                    {
                        if stored_time_overlaps(posting.event_time, interval, start, end)
                            .map_err(EventIndexError::TemporalComparison)?
                        {
                            matches.insert(posting.event_id);
                        }
                    }
                }
                self.include_other_timelines(start.timeline().id(), &mut matches);
                Ok(matches)
            }
        }
    }

    fn include_other_timelines(&self, query_timeline: TimelineId, matches: &mut BTreeSet<EventId>) {
        for (timeline, starts) in &self.by_timeline_start {
            if *timeline == query_timeline {
                continue;
            }
            for postings in starts.values() {
                matches.extend(postings.iter().map(|posting| posting.event_id));
            }
        }
    }
}

fn stored_time_matches_at(event_time: EventTime, point: crate::temporal::WorldTime) -> bool {
    match event_time {
        EventTime::Instant(instant) => instant.nanoseconds() == point.nanoseconds(),
        EventTime::Span { start, end } => {
            start.nanoseconds() <= point.nanoseconds()
                && end.is_none_or(|exclusive_end| exclusive_end.nanoseconds() > point.nanoseconds())
        }
    }
}

fn stored_time_overlaps(
    event_time: EventTime,
    query: TimeInterval,
    query_start: crate::temporal::WorldTime,
    query_end: crate::temporal::WorldTime,
) -> Result<bool, TemporalError> {
    Ok(match event_time {
        EventTime::Instant(point) => query.contains(point)?,
        EventTime::Span {
            start: event_start,
            end: event_end,
        } => {
            event_start.nanoseconds() < query_end.nanoseconds()
                && event_end.is_none_or(|exclusive_end| {
                    exclusive_end.nanoseconds() > query_start.nanoseconds()
                })
        }
    })
}

/// Exact-target EventMask postings; lifecycle and archive state remain separate.
#[derive(Clone, Debug, Default)]
pub struct EventMaskIndex {
    by_target_event: BTreeMap<EventId, Vec<EventMask>>,
    mask_ids: BTreeSet<EventMaskId>,
}

impl EventMaskIndex {
    /// Builds exact target Event postings and rejects repeated Mask identities.
    pub fn build(masks: &[EventMask]) -> Result<Self, EventIndexError> {
        let mut index = Self::default();
        for mask in masks {
            if !index.mask_ids.insert(mask.id()) {
                return Err(EventIndexError::DuplicateEventMaskId(mask.id()));
            }
            index
                .by_target_event
                .entry(mask.target_event())
                .or_default()
                .push(*mask);
        }
        for masks in index.by_target_event.values_mut() {
            masks.sort_unstable_by_key(|mask| mask.id());
        }
        Ok(index)
    }

    /// Returns only masks directly targeting the supplied Event.
    #[must_use]
    pub fn for_event(&self, event_id: EventId) -> Vec<EventMask> {
        self.by_target_event
            .get(&event_id)
            .cloned()
            .unwrap_or_default()
    }
}

/// Direct EventRelation postings, with no temporal or graph-transitive inference.
#[derive(Clone, Debug, Default)]
pub struct EventRelationIndex {
    relations: Vec<EventRelation>,
    by_event: BTreeMap<EventId, BTreeSet<usize>>,
    by_kind: BTreeMap<EventRelationKind, BTreeSet<usize>>,
    relation_ids: BTreeSet<EventRelationId>,
}

impl EventRelationIndex {
    /// Builds direct endpoint and explicit relation-kind postings.
    ///
    /// Different records may share a logical key when history contains a
    /// retracted record and a later replacement; as-of lifecycle validation is
    /// performed by `project_active_event_relations` before querying this index.
    pub fn build(relations: &[EventRelation]) -> Result<Self, EventIndexError> {
        let mut index = Self::default();
        for relation in relations {
            if !index.relation_ids.insert(relation.id()) {
                return Err(EventIndexError::DuplicateEventRelationId(relation.id()));
            }
            let position = index.relations.len();
            index.relations.push(*relation);
            index
                .by_event
                .entry(relation.from_event())
                .or_default()
                .insert(position);
            index
                .by_event
                .entry(relation.to_event())
                .or_default()
                .insert(position);
            index
                .by_kind
                .entry(relation.kind())
                .or_default()
                .insert(position);
        }
        Ok(index)
    }

    /// Returns explicit relations incident to one Event, in logical-key order.
    #[must_use]
    pub fn for_event(&self, event_id: EventId) -> Vec<EventRelation> {
        let mut matches = self
            .by_event
            .get(&event_id)
            .into_iter()
            .flatten()
            .filter_map(|position| self.relations.get(*position).copied())
            .collect::<Vec<_>>();
        matches.sort_unstable_by_key(|relation| (relation.key(), relation.id()));
        matches
    }

    /// Returns explicit records of one persisted relation kind.
    #[must_use]
    pub fn for_kind(&self, kind: EventRelationKind) -> Vec<EventRelation> {
        let mut matches = self
            .by_kind
            .get(&kind)
            .into_iter()
            .flatten()
            .filter_map(|position| self.relations.get(*position).copied())
            .collect::<Vec<_>>();
        matches.sort_unstable_by_key(|relation| (relation.key(), relation.id()));
        matches
    }

    /// Returns only direct records connecting these two endpoints.
    #[must_use]
    pub fn between(&self, left: EventId, right: EventId) -> Vec<EventRelation> {
        self.for_event(left)
            .into_iter()
            .filter(|relation| {
                (relation.from_event() == left && relation.to_event() == right)
                    || (relation.from_event() == right && relation.to_event() == left)
            })
            .collect()
    }
}

/// Duplicate identity or malformed time filter encountered by an Event index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventIndexError {
    /// A source Event identity occurs more than once.
    DuplicateEventId(EventId),
    /// A source EventMask identity occurs more than once.
    DuplicateEventMaskId(EventMaskId),
    /// A source EventRelation identity occurs more than once.
    DuplicateEventRelationId(EventRelationId),
    /// An EventTime overlap filter is not a positive interval on one Timeline.
    InvalidEventTimeFilter(TemporalError),
    /// EventTime records could not be compared on the indexed Timeline.
    TemporalComparison(TemporalError),
}

impl fmt::Display for EventIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateEventId(id) => write!(formatter, "duplicate EventId {id}"),
            Self::DuplicateEventMaskId(id) => write!(formatter, "duplicate EventMaskId {id}"),
            Self::DuplicateEventRelationId(id) => {
                write!(formatter, "duplicate EventRelationId {id}")
            }
            Self::InvalidEventTimeFilter(error) => {
                write!(formatter, "invalid EventTime filter: {error}")
            }
            Self::TemporalComparison(error) => {
                write!(formatter, "EventTime comparison failed: {error}")
            }
        }
    }
}

impl std::error::Error for EventIndexError {}
