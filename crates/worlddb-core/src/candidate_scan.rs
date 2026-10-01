//! Index-free assertion candidate oracle for slow reference-model queries.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::archive::ArchiveTargetRef;
use crate::archive_projection::{ArchiveHistoryReferenceModel, ArchiveProjectionError};
use crate::assertion_projection::{AssertionLifecycleProjection, AssertionProjectionError};
use crate::assertions::{Assertion, AssertionRetraction, AssertionValidityClosure};
use crate::context::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
use crate::history_model::{HistorySpaceModelError, HistorySpaceReferenceModel};
use crate::ids::{AssertionId, HistorySpaceId, PrincipalId, Revision};
use crate::layers::{LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
use crate::query_context::{QueryContext, WorldTimeSelector};
use crate::record_refs::RecordRef;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicySnapshot,
};
use crate::temporal::{RecordedAsOf, WorldTime};

/// Assertion-family history entry stored in the HistorySpace reference model.
#[derive(Clone, Debug)]
pub enum AssertionHistoryRecord {
    /// An immutable assertion payload.
    Assertion(Box<Assertion>),
    /// A world-time validity closure for a prior assertion.
    ValidityClosure(AssertionValidityClosure),
    /// A transaction-time retraction for a prior assertion.
    Retraction(AssertionRetraction),
}

impl AssertionHistoryRecord {
    /// Wraps an assertion payload for storage in a HistorySpace record batch.
    #[must_use]
    pub fn from_assertion(value: Assertion) -> Self {
        Self::Assertion(Box::new(value))
    }

    fn created_revision(&self) -> Revision {
        match self {
            Self::Assertion(value) => value.created_revision(),
            Self::ValidityClosure(value) => value.created_revision(),
            Self::Retraction(value) => value.created_revision(),
        }
    }
}

/// Query coordinates for one index-free assertion candidate scan.
#[derive(Clone, Debug)]
pub struct AssertionCandidateQuery {
    history_space_id: HistorySpaceId,
    layer_selection: LayerSelection,
    perspective_scope: PerspectiveScope,
    epistemic_mode: EpistemicMode,
    recorded_as_of: RecordedAsOf,
    world_time: WorldTime,
}

impl AssertionCandidateQuery {
    /// Binds the HistorySpace, selected layers, epistemic partition, and both time axes.
    #[must_use]
    pub const fn new(
        history_space_id: HistorySpaceId,
        layer_selection: LayerSelection,
        perspective_scope: PerspectiveScope,
        epistemic_mode: EpistemicMode,
        recorded_as_of: RecordedAsOf,
        world_time: WorldTime,
    ) -> Self {
        Self {
            history_space_id,
            layer_selection,
            perspective_scope,
            epistemic_mode,
            recorded_as_of,
            world_time,
        }
    }
}

/// One matching Assertion and its HistorySpace-before-Layer precedence coordinates.
#[derive(Clone, Debug)]
pub struct AssertionCandidate {
    pub(crate) assertion: Assertion,
    pub(crate) source_history_space_id: HistorySpaceId,
    pub(crate) query_history_space_id: HistorySpaceId,
    pub(crate) selected_layer_ids: BTreeSet<crate::ids::LayerId>,
    pub(crate) precedence: ContextPrecedence,
}

impl AssertionCandidate {
    /// Returns the immutable candidate record.
    #[must_use]
    pub const fn assertion(&self) -> &Assertion {
        &self.assertion
    }

    /// Returns the HistorySpace that committed this candidate.
    #[must_use]
    pub const fn source_history_space_id(&self) -> HistorySpaceId {
        self.source_history_space_id
    }

    /// Returns its HistorySpace distance and pinned LayerRank priority.
    #[must_use]
    pub const fn precedence(&self) -> ContextPrecedence {
        self.precedence
    }

    /// Returns whether the query's pinned layer selection included this layer.
    #[must_use]
    pub fn selected_layer_ids(&self) -> &BTreeSet<crate::ids::LayerId> {
        &self.selected_layer_ids
    }
}

/// Collects context-, archive-, and time-visible assertions by scanning retained history.
///
/// This is the slow reference oracle. It has no indexes, performs no masking or
/// resolution, and intentionally returns candidates in stable AssertionId order
/// so insertion order cannot affect the result set or representation.
pub fn full_scan_assertion_candidates(
    history: &HistorySpaceReferenceModel<AssertionHistoryRecord>,
    archive: &ArchiveHistoryReferenceModel,
    query: &AssertionCandidateQuery,
    layers: &LayerSchemaSnapshot,
) -> Result<Vec<AssertionCandidate>, CandidateScanError> {
    full_scan_assertion_candidates_with_security(history, archive, query, layers, None)
}

/// Full assertion scan with policy filtering before candidate construction.
/// The record, owner HistorySpace, Layer, and every field used by candidate
/// matching/resolution must be visible to the authenticated Principal.
pub fn full_scan_authorized_assertion_candidates(
    history: &HistorySpaceReferenceModel<AssertionHistoryRecord>,
    archive: &ArchiveHistoryReferenceModel,
    query: &AssertionCandidateQuery,
    layers: &LayerSchemaSnapshot,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<Vec<AssertionCandidate>, CandidateScanError> {
    if context.history_space() != query.history_space_id
        || context.recorded_as_of() != query.recorded_as_of
        || context.layers().requested() != &query.layer_selection
        || context.layers().schema_revision() != layers.revision()
        || context.perspective() != query.perspective_scope
        || context.epistemic_mode() != query.epistemic_mode
        || context.world_time() != WorldTimeSelector::At(query.world_time)
    {
        return Err(CandidateScanError::QueryContextMismatch);
    }
    if policy.authorize(
        context.security().principal_id(),
        Capability::QueryResolve,
        PolicyTarget::default(),
    ) != AuthorizationDecision::Allow
    {
        return Ok(Vec::new());
    }
    full_scan_assertion_candidates_with_security(
        history,
        archive,
        query,
        layers,
        Some((policy, context.security().principal_id())),
    )
}

fn full_scan_assertion_candidates_with_security(
    history: &HistorySpaceReferenceModel<AssertionHistoryRecord>,
    archive: &ArchiveHistoryReferenceModel,
    query: &AssertionCandidateQuery,
    layers: &LayerSchemaSnapshot,
    security: Option<(&SecurityPolicySnapshot, PrincipalId)>,
) -> Result<Vec<AssertionCandidate>, CandidateScanError> {
    let selected_layers = layers.resolve(&query.layer_selection)?;
    let query_layer = *selected_layers
        .as_slice()
        .first()
        .ok_or(CandidateScanError::EmptyLayerSelection)?;
    let _ = ContextKey::new(
        query.history_space_id,
        query_layer,
        query.perspective_scope,
        query.epistemic_mode,
    )?;
    let selected_layers = selected_layers
        .as_slice()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    let mut assertions = Vec::new();
    let mut closures = Vec::new();
    let mut retractions = Vec::new();
    let mut source_spaces = BTreeMap::new();
    for (stored_revision, owner_history_space_id, record) in
        history.read_at(query.history_space_id, query.recorded_as_of.revision())?
    {
        match record {
            AssertionHistoryRecord::Assertion(assertion) => {
                if let Some((policy, principal_id)) = security {
                    if !assertion_candidate_is_authorized(
                        policy,
                        principal_id,
                        owner_history_space_id,
                        assertion,
                    ) {
                        continue;
                    }
                }
                let record_revision = record.created_revision();
                if stored_revision != record_revision {
                    return Err(CandidateScanError::RecordRevisionMismatch {
                        owner_history_space_id,
                        stored_revision,
                        record_revision,
                    });
                }
                if assertion.context().history_space_id() != owner_history_space_id {
                    return Err(CandidateScanError::ContextHistorySpaceMismatch {
                        assertion_id: assertion.id(),
                        owner_history_space_id,
                        context_history_space_id: assertion.context().history_space_id(),
                    });
                }
                if assertion.context().perspective_scope() == query.perspective_scope
                    && assertion.context().epistemic_mode() == query.epistemic_mode
                    && selected_layers.contains(&assertion.context().layer_id())
                {
                    source_spaces.insert(assertion.id(), owner_history_space_id);
                    assertions.push((**assertion).clone());
                }
            }
            AssertionHistoryRecord::ValidityClosure(closure) => {
                if !source_spaces.contains_key(&closure.assertion_id()) {
                    continue;
                }
                validate_stored_record_revision(
                    owner_history_space_id,
                    stored_revision,
                    closure.created_revision(),
                )?;
                closures.push(*closure);
            }
            AssertionHistoryRecord::Retraction(retraction) => {
                if !source_spaces.contains_key(&retraction.assertion_id()) {
                    continue;
                }
                validate_stored_record_revision(
                    owner_history_space_id,
                    stored_revision,
                    retraction.created_revision(),
                )?;
                retractions.push(retraction.clone());
            }
        }
    }

    let lifecycle = AssertionLifecycleProjection::new(assertions, closures, retractions)?;
    let ordinary_targets = archive
        .ordinary_targets_at(query.recorded_as_of)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    for assertion in lifecycle.assertions() {
        let target = ArchiveTargetRef::Assertion(assertion.id());
        let archive_record = archive
            .targets()
            .iter()
            .find(|record| record.target() == target)
            .ok_or(CandidateScanError::MissingArchiveInventoryTarget {
                assertion_id: assertion.id(),
            })?;
        if archive_record.created_revision() != assertion.created_revision() {
            return Err(CandidateScanError::ArchiveTargetRevisionMismatch {
                assertion_id: assertion.id(),
                assertion_revision: assertion.created_revision(),
                archive_target_revision: archive_record.created_revision(),
            });
        }
    }

    let mut candidates = Vec::new();
    for assertion in lifecycle.candidates(query.recorded_as_of, query.world_time)? {
        let context = assertion.context();
        if context.perspective_scope() != query.perspective_scope
            || context.epistemic_mode() != query.epistemic_mode
            || !selected_layers.contains(&context.layer_id())
            || !ordinary_targets.contains(&ArchiveTargetRef::Assertion(assertion.id()))
        {
            continue;
        }
        let source_history_space_id = source_spaces.get(&assertion.id()).copied().ok_or(
            CandidateScanError::MissingSourceHistorySpace {
                assertion_id: assertion.id(),
            },
        )?;
        let precedence = ContextPrecedence::for_context(
            query.history_space_id,
            source_history_space_id,
            context.layer_id(),
            history.catalog(),
            layers,
        )?;
        candidates.push(AssertionCandidate {
            assertion: assertion.clone(),
            source_history_space_id,
            query_history_space_id: query.history_space_id,
            selected_layer_ids: selected_layers.clone(),
            precedence,
        });
    }
    candidates.sort_by_key(|candidate| candidate.assertion.id());
    Ok(candidates)
}

fn validate_stored_record_revision(
    owner_history_space_id: HistorySpaceId,
    stored_revision: Revision,
    record_revision: Revision,
) -> Result<(), CandidateScanError> {
    if stored_revision == record_revision {
        Ok(())
    } else {
        Err(CandidateScanError::RecordRevisionMismatch {
            owner_history_space_id,
            stored_revision,
            record_revision,
        })
    }
}

fn assertion_candidate_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal_id: PrincipalId,
    owner_history_space_id: HistorySpaceId,
    assertion: &Assertion,
) -> bool {
    let context = assertion.context();
    let record = Some(RecordRef::Assertion(assertion.id()));
    let base = PolicyTarget::new(
        Some(owner_history_space_id),
        Some(context.layer_id()),
        record,
        None,
        None,
    );
    for capability in [
        Capability::HistorySpaceRead,
        Capability::LayerRead,
        Capability::AssertionRead,
    ] {
        if policy.authorize(principal_id, capability, base) != AuthorizationDecision::Allow {
            return false;
        }
    }
    for field in [
        FieldSelector::AssertionSubject,
        FieldSelector::AssertionPredicate,
        FieldSelector::AssertionValue(assertion.predicate_id()),
        FieldSelector::AssertionPolarity,
        FieldSelector::AssertionValidity,
        FieldSelector::AssertionPerspective,
        FieldSelector::AssertionEpistemicMode,
    ] {
        let target = PolicyTarget::new(
            Some(owner_history_space_id),
            Some(context.layer_id()),
            record,
            Some(field),
            None,
        );
        if policy.authorize(principal_id, Capability::FieldRead, target)
            != AuthorizationDecision::Allow
        {
            return false;
        }
    }
    true
}

/// Errors raised while collecting assertion candidates from retained history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateScanError {
    /// The selected layer set unexpectedly resolved to empty.
    EmptyLayerSelection,
    /// Query arguments disagree with one or more validated QueryContext pins.
    QueryContextMismatch,
    /// The HistorySpace record owner and assertion context disagree.
    ContextHistorySpaceMismatch {
        assertion_id: AssertionId,
        owner_history_space_id: HistorySpaceId,
        context_history_space_id: HistorySpaceId,
    },
    /// A stored record's embedded creation revision disagrees with its commit revision.
    RecordRevisionMismatch {
        owner_history_space_id: HistorySpaceId,
        stored_revision: Revision,
        record_revision: Revision,
    },
    /// A visible assertion has no entry in the complete archive-target inventory.
    MissingArchiveInventoryTarget {
        assertion_id: AssertionId,
    },
    /// The archive inventory and assertion history disagree on target creation revision.
    ArchiveTargetRevisionMismatch {
        assertion_id: AssertionId,
        assertion_revision: Revision,
        archive_target_revision: Revision,
    },
    /// A selected visible assertion lost its source-space provenance during the scan.
    MissingSourceHistorySpace {
        assertion_id: AssertionId,
    },
    /// Invalid assertion context partition or candidate history/lifecycle data.
    Context(ContextError),
    History(HistorySpaceModelError),
    LayerSchema(LayerSchemaError),
    Lifecycle(AssertionProjectionError),
    Archive(ArchiveProjectionError),
    Precedence(ContextPrecedenceError),
}

impl From<ContextError> for CandidateScanError {
    fn from(error: ContextError) -> Self {
        Self::Context(error)
    }
}
impl From<HistorySpaceModelError> for CandidateScanError {
    fn from(error: HistorySpaceModelError) -> Self {
        Self::History(error)
    }
}
impl From<LayerSchemaError> for CandidateScanError {
    fn from(error: LayerSchemaError) -> Self {
        Self::LayerSchema(error)
    }
}
impl From<AssertionProjectionError> for CandidateScanError {
    fn from(error: AssertionProjectionError) -> Self {
        Self::Lifecycle(error)
    }
}
impl From<ArchiveProjectionError> for CandidateScanError {
    fn from(error: ArchiveProjectionError) -> Self {
        Self::Archive(error)
    }
}
impl From<ContextPrecedenceError> for CandidateScanError {
    fn from(error: ContextPrecedenceError) -> Self {
        Self::Precedence(error)
    }
}

impl fmt::Display for CandidateScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyLayerSelection => formatter.write_str("candidate query selected no layers"),
            Self::QueryContextMismatch => {
                formatter.write_str("candidate request differs from its query context")
            }
            Self::ContextHistorySpaceMismatch {
                assertion_id,
                owner_history_space_id,
                context_history_space_id,
            } => write!(
                formatter,
                "assertion {assertion_id} is stored in {owner_history_space_id} but its context names {context_history_space_id}"
            ),
            Self::RecordRevisionMismatch {
                owner_history_space_id,
                stored_revision,
                record_revision,
            } => write!(
                formatter,
                "record in {owner_history_space_id} has revision {record_revision}, but was stored at {stored_revision}"
            ),
            Self::MissingArchiveInventoryTarget { assertion_id } => write!(
                formatter,
                "assertion {assertion_id} is missing from the complete archive-target inventory"
            ),
            Self::ArchiveTargetRevisionMismatch {
                assertion_id,
                assertion_revision,
                archive_target_revision,
            } => write!(
                formatter,
                "assertion {assertion_id} was created at {assertion_revision}, archive inventory says {archive_target_revision}"
            ),
            Self::MissingSourceHistorySpace { assertion_id } => write!(
                formatter,
                "visible assertion {assertion_id} lost its source HistorySpace during full scan"
            ),
            Self::Context(error) => write!(formatter, "invalid candidate context: {error}"),
            Self::History(error) => write!(formatter, "candidate history read failed: {error}"),
            Self::LayerSchema(error) => {
                write!(formatter, "candidate layer selection failed: {error}")
            }
            Self::Lifecycle(error) => {
                write!(formatter, "candidate lifecycle projection failed: {error}")
            }
            Self::Archive(error) => {
                write!(formatter, "candidate archive projection failed: {error}")
            }
            Self::Precedence(error) => write!(formatter, "candidate precedence failed: {error}"),
        }
    }
}

impl std::error::Error for CandidateScanError {}

#[cfg(test)]
mod tests {
    use super::{
        AssertionCandidateQuery, AssertionHistoryRecord, CandidateScanError,
        full_scan_assertion_candidates, full_scan_authorized_assertion_candidates,
    };
    use crate::archive::{ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition};
    use crate::archive_projection::{ArchiveHistoryReferenceModel, ArchiveTargetRecord};
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::catalog::{HistorySpaceDefinition, HistorySpaceError};
    use crate::context::{ContextError, EpistemicMode, PerspectiveScope};
    use crate::history_model::{HistorySpaceModelError, HistorySpaceReferenceModel};
    use crate::ids::{
        ArchiveTransitionId, AssertionId, DomainId, EntityId, HistorySpaceId, IdValidationError,
        LayerId, PredicateId, Revision, RevisionError, SchemaRevision, TimelineId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
    use crate::schema::Lifecycle;
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, FieldSelector, GrantEffect, PolicyBundle,
        PolicyScope, PolicySubject, Principal, RoleAssignment, RoleDefinition, SecurityPolicyError,
        SecurityPolicySnapshot,
    };
    use crate::temporal::{
        AssertionValidity, RecordedAsOf, TemporalError, TimeInterval, Timeline, WorldTime,
    };
    use crate::values::{Symbol, SymbolError, Value};
    use std::error::Error;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Archive(crate::ArchiveProjectionError),
        ArchiveTransition(crate::ArchiveTransitionError),
        Assertion(crate::AssertionRecordError),
        Candidate(CandidateScanError),
        Context(ContextError),
        History(HistorySpaceModelError),
        HistorySpace(HistorySpaceError),
        Id(IdValidationError),
        Layer(LayerSchemaError),
        Revision(RevisionError),
        Temporal(TemporalError),
        Symbol(SymbolError),
        Security(SecurityPolicyError),
        Role(crate::RoleDefinitionError),
        Bundle(crate::PolicyBundleError),
        SchemaHistory(crate::SchemaHistoryError),
        QueryBudget(crate::QueryBudgetError),
        QueryContext(crate::QueryContextError),
        UnexpectedCandidateCount(usize),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Archive(error) => write!(formatter, "{error}"),
                Self::ArchiveTransition(error) => write!(formatter, "{error}"),
                Self::Assertion(error) => write!(formatter, "{error}"),
                Self::Candidate(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
                Self::History(error) => write!(formatter, "{error}"),
                Self::HistorySpace(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Layer(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Temporal(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
                Self::Security(error) => write!(formatter, "{error}"),
                Self::Role(error) => write!(formatter, "{error}"),
                Self::Bundle(error) => write!(formatter, "{error}"),
                Self::SchemaHistory(error) => write!(formatter, "{error}"),
                Self::QueryBudget(error) => write!(formatter, "{error}"),
                Self::QueryContext(error) => write!(formatter, "{error}"),
                Self::UnexpectedCandidateCount(count) => {
                    write!(formatter, "expected two candidates, found {count}")
                }
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
    error_conversion!(crate::ArchiveProjectionError, Archive);
    error_conversion!(crate::ArchiveTransitionError, ArchiveTransition);
    error_conversion!(crate::AssertionRecordError, Assertion);
    error_conversion!(CandidateScanError, Candidate);
    error_conversion!(ContextError, Context);
    error_conversion!(HistorySpaceModelError, History);
    error_conversion!(HistorySpaceError, HistorySpace);
    error_conversion!(IdValidationError, Id);
    error_conversion!(LayerSchemaError, Layer);
    error_conversion!(RevisionError, Revision);
    error_conversion!(TemporalError, Temporal);
    error_conversion!(SymbolError, Symbol);
    error_conversion!(SecurityPolicyError, Security);
    error_conversion!(crate::RoleDefinitionError, Role);
    error_conversion!(crate::PolicyBundleError, Bundle);
    error_conversion!(crate::SchemaHistoryError, SchemaHistory);
    error_conversion!(crate::QueryBudgetError, QueryBudget);
    error_conversion!(crate::QueryContextError, QueryContext);

    fn id<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }
    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }
    fn as_of(value: u64) -> Result<RecordedAsOf, RevisionError> {
        Ok(RecordedAsOf::from_published_revision(revision(value)?))
    }

    fn layers() -> Result<(LayerSchemaSnapshot, LayerId, LayerId), TestError> {
        let base = id::<LayerId>(20)?;
        let overlay = id::<LayerId>(21)?;
        let schema_revision = SchemaRevision::from_published_revision(Revision::GENESIS);
        let snapshot = LayerSchemaSnapshot::new(
            schema_revision,
            vec![
                LayerDefinition::new(
                    base,
                    Symbol::new("base")?,
                    None,
                    0,
                    Lifecycle::Active,
                    schema_revision,
                ),
                LayerDefinition::new(
                    overlay,
                    Symbol::new("overlay")?,
                    None,
                    1,
                    Lifecycle::Active,
                    schema_revision,
                ),
            ],
            base,
        )?;
        Ok((snapshot, base, overlay))
    }

    fn assertion(
        tail: u8,
        context_parts: (HistorySpaceId, LayerId, PerspectiveScope, EpistemicMode),
        created_revision: Revision,
        validity_bounds: (i128, i128),
        timeline: Timeline,
    ) -> Result<Assertion, TestError> {
        let (history_space_id, layer_id, scope, mode) = context_parts;
        let (start, end) = validity_bounds;
        let context = crate::ContextKey::new(history_space_id, layer_id, scope, mode)?;
        let validity = AssertionValidity::new(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, start)),
            Some(WorldTime::from_nanoseconds(timeline, end)),
        )?);
        Ok(Assertion::new(
            id::<AssertionId>(tail)?,
            AssertionDraft::new(
                context,
                Subject::new(id::<EntityId>(30)?),
                id::<PredicateId>(31)?,
                Value::String(String::from("candidate")),
                Polarity::Positive,
                validity,
            ),
            created_revision,
        ))
    }

    fn assertion_with_polarity(
        tail: u8,
        context_parts: (HistorySpaceId, LayerId, PerspectiveScope, EpistemicMode),
        created_revision: Revision,
        validity_bounds: (i128, i128),
        timeline: Timeline,
        polarity: Polarity,
    ) -> Result<Assertion, TestError> {
        let original = assertion(
            tail,
            context_parts,
            created_revision,
            validity_bounds,
            timeline,
        )?;
        let (subject, predicate_id, value, _) = original.proposition_components();
        Ok(Assertion::new(
            original.id(),
            AssertionDraft::new(
                original.context(),
                subject,
                predicate_id,
                value.clone(),
                polarity,
                original.validity(),
            ),
            original.created_revision(),
        ))
    }

    fn query(
        history_space_id: HistorySpaceId,
        layer_selection: LayerSelection,
        scope: PerspectiveScope,
        mode: EpistemicMode,
        recorded_as_of: RecordedAsOf,
        timeline: Timeline,
        world_time: i128,
    ) -> AssertionCandidateQuery {
        AssertionCandidateQuery::new(
            history_space_id,
            layer_selection,
            scope,
            mode,
            recorded_as_of,
            WorldTime::from_nanoseconds(timeline, world_time),
        )
    }

    fn archive_for(
        assertions: &[(&Assertion, u64)],
        transition: Option<ArchiveTransition>,
    ) -> Result<ArchiveHistoryReferenceModel, TestError> {
        let targets = assertions
            .iter()
            .map(|(assertion, revision_value)| {
                Ok(ArchiveTargetRecord::new(
                    ArchiveTargetRef::Assertion(assertion.id()),
                    revision(*revision_value)?,
                ))
            })
            .collect::<Result<Vec<_>, TestError>>()?;
        ArchiveHistoryReferenceModel::new(targets, transition.into_iter().collect())
            .map_err(Into::into)
    }

    #[test]
    fn authorization_filters_assertion_before_candidate_construction() -> TestResult {
        let history_space = id::<HistorySpaceId>(50)?;
        let (layer_schema, base, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(51)?);
        let record = assertion(
            52,
            (
                history_space,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            history_space,
            None,
            Revision::GENESIS,
        )?])?;
        history.publish(
            history_space,
            vec![AssertionHistoryRecord::from_assertion(record.clone())],
        )?;
        let archive = archive_for(&[(&record, 1)], None)?;
        let query = query(
            history_space,
            LayerSelection::BaseOnly,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
            as_of(1)?,
            timeline,
            50,
        );
        let principal_id = id::<crate::PrincipalId>(53)?;
        let role_id = id::<crate::RoleId>(54)?;
        let assignment_id = id::<crate::RoleAssignmentId>(55)?;
        let grants = [
            Capability::HistorySpaceRead,
            Capability::LayerRead,
            Capability::AssertionRead,
            Capability::FieldRead,
            Capability::QueryResolve,
        ]
        .into_iter()
        .map(|capability| CapabilityGrant::new(capability, GrantEffect::Allow));
        let role = RoleDefinition::new(role_id, "reader", PolicyBundle::from_grants(grants)?)?;
        let assignment =
            RoleAssignment::new(assignment_id, principal_id, role_id, PolicyScope::project());
        let broad_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            vec![role.clone()],
            vec![assignment],
            Vec::new(),
        )?;
        let schema_revision = SchemaRevision::from_published_revision(revision(1)?);
        let layer_schema =
            LayerSchemaSnapshot::new(schema_revision, layer_schema.definitions().to_vec(), base)?;
        let mut schema_history = crate::SchemaHistoryReferenceModel::new();
        schema_history.publish(
            revision(1)?,
            vec![crate::SchemaDefinition::LayerSnapshot(layer_schema.clone())],
        )?;
        let schema_binding = crate::HistoricalQueryBinding::bind(
            &schema_history,
            as_of(1)?,
            crate::SchemaMode::Historical,
        )?;
        let validated_layers =
            crate::ValidatedLayerSelection::resolve(&layer_schema, LayerSelection::BaseOnly)?;
        let budget_limits = crate::QueryBudgetLimits::new(100, 1_000, 100)?;
        let budget = crate::QueryBudget::new(10, 100, 10, budget_limits)?;
        let context = crate::QueryContext::new(crate::QueryContextInput {
            snapshot: crate::SnapshotRef::new(id::<crate::SnapshotId>(57)?),
            snapshot_revision: revision(1)?,
            recorded_as_of: as_of(1)?,
            history_space,
            layers: validated_layers,
            world_time: crate::WorldTimeSelector::At(WorldTime::from_nanoseconds(timeline, 50)),
            perspective: PerspectiveScope::World,
            epistemic_mode: EpistemicMode::WorldState,
            schema_binding,
            security: crate::SecurityContext::new(principal_id, crate::AuthorizationMode::Now),
            budget,
            cancellation: crate::CancellationToken::new(),
        })?;
        let allowed = full_scan_authorized_assertion_candidates(
            &history,
            &archive,
            &query,
            &layer_schema,
            &broad_policy,
            &context,
        )?;
        assert_eq!(allowed.len(), 1);

        let deny_value_rule = CapabilityRule::new(
            id::<crate::PolicyRuleId>(56)?,
            PolicySubject::Principal(principal_id),
            CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
            PolicyScope::new(
                Some(history_space),
                Some(base),
                Some(crate::RecordRef::Assertion(record.id())),
                Some(FieldSelector::AssertionValue(record.predicate_id())),
                None,
            ),
        );
        let denied_value_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            vec![role],
            vec![assignment],
            vec![deny_value_rule],
        )?;
        let hidden = full_scan_authorized_assertion_candidates(
            &history,
            &archive,
            &query,
            &layer_schema,
            &denied_value_policy,
            &context,
        )?;
        assert!(hidden.is_empty());

        let missing_allow = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )?;
        let absent = full_scan_authorized_assertion_candidates(
            &history,
            &archive,
            &query,
            &layer_schema,
            &missing_allow,
            &context,
        )?;
        assert!(absent.is_empty());

        // A duplicate immutable identity is an error when visible, but two
        // copies of the same hidden record must not reach lifecycle validation.
        let mut hidden_duplicate_history =
            HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
                history_space,
                None,
                Revision::GENESIS,
            )?])?;
        hidden_duplicate_history.publish(
            history_space,
            vec![
                AssertionHistoryRecord::from_assertion(record.clone()),
                AssertionHistoryRecord::from_assertion(record.clone()),
            ],
        )?;
        let query_only_role_id = id::<crate::RoleId>(58)?;
        let query_only_assignment_id = id::<crate::RoleAssignmentId>(59)?;
        let query_only_role = RoleDefinition::new(
            query_only_role_id,
            "query_only",
            PolicyBundle::from_grants([
                CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::LayerRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::QueryResolve, GrantEffect::Allow),
            ])?,
        )?;
        let query_only_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            vec![query_only_role],
            vec![RoleAssignment::new(
                query_only_assignment_id,
                principal_id,
                query_only_role_id,
                PolicyScope::project(),
            )],
            Vec::new(),
        )?;
        let hidden_duplicates = full_scan_authorized_assertion_candidates(
            &hidden_duplicate_history,
            &archive,
            &query,
            &layer_schema,
            &query_only_policy,
            &context,
        )?;
        assert!(hidden_duplicates.is_empty());

        // The same Perspective does not inherit the authenticated Principal's
        // grant: only a Principal explicitly present in policy can resolve.
        let unrelated_principal = id::<crate::PrincipalId>(61)?;
        let unrelated_context = crate::QueryContext::new(crate::QueryContextInput {
            snapshot: context.snapshot(),
            snapshot_revision: context.snapshot_revision(),
            recorded_as_of: context.recorded_as_of(),
            history_space: context.history_space(),
            layers: context.layers().clone(),
            world_time: context.world_time(),
            perspective: context.perspective(),
            epistemic_mode: context.epistemic_mode(),
            schema_binding: crate::HistoricalQueryBinding::bind(
                &schema_history,
                as_of(1)?,
                crate::SchemaMode::Historical,
            )?,
            security: crate::SecurityContext::new(
                unrelated_principal,
                crate::AuthorizationMode::Now,
            ),
            budget: context.budget(),
            cancellation: crate::CancellationToken::new(),
        })?;
        let same_perspective_other_principal = full_scan_authorized_assertion_candidates(
            &history,
            &archive,
            &query,
            &layer_schema,
            &broad_policy,
            &unrelated_context,
        )?;
        assert!(same_perspective_other_principal.is_empty());
        Ok(())
    }

    #[test]
    fn full_scan_respects_ancestry_cutoffs_context_layer_and_world_time() -> TestResult {
        let root = id::<HistorySpaceId>(1)?;
        let child = id::<HistorySpaceId>(2)?;
        let (layer_snapshot, base, overlay) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(32)?);
        let root_definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut history = HistorySpaceReferenceModel::new(vec![root_definition])?;
        let inherited = assertion(
            1,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(inherited.clone())],
        )?;
        history.add_history_space(HistorySpaceDefinition::new(
            child,
            Some(root),
            revision(1)?,
        )?)?;
        let local = assertion(
            2,
            (
                child,
                overlay,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(2)?,
            (0, 100),
            timeline,
        )?;
        let expired = assertion(
            3,
            (
                child,
                overlay,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(2)?,
            (0, 10),
            timeline,
        )?;
        history.publish(
            child,
            vec![
                AssertionHistoryRecord::from_assertion(expired.clone()),
                AssertionHistoryRecord::from_assertion(local.clone()),
            ],
        )?;
        let too_late = assertion(
            4,
            (
                root,
                overlay,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(3)?,
            (0, 100),
            timeline,
        )?;
        history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(too_late.clone())],
        )?;
        let archive = archive_for(
            &[(&inherited, 1), (&local, 2), (&expired, 2), (&too_late, 3)],
            None,
        )?;
        let candidate_query = query(
            child,
            LayerSelection::AllActive,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
            as_of(3)?,
            timeline,
            10,
        );
        let candidates =
            full_scan_assertion_candidates(&history, &archive, &candidate_query, &layer_snapshot)?;
        let ids = candidates
            .iter()
            .map(|candidate| candidate.assertion().id())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![inherited.id(), local.id()]);
        let [inherited_candidate, local_candidate] = candidates.as_slice() else {
            return Err(TestError::UnexpectedCandidateCount(candidates.len()));
        };
        assert_eq!(inherited_candidate.precedence().history_space_distance(), 1);
        assert_eq!(local_candidate.precedence().history_space_distance(), 0);
        assert_eq!(local_candidate.precedence().layer_rank(), 1);
        Ok(())
    }

    #[test]
    fn full_scan_filters_epistemic_partitions_and_archived_records() -> TestResult {
        let root = id::<HistorySpaceId>(5)?;
        let (layer_snapshot, base, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(6)?);
        let unrelated_timeline = Timeline::new(id::<TimelineId>(7)?);
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?])?;
        let world = assertion(
            7,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            unrelated_timeline,
        )?;
        let knows = assertion(
            8,
            (
                root,
                base,
                PerspectiveScope::Perspective(id::<crate::PerspectiveId>(9)?),
                EpistemicMode::Knows,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        history.publish(
            root,
            vec![
                AssertionHistoryRecord::from_assertion(world.clone()),
                AssertionHistoryRecord::from_assertion(knows.clone()),
            ],
        )?;
        let archive_transition = ArchiveTransition::new(
            id::<ArchiveTransitionId>(10)?,
            ArchiveTargetRef::Assertion(knows.id()),
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision(2)?,
        )?;
        history.publish(root, vec![])?;
        let archive = archive_for(&[(&world, 1), (&knows, 1)], Some(archive_transition))?;
        let missing_inventory_query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::Perspective(id::<crate::PerspectiveId>(9)?),
            EpistemicMode::Knows,
            as_of(2)?,
            timeline,
            20,
        );
        let candidates = full_scan_assertion_candidates(
            &history,
            &archive,
            &missing_inventory_query,
            &layer_snapshot,
        )?;
        assert!(candidates.is_empty());
        let unarchived_inventory = archive_for(&[(&world, 1), (&knows, 1)], None)?;
        let matching_partition = full_scan_assertion_candidates(
            &history,
            &unarchived_inventory,
            &missing_inventory_query,
            &layer_snapshot,
        )?;
        assert_eq!(
            matching_partition
                .iter()
                .map(|candidate| candidate.assertion().id())
                .collect::<Vec<_>>(),
            vec![knows.id()]
        );
        let raw = history.read_at(root, revision(2)?)?;
        assert_eq!(raw.len(), 2);
        Ok(())
    }

    #[test]
    fn full_scan_keeps_world_knows_believes_and_claims_independent() -> TestResult {
        let root = id::<HistorySpaceId>(40)?;
        let perspective = crate::PerspectiveId::try_from_bytes({
            let mut bytes = [0_u8; 16];
            bytes[6] = 0x70;
            bytes[8] = 0x80;
            bytes[15] = 41;
            bytes
        })?;
        let other_perspective = crate::PerspectiveId::try_from_bytes({
            let mut bytes = [0_u8; 16];
            bytes[6] = 0x70;
            bytes[8] = 0x80;
            bytes[15] = 42;
            bytes
        })?;
        let (layer_snapshot, base, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(43)?);
        let world = assertion_with_polarity(
            44,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
            Polarity::Positive,
        )?;
        let knows_negative = assertion_with_polarity(
            45,
            (
                root,
                base,
                PerspectiveScope::Perspective(perspective),
                EpistemicMode::Knows,
            ),
            revision(1)?,
            (0, 100),
            timeline,
            Polarity::Negative,
        )?;
        let believes_other_perspective = assertion_with_polarity(
            46,
            (
                root,
                base,
                PerspectiveScope::Perspective(other_perspective),
                EpistemicMode::Believes,
            ),
            revision(1)?,
            (0, 100),
            timeline,
            Polarity::Positive,
        )?;
        let claims = assertion_with_polarity(
            47,
            (
                root,
                base,
                PerspectiveScope::Perspective(perspective),
                EpistemicMode::Claims,
            ),
            revision(1)?,
            (0, 100),
            timeline,
            Polarity::Positive,
        )?;
        let all = [
            &world,
            &knows_negative,
            &believes_other_perspective,
            &claims,
        ];
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?])?;
        history.publish(
            root,
            all.iter()
                .map(|item| AssertionHistoryRecord::from_assertion((*item).clone()))
                .collect(),
        )?;
        let archive = archive_for(&all.iter().map(|item| (*item, 1)).collect::<Vec<_>>(), None)?;

        let world_query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
            as_of(1)?,
            timeline,
            10,
        );
        let world_result =
            full_scan_assertion_candidates(&history, &archive, &world_query, &layer_snapshot)?;
        assert_eq!(
            world_result
                .iter()
                .map(|candidate| candidate.assertion().id())
                .collect::<Vec<_>>(),
            vec![world.id()]
        );

        let knows_query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::Perspective(perspective),
            EpistemicMode::Knows,
            as_of(1)?,
            timeline,
            10,
        );
        let knows_result =
            full_scan_assertion_candidates(&history, &archive, &knows_query, &layer_snapshot)?;
        assert_eq!(
            knows_result
                .iter()
                .map(|candidate| candidate.assertion().id())
                .collect::<Vec<_>>(),
            vec![knows_negative.id()]
        );
        assert_eq!(
            knows_result
                .first()
                .map(|candidate| candidate.assertion().polarity()),
            Some(Polarity::Negative)
        );

        let believes_missing = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::Perspective(perspective),
            EpistemicMode::Believes,
            as_of(1)?,
            timeline,
            10,
        );
        assert!(
            full_scan_assertion_candidates(&history, &archive, &believes_missing, &layer_snapshot)?
                .is_empty()
        );
        let believes_other = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::Perspective(other_perspective),
            EpistemicMode::Believes,
            as_of(1)?,
            timeline,
            10,
        );
        assert_eq!(
            full_scan_assertion_candidates(&history, &archive, &believes_other, &layer_snapshot)?
                .first()
                .map(|candidate| candidate.assertion().id()),
            Some(believes_other_perspective.id())
        );

        let claims_query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::Perspective(perspective),
            EpistemicMode::Claims,
            as_of(1)?,
            timeline,
            10,
        );
        assert_eq!(
            full_scan_assertion_candidates(&history, &archive, &claims_query, &layer_snapshot)?
                .first()
                .map(|candidate| candidate.assertion().id()),
            Some(claims.id())
        );
        Ok(())
    }

    #[test]
    fn stored_insertion_order_does_not_change_canonical_candidate_set() -> TestResult {
        let root = id::<HistorySpaceId>(11)?;
        let (layer_snapshot, base, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(12)?);
        let definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let left = assertion(
            13,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        let right = assertion(
            14,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        let third = assertion(
            15,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        let archive = archive_for(&[(&left, 1), (&right, 1), (&third, 1)], None)?;
        let query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
            as_of(1)?,
            timeline,
            20,
        );
        let left_record = AssertionHistoryRecord::from_assertion(left);
        let right_record = AssertionHistoryRecord::from_assertion(right);
        let third_record = AssertionHistoryRecord::from_assertion(third);
        let mut baseline = HistorySpaceReferenceModel::new(vec![definition])?;
        baseline.publish(
            root,
            vec![
                left_record.clone(),
                right_record.clone(),
                third_record.clone(),
            ],
        )?;
        let expected =
            full_scan_assertion_candidates(&baseline, &archive, &query, &layer_snapshot)?
                .into_iter()
                .map(|candidate| candidate.assertion().id())
                .collect::<Vec<_>>();
        let permutations = [
            vec![
                left_record.clone(),
                right_record.clone(),
                third_record.clone(),
            ],
            vec![
                left_record.clone(),
                third_record.clone(),
                right_record.clone(),
            ],
            vec![
                right_record.clone(),
                left_record.clone(),
                third_record.clone(),
            ],
            vec![
                right_record.clone(),
                third_record.clone(),
                left_record.clone(),
            ],
            vec![
                third_record.clone(),
                left_record.clone(),
                right_record.clone(),
            ],
            vec![third_record, right_record, left_record],
        ];
        for records in permutations {
            let definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
            let mut history = HistorySpaceReferenceModel::new(vec![definition])?;
            history.publish(root, records)?;
            let actual =
                full_scan_assertion_candidates(&history, &archive, &query, &layer_snapshot)?
                    .into_iter()
                    .map(|candidate| candidate.assertion().id())
                    .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
        Ok(())
    }

    #[test]
    fn malformed_partition_and_missing_archive_inventory_fail_closed() -> TestResult {
        let root = id::<HistorySpaceId>(15)?;
        let (layer_snapshot, base, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(16)?);
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?])?;
        let world = assertion(
            17,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(1)?,
            (0, 100),
            timeline,
        )?;
        history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(world.clone())],
        )?;
        let archive = ArchiveHistoryReferenceModel::new(vec![], vec![])?;
        let missing_inventory_query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
            as_of(1)?,
            timeline,
            20,
        );
        assert!(matches!(
            full_scan_assertion_candidates(
                &history,
                &archive,
                &missing_inventory_query,
                &layer_snapshot
            ),
            Err(CandidateScanError::MissingArchiveInventoryTarget { .. })
        ));
        let invalid_partition_query = query(
            root,
            LayerSelection::BaseOnly,
            PerspectiveScope::World,
            EpistemicMode::Knows,
            as_of(1)?,
            timeline,
            20,
        );
        let complete_archive = archive_for(&[(&world, 1)], None)?;
        assert!(matches!(
            full_scan_assertion_candidates(
                &history,
                &complete_archive,
                &invalid_partition_query,
                &layer_snapshot
            ),
            Err(CandidateScanError::Context(_))
        ));

        let mut malformed_history =
            HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
                root,
                None,
                Revision::GENESIS,
            )?])?;
        let wrong_revision = assertion(
            18,
            (
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            ),
            revision(2)?,
            (0, 100),
            timeline,
        )?;
        malformed_history.publish(
            root,
            vec![AssertionHistoryRecord::from_assertion(
                wrong_revision.clone(),
            )],
        )?;
        let revision_mismatch_archive = archive_for(&[(&wrong_revision, 2)], None)?;
        assert!(matches!(
            full_scan_assertion_candidates(
                &malformed_history,
                &revision_mismatch_archive,
                &missing_inventory_query,
                &layer_snapshot
            ),
            Err(CandidateScanError::RecordRevisionMismatch { .. })
        ));
        Ok(())
    }
}
