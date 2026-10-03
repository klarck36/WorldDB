//! Versioned, storage-independent public Rust facade.
//!
//! Only the types in a numbered `api` module form the supported application
//! boundary. The crate root retains compatibility exports for existing
//! internal adapters; those exports are not part of this facade.
//!
//! The facade deliberately has no Storage trait, lock, filesystem path,
//! authenticated Principal, Capability decision, or secret-bearing type.
//!
//! ```compile_fail
//! use worlddb_core::api::v1::RevisionBackend;
//! ```
//!
//! ```compile_fail
//! use worlddb_core::api::v1::{CursorStateStore, SecurityContext};
//! ```
//!
//! ```compile_fail
//! use worlddb_core::api::v1::QueryContextDto;
//! fn caller_cannot_supply_identity(context: QueryContextDto) {
//!     let _principal = context.principal_id;
//! }
//! ```

/// Stable application DTOs and engine operation boundary for protocol 1.x.
pub mod v1 {
    use std::cmp::Ordering;
    use std::fmt;
    use std::sync::Arc;

    use crate::wire::encode_value;
    use crate::wire_records::encode_record_ref;

    pub use crate::context::{EpistemicMode, PerspectiveScope};
    pub use crate::ids::{
        ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
        DomainId, EntityId, EntityRetirementId, EventAttributeId, EventId, EventKindId,
        EventMaskId, EventMaskRetractionId, EventRelationId, EventRelationRetractionId,
        EventRetractionId, EventRoleId, EventSpanClosureId, EvidenceId, EvidenceRetractionId,
        HistorySpaceId, JobId, LayerId, MaskId, MaskRetractionId, MaskValidityClosureId,
        OperationId, PerspectiveId, PerspectiveRetirementId, PredicateId, ProvenanceId,
        ProvenanceRetractionId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
        ReplacementBoundaryValidityClosureId, Revision, SchemaRevision, SnapshotId, SourceId,
        TimelineId, TransferLineageId,
    };
    pub use crate::layers::LayerSelection;
    pub use crate::query_aggregate::{
        AggregateError, AggregateGroupValue, AggregateResult, AggregateSpec, GroupValueKey,
        GroupedCountRow,
    };
    pub use crate::query_graph::{
        GraphCyclePolicy, GraphDirection, GraphError, GraphRelationshipKind, GraphResult,
        GraphSpec, TraversedGraphEdge,
    };
    pub use crate::query_search::{
        QuerySearchError, SearchHit, SearchMatch, SearchSpec, SearchToken,
    };
    pub use crate::record_refs::RecordRef;
    pub use crate::reference_query::{
        ExplainStage, ExplainStageKind, RawHistoryRow, ReferenceExplain, ResolvedOutcome,
        ResolvedView,
    };
    pub use crate::schema::{CalendarPeriod, NonEmptySet, SchemaDefinitionError};
    pub use crate::schema_history::SchemaMode;
    pub use crate::values::{Time, Value};
    pub use crate::wire_records::{Record, RecordKind};
    pub use crate::{
        AuthorizationMode, Decimal, FieldSelector, Int, QueryBudget, QueryBudgetError,
        QueryBudgetLimits, RetryHint, SnapshotSelector, Subject, Symbol, SymbolError, UInt,
        WorldTime, WorldTimeSelector,
    };

    /// Current wire and Rust-facade protocol version.
    pub const CURRENT_PROTOCOL: ProtocolVersion = ProtocolVersion::new(1, 0);
    /// Cursor-token wire length fixed by the session-local cursor contract.
    pub const CURSOR_TOKEN_BYTES: usize = 73;
    const MAX_FILTER_DEPTH: u8 = 32;
    const MAX_FILTER_TESTS: u16 = 256;
    const MAX_SORT_TERMS: usize = 8;

    /// Protocol version carried by every request and response envelope.
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct ProtocolVersion {
        major: u16,
        minor: u16,
    }

    impl ProtocolVersion {
        /// Creates a protocol version from its numeric components.
        #[must_use]
        pub const fn new(major: u16, minor: u16) -> Self {
            Self { major, minor }
        }

        /// Major protocol component.
        #[must_use]
        pub const fn major(self) -> u16 {
            self.major
        }

        /// Minor protocol component.
        #[must_use]
        pub const fn minor(self) -> u16 {
            self.minor
        }
    }

    /// Opaque request correlation identity in canonical 16-byte UUID form.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct RequestId([u8; 16]);

    impl RequestId {
        /// Wraps the exact UUID bytes; the value is correlation only and grants no authority.
        #[must_use]
        pub const fn from_bytes(bytes: [u8; 16]) -> Self {
            Self(bytes)
        }

        /// Returns the canonical network-order UUID bytes.
        #[must_use]
        pub const fn as_bytes(self) -> [u8; 16] {
            self.0
        }
    }

    /// Optional protocol capabilities negotiated at the boundary.
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub enum ProtocolFeature {
        /// Closed boolean full-text query expressions.
        FullTextSearch,
    }

    /// Nonempty set of versions and optional required capabilities for negotiation.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct HandshakeRequest {
        request_id: RequestId,
        supported_versions: NonEmptySet<ProtocolVersion>,
        required_features: Vec<ProtocolFeature>,
    }

    impl HandshakeRequest {
        /// Creates a canonical handshake request.
        pub fn new(
            request_id: RequestId,
            versions: Vec<ProtocolVersion>,
            mut required_features: Vec<ProtocolFeature>,
        ) -> Result<Self, ApiValidationError> {
            let supported_versions = NonEmptySet::new(versions)
                .map_err(|_| ApiValidationError::EmptySupportedVersions)?;
            required_features.sort_unstable();
            if required_features
                .windows(2)
                .any(|pair| pair.first() == pair.get(1))
            {
                return Err(ApiValidationError::DuplicateProtocolFeature);
            }
            Ok(Self {
                request_id,
                supported_versions,
                required_features,
            })
        }

        /// Correlation identity for the handshake response.
        #[must_use]
        pub const fn request_id(&self) -> RequestId {
            self.request_id
        }

        /// Canonically ordered nonempty version set.
        #[must_use]
        pub fn supported_versions(&self) -> &[ProtocolVersion] {
            self.supported_versions.as_slice()
        }

        /// Required features in canonical order.
        #[must_use]
        pub fn required_features(&self) -> &[ProtocolFeature] {
            &self.required_features
        }
    }

    /// Successful protocol and feature selection.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct HandshakeResponse {
        request_id: RequestId,
        selected_version: ProtocolVersion,
        supported_features: Vec<ProtocolFeature>,
    }

    impl HandshakeResponse {
        /// Negotiates the highest common version if all required features are supported.
        pub fn negotiate(
            request: &HandshakeRequest,
            server_versions: &[ProtocolVersion],
            server_features: &[ProtocolFeature],
        ) -> Result<Self, ApiValidationError> {
            let selected_version = request
                .supported_versions
                .as_slice()
                .iter()
                .copied()
                .filter(|version| server_versions.contains(version))
                .max()
                .ok_or(ApiValidationError::NoCommonProtocolVersion)?;
            if request
                .required_features
                .iter()
                .any(|feature| !server_features.contains(feature))
            {
                return Err(ApiValidationError::RequiredFeatureUnavailable);
            }
            let mut supported_features = server_features.to_vec();
            supported_features.sort_unstable();
            supported_features.dedup();
            Ok(Self {
                request_id: request.request_id,
                selected_version,
                supported_features,
            })
        }

        /// Correlation identity copied from the request.
        #[must_use]
        pub const fn request_id(&self) -> RequestId {
            self.request_id
        }

        /// Selected protocol version.
        #[must_use]
        pub const fn selected_version(&self) -> ProtocolVersion {
            self.selected_version
        }

        /// Capabilities the server can use at the selected version.
        #[must_use]
        pub fn supported_features(&self) -> &[ProtocolFeature] {
            &self.supported_features
        }
    }

    /// Caller-selectable semantic axes. Authentication and cancellation are host-bound.
    #[derive(Clone, Debug)]
    pub struct QueryContextDto {
        snapshot: crate::SnapshotSelector,
        recorded_as_of: Revision,
        history_space: HistorySpaceId,
        layers: LayerSelection,
        world_time: crate::WorldTimeSelector,
        perspective: PerspectiveScope,
        epistemic_mode: EpistemicMode,
        schema_mode: SchemaMode,
        authorization_mode: AuthorizationMode,
        budget: QueryBudget,
    }

    /// Required query-context input fields before partition validation.
    #[derive(Clone, Debug)]
    pub struct QueryContextDtoInput {
        /// Requested snapshot selector.
        pub snapshot: SnapshotSelector,
        /// Transaction-time read point.
        pub recorded_as_of: Revision,
        /// Selected HistorySpace.
        pub history_space: HistorySpaceId,
        /// Typed layer selection.
        pub layers: LayerSelection,
        /// Explicit World-Time selector.
        pub world_time: WorldTimeSelector,
        /// Perspective partition.
        pub perspective: PerspectiveScope,
        /// Epistemic partition.
        pub epistemic_mode: EpistemicMode,
        /// Schema interpretation.
        pub schema_mode: SchemaMode,
        /// Authorization time basis; no identity or capability is accepted.
        pub authorization_mode: AuthorizationMode,
        /// Explicit positive finite semantic budget.
        pub budget: QueryBudget,
    }

    impl QueryContextDto {
        /// Binds every semantic query dimension without accepting caller identity or privileges.
        pub fn new(input: QueryContextDtoInput) -> Result<Self, ApiValidationError> {
            let QueryContextDtoInput {
                snapshot,
                recorded_as_of,
                history_space,
                layers,
                world_time,
                perspective,
                epistemic_mode,
                schema_mode,
                authorization_mode,
                budget,
            } = input;
            let valid_partition = matches!(
                (perspective, epistemic_mode),
                (PerspectiveScope::World, EpistemicMode::WorldState)
                    | (
                        PerspectiveScope::Perspective(_),
                        EpistemicMode::Knows | EpistemicMode::Believes | EpistemicMode::Claims
                    )
            );
            if !valid_partition {
                return Err(ApiValidationError::InvalidPerspectiveEpistemicPair);
            }
            Ok(Self {
                snapshot,
                recorded_as_of,
                history_space,
                layers,
                world_time,
                perspective,
                epistemic_mode,
                schema_mode,
                authorization_mode,
                budget,
            })
        }

        /// Requested snapshot; `Current` must be resolved once by the trusted engine session.
        #[must_use]
        pub const fn snapshot(&self) -> crate::SnapshotSelector {
            self.snapshot
        }

        /// Transaction-time read point.
        #[must_use]
        pub const fn recorded_as_of(&self) -> Revision {
            self.recorded_as_of
        }

        /// Selected HistorySpace.
        #[must_use]
        pub const fn history_space(&self) -> HistorySpaceId {
            self.history_space
        }

        /// Typed layer selection to be validated against the pinned schema.
        #[must_use]
        pub const fn layers(&self) -> &LayerSelection {
            &self.layers
        }

        /// Explicit world-time selector.
        #[must_use]
        pub const fn world_time(&self) -> crate::WorldTimeSelector {
            self.world_time
        }

        /// Perspective partition.
        #[must_use]
        pub const fn perspective(&self) -> PerspectiveScope {
            self.perspective
        }

        /// Epistemic partition.
        #[must_use]
        pub const fn epistemic_mode(&self) -> EpistemicMode {
            self.epistemic_mode
        }

        /// Schema interpretation.
        #[must_use]
        pub const fn schema_mode(&self) -> SchemaMode {
            self.schema_mode
        }

        /// Authorization time basis; authenticated identity remains host-bound.
        #[must_use]
        pub const fn authorization_mode(&self) -> AuthorizationMode {
            self.authorization_mode
        }

        /// Explicit finite semantic budget.
        #[must_use]
        pub const fn budget(&self) -> QueryBudget {
            self.budget
        }
    }

    /// Inclusive or exclusive edge for revision and scalar bounds.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum BoundEdge {
        /// The endpoint is part of the range.
        Inclusive,
        /// The endpoint is excluded from the range.
        Exclusive,
    }

    /// One typed transaction-time bound.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct RevisionBound {
        value: Revision,
        edge: BoundEdge,
    }

    impl RevisionBound {
        /// Creates a bound at one published-history revision.
        #[must_use]
        pub const fn new(value: Revision, edge: BoundEdge) -> Self {
            Self { value, edge }
        }

        /// Bound revision.
        #[must_use]
        pub const fn value(self) -> Revision {
            self.value
        }

        /// Bound inclusion rule.
        #[must_use]
        pub const fn edge(self) -> BoundEdge {
            self.edge
        }
    }

    /// Nonempty, checked revision interval.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct RevisionRange {
        lower: Option<RevisionBound>,
        upper: Option<RevisionBound>,
    }

    impl RevisionRange {
        /// Creates a nonempty interval with explicit inclusive/exclusive endpoints.
        pub fn new(
            lower: Option<RevisionBound>,
            upper: Option<RevisionBound>,
        ) -> Result<Self, ApiValidationError> {
            match (lower, upper) {
                (None, None) => Err(ApiValidationError::RangeNeedsBound),
                (Some(lower), Some(upper)) => {
                    let ordering = lower.value.cmp(&upper.value);
                    if ordering == Ordering::Greater
                        || (ordering == Ordering::Equal
                            && (lower.edge == BoundEdge::Exclusive
                                || upper.edge == BoundEdge::Exclusive))
                    {
                        return Err(ApiValidationError::EmptyOrReversedRange);
                    }
                    Ok(Self {
                        lower: Some(lower),
                        upper: Some(upper),
                    })
                }
                _ => Ok(Self { lower, upper }),
            }
        }

        /// Optional lower bound.
        #[must_use]
        pub const fn lower(self) -> Option<RevisionBound> {
            self.lower
        }

        /// Optional upper bound.
        #[must_use]
        pub const fn upper(self) -> Option<RevisionBound> {
            self.upper
        }
    }

    /// One scalar range endpoint. Kind/order checks use the pinned schema at execution.
    #[derive(Clone, Debug)]
    pub struct ValueBound {
        value: Value,
        edge: BoundEdge,
    }

    impl ValueBound {
        /// Creates a typed endpoint; comparison is deferred until schema resolution.
        #[must_use]
        pub const fn new(value: Value, edge: BoundEdge) -> Self {
            Self { value, edge }
        }

        /// Endpoint value.
        #[must_use]
        pub const fn value(&self) -> &Value {
            &self.value
        }

        /// Endpoint inclusion rule.
        #[must_use]
        pub const fn edge(&self) -> BoundEdge {
            self.edge
        }
    }

    /// Time-bearing field family admitted by the query filter contract.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum TimeFilterField {
        /// Assertion world-time validity.
        AssertionValidity,
        /// Event world-time span or instant.
        EventTime,
    }

    /// Direction for an explicitly anchored calendar-relative query window.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum CalendarDirection {
        /// `[shift(anchor, period, Past), anchor)`.
        Past,
        /// `[anchor, shift(anchor, period, Future))`.
        Future,
    }

    /// Absolute half-open query interval in one Timeline.
    #[derive(Clone, Debug)]
    pub struct AbsoluteTimeRange {
        start: Time,
        end: Time,
    }

    impl AbsoluteTimeRange {
        /// Creates `[start, end)`; the pinned schema resolves unit scales when needed.
        pub fn new(start: Time, end: Time) -> Result<Self, ApiValidationError> {
            if start.timeline_id() != end.timeline_id() {
                return Err(ApiValidationError::CrossTimelineRange);
            }
            if start.unit() == end.unit() && start.ticks() >= end.ticks() {
                return Err(ApiValidationError::EmptyOrReversedRange);
            }
            Ok(Self { start, end })
        }

        /// Inclusive start coordinate of the half-open interval.
        #[must_use]
        pub const fn start(&self) -> &Time {
            &self.start
        }

        /// Exclusive end coordinate of the half-open interval.
        #[must_use]
        pub const fn end(&self) -> &Time {
            &self.end
        }
    }

    /// Explicitly anchored relative time window; the host never supplies an implicit clock.
    #[derive(Clone, Debug)]
    pub struct CalendarRelativeWindow {
        anchor: Time,
        period: CalendarPeriod,
        direction: CalendarDirection,
    }

    impl CalendarRelativeWindow {
        /// Creates a nonempty calendar window resolved against the selected schema snapshot.
        pub fn new(
            anchor: Time,
            period: CalendarPeriod,
            direction: CalendarDirection,
        ) -> Result<Self, ApiValidationError> {
            if period.years() == 0 && period.months() == 0 && period.days() == 0 {
                return Err(ApiValidationError::EmptyCalendarPeriod);
            }
            Ok(Self {
                anchor,
                period,
                direction,
            })
        }

        /// Explicit calendar anchor.
        #[must_use]
        pub const fn anchor(&self) -> &Time {
            &self.anchor
        }

        /// Nonnegative canonical Gregorian period.
        #[must_use]
        pub const fn period(&self) -> CalendarPeriod {
            self.period
        }

        /// Past or future interval direction.
        #[must_use]
        pub const fn direction(&self) -> CalendarDirection {
            self.direction
        }
    }

    /// Time filter input accepted by the query contract.
    #[derive(Clone, Debug)]
    pub enum TimeWindow {
        /// Caller-supplied absolute interval.
        Absolute(AbsoluteTimeRange),
        /// Calendar-relative interval with an explicit anchor.
        CalendarRelative(CalendarRelativeWindow),
    }

    /// Closed AST test. Values never include arbitrary JSON or query text.
    #[derive(Clone, Debug)]
    pub enum FilterTest {
        /// Match one or more closed record kinds.
        RecordKindIn(NonEmptySet<RecordKind>),
        /// Match one or more exact typed record identities.
        RecordRefIn(NonEmptySet<RecordRef>),
        /// Match one exact entity subject.
        SubjectIs(crate::Subject),
        /// Match one exact predicate.
        PredicateIs(PredicateId),
        /// Match one exact event-kind identity.
        EventKindIs(EventKindId),
        /// Match a transaction-time revision interval.
        RecordedRevision(RevisionRange),
        /// Match overlap with an assertion-validity or event-time interval.
        TimeOverlaps {
            field: TimeFilterField,
            window: TimeWindow,
        },
        /// Require the selected field to be present.
        FieldExists(FieldSelector),
        /// Compare one exact typed scalar value.
        FieldEquals { field: FieldSelector, value: Value },
        /// Compare against one or two same-kind ordered bounds.
        FieldRange {
            field: FieldSelector,
            lower: Option<ValueBound>,
            upper: Option<ValueBound>,
        },
    }

    /// Public closed syntax used to build a [`FilterExpr`].
    #[derive(Clone, Debug)]
    pub enum FilterNode {
        /// The only explicit no-filter expression.
        MatchAll,
        /// All children must match; must be nonempty.
        All(Vec<FilterExpr>),
        /// At least one child must match; must be nonempty.
        Any(Vec<FilterExpr>),
        /// Negates exactly one expression.
        Not(Box<FilterExpr>),
        /// One typed leaf test.
        Test(FilterTest),
    }

    /// Validated, bounded, canonical filter AST.
    #[derive(Clone, Debug)]
    pub struct FilterExpr {
        node: FilterNode,
        depth: u8,
        test_count: u16,
    }

    impl FilterExpr {
        /// Creates and normalizes a closed filter expression.
        pub fn new(node: FilterNode) -> Result<Self, ApiValidationError> {
            match node {
                FilterNode::MatchAll => Ok(Self {
                    node: FilterNode::MatchAll,
                    depth: 1,
                    test_count: 0,
                }),
                FilterNode::Test(test) => Ok(Self {
                    node: {
                        let mut canonical = Vec::new();
                        write_filter_test(&mut canonical, &test)?;
                        FilterNode::Test(test)
                    },
                    depth: 1,
                    test_count: 1,
                }),
                FilterNode::Not(child) => {
                    let depth = child
                        .depth
                        .checked_add(1)
                        .ok_or(ApiValidationError::FilterTooDeep)?;
                    if depth > MAX_FILTER_DEPTH {
                        return Err(ApiValidationError::FilterTooDeep);
                    }
                    Ok(Self {
                        test_count: child.test_count,
                        node: FilterNode::Not(child),
                        depth,
                    })
                }
                FilterNode::All(children) => Self::new_group(children, true),
                FilterNode::Any(children) => Self::new_group(children, false),
            }
        }

        /// Returns a MatchAll expression.
        pub fn match_all() -> Self {
            Self {
                node: FilterNode::MatchAll,
                depth: 1,
                test_count: 0,
            }
        }

        /// Returns the immutable normalized AST.
        #[must_use]
        pub const fn node(&self) -> &FilterNode {
            &self.node
        }

        /// Maximum nesting depth including the root node.
        #[must_use]
        pub const fn depth(&self) -> u8 {
            self.depth
        }

        /// Number of leaf tests in the expression.
        #[must_use]
        pub const fn test_count(&self) -> u16 {
            self.test_count
        }

        fn new_group(children: Vec<Self>, all: bool) -> Result<Self, ApiValidationError> {
            if children.is_empty() {
                return Err(ApiValidationError::EmptyBooleanGroup);
            }
            let depth = children
                .iter()
                .map(|child| child.depth)
                .max()
                .ok_or(ApiValidationError::EmptyBooleanGroup)?
                .checked_add(1)
                .ok_or(ApiValidationError::FilterTooDeep)?;
            if depth > MAX_FILTER_DEPTH {
                return Err(ApiValidationError::FilterTooDeep);
            }
            let input_test_count = children.iter().try_fold(0_u16, |count, child| {
                count
                    .checked_add(child.test_count)
                    .ok_or(ApiValidationError::TooManyFilterTests)
            })?;
            if input_test_count > MAX_FILTER_TESTS {
                return Err(ApiValidationError::TooManyFilterTests);
            }
            let mut keyed = children
                .into_iter()
                .map(|child| Ok((child.canonical_bytes()?, child)))
                .collect::<Result<Vec<_>, ApiValidationError>>()?;
            keyed.sort_by(|left, right| left.0.cmp(&right.0));
            keyed.dedup_by(|left, right| left.0 == right.0);
            let test_count = keyed.iter().try_fold(0_u16, |count, (_, child)| {
                count
                    .checked_add(child.test_count)
                    .ok_or(ApiValidationError::TooManyFilterTests)
            })?;
            let canonical_children = keyed.into_iter().map(|(_, child)| child).collect();
            Ok(Self {
                node: if all {
                    FilterNode::All(canonical_children)
                } else {
                    FilterNode::Any(canonical_children)
                },
                depth,
                test_count,
            })
        }

        fn canonical_bytes(&self) -> Result<Vec<u8>, ApiValidationError> {
            let mut bytes = Vec::new();
            self.write_canonical(&mut bytes)?;
            Ok(bytes)
        }

        fn write_canonical(&self, bytes: &mut Vec<u8>) -> Result<(), ApiValidationError> {
            match &self.node {
                FilterNode::MatchAll => bytes.push(0),
                FilterNode::All(children) | FilterNode::Any(children) => {
                    bytes.push(if matches!(&self.node, FilterNode::All(_)) {
                        1
                    } else {
                        2
                    });
                    write_len(bytes, children.len())?;
                    for child in children {
                        let child_bytes = child.canonical_bytes()?;
                        write_bytes(bytes, &child_bytes)?;
                    }
                }
                FilterNode::Not(child) => {
                    bytes.push(3);
                    child.write_canonical(bytes)?;
                }
                FilterNode::Test(test) => {
                    bytes.push(4);
                    write_filter_test(bytes, test)?;
                }
            }
            Ok(())
        }
    }

    fn write_filter_test(bytes: &mut Vec<u8>, test: &FilterTest) -> Result<(), ApiValidationError> {
        match test {
            FilterTest::RecordKindIn(kinds) => {
                bytes.push(0);
                write_len(bytes, kinds.as_slice().len())?;
                for kind in kinds.as_slice() {
                    bytes.extend_from_slice(&(*kind as u32).to_be_bytes());
                }
            }
            FilterTest::RecordRefIn(references) => {
                bytes.push(1);
                write_len(bytes, references.as_slice().len())?;
                for reference in references.as_slice() {
                    let encoded = encode_record_ref(*reference)
                        .map_err(|_| ApiValidationError::InvalidTypedReference)?;
                    write_bytes(bytes, &encoded)?;
                }
            }
            FilterTest::SubjectIs(subject) => {
                bytes.push(2);
                bytes.extend_from_slice(&subject.entity_id().to_bytes());
            }
            FilterTest::PredicateIs(predicate) => {
                bytes.push(3);
                bytes.extend_from_slice(&predicate.to_bytes());
            }
            FilterTest::EventKindIs(kind) => {
                bytes.push(4);
                bytes.extend_from_slice(&kind.to_bytes());
            }
            FilterTest::RecordedRevision(range) => {
                bytes.push(5);
                write_revision_bound(bytes, range.lower);
                write_revision_bound(bytes, range.upper);
            }
            FilterTest::TimeOverlaps { field, window } => {
                bytes.push(6);
                bytes.push(match field {
                    TimeFilterField::AssertionValidity => 0,
                    TimeFilterField::EventTime => 1,
                });
                match window {
                    TimeWindow::Absolute(range) => {
                        bytes.push(0);
                        write_value(bytes, &Value::Time(range.start.clone()))?;
                        write_value(bytes, &Value::Time(range.end.clone()))?;
                    }
                    TimeWindow::CalendarRelative(window) => {
                        bytes.push(1);
                        write_value(bytes, &Value::Time(window.anchor.clone()))?;
                        bytes.extend_from_slice(&window.period.years().to_be_bytes());
                        bytes.push(window.period.months());
                        bytes.extend_from_slice(&window.period.days().to_be_bytes());
                        bytes.push(match window.direction {
                            CalendarDirection::Past => 0,
                            CalendarDirection::Future => 1,
                        });
                    }
                }
            }
            FilterTest::FieldExists(field) => {
                bytes.push(7);
                write_field_selector(bytes, *field);
            }
            FilterTest::FieldEquals { field, value } => {
                bytes.push(8);
                write_field_selector(bytes, *field);
                write_value(bytes, value)?;
            }
            FilterTest::FieldRange {
                field,
                lower,
                upper,
            } => {
                if lower.is_none() && upper.is_none() {
                    return Err(ApiValidationError::RangeNeedsBound);
                }
                bytes.push(9);
                write_field_selector(bytes, *field);
                write_value_bound(bytes, lower.as_ref())?;
                write_value_bound(bytes, upper.as_ref())?;
            }
        }
        Ok(())
    }

    fn write_field_selector(bytes: &mut Vec<u8>, field: FieldSelector) {
        macro_rules! tag {
            ($tag:expr) => {{
                bytes.push($tag);
            }};
            ($tag:expr, $id:expr) => {{
                bytes.push($tag);
                bytes.extend_from_slice(&$id.to_bytes());
            }};
            ($tag:expr, $first:expr, $second:expr) => {{
                bytes.push($tag);
                bytes.extend_from_slice(&$first.to_bytes());
                bytes.extend_from_slice(&$second.to_bytes());
            }};
        }
        match field {
            FieldSelector::AssertionSubject => tag!(0),
            FieldSelector::AssertionPredicate => tag!(1),
            FieldSelector::AssertionValue(id) => tag!(2, id),
            FieldSelector::AssertionPolarity => tag!(3),
            FieldSelector::AssertionValidity => tag!(4),
            FieldSelector::AssertionPerspective => tag!(5),
            FieldSelector::AssertionEpistemicMode => tag!(6),
            FieldSelector::MaskSelector => tag!(7),
            FieldSelector::MaskValidity => tag!(8),
            FieldSelector::ReplacementBoundarySubject => tag!(9),
            FieldSelector::ReplacementBoundaryPredicate => tag!(10),
            FieldSelector::ReplacementBoundaryValidity => tag!(11),
            FieldSelector::EventKind => tag!(12),
            FieldSelector::EventParticipant(kind, role) => tag!(13, kind, role),
            FieldSelector::EventAttribute(kind, attribute) => tag!(14, kind, attribute),
            FieldSelector::EventTime(kind) => tag!(15, kind),
            FieldSelector::EventMaskTarget => tag!(16),
            FieldSelector::SourceKind => tag!(17),
            FieldSelector::SourceLocator => tag!(18),
            FieldSelector::SourceContentDigest => tag!(19),
            FieldSelector::SourceMetadata => tag!(20),
        }
    }

    fn write_value(bytes: &mut Vec<u8>, value: &Value) -> Result<(), ApiValidationError> {
        let encoded = encode_value(value);
        write_bytes(bytes, &encoded)
    }

    fn write_revision_bound(bytes: &mut Vec<u8>, bound: Option<RevisionBound>) {
        match bound {
            None => bytes.push(0),
            Some(bound) => {
                bytes.push(1);
                bytes.extend_from_slice(&bound.value.value().to_be_bytes());
                bytes.push(match bound.edge {
                    BoundEdge::Inclusive => 0,
                    BoundEdge::Exclusive => 1,
                });
            }
        }
    }

    fn write_value_bound(
        bytes: &mut Vec<u8>,
        bound: Option<&ValueBound>,
    ) -> Result<(), ApiValidationError> {
        match bound {
            None => bytes.push(0),
            Some(bound) => {
                bytes.push(1);
                bytes.push(match bound.edge {
                    BoundEdge::Inclusive => 0,
                    BoundEdge::Exclusive => 1,
                });
                write_value(bytes, &bound.value)?;
            }
        }
        Ok(())
    }

    fn write_len(bytes: &mut Vec<u8>, length: usize) -> Result<(), ApiValidationError> {
        let length = u32::try_from(length).map_err(|_| ApiValidationError::RequestTooLarge)?;
        bytes.extend_from_slice(&length.to_be_bytes());
        Ok(())
    }

    fn write_bytes(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), ApiValidationError> {
        write_len(bytes, value.len())?;
        bytes.extend_from_slice(value);
        Ok(())
    }

    /// Closed boolean expression for optional full-text search.
    #[derive(Clone, Debug)]
    pub enum FullTextNode {
        /// Exact 1.0 token match.
        Token(SearchToken),
        /// Every child expression must match.
        All(Vec<FullTextExpr>),
        /// At least one child expression must match.
        Any(Vec<FullTextExpr>),
        /// Negates one child expression.
        Not(Box<FullTextExpr>),
    }

    /// Validated bounded full-text expression.
    #[derive(Clone, Debug)]
    pub struct FullTextExpr {
        node: FullTextNode,
        depth: u8,
        token_count: u16,
    }

    impl FullTextExpr {
        /// Creates a normalized expression with the filter depth and term bounds.
        pub fn new(node: FullTextNode) -> Result<Self, ApiValidationError> {
            match node {
                FullTextNode::Token(token) => Ok(Self {
                    node: FullTextNode::Token(token),
                    depth: 1,
                    token_count: 1,
                }),
                FullTextNode::Not(child) => {
                    let depth = child
                        .depth
                        .checked_add(1)
                        .ok_or(ApiValidationError::FilterTooDeep)?;
                    if depth > MAX_FILTER_DEPTH {
                        return Err(ApiValidationError::FilterTooDeep);
                    }
                    Ok(Self {
                        token_count: child.token_count,
                        node: FullTextNode::Not(child),
                        depth,
                    })
                }
                FullTextNode::All(children) => Self::new_group(children, true),
                FullTextNode::Any(children) => Self::new_group(children, false),
            }
        }

        /// Read-only expression syntax.
        #[must_use]
        pub const fn node(&self) -> &FullTextNode {
            &self.node
        }

        fn new_group(children: Vec<Self>, all: bool) -> Result<Self, ApiValidationError> {
            if children.is_empty() {
                return Err(ApiValidationError::EmptyBooleanGroup);
            }
            let depth = children
                .iter()
                .map(|child| child.depth)
                .max()
                .ok_or(ApiValidationError::EmptyBooleanGroup)?
                .checked_add(1)
                .ok_or(ApiValidationError::FilterTooDeep)?;
            let token_count = children.iter().try_fold(0_u16, |count, child| {
                count
                    .checked_add(child.token_count)
                    .ok_or(ApiValidationError::TooManyFilterTests)
            })?;
            if depth > MAX_FILTER_DEPTH || token_count > MAX_FILTER_TESTS {
                return Err(if depth > MAX_FILTER_DEPTH {
                    ApiValidationError::FilterTooDeep
                } else {
                    ApiValidationError::TooManyFilterTests
                });
            }
            let mut keyed = children
                .into_iter()
                .map(|child| Ok((child.canonical_bytes()?, child)))
                .collect::<Result<Vec<_>, ApiValidationError>>()?;
            keyed.sort_by(|left, right| left.0.cmp(&right.0));
            keyed.dedup_by(|left, right| left.0 == right.0);
            let token_count = keyed.iter().try_fold(0_u16, |count, (_, child)| {
                count
                    .checked_add(child.token_count)
                    .ok_or(ApiValidationError::TooManyFilterTests)
            })?;
            let children = keyed.into_iter().map(|(_, child)| child).collect();
            Ok(Self {
                node: if all {
                    FullTextNode::All(children)
                } else {
                    FullTextNode::Any(children)
                },
                depth,
                token_count,
            })
        }

        fn canonical_bytes(&self) -> Result<Vec<u8>, ApiValidationError> {
            match &self.node {
                FullTextNode::Token(token) => {
                    let mut bytes = vec![0];
                    bytes.extend_from_slice(token.as_str().as_bytes());
                    Ok(bytes)
                }
                FullTextNode::All(children) | FullTextNode::Any(children) => {
                    let mut bytes = vec![if matches!(&self.node, FullTextNode::All(_)) {
                        1
                    } else {
                        2
                    }];
                    for child in children {
                        write_bytes(&mut bytes, &child.canonical_bytes()?)?;
                    }
                    Ok(bytes)
                }
                FullTextNode::Not(child) => {
                    let mut bytes = vec![3];
                    bytes.extend_from_slice(&child.canonical_bytes()?);
                    Ok(bytes)
                }
            }
        }
    }

    /// Full-text fields and expression; the host may report the optional capability unsupported.
    #[derive(Clone, Debug)]
    pub struct FullTextSpec {
        fields: NonEmptySet<FieldSelector>,
        expression: FullTextExpr,
    }

    impl FullTextSpec {
        /// Creates a nonempty field selection and closed expression.
        pub fn new(
            fields: Vec<FieldSelector>,
            expression: FullTextExpr,
        ) -> Result<Self, ApiValidationError> {
            let fields = NonEmptySet::new(fields).map_err(|_| ApiValidationError::EmptyFieldSet)?;
            Ok(Self { fields, expression })
        }

        /// Canonically ordered searchable fields.
        #[must_use]
        pub fn fields(&self) -> &[FieldSelector] {
            self.fields.as_slice()
        }

        /// Closed boolean search expression.
        #[must_use]
        pub const fn expression(&self) -> &FullTextExpr {
            &self.expression
        }
    }

    /// One of the closed version 1 query operations.
    #[derive(Clone, Debug)]
    pub enum QueryOperation {
        /// Enumerate authorized raw history.
        RawHistory,
        /// Produce the canonical resolved view.
        ResolvedView,
        /// Explain the resolved view.
        Explain,
        /// Deterministic ASCII-whitespace token search.
        TokenSearch(SearchSpec),
        /// Optional full-text expression search.
        FullTextSearch(FullTextSpec),
        /// Bounded authorized relationship traversal.
        GraphTraversal(GraphSpec),
        /// Count, exists, or grouped count over visible resolved results.
        Aggregate(AggregateSpec),
    }

    /// Output projection mode.
    #[derive(Clone, Debug)]
    pub enum Projection {
        /// The operation's versioned default result shape.
        OperationDefault,
        /// Explicit nonempty set of fields.
        Only(NonEmptySet<FieldSelector>),
    }

    impl Projection {
        /// Creates an explicit nonempty field projection.
        pub fn only(fields: Vec<FieldSelector>) -> Result<Self, ApiValidationError> {
            NonEmptySet::new(fields)
                .map(Self::Only)
                .map_err(|_| ApiValidationError::EmptyFieldSet)
        }
    }

    /// Sort field supported by the closed operation outputs.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum SortField {
        /// Stable typed identity key.
        ResultKey,
        /// Transaction-time revision.
        RecordedRevision,
        /// Entity identity.
        EntityId,
        /// Predicate identity.
        PredicateId,
        /// Event-time value on one comparable Timeline.
        EventTime,
        /// Typed output field value.
        FieldValue(FieldSelector),
        /// Aggregate group key.
        GroupValue(FieldSelector),
        /// Aggregate count.
        AggregateCount,
    }

    /// Sort direction.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum SortDirection {
        /// Ascending typed value order.
        Ascending,
        /// Descending typed value order.
        Descending,
    }

    /// Placement for absent values; it is independent from sort direction.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum MissingPlacement {
        /// Missing value sorts first.
        First,
        /// Missing value sorts last.
        Last,
    }

    /// One ordered sort term.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct SortTerm {
        field: SortField,
        direction: SortDirection,
        missing: MissingPlacement,
    }

    impl SortTerm {
        /// Creates one typed sort term.
        #[must_use]
        pub const fn new(
            field: SortField,
            direction: SortDirection,
            missing: MissingPlacement,
        ) -> Self {
            Self {
                field,
                direction,
                missing,
            }
        }

        /// Sort field.
        #[must_use]
        pub const fn field(self) -> SortField {
            self.field
        }

        /// Sort direction.
        #[must_use]
        pub const fn direction(self) -> SortDirection {
            self.direction
        }

        /// Missing-value placement.
        #[must_use]
        pub const fn missing(self) -> MissingPlacement {
            self.missing
        }
    }

    /// Opaque continuation cursor with the fixed version 1 wire size.
    #[derive(Clone, Eq, Hash, PartialEq)]
    pub struct OpaqueCursor(Arc<[u8]>);

    impl OpaqueCursor {
        /// Wraps untrusted opaque bytes. Malformed shapes reach the engine and map uniformly
        /// to `CursorInvalidated` instead of exposing cursor-decoding distinctions.
        #[must_use]
        pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
            Self(Arc::from(bytes.into()))
        }

        /// Exact opaque cursor bytes. Callers must not parse or modify them.
        #[must_use]
        pub fn as_bytes(&self) -> &[u8] {
            &self.0
        }
    }

    impl fmt::Debug for OpaqueCursor {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("OpaqueCursor([redacted])")
        }
    }

    /// Positive page size and optional opaque continuation.
    #[derive(Clone, Debug)]
    pub struct PageRequest {
        limit: u32,
        cursor: Option<OpaqueCursor>,
    }

    impl PageRequest {
        /// Creates a bounded-shape page request. Engine configured maxima are checked on execution.
        pub fn new(limit: u32, cursor: Option<OpaqueCursor>) -> Result<Self, ApiValidationError> {
            if limit == 0 {
                return Err(ApiValidationError::ZeroPageLimit);
            }
            Ok(Self { limit, cursor })
        }

        /// Requested number of caller-visible results.
        #[must_use]
        pub const fn limit(&self) -> u32 {
            self.limit
        }

        /// Opaque continuation, if this is a later page.
        #[must_use]
        pub const fn cursor(&self) -> Option<&OpaqueCursor> {
            self.cursor.as_ref()
        }
    }

    /// One complete, owned semantic query request.
    #[derive(Clone, Debug)]
    pub struct QueryRequest {
        context: QueryContextDto,
        operation: QueryOperation,
        filter: FilterExpr,
        projection: Projection,
        sort: Vec<SortTerm>,
        page: PageRequest,
    }

    impl QueryRequest {
        /// Builds the common request used by Rust, CLI, and IPC adapters.
        pub fn new(
            context: QueryContextDto,
            operation: QueryOperation,
            filter: FilterExpr,
            projection: Projection,
            sort: Vec<SortTerm>,
            page: PageRequest,
        ) -> Result<Self, ApiValidationError> {
            if sort.len() > MAX_SORT_TERMS {
                return Err(ApiValidationError::TooManySortTerms);
            }
            let operation = match operation {
                QueryOperation::Aggregate(AggregateSpec::GroupedCount { fields }) => {
                    QueryOperation::Aggregate(
                        AggregateSpec::grouped_count(fields)
                            .map_err(|_| ApiValidationError::InvalidAggregateSpec)?,
                    )
                }
                operation => operation,
            };
            if sort.iter().any(|term| match term.field {
                SortField::GroupValue(_) => !matches!(
                    &operation,
                    QueryOperation::Aggregate(AggregateSpec::GroupedCount { .. })
                ),
                SortField::AggregateCount => !matches!(
                    &operation,
                    QueryOperation::Aggregate(
                        AggregateSpec::Count | AggregateSpec::GroupedCount { .. }
                    )
                ),
                _ => false,
            }) {
                return Err(ApiValidationError::InvalidSortFieldForOperation);
            }
            Ok(Self {
                context,
                operation,
                filter,
                projection,
                sort,
                page,
            })
        }

        /// Semantic and trusted-host-selected context values.
        #[must_use]
        pub const fn context(&self) -> &QueryContextDto {
            &self.context
        }

        /// Closed query operation and its typed parameters.
        #[must_use]
        pub const fn operation(&self) -> &QueryOperation {
            &self.operation
        }

        /// Closed, bounded filter AST.
        #[must_use]
        pub const fn filter(&self) -> &FilterExpr {
            &self.filter
        }

        /// Output projection.
        #[must_use]
        pub const fn projection(&self) -> &Projection {
            &self.projection
        }

        /// Sort terms in precedence order.
        #[must_use]
        pub fn sort(&self) -> &[SortTerm] {
            &self.sort
        }

        /// Page size and continuation.
        #[must_use]
        pub const fn page(&self) -> &PageRequest {
            &self.page
        }
    }

    /// Safe public code selected from the closed version 1 vocabulary.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct PublicCode(&'static str);

    impl PublicCode {
        /// Stable `InvalidRequest` code.
        pub const INVALID_REQUEST: Self = Self("InvalidRequest");
        /// Stable `UnsupportedProtocolVersion` code.
        pub const UNSUPPORTED_PROTOCOL_VERSION: Self = Self("UnsupportedProtocolVersion");
        /// Stable `UnsupportedQueryCapability` code.
        pub const UNSUPPORTED_QUERY_CAPABILITY: Self = Self("UnsupportedQueryCapability");
        /// Stable `InvalidQuery` code.
        pub const INVALID_QUERY: Self = Self("InvalidQuery");
        /// Stable `Unauthorized` code.
        pub const UNAUTHORIZED: Self = Self("Unauthorized");
        /// Stable `NotFound` code.
        pub const NOT_FOUND: Self = Self("NotFound");
        /// Stable `SnapshotExpired` code.
        pub const SNAPSHOT_EXPIRED: Self = Self("SnapshotExpired");
        /// Stable `CursorInvalidated` code.
        pub const CURSOR_INVALIDATED: Self = Self("CursorInvalidated");
        /// Stable `Cancelled` code.
        pub const CANCELLED: Self = Self("Cancelled");
        /// Stable `BudgetExceeded` code.
        pub const BUDGET_EXCEEDED: Self = Self("BudgetExceeded");
        /// Stable `StorageRead` code.
        pub const STORAGE_READ: Self = Self("StorageRead");
        /// Stable `CorruptData` code.
        pub const CORRUPT_DATA: Self = Self("CorruptData");
        /// Stable `UnsupportedOperation` code.
        pub const UNSUPPORTED_OPERATION: Self = Self("UnsupportedOperation");
        /// Stable `Internal` code.
        pub const INTERNAL: Self = Self("Internal");

        /// Parses a known version 1 code; arbitrary error text is never accepted.
        #[must_use]
        pub fn parse(value: &str) -> Option<Self> {
            PUBLIC_CODES
                .iter()
                .copied()
                .find(|code| *code == value)
                .map(Self)
        }

        /// Canonical stable code string.
        #[must_use]
        pub const fn as_str(self) -> &'static str {
            self.0
        }
    }

    impl fmt::Display for PublicCode {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.as_str())
        }
    }

    /// Stable public codes defined by the query transport contract.
    pub const PUBLIC_CODES: &[&str] = &[
        "BudgetExceeded",
        "Cancelled",
        "CorruptData",
        "CursorInvalidated",
        "Internal",
        "InvalidQuery",
        "InvalidRequest",
        "NotFound",
        "SnapshotExpired",
        "StorageRead",
        "Unauthorized",
        "UnsupportedOperation",
        "UnsupportedProtocolVersion",
        "UnsupportedQueryCapability",
    ];

    const CORE_TO_PUBLIC_CODES: &[(&str, &str)] = &[
        ("WDB-BACKUP-AUTHENTICATION", "Unauthorized"),
        ("WDB-BACKUP-INCOMPATIBLE-FORMAT", "UnsupportedOperation"),
        ("WDB-BACKUP-INTEGRITY-MISMATCH", "CorruptData"),
        ("WDB-BACKUP-INVALID-MANIFEST", "InvalidRequest"),
        ("WDB-BACKUP-SNAPSHOT-UNAVAILABLE", "SnapshotExpired"),
        ("WDB-BACKUP-STORAGE", "Internal"),
        ("WDB-COMMIT-AUTHORIZATION", "Unauthorized"),
        ("WDB-COMMIT-CONFLICT", "Internal"),
        ("WDB-COMMIT-READ-ONLY", "UnsupportedOperation"),
        ("WDB-COMMIT-SHUTTING-DOWN", "Internal"),
        ("WDB-COMMIT-STORAGE", "Internal"),
        ("WDB-COMMIT-UNKNOWN", "Internal"),
        ("WDB-COMMIT-VALIDATION", "InvalidRequest"),
        ("WDB-EXPORT-INVALID-SCOPE", "InvalidRequest"),
        ("WDB-EXPORT-IO", "Internal"),
        ("WDB-EXPORT-MISSING-DEPENDENCY", "NotFound"),
        ("WDB-EXPORT-UNAUTHORIZED", "Unauthorized"),
        ("WDB-EXPORT-UNSUPPORTED-FORMAT", "UnsupportedOperation"),
        ("WDB-JOB-BUDGET-EXCEEDED", "BudgetExceeded"),
        ("WDB-JOB-CANCELLED", "Cancelled"),
        ("WDB-JOB-DEPENDENCY-FAILED", "Internal"),
        ("WDB-JOB-NOT-FOUND", "NotFound"),
        ("WDB-JOB-RESUME-MISMATCH", "InvalidRequest"),
        ("WDB-MIGRATION-CONSTRAINT-VIOLATION", "InvalidRequest"),
        ("WDB-MIGRATION-DECISION-REQUIRED", "Unauthorized"),
        ("WDB-MIGRATION-INVALID-PLAN", "InvalidRequest"),
        ("WDB-MIGRATION-RESUME-MISMATCH", "InvalidRequest"),
        ("WDB-MIGRATION-STORAGE", "Internal"),
        ("WDB-MIGRATION-UNSUPPORTED-FORMAT", "UnsupportedOperation"),
        ("WDB-OPEN-CORRUPT", "CorruptData"),
        ("WDB-OPEN-IO", "Internal"),
        ("WDB-OPEN-LOCKED", "Internal"),
        ("WDB-OPEN-NEEDS-MIGRATION", "UnsupportedOperation"),
        ("WDB-OPEN-NOT-FOUND", "NotFound"),
        ("WDB-OPEN-PERMISSION-DENIED", "Unauthorized"),
        ("WDB-OPEN-RECOVERY-REQUIRED", "Internal"),
        ("WDB-OPEN-UNSUPPORTED-FORMAT", "UnsupportedOperation"),
        ("WDB-QUERY-BUDGET-EXCEEDED", "BudgetExceeded"),
        ("WDB-QUERY-CANCELLED", "Cancelled"),
        ("WDB-QUERY-CORRUPT-DATA", "CorruptData"),
        ("WDB-QUERY-INVALID", "InvalidQuery"),
        ("WDB-QUERY-SNAPSHOT-EXPIRED", "SnapshotExpired"),
        ("WDB-QUERY-STORAGE-READ", "StorageRead"),
        ("WDB-QUERY-UNAUTHORIZED", "Unauthorized"),
        ("WDB-RECOVERY-CORRUPT-COMMITTED-DATA", "CorruptData"),
        ("WDB-RECOVERY-DECISION-REQUIRED", "Unauthorized"),
        ("WDB-RECOVERY-HISTORY-GAP", "CorruptData"),
        ("WDB-RECOVERY-INDETERMINATE-COMMIT", "Internal"),
        ("WDB-RECOVERY-STORAGE", "Internal"),
        ("WDB-SECURITY-ACCESS-DENIED", "Unauthorized"),
        ("WDB-SECURITY-CAPABILITY-MISSING", "Unauthorized"),
        ("WDB-SECURITY-EPOCH-CHANGED", "Unauthorized"),
        ("WDB-SECURITY-INVALID-PRINCIPAL", "Unauthorized"),
        ("WDB-SECURITY-NOT-FOUND", "NotFound"),
        ("WDB-SECURITY-RESOURCE-UNAVAILABLE", "NotFound"),
        ("WDB-STORAGE-CORRUPT", "CorruptData"),
        ("WDB-STORAGE-DURABILITY", "Internal"),
        ("WDB-STORAGE-IO", "Internal"),
        ("WDB-STORAGE-LOCKED", "Internal"),
        ("WDB-STORAGE-NOT-FOUND", "NotFound"),
        ("WDB-STORAGE-PERMISSION-DENIED", "Unauthorized"),
        ("WDB-VALIDATION-BUDGET-EXCEEDED", "BudgetExceeded"),
        ("WDB-VALIDATION-INVALID-REFERENCE", "InvalidRequest"),
        ("WDB-VALIDATION-INVALID-TEMPORAL-RANGE", "InvalidQuery"),
        ("WDB-VALIDATION-INVALID-VALUE", "InvalidRequest"),
        ("WDB-VALIDATION-SCHEMA-VIOLATION", "InvalidQuery"),
        (
            "WDB-VALIDATION-UNSUPPORTED-OPERATION",
            "UnsupportedOperation",
        ),
        ("WDB-WIRE-INVALID-FRAME", "InvalidRequest"),
        ("WDB-WIRE-RESOURCE-LIMIT", "BudgetExceeded"),
        ("WDB-WIRE-UNKNOWN-EXTERNAL-CODE", "InvalidRequest"),
        ("WDB-WIRE-UNKNOWN-VALUE-TAG", "InvalidRequest"),
    ];

    /// Cause-free error DTO exposed to all application adapters.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct PublicApiError {
        code: PublicCode,
        retry_hint: RetryHint,
        operation_id: Option<OperationId>,
        job_id: Option<JobId>,
    }

    impl PublicApiError {
        /// Creates a public error from a closed public code and safe identifiers only.
        #[must_use]
        pub const fn new(
            code: PublicCode,
            retry_hint: RetryHint,
            operation_id: Option<OperationId>,
            job_id: Option<JobId>,
        ) -> Self {
            Self {
                code,
                retry_hint,
                operation_id,
                job_id,
            }
        }

        /// Projects the core's cause-free DTO into the versioned code type.
        pub fn from_core(error: crate::PublicErrorDto) -> Result<Self, ApiValidationError> {
            let protocol_code = CORE_TO_PUBLIC_CODES
                .iter()
                .find(|(core_code, _)| *core_code == error.code())
                .map(|(_, public_code)| *public_code)
                .ok_or(ApiValidationError::UnknownPublicCode)?;
            let code =
                PublicCode::parse(protocol_code).ok_or(ApiValidationError::UnknownPublicCode)?;
            Ok(Self::new(
                code,
                error.retry_hint(),
                error.operation_id(),
                error.job_id(),
            ))
        }

        /// Stable public code.
        #[must_use]
        pub const fn code(self) -> PublicCode {
            self.code
        }

        /// Localization key associated with the public code.
        #[must_use]
        pub const fn message_key(self) -> &'static str {
            self.code.as_str()
        }

        /// Safe retry guidance.
        #[must_use]
        pub const fn retry_hint(self) -> RetryHint {
            self.retry_hint
        }

        /// Operation identity only when outcome resolution is required.
        #[must_use]
        pub const fn operation_id(self) -> Option<OperationId> {
            self.operation_id
        }

        /// Visible job identity, when the failure belongs to a job.
        #[must_use]
        pub const fn job_id(self) -> Option<JobId> {
            self.job_id
        }
    }

    impl fmt::Display for PublicApiError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.code.as_str())
        }
    }

    impl std::error::Error for PublicApiError {}

    /// Owned raw row without an internal query binding or storage borrow.
    #[derive(Clone, Debug)]
    pub struct RawRecordDto {
        recorded_revision: Revision,
        owner_history_space_id: HistorySpaceId,
        record_ref: RecordRef,
        record: Record,
    }

    impl RawRecordDto {
        /// Projects an owned record row into its public payload.
        #[must_use]
        pub fn new(row: &RawHistoryRow<Record>) -> Self {
            Self {
                recorded_revision: row.recorded_revision(),
                owner_history_space_id: row.owner_history_space_id(),
                record_ref: row.record_ref(),
                record: row.value().clone(),
            }
        }

        /// Revision at which this record was stored.
        #[must_use]
        pub const fn recorded_revision(&self) -> Revision {
            self.recorded_revision
        }

        /// Owning HistorySpace.
        #[must_use]
        pub const fn owner_history_space_id(&self) -> HistorySpaceId {
            self.owner_history_space_id
        }

        /// Typed record identity.
        #[must_use]
        pub const fn record_ref(&self) -> RecordRef {
            self.record_ref
        }

        /// Owned record value.
        #[must_use]
        pub const fn record(&self) -> &Record {
            &self.record
        }
    }

    /// Explain output without its internal historical-query binding.
    #[derive(Clone, Debug)]
    pub struct ExplainDto {
        stages: Vec<ExplainStage>,
        resolved_view: ResolvedView,
    }

    impl ExplainDto {
        /// Projects only the checked, caller-visible stages and resolved outcome.
        #[must_use]
        pub fn new(explain: &ReferenceExplain) -> Self {
            Self {
                stages: explain.stages().to_vec(),
                resolved_view: explain.resolved_view().clone(),
            }
        }

        /// Ordered caller-visible explanation stages.
        #[must_use]
        pub fn stages(&self) -> &[ExplainStage] {
            &self.stages
        }

        /// Resolved view explained by these stages.
        #[must_use]
        pub const fn resolved_view(&self) -> &ResolvedView {
            &self.resolved_view
        }
    }

    /// Typed owned query result variants supported by the version 1 facade.
    #[derive(Clone, Debug)]
    pub enum QueryResultDto {
        /// Authorized raw record.
        RawRecord(RawRecordDto),
        /// Resolved view with visible contributors.
        Resolved(ResolvedView),
        /// Checked explanation output.
        Explanation(ExplainDto),
        /// Token-search match without snippets.
        SearchHit(SearchHit),
        /// Complete visible graph traversal.
        Graph(GraphResult),
        /// Complete visible aggregate.
        Aggregate(AggregateResult),
    }

    /// Complete owned page. No storage borrow, path, policy object, or partial-result marker.
    #[derive(Clone, Debug)]
    pub struct QueryPageDto {
        snapshot_id: SnapshotId,
        results: Vec<QueryResultDto>,
        next_cursor: Option<OpaqueCursor>,
    }

    impl QueryPageDto {
        /// Creates a complete page returned by an engine operation.
        #[must_use]
        pub const fn new(
            snapshot_id: SnapshotId,
            results: Vec<QueryResultDto>,
            next_cursor: Option<OpaqueCursor>,
        ) -> Self {
            Self {
                snapshot_id,
                results,
                next_cursor,
            }
        }

        /// Concrete pinned snapshot for every row in this page.
        #[must_use]
        pub const fn snapshot_id(&self) -> SnapshotId {
            self.snapshot_id
        }

        /// Complete owned result rows.
        #[must_use]
        pub fn results(&self) -> &[QueryResultDto] {
            &self.results
        }

        /// Continuation handle only when another visible result exists.
        #[must_use]
        pub const fn next_cursor(&self) -> Option<&OpaqueCursor> {
            self.next_cursor.as_ref()
        }
    }

    /// Adapter request operation in the version 1 envelope.
    #[derive(Clone, Debug)]
    pub enum RequestOperation {
        /// Execute the common semantic query DTO.
        Query(QueryRequest),
        /// Cancel a request on the authenticated host session.
        Cancel { target_request_id: RequestId },
    }

    /// Versioned request envelope shared by Rust, CLI, and desktop IPC.
    #[derive(Clone, Debug)]
    pub struct RequestEnvelope {
        protocol: ProtocolVersion,
        request_id: RequestId,
        operation: RequestOperation,
    }

    impl RequestEnvelope {
        /// Binds one typed operation to a protocol version and correlation identity.
        #[must_use]
        pub const fn new(
            protocol: ProtocolVersion,
            request_id: RequestId,
            operation: RequestOperation,
        ) -> Self {
            Self {
                protocol,
                request_id,
                operation,
            }
        }

        /// Protocol version supplied by the caller.
        #[must_use]
        pub const fn protocol(&self) -> ProtocolVersion {
            self.protocol
        }

        /// Request correlation identity.
        #[must_use]
        pub const fn request_id(&self) -> RequestId {
            self.request_id
        }

        /// Closed adapter operation.
        #[must_use]
        pub const fn operation(&self) -> &RequestOperation {
            &self.operation
        }
    }

    /// Winner of a cancellation/completion race.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum CancellationDisposition {
        /// A running query received a cancellation signal.
        CancellationSignalled,
        /// The query was already terminal when cancellation arrived.
        AlreadyTerminal,
    }

    /// Versioned response body.
    #[derive(Clone, Debug)]
    pub enum ResponseBody {
        /// Complete query page.
        QueryPage(QueryPageDto),
        /// Cancellation disposition for its target request.
        CancelResult {
            target_request_id: RequestId,
            disposition: CancellationDisposition,
        },
    }

    /// Versioned response envelope with either one complete body or a safe public error.
    #[derive(Clone, Debug)]
    pub struct ResponseEnvelope {
        protocol: ProtocolVersion,
        request_id: RequestId,
        outcome: Result<ResponseBody, PublicApiError>,
    }

    impl ResponseEnvelope {
        /// Creates one terminal response.
        #[must_use]
        pub const fn new(
            protocol: ProtocolVersion,
            request_id: RequestId,
            outcome: Result<ResponseBody, PublicApiError>,
        ) -> Self {
            Self {
                protocol,
                request_id,
                outcome,
            }
        }

        /// Response protocol version.
        #[must_use]
        pub const fn protocol(&self) -> ProtocolVersion {
            self.protocol
        }

        /// Correlation identity echoed from the request.
        #[must_use]
        pub const fn request_id(&self) -> RequestId {
            self.request_id
        }

        /// Complete result or cause-free public error.
        pub const fn outcome(&self) -> &Result<ResponseBody, PublicApiError> {
            &self.outcome
        }
    }

    /// High-level engine operations exposed to all application adapters.
    ///
    /// Implementations bind the authenticated host Principal, cancellation state,
    /// snapshots, storage, and policy decisions internally. None is caller-supplied.
    pub trait EngineOperations: Send + Sync {
        /// Executes one semantic query and returns a complete owned page or a public error.
        fn execute_query(&self, request: QueryRequest) -> Result<QueryPageDto, PublicApiError>;

        /// Signals cancellation for one active request without accepting a caller-owned token.
        fn cancel_request(
            &self,
            target_request_id: RequestId,
        ) -> Result<CancellationDisposition, PublicApiError>;
    }

    /// Construction error for a typed version 1 request.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum ApiValidationError {
        /// A handshake needs at least one supported version.
        EmptySupportedVersions,
        /// Required protocol features must be unique.
        DuplicateProtocolFeature,
        /// No supported protocol version intersects.
        NoCommonProtocolVersion,
        /// A required feature is not supported.
        RequiredFeatureUnavailable,
        /// Perspective and EpistemicMode are not a valid Master-defined pair.
        InvalidPerspectiveEpistemicPair,
        /// A closed boolean group cannot be empty.
        EmptyBooleanGroup,
        /// Filter or full-text nesting exceeds 32 levels.
        FilterTooDeep,
        /// Filter tests or full-text terms exceed 256.
        TooManyFilterTests,
        /// A revision or scalar interval is empty or reversed.
        EmptyOrReversedRange,
        /// A time range spans two Timelines.
        CrossTimelineRange,
        /// A relative calendar period cannot be zero.
        EmptyCalendarPeriod,
        /// A range needs at least one endpoint.
        RangeNeedsBound,
        /// A time/record reference has no valid canonical representation.
        InvalidTypedReference,
        /// A field/projection set cannot be empty or contain duplicates.
        EmptyFieldSet,
        /// A page limit must be positive.
        ZeroPageLimit,
        /// At most eight sort terms are accepted.
        TooManySortTerms,
        /// A sort field is not defined for the selected operation.
        InvalidSortFieldForOperation,
        /// A grouped aggregate has no unique, nonempty group selector set.
        InvalidAggregateSpec,
        /// The versioned core code set does not contain this error code.
        UnknownPublicCode,
        /// An AST or list cannot be represented within protocol length limits.
        RequestTooLarge,
    }

    impl fmt::Display for ApiValidationError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(match self {
                Self::EmptySupportedVersions => "supported protocol versions must not be empty",
                Self::DuplicateProtocolFeature => "protocol feature is duplicated",
                Self::NoCommonProtocolVersion => "no common protocol version is supported",
                Self::RequiredFeatureUnavailable => "a required protocol feature is unavailable",
                Self::InvalidPerspectiveEpistemicPair => {
                    "perspective and epistemic mode do not form a valid partition"
                }
                Self::EmptyBooleanGroup => "boolean groups must contain at least one child",
                Self::FilterTooDeep => "filter expression exceeds its nesting bound",
                Self::TooManyFilterTests => "filter expression exceeds its test bound",
                Self::EmptyOrReversedRange => "range is empty or reversed",
                Self::CrossTimelineRange => "time range crosses Timelines",
                Self::EmptyCalendarPeriod => "calendar-relative period must be positive",
                Self::RangeNeedsBound => "range must include at least one bound",
                Self::InvalidTypedReference => "typed reference has no canonical encoding",
                Self::EmptyFieldSet => "field set must be nonempty and unique",
                Self::ZeroPageLimit => "page limit must be positive",
                Self::TooManySortTerms => "sort has more than eight terms",
                Self::InvalidSortFieldForOperation => {
                    "sort field is not defined for the selected operation"
                }
                Self::InvalidAggregateSpec => "grouped aggregate fields are invalid",
                Self::UnknownPublicCode => "public error code is outside protocol version 1",
                Self::RequestTooLarge => "request exceeds protocol length limits",
            })
        }
    }

    impl std::error::Error for ApiValidationError {}

    impl From<ApiValidationError> for PublicApiError {
        fn from(error: ApiValidationError) -> Self {
            let code = match error {
                ApiValidationError::EmptySupportedVersions
                | ApiValidationError::DuplicateProtocolFeature
                | ApiValidationError::InvalidTypedReference
                | ApiValidationError::RequestTooLarge => PublicCode::INVALID_REQUEST,
                ApiValidationError::NoCommonProtocolVersion => {
                    PublicCode::UNSUPPORTED_PROTOCOL_VERSION
                }
                ApiValidationError::RequiredFeatureUnavailable => {
                    PublicCode::UNSUPPORTED_QUERY_CAPABILITY
                }
                ApiValidationError::EmptyBooleanGroup
                | ApiValidationError::FilterTooDeep
                | ApiValidationError::TooManyFilterTests
                | ApiValidationError::EmptyOrReversedRange
                | ApiValidationError::CrossTimelineRange
                | ApiValidationError::EmptyCalendarPeriod
                | ApiValidationError::RangeNeedsBound
                | ApiValidationError::EmptyFieldSet
                | ApiValidationError::ZeroPageLimit
                | ApiValidationError::TooManySortTerms
                | ApiValidationError::InvalidSortFieldForOperation
                | ApiValidationError::InvalidAggregateSpec
                | ApiValidationError::InvalidPerspectiveEpistemicPair => PublicCode::INVALID_QUERY,
                ApiValidationError::UnknownPublicCode => PublicCode::INTERNAL,
            };
            Self::new(code, RetryHint::DoNotRetry, None, None)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            ApiValidationError, BoundEdge, CORE_TO_PUBLIC_CODES, CURRENT_PROTOCOL, FilterExpr,
            FilterNode, FilterTest, HandshakeRequest, HandshakeResponse, OpaqueCursor,
            PUBLIC_CODES, ProtocolFeature, ProtocolVersion, PublicApiError, PublicCode,
            QueryContextDto, QueryOperation,
        };
        use crate::{
            AuthorizationMode, LayerSelection, QueryBudget, QueryBudgetLimits, RecordKind,
            Revision, SchemaMode, Subject, WorldTimeSelector,
        };
        use std::str::FromStr;

        #[test]
        fn facade_has_fixed_current_protocol_and_closed_public_codes() {
            assert_eq!(CURRENT_PROTOCOL, ProtocolVersion::new(1, 0));
            assert!(PublicCode::parse("InvalidQuery").is_some());
            assert!(PublicCode::parse("secret filesystem path").is_none());
            assert!(PUBLIC_CODES.windows(2).all(|pair| {
                pair.first()
                    .zip(pair.get(1))
                    .is_some_and(|(left, right)| left < right)
            }));
            assert!(
                CORE_TO_PUBLIC_CODES
                    .iter()
                    .all(|(_, code)| PublicCode::parse(code).is_some())
            );
        }

        #[test]
        fn handshake_selects_highest_common_version_and_checks_required_features() {
            let request = HandshakeRequest::new(
                super::RequestId::from_bytes([7; 16]),
                vec![ProtocolVersion::new(1, 0), ProtocolVersion::new(1, 1)],
                vec![ProtocolFeature::FullTextSearch],
            );
            assert!(request.is_ok());
            let Some(request) = request.ok() else {
                return;
            };
            let selected = HandshakeResponse::negotiate(
                &request,
                &[ProtocolVersion::new(1, 0), ProtocolVersion::new(1, 1)],
                &[ProtocolFeature::FullTextSearch],
            );
            assert!(selected.is_ok());
            let Some(selected) = selected.ok() else {
                return;
            };
            assert_eq!(selected.selected_version(), ProtocolVersion::new(1, 1));
            assert_eq!(
                HandshakeResponse::negotiate(&request, &[ProtocolVersion::new(1, 0)], &[]),
                Err(ApiValidationError::RequiredFeatureUnavailable)
            );
        }

        #[test]
        fn boolean_filter_groups_reject_empty_and_canonicalize_children() {
            assert_eq!(
                FilterExpr::new(FilterNode::All(Vec::new())).err(),
                Some(ApiValidationError::EmptyBooleanGroup)
            );
            let later_set = crate::NonEmptySet::new(vec![RecordKind::Event]);
            let earlier_set = crate::NonEmptySet::new(vec![RecordKind::Assertion]);
            assert!(later_set.is_ok());
            assert!(earlier_set.is_ok());
            let (Some(later_set), Some(earlier_set)) = (later_set.ok(), earlier_set.ok()) else {
                return;
            };
            let later = FilterExpr::new(FilterNode::Test(FilterTest::RecordKindIn(later_set)));
            let earlier = FilterExpr::new(FilterNode::Test(FilterTest::RecordKindIn(earlier_set)));
            assert!(later.is_ok());
            assert!(earlier.is_ok());
            let (Some(later), Some(earlier)) = (later.ok(), earlier.ok()) else {
                return;
            };
            let combined = FilterExpr::new(FilterNode::All(vec![later, earlier.clone(), earlier]));
            assert!(combined.is_ok());
            let Some(combined) = combined.ok() else {
                return;
            };
            assert!(matches!(combined.node(), FilterNode::All(_)));
            if let FilterNode::All(children) = combined.node() {
                assert_eq!(children.len(), 2);
            }
            assert_eq!(combined.test_count(), 2);
        }

        #[test]
        fn filter_bounds_check_revision_ranges_and_cursor_shape() {
            let revision = Revision::new(4);
            assert!(revision.is_ok());
            let Some(revision) = revision.ok() else {
                return;
            };
            assert!(
                super::RevisionRange::new(
                    Some(super::RevisionBound::new(revision, BoundEdge::Exclusive)),
                    Some(super::RevisionBound::new(revision, BoundEdge::Inclusive)),
                )
                .is_err()
            );
            let malformed_cursor = OpaqueCursor::new(vec![0; 72]);
            assert_eq!(malformed_cursor.as_bytes().len(), 72);
            assert_eq!(format!("{malformed_cursor:?}"), "OpaqueCursor([redacted])");
        }

        #[test]
        fn identity_and_filter_types_remain_explicitly_typed() {
            let id = crate::EntityId::from_str("018f0000-0000-7000-8000-000000000001");
            assert!(id.is_ok());
            let Some(id) = id.ok() else {
                return;
            };
            let filter = FilterExpr::new(FilterNode::Test(FilterTest::SubjectIs(Subject::new(id))));
            assert!(filter.is_ok());
            let _operation = QueryOperation::RawHistory;
        }

        #[test]
        fn query_context_dto_rejects_invalid_perspective_partition() {
            let history_space =
                crate::HistorySpaceId::from_str("018f0000-0000-7000-8000-000000000002");
            let limits = QueryBudgetLimits::new(1, 1, 1);
            assert!(history_space.is_ok() && limits.is_ok());
            let (Some(history_space), Some(limits)) = (history_space.ok(), limits.ok()) else {
                return;
            };
            let budget = QueryBudget::new(1, 1, 1, limits);
            assert!(budget.is_ok());
            let Some(budget) = budget.ok() else {
                return;
            };
            let invalid = QueryContextDto::new(super::QueryContextDtoInput {
                snapshot: crate::SnapshotSelector::Current,
                recorded_as_of: Revision::GENESIS,
                history_space,
                layers: LayerSelection::BaseOnly,
                world_time: WorldTimeSelector::AllTimes,
                perspective: crate::PerspectiveScope::World,
                epistemic_mode: crate::EpistemicMode::Knows,
                schema_mode: SchemaMode::Historical,
                authorization_mode: AuthorizationMode::Now,
                budget,
            });
            assert_eq!(
                invalid.err(),
                Some(ApiValidationError::InvalidPerspectiveEpistemicPair)
            );
        }

        #[test]
        fn public_api_error_projects_only_the_closed_code_and_safe_hint() {
            let core = crate::to_public_error(&crate::QueryError::InvalidQuery);
            let projected = PublicApiError::from_core(core);
            assert!(projected.is_ok());
            let Some(projected) = projected.ok() else {
                return;
            };
            assert_eq!(projected.code().as_str(), "InvalidQuery");
            assert_eq!(projected.message_key(), "InvalidQuery");
            assert_eq!(projected.to_string(), "InvalidQuery");
        }
    }
}
