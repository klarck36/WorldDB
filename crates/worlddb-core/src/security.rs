//! Typed security principals, roles, and explicit capability bundles.
//!
//! This module defines policy vocabulary, bootstrap role bundles, and an
//! immutable policy evaluator. Authentication remains a host responsibility;
//! the engine supplies its authenticated principal and resource coordinates.

use std::collections::BTreeSet;
use std::fmt;

use crate::ids::{
    EventAttributeId, EventKindId, EventRoleId, HistorySpaceId, LayerId, PolicyRuleId, PredicateId,
    PrincipalId, Revision, RoleAssignmentId, RoleId, SecurityEpoch,
};
use crate::query_context::{AuthorizationMode, QueryContext};
use crate::record_refs::RecordRef;

/// Current lifecycle state of an authenticated project principal.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrincipalState {
    /// The host may authenticate this principal for project operations.
    Active,
    /// Authentication is retained, but project operations are denied.
    Disabled,
    /// Terminal state; this principal can never be reactivated.
    Retired,
}

/// A security principal registered from an identity authenticated by the host.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Principal {
    id: PrincipalId,
    state: PrincipalState,
}

impl Principal {
    /// Registers an active principal identity.
    #[must_use]
    pub const fn new(id: PrincipalId) -> Self {
        Self {
            id,
            state: PrincipalState::Active,
        }
    }

    /// Stable security identity.
    #[must_use]
    pub const fn id(self) -> PrincipalId {
        self.id
    }

    /// Current append-only lifecycle state.
    #[must_use]
    pub const fn state(self) -> PrincipalState {
        self.state
    }

    /// Returns a new state projection; it does not mutate historical records.
    #[must_use]
    pub const fn with_state(self, state: PrincipalState) -> Self {
        Self { state, ..self }
    }
}

/// A closed capability catalog. Each variant names one independently
/// grantable permission; variants do not imply one another.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    ProjectRead,
    SchemaRead,
    SchemaManage,
    HistorySpaceRead,
    HistorySpaceCreate,
    HistorySpaceTransfer,
    LayerRead,
    LayerWrite,
    LayerManage,
    AssertionRead,
    AssertionCreate,
    AssertionCorrect,
    AssertionRetract,
    EventRead,
    EventCreate,
    EventCorrect,
    EventRetract,
    EventSpanClose,
    MaskRead,
    MaskCreate,
    MaskRetract,
    EventMaskRead,
    EventMaskCreate,
    EventMaskRetract,
    ReplacementBoundaryRead,
    ReplacementBoundaryCreate,
    ReplacementBoundaryRetract,
    LifecycleRead,
    SourceRead,
    SourceCreate,
    SourceSupersede,
    EvidenceRead,
    EvidenceCreate,
    EvidenceRetract,
    ProvenanceRead,
    ProvenanceCreate,
    ProvenanceRetract,
    Archive,
    Unarchive,
    EntityCreate,
    EntityRead,
    EntityReference,
    EntityRetire,
    PerspectiveCreate,
    PerspectiveRead,
    PerspectiveUpdate,
    PerspectiveUse,
    PerspectiveRetire,
    FieldRead,
    FieldWrite,
    RelationshipRead,
    RelationshipCreate,
    RelationshipRetract,
    QueryResolve,
    QuerySearch,
    QueryFullText,
    QueryExplain,
    QueryGraphTraverse,
    QueryAggregate,
    RawHistoryRead,
    AdminRawRead,
    MigrationPlan,
    MigrationExecute,
    DataImport,
    DataExport,
    BackupCreate,
    BackupRestore,
    Purge,
    JobRead,
    JobCancel,
    JobManage,
    SecurityPolicyRead,
    SecurityPolicyManage,
    SecurityPermissionHistoryRead,
    AuditRead,
    AuditExport,
    AuditConfigure,
}

impl Capability {
    /// Closed catalog in canonical order, used to fingerprint effective rights.
    pub const ALL: [Self; 77] = [
        Self::ProjectRead,
        Self::SchemaRead,
        Self::SchemaManage,
        Self::HistorySpaceRead,
        Self::HistorySpaceCreate,
        Self::HistorySpaceTransfer,
        Self::LayerRead,
        Self::LayerWrite,
        Self::LayerManage,
        Self::AssertionRead,
        Self::AssertionCreate,
        Self::AssertionCorrect,
        Self::AssertionRetract,
        Self::EventRead,
        Self::EventCreate,
        Self::EventCorrect,
        Self::EventRetract,
        Self::EventSpanClose,
        Self::MaskRead,
        Self::MaskCreate,
        Self::MaskRetract,
        Self::EventMaskRead,
        Self::EventMaskCreate,
        Self::EventMaskRetract,
        Self::ReplacementBoundaryRead,
        Self::ReplacementBoundaryCreate,
        Self::ReplacementBoundaryRetract,
        Self::LifecycleRead,
        Self::SourceRead,
        Self::SourceCreate,
        Self::SourceSupersede,
        Self::EvidenceRead,
        Self::EvidenceCreate,
        Self::EvidenceRetract,
        Self::ProvenanceRead,
        Self::ProvenanceCreate,
        Self::ProvenanceRetract,
        Self::Archive,
        Self::Unarchive,
        Self::EntityCreate,
        Self::EntityRead,
        Self::EntityReference,
        Self::EntityRetire,
        Self::PerspectiveCreate,
        Self::PerspectiveRead,
        Self::PerspectiveUpdate,
        Self::PerspectiveUse,
        Self::PerspectiveRetire,
        Self::FieldRead,
        Self::FieldWrite,
        Self::RelationshipRead,
        Self::RelationshipCreate,
        Self::RelationshipRetract,
        Self::QueryResolve,
        Self::QuerySearch,
        Self::QueryFullText,
        Self::QueryExplain,
        Self::QueryGraphTraverse,
        Self::QueryAggregate,
        Self::RawHistoryRead,
        Self::AdminRawRead,
        Self::MigrationPlan,
        Self::MigrationExecute,
        Self::DataImport,
        Self::DataExport,
        Self::BackupCreate,
        Self::BackupRestore,
        Self::Purge,
        Self::JobRead,
        Self::JobCancel,
        Self::JobManage,
        Self::SecurityPolicyRead,
        Self::SecurityPolicyManage,
        Self::SecurityPermissionHistoryRead,
        Self::AuditRead,
        Self::AuditExport,
        Self::AuditConfigure,
    ];
}

/// Allow or deny effect attached to one explicit capability rule.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GrantEffect {
    /// This rule contributes an allow; a matching deny still takes precedence.
    Allow,
    /// This rule explicitly denies; absence of an allow also denies.
    Deny,
}

/// One explicit capability rule in a role policy bundle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CapabilityGrant {
    capability: Capability,
    effect: GrantEffect,
}

impl CapabilityGrant {
    /// Creates an explicit rule. Scope binding is added by the policy engine.
    #[must_use]
    pub const fn new(capability: Capability, effect: GrantEffect) -> Self {
        Self { capability, effect }
    }

    /// Capability named by this rule.
    #[must_use]
    pub const fn capability(self) -> Capability {
        self.capability
    }

    /// Effect named by this rule.
    #[must_use]
    pub const fn effect(self) -> GrantEffect {
        self.effect
    }
}

/// Typed principal or role to which a capability rule applies.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PolicySubject {
    /// Direct rule for one authenticated principal.
    Principal(PrincipalId),
    /// Rule attached to one explicitly registered role.
    Role(RoleId),
}

/// Typed schema/data field classes available for field-level policy rules.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FieldSelector {
    AssertionSubject,
    AssertionPredicate,
    AssertionValue(PredicateId),
    AssertionPolarity,
    AssertionValidity,
    AssertionPerspective,
    AssertionEpistemicMode,
    MaskSelector,
    MaskValidity,
    ReplacementBoundarySubject,
    ReplacementBoundaryPredicate,
    ReplacementBoundaryValidity,
    EventKind,
    EventParticipant(EventKindId, EventRoleId),
    EventAttribute(EventKindId, EventAttributeId),
    EventTime(EventKindId),
    EventMaskTarget,
    SourceKind,
    SourceLocator,
    SourceContentDigest,
    SourceMetadata,
}

/// Typed relationship families available for edge-level policy rules.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RelationshipSelector {
    EventRelation(PolicyEventRelationKind),
    Evidence(EvidenceRelationship),
    Provenance(ProvenanceRelationship),
    LifecycleTarget,
}

/// Closed event relationship selector.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PolicyEventRelationKind {
    Before,
    SameTime,
    Causes,
}

/// Closed evidence relationship selector.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceRelationship {
    Supports,
    Contradicts,
    Documents,
}

/// Closed provenance relationship selector.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProvenanceRelationship {
    Corrects,
    DerivedFrom,
    ResultedFrom,
}

/// Exact conjunctive policy scope. Empty scope is project scope; populated
/// dimensions must all match the target under the security evaluator.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PolicyScope {
    history_space: Option<HistorySpaceId>,
    layer: Option<LayerId>,
    record: Option<RecordRef>,
    field: Option<FieldSelector>,
    relationship: Option<RelationshipSelector>,
}

impl PolicyScope {
    /// Explicit project-wide scope (no dimensions are inferred).
    #[must_use]
    pub const fn project() -> Self {
        Self {
            history_space: None,
            layer: None,
            record: None,
            field: None,
            relationship: None,
        }
    }

    /// Creates a scope over selected exact dimensions; all selectors are ANDed.
    #[must_use]
    pub const fn new(
        history_space: Option<HistorySpaceId>,
        layer: Option<LayerId>,
        record: Option<RecordRef>,
        field: Option<FieldSelector>,
        relationship: Option<RelationshipSelector>,
    ) -> Self {
        Self {
            history_space,
            layer,
            record,
            field,
            relationship,
        }
    }

    /// HistorySpace dimension.
    #[must_use]
    pub const fn history_space(self) -> Option<HistorySpaceId> {
        self.history_space
    }
    /// Layer dimension.
    #[must_use]
    pub const fn layer(self) -> Option<LayerId> {
        self.layer
    }
    /// Record dimension.
    #[must_use]
    pub const fn record(self) -> Option<RecordRef> {
        self.record
    }
    /// Field dimension.
    #[must_use]
    pub const fn field(self) -> Option<FieldSelector> {
        self.field
    }
    /// Relationship dimension.
    #[must_use]
    pub const fn relationship(self) -> Option<RelationshipSelector> {
        self.relationship
    }

    /// Returns whether every populated selector exactly matches the target.
    #[must_use]
    pub fn matches(self, target: PolicyTarget) -> bool {
        dimension_matches(self.history_space, target.history_space)
            && dimension_matches(self.layer, target.layer)
            && dimension_matches(self.record, target.record)
            && dimension_matches(self.field, target.field)
            && dimension_matches(self.relationship, target.relationship)
    }
}

fn dimension_matches<T: Eq>(scope: Option<T>, target: Option<T>) -> bool {
    match (scope, target) {
        (None, _) => true,
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => false,
    }
}

/// Exact typed resource coordinates against which policy scope is evaluated.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PolicyTarget {
    history_space: Option<HistorySpaceId>,
    layer: Option<LayerId>,
    record: Option<RecordRef>,
    field: Option<FieldSelector>,
    relationship: Option<RelationshipSelector>,
}

impl PolicyTarget {
    /// Creates target coordinates; selectors with no target value cannot match.
    #[must_use]
    pub const fn new(
        history_space: Option<HistorySpaceId>,
        layer: Option<LayerId>,
        record: Option<RecordRef>,
        field: Option<FieldSelector>,
        relationship: Option<RelationshipSelector>,
    ) -> Self {
        Self {
            history_space,
            layer,
            record,
            field,
            relationship,
        }
    }

    /// HistorySpace coordinate of this policy target.
    #[must_use]
    pub const fn history_space(self) -> Option<HistorySpaceId> {
        self.history_space
    }

    /// Layer coordinate of this policy target.
    #[must_use]
    pub const fn layer(self) -> Option<LayerId> {
        self.layer
    }

    /// Concrete record coordinate of this policy target.
    #[must_use]
    pub const fn record(self) -> Option<RecordRef> {
        self.record
    }

    /// Field coordinate of this policy target.
    #[must_use]
    pub const fn field(self) -> Option<FieldSelector> {
        self.field
    }

    /// Relationship coordinate of this policy target.
    #[must_use]
    pub const fn relationship(self) -> Option<RelationshipSelector> {
        self.relationship
    }
}

/// Durable typed capability rule with explicit subject, effect, and scope.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CapabilityRule {
    id: PolicyRuleId,
    subject: PolicySubject,
    grant: CapabilityGrant,
    scope: PolicyScope,
}

impl CapabilityRule {
    /// Creates a new immutable rule value.
    #[must_use]
    pub const fn new(
        id: PolicyRuleId,
        subject: PolicySubject,
        grant: CapabilityGrant,
        scope: PolicyScope,
    ) -> Self {
        Self {
            id,
            subject,
            grant,
            scope,
        }
    }

    /// Stable identity of this policy rule.
    #[must_use]
    pub const fn id(self) -> PolicyRuleId {
        self.id
    }
    /// Rule subject.
    #[must_use]
    pub const fn subject(self) -> PolicySubject {
        self.subject
    }
    /// Exact typed capability and effect.
    #[must_use]
    pub const fn grant(self) -> CapabilityGrant {
        self.grant
    }
    /// Exact conjunctive scope.
    #[must_use]
    pub const fn scope(self) -> PolicyScope {
        self.scope
    }
}

/// A sorted, duplicate-free set of explicit capability rules.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicyBundle {
    grants: BTreeSet<(Capability, GrantEffect)>,
}

impl PolicyBundle {
    /// Creates an empty bundle. No grant means deny.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a bundle from explicit rules, rejecting duplicate pairs.
    pub fn from_grants(
        grants: impl IntoIterator<Item = CapabilityGrant>,
    ) -> Result<Self, PolicyBundleError> {
        let mut bundle = Self::new();
        for grant in grants {
            if !bundle.grants.insert((grant.capability, grant.effect)) {
                return Err(PolicyBundleError::DuplicateRule {
                    capability: grant.capability,
                    effect: grant.effect,
                });
            }
        }
        Ok(bundle)
    }

    /// Builds the explicit initial GM bundle defined by the project workflow.
    /// The role symbol itself has no policy meaning.
    #[must_use]
    pub fn standard_gm() -> Self {
        Self::from_capabilities([
            Capability::ProjectRead,
            Capability::SchemaRead,
            Capability::SchemaManage,
            Capability::HistorySpaceRead,
            Capability::HistorySpaceCreate,
            Capability::HistorySpaceTransfer,
            Capability::LayerRead,
            Capability::LayerWrite,
            Capability::LayerManage,
            Capability::EntityCreate,
            Capability::EntityRead,
            Capability::EntityReference,
            Capability::EntityRetire,
            Capability::PerspectiveCreate,
            Capability::PerspectiveRead,
            Capability::PerspectiveUpdate,
            Capability::PerspectiveUse,
            Capability::PerspectiveRetire,
            Capability::AssertionRead,
            Capability::AssertionCreate,
            Capability::AssertionCorrect,
            Capability::AssertionRetract,
            Capability::EventRead,
            Capability::EventCreate,
            Capability::EventCorrect,
            Capability::EventRetract,
            Capability::EventSpanClose,
            Capability::MaskRead,
            Capability::MaskCreate,
            Capability::MaskRetract,
            Capability::EventMaskRead,
            Capability::EventMaskCreate,
            Capability::EventMaskRetract,
            Capability::ReplacementBoundaryRead,
            Capability::ReplacementBoundaryCreate,
            Capability::ReplacementBoundaryRetract,
            Capability::EvidenceRead,
            Capability::EvidenceCreate,
            Capability::EvidenceRetract,
            Capability::ProvenanceRead,
            Capability::ProvenanceCreate,
            Capability::ProvenanceRetract,
            Capability::LifecycleRead,
            Capability::Archive,
            Capability::Unarchive,
            Capability::SourceRead,
            Capability::SourceCreate,
            Capability::SourceSupersede,
            Capability::FieldRead,
            Capability::FieldWrite,
            Capability::RelationshipRead,
            Capability::RelationshipCreate,
            Capability::RelationshipRetract,
            Capability::QueryResolve,
            Capability::QuerySearch,
            Capability::QueryFullText,
            Capability::QueryExplain,
            Capability::QueryGraphTraverse,
            Capability::QueryAggregate,
            Capability::RawHistoryRead,
            Capability::SecurityPolicyRead,
            Capability::SecurityPolicyManage,
            Capability::SecurityPermissionHistoryRead,
            Capability::AuditRead,
            Capability::BackupCreate,
            Capability::BackupRestore,
            Capability::JobRead,
            Capability::JobCancel,
        ])
    }

    /// Builds the initial unprivileged Player bundle. No grants means deny.
    #[must_use]
    pub fn standard_player() -> Self {
        Self::new()
    }

    fn from_capabilities(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            grants: capabilities
                .into_iter()
                .map(|capability| (capability, GrantEffect::Allow))
                .collect(),
        }
    }

    /// Returns whether this bundle contains the exact capability/effect pair.
    #[must_use]
    pub fn contains(&self, capability: Capability, effect: GrantEffect) -> bool {
        self.grants.contains(&(capability, effect))
    }

    /// Returns the number of explicit rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    /// Returns whether the bundle has no explicit rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// Iterates over rules in a stable order.
    pub fn iter(&self) -> impl Iterator<Item = CapabilityGrant> + '_ {
        self.grants
            .iter()
            .map(|(capability, effect)| CapabilityGrant::new(*capability, *effect))
    }
}

/// A registered role's stable security identity and explicit policy bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleDefinition {
    id: RoleId,
    symbol: String,
    bundle: PolicyBundle,
}

impl RoleDefinition {
    /// Registers a role symbol using `[a-z][a-z0-9_]*`.
    pub fn new(
        id: RoleId,
        symbol: impl Into<String>,
        bundle: PolicyBundle,
    ) -> Result<Self, RoleDefinitionError> {
        let symbol = symbol.into();
        if !valid_role_symbol(&symbol) {
            return Err(RoleDefinitionError::InvalidSymbol);
        }
        Ok(Self { id, symbol, bundle })
    }

    /// Stable role identity.
    #[must_use]
    pub const fn id(&self) -> RoleId {
        self.id
    }

    /// Canonical lowercase role symbol.
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Explicit rules; the evaluator never grants rights from the symbol.
    #[must_use]
    pub const fn bundle(&self) -> &PolicyBundle {
        &self.bundle
    }
}

/// Errors while constructing an explicit role policy bundle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PolicyBundleError {
    /// The same effect for a capability appears more than once.
    DuplicateRule {
        /// Duplicated permission.
        capability: Capability,
        /// Duplicated effect.
        effect: GrantEffect,
    },
}

impl fmt::Display for PolicyBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateRule { capability, effect } => {
                write!(formatter, "duplicate {effect:?} rule for {capability:?}")
            }
        }
    }
}

impl std::error::Error for PolicyBundleError {}

/// Errors while constructing a role definition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RoleDefinitionError {
    /// Role symbols must match `[a-z][a-z0-9_]*`.
    InvalidSymbol,
}

impl fmt::Display for RoleDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSymbol => formatter.write_str("invalid role symbol"),
        }
    }
}

impl std::error::Error for RoleDefinitionError {}

/// Result of evaluating one typed capability request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuthorizationDecision {
    /// At least one matching Allow exists and no matching Deny exists.
    Allow,
    /// Principal is inactive, a Deny matched, or no Allow matched.
    Deny,
}

/// Immutable policy projection used at a query/read authorization boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SecurityPolicySnapshot {
    principals: Vec<Principal>,
    roles: Vec<RoleDefinition>,
    assignments: Vec<RoleAssignment>,
    rules: Vec<CapabilityRule>,
}

impl SecurityPolicySnapshot {
    /// Validates and freezes one set of active principal, role, assignment, and rule values.
    pub fn new(
        principals: Vec<Principal>,
        roles: Vec<RoleDefinition>,
        assignments: Vec<RoleAssignment>,
        rules: Vec<CapabilityRule>,
    ) -> Result<Self, SecurityPolicyError> {
        let principal_ids = principals
            .iter()
            .map(|principal| principal.id)
            .collect::<BTreeSet<_>>();
        if principal_ids.len() != principals.len() {
            return Err(SecurityPolicyError::DuplicatePrincipal);
        }
        let role_ids = roles
            .iter()
            .map(RoleDefinition::id)
            .collect::<BTreeSet<_>>();
        if role_ids.len() != roles.len() {
            return Err(SecurityPolicyError::DuplicateRole);
        }
        let mut role_symbols = BTreeSet::new();
        if roles
            .iter()
            .any(|role| !role_symbols.insert(role.symbol.clone()))
        {
            return Err(SecurityPolicyError::DuplicateRoleSymbol);
        }
        let assignment_ids = assignments
            .iter()
            .map(|assignment| assignment.id)
            .collect::<BTreeSet<_>>();
        if assignment_ids.len() != assignments.len() {
            return Err(SecurityPolicyError::DuplicateAssignment);
        }
        if assignments.iter().any(|assignment| {
            !principal_ids.contains(&assignment.principal) || !role_ids.contains(&assignment.role)
        }) {
            return Err(SecurityPolicyError::UnknownAssignmentSubject);
        }
        let rule_ids = rules.iter().map(|rule| rule.id).collect::<BTreeSet<_>>();
        if rule_ids.len() != rules.len() {
            return Err(SecurityPolicyError::DuplicateRule);
        }
        if rules.iter().any(|rule| match rule.subject {
            PolicySubject::Principal(principal) => !principal_ids.contains(&principal),
            PolicySubject::Role(role) => !role_ids.contains(&role),
        }) {
            return Err(SecurityPolicyError::UnknownRuleSubject);
        }
        Ok(Self {
            principals,
            roles,
            assignments,
            rules,
        })
    }

    /// Complete active and terminal principal states in this immutable snapshot.
    #[must_use]
    pub fn principals(&self) -> &[Principal] {
        &self.principals
    }

    /// Complete role definitions and their explicit capability bundles.
    #[must_use]
    pub fn roles(&self) -> &[RoleDefinition] {
        &self.roles
    }

    /// Complete principal-to-role assignments with their exact scopes.
    #[must_use]
    pub fn assignments(&self) -> &[RoleAssignment] {
        &self.assignments
    }

    /// Complete direct and role-subject capability rules.
    #[must_use]
    pub fn rules(&self) -> &[CapabilityRule] {
        &self.rules
    }

    /// Evaluates one exact capability and resource request. Matching Deny
    /// rules across direct and assigned roles always override every Allow.
    #[must_use]
    pub fn authorize(
        &self,
        principal_id: PrincipalId,
        capability: Capability,
        target: PolicyTarget,
    ) -> AuthorizationDecision {
        let Some(principal) = self.principals.iter().find(|item| item.id == principal_id) else {
            return AuthorizationDecision::Deny;
        };
        if principal.state != PrincipalState::Active {
            return AuthorizationDecision::Deny;
        }

        let assigned_roles = self
            .assignments
            .iter()
            .filter(|assignment| {
                assignment.principal == principal_id && assignment.scope.matches(target)
            })
            .map(|assignment| assignment.role)
            .collect::<BTreeSet<_>>();
        let mut allowed = false;
        for role in self
            .roles
            .iter()
            .filter(|role| assigned_roles.contains(&role.id))
        {
            for grant in role
                .bundle
                .iter()
                .filter(|grant| grant.capability == capability)
            {
                match grant.effect {
                    GrantEffect::Allow => allowed = true,
                    GrantEffect::Deny => return AuthorizationDecision::Deny,
                }
            }
        }
        for rule in self.rules.iter().filter(|rule| {
            rule.grant.capability == capability
                && rule.scope.matches(target)
                && match rule.subject {
                    PolicySubject::Principal(id) => id == principal_id,
                    PolicySubject::Role(id) => assigned_roles.contains(&id),
                }
        }) {
            match rule.grant.effect {
                GrantEffect::Allow => allowed = true,
                GrantEffect::Deny => return AuthorizationDecision::Deny,
            }
        }
        if allowed {
            AuthorizationDecision::Allow
        } else {
            AuthorizationDecision::Deny
        }
    }

    /// Opaque fingerprint of every capability effectively allowed for this principal and target.
    /// The digest is retained only in process-local cursor state, never placed on the wire.
    #[must_use]
    pub fn effective_capability_fingerprint(
        &self,
        principal_id: PrincipalId,
        target: PolicyTarget,
    ) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        for capability in Capability::ALL {
            hasher.update(&[u8::from(
                self.authorize(principal_id, capability, target) == AuthorizationDecision::Allow,
            )]);
        }
        *hasher.finalize().as_bytes()
    }
}

/// One immutable policy projection at the commit that changed security policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityPolicyVersion {
    revision: Revision,
    epoch: SecurityEpoch,
    snapshot: SecurityPolicySnapshot,
}

impl SecurityPolicyVersion {
    /// Binds a validated policy projection to its committed revision and epoch.
    #[must_use]
    pub const fn new(
        revision: Revision,
        epoch: SecurityEpoch,
        snapshot: SecurityPolicySnapshot,
    ) -> Self {
        Self {
            revision,
            epoch,
            snapshot,
        }
    }

    /// Revision at which this policy became effective.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Epoch identifying this policy projection.
    #[must_use]
    pub const fn epoch(&self) -> SecurityEpoch {
        self.epoch
    }

    /// Policy snapshot that became effective at this version.
    #[must_use]
    pub const fn snapshot(&self) -> &SecurityPolicySnapshot {
        &self.snapshot
    }
}

/// Complete, immutable policy projection for every revision through one committed revision.
#[derive(Clone, Debug)]
pub struct SecurityPolicyHistory {
    committed_revision: Revision,
    versions: Vec<SecurityPolicyVersion>,
}

impl SecurityPolicyHistory {
    /// Validates a complete append-only policy history from the initial revision.
    pub fn new(
        committed_revision: Revision,
        versions: Vec<SecurityPolicyVersion>,
    ) -> Result<Self, SecurityPolicyHistoryError> {
        if versions.first().is_none_or(|version| {
            version.revision != Revision::GENESIS || version.epoch != SecurityEpoch::INITIAL
        }) {
            return Err(SecurityPolicyHistoryError::MissingInitialSnapshot);
        }
        if versions
            .last()
            .is_none_or(|version| version.revision != committed_revision)
        {
            return Err(SecurityPolicyHistoryError::IncompleteThroughCommittedRevision);
        }
        for (previous, current) in versions.iter().zip(versions.iter().skip(1)) {
            if previous.revision.value().checked_add(1) != Some(current.revision.value()) {
                return Err(SecurityPolicyHistoryError::IncompleteRevisionCoverage);
            }
            if current.epoch != previous.epoch && previous.epoch.next().ok() != Some(current.epoch)
            {
                return Err(SecurityPolicyHistoryError::NonConsecutiveEpoch);
            }
        }
        Ok(Self {
            committed_revision,
            versions,
        })
    }

    /// Current committed data revision through which this policy history is complete.
    #[must_use]
    pub const fn committed_revision(&self) -> Revision {
        self.committed_revision
    }

    /// Complete policy snapshots, with exactly one version per committed revision.
    #[must_use]
    pub fn versions(&self) -> &[SecurityPolicyVersion] {
        &self.versions
    }

    /// Policy version at one committed revision; a missing version fails closed.
    pub fn version_at(
        &self,
        revision: Revision,
    ) -> Result<&SecurityPolicyVersion, SecurityPolicyHistoryError> {
        if revision > self.committed_revision {
            return Err(SecurityPolicyHistoryError::RevisionAfterCommittedRevision);
        }
        let index = usize::try_from(revision.value())
            .map_err(|_| SecurityPolicyHistoryError::MissingInitialSnapshot)?;
        self.versions
            .get(index)
            .filter(|version| version.revision == revision)
            .ok_or(SecurityPolicyHistoryError::MissingInitialSnapshot)
    }

    /// Appends the security-policy projection for the next shared data revision.
    ///
    /// A data-only commit carries forward the prior snapshot and epoch. A
    /// policy-changing commit advances the epoch exactly once.
    pub fn append_revision(
        &self,
        version: SecurityPolicyVersion,
    ) -> Result<Self, SecurityPolicyHistoryError> {
        let revision = self
            .committed_revision
            .next_commit()
            .map_err(SecurityPolicyHistoryError::RevisionExhausted)?;
        if revision != version.revision {
            return Err(SecurityPolicyHistoryError::IncompleteRevisionCoverage);
        }
        let previous_epoch = self.latest_version()?.epoch;
        let epoch_unchanged = version.epoch == previous_epoch;
        let epoch_advanced = previous_epoch.next().ok() == Some(version.epoch);
        if !epoch_unchanged && !epoch_advanced {
            return Err(SecurityPolicyHistoryError::NonConsecutiveEpoch);
        }
        let mut versions = self.versions.clone();
        versions.push(version);
        Self::new(revision, versions)
    }

    /// Latest complete policy version.
    pub fn latest_version(&self) -> Result<&SecurityPolicyVersion, SecurityPolicyHistoryError> {
        self.versions
            .last()
            .ok_or(SecurityPolicyHistoryError::MissingInitialSnapshot)
    }

    /// Returns an append-only candidate after exactly one policy-changing commit.
    pub fn append_policy_change(
        &self,
        version: SecurityPolicyVersion,
    ) -> Result<Self, SecurityPolicyHistoryError> {
        let previous_epoch = self.latest_version()?.epoch;
        if previous_epoch.next().ok() != Some(version.epoch) {
            return Err(SecurityPolicyHistoryError::NonConsecutiveEpoch);
        }
        self.append_revision(version)
    }

    /// Returns current and query-selected policy views, enforcing the administrative gate.
    pub fn resolve<'a>(
        &'a self,
        context: &QueryContext,
    ) -> Result<SecurityPolicyView<'a>, SecurityPolicyHistoryError> {
        self.select(
            context.security().authorization_mode(),
            context.security().principal_id(),
            context.snapshot_revision(),
        )
    }

    /// Selects a policy view with the same gate used by a complete query context.
    pub fn select(
        &self,
        mode: AuthorizationMode,
        principal: PrincipalId,
        query_snapshot_revision: Revision,
    ) -> Result<SecurityPolicyView<'_>, SecurityPolicyHistoryError> {
        if query_snapshot_revision > self.committed_revision {
            return Err(SecurityPolicyHistoryError::RevisionAfterCommittedRevision);
        }
        let current = self
            .versions
            .last()
            .ok_or(SecurityPolicyHistoryError::MissingInitialSnapshot)?;
        let current_policy_target = PolicyTarget::new(None, None, None, None, None);
        match mode {
            AuthorizationMode::Now => Ok(SecurityPolicyView {
                current,
                evaluated: current,
                principal_id: principal,
            }),
            AuthorizationMode::AtRevision(revision) => {
                if revision > query_snapshot_revision {
                    return Err(SecurityPolicyHistoryError::RevisionAfterCommittedRevision);
                }
                if current.snapshot.authorize(
                    principal,
                    Capability::SecurityPermissionHistoryRead,
                    current_policy_target,
                ) != AuthorizationDecision::Allow
                {
                    return Err(SecurityPolicyHistoryError::HistoricalPermissionDenied);
                }
                let evaluated = self
                    .versions
                    .get(
                        usize::try_from(revision.value())
                            .map_err(|_| SecurityPolicyHistoryError::MissingInitialSnapshot)?,
                    )
                    .ok_or(SecurityPolicyHistoryError::MissingInitialSnapshot)?;
                Ok(SecurityPolicyView {
                    current,
                    evaluated,
                    principal_id: principal,
                })
            }
        }
    }
}

/// Current policy gate and the policy used for this query.
#[derive(Clone, Copy, Debug)]
pub struct SecurityPolicyView<'a> {
    current: &'a SecurityPolicyVersion,
    evaluated: &'a SecurityPolicyVersion,
    principal_id: PrincipalId,
}

impl<'a> SecurityPolicyView<'a> {
    /// Principal for which historical access and policy selection were authorized.
    #[must_use]
    pub const fn principal_id(self) -> PrincipalId {
        self.principal_id
    }

    /// Always-current policy projection used to reauthorize continuations.
    #[must_use]
    pub const fn current_snapshot(self) -> &'a SecurityPolicySnapshot {
        &self.current.snapshot
    }

    /// Policy snapshot selected by AuthorizationNow or AuthorizationAtRevision.
    #[must_use]
    pub const fn snapshot(self) -> &'a SecurityPolicySnapshot {
        &self.evaluated.snapshot
    }

    /// Epoch checked for current authorization and cursor invalidation.
    #[must_use]
    pub const fn current_epoch(self) -> SecurityEpoch {
        self.current.epoch
    }

    /// Epoch of the policy snapshot actually used for evaluation.
    #[must_use]
    pub const fn evaluated_epoch(self) -> SecurityEpoch {
        self.evaluated.epoch
    }
}

/// Invalid, incomplete, or unauthorized historical policy selection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SecurityPolicyHistoryError {
    /// No complete policy projection exists at the genesis revision.
    MissingInitialSnapshot,
    /// Policy history does not extend through the declared committed revision.
    IncompleteThroughCommittedRevision,
    /// A committed revision projection is missing from the supplied history.
    IncompleteRevisionCoverage,
    /// A requested security revision has not been committed.
    RevisionAfterCommittedRevision,
    /// Shared revision space cannot advance for another policy commit.
    RevisionExhausted(crate::ids::RevisionError),
    /// Every policy-changing commit must advance the epoch exactly once.
    NonConsecutiveEpoch,
    /// The current policy does not grant historical permission inspection.
    HistoricalPermissionDenied,
}

impl fmt::Display for SecurityPolicyHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingInitialSnapshot => "security policy history has no initial snapshot",
            Self::IncompleteThroughCommittedRevision => {
                "security policy history ends before committed revision"
            }
            Self::IncompleteRevisionCoverage => {
                "security policy history omits a committed revision"
            }
            Self::RevisionAfterCommittedRevision => "requested security revision is not committed",
            Self::RevisionExhausted(_) => "shared revision space is exhausted",
            Self::NonConsecutiveEpoch => "security policy epoch did not advance exactly once",
            Self::HistoricalPermissionDenied => {
                "current policy denies historical permission access"
            }
        })
    }
}

impl std::error::Error for SecurityPolicyHistoryError {}

/// Invalid references or duplicate identities in an immutable policy snapshot.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SecurityPolicyError {
    /// Principal identity occurs more than once.
    DuplicatePrincipal,
    /// Role identity occurs more than once.
    DuplicateRole,
    /// Role symbol occurs more than once.
    DuplicateRoleSymbol,
    /// Assignment identity occurs more than once.
    DuplicateAssignment,
    /// Assignment references an unregistered principal or role.
    UnknownAssignmentSubject,
    /// Policy rule identity occurs more than once.
    DuplicateRule,
    /// Policy rule references an unregistered principal or role.
    UnknownRuleSubject,
}

impl fmt::Display for SecurityPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePrincipal => formatter.write_str("duplicate principal identity"),
            Self::DuplicateRole => formatter.write_str("duplicate role identity"),
            Self::DuplicateRoleSymbol => formatter.write_str("duplicate role symbol"),
            Self::DuplicateAssignment => formatter.write_str("duplicate role assignment identity"),
            Self::UnknownAssignmentSubject => {
                formatter.write_str("role assignment has unknown subject")
            }
            Self::DuplicateRule => formatter.write_str("duplicate policy rule identity"),
            Self::UnknownRuleSubject => formatter.write_str("policy rule has unknown subject"),
        }
    }
}

impl std::error::Error for SecurityPolicyError {}

fn valid_role_symbol(symbol: &str) -> bool {
    let mut bytes = symbol.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// A role assignment explicitly binds a principal to a registered role.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RoleAssignment {
    id: RoleAssignmentId,
    principal: PrincipalId,
    role: RoleId,
    scope: PolicyScope,
}

impl RoleAssignment {
    /// Creates an explicit principal-to-role assignment.
    #[must_use]
    pub const fn new(
        id: RoleAssignmentId,
        principal: PrincipalId,
        role: RoleId,
        scope: PolicyScope,
    ) -> Self {
        Self {
            id,
            principal,
            role,
            scope,
        }
    }

    /// Stable assignment identity.
    #[must_use]
    pub const fn id(&self) -> RoleAssignmentId {
        self.id
    }

    /// Assigned authenticated principal.
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    /// Assigned role identity.
    #[must_use]
    pub const fn role(&self) -> RoleId {
        self.role
    }

    /// Exact assignment scope.
    #[must_use]
    pub const fn scope(&self) -> PolicyScope {
        self.scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    enum PolicyTestError {
        Id(crate::ids::IdValidationError),
        Policy(SecurityPolicyError),
        Role(RoleDefinitionError),
        Bundle(PolicyBundleError),
        History(SecurityPolicyHistoryError),
        Revision(crate::ids::RevisionError),
    }

    impl fmt::Display for PolicyTestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(e) => write!(f, "{e}"),
                Self::Policy(e) => write!(f, "{e}"),
                Self::Role(e) => write!(f, "{e}"),
                Self::Bundle(e) => write!(f, "{e}"),
                Self::History(e) => write!(f, "{e}"),
                Self::Revision(e) => write!(f, "{e}"),
            }
        }
    }

    impl std::error::Error for PolicyTestError {}
    impl From<crate::ids::IdValidationError> for PolicyTestError {
        fn from(e: crate::ids::IdValidationError) -> Self {
            Self::Id(e)
        }
    }
    impl From<SecurityPolicyError> for PolicyTestError {
        fn from(e: SecurityPolicyError) -> Self {
            Self::Policy(e)
        }
    }
    impl From<RoleDefinitionError> for PolicyTestError {
        fn from(e: RoleDefinitionError) -> Self {
            Self::Role(e)
        }
    }
    impl From<PolicyBundleError> for PolicyTestError {
        fn from(e: PolicyBundleError) -> Self {
            Self::Bundle(e)
        }
    }
    impl From<SecurityPolicyHistoryError> for PolicyTestError {
        fn from(e: SecurityPolicyHistoryError) -> Self {
            Self::History(e)
        }
    }
    impl From<crate::ids::RevisionError> for PolicyTestError {
        fn from(e: crate::ids::RevisionError) -> Self {
            Self::Revision(e)
        }
    }

    #[test]
    fn admin_raw_read_is_independent_of_raw_history_and_role_names() {
        let gm = PolicyBundle::standard_gm();
        assert!(gm.contains(Capability::RawHistoryRead, GrantEffect::Allow));
        assert!(!gm.contains(Capability::AdminRawRead, GrantEffect::Allow));
        assert!(
            !PolicyBundle::standard_player()
                .contains(Capability::RawHistoryRead, GrantEffect::Allow)
        );
    }

    #[test]
    fn duplicate_rules_and_invalid_symbols_are_rejected() {
        use crate::ids::DomainId;

        let grant = CapabilityGrant::new(Capability::ProjectRead, GrantEffect::Allow);
        assert!(matches!(
            PolicyBundle::from_grants([grant, grant]),
            Err(PolicyBundleError::DuplicateRule { .. })
        ));
        let id = crate::ids::RoleId::try_from_bytes(test_uuid_bytes());
        assert!(id.is_ok());
        if let Ok(id) = id {
            assert_eq!(
                RoleDefinition::new(id, "GM", PolicyBundle::new()),
                Err(RoleDefinitionError::InvalidSymbol)
            );
        }
    }

    #[test]
    fn initial_roles_are_plain_symbols_with_explicit_bundles() {
        use crate::ids::DomainId;

        let gm_id = crate::ids::RoleId::try_from_bytes(test_uuid_bytes());
        let player_id = crate::ids::RoleId::try_from_bytes(test_uuid_bytes_other());
        assert!(gm_id.is_ok());
        assert!(player_id.is_ok());
        if let (Ok(gm_id), Ok(player_id)) = (gm_id, player_id) {
            let gm = RoleDefinition::new(gm_id, "gm", PolicyBundle::standard_gm());
            let player = RoleDefinition::new(player_id, "player", PolicyBundle::standard_player());
            assert!(gm.is_ok());
            assert!(player.is_ok());
            if let (Ok(gm), Ok(player)) = (gm, player) {
                assert_eq!(gm.symbol(), "gm");
                assert_eq!(gm.bundle().len(), 68);
                assert!(player.bundle().is_empty());
            }
        }
    }

    #[test]
    fn evaluator_defaults_to_deny_and_denies_override_role_allow() -> Result<(), PolicyTestError> {
        use crate::ids::DomainId;

        let principal_id = PrincipalId::try_from_bytes(uuid_bytes(6))?;
        let role_id = RoleId::try_from_bytes(uuid_bytes(7))?;
        let assignment_id = RoleAssignmentId::try_from_bytes(uuid_bytes(8))?;
        let deny_id = PolicyRuleId::try_from_bytes(uuid_bytes(9))?;
        let history_space = HistorySpaceId::try_from_bytes(uuid_bytes(10))?;
        let layer = LayerId::try_from_bytes(uuid_bytes(11))?;
        let assertion_id = crate::ids::AssertionId::try_from_bytes(uuid_bytes(12))?;
        let role = RoleDefinition::new(role_id, "gm", PolicyBundle::standard_gm())?;
        let assignment =
            RoleAssignment::new(assignment_id, principal_id, role_id, PolicyScope::project());
        let deny = CapabilityRule::new(
            deny_id,
            PolicySubject::Principal(principal_id),
            CapabilityGrant::new(Capability::ProjectRead, GrantEffect::Deny),
            PolicyScope::project(),
        );
        let policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            vec![role],
            vec![assignment],
            vec![deny],
        )?;
        let target = PolicyTarget::new(
            Some(history_space),
            Some(layer),
            Some(RecordRef::Assertion(assertion_id)),
            None,
            None,
        );

        assert_eq!(
            policy.authorize(principal_id, Capability::RawHistoryRead, target),
            AuthorizationDecision::Allow
        );
        assert_eq!(
            policy.authorize(principal_id, Capability::AdminRawRead, target),
            AuthorizationDecision::Deny
        );
        assert_eq!(
            policy.authorize(principal_id, Capability::ProjectRead, target),
            AuthorizationDecision::Deny
        );
        assert_eq!(
            policy.authorize(principal_id, Capability::DataExport, target),
            AuthorizationDecision::Deny
        );
        Ok(())
    }

    #[test]
    fn historical_policy_is_explicit_admin_gated_and_now_never_revives_rights()
    -> Result<(), PolicyTestError> {
        use crate::ids::{DomainId, Revision};

        let principal = PrincipalId::try_from_bytes(uuid_bytes(60))?;
        let old_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![],
            vec![],
            vec![CapabilityRule::new(
                PolicyRuleId::try_from_bytes(uuid_bytes(61))?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::ProjectRead, GrantEffect::Allow),
                PolicyScope::project(),
            )],
        )?;
        let current_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![],
            vec![],
            vec![
                CapabilityRule::new(
                    PolicyRuleId::try_from_bytes(uuid_bytes(62))?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(Capability::ProjectRead, GrantEffect::Deny),
                    PolicyScope::project(),
                ),
                CapabilityRule::new(
                    PolicyRuleId::try_from_bytes(uuid_bytes(63))?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(
                        Capability::SecurityPermissionHistoryRead,
                        GrantEffect::Allow,
                    ),
                    PolicyScope::project(),
                ),
            ],
        )?;
        let history = SecurityPolicyHistory::new(
            Revision::new(3)?,
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    old_policy.clone(),
                ),
                SecurityPolicyVersion::new(Revision::new(1)?, SecurityEpoch::INITIAL, old_policy),
                SecurityPolicyVersion::new(
                    Revision::new(2)?,
                    SecurityEpoch::new(1),
                    current_policy.clone(),
                ),
                SecurityPolicyVersion::new(
                    Revision::new(3)?,
                    SecurityEpoch::new(1),
                    current_policy,
                ),
            ],
        )?;
        let now = history.select(AuthorizationMode::Now, principal, Revision::new(3)?)?;
        assert_eq!(now.current_epoch(), SecurityEpoch::new(1));
        assert_eq!(now.evaluated_epoch(), SecurityEpoch::new(1));
        assert_eq!(
            now.snapshot().authorize(
                principal,
                Capability::ProjectRead,
                PolicyTarget::new(None, None, None, None, None),
            ),
            AuthorizationDecision::Deny
        );

        let historical = history.select(
            AuthorizationMode::AtRevision(Revision::new(1)?),
            principal,
            Revision::new(3)?,
        )?;
        assert_eq!(historical.current_epoch(), SecurityEpoch::new(1));
        assert_eq!(historical.evaluated_epoch(), SecurityEpoch::INITIAL);
        assert_eq!(
            historical.snapshot().authorize(
                principal,
                Capability::ProjectRead,
                PolicyTarget::new(None, None, None, None, None),
            ),
            AuthorizationDecision::Allow
        );
        Ok(())
    }

    #[test]
    fn historical_policy_requires_current_permission_and_complete_genesis_history()
    -> Result<(), PolicyTestError> {
        use crate::ids::{DomainId, Revision};

        let principal = PrincipalId::try_from_bytes(uuid_bytes(64))?;
        let policy =
            SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], vec![])?;
        let history = SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                policy.clone(),
            )],
        )?;
        assert_eq!(
            history
                .select(
                    AuthorizationMode::AtRevision(Revision::GENESIS),
                    principal,
                    Revision::GENESIS,
                )
                .err(),
            Some(SecurityPolicyHistoryError::HistoricalPermissionDenied)
        );
        assert_eq!(
            SecurityPolicyHistory::new(Revision::GENESIS, vec![]).err(),
            Some(SecurityPolicyHistoryError::MissingInitialSnapshot)
        );

        let with_gap = SecurityPolicyHistory::new(
            Revision::new(2)?,
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    policy.clone(),
                ),
                SecurityPolicyVersion::new(
                    Revision::new(2)?,
                    SecurityEpoch::INITIAL,
                    policy.clone(),
                ),
            ],
        );
        assert_eq!(
            with_gap.err(),
            Some(SecurityPolicyHistoryError::IncompleteRevisionCoverage)
        );

        Ok(())
    }

    #[test]
    fn evaluator_fails_closed_for_inactive_principals_and_checks_scope_dimensions()
    -> Result<(), PolicyTestError> {
        use crate::ids::DomainId;

        let principal_id = PrincipalId::try_from_bytes(uuid_bytes(16))?;
        let role_id = RoleId::try_from_bytes(uuid_bytes(17))?;
        let assignment_id = RoleAssignmentId::try_from_bytes(uuid_bytes(18))?;
        let history_space = HistorySpaceId::try_from_bytes(uuid_bytes(19))?;
        let predicate_id = PredicateId::try_from_bytes(uuid_bytes(20))?;
        let role = RoleDefinition::new(
            role_id,
            "reader",
            PolicyBundle::from_grants([CapabilityGrant::new(
                Capability::FieldRead,
                GrantEffect::Allow,
            )])?,
        )?;
        let assignment = RoleAssignment::new(
            assignment_id,
            principal_id,
            role_id,
            PolicyScope::new(Some(history_space), None, None, None, None),
        );
        let policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id).with_state(PrincipalState::Disabled)],
            vec![role],
            vec![assignment],
            Vec::new(),
        )?;
        let target = PolicyTarget::new(
            Some(history_space),
            None,
            None,
            Some(FieldSelector::AssertionValue(predicate_id)),
            None,
        );
        assert!(
            PolicyScope::new(
                Some(history_space),
                None,
                None,
                Some(FieldSelector::AssertionValue(predicate_id)),
                None,
            )
            .matches(target)
        );
        assert!(
            !PolicyScope::new(
                Some(history_space),
                None,
                None,
                Some(FieldSelector::AssertionSubject),
                None,
            )
            .matches(target)
        );
        assert_eq!(
            policy.authorize(principal_id, Capability::FieldRead, target),
            AuthorizationDecision::Deny
        );
        Ok(())
    }

    fn uuid_bytes(tail: u8) -> [u8; 16] {
        [
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, tail,
        ]
    }

    fn test_uuid_bytes() -> [u8; 16] {
        [
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ]
    }

    fn test_uuid_bytes_other() -> [u8; 16] {
        [
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x02,
        ]
    }
}
