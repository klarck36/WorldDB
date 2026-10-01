//! Bounded, fail-open diagnostic events, separate from durable audit records.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, TryLockError};

/// Maximum fields accepted in one diagnostic event.
pub const MAX_DIAGNOSTIC_FIELDS: usize = 16;
/// Maximum configured queue entries, preserving a hard process-local memory bound.
pub const MAX_DIAGNOSTIC_QUEUE_CAPACITY: usize = 4096;

/// Closed stable span-name catalog. Names are wire-independent and versioned by code review.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticSpanName {
    QueryResolve,
    Search,
    GraphTraversal,
    Aggregate,
    TransactionCommit,
    SnapshotOpen,
    Recovery,
}

impl DiagnosticSpanName {
    /// Stable lowercase span name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueryResolve => "query.resolve",
            Self::Search => "query.search",
            Self::GraphTraversal => "query.graph_traversal",
            Self::Aggregate => "query.aggregate",
            Self::TransactionCommit => "transaction.commit",
            Self::SnapshotOpen => "snapshot.open",
            Self::Recovery => "storage.recovery",
        }
    }
}

/// Closed event-kind catalog; values never contain user-controlled strings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticEventKind {
    Started,
    Completed,
    Failed,
    Conflict,
    UnknownCommitOutcome,
    RecoveryAction,
    CorruptFrame,
    SnapshotPin,
    SecurityDenial,
}

/// Safe scalar codes permitted in explicitly shown diagnostics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticCode {
    BudgetExceeded,
    Cancelled,
    Conflict,
    CorruptFrame,
    Unauthorized,
    UnknownCommitOutcome,
}

/// Small allowlist of values safe to show after an explicit opt-in.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SafeDiagnosticValue {
    Count(u64),
    DurationMillis(u64),
    Flag(bool),
    Code(DiagnosticCode),
}

/// Caller-supplied stable diagnostic digest. Raw inputs are never retained in this type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StableDiagnosticHash([u8; 16]);

impl StableDiagnosticHash {
    /// Wraps an already computed stable digest; the caller must not pass raw data bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns the fixed-size digest bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}

/// Closed classification for one diagnostic field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticRedaction {
    /// Default for every field not explicitly classified.
    Omitted,
    /// An allowlisted scalar explicitly approved for display.
    Shown(SafeDiagnosticValue),
    /// An explicitly approved fixed-size digest.
    Hashed(StableDiagnosticHash),
}

/// Closed diagnostic field-name catalog.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticFieldKey {
    Count,
    DurationMillis,
    ErrorCode,
    LayerClass,
    OperationClass,
    ResultClass,
    RetryClass,
    StableHash,
}

/// Redacted field. Constructing a field without an explicit classification always omits it.
/// Raw strings, query text, paths, and principal names cannot be supplied as shown values.
///
/// ```compile_fail
/// use worlddb_core::{DiagnosticField, DiagnosticFieldKey};
/// let _ = DiagnosticField::shown(DiagnosticFieldKey::ErrorCode, "secret query text".to_owned());
/// ```
///
/// There is no public RAII span guard that can be accidentally retained across an async wait:
///
/// ```compile_fail
/// use worlddb_core::DiagnosticSpanGuard;
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DiagnosticField {
    key: DiagnosticFieldKey,
    redaction: DiagnosticRedaction,
}

impl DiagnosticField {
    /// Creates a field using the required `Omitted` default.
    #[must_use]
    pub const fn omitted(key: DiagnosticFieldKey) -> Self {
        Self {
            key,
            redaction: DiagnosticRedaction::Omitted,
        }
    }

    /// Explicitly displays an allowlisted scalar value.
    #[must_use]
    pub const fn shown(key: DiagnosticFieldKey, value: SafeDiagnosticValue) -> Self {
        Self {
            key,
            redaction: DiagnosticRedaction::Shown(value),
        }
    }

    /// Explicitly records a fixed-size precomputed stable digest.
    #[must_use]
    pub const fn hashed(key: DiagnosticFieldKey, value: StableDiagnosticHash) -> Self {
        Self {
            key,
            redaction: DiagnosticRedaction::Hashed(value),
        }
    }

    /// Field classification key.
    #[must_use]
    pub const fn key(self) -> DiagnosticFieldKey {
        self.key
    }

    /// Applied redaction decision.
    #[must_use]
    pub const fn redaction(self) -> DiagnosticRedaction {
        self.redaction
    }
}

/// Immutable event made only of stable names, closed kinds, and redacted fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticEvent {
    span: DiagnosticSpanName,
    kind: DiagnosticEventKind,
    fields: Vec<DiagnosticField>,
}

impl DiagnosticEvent {
    /// Creates a canonically ordered event with unique fields and a fixed field-count bound.
    pub fn new(
        span: DiagnosticSpanName,
        kind: DiagnosticEventKind,
        mut fields: Vec<DiagnosticField>,
    ) -> Result<Self, DiagnosticEventError> {
        if fields.len() > MAX_DIAGNOSTIC_FIELDS {
            return Err(DiagnosticEventError::TooManyFields);
        }
        fields.sort_by_key(|field| field.key);
        if fields.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left.key == right.key)
        }) {
            return Err(DiagnosticEventError::DuplicateField);
        }
        Ok(Self { span, kind, fields })
    }

    /// Stable operation span.
    #[must_use]
    pub const fn span(&self) -> DiagnosticSpanName {
        self.span
    }

    /// Closed event kind.
    #[must_use]
    pub const fn kind(&self) -> DiagnosticEventKind {
        self.kind
    }

    /// Canonically ordered, redacted fields.
    #[must_use]
    pub fn fields(&self) -> &[DiagnosticField] {
        &self.fields
    }
}

/// Invalid event field shape.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticEventError {
    DuplicateField,
    TooManyFields,
}

impl fmt::Display for DiagnosticEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateField => "diagnostic event has a duplicate field",
            Self::TooManyFields => "diagnostic event exceeds its field limit",
        })
    }
}

impl std::error::Error for DiagnosticEventError {}

/// Safe aggregated counter catalog.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(usize)]
pub enum DiagnosticCounter {
    Conflicts = 0,
    UnknownCommitOutcomes = 1,
    RecoveryActions = 2,
    CorruptFrames = 3,
    SnapshotPins = 4,
    DroppedTelemetry = 5,
    SecurityDenials = 6,
}

const COUNTER_COUNT: usize = 7;

/// Fail-open diagnostic port. Reporting has no return value and cannot veto domain work.
pub trait DiagnosticPort: Send + Sync {
    /// Attempts one nonblocking enqueue. Full, contended, or poisoned queues are dropped/counts.
    fn report(&self, event: DiagnosticEvent);

    /// Increments one safe aggregate counter using saturating arithmetic.
    fn increment(&self, counter: DiagnosticCounter);
}

/// Bounded in-memory diagnostic queue with nonblocking producers and drop counters.
pub struct BoundedDiagnostics {
    capacity: usize,
    events: Mutex<VecDeque<DiagnosticEvent>>,
    counters: [AtomicU64; COUNTER_COUNT],
}

impl BoundedDiagnostics {
    /// Creates a bounded queue; zero capacity is rejected.
    pub fn new(capacity: usize) -> Result<Self, DiagnosticQueueError> {
        if capacity == 0 {
            return Err(DiagnosticQueueError::ZeroCapacity);
        }
        if capacity > MAX_DIAGNOSTIC_QUEUE_CAPACITY {
            return Err(DiagnosticQueueError::CapacityExceeded);
        }
        Ok(Self {
            capacity,
            events: Mutex::new(VecDeque::with_capacity(capacity)),
            counters: std::array::from_fn(|_| AtomicU64::new(0)),
        })
    }

    /// Returns one aggregate counter, or `None` only for an invalid internal index.
    #[must_use]
    pub fn counter(&self, counter: DiagnosticCounter) -> u64 {
        self.counters
            .get(counter as usize)
            .map_or(0, |value| value.load(Ordering::Relaxed))
    }

    /// Drains up to `limit` events for a background exporter.
    ///
    /// This is an exporter path and may wait for the queue lock. Domain reporting never calls it.
    pub fn drain(&self, limit: usize) -> Vec<DiagnosticEvent> {
        let Ok(mut events) = self.events.lock() else {
            return Vec::new();
        };
        let take = limit.min(events.len());
        events.drain(..take).collect()
    }

    fn increment_saturating(&self, counter: DiagnosticCounter) {
        if let Some(counter) = self.counters.get(counter as usize) {
            let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(1))
            });
        }
    }
}

impl DiagnosticPort for BoundedDiagnostics {
    fn report(&self, event: DiagnosticEvent) {
        match self.events.try_lock() {
            Ok(mut events) if events.len() < self.capacity => events.push_back(event),
            Ok(_) | Err(TryLockError::WouldBlock) | Err(TryLockError::Poisoned(_)) => {
                self.increment_saturating(DiagnosticCounter::DroppedTelemetry);
            }
        }
    }

    fn increment(&self, counter: DiagnosticCounter) {
        self.increment_saturating(counter);
    }
}

/// Invalid queue configuration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticQueueError {
    ZeroCapacity,
    CapacityExceeded,
}

impl fmt::Display for DiagnosticQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroCapacity => "diagnostic queue capacity must be positive",
            Self::CapacityExceeded => "diagnostic queue capacity exceeds the configured maximum",
        })
    }
}

impl std::error::Error for DiagnosticQueueError {}

#[cfg(test)]
mod tests {
    use super::{
        BoundedDiagnostics, DiagnosticCode, DiagnosticCounter, DiagnosticEvent,
        DiagnosticEventError, DiagnosticEventKind, DiagnosticField, DiagnosticFieldKey,
        DiagnosticPort, DiagnosticQueueError, DiagnosticRedaction, DiagnosticSpanName,
        MAX_DIAGNOSTIC_FIELDS, MAX_DIAGNOSTIC_QUEUE_CAPACITY, SafeDiagnosticValue,
        StableDiagnosticHash,
    };
    use std::sync::atomic::Ordering;

    fn event(kind: DiagnosticEventKind) -> Result<DiagnosticEvent, DiagnosticEventError> {
        DiagnosticEvent::new(
            DiagnosticSpanName::QueryResolve,
            kind,
            vec![
                DiagnosticField::omitted(DiagnosticFieldKey::OperationClass),
                DiagnosticField::shown(DiagnosticFieldKey::Count, SafeDiagnosticValue::Count(2)),
            ],
        )
    }

    #[test]
    fn event_names_are_stable_and_unknown_fields_default_to_omitted() -> Result<(), String> {
        let event = event(DiagnosticEventKind::Completed).map_err(|error| error.to_string())?;
        if event.span().as_str() != "query.resolve" {
            return Err("span name changed unexpectedly".to_owned());
        }
        if event
            .fields()
            .iter()
            .find(|field| field.key() == DiagnosticFieldKey::OperationClass)
            .map(|field| field.redaction())
            != Some(DiagnosticRedaction::Omitted)
        {
            return Err("unclassified diagnostic field was not omitted".to_owned());
        }
        Ok(())
    }

    #[test]
    fn shown_and_hashed_values_are_explicit_and_events_are_bounded() {
        let hashed = DiagnosticField::hashed(
            DiagnosticFieldKey::StableHash,
            StableDiagnosticHash::new([7; 16]),
        );
        assert_eq!(
            hashed.redaction(),
            DiagnosticRedaction::Hashed(StableDiagnosticHash::new([7; 16]))
        );
        assert_eq!(
            DiagnosticField::shown(
                DiagnosticFieldKey::ErrorCode,
                SafeDiagnosticValue::Code(DiagnosticCode::Unauthorized)
            )
            .redaction(),
            DiagnosticRedaction::Shown(SafeDiagnosticValue::Code(DiagnosticCode::Unauthorized))
        );
        let too_many =
            vec![DiagnosticField::omitted(DiagnosticFieldKey::Count); MAX_DIAGNOSTIC_FIELDS + 1];
        assert_eq!(
            DiagnosticEvent::new(
                DiagnosticSpanName::Search,
                DiagnosticEventKind::Started,
                too_many,
            ),
            Err(DiagnosticEventError::TooManyFields)
        );
        assert_eq!(
            DiagnosticEvent::new(
                DiagnosticSpanName::Search,
                DiagnosticEventKind::Started,
                vec![
                    DiagnosticField::omitted(DiagnosticFieldKey::Count),
                    DiagnosticField::omitted(DiagnosticFieldKey::Count),
                ],
            ),
            Err(DiagnosticEventError::DuplicateField)
        );
    }

    #[test]
    fn queue_is_bounded_nonblocking_and_counts_drops_without_vetoing_work() -> Result<(), String> {
        assert_eq!(
            BoundedDiagnostics::new(0).map(|_| ()),
            Err(DiagnosticQueueError::ZeroCapacity)
        );
        assert_eq!(
            BoundedDiagnostics::new(MAX_DIAGNOSTIC_QUEUE_CAPACITY + 1).map(|_| ()),
            Err(DiagnosticQueueError::CapacityExceeded)
        );
        let queue = BoundedDiagnostics::new(1).map_err(|error| error.to_string())?;
        queue.report(event(DiagnosticEventKind::Started).map_err(|error| error.to_string())?);
        queue.report(event(DiagnosticEventKind::Completed).map_err(|error| error.to_string())?);
        queue.increment(DiagnosticCounter::Conflicts);
        queue.increment(DiagnosticCounter::UnknownCommitOutcomes);
        queue.increment(DiagnosticCounter::RecoveryActions);
        queue.increment(DiagnosticCounter::CorruptFrames);
        queue.increment(DiagnosticCounter::SnapshotPins);
        queue.increment(DiagnosticCounter::SecurityDenials);
        let domain_operation_result = Ok::<u8, &'static str>(7);
        if domain_operation_result != Ok(7)
            || queue.counter(DiagnosticCounter::DroppedTelemetry) != 1
            || queue.counter(DiagnosticCounter::Conflicts) != 1
            || queue.counter(DiagnosticCounter::UnknownCommitOutcomes) != 1
            || queue.counter(DiagnosticCounter::RecoveryActions) != 1
            || queue.counter(DiagnosticCounter::CorruptFrames) != 1
            || queue.counter(DiagnosticCounter::SnapshotPins) != 1
            || queue.counter(DiagnosticCounter::SecurityDenials) != 1
            || queue.drain(10).len() != 1
        {
            return Err(
                "diagnostic drop blocked work or counters/queue exceeded bounds".to_owned(),
            );
        }
        Ok(())
    }

    #[test]
    fn lock_contention_drops_immediately_and_saturates_counters() -> Result<(), String> {
        let queue = BoundedDiagnostics::new(2).map_err(|error| error.to_string())?;
        if let Some(counter) = queue
            .counters
            .get(DiagnosticCounter::DroppedTelemetry as usize)
        {
            counter.store(u64::MAX, Ordering::Relaxed);
        } else {
            return Err("dropped telemetry counter is missing".to_owned());
        }
        let _guard = queue.events.lock().map_err(|error| error.to_string())?;
        queue.report(event(DiagnosticEventKind::Failed).map_err(|error| error.to_string())?);
        if queue.counter(DiagnosticCounter::DroppedTelemetry) != u64::MAX {
            return Err("contended drop counter did not saturate".to_owned());
        }
        Ok(())
    }
}
