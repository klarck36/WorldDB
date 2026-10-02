//! Complete snapshot-bound context required before a query can execute.

use std::fmt;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::context::{EpistemicMode, PerspectiveScope};
use crate::ids::{HistorySpaceId, LayerId, PrincipalId, Revision, SchemaRevision};
use crate::layers::{LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
use crate::record_refs::SnapshotRef;
use crate::reference_query::HistoricalQueryBinding;
use crate::resource_profile::{ProcessMemoryBudget, process_memory_budget};
use crate::schema::NonEmptySet;
use crate::schema_history::SchemaMode;
use crate::snapshot_lease::{SnapshotLease, SnapshotLifetimeStatus};
use crate::temporal::{RecordedAsOf, WorldTime};

/// Absolute candidate ceiling accepted by any one query.
pub const MAX_QUERY_CANDIDATES: u64 = 1_000_000;
/// Absolute work-unit ceiling accepted by any one query.
pub const MAX_QUERY_WORK_UNITS: u64 = 10_000_000;
/// Absolute result-row ceiling accepted by any one query.
pub const MAX_QUERY_RESULTS: u64 = 100_000;

/// Temporal interpretation required by every query context.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WorldTimeSelector {
    /// Do not select a single current world-time point.
    AllTimes,
    /// Select records and event states at one resolved world-time coordinate.
    At(WorldTime),
}

/// Snapshot selector accepted at the host request boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SnapshotSelector {
    /// Resolve the current published snapshot exactly once before query execution.
    Current,
    /// Resolve a concrete published snapshot at this transaction revision.
    AtRevision(Revision),
}

/// Security time basis for authorization of one query.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuthorizationMode {
    /// Evaluate permissions against the current policy snapshot.
    Now,
    /// Explicitly request policy as it existed at a committed revision.
    AtRevision(Revision),
}

/// Trusted host security identity bound to a query context.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SecurityContext {
    principal_id: PrincipalId,
    authorization_mode: AuthorizationMode,
}

impl SecurityContext {
    /// Binds the host-authenticated Principal and explicit authorization mode.
    #[must_use]
    pub const fn new(principal_id: PrincipalId, authorization_mode: AuthorizationMode) -> Self {
        Self {
            principal_id,
            authorization_mode,
        }
    }

    /// Host-authenticated security Principal.
    #[must_use]
    pub const fn principal_id(self) -> PrincipalId {
        self.principal_id
    }

    /// Explicit policy time basis.
    #[must_use]
    pub const fn authorization_mode(self) -> AuthorizationMode {
        self.authorization_mode
    }
}

/// Finite positive work limits configured by the engine.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QueryBudgetLimits {
    max_candidates: NonZeroU64,
    max_work_units: NonZeroU64,
    max_results: NonZeroU64,
}

impl QueryBudgetLimits {
    /// Creates positive engine hard maxima for the three semantic budgets.
    pub fn new(
        max_candidates: u64,
        max_work_units: u64,
        max_results: u64,
    ) -> Result<Self, QueryBudgetError> {
        let limits = Self {
            max_candidates: NonZeroU64::new(max_candidates)
                .ok_or(QueryBudgetError::ZeroLimit(BudgetDimension::Candidates))?,
            max_work_units: NonZeroU64::new(max_work_units)
                .ok_or(QueryBudgetError::ZeroLimit(BudgetDimension::WorkUnits))?,
            max_results: NonZeroU64::new(max_results)
                .ok_or(QueryBudgetError::ZeroLimit(BudgetDimension::Results))?,
        };
        for (dimension, configured, maximum) in [
            (
                BudgetDimension::Candidates,
                limits.max_candidates,
                MAX_QUERY_CANDIDATES,
            ),
            (
                BudgetDimension::WorkUnits,
                limits.max_work_units,
                MAX_QUERY_WORK_UNITS,
            ),
            (
                BudgetDimension::Results,
                limits.max_results,
                MAX_QUERY_RESULTS,
            ),
        ] {
            if configured.get() > maximum {
                return Err(QueryBudgetError::ExceedsAbsoluteMaximum {
                    dimension,
                    requested: configured.get(),
                    maximum,
                });
            }
        }
        Ok(limits)
    }
}

/// Positive finite limits requested for one query.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QueryBudget {
    max_candidates: NonZeroU64,
    max_work_units: NonZeroU64,
    max_results: NonZeroU64,
}

impl QueryBudget {
    /// Creates a query budget no larger than the configured engine hard limits.
    pub fn new(
        max_candidates: u64,
        max_work_units: u64,
        max_results: u64,
        limits: QueryBudgetLimits,
    ) -> Result<Self, QueryBudgetError> {
        let budget = Self {
            max_candidates: NonZeroU64::new(max_candidates)
                .ok_or(QueryBudgetError::ZeroRequested(BudgetDimension::Candidates))?,
            max_work_units: NonZeroU64::new(max_work_units)
                .ok_or(QueryBudgetError::ZeroRequested(BudgetDimension::WorkUnits))?,
            max_results: NonZeroU64::new(max_results)
                .ok_or(QueryBudgetError::ZeroRequested(BudgetDimension::Results))?,
        };
        for (dimension, requested, hard_maximum) in [
            (
                BudgetDimension::Candidates,
                budget.max_candidates,
                limits.max_candidates,
            ),
            (
                BudgetDimension::WorkUnits,
                budget.max_work_units,
                limits.max_work_units,
            ),
            (
                BudgetDimension::Results,
                budget.max_results,
                limits.max_results,
            ),
        ] {
            if requested > hard_maximum {
                return Err(QueryBudgetError::ExceedsHardMaximum {
                    dimension,
                    requested: requested.get(),
                    hard_maximum: hard_maximum.get(),
                });
            }
        }
        Ok(budget)
    }

    /// Candidate limit; only authorized and FieldRead-eligible candidates count.
    #[must_use]
    pub const fn max_candidates(self) -> NonZeroU64 {
        self.max_candidates
    }
    /// Work-unit limit; only authorized query work counts.
    #[must_use]
    pub const fn max_work_units(self) -> NonZeroU64 {
        self.max_work_units
    }
    /// Caller-visible result-row limit.
    #[must_use]
    pub const fn max_results(self) -> NonZeroU64 {
        self.max_results
    }
}

/// Query budget dimension used in safe validation errors.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BudgetDimension {
    Candidates,
    WorkUnits,
    Results,
}

/// Invalid requested or configured query work limit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QueryBudgetError {
    /// An engine hard maximum was configured as zero.
    ZeroLimit(BudgetDimension),
    /// A caller requested a zero semantic budget.
    ZeroRequested(BudgetDimension),
    /// A caller requested more work than the configured hard maximum.
    ExceedsHardMaximum {
        dimension: BudgetDimension,
        requested: u64,
        hard_maximum: u64,
    },
    /// An engine maximum exceeded the implementation's absolute finite ceiling.
    ExceedsAbsoluteMaximum {
        dimension: BudgetDimension,
        requested: u64,
        maximum: u64,
    },
}

impl fmt::Display for QueryBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit(dimension) => write!(formatter, "zero engine limit for {dimension:?}"),
            Self::ZeroRequested(dimension) => {
                write!(formatter, "zero query budget for {dimension:?}")
            }
            Self::ExceedsHardMaximum { dimension, .. } => {
                write!(
                    formatter,
                    "query budget exceeds engine maximum for {dimension:?}"
                )
            }
            Self::ExceedsAbsoluteMaximum { dimension, .. } => write!(
                formatter,
                "configured query limit exceeds the absolute maximum for {dimension:?}"
            ),
        }
    }
}

impl std::error::Error for QueryBudgetError {}

/// Shared out-of-band cancellation state supplied by the trusted host.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Creates one host-owned cancellation source/token pair.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation for every clone sharing this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// A layer request proven non-empty and valid against one pinned schema snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedLayerSelection {
    requested: LayerSelection,
    schema_revision: SchemaRevision,
    resolved: NonEmptySet<LayerId>,
}

impl ValidatedLayerSelection {
    /// Resolves and validates the selection against the pinned schema.
    pub fn resolve(
        schema: &LayerSchemaSnapshot,
        requested: LayerSelection,
    ) -> Result<Self, LayerSchemaError> {
        let resolved = schema.resolve(&requested)?;
        Ok(Self {
            requested,
            schema_revision: schema.revision(),
            resolved,
        })
    }

    /// Original request, retained so the context cannot silently widen it.
    #[must_use]
    pub const fn requested(&self) -> &LayerSelection {
        &self.requested
    }
    /// Exact schema revision used to validate the layer choice.
    #[must_use]
    pub const fn schema_revision(&self) -> SchemaRevision {
        self.schema_revision
    }
    /// Non-empty concrete layers selected against that schema.
    #[must_use]
    pub const fn resolved(&self) -> &NonEmptySet<LayerId> {
        &self.resolved
    }
}

/// Every required input to construct a query context. There are no defaults.
#[derive(Clone, Debug)]
pub struct QueryContextInput {
    /// Resolved session-local snapshot selected by the trusted engine host.
    pub snapshot: SnapshotRef,
    /// Published data revision pinned by `snapshot`.
    pub snapshot_revision: Revision,
    /// Transaction-time read point; cannot exceed the pinned snapshot.
    pub recorded_as_of: RecordedAsOf,
    /// Required HistorySpace selection.
    pub history_space: HistorySpaceId,
    /// Layer selection already validated against the selected schema snapshot.
    pub layers: ValidatedLayerSelection,
    /// Explicit world-time interpretation.
    pub world_time: WorldTimeSelector,
    /// Perspective/epistemic selection; `WorldState` is perspective-free.
    pub perspective: PerspectiveScope,
    /// Explicit epistemic partition.
    pub epistemic_mode: EpistemicMode,
    /// Fully resolved schema interpretation bound to the data read point.
    pub schema_binding: HistoricalQueryBinding,
    /// Host-bound Principal and authorization time basis.
    pub security: SecurityContext,
    /// Positive finite semantic budget.
    pub budget: QueryBudget,
    /// Out-of-band host cancellation state.
    pub cancellation: CancellationToken,
}

/// Complete immutable query context; all required axes are bound before execution.
#[derive(Debug)]
pub struct QueryContext {
    snapshot: SnapshotRef,
    snapshot_revision: Revision,
    recorded_as_of: RecordedAsOf,
    history_space: HistorySpaceId,
    layers: ValidatedLayerSelection,
    world_time: WorldTimeSelector,
    perspective: PerspectiveScope,
    epistemic_mode: EpistemicMode,
    schema_binding: HistoricalQueryBinding,
    security: SecurityContext,
    budget: QueryBudget,
    cancellation: CancellationToken,
    resource_budget: ProcessMemoryBudget,
    snapshot_lease: Option<SnapshotLease>,
}

/// Immutable semantic axes bound to one query result, excluding only out-of-band cancellation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryContextBinding {
    snapshot: SnapshotRef,
    snapshot_revision: Revision,
    recorded_as_of: RecordedAsOf,
    history_space: HistorySpaceId,
    requested_layers: LayerSelection,
    resolved_layers: Vec<LayerId>,
    world_time: WorldTimeSelector,
    perspective: PerspectiveScope,
    epistemic_mode: EpistemicMode,
    schema_binding: HistoricalQueryBinding,
    security: SecurityContext,
    budget: QueryBudget,
}

impl QueryContextBinding {
    /// Concrete session-local snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> SnapshotRef {
        self.snapshot
    }

    /// Immutable published data revision pinned by the snapshot.
    #[must_use]
    pub const fn snapshot_revision(&self) -> Revision {
        self.snapshot_revision
    }

    /// Complete semantic matching test for another constructed query context.
    #[must_use]
    pub fn matches(&self, context: &QueryContext) -> bool {
        self == &context.binding()
    }
}

impl QueryContext {
    /// Validates all cross-field invariants before constructing an executable context.
    pub fn new(input: QueryContextInput) -> Result<Self, QueryContextError> {
        Self::new_with_resource_budget(input, process_memory_budget().clone())
    }

    /// Validates all query pins and binds a caller-supplied shared process ledger.
    ///
    /// Runtime hosts should pass the same ledger to every context created for one
    /// process. [`new`](Self::new) uses the configured process default.
    pub fn new_with_resource_budget(
        input: QueryContextInput,
        resource_budget: ProcessMemoryBudget,
    ) -> Result<Self, QueryContextError> {
        if input.recorded_as_of.revision() > input.snapshot_revision {
            return Err(QueryContextError::AsOfAfterSnapshot);
        }
        if !valid_perspective_mode(input.perspective, input.epistemic_mode) {
            return Err(QueryContextError::InvalidPerspectiveModePair);
        }
        if input.schema_binding.recorded_as_of() != input.recorded_as_of {
            return Err(QueryContextError::SchemaBindingAsOfMismatch);
        }
        if input.schema_binding.schema_revision() != input.layers.schema_revision() {
            return Err(QueryContextError::SchemaLayerRevisionMismatch);
        }
        match input.security.authorization_mode() {
            AuthorizationMode::AtRevision(revision) if revision > input.snapshot_revision => {
                return Err(QueryContextError::AuthorizationRevisionAfterSnapshot);
            }
            AuthorizationMode::Now | AuthorizationMode::AtRevision(_) => {}
        }
        Ok(Self {
            snapshot: input.snapshot,
            snapshot_revision: input.snapshot_revision,
            recorded_as_of: input.recorded_as_of,
            history_space: input.history_space,
            layers: input.layers,
            world_time: input.world_time,
            perspective: input.perspective,
            epistemic_mode: input.epistemic_mode,
            schema_binding: input.schema_binding,
            security: input.security,
            budget: input.budget,
            cancellation: input.cancellation,
            resource_budget,
            snapshot_lease: None,
        })
    }

    /// Constructs a query context whose semantic axes match a live registered snapshot lease.
    ///
    /// Reference-model callers may use [`new`](Self::new). Runtime query entry points
    /// should use this constructor so every read owns the pin that protects its view.
    pub fn new_leased(
        input: QueryContextInput,
        lease: SnapshotLease,
        now_ms: u64,
    ) -> Result<Self, QueryContextError> {
        let context = Self::new(input)?;
        let pinned = lease
            .binding()
            .map_err(|_| QueryContextError::SnapshotLeaseUnavailable)?;
        let resolved_layers = pinned
            .layer_schema()
            .resolve(context.layers.requested())
            .map_err(|_| QueryContextError::SnapshotLeaseMismatch)?;
        if pinned.snapshot() != context.snapshot
            || pinned.data_revision() != context.snapshot_revision
            || pinned.recorded_as_of() != context.recorded_as_of
            || pinned.schema() != context.schema_binding
            || pinned.history_space().selected() != context.history_space
            || pinned.layer_selection() != context.layers.requested()
            || resolved_layers.as_slice() != context.layers.resolved().as_slice()
            || pinned.security().principal_id() != context.security.principal_id()
            || pinned.security().authorization_mode() != context.security.authorization_mode()
        {
            return Err(QueryContextError::SnapshotLeaseMismatch);
        }
        match lease.lifetime_status(now_ms) {
            Ok(
                SnapshotLifetimeStatus::WithinBudget
                | SnapshotLifetimeStatus::SoftLimitExceeded { .. },
            ) => {}
            Ok(SnapshotLifetimeStatus::Expired { .. }) => {
                return Err(QueryContextError::SnapshotExpired);
            }
            Err(_) => return Err(QueryContextError::SnapshotLeaseUnavailable),
        }
        let mut context = context;
        context.snapshot_lease = Some(lease);
        Ok(context)
    }

    /// Captures every semantic axis used by a result, excluding host cancellation state.
    #[must_use]
    pub fn binding(&self) -> QueryContextBinding {
        QueryContextBinding {
            snapshot: self.snapshot,
            snapshot_revision: self.snapshot_revision,
            recorded_as_of: self.recorded_as_of,
            history_space: self.history_space,
            requested_layers: self.layers.requested().clone(),
            resolved_layers: self.layers.resolved().as_slice().to_vec(),
            world_time: self.world_time,
            perspective: self.perspective,
            epistemic_mode: self.epistemic_mode,
            schema_binding: self.schema_binding,
            security: self.security,
            budget: self.budget,
        }
    }

    /// Concrete pinned snapshot reference.
    #[must_use]
    pub const fn snapshot(&self) -> SnapshotRef {
        self.snapshot
    }

    /// Whether this context owns a registered storage pin rather than only a reference-model ID.
    #[must_use]
    pub const fn has_snapshot_lease(&self) -> bool {
        self.snapshot_lease.is_some()
    }

    /// Checks the owned lease at a query/commit boundary; reference contexts have no storage pin.
    pub fn ensure_snapshot_live(&self, now_ms: u64) -> Result<(), crate::errors::QueryError> {
        match &self.snapshot_lease {
            Some(lease) => lease.ensure_live(now_ms),
            None => Ok(()),
        }
    }

    /// Registers an independent pin for a continuation cursor, if this context is leased.
    /// The returned lease is owned by the cursor store and is released when its state is
    /// consumed, invalidated, expired, or the store is dropped.
    pub fn fork_snapshot_lease(
        &self,
        now_ms: u64,
    ) -> Result<Option<SnapshotLease>, crate::snapshot_lease::SnapshotError> {
        self.snapshot_lease
            .as_ref()
            .map(|lease| lease.fork_reader(now_ms))
            .transpose()
    }

    /// Published revision pinned by the snapshot.
    #[must_use]
    pub const fn snapshot_revision(&self) -> Revision {
        self.snapshot_revision
    }
    /// Explicit transaction-time read point.
    #[must_use]
    pub const fn recorded_as_of(&self) -> RecordedAsOf {
        self.recorded_as_of
    }
    /// Selected HistorySpace.
    #[must_use]
    pub const fn history_space(&self) -> HistorySpaceId {
        self.history_space
    }
    /// Pinned and schema-validated layer selection.
    #[must_use]
    pub const fn layers(&self) -> &ValidatedLayerSelection {
        &self.layers
    }
    /// World-time interpretation.
    #[must_use]
    pub const fn world_time(&self) -> WorldTimeSelector {
        self.world_time
    }
    /// Perspective selection, independent of security identity.
    #[must_use]
    pub const fn perspective(&self) -> PerspectiveScope {
        self.perspective
    }
    /// Epistemic partition.
    #[must_use]
    pub const fn epistemic_mode(&self) -> EpistemicMode {
        self.epistemic_mode
    }
    /// Explicit schema interpretation mode.
    #[must_use]
    pub const fn schema_mode(&self) -> SchemaMode {
        self.schema_binding.schema_mode()
    }
    /// Full immutable data/schema binding selected when this context was built.
    #[must_use]
    pub const fn schema_binding(&self) -> HistoricalQueryBinding {
        self.schema_binding
    }
    /// Concrete schema revision pinned by the schema binding.
    #[must_use]
    pub const fn schema_revision(&self) -> SchemaRevision {
        self.schema_binding.schema_revision()
    }
    /// Canonical fingerprint of the exact bound schema snapshot.
    #[must_use]
    pub const fn schema_fingerprint(&self) -> [u8; 32] {
        self.schema_binding.schema_fingerprint()
    }
    /// Trusted host-bound security context.
    #[must_use]
    pub const fn security(&self) -> SecurityContext {
        self.security
    }
    /// Finite semantic query budget.
    #[must_use]
    pub const fn budget(&self) -> QueryBudget {
        self.budget
    }
    /// Shared process ledger for bounded query allocations.
    #[must_use]
    pub const fn resource_budget(&self) -> &ProcessMemoryBudget {
        &self.resource_budget
    }
    /// Shared cancellation token.
    #[must_use]
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

fn valid_perspective_mode(perspective: PerspectiveScope, mode: EpistemicMode) -> bool {
    matches!(
        (perspective, mode),
        (PerspectiveScope::World, EpistemicMode::WorldState)
            | (
                PerspectiveScope::Perspective(_),
                EpistemicMode::Knows | EpistemicMode::Believes | EpistemicMode::Claims
            )
    )
}

/// Invalid or internally inconsistent query context.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QueryContextError {
    /// `RecordedAsOf` cannot be later than the pinned data snapshot.
    AsOfAfterSnapshot,
    /// Perspective and epistemic partitions are an invalid pair.
    InvalidPerspectiveModePair,
    /// Schema binding and data transaction-time read point must be the same.
    SchemaBindingAsOfMismatch,
    /// Layer selection must be resolved against the exact bound schema revision.
    SchemaLayerRevisionMismatch,
    /// Historical authorization cannot select a policy revision after the data snapshot.
    AuthorizationRevisionAfterSnapshot,
    /// The lease no longer has an authoritative registry entry.
    SnapshotLeaseUnavailable,
    /// One or more query axes differ from the registered snapshot binding.
    SnapshotLeaseMismatch,
    /// The snapshot lease crossed its configured hard lifetime.
    SnapshotExpired,
}

impl fmt::Display for QueryContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AsOfAfterSnapshot => {
                formatter.write_str("recorded-as-of is after the pinned snapshot")
            }
            Self::InvalidPerspectiveModePair => {
                formatter.write_str("invalid perspective and epistemic mode pair")
            }
            Self::SchemaBindingAsOfMismatch => {
                formatter.write_str("schema binding does not match recorded-as-of")
            }
            Self::SchemaLayerRevisionMismatch => {
                formatter.write_str("layer selection was resolved against another schema revision")
            }
            Self::AuthorizationRevisionAfterSnapshot => {
                formatter.write_str("authorization revision is after the pinned snapshot")
            }
            Self::SnapshotLeaseUnavailable => formatter.write_str("snapshot lease is unavailable"),
            Self::SnapshotLeaseMismatch => {
                formatter.write_str("query axes do not match the snapshot lease")
            }
            Self::SnapshotExpired => formatter.write_str("snapshot lease has expired"),
        }
    }
}

impl std::error::Error for QueryContextError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::HistorySpaceDefinition;
    use crate::ids::{DatabaseId, DomainId, IdValidationError, SecurityEpoch, SnapshotId};
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
    use crate::schema::{Lifecycle, SchemaDefinitionError};
    use crate::schema_history::{
        SchemaDefinition, SchemaHistoryError, SchemaHistoryReferenceModel,
    };
    use crate::snapshot_lease::{
        HistorySpaceView, SnapshotBinding, SnapshotBindingInput, SnapshotLifetimeLimits,
        SnapshotPinPurpose, SnapshotRegistry, SnapshotSecurityBinding,
    };
    use crate::values::Symbol;

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(crate::ids::RevisionError),
        Budget(QueryBudgetError),
        Layer(LayerSchemaError),
        Schema(SchemaDefinitionError),
        Symbol(crate::values::SymbolError),
        Context(QueryContextError),
        SchemaHistory(SchemaHistoryError),
        SecurityPolicy(crate::security::SecurityPolicyError),
        SecurityHistory(crate::security::SecurityPolicyHistoryError),
        QueryPort(crate::query_ports::QueryPortError),
        Resolved(crate::reference_query::ResolvedViewError),
        HistorySpace(crate::HistorySpaceError),
        History(crate::HistorySpaceModelError),
        RawHistory(crate::RawHistoryError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(e) => write!(f, "{e}"),
                Self::Revision(e) => write!(f, "{e}"),
                Self::Budget(e) => write!(f, "{e}"),
                Self::Layer(e) => write!(f, "{e}"),
                Self::Schema(e) => write!(f, "{e}"),
                Self::Symbol(e) => write!(f, "{e}"),
                Self::Context(e) => write!(f, "{e}"),
                Self::SchemaHistory(e) => write!(f, "{e}"),
                Self::SecurityPolicy(e) => write!(f, "{e}"),
                Self::SecurityHistory(e) => write!(f, "{e}"),
                Self::QueryPort(e) => write!(f, "{e}"),
                Self::Resolved(e) => write!(f, "{e}"),
                Self::HistorySpace(e) => write!(f, "{e}"),
                Self::History(e) => write!(f, "{e}"),
                Self::RawHistory(e) => write!(f, "{e}"),
            }
        }
    }

    impl std::error::Error for TestError {}
    impl From<IdValidationError> for TestError {
        fn from(e: IdValidationError) -> Self {
            Self::Id(e)
        }
    }
    impl From<crate::ids::RevisionError> for TestError {
        fn from(e: crate::ids::RevisionError) -> Self {
            Self::Revision(e)
        }
    }
    impl From<QueryBudgetError> for TestError {
        fn from(e: QueryBudgetError) -> Self {
            Self::Budget(e)
        }
    }
    impl From<LayerSchemaError> for TestError {
        fn from(e: LayerSchemaError) -> Self {
            Self::Layer(e)
        }
    }
    impl From<SchemaDefinitionError> for TestError {
        fn from(e: SchemaDefinitionError) -> Self {
            Self::Schema(e)
        }
    }
    impl From<crate::values::SymbolError> for TestError {
        fn from(e: crate::values::SymbolError) -> Self {
            Self::Symbol(e)
        }
    }
    impl From<QueryContextError> for TestError {
        fn from(e: QueryContextError) -> Self {
            Self::Context(e)
        }
    }
    impl From<SchemaHistoryError> for TestError {
        fn from(e: SchemaHistoryError) -> Self {
            Self::SchemaHistory(e)
        }
    }
    impl From<crate::security::SecurityPolicyError> for TestError {
        fn from(e: crate::security::SecurityPolicyError) -> Self {
            Self::SecurityPolicy(e)
        }
    }
    impl From<crate::security::SecurityPolicyHistoryError> for TestError {
        fn from(e: crate::security::SecurityPolicyHistoryError) -> Self {
            Self::SecurityHistory(e)
        }
    }
    impl From<crate::query_ports::QueryPortError> for TestError {
        fn from(e: crate::query_ports::QueryPortError) -> Self {
            Self::QueryPort(e)
        }
    }
    impl From<crate::reference_query::ResolvedViewError> for TestError {
        fn from(e: crate::reference_query::ResolvedViewError) -> Self {
            Self::Resolved(e)
        }
    }
    impl From<crate::HistorySpaceError> for TestError {
        fn from(e: crate::HistorySpaceError) -> Self {
            Self::HistorySpace(e)
        }
    }
    impl From<crate::HistorySpaceModelError> for TestError {
        fn from(e: crate::HistorySpaceModelError) -> Self {
            Self::History(e)
        }
    }
    impl From<crate::RawHistoryError> for TestError {
        fn from(e: crate::RawHistoryError) -> Self {
            Self::RawHistory(e)
        }
    }

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn id<T: DomainId>(byte: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, crate::ids::RevisionError> {
        Revision::new(value)
    }

    fn schema() -> Result<(LayerSchemaSnapshot, SchemaHistoryReferenceModel), TestError> {
        let layer_id = value!(id::<LayerId>(1));
        let schema_revision = SchemaRevision::from_published_revision(value!(revision(2)));
        let definition = LayerDefinition::new(
            layer_id,
            value!(Symbol::new("world")),
            None,
            0,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(value!(revision(0))),
        );
        let snapshot = value!(LayerSchemaSnapshot::new(
            schema_revision,
            vec![definition],
            layer_id
        ));
        let mut history = SchemaHistoryReferenceModel::new();
        history.publish(
            schema_revision.revision(),
            vec![SchemaDefinition::LayerSnapshot(snapshot.clone())],
        )?;
        Ok((snapshot, history))
    }

    fn schema_binding(
        history: &SchemaHistoryReferenceModel,
        as_of: RecordedAsOf,
        mode: SchemaMode,
    ) -> Result<HistoricalQueryBinding, SchemaHistoryError> {
        HistoricalQueryBinding::bind(history, as_of, mode)
    }

    fn input() -> Result<QueryContextInput, TestError> {
        let (schema, history) = value!(schema());
        let as_of = RecordedAsOf::from_published_revision(value!(revision(2)));
        let layers = value!(ValidatedLayerSelection::resolve(
            &schema,
            LayerSelection::BaseOnly
        ));
        let budget_limits = value!(QueryBudgetLimits::new(100, 1_000, 1_000));
        let budget = value!(QueryBudget::new(10, 100, 100, budget_limits));
        Ok(QueryContextInput {
            snapshot: SnapshotRef::new(value!(id::<SnapshotId>(2))),
            snapshot_revision: value!(revision(3)),
            recorded_as_of: as_of,
            history_space: value!(id::<HistorySpaceId>(3)),
            layers,
            world_time: WorldTimeSelector::AllTimes,
            perspective: PerspectiveScope::World,
            epistemic_mode: EpistemicMode::WorldState,
            schema_binding: value!(schema_binding(&history, as_of, SchemaMode::Historical)),
            security: SecurityContext::new(value!(id::<PrincipalId>(4)), AuthorizationMode::Now),
            budget,
            cancellation: CancellationToken::new(),
        })
    }

    fn lease_for(input: &QueryContextInput) -> Result<SnapshotLease, String> {
        let (layers, _) = schema().map_err(|error| error.to_string())?;
        let space = HistorySpaceDefinition::new(input.history_space, None, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        let binding = SnapshotBinding::new(SnapshotBindingInput {
            database_id: id::<DatabaseId>(8).map_err(|error| error.to_string())?,
            snapshot_id: input.snapshot.id(),
            data_revision: input.snapshot_revision,
            recorded_as_of: input.recorded_as_of,
            schema: input.schema_binding,
            history_space: HistorySpaceView::new(vec![space]).map_err(|error| error.to_string())?,
            layer_schema: layers,
            layer_selection: input.layers.requested().clone(),
            security: SnapshotSecurityBinding::new(
                input.security.principal_id(),
                input.security.authorization_mode(),
                SecurityEpoch::INITIAL,
            ),
            backend_generation: 4,
        })
        .map_err(|error| error.to_string())?;
        let limits = SnapshotLifetimeLimits::new(10, 20, 100).map_err(|error| error.to_string())?;
        SnapshotRegistry::new(limits)
            .pin(binding, SnapshotPinPurpose::Interactive, 0)
            .map_err(|error| error.to_string())
    }

    #[test]
    fn valid_context_binds_every_axis_and_pinned_layers() -> Result<(), TestError> {
        let context = value!(QueryContext::new(value!(input())));
        assert_eq!(context.recorded_as_of().revision(), revision(2)?);
        assert_eq!(context.layers().resolved().as_slice().len(), 1);
        assert_eq!(context.schema_mode(), SchemaMode::Historical);
        assert_eq!(context.schema_revision().revision(), revision(2)?);
        assert_eq!(context.perspective(), PerspectiveScope::World);
        assert_eq!(
            context.security().authorization_mode(),
            AuthorizationMode::Now
        );
        assert_eq!(context.budget().max_results().get(), 100);
        Ok(())
    }

    #[test]
    fn leased_context_owns_matching_pin_and_expires_at_hard_limit() -> Result<(), String> {
        let input = input().map_err(|error| error.to_string())?;
        let registry_lease = lease_for(&input)?;
        let context = QueryContext::new_leased(input, registry_lease, 1)
            .map_err(|error| error.to_string())?;

        assert!(context.has_snapshot_lease());
        assert_eq!(context.ensure_snapshot_live(19), Ok(()));
        let cursor_lease = context
            .fork_snapshot_lease(1)
            .map_err(|error| error.to_string())?;
        assert!(cursor_lease.is_some());
        assert_eq!(
            context.ensure_snapshot_live(20),
            Err(crate::QueryError::SnapshotExpired)
        );
        drop(cursor_lease);
        Ok(())
    }

    #[test]
    fn leased_context_rejects_a_revision_that_differs_from_its_pin() -> Result<(), String> {
        let mut input = input().map_err(|error| error.to_string())?;
        let registry_lease = lease_for(&input)?;
        input.snapshot_revision = Revision::new(4).map_err(|error| error.to_string())?;
        assert_eq!(
            QueryContext::new_leased(input, registry_lease, 1).err(),
            Some(QueryContextError::SnapshotLeaseMismatch)
        );
        Ok(())
    }

    #[test]
    fn owned_query_context_binding_covers_snapshot_and_all_semantic_axes() -> Result<(), TestError>
    {
        let context = QueryContext::new(input()?)?;
        let binding = context.binding();
        assert!(binding.matches(&context));

        let mut other_input = input()?;
        other_input.snapshot = SnapshotRef::new(id::<SnapshotId>(99)?);
        let other_context = QueryContext::new(other_input)?;
        assert!(!binding.matches(&other_context));
        Ok(())
    }

    #[test]
    fn owned_resolved_port_pins_schema_and_security_epochs() -> Result<(), TestError> {
        use crate::record_refs::RecordRef;
        use crate::security::{
            Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyScope, PolicySubject,
            Principal, SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
        };
        use crate::single_value_resolution::SingleValueOutcome;

        let context = QueryContext::new(input()?)?;
        let principal = context.security().principal_id();
        let rights = [
            Capability::HistorySpaceRead,
            Capability::RawHistoryRead,
            Capability::AssertionRead,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, capability)| {
            let rule_id = id::<crate::ids::PolicyRuleId>((40 + index) as u8)?;
            Ok(CapabilityRule::new(
                rule_id,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, IdValidationError>>()?;
        let policy =
            SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rights)?;
        let versions = vec![
            SecurityPolicyVersion::new(
                Revision::GENESIS,
                crate::SecurityEpoch::INITIAL,
                policy.clone(),
            ),
            SecurityPolicyVersion::new(revision(1)?, crate::SecurityEpoch::INITIAL, policy.clone()),
            SecurityPolicyVersion::new(revision(2)?, crate::SecurityEpoch::INITIAL, policy.clone()),
            SecurityPolicyVersion::new(revision(3)?, crate::SecurityEpoch::INITIAL, policy),
        ];
        let policies = SecurityPolicyHistory::new(revision(3)?, versions)?;
        let view = crate::reference_query::ResolvedView::from_single(SingleValueOutcome::Unknown)?;
        let result =
            crate::query_ports::bind_authorized_resolved_view(&context, &policies, &[], view)?;
        assert_eq!(result.binding(), context.schema_binding());
        assert_eq!(
            result.current_security_epoch(),
            crate::SecurityEpoch::INITIAL
        );
        assert_eq!(
            result.evaluated_security_epoch(),
            crate::SecurityEpoch::INITIAL
        );
        assert!(matches!(
            result.value().outcome(),
            crate::ResolvedOutcome::Single(SingleValueOutcome::Unknown)
        ));

        #[derive(Clone)]
        struct RawRecord(RecordRef);
        let space = context.history_space();
        let record = RecordRef::Assertion(value!(id::<crate::ids::AssertionId>(50)));
        let mut raw_history =
            crate::HistorySpaceReferenceModel::new(vec![crate::HistorySpaceDefinition::new(
                space,
                None,
                Revision::GENESIS,
            )?])?;
        raw_history.publish(space, vec![RawRecord(record)])?;
        raw_history.publish(
            space,
            vec![RawRecord(RecordRef::Assertion(value!(id::<
                crate::ids::AssertionId,
            >(51))))],
        )?;
        let raw = crate::query_ports::full_scan_owned_authorized_raw_history(
            &raw_history,
            &context,
            &policies,
            |row| row.0,
        )?;
        assert_eq!(raw.binding(), context.schema_binding());
        assert_eq!(raw.value().len(), 2);
        assert_eq!(
            raw.value().first().map(|row| row.record_ref()),
            Some(record)
        );
        Ok(())
    }

    #[test]
    fn context_rejects_future_data_and_policy_revisions() -> Result<(), TestError> {
        let mut after_snapshot = value!(input());
        after_snapshot.recorded_as_of = RecordedAsOf::from_published_revision(value!(revision(4)));
        assert_eq!(
            QueryContext::new(after_snapshot).err(),
            Some(QueryContextError::AsOfAfterSnapshot)
        );

        let mut future_policy = value!(input());
        future_policy.security = SecurityContext::new(
            future_policy.security.principal_id(),
            AuthorizationMode::AtRevision(value!(revision(4))),
        );
        assert_eq!(
            QueryContext::new(future_policy).err(),
            Some(QueryContextError::AuthorizationRevisionAfterSnapshot)
        );
        Ok(())
    }

    #[test]
    fn context_rejects_invalid_partitions_and_schema_pins() -> Result<(), TestError> {
        let mut invalid_pair = value!(input());
        invalid_pair.epistemic_mode = EpistemicMode::Knows;
        assert_eq!(
            QueryContext::new(invalid_pair).err(),
            Some(QueryContextError::InvalidPerspectiveModePair)
        );

        let mut future_historical_schema = value!(input());
        future_historical_schema.recorded_as_of =
            RecordedAsOf::from_published_revision(value!(revision(1)));
        assert_eq!(
            QueryContext::new(future_historical_schema).err(),
            Some(QueryContextError::SchemaBindingAsOfMismatch)
        );

        let mut explicit_mismatch = value!(input());
        let (_, history) = value!(schema());
        let as_of = explicit_mismatch.recorded_as_of;
        explicit_mismatch.schema_binding = value!(schema_binding(
            &history,
            as_of,
            SchemaMode::Explicit(SchemaRevision::from_published_revision(value!(revision(1))))
        ));
        assert_eq!(
            QueryContext::new(explicit_mismatch).err(),
            Some(QueryContextError::SchemaLayerRevisionMismatch)
        );
        Ok(())
    }

    #[test]
    fn budgets_are_positive_finite_and_bounded() -> Result<(), TestError> {
        assert_eq!(
            QueryBudgetLimits::new(0, 1, 1).err(),
            Some(QueryBudgetError::ZeroLimit(BudgetDimension::Candidates))
        );
        assert_eq!(
            QueryBudgetLimits::new(MAX_QUERY_CANDIDATES + 1, 1, 1).err(),
            Some(QueryBudgetError::ExceedsAbsoluteMaximum {
                dimension: BudgetDimension::Candidates,
                requested: MAX_QUERY_CANDIDATES + 1,
                maximum: MAX_QUERY_CANDIDATES,
            })
        );
        assert!(
            QueryBudgetLimits::new(
                MAX_QUERY_CANDIDATES,
                MAX_QUERY_WORK_UNITS,
                MAX_QUERY_RESULTS,
            )
            .is_ok()
        );
        let limits = value!(QueryBudgetLimits::new(5, 20, 10));
        assert_eq!(
            QueryBudget::new(1, 0, 1, limits).err(),
            Some(QueryBudgetError::ZeroRequested(BudgetDimension::WorkUnits))
        );
        assert_eq!(
            QueryBudget::new(6, 20, 10, limits).err(),
            Some(QueryBudgetError::ExceedsHardMaximum {
                dimension: BudgetDimension::Candidates,
                requested: 6,
                hard_maximum: 5
            })
        );
        Ok(())
    }

    #[test]
    fn layer_selection_must_resolve_against_the_bound_schema() -> Result<(), TestError> {
        let (schema, _) = value!(schema());
        let unknown_layer = value!(id::<LayerId>(9));
        let explicit = crate::schema::NonEmptySet::new(vec![unknown_layer]);
        assert!(explicit.is_ok());
        if let Ok(explicit) = explicit {
            assert_eq!(
                ValidatedLayerSelection::resolve(&schema, LayerSelection::Explicit(explicit)).err(),
                Some(LayerSchemaError::UnknownSelectedLayer)
            );
        }
        Ok(())
    }

    #[test]
    fn cancellation_is_shared_and_distinct_from_budget() {
        let token = CancellationToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }
}
