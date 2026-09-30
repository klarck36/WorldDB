//! Timeline-aware temporal types and half-open intervals.

use std::cmp::Ordering;
use std::fmt;

use crate::ids::{Revision, TimelineId};

/// A published-history coordinate used for transaction-time queries.
///
/// Constructing this value records the caller's assertion that the revision
/// was published. A history reader must verify that assertion against its
/// database before returning the value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordedAsOf(Revision);

impl RecordedAsOf {
    /// Tags a revision that the caller has verified as published.
    #[must_use]
    pub const fn from_published_revision(revision: Revision) -> Self {
        Self(revision)
    }

    /// Returns the underlying transaction-time revision.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.0
    }
}

/// A temporal axis identified by its stable project-schema identity.
///
/// Registration and lifecycle state are supplied by a schema snapshot; this
/// value carries only the axis identity used by temporal coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Timeline(TimelineId);

impl Timeline {
    /// Binds a validated identity to a timeline axis.
    #[must_use]
    pub const fn new(id: TimelineId) -> Self {
        Self(id)
    }

    /// Returns the stable identity of this timeline axis.
    #[must_use]
    pub const fn id(self) -> TimelineId {
        self.0
    }
}

/// A schema-resolved WorldDB time coordinate, normalized to signed nanoseconds.
///
/// The value deliberately has no `Ord` or `PartialOrd` implementation: values
/// on different timelines cannot be globally ordered. The later `Time` core
/// value retains its input ticks and unit symbol; schema resolution converts
/// it to this comparison coordinate with checked arithmetic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorldTime {
    timeline: Timeline,
    nanoseconds: i128,
}

impl WorldTime {
    /// Creates a coordinate whose nanosecond value has already been resolved
    /// and checked against the selected timeline schema.
    #[must_use]
    pub const fn from_nanoseconds(timeline: Timeline, nanoseconds: i128) -> Self {
        Self {
            timeline,
            nanoseconds,
        }
    }

    /// Returns the timeline that owns this coordinate.
    #[must_use]
    pub const fn timeline(self) -> Timeline {
        self.timeline
    }

    /// Returns the normalized signed nanosecond coordinate.
    #[must_use]
    pub const fn nanoseconds(self) -> i128 {
        self.nanoseconds
    }

    /// Compares coordinates only when they belong to the same timeline.
    pub fn checked_cmp(self, other: Self) -> Result<Ordering, TemporalError> {
        ensure_same_timeline(self.timeline, other.timeline)?;
        Ok(self.nanoseconds.cmp(&other.nanoseconds))
    }

    /// Adds a physical duration without changing timelines or wrapping.
    pub const fn checked_add(self, duration: Duration) -> Result<Self, TemporalError> {
        match self.nanoseconds.checked_add(duration.0) {
            Some(nanoseconds) => Ok(Self {
                timeline: self.timeline,
                nanoseconds,
            }),
            None => Err(TemporalError::Overflow),
        }
    }
}

/// A signed physical duration in nanoseconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Duration(i128);

impl Duration {
    /// Constructs a duration from signed nanoseconds.
    #[must_use]
    pub const fn from_nanoseconds(nanoseconds: i128) -> Self {
        Self(nanoseconds)
    }

    /// Returns the signed nanosecond length.
    #[must_use]
    pub const fn nanoseconds(self) -> i128 {
        self.0
    }
}

/// A bounded or open-ended interval on one timeline, with `[start, end)` semantics.
///
/// `None` for `start` means there is no lower bound. `None` for `end` means the
/// interval remains open. Equal finite endpoints describe an empty interval.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TimeInterval {
    timeline: Timeline,
    start: Option<WorldTime>,
    end: Option<WorldTime>,
}

impl TimeInterval {
    /// Creates a half-open interval and rejects reversed or cross-timeline bounds.
    pub fn new(
        timeline: Timeline,
        start: Option<WorldTime>,
        end: Option<WorldTime>,
    ) -> Result<Self, TemporalError> {
        if let Some(value) = start {
            ensure_same_timeline(timeline, value.timeline)?;
        }
        if let Some(value) = end {
            ensure_same_timeline(timeline, value.timeline)?;
        }
        if let (Some(start), Some(end)) = (start, end) {
            if start.nanoseconds > end.nanoseconds {
                return Err(TemporalError::StartAfterEnd);
            }
        }
        Ok(Self {
            timeline,
            start,
            end,
        })
    }

    /// Returns the timeline that owns this interval.
    #[must_use]
    pub const fn timeline(self) -> Timeline {
        self.timeline
    }

    /// Returns the inclusive start boundary, if bounded.
    #[must_use]
    pub const fn start(self) -> Option<WorldTime> {
        self.start
    }

    /// Returns the exclusive end boundary, if bounded.
    #[must_use]
    pub const fn end(self) -> Option<WorldTime> {
        self.end
    }

    /// Returns whether the interval contains the supplied coordinate.
    pub fn contains(self, time: WorldTime) -> Result<bool, TemporalError> {
        ensure_same_timeline(self.timeline, time.timeline)?;
        if self
            .start
            .is_some_and(|start| time.nanoseconds < start.nanoseconds)
        {
            return Ok(false);
        }
        if self
            .end
            .is_some_and(|end| time.nanoseconds >= end.nanoseconds)
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Returns whether equal finite endpoints make this interval empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        matches!((self.start, self.end), (Some(start), Some(end)) if start.nanoseconds == end.nanoseconds)
    }
}

/// The world-time interval during which an assertion is valid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssertionValidity(TimeInterval);

impl AssertionValidity {
    /// Constructs validity from a validated half-open interval.
    #[must_use]
    pub const fn new(interval: TimeInterval) -> Self {
        Self(interval)
    }

    /// Returns the underlying half-open interval.
    #[must_use]
    pub const fn interval(self) -> TimeInterval {
        self.0
    }

    /// Returns whether the validity includes the supplied world time.
    pub fn contains(self, time: WorldTime) -> Result<bool, TemporalError> {
        self.0.contains(time)
    }
}

/// The time carried by an event, either one instant or a half-open span.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventTime {
    /// One instant on a timeline.
    Instant(WorldTime),
    /// A span beginning at `start`; `None` means it is still open.
    Span {
        /// Inclusive start coordinate.
        start: WorldTime,
        /// Exclusive end coordinate, or absent while the span is open.
        end: Option<WorldTime>,
    },
}

impl EventTime {
    /// Creates a half-open event span; closed spans must have `start < end`.
    pub fn span(start: WorldTime, end: Option<WorldTime>) -> Result<Self, TemporalError> {
        if let Some(end) = end {
            ensure_same_timeline(start.timeline, end.timeline)?;
            if start.nanoseconds >= end.nanoseconds {
                return Err(TemporalError::NonPositiveEventSpan);
            }
        }
        Ok(Self::Span { start, end })
    }

    /// Returns whether this event time contains the supplied coordinate.
    pub fn contains(self, time: WorldTime) -> Result<bool, TemporalError> {
        match self {
            Self::Instant(instant) => {
                ensure_same_timeline(instant.timeline, time.timeline)?;
                Ok(instant.nanoseconds == time.nanoseconds)
            }
            Self::Span { start, end } => {
                let interval = TimeInterval::new(start.timeline, Some(start), end)?;
                interval.contains(time)
            }
        }
    }
}

/// Invalid timeline comparison, interval, or checked time arithmetic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TemporalError {
    /// Values belong to distinct timeline identities and cannot be compared.
    IncomparableTimelines {
        /// Timeline required by the operation.
        expected: TimelineId,
        /// Timeline found on the supplied value.
        actual: TimelineId,
    },
    /// A finite interval has a start after its end.
    StartAfterEnd,
    /// A closed event span requires `start < end`.
    NonPositiveEventSpan,
    /// Nanosecond arithmetic exceeded the signed 128-bit range.
    Overflow,
}

impl fmt::Display for TemporalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncomparableTimelines { expected, actual } => write!(
                formatter,
                "time belongs to timeline {actual}, expected timeline {expected}"
            ),
            Self::StartAfterEnd => formatter.write_str("interval start is after its end"),
            Self::NonPositiveEventSpan => {
                formatter.write_str("closed event span requires start before end")
            }
            Self::Overflow => formatter.write_str("signed nanosecond arithmetic overflowed"),
        }
    }
}

impl std::error::Error for TemporalError {}

fn ensure_same_timeline(expected: Timeline, actual: Timeline) -> Result<(), TemporalError> {
    if expected.id() == actual.id() {
        Ok(())
    } else {
        Err(TemporalError::IncomparableTimelines {
            expected: expected.id(),
            actual: actual.id(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AssertionValidity, Duration, EventTime, Ordering, RecordedAsOf, TemporalError,
        TimeInterval, Timeline, WorldTime,
    };
    use crate::ids::{Revision, TimelineId};
    use std::str::FromStr;

    fn timeline(value: &str) -> Option<Timeline> {
        TimelineId::from_str(value).ok().map(Timeline::new)
    }

    #[test]
    fn recorded_as_of_is_bound_to_the_revision_axis() {
        let recorded = RecordedAsOf::from_published_revision(Revision::GENESIS);
        assert_eq!(recorded.revision(), Revision::GENESIS);
    }

    #[test]
    fn world_time_refuses_implicit_cross_timeline_order() {
        let first = timeline("00000000-0000-7000-8000-000000000001");
        let second = timeline("00000000-0000-7000-8000-000000000002");
        assert!(first.is_some());
        assert!(second.is_some());
        if let (Some(first), Some(second)) = (first, second) {
            let left = WorldTime::from_nanoseconds(first, 100);
            let right = WorldTime::from_nanoseconds(second, 200);
            assert!(matches!(
                left.checked_cmp(right),
                Err(TemporalError::IncomparableTimelines { .. })
            ));
        }
    }

    #[test]
    fn world_time_compares_explicitly_within_one_timeline() {
        let selected_timeline = timeline("00000000-0000-7000-8000-000000000001");
        assert!(selected_timeline.is_some());
        if let Some(selected_timeline) = selected_timeline {
            let earlier = WorldTime::from_nanoseconds(selected_timeline, -10);
            let later = WorldTime::from_nanoseconds(selected_timeline, 10);
            assert_eq!(earlier.checked_cmp(later), Ok(Ordering::Less));
        }
    }

    #[test]
    fn half_open_intervals_include_start_and_exclude_end() {
        let selected_timeline = timeline("00000000-0000-7000-8000-000000000001");
        assert!(selected_timeline.is_some());
        if let Some(selected_timeline) = selected_timeline {
            let start = WorldTime::from_nanoseconds(selected_timeline, 10);
            let end = WorldTime::from_nanoseconds(selected_timeline, 20);
            let interval = TimeInterval::new(selected_timeline, Some(start), Some(end));
            assert!(interval.is_ok());
            if let Ok(interval) = interval {
                assert_eq!(interval.contains(start), Ok(true));
                assert_eq!(
                    interval.contains(WorldTime::from_nanoseconds(selected_timeline, 19)),
                    Ok(true)
                );
                assert_eq!(interval.contains(end), Ok(false));
            }
            assert_eq!(
                TimeInterval::new(selected_timeline, Some(end), Some(start)),
                Err(TemporalError::StartAfterEnd)
            );
            let empty = TimeInterval::new(selected_timeline, Some(start), Some(start));
            assert!(empty.is_ok_and(TimeInterval::is_empty));
        }
    }

    #[test]
    fn interval_and_validity_reject_incompatible_timeline_values() {
        let first = timeline("00000000-0000-7000-8000-000000000001");
        let second = timeline("00000000-0000-7000-8000-000000000002");
        assert!(first.is_some());
        assert!(second.is_some());
        if let (Some(first), Some(second)) = (first, second) {
            let start = WorldTime::from_nanoseconds(first, 10);
            let end = WorldTime::from_nanoseconds(second, 20);
            assert!(matches!(
                TimeInterval::new(first, Some(start), Some(end)),
                Err(TemporalError::IncomparableTimelines { .. })
            ));

            let interval = TimeInterval::new(first, Some(start), None);
            assert!(interval.is_ok());
            if let Ok(interval) = interval {
                assert!(matches!(
                    AssertionValidity::new(interval).contains(end),
                    Err(TemporalError::IncomparableTimelines { .. })
                ));
            }
        }
    }

    #[test]
    fn event_time_distinguishes_instants_closed_spans_and_open_spans() {
        let selected_timeline = timeline("00000000-0000-7000-8000-000000000001");
        assert!(selected_timeline.is_some());
        if let Some(selected_timeline) = selected_timeline {
            let start = WorldTime::from_nanoseconds(selected_timeline, 10);
            let end = WorldTime::from_nanoseconds(selected_timeline, 20);
            let instant = EventTime::Instant(start);
            let closed = EventTime::span(start, Some(end));
            let open = EventTime::span(start, None);

            assert_eq!(instant.contains(start), Ok(true));
            assert_eq!(instant.contains(end), Ok(false));
            assert_eq!(
                closed,
                Ok(EventTime::Span {
                    start,
                    end: Some(end)
                })
            );
            assert_eq!(open, Ok(EventTime::Span { start, end: None }));
            if let Ok(closed) = closed {
                assert_eq!(closed.contains(start), Ok(true));
                assert_eq!(closed.contains(end), Ok(false));
            }
            assert_eq!(
                EventTime::span(start, Some(start)),
                Err(TemporalError::NonPositiveEventSpan)
            );
        }
    }

    #[test]
    fn duration_arithmetic_is_checked_and_preserves_timeline() {
        let selected_timeline = timeline("00000000-0000-7000-8000-000000000001");
        assert!(selected_timeline.is_some());
        if let Some(selected_timeline) = selected_timeline {
            let start = WorldTime::from_nanoseconds(selected_timeline, 100);
            assert_eq!(
                start.checked_add(Duration::from_nanoseconds(-30)),
                Ok(WorldTime::from_nanoseconds(selected_timeline, 70))
            );
            assert_eq!(
                WorldTime::from_nanoseconds(selected_timeline, i128::MAX)
                    .checked_add(Duration::from_nanoseconds(1)),
                Err(TemporalError::Overflow)
            );
        }
    }
}
