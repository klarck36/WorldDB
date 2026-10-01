//! Index-free Event visibility, schema, time, and correction reference projection.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;

use crate::archive::ArchiveTargetRef;
use crate::archive_projection::ArchiveHistoryReferenceModel;
use crate::catalog::HistorySpaceCatalog;
use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
use crate::events::{
    Event, EventDraft, EventMask, EventMaskRetraction, EventRecordError, EventRetraction,
    EventSpanClosure,
};
use crate::ids::PrincipalId;
use crate::ids::{
    EventId, EventKindId, EventMaskId, EventMaskRetractionId, EventRetractionId,
    EventSpanClosureId, HistorySpaceId, ProvenanceId, Revision,
};
use crate::layers::{LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
use crate::query_context::QueryContext;
use crate::record_refs::RecordRef;
use crate::schema::EventKindDefinition;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicySnapshot,
};
use crate::source_provenance::{
    ProvenanceEdge, ProvenanceEndpointRef, ProvenanceRelation, SourceEvidenceProvenanceError,
};
use crate::temporal::{EventTime, RecordedAsOf, TemporalError, TimeInterval, WorldTime};

/// Event-time filter supported by the index-free reference query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventTimeFilter {
    /// Do not filter by EventTime.
    Any,
    /// Match an instant inside an EventTime or a coordinate in a half-open span.
    At(WorldTime),
    /// Match Event instants or spans overlapping this half-open interval.
    Overlaps { start: WorldTime, end: WorldTime },
}

/// Immutable query coordinates for an Event full scan.
#[derive(Clone, Debug)]
pub struct EventCandidateQuery {
    history_space_id: HistorySpaceId,
    layer_selection: LayerSelection,
    recorded_as_of: RecordedAsOf,
    time_filter: EventTimeFilter,
}

impl EventCandidateQuery {
    /// Selects one HistorySpace, layer set, transaction-time point, and EventTime filter.
    #[must_use]
    pub const fn new(
        history_space_id: HistorySpaceId,
        layer_selection: LayerSelection,
        recorded_as_of: RecordedAsOf,
        time_filter: EventTimeFilter,
    ) -> Self {
        Self {
            history_space_id,
            layer_selection,
            recorded_as_of,
            time_filter,
        }
    }
}

/// Event-family records and the pinned EventKind/archive snapshots for a scan.
pub struct EventHistory<'a> {
    events: &'a [Event],
    closures: &'a [EventSpanClosure],
    retractions: &'a [EventRetraction],
    masks: &'a [EventMask],
    mask_retractions: &'a [EventMaskRetraction],
    archive: &'a ArchiveHistoryReferenceModel,
    event_kinds: &'a [EventKindDefinition],
}

impl<'a> EventHistory<'a> {
    /// Binds Event records, lifecycle records, archive view, and pinned EventKind definitions.
    #[must_use]
    pub const fn new(
        events: &'a [Event],
        closures: &'a [EventSpanClosure],
        retractions: &'a [EventRetraction],
        archive: &'a ArchiveHistoryReferenceModel,
        event_kinds: &'a [EventKindDefinition],
    ) -> Self {
        Self {
            events,
            closures,
            retractions,
            masks: &[],
            mask_retractions: &[],
            archive,
            event_kinds,
        }
    }

    /// Adds the EventMask history and its separate retractions to this scan.
    #[must_use]
    pub const fn with_masks(
        mut self,
        masks: &'a [EventMask],
        mask_retractions: &'a [EventMaskRetraction],
    ) -> Self {
        self.masks = masks;
        self.mask_retractions = mask_retractions;
        self
    }
}

/// One active, schema-valid Event and its HistorySpace-before-Layer precedence.
#[derive(Clone, Debug)]
pub struct EventCandidate {
    event: Event,
    precedence: ContextPrecedence,
}

impl EventCandidate {
    /// Returns the immutable visible Event.
    #[must_use]
    pub const fn event(&self) -> &Event {
        &self.event
    }

    /// Returns its HistorySpace distance and pinned LayerRank priority.
    #[must_use]
    pub const fn precedence(&self) -> ContextPrecedence {
        self.precedence
    }
}

/// Collects schema-valid Events visible at one historical query point.
///
/// The caller supplies the pinned EventKind definitions. Events are filtered by
/// HistorySpace ancestry, selected layer, RecordedAsOf, operational archive
/// state, EventMask precedence, EventRetraction, EventSpanClosure, and the
/// optional EventTime selector. EventMask affects Event visibility only; it
/// does not retract the Event or alter Assertions. Event relations and
/// Provenance do not alter this projection.
pub fn full_scan_event_candidates(
    history: EventHistory<'_>,
    history_spaces: &HistorySpaceCatalog,
    layers: &LayerSchemaSnapshot,
    query: &EventCandidateQuery,
) -> Result<Vec<EventCandidate>, EventProjectionError> {
    let selected_layers = layers.resolve(&query.layer_selection)?;
    let selected_layers = selected_layers
        .as_slice()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if selected_layers.is_empty() {
        return Err(EventProjectionError::EmptyLayerSelection);
    }

    let mut event_ids = BTreeSet::new();
    for event in history.events {
        if !event_ids.insert(event.id()) {
            return Err(EventProjectionError::DuplicateEvent {
                event_id: event.id(),
            });
        }
    }
    validate_event_lifecycle(history.events, history.closures, history.retractions)?;
    validate_event_mask_lifecycle(history.events, history.masks, history.mask_retractions)?;
    let mut kinds = std::collections::BTreeMap::new();
    for kind in history.event_kinds {
        if kinds.insert(kind.event_kind_id(), kind).is_some() {
            return Err(EventProjectionError::DuplicateEventKind {
                event_kind_id: kind.event_kind_id(),
            });
        }
    }

    let ordinary_targets = history
        .archive
        .ordinary_targets_at(query.recorded_as_of)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut masked_events = BTreeSet::new();
    for mask in history.masks {
        if mask.created_revision() > query.recorded_as_of.revision()
            || !selected_layers.contains(&mask.layer_id())
        {
            continue;
        }
        let mask_target = ArchiveTargetRef::EventMask(mask.id());
        let archive_record = history
            .archive
            .targets()
            .iter()
            .find(|record| record.target() == mask_target)
            .ok_or(EventProjectionError::MissingEventMaskArchiveTarget {
                event_mask_id: mask.id(),
            })?;
        if archive_record.created_revision() != mask.created_revision() {
            return Err(EventProjectionError::EventMaskArchiveRevisionMismatch {
                event_mask_id: mask.id(),
            });
        }
        if !ordinary_targets.contains(&mask_target)
            || history.mask_retractions.iter().any(|retraction| {
                retraction.event_mask_id() == mask.id()
                    && retraction.created_revision() <= query.recorded_as_of.revision()
            })
        {
            continue;
        }
        let target_event = history
            .events
            .iter()
            .find(|event| event.id() == mask.target_event())
            .ok_or(EventProjectionError::MissingEvent {
                event_id: mask.target_event(),
            })?;
        if target_event.created_revision() > query.recorded_as_of.revision() {
            continue;
        }
        let mask_precedence = match ContextPrecedence::for_context(
            query.history_space_id,
            mask.history_space_id(),
            mask.layer_id(),
            history_spaces,
            layers,
        ) {
            Ok(value) => value,
            Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. }) => continue,
            Err(error) => return Err(error.into()),
        };
        let event_precedence = match ContextPrecedence::for_context(
            query.history_space_id,
            target_event.history_space_id(),
            target_event.layer_id(),
            history_spaces,
            layers,
        ) {
            Ok(value) => value,
            Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. }) => continue,
            Err(error) => return Err(error.into()),
        };
        if mask_precedence > event_precedence {
            masked_events.insert(target_event.id());
        }
    }
    let mut output = Vec::new();
    for event in history.events {
        if event.created_revision() > query.recorded_as_of.revision()
            || !selected_layers.contains(&event.layer_id())
            || masked_events.contains(&event.id())
        {
            continue;
        }
        let target = ArchiveTargetRef::Event(event.id());
        let archive_record = history
            .archive
            .targets()
            .iter()
            .find(|record| record.target() == target)
            .ok_or(EventProjectionError::MissingArchiveTarget {
                event_id: event.id(),
            })?;
        if archive_record.created_revision() != event.created_revision() {
            return Err(EventProjectionError::ArchiveRevisionMismatch {
                event_id: event.id(),
            });
        }
        if !ordinary_targets.contains(&target) {
            continue;
        }
        if history.retractions.iter().any(|retraction| {
            retraction.event_id() == event.id()
                && retraction.created_revision() <= query.recorded_as_of.revision()
        }) {
            continue;
        }

        let event_kind = kinds.get(&event.event_kind_id()).copied().ok_or(
            EventProjectionError::MissingEventKind {
                event_id: event.id(),
                event_kind_id: event.event_kind_id(),
            },
        )?;
        if event_kind.created_revision() > query.recorded_as_of.revision() {
            return Err(EventProjectionError::EventKindAfterQuery {
                event_id: event.id(),
            });
        }
        let draft = EventDraft::new(
            event.history_space_id(),
            event.layer_id(),
            event_kind,
            event.participants().as_slice().to_vec(),
            event.attributes().as_slice().to_vec(),
            event.event_time(),
        )?;
        let validated = Event::new(event.id(), draft, event.created_revision())?;
        let effective_time =
            effective_event_time(event, history.closures, query.recorded_as_of.revision())?;
        if !matches_event_time(effective_time, query.time_filter)? {
            continue;
        }

        let precedence = match ContextPrecedence::for_context(
            query.history_space_id,
            event.history_space_id(),
            event.layer_id(),
            history_spaces,
            layers,
        ) {
            Ok(value) => value,
            Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. }) => continue,
            Err(error) => return Err(error.into()),
        };
        output.push(EventCandidate {
            event: validated,
            precedence,
        });
    }
    output.sort_by_key(|candidate| candidate.event.id());
    Ok(output)
}

/// Full Event scan with authorization applied before lifecycle and mask projection.
/// Events that fail record, owner, layer, or any candidate-relevant field check
/// are removed together with their lifecycle records before the ordinary scan.
pub fn full_scan_authorized_event_candidates(
    history: EventHistory<'_>,
    history_spaces: &HistorySpaceCatalog,
    layers: &LayerSchemaSnapshot,
    query: &EventCandidateQuery,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<Vec<EventCandidate>, EventProjectionError> {
    if context.history_space() != query.history_space_id
        || context.recorded_as_of() != query.recorded_as_of
        || context.layers().requested() != &query.layer_selection
        || context.layers().schema_revision() != layers.revision()
    {
        return Err(EventProjectionError::QueryContextMismatch);
    }
    let principal = context.security().principal_id();
    if policy.authorize(principal, Capability::QueryResolve, PolicyTarget::default())
        != AuthorizationDecision::Allow
    {
        return Ok(Vec::new());
    }
    let visible_events = history
        .events
        .iter()
        .filter(|event| event_is_authorized(policy, principal, event))
        .cloned()
        .collect::<Vec<_>>();
    let visible_ids = visible_events
        .iter()
        .map(Event::id)
        .collect::<BTreeSet<_>>();
    let visible_closures = history
        .closures
        .iter()
        .filter(|item| visible_ids.contains(&item.event_id()))
        .copied()
        .collect::<Vec<_>>();
    let visible_retractions = history
        .retractions
        .iter()
        .filter(|item| visible_ids.contains(&item.event_id()))
        .cloned()
        .collect::<Vec<_>>();
    let visible_masks = history
        .masks
        .iter()
        .filter(|mask| {
            visible_ids.contains(&mask.target_event())
                && record_authorized(
                    policy,
                    principal,
                    Capability::EventMaskRead,
                    mask.history_space_id(),
                    mask.layer_id(),
                    RecordRef::EventMask(mask.id()),
                )
                && field_authorized(
                    policy,
                    principal,
                    mask.history_space_id(),
                    mask.layer_id(),
                    RecordRef::EventMask(mask.id()),
                    FieldSelector::EventMaskTarget,
                )
        })
        .copied()
        .collect::<Vec<_>>();
    let visible_mask_ids = visible_masks
        .iter()
        .map(|item| item.id())
        .collect::<BTreeSet<_>>();
    let visible_mask_retractions = history
        .mask_retractions
        .iter()
        .filter(|item| visible_mask_ids.contains(&item.event_mask_id()))
        .cloned()
        .collect::<Vec<_>>();
    let visible_kind_ids = visible_events
        .iter()
        .map(Event::event_kind_id)
        .collect::<BTreeSet<_>>();
    let visible_kinds = history
        .event_kinds
        .iter()
        .filter(|kind| visible_kind_ids.contains(&kind.event_kind_id()))
        .cloned()
        .collect::<Vec<_>>();
    full_scan_event_candidates(
        EventHistory::new(
            &visible_events,
            &visible_closures,
            &visible_retractions,
            history.archive,
            &visible_kinds,
        )
        .with_masks(&visible_masks, &visible_mask_retractions),
        history_spaces,
        layers,
        query,
    )
}

pub(crate) fn event_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    event: &Event,
) -> bool {
    let kind = event.event_kind_id();
    let record = RecordRef::Event(event.id());
    if !record_authorized(
        policy,
        principal,
        Capability::EventRead,
        event.history_space_id(),
        event.layer_id(),
        record,
    ) {
        return false;
    }
    if !field_authorized(
        policy,
        principal,
        event.history_space_id(),
        event.layer_id(),
        record,
        FieldSelector::EventKind,
    ) || !field_authorized(
        policy,
        principal,
        event.history_space_id(),
        event.layer_id(),
        record,
        FieldSelector::EventTime(kind),
    ) {
        return false;
    }
    event.participants().as_slice().iter().all(|participant| {
        field_authorized(
            policy,
            principal,
            event.history_space_id(),
            event.layer_id(),
            record,
            FieldSelector::EventParticipant(kind, participant.role_id()),
        )
    }) && event.attributes().as_slice().iter().all(|attribute| {
        field_authorized(
            policy,
            principal,
            event.history_space_id(),
            event.layer_id(),
            record,
            FieldSelector::EventAttribute(kind, attribute.attribute_id()),
        )
    })
}

fn record_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    capability: Capability,
    history_space: HistorySpaceId,
    layer: crate::ids::LayerId,
    record: RecordRef,
) -> bool {
    [
        Capability::HistorySpaceRead,
        Capability::LayerRead,
        capability,
    ]
    .into_iter()
    .all(|capability| {
        policy.authorize(
            principal,
            capability,
            PolicyTarget::new(Some(history_space), Some(layer), Some(record), None, None),
        ) == AuthorizationDecision::Allow
    })
}

fn field_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    history_space: HistorySpaceId,
    layer: crate::ids::LayerId,
    record: RecordRef,
    field: FieldSelector,
) -> bool {
    policy.authorize(
        principal,
        Capability::FieldRead,
        PolicyTarget::new(
            Some(history_space),
            Some(layer),
            Some(record),
            Some(field),
            None,
        ),
    ) == AuthorizationDecision::Allow
}

/// One explicit Event correction: a new Event and explanatory Corrects edge.
///
/// The returned shape has no EventRetraction field. The original remains an
/// active history record until a separate explicit retraction is supplied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventCorrection {
    replacement: EventId,
    corrects: ProvenanceId,
}

impl EventCorrection {
    /// Returns the new immutable Event identity.
    #[must_use]
    pub const fn replacement_id(self) -> EventId {
        self.replacement
    }

    /// Returns the explanatory Corrects edge identity.
    #[must_use]
    pub const fn corrects_id(self) -> ProvenanceId {
        self.corrects
    }
}

/// Builds an Event correction batch with no implicit EventRetraction or chronology relation.
pub fn prepare_event_correction<F>(
    target: &Event,
    replacement_id: EventId,
    replacement: EventDraft,
    corrects_id: ProvenanceId,
    commit_revision: Revision,
    mut compatible_successor: F,
) -> Result<(Event, ProvenanceEdge, EventCorrection), EventProjectionError>
where
    F: FnMut(EventKindId, EventKindId) -> bool,
{
    if commit_revision <= target.created_revision() {
        return Err(EventProjectionError::CorrectionRevisionNotAfterTarget {
            event_id: target.id(),
        });
    }
    let replacement = Event::new(replacement_id, replacement, commit_revision)?;
    if replacement.history_space_id() != target.history_space_id()
        || replacement.layer_id() != target.layer_id()
    {
        return Err(EventProjectionError::CorrectionContextMismatch {
            event_id: target.id(),
        });
    }
    if !compatible_successor(target.event_kind_id(), replacement.event_kind_id()) {
        return Err(EventProjectionError::IncompatibleCorrectionEventKind {
            target_event_id: target.id(),
            target_kind: target.event_kind_id(),
            replacement_kind: replacement.event_kind_id(),
        });
    }
    let edge = ProvenanceEdge::new(
        corrects_id,
        ProvenanceEndpointRef::Event(replacement.id()),
        ProvenanceEndpointRef::Event(target.id()),
        ProvenanceRelation::Corrects,
        commit_revision,
    )?;
    let result = EventCorrection {
        replacement: replacement.id(),
        corrects: edge.id(),
    };
    Ok((replacement, edge, result))
}

fn effective_event_time(
    event: &Event,
    closures: &[EventSpanClosure],
    recorded_revision: Revision,
) -> Result<EventTime, EventProjectionError> {
    if let EventTime::Span { start, end: None } = event.event_time() {
        let visible = closures.iter().find(|closure| {
            closure.event_id() == event.id() && closure.created_revision() <= recorded_revision
        });
        if let Some(closure) = visible {
            return Ok(EventTime::span(start, Some(closure.close_at_event_time()))?);
        }
    }
    Ok(event.event_time())
}

fn matches_event_time(
    time: EventTime,
    filter: EventTimeFilter,
) -> Result<bool, EventProjectionError> {
    match filter {
        EventTimeFilter::Any => Ok(true),
        EventTimeFilter::At(point) => Ok(time.contains(point)?),
        EventTimeFilter::Overlaps { start, end } => {
            let query = TimeInterval::new(start.timeline(), Some(start), Some(end))?;
            match time {
                EventTime::Instant(point) => query.contains(point).map_err(Into::into),
                EventTime::Span {
                    start: event_start,
                    end: event_end,
                } => {
                    let starts_before_query_end = event_start.checked_cmp(end)? == Ordering::Less;
                    let ends_after_query_start = match event_end {
                        Some(event_end) => event_end.checked_cmp(start)? == Ordering::Greater,
                        None => true,
                    };
                    Ok(starts_before_query_end && ends_after_query_start)
                }
            }
        }
    }
}

fn validate_event_lifecycle(
    events: &[Event],
    closures: &[EventSpanClosure],
    retractions: &[EventRetraction],
) -> Result<(), EventProjectionError> {
    let mut closure_ids = BTreeSet::<EventSpanClosureId>::new();
    let mut closed_targets = BTreeSet::<EventId>::new();
    for closure in closures {
        if !closure_ids.insert(closure.id()) {
            return Err(EventProjectionError::DuplicateClosure {
                closure_id: closure.id(),
            });
        }
        if !closed_targets.insert(closure.event_id()) {
            return Err(EventProjectionError::DuplicateEventClosure {
                event_id: closure.event_id(),
            });
        }
        let event = events
            .iter()
            .find(|event| event.id() == closure.event_id())
            .ok_or(EventProjectionError::MissingEvent {
                event_id: closure.event_id(),
            })?;
        if closure.created_revision() <= event.created_revision() {
            return Err(EventProjectionError::LifecycleRevisionOrder {
                event_id: event.id(),
            });
        }
        let EventTime::Span { start, end: None } = event.event_time() else {
            return Err(EventProjectionError::ClosureRequiresOpenSpan {
                event_id: event.id(),
            });
        };
        if closure.close_at_event_time().checked_cmp(start)? != Ordering::Greater {
            return Err(EventProjectionError::ClosureNotAfterStart {
                event_id: event.id(),
            });
        }
    }
    let mut retraction_ids = BTreeSet::<EventRetractionId>::new();
    for retraction in retractions {
        if !retraction_ids.insert(retraction.id()) {
            return Err(EventProjectionError::DuplicateRetraction {
                retraction_id: retraction.id(),
            });
        }
        let event = events
            .iter()
            .find(|event| event.id() == retraction.event_id())
            .ok_or(EventProjectionError::MissingEvent {
                event_id: retraction.event_id(),
            })?;
        if retraction.created_revision() <= event.created_revision() {
            return Err(EventProjectionError::LifecycleRevisionOrder {
                event_id: event.id(),
            });
        }
    }
    Ok(())
}

fn validate_event_mask_lifecycle(
    events: &[Event],
    masks: &[EventMask],
    retractions: &[EventMaskRetraction],
) -> Result<(), EventProjectionError> {
    let event_by_id = events
        .iter()
        .map(|event| (event.id(), event))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut mask_by_id = std::collections::BTreeMap::new();
    for mask in masks {
        if mask_by_id.insert(mask.id(), mask).is_some() {
            return Err(EventProjectionError::DuplicateEventMask {
                event_mask_id: mask.id(),
            });
        }
        let event =
            event_by_id
                .get(&mask.target_event())
                .ok_or(EventProjectionError::MissingEvent {
                    event_id: mask.target_event(),
                })?;
        if mask.created_revision() <= event.created_revision() {
            return Err(EventProjectionError::LifecycleRevisionOrder {
                event_id: event.id(),
            });
        }
    }

    let mut retraction_ids = BTreeSet::new();
    for retraction in retractions {
        if !retraction_ids.insert(retraction.id()) {
            return Err(EventProjectionError::DuplicateEventMaskRetraction {
                event_mask_retraction_id: retraction.id(),
            });
        }
        let mask = mask_by_id.get(&retraction.event_mask_id()).ok_or(
            EventProjectionError::MissingEventMask {
                event_mask_id: retraction.event_mask_id(),
            },
        )?;
        if retraction.created_revision() <= mask.created_revision() {
            return Err(EventProjectionError::LifecycleRevisionOrder {
                event_id: mask.target_event(),
            });
        }
    }
    Ok(())
}

/// Invalid Event history, schema, time, or correction input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventProjectionError {
    /// Query arguments disagree with the validated security-bearing context.
    QueryContextMismatch,
    EmptyLayerSelection,
    DuplicateEvent {
        event_id: EventId,
    },
    DuplicateEventKind {
        event_kind_id: EventKindId,
    },
    MissingEventKind {
        event_id: EventId,
        event_kind_id: EventKindId,
    },
    EventKindAfterQuery {
        event_id: EventId,
    },
    MissingArchiveTarget {
        event_id: EventId,
    },
    ArchiveRevisionMismatch {
        event_id: EventId,
    },
    DuplicateClosure {
        closure_id: EventSpanClosureId,
    },
    DuplicateEventClosure {
        event_id: EventId,
    },
    DuplicateRetraction {
        retraction_id: EventRetractionId,
    },
    DuplicateEventMask {
        event_mask_id: EventMaskId,
    },
    DuplicateEventMaskRetraction {
        event_mask_retraction_id: EventMaskRetractionId,
    },
    MissingEventMask {
        event_mask_id: EventMaskId,
    },
    MissingEventMaskArchiveTarget {
        event_mask_id: EventMaskId,
    },
    EventMaskArchiveRevisionMismatch {
        event_mask_id: EventMaskId,
    },
    MissingEvent {
        event_id: EventId,
    },
    LifecycleRevisionOrder {
        event_id: EventId,
    },
    ClosureRequiresOpenSpan {
        event_id: EventId,
    },
    ClosureNotAfterStart {
        event_id: EventId,
    },
    CorrectionRevisionNotAfterTarget {
        event_id: EventId,
    },
    CorrectionContextMismatch {
        event_id: EventId,
    },
    IncompatibleCorrectionEventKind {
        target_event_id: EventId,
        target_kind: EventKindId,
        replacement_kind: EventKindId,
    },
    Layer(LayerSchemaError),
    Precedence(ContextPrecedenceError),
    Temporal(TemporalError),
    Event(EventRecordError),
    Provenance(SourceEvidenceProvenanceError),
    Archive(crate::archive_projection::ArchiveProjectionError),
}

impl From<LayerSchemaError> for EventProjectionError {
    fn from(value: LayerSchemaError) -> Self {
        Self::Layer(value)
    }
}
impl From<ContextPrecedenceError> for EventProjectionError {
    fn from(value: ContextPrecedenceError) -> Self {
        Self::Precedence(value)
    }
}
impl From<TemporalError> for EventProjectionError {
    fn from(value: TemporalError) -> Self {
        Self::Temporal(value)
    }
}
impl From<EventRecordError> for EventProjectionError {
    fn from(value: EventRecordError) -> Self {
        Self::Event(value)
    }
}
impl From<SourceEvidenceProvenanceError> for EventProjectionError {
    fn from(value: SourceEvidenceProvenanceError) -> Self {
        Self::Provenance(value)
    }
}
impl From<crate::archive_projection::ArchiveProjectionError> for EventProjectionError {
    fn from(value: crate::archive_projection::ArchiveProjectionError) -> Self {
        Self::Archive(value)
    }
}

impl fmt::Display for EventProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Event projection failed: {self:?}")
    }
}
impl std::error::Error for EventProjectionError {}
