//! Snapshot-bounded Assertion point and history indexes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::assertions::{Assertion, AssertionRetraction, AssertionValidityClosure, Subject};
use crate::candidate_scan::AssertionHistoryRecord;
use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
use crate::history_model::{HistorySpaceModelError, HistorySpaceReferenceModel};
use crate::ids::{AssertionId, HistorySpaceId, LayerId, PredicateId, Revision};
use crate::resource_profile::{
    MemoryReservation, ProcessMemoryBudget, ResourceClass, process_memory_budget,
};

const INDEXED_ASSERTION_ACCOUNTING_BYTES: u64 = 384;
const INDEXED_LIFECYCLE_ACCOUNTING_BYTES: u64 = 192;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IndexPerspective {
    World,
    Perspective(crate::ids::PerspectiveId),
}

impl From<PerspectiveScope> for IndexPerspective {
    fn from(value: PerspectiveScope) -> Self {
        match value {
            PerspectiveScope::World => Self::World,
            PerspectiveScope::Perspective(id) => Self::Perspective(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IndexEpistemicMode {
    WorldState,
    Knows,
    Believes,
    Claims,
}

impl From<EpistemicMode> for IndexEpistemicMode {
    fn from(value: EpistemicMode) -> Self {
        match value {
            EpistemicMode::WorldState => Self::WorldState,
            EpistemicMode::Knows => Self::Knows,
            EpistemicMode::Believes => Self::Believes,
            EpistemicMode::Claims => Self::Claims,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PointKey {
    owner_history_space_id: HistorySpaceId,
    layer_id: LayerId,
    perspective: IndexPerspective,
    epistemic_mode: IndexEpistemicMode,
    subject: Subject,
    predicate_id: PredicateId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EntityHistoryKey {
    owner_history_space_id: HistorySpaceId,
    layer_id: LayerId,
    perspective: IndexPerspective,
    epistemic_mode: IndexEpistemicMode,
    subject: Subject,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PredicateHistoryKey {
    owner_history_space_id: HistorySpaceId,
    layer_id: LayerId,
    perspective: IndexPerspective,
    epistemic_mode: IndexEpistemicMode,
    predicate_id: PredicateId,
}

#[derive(Clone, Debug)]
struct AssertionPosting {
    assertion: Arc<Assertion>,
    owner_history_space_id: HistorySpaceId,
    recorded_revision: Revision,
}

#[derive(Clone, Debug)]
enum IndexedAssertionLifecycleRecord {
    ValidityClosure(AssertionValidityClosure),
    Retraction(AssertionRetraction),
}

/// One index hit with the immutable Assertion and its original commit coordinates.
#[derive(Clone, Copy, Debug)]
pub struct AssertionIndexHit<'a> {
    posting: &'a AssertionPosting,
}

impl<'a> AssertionIndexHit<'a> {
    /// Returns the immutable Assertion payload.
    #[must_use]
    pub fn assertion(self) -> &'a Assertion {
        self.posting.assertion.as_ref()
    }

    /// Returns the HistorySpace that committed the Assertion.
    #[must_use]
    pub const fn owner_history_space_id(self) -> HistorySpaceId {
        self.posting.owner_history_space_id
    }

    /// Returns the revision that committed the Assertion.
    #[must_use]
    pub const fn recorded_revision(self) -> Revision {
        self.posting.recorded_revision
    }
}

/// Exact Point lookup coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssertionPointQuery {
    context: ContextKey,
    subject: Subject,
    predicate_id: PredicateId,
    as_of: Revision,
}

impl AssertionPointQuery {
    /// Binds one validated context, subject/predicate pair, and inclusive revision.
    #[must_use]
    pub const fn new(
        context: ContextKey,
        subject: Subject,
        predicate_id: PredicateId,
        as_of: Revision,
    ) -> Self {
        Self {
            context,
            subject,
            predicate_id,
            as_of,
        }
    }
}

/// Entity-history lookup coordinates for one subject in one exact context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssertionEntityHistoryQuery {
    context: ContextKey,
    subject: Subject,
    as_of: Revision,
}

impl AssertionEntityHistoryQuery {
    /// Binds one validated context, subject, and inclusive revision.
    #[must_use]
    pub const fn new(context: ContextKey, subject: Subject, as_of: Revision) -> Self {
        Self {
            context,
            subject,
            as_of,
        }
    }
}

/// Predicate-history lookup coordinates for one predicate in one exact context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssertionPredicateHistoryQuery {
    context: ContextKey,
    predicate_id: PredicateId,
    as_of: Revision,
}

impl AssertionPredicateHistoryQuery {
    /// Binds one validated context, predicate, and inclusive revision.
    #[must_use]
    pub const fn new(context: ContextKey, predicate_id: PredicateId, as_of: Revision) -> Self {
        Self {
            context,
            predicate_id,
            as_of,
        }
    }
}

/// Local Assertion postings for a fixed HistorySpace catalog and published revision.
///
/// Postings remain attached to the HistorySpace that created them. Reads walk the
/// selected space's ancestry and apply each child's immutable parent cutoff; parent
/// history is never copied into a child index partition.
#[derive(Clone, Debug)]
pub struct AssertionPointHistoryIndex {
    indexed_through: Revision,
    catalog: crate::catalog::HistorySpaceCatalog,
    postings: Vec<AssertionPosting>,
    point: BTreeMap<PointKey, Vec<usize>>,
    entity_history: BTreeMap<EntityHistoryKey, Vec<usize>>,
    predicate_history: BTreeMap<PredicateHistoryKey, Vec<usize>>,
    lifecycle:
        BTreeMap<(HistorySpaceId, AssertionId), Vec<(Revision, IndexedAssertionLifecycleRecord)>>,
    _memory_reservations: Vec<MemoryReservation>,
}

impl AssertionPointHistoryIndex {
    /// Builds local-delta postings from the index-free HistorySpace oracle.
    pub fn build(
        history: &HistorySpaceReferenceModel<AssertionHistoryRecord>,
    ) -> Result<Self, AssertionPointIndexError> {
        Self::build_with_memory_budget(history, process_memory_budget().clone())
    }

    /// Builds a complete generation while charging the supplied shared process ledger.
    pub fn build_with_memory_budget(
        history: &HistorySpaceReferenceModel<AssertionHistoryRecord>,
        memory_budget: ProcessMemoryBudget,
    ) -> Result<Self, AssertionPointIndexError> {
        let indexed_through = history.latest_published();
        let catalog = history.catalog().clone();
        let mut index = Self {
            indexed_through,
            catalog,
            postings: Vec::new(),
            point: BTreeMap::new(),
            entity_history: BTreeMap::new(),
            predicate_history: BTreeMap::new(),
            lifecycle: BTreeMap::new(),
            _memory_reservations: Vec::new(),
        };
        let mut assertion_ids = BTreeSet::new();
        let definitions = index.catalog.definitions().to_vec();

        for definition in &definitions {
            let owner_history_space_id = definition.history_space_id();
            for (stored_revision, visible_owner, record) in
                history.iter_at(owner_history_space_id, indexed_through)?
            {
                // Each space's read exposes its local entries and pinned ancestry.
                // Keep only this space's local delta; each ancestor is indexed once
                // when its own definition is visited.
                if visible_owner != owner_history_space_id {
                    continue;
                }
                match record {
                    AssertionHistoryRecord::Assertion(assertion) => {
                        index.reserve_index_memory(
                            &memory_budget,
                            INDEXED_ASSERTION_ACCOUNTING_BYTES,
                        )?;
                        let assertion = Arc::clone(assertion);
                        let record_revision = assertion.created_revision();
                        if stored_revision != record_revision {
                            return Err(AssertionPointIndexError::StoredRevisionMismatch {
                                owner_history_space_id,
                                stored_revision,
                                record_revision,
                            });
                        }
                        if assertion.context().history_space_id() != owner_history_space_id {
                            return Err(AssertionPointIndexError::ContextHistorySpaceMismatch {
                                assertion_id: assertion.id(),
                                owner_history_space_id,
                                context_history_space_id: assertion.context().history_space_id(),
                            });
                        }
                        if !assertion_ids.insert(assertion.id()) {
                            return Err(AssertionPointIndexError::DuplicateAssertionId(
                                assertion.id(),
                            ));
                        }

                        let context = assertion.context();
                        let subject = assertion.subject();
                        let predicate_id = assertion.predicate_id();
                        let position = index.postings.len();
                        index.postings.push(AssertionPosting {
                            assertion,
                            owner_history_space_id,
                            recorded_revision: stored_revision,
                        });
                        let perspective = IndexPerspective::from(context.perspective_scope());
                        let epistemic_mode = IndexEpistemicMode::from(context.epistemic_mode());
                        index
                            .point
                            .entry(PointKey {
                                owner_history_space_id,
                                layer_id: context.layer_id(),
                                perspective,
                                epistemic_mode,
                                subject,
                                predicate_id,
                            })
                            .or_default()
                            .push(position);
                        index
                            .entity_history
                            .entry(EntityHistoryKey {
                                owner_history_space_id,
                                layer_id: context.layer_id(),
                                perspective,
                                epistemic_mode,
                                subject,
                            })
                            .or_default()
                            .push(position);
                        index
                            .predicate_history
                            .entry(PredicateHistoryKey {
                                owner_history_space_id,
                                layer_id: context.layer_id(),
                                perspective,
                                epistemic_mode,
                                predicate_id,
                            })
                            .or_default()
                            .push(position);
                    }
                    AssertionHistoryRecord::ValidityClosure(closure) => {
                        index.reserve_index_memory(
                            &memory_budget,
                            INDEXED_LIFECYCLE_ACCOUNTING_BYTES,
                        )?;
                        if stored_revision != closure.created_revision() {
                            return Err(AssertionPointIndexError::StoredRevisionMismatch {
                                owner_history_space_id,
                                stored_revision,
                                record_revision: closure.created_revision(),
                            });
                        }
                        index
                            .lifecycle
                            .entry((owner_history_space_id, closure.assertion_id()))
                            .or_default()
                            .push((
                                stored_revision,
                                IndexedAssertionLifecycleRecord::ValidityClosure(*closure),
                            ));
                    }
                    AssertionHistoryRecord::Retraction(retraction) => {
                        index.reserve_index_memory(
                            &memory_budget,
                            INDEXED_LIFECYCLE_ACCOUNTING_BYTES,
                        )?;
                        if stored_revision != retraction.created_revision() {
                            return Err(AssertionPointIndexError::StoredRevisionMismatch {
                                owner_history_space_id,
                                stored_revision,
                                record_revision: retraction.created_revision(),
                            });
                        }
                        index
                            .lifecycle
                            .entry((owner_history_space_id, retraction.assertion_id()))
                            .or_default()
                            .push((
                                stored_revision,
                                IndexedAssertionLifecycleRecord::Retraction(retraction.clone()),
                            ));
                    }
                }
            }
        }
        Ok(index)
    }

    fn reserve_index_memory(
        &mut self,
        memory_budget: &ProcessMemoryBudget,
        bytes: u64,
    ) -> Result<(), AssertionPointIndexError> {
        self._memory_reservations
            .try_reserve(1)
            .map_err(|_| AssertionPointIndexError::AllocationFailed)?;
        let reservation = memory_budget
            .reserve(ResourceClass::Index, bytes)
            .map_err(|_| AssertionPointIndexError::ResourceBudgetExceeded)?;
        self._memory_reservations.push(reservation);
        Ok(())
    }

    /// Returns the inclusive revision through which this generation is complete.
    #[must_use]
    pub const fn indexed_through(&self) -> Revision {
        self.indexed_through
    }

    /// Returns the immutable HistorySpace catalog pinned into this generation.
    #[must_use]
    pub(crate) fn catalog(&self) -> &crate::catalog::HistorySpaceCatalog {
        &self.catalog
    }

    /// Finds all Assertions at one exact Entity/Predicate point through `as_of`.
    pub fn point(
        &self,
        query: AssertionPointQuery,
    ) -> Result<Vec<AssertionIndexHit<'_>>, AssertionPointIndexError> {
        let mut hits = Vec::new();
        let visited = self.try_for_each_point_hit(query, |hit| {
            hits.push(hit);
            Ok::<(), std::convert::Infallible>(())
        })?;
        if let Err(never) = visited {
            match never {}
        }
        hits.sort_unstable_by_key(|hit| {
            (hit.posting.recorded_revision, hit.posting.assertion.id())
        });
        Ok(hits)
    }

    /// Visits point hits without allocating an intermediate hit vector.
    ///
    /// The inner result is the visitor's error; the outer result reports an
    /// invalid index query. Returning from the visitor with an error stops the
    /// scan immediately.
    pub(crate) fn try_for_each_point_hit<'a, E>(
        &'a self,
        query: AssertionPointQuery,
        mut visitor: impl FnMut(AssertionIndexHit<'a>) -> Result<(), E>,
    ) -> Result<Result<(), E>, AssertionPointIndexError> {
        let visible_spaces = self.visible_spaces(query.context.history_space_id(), query.as_of)?;
        let perspective = IndexPerspective::from(query.context.perspective_scope());
        let epistemic_mode = IndexEpistemicMode::from(query.context.epistemic_mode());

        for (owner_history_space_id, cutoff) in visible_spaces {
            let key = PointKey {
                owner_history_space_id,
                layer_id: query.context.layer_id(),
                perspective,
                epistemic_mode,
                subject: query.subject,
                predicate_id: query.predicate_id,
            };
            let Some(positions) = self.point.get(&key) else {
                continue;
            };
            for position in positions {
                let Some(posting) = self.postings.get(*position) else {
                    // A malformed internal position fails closed as a miss.
                    continue;
                };
                if posting.recorded_revision <= cutoff {
                    if let Err(error) = visitor(AssertionIndexHit { posting }) {
                        return Ok(Err(error));
                    }
                }
            }
        }
        Ok(Ok(()))
    }

    /// Finds all Assertions for one subject through `as_of` in ancestry order.
    pub fn entity_history(
        &self,
        query: AssertionEntityHistoryQuery,
    ) -> Result<Vec<AssertionIndexHit<'_>>, AssertionPointIndexError> {
        let visible_spaces = self.visible_spaces(query.context.history_space_id(), query.as_of)?;
        let perspective = IndexPerspective::from(query.context.perspective_scope());
        let epistemic_mode = IndexEpistemicMode::from(query.context.epistemic_mode());
        let hits = collect_hits(
            &self.postings,
            &self.entity_history,
            visible_spaces,
            |owner_history_space_id| EntityHistoryKey {
                owner_history_space_id,
                layer_id: query.context.layer_id(),
                perspective,
                epistemic_mode,
                subject: query.subject,
            },
        );
        Ok(hits)
    }

    /// Finds all Assertions for one predicate through `as_of` in ancestry order.
    pub fn predicate_history(
        &self,
        query: AssertionPredicateHistoryQuery,
    ) -> Result<Vec<AssertionIndexHit<'_>>, AssertionPointIndexError> {
        let visible_spaces = self.visible_spaces(query.context.history_space_id(), query.as_of)?;
        let perspective = IndexPerspective::from(query.context.perspective_scope());
        let epistemic_mode = IndexEpistemicMode::from(query.context.epistemic_mode());
        let hits = collect_hits(
            &self.postings,
            &self.predicate_history,
            visible_spaces,
            |owner_history_space_id| PredicateHistoryKey {
                owner_history_space_id,
                layer_id: query.context.layer_id(),
                perspective,
                epistemic_mode,
                predicate_id: query.predicate_id,
            },
        );
        Ok(hits)
    }

    pub(crate) fn lifecycle_for_assertions(
        &self,
        history_space_id: HistorySpaceId,
        as_of: Revision,
        assertion_ids: &BTreeSet<AssertionId>,
        max_records: u64,
    ) -> Result<(Vec<AssertionValidityClosure>, Vec<AssertionRetraction>), AssertionPointIndexError>
    {
        let visible_spaces = self.visible_spaces(history_space_id, as_of)?;
        let mut closures = Vec::new();
        let mut retractions = Vec::new();
        let mut records_seen = 0_u64;
        for (owner_history_space_id, cutoff) in visible_spaces {
            for assertion_id in assertion_ids {
                let Some(records) = self.lifecycle.get(&(owner_history_space_id, *assertion_id))
                else {
                    continue;
                };
                for (stored_revision, record) in records {
                    if *stored_revision > cutoff || *stored_revision > as_of {
                        continue;
                    }
                    match record {
                        IndexedAssertionLifecycleRecord::ValidityClosure(closure) => {
                            if records_seen >= max_records {
                                return Err(AssertionPointIndexError::LifecycleBudgetExceeded);
                            }
                            closures
                                .try_reserve(1)
                                .map_err(|_| AssertionPointIndexError::AllocationFailed)?;
                            closures.push(*closure);
                            records_seen = records_seen.saturating_add(1);
                        }
                        IndexedAssertionLifecycleRecord::Retraction(retraction) => {
                            if records_seen >= max_records {
                                return Err(AssertionPointIndexError::LifecycleBudgetExceeded);
                            }
                            retractions
                                .try_reserve(1)
                                .map_err(|_| AssertionPointIndexError::AllocationFailed)?;
                            retractions.push(retraction.clone());
                            records_seen = records_seen.saturating_add(1);
                        }
                    }
                }
            }
        }
        Ok((closures, retractions))
    }

    fn visible_spaces(
        &self,
        selected_history_space_id: HistorySpaceId,
        as_of: Revision,
    ) -> Result<Vec<(HistorySpaceId, Revision)>, AssertionPointIndexError> {
        if as_of > self.indexed_through {
            return Err(AssertionPointIndexError::QueryBeyondIndexedRevision {
                requested: as_of,
                indexed_through: self.indexed_through,
            });
        }
        let selected = self
            .catalog
            .definition(selected_history_space_id)
            .ok_or(AssertionPointIndexError::UnknownHistorySpace)?;
        if as_of < selected.base_revision() {
            return Err(AssertionPointIndexError::ReadBeforeBase {
                requested: as_of,
                base_revision: selected.base_revision(),
            });
        }

        let mut visible = Vec::new();
        let mut current = selected;
        let mut cutoff = as_of;
        loop {
            visible.push((current.history_space_id(), cutoff));
            let Some(parent_id) = current.parent_history_space_id() else {
                break;
            };
            cutoff = cutoff.min(current.base_revision());
            current = self
                .catalog
                .definition(parent_id)
                .ok_or(AssertionPointIndexError::UnknownHistorySpace)?;
        }
        Ok(visible)
    }
}

fn collect_hits<'a, K, F>(
    postings: &'a [AssertionPosting],
    index: &BTreeMap<K, Vec<usize>>,
    visible_spaces: Vec<(HistorySpaceId, Revision)>,
    key_for_space: F,
) -> Vec<AssertionIndexHit<'a>>
where
    K: Ord,
    F: Fn(HistorySpaceId) -> K,
{
    let mut hits = Vec::new();
    for (owner_history_space_id, cutoff) in visible_spaces {
        let key = key_for_space(owner_history_space_id);
        let Some(positions) = index.get(&key) else {
            continue;
        };
        for position in positions {
            let Some(posting) = postings.get(*position) else {
                // A malformed internal position fails closed as a miss.
                continue;
            };
            if posting.recorded_revision <= cutoff {
                hits.push(AssertionIndexHit { posting });
            }
        }
    }
    hits.sort_unstable_by_key(|hit| (hit.posting.recorded_revision, hit.posting.assertion.id()));
    hits
}

/// Invalid or incomplete Assertion index source or query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionPointIndexError {
    /// A retained Assertion ID appeared more than once in the source history.
    DuplicateAssertionId(AssertionId),
    /// A point-index result does not match the coordinates used to request it.
    IndexHitMismatch,
    /// The source log revision differs from the immutable Assertion record.
    StoredRevisionMismatch {
        owner_history_space_id: HistorySpaceId,
        stored_revision: Revision,
        record_revision: Revision,
    },
    /// Assertion context names a different HistorySpace than the source log owner.
    ContextHistorySpaceMismatch {
        assertion_id: AssertionId,
        owner_history_space_id: HistorySpaceId,
        context_history_space_id: HistorySpaceId,
    },
    /// The requested HistorySpace does not exist in this index generation.
    UnknownHistorySpace,
    /// The read predates the selected HistorySpace's pinned base.
    ReadBeforeBase {
        requested: Revision,
        base_revision: Revision,
    },
    /// The requested revision is newer than this immutable index generation.
    QueryBeyondIndexedRevision {
        requested: Revision,
        indexed_through: Revision,
    },
    /// Authorized lifecycle rows exceeded the query work-unit allowance.
    LifecycleBudgetExceeded,
    /// A bounded lifecycle result could not reserve its next element.
    AllocationFailed,
    /// The shared process index-memory admission budget was exhausted.
    ResourceBudgetExceeded,
    /// The reference model rejected a source read.
    History(HistorySpaceModelError),
}

impl From<HistorySpaceModelError> for AssertionPointIndexError {
    fn from(error: HistorySpaceModelError) -> Self {
        Self::History(error)
    }
}

impl fmt::Display for AssertionPointIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAssertionId(id) => write!(formatter, "duplicate AssertionId {id}"),
            Self::IndexHitMismatch => {
                formatter.write_str("Assertion point index returned a hit outside its query")
            }
            Self::StoredRevisionMismatch {
                owner_history_space_id,
                stored_revision,
                record_revision,
            } => write!(
                formatter,
                "Assertion in HistorySpace {owner_history_space_id} is stored at revision {stored_revision}, but records revision {record_revision}"
            ),
            Self::ContextHistorySpaceMismatch {
                assertion_id,
                owner_history_space_id,
                context_history_space_id,
            } => write!(
                formatter,
                "Assertion {assertion_id} is owned by HistorySpace {owner_history_space_id}, but its context names {context_history_space_id}"
            ),
            Self::UnknownHistorySpace => formatter.write_str("unknown HistorySpaceId"),
            Self::ReadBeforeBase {
                requested,
                base_revision,
            } => write!(
                formatter,
                "read revision {requested} predates HistorySpace base {base_revision}"
            ),
            Self::QueryBeyondIndexedRevision {
                requested,
                indexed_through,
            } => write!(
                formatter,
                "read revision {requested} is newer than index coverage {indexed_through}"
            ),
            Self::LifecycleBudgetExceeded => {
                formatter.write_str("Assertion lifecycle work-unit budget exceeded")
            }
            Self::AllocationFailed => {
                formatter.write_str("Assertion lifecycle result could not reserve memory")
            }
            Self::ResourceBudgetExceeded => {
                formatter.write_str("Assertion index exceeded the process index-memory budget")
            }
            Self::History(error) => write!(formatter, "invalid index source history: {error}"),
        }
    }
}

impl std::error::Error for AssertionPointIndexError {}

#[cfg(test)]
mod tests {
    use super::{
        AssertionEntityHistoryQuery, AssertionPointHistoryIndex, AssertionPointIndexError,
        AssertionPointQuery, AssertionPredicateHistoryQuery,
    };
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::candidate_scan::AssertionHistoryRecord;
    use crate::catalog::HistorySpaceDefinition;
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::history_model::HistorySpaceReferenceModel;
    use crate::ids::{
        AssertionId, DomainId, EntityId, HistorySpaceId, IdValidationError, LayerId, PerspectiveId,
        PredicateId, Revision, RevisionError, TimelineId,
    };
    use crate::temporal::{AssertionValidity, TemporalError, TimeInterval, Timeline, WorldTime};
    use crate::values::Value;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        Catalog(crate::catalog::HistorySpaceError),
        Context(crate::context::ContextError),
        History(crate::history_model::HistorySpaceModelError),
        Temporal(TemporalError),
        ResourceBudget(crate::ResourceBudgetError),
        Index(AssertionPointIndexError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "invalid test ID: {error}"),
                Self::Revision(error) => write!(formatter, "invalid test revision: {error}"),
                Self::Catalog(error) => write!(formatter, "invalid test catalog: {error}"),
                Self::Context(error) => write!(formatter, "invalid test context: {error}"),
                Self::History(error) => write!(formatter, "invalid test history: {error}"),
                Self::Temporal(error) => write!(formatter, "invalid test time: {error}"),
                Self::ResourceBudget(error) => write!(formatter, "invalid test budget: {error}"),
                Self::Index(error) => write!(formatter, "invalid test index: {error}"),
            }
        }
    }

    impl std::error::Error for TestError {}

    macro_rules! test_error_from {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    test_error_from!(IdValidationError, Id);
    test_error_from!(RevisionError, Revision);
    test_error_from!(crate::catalog::HistorySpaceError, Catalog);
    test_error_from!(crate::context::ContextError, Context);
    test_error_from!(crate::history_model::HistorySpaceModelError, History);
    test_error_from!(TemporalError, Temporal);
    test_error_from!(crate::ResourceBudgetError, ResourceBudget);
    test_error_from!(AssertionPointIndexError, Index);

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn assertion(
        id_tail: u8,
        context: ContextKey,
        subject: EntityId,
        predicate: PredicateId,
        created_revision: Revision,
    ) -> Result<Assertion, TestError> {
        let timeline = Timeline::new(id::<TimelineId>(250)?);
        let validity = AssertionValidity::new(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 0)),
            None,
        )?);
        Ok(Assertion::new(
            id::<AssertionId>(id_tail)?,
            AssertionDraft::new(
                context,
                Subject::new(subject),
                predicate,
                Value::String(format!("value-{id_tail}")),
                Polarity::Positive,
                validity,
            ),
            created_revision,
        ))
    }

    fn ids(hits: &[super::AssertionIndexHit<'_>]) -> Vec<AssertionId> {
        hits.iter().map(|hit| hit.assertion().id()).collect()
    }

    fn oracle_ids(
        history: &HistorySpaceReferenceModel<AssertionHistoryRecord>,
        query_context: ContextKey,
        as_of: Revision,
        subject: Option<Subject>,
        predicate: Option<PredicateId>,
    ) -> Result<Vec<AssertionId>, TestError> {
        let mut result = history
            .read_at(query_context.history_space_id(), as_of)?
            .into_iter()
            .filter_map(|(_, _, record)| match record {
                AssertionHistoryRecord::Assertion(assertion)
                    if assertion.context().layer_id() == query_context.layer_id()
                        && assertion.context().perspective_scope()
                            == query_context.perspective_scope()
                        && assertion.context().epistemic_mode()
                            == query_context.epistemic_mode()
                        && subject.is_none_or(|subject| assertion.subject() == subject)
                        && predicate
                            .is_none_or(|predicate| assertion.predicate_id() == predicate) =>
                {
                    Some(assertion.clone())
                }
                _ => None,
            })
            .map(|assertion| (assertion.created_revision(), assertion.id()))
            .collect::<Vec<_>>();
        result.sort_unstable();
        Ok(result.into_iter().map(|(_, id)| id).collect())
    }

    #[test]
    fn point_and_both_history_indexes_match_full_scan_across_pinned_branches() -> TestResult {
        let root = id::<HistorySpaceId>(1)?;
        let child = id::<HistorySpaceId>(2)?;
        let sibling = id::<HistorySpaceId>(3)?;
        let grandchild = id::<HistorySpaceId>(4)?;
        let layer = id::<LayerId>(5)?;
        let other_layer = id::<LayerId>(6)?;
        let subject = id::<EntityId>(7)?;
        let other_subject = id::<EntityId>(8)?;
        let predicate = id::<PredicateId>(9)?;
        let other_predicate = id::<PredicateId>(10)?;
        let perspective = PerspectiveScope::Perspective(id::<PerspectiveId>(11)?);
        let root_definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut history = HistorySpaceReferenceModel::new(vec![root_definition])?;

        let root_one_assertion = assertion(
            12,
            ContextKey::new(
                root,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            predicate,
            revision(1)?,
        )?;
        history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(
                root_one_assertion.clone(),
            )],
        )?;
        history.add_history_space(HistorySpaceDefinition::new(
            child,
            Some(root),
            revision(1)?,
        )?)?;
        history.add_history_space(HistorySpaceDefinition::new(
            sibling,
            Some(root),
            revision(1)?,
        )?)?;

        let root_two_assertion = assertion(
            13,
            ContextKey::new(
                root,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            predicate,
            revision(2)?,
        )?;
        history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(
                root_two_assertion.clone(),
            )],
        )?;
        let child_three_assertion = assertion(
            14,
            ContextKey::new(
                child,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            other_predicate,
            revision(3)?,
        )?;
        history.publish(
            child,
            vec![AssertionHistoryRecord::from_assertion(
                child_three_assertion.clone(),
            )],
        )?;
        history.add_history_space(HistorySpaceDefinition::new(
            grandchild,
            Some(child),
            revision(3)?,
        )?)?;
        let grandchild_four_assertion = assertion(
            15,
            ContextKey::new(
                grandchild,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            other_subject,
            predicate,
            revision(4)?,
        )?;
        history.publish(
            grandchild,
            vec![AssertionHistoryRecord::from_assertion(
                grandchild_four_assertion.clone(),
            )],
        )?;
        let root_five_assertion = assertion(
            16,
            ContextKey::new(
                root,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            predicate,
            revision(5)?,
        )?;
        history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(
                root_five_assertion.clone(),
            )],
        )?;
        let sibling_six_assertion = assertion(
            17,
            ContextKey::new(
                sibling,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            predicate,
            revision(6)?,
        )?;
        let sibling_noise = assertion(
            18,
            ContextKey::new(sibling, other_layer, perspective, EpistemicMode::Knows)?,
            subject,
            predicate,
            revision(6)?,
        )?;
        history.publish(
            sibling,
            vec![
                AssertionHistoryRecord::from_assertion(sibling_six_assertion.clone()),
                AssertionHistoryRecord::from_assertion(sibling_noise),
            ],
        )?;

        let tiny_profile = crate::ProcessResourceProfile::new(
            1,
            1,
            1,
            crate::ResourceClassLimits::new(1, 1, 1, 1, 1),
        )?;
        let tiny_memory = tiny_profile.memory_budget();
        assert_eq!(
            AssertionPointHistoryIndex::build_with_memory_budget(&history, tiny_memory.clone())
                .err(),
            Some(AssertionPointIndexError::ResourceBudgetExceeded)
        );
        assert_eq!(tiny_memory.reserved_bytes(), Ok(0));

        let index = AssertionPointHistoryIndex::build(&history)?;
        assert_eq!(index.indexed_through(), revision(6)?);

        let source_assertion = history
            .read_at(root, revision(6)?)?
            .into_iter()
            .find_map(|(_, owner, record)| match record {
                AssertionHistoryRecord::Assertion(assertion)
                    if owner == root && assertion.id() == root_one_assertion.id() =>
                {
                    Some(assertion.as_ref())
                }
                _ => None,
            })
            .ok_or(AssertionPointIndexError::UnknownHistorySpace)?;
        let root_context = ContextKey::new(
            root,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let indexed_assertion = index
            .point(AssertionPointQuery::new(
                root_context,
                Subject::new(subject),
                predicate,
                revision(6)?,
            ))?
            .into_iter()
            .find(|hit| hit.assertion().id() == root_one_assertion.id())
            .ok_or(AssertionPointIndexError::IndexHitMismatch)?
            .assertion();
        assert!(std::ptr::eq(source_assertion, indexed_assertion));

        for (space, as_of) in [
            (root, revision(6)?),
            (child, revision(6)?),
            (sibling, revision(6)?),
            (grandchild, revision(6)?),
        ] {
            let context = ContextKey::new(
                space,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?;
            let point_query =
                AssertionPointQuery::new(context, Subject::new(subject), predicate, as_of);
            let entity_query =
                AssertionEntityHistoryQuery::new(context, Subject::new(subject), as_of);
            let predicate_query = AssertionPredicateHistoryQuery::new(context, predicate, as_of);
            assert_eq!(
                ids(&index.point(point_query)?),
                oracle_ids(
                    &history,
                    context,
                    as_of,
                    Some(Subject::new(subject)),
                    Some(predicate)
                )?
            );
            assert_eq!(
                ids(&index.entity_history(entity_query)?),
                oracle_ids(&history, context, as_of, Some(Subject::new(subject)), None)?
            );
            assert_eq!(
                ids(&index.predicate_history(predicate_query)?),
                oracle_ids(&history, context, as_of, None, Some(predicate))?
            );
        }

        let child_context = ContextKey::new(
            child,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        assert_eq!(
            ids(&index.point(AssertionPointQuery::new(
                child_context,
                Subject::new(subject),
                predicate,
                revision(6)?,
            ))?),
            vec![root_one_assertion.id()]
        );
        assert!(
            !ids(&index.entity_history(AssertionEntityHistoryQuery::new(
                child_context,
                Subject::new(subject),
                revision(6)?,
            ))?)
            .contains(&root_two_assertion.id())
        );
        assert!(
            !ids(
                &index.predicate_history(AssertionPredicateHistoryQuery::new(
                    child_context,
                    predicate,
                    revision(6)?,
                ))?
            )
            .contains(&root_five_assertion.id())
        );
        assert!(
            ids(&index.entity_history(AssertionEntityHistoryQuery::new(
                child_context,
                Subject::new(subject),
                revision(6)?,
            ))?)
            .contains(&child_three_assertion.id())
        );
        assert!(
            ids(
                &index.predicate_history(AssertionPredicateHistoryQuery::new(
                    ContextKey::new(
                        grandchild,
                        layer,
                        PerspectiveScope::World,
                        EpistemicMode::WorldState
                    )?,
                    predicate,
                    revision(6)?,
                ))?
            )
            .contains(&grandchild_four_assertion.id())
        );
        assert!(
            ids(&index.point(AssertionPointQuery::new(
                ContextKey::new(
                    sibling,
                    layer,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState
                )?,
                Subject::new(subject),
                predicate,
                revision(6)?,
            ))?)
            .contains(&sibling_six_assertion.id())
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_and_inconsistent_source_assertions() -> TestResult {
        let root = id::<HistorySpaceId>(31)?;
        let layer = id::<LayerId>(32)?;
        let subject = id::<EntityId>(33)?;
        let predicate = id::<PredicateId>(34)?;
        let definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut duplicate_history = HistorySpaceReferenceModel::new(vec![definition])?;
        let duplicate = assertion(
            35,
            ContextKey::new(
                root,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            predicate,
            revision(1)?,
        )?;
        duplicate_history.publish(
            root,
            vec![
                AssertionHistoryRecord::from_assertion(duplicate.clone()),
                AssertionHistoryRecord::from_assertion(duplicate),
            ],
        )?;
        assert_eq!(
            AssertionPointHistoryIndex::build(&duplicate_history).err(),
            Some(AssertionPointIndexError::DuplicateAssertionId(id::<
                AssertionId,
            >(
                35
            )?))
        );

        let child = id::<HistorySpaceId>(36)?;
        let mut mismatched_history = HistorySpaceReferenceModel::new(vec![definition])?;
        mismatched_history.add_history_space(HistorySpaceDefinition::new(
            child,
            Some(root),
            Revision::GENESIS,
        )?)?;
        let wrong_owner = assertion(
            37,
            ContextKey::new(
                root,
                layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            subject,
            predicate,
            revision(1)?,
        )?;
        mismatched_history.publish(
            child,
            vec![AssertionHistoryRecord::from_assertion(wrong_owner.clone())],
        )?;
        assert_eq!(
            AssertionPointHistoryIndex::build(&mismatched_history).err(),
            Some(AssertionPointIndexError::ContextHistorySpaceMismatch {
                assertion_id: wrong_owner.id(),
                owner_history_space_id: child,
                context_history_space_id: root,
            })
        );
        Ok(())
    }

    #[test]
    fn query_errors_fail_closed_at_unknown_future_and_pre_base_revisions() -> TestResult {
        let root = id::<HistorySpaceId>(41)?;
        let child = id::<HistorySpaceId>(42)?;
        let unknown = id::<HistorySpaceId>(43)?;
        let layer = id::<LayerId>(44)?;
        let subject = id::<EntityId>(45)?;
        let predicate = id::<PredicateId>(46)?;
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?])?;
        history.publish(root, vec![])?;
        history.add_history_space(HistorySpaceDefinition::new(
            child,
            Some(root),
            revision(1)?,
        )?)?;
        let index = AssertionPointHistoryIndex::build(&history)?;

        let unknown_context = ContextKey::new(
            unknown,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        assert_eq!(
            index
                .point(AssertionPointQuery::new(
                    unknown_context,
                    Subject::new(subject),
                    predicate,
                    revision(1)?,
                ))
                .err(),
            Some(AssertionPointIndexError::UnknownHistorySpace)
        );
        let child_context = ContextKey::new(
            child,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        assert_eq!(
            index
                .point(AssertionPointQuery::new(
                    child_context,
                    Subject::new(subject),
                    predicate,
                    Revision::GENESIS,
                ))
                .err(),
            Some(AssertionPointIndexError::ReadBeforeBase {
                requested: Revision::GENESIS,
                base_revision: revision(1)?,
            })
        );
        assert_eq!(
            index
                .point(AssertionPointQuery::new(
                    child_context,
                    Subject::new(subject),
                    predicate,
                    revision(2)?,
                ))
                .err(),
            Some(AssertionPointIndexError::QueryBeyondIndexedRevision {
                requested: revision(2)?,
                indexed_through: revision(1)?,
            })
        );
        Ok(())
    }
}
