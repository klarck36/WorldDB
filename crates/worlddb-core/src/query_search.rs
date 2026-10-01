//! Index-free token search over caller-visible, field-authorized query rows.

use std::collections::BTreeSet;
use std::fmt;

use crate::ids::{HistorySpaceId, LayerId};
use crate::query_context::QueryContext;
use crate::query_ports::OwnedQueryResult;
use crate::record_refs::RecordRef;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicyHistory,
    SecurityPolicyHistoryError,
};

/// Token matching semantics for deterministic search.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SearchMatch {
    /// Every distinct request term must occur in at least one selected field.
    AllTerms,
    /// At least one request term must occur in a selected field.
    AnyTerm,
}

/// Validated exact UTF-8 token. Tokens cannot be empty or contain ASCII whitespace.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SearchToken(String);

impl SearchToken {
    /// Creates a token with the fixed 1.0 lexical rules.
    pub fn new(value: impl Into<String>) -> Result<Self, QuerySearchError> {
        let value = value.into();
        if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(QuerySearchError::InvalidToken);
        }
        Ok(Self(value))
    }

    /// Exact case-sensitive UTF-8 token bytes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical request for index-free TokenSearch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchSpec {
    fields: Vec<FieldSelector>,
    terms: Vec<SearchToken>,
    matching: SearchMatch,
}

impl SearchSpec {
    /// Requires at least one field and term; canonicalizes both lists and removes duplicate terms.
    pub fn new(
        mut fields: Vec<FieldSelector>,
        mut terms: Vec<SearchToken>,
        matching: SearchMatch,
    ) -> Result<Self, QuerySearchError> {
        if fields.is_empty() || terms.is_empty() {
            return Err(QuerySearchError::EmptySpec);
        }
        fields.sort_unstable();
        if fields.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left == right)
        }) {
            return Err(QuerySearchError::DuplicateField);
        }
        terms.sort_unstable();
        terms.dedup();
        Ok(Self {
            fields,
            terms,
            matching,
        })
    }

    /// Canonical searchable fields.
    #[must_use]
    pub fn fields(&self) -> &[FieldSelector] {
        &self.fields
    }

    /// Canonical request terms.
    #[must_use]
    pub fn terms(&self) -> &[SearchToken] {
        &self.terms
    }
}

/// One typed text field on a candidate record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchTextField {
    selector: FieldSelector,
    text: String,
}

impl SearchTextField {
    /// Binds exact text to a schema field selector.
    #[must_use]
    pub fn new(selector: FieldSelector, text: impl Into<String>) -> Self {
        Self {
            selector,
            text: text.into(),
        }
    }

    /// Field selector.
    #[must_use]
    pub const fn selector(&self) -> FieldSelector {
        self.selector
    }
}

/// Candidate supplied by the index-free reference source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchDocument {
    result_key: RecordRef,
    history_space: HistorySpaceId,
    layer: LayerId,
    text_fields: Vec<SearchTextField>,
}

impl SearchDocument {
    /// Creates one candidate with unique text-field selectors.
    pub fn new(
        result_key: RecordRef,
        history_space: HistorySpaceId,
        layer: LayerId,
        text_fields: Vec<SearchTextField>,
    ) -> Result<Self, QuerySearchError> {
        let mut selectors = text_fields
            .iter()
            .map(SearchTextField::selector)
            .collect::<Vec<_>>();
        selectors.sort_unstable();
        if selectors.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left == right)
        }) {
            return Err(QuerySearchError::DuplicateField);
        }
        Ok(Self {
            result_key,
            history_space,
            layer,
            text_fields,
        })
    }
}

/// Snippet-free match result, ordered by the typed caller-visible `RecordRef` key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchHit {
    result_key: RecordRef,
    matched_fields: Vec<FieldSelector>,
}

impl SearchHit {
    /// Typed result identity.
    #[must_use]
    pub const fn result_key(&self) -> RecordRef {
        self.result_key
    }

    /// Fields that contain one or more matched request terms; no text is returned.
    #[must_use]
    pub fn matched_fields(&self) -> &[FieldSelector] {
        &self.matched_fields
    }
}

/// Executes deterministic TokenSearch on an index-free candidate slice.
///
/// Candidates are security-filtered before field tokenization, candidate/work budgets,
/// or result assembly. The schema-derived searchable-field set must identify fields
/// whose pinned-schema ValueKind is text. Budget, cancellation, and security failures
/// return no partial result.
pub fn full_scan_token_search(
    documents: &[SearchDocument],
    spec: &SearchSpec,
    schema_text_fields: &[FieldSelector],
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
) -> Result<OwnedQueryResult<Vec<SearchHit>>, QuerySearchError> {
    let text_fields = schema_text_fields.iter().copied().collect::<BTreeSet<_>>();
    if spec.fields.iter().any(|field| !text_fields.contains(field)) {
        return Err(QuerySearchError::FieldNotTextValued);
    }
    let security = policies.resolve(context)?;
    let policy = security.snapshot();
    let principal = context.security().principal_id();
    if policy.authorize(principal, Capability::QuerySearch, PolicyTarget::default())
        != AuthorizationDecision::Allow
    {
        return Err(QuerySearchError::Unauthorized);
    }

    let mut authorized = documents.iter().collect::<Vec<_>>();
    authorized.sort_by_key(|document| document.result_key);
    let mut hits = Vec::new();
    let mut seen = BTreeSet::new();
    let mut candidates = 0_u64;
    let mut work = 0_u64;
    let budget = context.budget();

    for document in authorized {
        check_cancelled(context)?;
        let Some(record_capability) = record_read_capability(document.result_key) else {
            continue;
        };
        if spec
            .fields
            .iter()
            .any(|field| !field_applies_to_record(document.result_key, *field))
        {
            continue;
        }
        let record_target = PolicyTarget::new(
            Some(document.history_space),
            Some(document.layer),
            Some(document.result_key),
            None,
            None,
        );
        if policy.authorize(principal, record_capability, record_target)
            != AuthorizationDecision::Allow
        {
            continue;
        }
        let fields_visible = spec.fields.iter().all(|field| {
            let target = PolicyTarget::new(
                Some(document.history_space),
                Some(document.layer),
                Some(document.result_key),
                Some(*field),
                None,
            );
            policy.authorize(principal, Capability::FieldRead, target)
                == AuthorizationDecision::Allow
        });
        if !fields_visible {
            continue;
        }

        candidates = candidates
            .checked_add(1)
            .ok_or(QuerySearchError::BudgetExceeded)?;
        if candidates > budget.max_candidates().get() {
            return Err(QuerySearchError::BudgetExceeded);
        }
        if !seen.insert(document.result_key) {
            return Err(QuerySearchError::DuplicateResultKey);
        }

        let mut matched_fields = Vec::new();
        let mut matched_terms = BTreeSet::new();
        for field in &document.text_fields {
            if spec.fields.binary_search(&field.selector).is_err() {
                continue;
            }
            check_cancelled(context)?;
            for token in field
                .text
                .split(|character: char| character.is_ascii_whitespace())
            {
                if token.is_empty() {
                    continue;
                }
                work = work
                    .checked_add(1)
                    .ok_or(QuerySearchError::BudgetExceeded)?;
                if work > budget.max_work_units().get() {
                    return Err(QuerySearchError::BudgetExceeded);
                }
                for (index, term) in spec.terms.iter().enumerate() {
                    work = work
                        .checked_add(1)
                        .ok_or(QuerySearchError::BudgetExceeded)?;
                    if work > budget.max_work_units().get() {
                        return Err(QuerySearchError::BudgetExceeded);
                    }
                    if token.as_bytes() == term.as_str().as_bytes() {
                        matched_terms.insert(index);
                        if !matched_fields.contains(&field.selector) {
                            matched_fields.push(field.selector);
                        }
                    }
                }
            }
        }
        let is_match = match spec.matching {
            SearchMatch::AllTerms => matched_terms.len() == spec.terms.len(),
            SearchMatch::AnyTerm => !matched_terms.is_empty(),
        };
        if is_match {
            if hits.len() as u64 >= budget.max_results().get() {
                return Err(QuerySearchError::BudgetExceeded);
            }
            matched_fields.sort_unstable();
            hits.push(SearchHit {
                result_key: document.result_key,
                matched_fields,
            });
        }
    }

    OwnedQueryResult::bind(context, policies, hits).map_err(QuerySearchError::Security)
}

fn check_cancelled(context: &QueryContext) -> Result<(), QuerySearchError> {
    if context.cancellation().is_cancelled() {
        Err(QuerySearchError::Cancelled)
    } else {
        Ok(())
    }
}

fn record_read_capability(record_ref: RecordRef) -> Option<Capability> {
    match record_ref {
        RecordRef::Assertion(_)
        | RecordRef::AssertionValidityClosure(_)
        | RecordRef::AssertionRetraction(_) => Some(Capability::AssertionRead),
        RecordRef::Mask(_) | RecordRef::MaskValidityClosure(_) | RecordRef::MaskRetraction(_) => {
            Some(Capability::MaskRead)
        }
        RecordRef::ReplacementBoundary(_)
        | RecordRef::ReplacementBoundaryValidityClosure(_)
        | RecordRef::ReplacementBoundaryRetraction(_) => Some(Capability::ReplacementBoundaryRead),
        RecordRef::Event(_) | RecordRef::EventSpanClosure(_) | RecordRef::EventRetraction(_) => {
            Some(Capability::EventRead)
        }
        RecordRef::EventMask(_) | RecordRef::EventMaskRetraction(_) => {
            Some(Capability::EventMaskRead)
        }
        RecordRef::Source(_) => Some(Capability::SourceRead),
        RecordRef::Evidence(_) | RecordRef::EvidenceRetraction(_) => Some(Capability::EvidenceRead),
        RecordRef::Provenance(_) | RecordRef::ProvenanceRetraction(_) => {
            Some(Capability::ProvenanceRead)
        }
        RecordRef::EntityRetirement(_) => Some(Capability::EntityRead),
        RecordRef::PerspectiveRetirement(_) => Some(Capability::PerspectiveRead),
        RecordRef::EventRelation(_)
        | RecordRef::EventRelationRetraction(_)
        | RecordRef::ArchiveTransition(_)
        | RecordRef::TransferLineage(_) => None,
    }
}

fn field_applies_to_record(record_ref: RecordRef, field: FieldSelector) -> bool {
    match record_ref {
        RecordRef::Assertion(_) => matches!(
            field,
            FieldSelector::AssertionSubject
                | FieldSelector::AssertionPredicate
                | FieldSelector::AssertionValue(_)
                | FieldSelector::AssertionPolarity
                | FieldSelector::AssertionValidity
                | FieldSelector::AssertionPerspective
                | FieldSelector::AssertionEpistemicMode
        ),
        RecordRef::Event(_) => matches!(
            field,
            FieldSelector::EventKind
                | FieldSelector::EventParticipant(_, _)
                | FieldSelector::EventAttribute(_, _)
                | FieldSelector::EventTime(_)
        ),
        RecordRef::Source(_) => matches!(
            field,
            FieldSelector::SourceKind
                | FieldSelector::SourceLocator
                | FieldSelector::SourceContentDigest
                | FieldSelector::SourceMetadata
        ),
        _ => false,
    }
}

/// Invalid search syntax, unavailable capability, or terminal query condition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QuerySearchError {
    /// No fields or no terms were supplied.
    EmptySpec,
    /// A field occurred more than once in a canonical request/document.
    DuplicateField,
    /// A token was empty or contained ASCII whitespace.
    InvalidToken,
    /// A requested field is not text-valued in the pinned schema.
    FieldNotTextValued,
    /// Operation permission is not granted.
    Unauthorized,
    /// Multiple visible candidates have the same typed result identity.
    DuplicateResultKey,
    /// Candidate, work, or result budget was exhausted; no partial result is returned.
    BudgetExceeded,
    /// The host cancelled the query; no partial result is returned.
    Cancelled,
    /// Query policy selection could not be resolved.
    Security(SecurityPolicyHistoryError),
}

impl fmt::Display for QuerySearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptySpec => "search requires at least one field and term",
            Self::DuplicateField => "search contains a duplicate field",
            Self::InvalidToken => "search token is invalid",
            Self::FieldNotTextValued => "search field is not text-valued",
            Self::Unauthorized => "search is not authorized",
            Self::DuplicateResultKey => "search contains duplicate visible result keys",
            Self::BudgetExceeded => "search budget exceeded",
            Self::Cancelled => "search was cancelled",
            Self::Security(_) => "search security context is invalid",
        })
    }
}

impl std::error::Error for QuerySearchError {}

impl From<SecurityPolicyHistoryError> for QuerySearchError {
    fn from(value: SecurityPolicyHistoryError) -> Self {
        Self::Security(value)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        QuerySearchError, SearchDocument, SearchMatch, SearchSpec, SearchTextField, SearchToken,
        full_scan_token_search,
    };
    use crate::context::{EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, DomainId as IdDomain, HistorySpaceId, LayerId, PolicyRuleId, PredicateId,
        PrincipalId, Revision, SchemaRevision, SecurityEpoch, SnapshotId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot, LayerSelection};
    use crate::non_interference::{
        CursorObservation, PairedWorld, PublicFailure, PublicObservation,
    };
    use crate::query_context::{
        AuthorizationMode, CancellationToken, QueryBudget, QueryBudgetLimits, QueryContext,
        QueryContextInput, SecurityContext, ValidatedLayerSelection, WorldTimeSelector,
    };
    use crate::record_refs::RecordRef;
    use crate::reference_query::HistoricalQueryBinding;
    use crate::schema::Lifecycle;
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        AuthorizationDecision, Capability, CapabilityGrant, CapabilityRule, FieldSelector,
        GrantEffect, PolicyScope, PolicySubject, PolicyTarget, Principal, ProvenanceRelationship,
        RelationshipSelector, SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
    };
    use crate::temporal::RecordedAsOf;
    use crate::values::Symbol;

    pub(crate) struct Fixture {
        pub(crate) context: QueryContext,
        pub(crate) policies: SecurityPolicyHistory,
        pub(crate) history_space: HistorySpaceId,
        pub(crate) layer: LayerId,
        pub(crate) selector: FieldSelector,
    }

    pub(crate) fn id<T: IdDomain>(tail: u8) -> T {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).unwrap_or_else(|_| unreachable!("test ID is valid"))
    }

    fn fixture(deny_record: Option<RecordRef>, result_limit: u64) -> Fixture {
        fixture_with_candidate_limit(deny_record, result_limit, 1)
    }

    pub(crate) fn fixture_with_candidate_limit(
        deny_record: Option<RecordRef>,
        result_limit: u64,
        candidate_limit: u64,
    ) -> Fixture {
        fixture_with_denials(deny_record, None, result_limit, candidate_limit)
    }

    pub(crate) fn fixture_with_denials(
        deny_record: Option<RecordRef>,
        deny_relationship_record: Option<RecordRef>,
        result_limit: u64,
        candidate_limit: u64,
    ) -> Fixture {
        let principal = id::<PrincipalId>(1);
        let history_space = id::<HistorySpaceId>(2);
        let layer = id::<LayerId>(3);
        let predicate = id::<PredicateId>(4);
        let selector = FieldSelector::AssertionValue(predicate);
        let revision = Revision::new(2).unwrap_or(Revision::GENESIS);
        let schema_revision = SchemaRevision::from_published_revision(revision);
        let layer_definition = LayerDefinition::new(
            layer,
            Symbol::new("world").unwrap_or_else(|_| unreachable!("test symbol is valid")),
            None,
            0,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(Revision::GENESIS),
        );
        let schema = LayerSchemaSnapshot::new(schema_revision, vec![layer_definition], layer)
            .unwrap_or_else(|_| unreachable!("test schema is valid"));
        let mut schema_history = SchemaHistoryReferenceModel::new();
        let _ = schema_history.publish(
            revision,
            vec![SchemaDefinition::LayerSnapshot(schema.clone())],
        );
        let as_of = RecordedAsOf::from_published_revision(revision);
        let schema_binding =
            HistoricalQueryBinding::bind(&schema_history, as_of, SchemaMode::Historical)
                .unwrap_or_else(|_| unreachable!("historical test schema is present"));
        let budget_limits = QueryBudgetLimits::new(10, 100, 10)
            .unwrap_or_else(|_| unreachable!("positive test limits"));
        let budget = QueryBudget::new(candidate_limit, 100, result_limit, budget_limits)
            .unwrap_or_else(|_| unreachable!("test request is within hard limits"));
        let context = QueryContext::new(QueryContextInput {
            snapshot: crate::record_refs::SnapshotRef::new(id::<SnapshotId>(5)),
            snapshot_revision: revision,
            recorded_as_of: as_of,
            history_space,
            layers: ValidatedLayerSelection::resolve(&schema, LayerSelection::BaseOnly)
                .unwrap_or_else(|_| unreachable!("base layer is valid")),
            world_time: WorldTimeSelector::AllTimes,
            perspective: PerspectiveScope::World,
            epistemic_mode: EpistemicMode::WorldState,
            schema_binding,
            security: SecurityContext::new(principal, AuthorizationMode::Now),
            budget,
            cancellation: CancellationToken::new(),
        })
        .unwrap_or_else(|_| unreachable!("test query context is valid"));

        let mut rules = vec![
            rule(
                40,
                principal,
                Capability::QuerySearch,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
            rule(
                41,
                principal,
                Capability::AssertionRead,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
            rule(
                42,
                principal,
                Capability::FieldRead,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
            rule(
                44,
                principal,
                Capability::QueryGraphTraverse,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
            rule(
                45,
                principal,
                Capability::QueryAggregate,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
            rule(
                46,
                principal,
                Capability::ProvenanceRead,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
            rule(
                47,
                principal,
                Capability::RelationshipRead,
                GrantEffect::Allow,
                PolicyScope::project(),
            ),
        ];
        if let Some(record) = deny_record {
            rules.push(rule(
                43,
                principal,
                Capability::AssertionRead,
                GrantEffect::Deny,
                PolicyScope::new(Some(history_space), Some(layer), Some(record), None, None),
            ));
        }
        if let Some(record) = deny_relationship_record {
            rules.push(rule(
                49,
                principal,
                Capability::RelationshipRead,
                GrantEffect::Deny,
                PolicyScope::new(
                    Some(history_space),
                    Some(layer),
                    Some(record),
                    None,
                    Some(RelationshipSelector::Provenance(
                        ProvenanceRelationship::DerivedFrom,
                    )),
                ),
            ));
        }
        let policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            Vec::new(),
            Vec::new(),
            rules,
        )
        .unwrap_or_else(|_| unreachable!("test policy is valid"));
        assert_eq!(
            policy.authorize(principal, Capability::QuerySearch, PolicyTarget::default()),
            AuthorizationDecision::Allow
        );
        let policies = SecurityPolicyHistory::new(
            revision,
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    policy.clone(),
                ),
                SecurityPolicyVersion::new(
                    Revision::new(1).unwrap_or(Revision::GENESIS),
                    SecurityEpoch::INITIAL,
                    policy.clone(),
                ),
                SecurityPolicyVersion::new(revision, SecurityEpoch::INITIAL, policy),
            ],
        )
        .unwrap_or_else(|_| unreachable!("complete policy history"));
        Fixture {
            context,
            policies,
            history_space,
            layer,
            selector,
        }
    }

    fn rule(
        tail: u8,
        principal: PrincipalId,
        capability: Capability,
        effect: GrantEffect,
        scope: PolicyScope,
    ) -> CapabilityRule {
        CapabilityRule::new(
            id::<PolicyRuleId>(tail),
            PolicySubject::Principal(principal),
            CapabilityGrant::new(capability, effect),
            scope,
        )
    }

    fn document(
        fixture: &Fixture,
        tail: u8,
        text: &str,
    ) -> Result<SearchDocument, QuerySearchError> {
        SearchDocument::new(
            RecordRef::Assertion(id::<AssertionId>(tail)),
            fixture.history_space,
            fixture.layer,
            vec![SearchTextField::new(fixture.selector, text)],
        )
    }

    fn spec(fixture: &Fixture) -> Result<SearchSpec, QuerySearchError> {
        SearchSpec::new(
            vec![fixture.selector],
            vec![SearchToken::new("WorldDB")?, SearchToken::new("worlddb")?],
            SearchMatch::AllTerms,
        )
    }

    #[test]
    fn token_search_is_exact_typed_and_excludes_hidden_candidates_before_budgets()
    -> Result<(), QuerySearchError> {
        let hidden = RecordRef::Assertion(id::<AssertionId>(7));
        let fixture = fixture(Some(hidden), 10);
        let documents = vec![
            document(&fixture, 6, "worlddb appears twice: WorldDB WorldDB")?,
            document(&fixture, 7, "WorldDB secret token")?,
        ];
        let mut request = spec(&fixture)?;
        request.terms = vec![SearchToken::new("WorldDB")?];
        let visible_documents = vec![document(
            &fixture,
            6,
            "worlddb appears twice: WorldDB WorldDB",
        )?];
        let paired = PairedWorld::new((&documents, &visible_documents), false, true);
        paired
            .compare(|(all_documents, visible_only), include_hidden| {
                let selected = if *include_hidden {
                    *all_documents
                } else {
                    *visible_only
                };
                match full_scan_token_search(
                    selected,
                    &request,
                    &[fixture.selector],
                    &fixture.context,
                    &fixture.policies,
                ) {
                    Ok(result) => PublicObservation::success(
                        result
                            .value()
                            .iter()
                            .map(|hit| hit.result_key())
                            .collect::<Vec<_>>(),
                        vec!["hits".to_owned()],
                        CursorObservation::Absent,
                    ),
                    Err(error) => PublicObservation::failure(
                        PublicFailure::new(error.to_string(), vec!["code".to_owned()]),
                        vec!["error".to_owned()],
                        CursorObservation::Absent,
                    ),
                }
            })
            .map_err(|_| QuerySearchError::DuplicateResultKey)?;
        let result = full_scan_token_search(
            &documents,
            &request,
            &[fixture.selector],
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(result.value().len(), 1);
        let hit = result
            .value()
            .first()
            .ok_or(QuerySearchError::DuplicateResultKey)?;
        assert_eq!(hit.result_key(), RecordRef::Assertion(id::<AssertionId>(6)));
        assert_eq!(hit.matched_fields(), &[fixture.selector]);
        assert_eq!(result.binding(), fixture.context.schema_binding());
        Ok(())
    }

    #[test]
    fn search_returns_no_partial_result_on_budget_or_cancellation() -> Result<(), QuerySearchError>
    {
        let fixture = fixture(None, 1);
        let documents = vec![
            document(&fixture, 8, "needle")?,
            document(&fixture, 9, "needle")?,
        ];
        let request = SearchSpec::new(
            vec![fixture.selector],
            vec![SearchToken::new("needle")?],
            SearchMatch::AnyTerm,
        )?;
        assert!(matches!(
            full_scan_token_search(
                &documents,
                &request,
                &[fixture.selector],
                &fixture.context,
                &fixture.policies,
            ),
            Err(QuerySearchError::BudgetExceeded)
        ));
        fixture.context.cancellation().cancel();
        assert!(matches!(
            full_scan_token_search(
                &documents,
                &request,
                &[fixture.selector],
                &fixture.context,
                &fixture.policies,
            ),
            Err(QuerySearchError::Cancelled)
        ));
        Ok(())
    }

    #[test]
    fn search_rejects_invalid_terms_and_nontext_schema_fields() -> Result<(), QuerySearchError> {
        let fixture = fixture(None, 10);
        assert_eq!(
            SearchToken::new("two words"),
            Err(QuerySearchError::InvalidToken)
        );
        let request = SearchSpec::new(
            vec![fixture.selector],
            vec![SearchToken::new("needle")?],
            SearchMatch::AnyTerm,
        )?;
        assert!(matches!(
            full_scan_token_search(&[], &request, &[], &fixture.context, &fixture.policies,),
            Err(QuerySearchError::FieldNotTextValued)
        ));
        Ok(())
    }
}
