//! Deterministic, index-free query result contracts for the M2 reference model.

use std::fmt;

use crate::history_model::{HistorySpaceModelError, HistorySpaceReferenceModel};
use crate::ids::{AssertionId, DomainId, HistorySpaceId, Revision};
use crate::multi_value_resolution::MultiValueOutcome;
use crate::query_context::QueryContext;
use crate::record_refs::{RecordRef, RecordRefWireTag};
use crate::schema_history::{SchemaHistoryError, SchemaHistoryReferenceModel, SchemaMode};
use crate::security::{
    AuthorizationDecision, Capability, PolicyTarget, SecurityPolicyHistory,
    SecurityPolicyHistoryError,
};
use crate::single_value_resolution::SingleValueOutcome;
use crate::temporal::RecordedAsOf;

/// Identifies the immutable data/schema interpretation used by one query.
///
/// The binding includes the effective schema revision and fingerprint, so a
/// repeated query can prove that the same `SchemaMode` resolved to the same
/// schema snapshot as well as the same data revision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HistoricalQueryBinding {
    recorded_as_of: RecordedAsOf,
    schema_mode: SchemaMode,
    schema_revision: crate::ids::SchemaRevision,
    schema_fingerprint: [u8; 32],
}

impl HistoricalQueryBinding {
    /// Resolves the requested mode once and captures its immutable schema identity.
    pub fn bind(
        schemas: &SchemaHistoryReferenceModel,
        recorded_as_of: RecordedAsOf,
        schema_mode: SchemaMode,
    ) -> Result<Self, SchemaHistoryError> {
        let schema = schemas.schema_at(schema_mode, recorded_as_of.revision())?;
        Ok(Self {
            recorded_as_of,
            schema_mode,
            schema_revision: schema.schema_revision(),
            schema_fingerprint: schema.fingerprint(),
        })
    }

    /// Returns the pinned data revision.
    #[must_use]
    pub const fn recorded_as_of(self) -> RecordedAsOf {
        self.recorded_as_of
    }

    /// Returns the explicit schema interpretation mode.
    #[must_use]
    pub const fn schema_mode(self) -> SchemaMode {
        self.schema_mode
    }

    /// Returns the schema revision selected by the mode.
    #[must_use]
    pub const fn schema_revision(self) -> crate::ids::SchemaRevision {
        self.schema_revision
    }

    /// Returns the canonical fingerprint of the selected schema snapshot.
    #[must_use]
    pub const fn schema_fingerprint(self) -> [u8; 32] {
        self.schema_fingerprint
    }
}

/// One owned row from an index-free raw HistorySpace scan.
#[derive(Clone, Debug)]
pub struct RawHistoryRow<T> {
    recorded_revision: Revision,
    owner_history_space_id: HistorySpaceId,
    record_ref: RecordRef,
    value: T,
}

impl<T> RawHistoryRow<T> {
    /// Creates an owned raw row with its stored revision and record identity.
    #[must_use]
    pub const fn new(
        recorded_revision: Revision,
        owner_history_space_id: HistorySpaceId,
        record_ref: RecordRef,
        value: T,
    ) -> Self {
        Self {
            recorded_revision,
            owner_history_space_id,
            record_ref,
            value,
        }
    }

    /// Returns the revision at which the row was stored.
    #[must_use]
    pub const fn recorded_revision(&self) -> Revision {
        self.recorded_revision
    }

    /// Returns the HistorySpace that owns the record.
    #[must_use]
    pub const fn owner_history_space_id(&self) -> HistorySpaceId {
        self.owner_history_space_id
    }

    /// Returns the record's stable typed identity.
    #[must_use]
    pub const fn record_ref(&self) -> RecordRef {
        self.record_ref
    }

    /// Returns the owned record value.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

/// Scans one HistorySpace and returns owned raw rows in canonical typed-key order.
///
/// The callback classifies each already-read immutable record. It must not apply
/// resolution rules. Authorization is an outer-boundary responsibility and
/// must filter records before any rows or explanations are exposed.
pub fn full_scan_raw_history<T: Clone>(
    history: &HistorySpaceReferenceModel<T>,
    history_space_id: HistorySpaceId,
    recorded_as_of: RecordedAsOf,
    record_ref: impl Fn(&T) -> RecordRef,
) -> Result<Vec<RawHistoryRow<T>>, RawHistoryError> {
    let rows = history
        .read_at(history_space_id, recorded_as_of.revision())?
        .into_iter()
        .map(|(revision, owner, value)| {
            RawHistoryRow::new(revision, owner, record_ref(value), value.clone())
        })
        .collect::<Vec<_>>();
    canonicalize_raw_history_rows(rows)
}

/// Raw HistorySpace scan whose operation and each record use the query's
/// policy time basis. Historical mode first checks the current admin grant.
pub fn full_scan_authorized_raw_history<T: Clone>(
    history: &HistorySpaceReferenceModel<T>,
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
    record_ref: impl Fn(&T) -> RecordRef,
) -> Result<Vec<RawHistoryRow<T>>, RawHistoryError> {
    let policy_view = policies.resolve(context)?;
    let policy = policy_view.snapshot();
    let principal = context.security().principal_id();
    let rows = history
        .read_at(context.history_space(), context.recorded_as_of().revision())?
        .into_iter()
        .filter_map(|(revision, owner, value)| {
            let record = record_ref(value);
            let target = PolicyTarget::new(Some(owner), None, Some(record), None, None);
            [Capability::HistorySpaceRead, Capability::RawHistoryRead]
                .into_iter()
                .all(|capability| {
                    policy.authorize(principal, capability, target) == AuthorizationDecision::Allow
                })
                .then(|| RawHistoryRow::new(revision, owner, record, value.clone()))
        })
        .collect::<Vec<_>>();
    canonicalize_raw_history_rows(rows)
}

/// Sorts owned raw rows by typed key and rejects repeated record identities.
pub fn canonicalize_raw_history_rows<T>(
    mut rows: Vec<RawHistoryRow<T>>,
) -> Result<Vec<RawHistoryRow<T>>, RawHistoryError> {
    rows.sort_by_key(|row| record_ref_order_key(row.record_ref));
    let mut previous = None;
    for row in &rows {
        let key = record_ref_order_key(row.record_ref);
        if previous == Some(key) {
            return Err(RawHistoryError::DuplicateRecordRef {
                record_ref: row.record_ref,
            });
        }
        previous = Some(key);
    }
    Ok(rows)
}

fn record_ref_order_key(record_ref: RecordRef) -> (u16, [u8; 16]) {
    macro_rules! key {
        ($tag:ident, $id:expr) => {
            (RecordRefWireTag::$tag.value(), $id.to_bytes())
        };
    }
    match record_ref {
        RecordRef::Assertion(id) => key!(Assertion, id),
        RecordRef::Mask(id) => key!(Mask, id),
        RecordRef::ReplacementBoundary(id) => key!(ReplacementBoundary, id),
        RecordRef::Event(id) => key!(Event, id),
        RecordRef::EventMask(id) => key!(EventMask, id),
        RecordRef::EventRelation(id) => key!(EventRelation, id),
        RecordRef::Source(id) => key!(Source, id),
        RecordRef::Evidence(id) => key!(Evidence, id),
        RecordRef::Provenance(id) => key!(Provenance, id),
        RecordRef::AssertionValidityClosure(id) => key!(AssertionValidityClosure, id),
        RecordRef::AssertionRetraction(id) => key!(AssertionRetraction, id),
        RecordRef::MaskValidityClosure(id) => key!(MaskValidityClosure, id),
        RecordRef::MaskRetraction(id) => key!(MaskRetraction, id),
        RecordRef::ReplacementBoundaryValidityClosure(id) => {
            key!(ReplacementBoundaryValidityClosure, id)
        }
        RecordRef::ReplacementBoundaryRetraction(id) => key!(ReplacementBoundaryRetraction, id),
        RecordRef::EventSpanClosure(id) => key!(EventSpanClosure, id),
        RecordRef::EventRetraction(id) => key!(EventRetraction, id),
        RecordRef::EventMaskRetraction(id) => key!(EventMaskRetraction, id),
        RecordRef::EventRelationRetraction(id) => key!(EventRelationRetraction, id),
        RecordRef::EvidenceRetraction(id) => key!(EvidenceRetraction, id),
        RecordRef::ProvenanceRetraction(id) => key!(ProvenanceRetraction, id),
        RecordRef::EntityRetirement(id) => key!(EntityRetirement, id),
        RecordRef::PerspectiveRetirement(id) => key!(PerspectiveRetirement, id),
        RecordRef::ArchiveTransition(id) => key!(ArchiveTransition, id),
        RecordRef::TransferLineage(id) => key!(TransferLineage, id),
    }
}

/// The policy-specific outcome retained by one canonical `Resolved View`.
#[derive(Clone, Debug)]
pub enum ResolvedOutcome {
    /// A single-value replacement result.
    Single(SingleValueOutcome),
    /// A multi-value overlay or replacement result.
    Multi(MultiValueOutcome),
}

/// One canonical `Resolved View` outcome and its contributor identities.
#[derive(Clone, Debug)]
pub struct ResolvedView {
    outcome: ResolvedOutcome,
    contributors: Vec<AssertionId>,
}

impl ResolvedView {
    /// Wraps a single-value outcome and derives contributors from that outcome.
    pub fn from_single(outcome: SingleValueOutcome) -> Result<Self, ResolvedViewError> {
        let contributors = match &outcome {
            SingleValueOutcome::Known { contributors, .. }
            | SingleValueOutcome::Conflict { contributors } => contributors.clone(),
            SingleValueOutcome::Unknown => Vec::new(),
        };
        Self::from_outcome(ResolvedOutcome::Single(outcome), contributors)
    }

    /// Wraps a multi-value outcome and derives contributors from every value/conflict group.
    pub fn from_multi(outcome: MultiValueOutcome) -> Result<Self, ResolvedViewError> {
        let mut contributors = Vec::new();
        match &outcome {
            MultiValueOutcome::Known { values } => {
                for value in values {
                    contributors.extend_from_slice(value.contributors());
                }
            }
            MultiValueOutcome::Unknown => {}
            MultiValueOutcome::Conflict { values, conflicts } => {
                for value in values {
                    contributors.extend_from_slice(value.contributors());
                }
                for conflict in conflicts {
                    contributors.extend_from_slice(conflict.positive_contributors());
                    contributors.extend_from_slice(conflict.negative_contributors());
                }
            }
        }
        Self::from_outcome(ResolvedOutcome::Multi(outcome), contributors)
    }

    fn from_outcome(
        outcome: ResolvedOutcome,
        mut contributors: Vec<AssertionId>,
    ) -> Result<Self, ResolvedViewError> {
        contributors.sort_unstable();
        if let Some(assertion_id) = contributors.windows(2).find_map(|pair| match pair {
            [left, right] if left == right => Some(*left),
            _ => None,
        }) {
            return Err(ResolvedViewError::DuplicateContributor { assertion_id });
        }
        Ok(Self {
            outcome,
            contributors,
        })
    }

    /// Returns the domain outcome without changing its semantic variant.
    #[must_use]
    pub const fn outcome(&self) -> &ResolvedOutcome {
        &self.outcome
    }

    /// Returns contributor IDs in canonical typed-ID order.
    #[must_use]
    pub fn contributors(&self) -> &[AssertionId] {
        &self.contributors
    }
}

/// One stage of a query explanation over records already visible to its caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplainStage {
    kind: ExplainStageKind,
    input_assertions: Vec<AssertionId>,
    output_assertions: Vec<AssertionId>,
    applied_records: Vec<RecordRef>,
}

/// Closed stage vocabulary for the assertion Resolved View reference path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExplainStageKind {
    /// Context, lifecycle, archive, and time filtered full-scan candidates.
    CandidateScan,
    /// Assertion Masks applied to visible full-scan candidates.
    MaskProjection,
    /// ReplacementBoundary cutoff applied to post-mask candidates.
    ReplacementBoundary,
    /// Schema-driven resolution selected the visible contributors.
    Resolution,
}

impl ExplainStage {
    /// Builds one stage and canonicalizes its assertion and record references.
    pub fn new(
        kind: ExplainStageKind,
        mut input_assertions: Vec<AssertionId>,
        mut output_assertions: Vec<AssertionId>,
        mut applied_records: Vec<RecordRef>,
    ) -> Result<Self, ExplainStageError> {
        input_assertions.sort_unstable();
        output_assertions.sort_unstable();
        applied_records.sort_by_key(|reference| record_ref_order_key(*reference));
        reject_duplicate_assertions(&input_assertions)?;
        reject_duplicate_assertions(&output_assertions)?;
        let records_match_stage = match kind {
            ExplainStageKind::CandidateScan | ExplainStageKind::Resolution => {
                applied_records.is_empty()
            }
            ExplainStageKind::MaskProjection => applied_records
                .iter()
                .all(|record| matches!(record, RecordRef::Mask(_))),
            ExplainStageKind::ReplacementBoundary => applied_records
                .iter()
                .all(|record| matches!(record, RecordRef::ReplacementBoundary(_))),
        };
        if !records_match_stage {
            return Err(ExplainStageError::RecordFamilyDoesNotMatchStage);
        }
        if let Some(record_ref) = applied_records.windows(2).find_map(|pair| match pair {
            [left, right] if record_ref_order_key(*left) == record_ref_order_key(*right) => {
                Some(*left)
            }
            _ => None,
        }) {
            return Err(ExplainStageError::DuplicateAppliedRecord { record_ref });
        }
        if kind != ExplainStageKind::CandidateScan
            && output_assertions
                .iter()
                .any(|assertion_id| input_assertions.binary_search(assertion_id).is_err())
        {
            return Err(ExplainStageError::StageIntroducedAssertion);
        }
        Ok(Self {
            kind,
            input_assertions,
            output_assertions,
            applied_records,
        })
    }

    /// Returns the stage class.
    #[must_use]
    pub const fn kind(&self) -> ExplainStageKind {
        self.kind
    }

    /// Returns the sorted input candidate identities.
    #[must_use]
    pub fn input_assertions(&self) -> &[AssertionId] {
        &self.input_assertions
    }

    /// Returns the sorted output candidate identities.
    #[must_use]
    pub fn output_assertions(&self) -> &[AssertionId] {
        &self.output_assertions
    }

    /// Returns the sorted Mask/Boundary record identities applied by this stage.
    #[must_use]
    pub fn applied_records(&self) -> &[RecordRef] {
        &self.applied_records
    }
}

/// Reproducible Explain trace captured against one immutable query binding.
#[derive(Clone, Debug)]
pub struct ReferenceExplain {
    binding: HistoricalQueryBinding,
    stages: Vec<ExplainStage>,
    resolved_view: ResolvedView,
}

impl ReferenceExplain {
    /// Validates a contiguous pipeline and binds it to the query's data/schema snapshot.
    pub fn new(
        binding: HistoricalQueryBinding,
        stages: Vec<ExplainStage>,
        resolved_view: ResolvedView,
    ) -> Result<Self, ReferenceExplainError> {
        if stages
            .first()
            .is_none_or(|stage| stage.kind != ExplainStageKind::CandidateScan)
        {
            return Err(ReferenceExplainError::MissingCandidateScan);
        }
        let mut previous_kind = None;
        let mut previous_output = None;
        for stage in &stages {
            if previous_kind.is_some_and(|kind| kind >= stage.kind) {
                return Err(ReferenceExplainError::StageOrderOrDuplicate);
            }
            if previous_output.is_some_and(|output| stage.input_assertions.as_slice() != output) {
                return Err(ReferenceExplainError::DiscontinuousStages);
            }
            previous_kind = Some(stage.kind);
            previous_output = Some(stage.output_assertions.as_slice());
        }
        let contributors = resolved_view.contributors();
        let final_output = &stages
            .last()
            .ok_or(ReferenceExplainError::MissingCandidateScan)?
            .output_assertions;
        if contributors
            .iter()
            .any(|id| final_output.binary_search(id).is_err())
        {
            return Err(ReferenceExplainError::ContributorOutsideFinalStage);
        }
        if stages
            .last()
            .is_some_and(|stage| stage.kind != ExplainStageKind::Resolution)
        {
            return Err(ReferenceExplainError::MissingResolution);
        }
        Ok(Self {
            binding,
            stages,
            resolved_view,
        })
    }

    /// Returns the immutable data/schema binding.
    #[must_use]
    pub const fn binding(&self) -> HistoricalQueryBinding {
        self.binding
    }

    /// Returns ordered resolution stages.
    #[must_use]
    pub fn stages(&self) -> &[ExplainStage] {
        &self.stages
    }

    /// Returns the canonical visible contributor set.
    #[must_use]
    pub fn contributors(&self) -> &[AssertionId] {
        self.resolved_view.contributors()
    }

    /// Returns the exact resolved outcome whose contributors the trace explains.
    #[must_use]
    pub const fn resolved_view(&self) -> &ResolvedView {
        &self.resolved_view
    }
}

fn reject_duplicate_assertions(ids: &[AssertionId]) -> Result<(), ExplainStageError> {
    if let Some(assertion_id) = ids.windows(2).find_map(|pair| match pair {
        [left, right] if left == right => Some(*left),
        _ => None,
    }) {
        return Err(ExplainStageError::DuplicateAssertion { assertion_id });
    }
    Ok(())
}

/// Raw scan errors, including duplicate stable identities from the supplied family mapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawHistoryError {
    /// The selected HistorySpace history could not be read.
    History(HistorySpaceModelError),
    /// Policy time selection was invalid, unavailable, or not administratively allowed.
    SecurityHistory(SecurityPolicyHistoryError),
    /// Two records were assigned the same typed identity.
    DuplicateRecordRef { record_ref: RecordRef },
}

impl From<HistorySpaceModelError> for RawHistoryError {
    fn from(error: HistorySpaceModelError) -> Self {
        Self::History(error)
    }
}

impl From<SecurityPolicyHistoryError> for RawHistoryError {
    fn from(error: SecurityPolicyHistoryError) -> Self {
        Self::SecurityHistory(error)
    }
}

impl fmt::Display for RawHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::History(error) => write!(formatter, "raw history scan failed: {error}"),
            Self::SecurityHistory(error) => {
                write!(formatter, "security history selection failed: {error}")
            }
            Self::DuplicateRecordRef { record_ref } => {
                write!(
                    formatter,
                    "raw history repeats record reference {record_ref:?}"
                )
            }
        }
    }
}

impl std::error::Error for RawHistoryError {}

/// Duplicate contributor identity in a resolved result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedViewError {
    /// One assertion was returned more than once as a contributor.
    DuplicateContributor { assertion_id: AssertionId },
}

impl fmt::Display for ResolvedViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateContributor { assertion_id } => {
                write!(
                    formatter,
                    "resolved view repeats contributor {assertion_id}"
                )
            }
        }
    }
}

impl std::error::Error for ResolvedViewError {}

/// Invalid explain-stage records or candidate transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplainStageError {
    /// One assertion identity occurs more than once in a stage.
    DuplicateAssertion { assertion_id: AssertionId },
    /// One stage lists the same Mask or Boundary record more than once.
    DuplicateAppliedRecord { record_ref: RecordRef },
    /// A filtering/resolution stage cannot introduce an assertion absent from its input.
    StageIntroducedAssertion,
    /// A stage's applied record references do not belong to that stage class.
    RecordFamilyDoesNotMatchStage,
}

impl fmt::Display for ExplainStageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAssertion { assertion_id } => {
                write!(formatter, "explain stage repeats assertion {assertion_id}")
            }
            Self::DuplicateAppliedRecord { record_ref } => {
                write!(
                    formatter,
                    "explain stage repeats applied record {record_ref:?}"
                )
            }
            Self::StageIntroducedAssertion => {
                formatter.write_str("explain stage output is not a subset of its input")
            }
            Self::RecordFamilyDoesNotMatchStage => {
                formatter.write_str("explain stage lists a record from another family")
            }
        }
    }
}

impl std::error::Error for ExplainStageError {}

/// Invalid explain trace ordering or result membership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceExplainError {
    /// The trace must begin with one CandidateScan stage.
    MissingCandidateScan,
    /// Stages must appear once in CandidateScan, Mask, Boundary, Resolution order.
    StageOrderOrDuplicate,
    /// A stage's input differs from the prior stage's output.
    DiscontinuousStages,
    /// The trace must end with schema-driven Resolution.
    MissingResolution,
    /// A contributor does not appear in the final stage's candidates.
    ContributorOutsideFinalStage,
}

impl fmt::Display for ReferenceExplainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingCandidateScan => "explain trace must start with CandidateScan",
            Self::StageOrderOrDuplicate => "explain stages are duplicated or out of order",
            Self::DiscontinuousStages => "explain stages do not form a contiguous pipeline",
            Self::MissingResolution => "explain trace must end with Resolution",
            Self::ContributorOutsideFinalStage => {
                "resolved contributor is absent from the final explain stage"
            }
        })
    }
}

impl std::error::Error for ReferenceExplainError {}

#[cfg(test)]
mod tests {
    use super::{
        ExplainStage, ExplainStageError, ExplainStageKind, HistoricalQueryBinding, RawHistoryError,
        RawHistoryRow, ReferenceExplain, ResolvedView, ResolvedViewError,
        canonicalize_raw_history_rows,
    };
    use crate::ids::{AssertionId, DomainId, HistorySpaceId, Revision, SchemaRevision};
    use crate::record_refs::RecordRef;
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::single_value_resolution::SingleValueOutcome;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(crate::IdValidationError),
        Revision(crate::RevisionError),
        HistorySpace(crate::HistorySpaceError),
        History(crate::HistorySpaceModelError),
        Schema(crate::SchemaHistoryError),
        Raw(RawHistoryError),
        Resolved(ResolvedViewError),
        ExplainStage(ExplainStageError),
        Explain(crate::ReferenceExplainError),
        Missing(&'static str),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::HistorySpace(error) => write!(formatter, "{error}"),
                Self::History(error) => write!(formatter, "{error}"),
                Self::Schema(error) => write!(formatter, "{error}"),
                Self::Raw(error) => write!(formatter, "{error}"),
                Self::Resolved(error) => write!(formatter, "{error}"),
                Self::ExplainStage(error) => write!(formatter, "{error}"),
                Self::Explain(error) => write!(formatter, "{error}"),
                Self::Missing(label) => write!(formatter, "missing expected {label}"),
            }
        }
    }

    impl std::error::Error for TestError {}

    macro_rules! error_conversion {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }
    error_conversion!(crate::IdValidationError, Id);
    error_conversion!(crate::RevisionError, Revision);
    error_conversion!(crate::HistorySpaceError, HistorySpace);
    error_conversion!(crate::HistorySpaceModelError, History);
    error_conversion!(crate::SchemaHistoryError, Schema);
    error_conversion!(RawHistoryError, Raw);
    error_conversion!(ResolvedViewError, Resolved);
    error_conversion!(ExplainStageError, ExplainStage);
    error_conversion!(crate::ReferenceExplainError, Explain);

    impl From<&'static str> for TestError {
        fn from(label: &'static str) -> Self {
            Self::Missing(label)
        }
    }
    use crate::temporal::RecordedAsOf;

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, crate::RevisionError> {
        Revision::new(value)
    }

    fn as_of(value: u64) -> Result<RecordedAsOf, crate::RevisionError> {
        Ok(RecordedAsOf::from_published_revision(revision(value)?))
    }

    #[test]
    fn historical_binding_repeats_exactly_for_the_same_data_and_schema_snapshots() -> TestResult {
        let mut schemas = SchemaHistoryReferenceModel::new();
        schemas.publish(revision(1)?, Vec::<SchemaDefinition>::new())?;
        let first = HistoricalQueryBinding::bind(&schemas, as_of(0)?, SchemaMode::Historical)?;
        let repeated = HistoricalQueryBinding::bind(&schemas, as_of(0)?, SchemaMode::Historical)?;
        let current = HistoricalQueryBinding::bind(&schemas, as_of(0)?, SchemaMode::Current)?;
        assert_eq!(first, repeated);
        assert_ne!(first, current);
        assert_eq!(
            first.schema_revision(),
            SchemaRevision::from_published_revision(revision(0)?)
        );
        assert_eq!(
            current.schema_revision(),
            SchemaRevision::from_published_revision(revision(1)?)
        );
        Ok(())
    }

    #[test]
    fn raw_rows_sort_by_fixed_typed_tag_then_id_and_reject_duplicate_identity() -> TestResult {
        let assertion = RecordRef::Assertion(id::<AssertionId>(9)?);
        let earlier_assertion = RecordRef::Assertion(id::<AssertionId>(2)?);
        let mask = RecordRef::Mask(id::<crate::MaskId>(1)?);
        let rows = vec![
            RawHistoryRow::new(revision(3)?, id::<HistorySpaceId>(3)?, mask, "mask"),
            RawHistoryRow::new(
                revision(2)?,
                id::<HistorySpaceId>(3)?,
                assertion,
                "assertion",
            ),
            RawHistoryRow::new(
                revision(1)?,
                id::<HistorySpaceId>(3)?,
                earlier_assertion,
                "early",
            ),
        ];
        let rows = canonicalize_raw_history_rows(rows)?;
        let mut row_iter = rows.iter();
        assert_eq!(
            row_iter.next().ok_or("missing first row")?.record_ref(),
            earlier_assertion
        );
        assert_eq!(
            row_iter.next().ok_or("missing second row")?.record_ref(),
            assertion
        );
        assert_eq!(
            row_iter.next().ok_or("missing third row")?.record_ref(),
            mask
        );

        let duplicate = canonicalize_raw_history_rows(vec![
            RawHistoryRow::new(revision(1)?, id::<HistorySpaceId>(3)?, assertion, "one"),
            RawHistoryRow::new(revision(2)?, id::<HistorySpaceId>(3)?, assertion, "two"),
        ]);
        assert_eq!(
            duplicate.err(),
            Some(RawHistoryError::DuplicateRecordRef {
                record_ref: assertion
            })
        );
        Ok(())
    }

    #[test]
    fn raw_full_scan_pins_recorded_revision_and_returns_owned_canonical_rows() -> TestResult {
        #[derive(Clone)]
        struct RawRecord(RecordRef);

        let space = id::<HistorySpaceId>(3)?;
        let earlier = RecordRef::Assertion(id::<AssertionId>(2)?);
        let later = RecordRef::Assertion(id::<AssertionId>(8)?);
        let mask = RecordRef::Mask(id::<crate::MaskId>(1)?);
        let mut history =
            crate::HistorySpaceReferenceModel::new(vec![crate::HistorySpaceDefinition::new(
                space,
                None,
                Revision::GENESIS,
            )?])?;
        history.publish(space, vec![RawRecord(later), RawRecord(earlier)])?;
        history.publish(space, vec![RawRecord(mask)])?;

        let rows = super::full_scan_raw_history(&history, space, as_of(1)?, |record| record.0)?;
        assert_eq!(rows.len(), 2);
        let mut row_iter = rows.iter();
        let first_row = row_iter.next().ok_or("missing first row")?;
        let second_row = row_iter.next().ok_or("missing second row")?;
        assert_eq!(first_row.record_ref(), earlier);
        assert_eq!(second_row.record_ref(), later);
        assert_eq!(first_row.recorded_revision(), revision(1)?);
        assert!(matches!(
            first_row.value(),
            RawRecord(RecordRef::Assertion(_))
        ));
        let current = super::full_scan_raw_history(&history, space, as_of(2)?, |record| record.0)?;
        assert_eq!(current.len(), 3);
        assert_eq!(
            current.last().ok_or("missing current rows")?.record_ref(),
            mask
        );
        Ok(())
    }

    #[test]
    fn resolved_view_orders_contributors_and_rejects_duplicates() -> TestResult {
        let later = id::<AssertionId>(8)?;
        let earlier = id::<AssertionId>(3)?;
        let resolved = ResolvedView::from_single(SingleValueOutcome::Conflict {
            contributors: vec![later, earlier],
        })?;
        assert_eq!(resolved.contributors(), &[earlier, later]);
        assert_eq!(
            ResolvedView::from_single(SingleValueOutcome::Conflict {
                contributors: vec![earlier, earlier],
            })
            .err(),
            Some(ResolvedViewError::DuplicateContributor {
                assertion_id: earlier
            })
        );
        Ok(())
    }

    #[test]
    fn explain_trace_binds_sorted_mask_and_boundary_stages_to_the_same_snapshot() -> TestResult {
        let schemas = SchemaHistoryReferenceModel::new();
        let binding = HistoricalQueryBinding::bind(&schemas, as_of(0)?, SchemaMode::Historical)?;
        let first = id::<AssertionId>(1)?;
        let second = id::<AssertionId>(2)?;
        let contributor = id::<AssertionId>(2)?;
        let first_mask = RecordRef::Mask(id::<crate::MaskId>(4)?);
        let second_mask = RecordRef::Mask(id::<crate::MaskId>(5)?);
        let stages = vec![
            ExplainStage::new(
                ExplainStageKind::CandidateScan,
                vec![],
                vec![second, first],
                vec![],
            )?,
            ExplainStage::new(
                ExplainStageKind::MaskProjection,
                vec![first, second],
                vec![second],
                vec![second_mask, first_mask],
            )?,
            ExplainStage::new(
                ExplainStageKind::ReplacementBoundary,
                vec![second],
                vec![second],
                vec![RecordRef::ReplacementBoundary(id::<
                    crate::ReplacementBoundaryId,
                >(6)?)],
            )?,
            ExplainStage::new(
                ExplainStageKind::Resolution,
                vec![second],
                vec![second],
                vec![],
            )?,
        ];
        let resolved = ResolvedView::from_single(SingleValueOutcome::Conflict {
            contributors: vec![contributor],
        })?;
        let explain = ReferenceExplain::new(binding, stages, resolved)?;
        assert_eq!(explain.binding(), binding);
        let mut stage_iter = explain.stages().iter();
        assert_eq!(
            stage_iter
                .next()
                .ok_or("missing scan stage")?
                .output_assertions(),
            &[first, second]
        );
        assert_eq!(
            stage_iter
                .next()
                .ok_or("missing mask stage")?
                .applied_records(),
            &[first_mask, second_mask]
        );
        assert_eq!(explain.contributors(), &[contributor]);
        assert!(matches!(
            ExplainStage::new(
                ExplainStageKind::MaskProjection,
                vec![first],
                vec![],
                vec![RecordRef::Event(id::<crate::EventId>(7)?)],
            ),
            Err(ExplainStageError::RecordFamilyDoesNotMatchStage)
        ));
        Ok(())
    }
}
