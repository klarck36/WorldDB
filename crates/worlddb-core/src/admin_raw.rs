//! Audit-authorized administrative raw-read boundary.

use std::fmt;

use crate::audit::{AuditScopeFingerprint, PageOrdinal};
use crate::ids::{ClientRequestId, SecurityEpoch, SnapshotId};
use crate::query_context::{QueryContext, QueryContextBinding};
use crate::query_ports::OwnedQueryResult;
use crate::reference_query::RawHistoryRow;
use crate::security::{
    AuthorizationDecision, Capability, PolicyTarget, SecurityPolicyHistory,
    SecurityPolicyHistoryError,
};

/// Safe identity and authorization facts supplied to the audit-authorization port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminRawAuthorizeRequest {
    principal_id: crate::ids::PrincipalId,
    scope_fingerprint: AuditScopeFingerprint,
    snapshot_id: SnapshotId,
    current_security_epoch: SecurityEpoch,
    evaluated_security_epoch: SecurityEpoch,
    page_ordinal: PageOrdinal,
    client_request_id: ClientRequestId,
}

impl AdminRawAuthorizeRequest {
    /// Authenticated principal requesting the page.
    #[must_use]
    pub const fn principal_id(&self) -> crate::ids::PrincipalId {
        self.principal_id
    }
    /// Opaque fingerprint of the requested administrative raw scope.
    #[must_use]
    pub const fn scope_fingerprint(&self) -> &AuditScopeFingerprint {
        &self.scope_fingerprint
    }
    /// Snapshot pinned for the page.
    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }
    /// Current policy epoch checked at authorization time.
    #[must_use]
    pub const fn current_security_epoch(&self) -> SecurityEpoch {
        self.current_security_epoch
    }
    /// Policy epoch used to evaluate the query.
    #[must_use]
    pub const fn evaluated_security_epoch(&self) -> SecurityEpoch {
        self.evaluated_security_epoch
    }
    /// Page ordinal in the caller's pagination contract.
    #[must_use]
    pub const fn page_ordinal(&self) -> PageOrdinal {
        self.page_ordinal
    }
    /// Stable client identity retained across retries of one raw-read request.
    #[must_use]
    pub const fn client_request_id(&self) -> ClientRequestId {
        self.client_request_id
    }
}

/// Synchronous authorization hook for the separate administrative raw path.
///
/// `Ok(())` means the required per-page `RawReadAttempt` is committed and synced
/// by the host's independent raw-read audit boundary. Any error blocks page release.
pub trait AdminRawAuditAuthorizer {
    /// Records/authorizes this page. Any error must prevent page release.
    fn authorize(&self, request: &AdminRawAuthorizeRequest) -> Result<(), AdminRawAuditError>;
}

/// Publicly safe audit-authorization failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AdminRawAuditError;

impl fmt::Display for AdminRawAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("administrative raw audit authorization failed")
    }
}
impl std::error::Error for AdminRawAuditError {}

/// Failure to authorize or bind an administrative raw page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdminRawError {
    /// Security policy time selection failed.
    SecurityHistory(SecurityPolicyHistoryError),
    /// One or more required capabilities were not granted.
    Unauthorized,
    /// The audit boundary rejected or could not record authorization.
    AuditAuthorization(AdminRawAuditError),
}

impl fmt::Display for AdminRawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecurityHistory(error) => {
                write!(formatter, "admin raw policy selection failed: {error}")
            }
            Self::Unauthorized => formatter.write_str("administrative raw read is not authorized"),
            Self::AuditAuthorization(error) => error.fmt(formatter),
        }
    }
}
impl std::error::Error for AdminRawError {}

/// Performs the independent admin capability checks, requires audit authorization,
/// then exposes one owned page bound to the complete query context.
pub fn release_admin_raw_page<T: 'static>(
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
    scope_fingerprint: AuditScopeFingerprint,
    page_ordinal: PageOrdinal,
    client_request_id: ClientRequestId,
    rows: Vec<RawHistoryRow<T>>,
    audit: &impl AdminRawAuditAuthorizer,
) -> Result<OwnedQueryResult<Vec<RawHistoryRow<T>>>, AdminRawError> {
    let view = policies
        .resolve(context)
        .map_err(AdminRawError::SecurityHistory)?;
    let principal = context.security().principal_id();
    let target = PolicyTarget::new(None, None, None, None, None);
    for capability in [
        Capability::HistorySpaceRead,
        Capability::RawHistoryRead,
        Capability::AdminRawRead,
    ] {
        if view.snapshot().authorize(principal, capability, target) != AuthorizationDecision::Allow
        {
            return Err(AdminRawError::Unauthorized);
        }
    }
    // An old AuthorizationAtRevision view never preserves administrative raw
    // access after the currently published policy revokes it.
    if view
        .current_snapshot()
        .authorize(principal, Capability::AdminRawRead, target)
        != AuthorizationDecision::Allow
    {
        return Err(AdminRawError::Unauthorized);
    }

    let request = AdminRawAuthorizeRequest {
        principal_id: principal,
        scope_fingerprint,
        snapshot_id: context.snapshot().id(),
        current_security_epoch: view.current_epoch(),
        evaluated_security_epoch: view.evaluated_epoch(),
        page_ordinal,
        client_request_id,
    };
    audit
        .authorize(&request)
        .map_err(AdminRawError::AuditAuthorization)?;

    // The local permit is deliberately unforgeable and is created only after the
    // synchronous audit authorization returned successfully.
    let permit = AdminRawReleasePermit {
        binding: context.binding(),
        page_ordinal,
        principal_id: principal,
        snapshot_id: request.snapshot_id,
        current_security_epoch: request.current_security_epoch,
        evaluated_security_epoch: request.evaluated_security_epoch,
    };
    if !permit.matches(
        context,
        page_ordinal,
        view.current_epoch(),
        view.evaluated_epoch(),
    ) {
        return Err(AdminRawError::Unauthorized);
    }
    OwnedQueryResult::bind(context, policies, rows).map_err(AdminRawError::SecurityHistory)
}

struct AdminRawReleasePermit {
    binding: QueryContextBinding,
    page_ordinal: PageOrdinal,
    principal_id: crate::ids::PrincipalId,
    snapshot_id: SnapshotId,
    current_security_epoch: SecurityEpoch,
    evaluated_security_epoch: SecurityEpoch,
}

impl AdminRawReleasePermit {
    fn matches(
        &self,
        context: &QueryContext,
        page_ordinal: PageOrdinal,
        current_epoch: SecurityEpoch,
        evaluated_epoch: SecurityEpoch,
    ) -> bool {
        self.binding.matches(context)
            && self.page_ordinal == page_ordinal
            && self.principal_id == context.security().principal_id()
            && self.snapshot_id == context.snapshot().id()
            && self.current_security_epoch == current_epoch
            && self.evaluated_security_epoch == evaluated_epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditScopeFingerprint;
    use crate::ids::{ClientRequestId, DomainId, PrincipalId, Revision, SecurityEpoch, SnapshotId};
    use crate::query_context::{
        AuthorizationMode, CancellationToken, QueryBudget, QueryBudgetLimits, QueryContext,
        QueryContextInput, SecurityContext, WorldTimeSelector,
    };
    use crate::record_refs::SnapshotRef;
    use crate::schema::Lifecycle;
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        CapabilityGrant, CapabilityRule, GrantEffect, PolicyScope, PolicySubject, Principal,
        SecurityPolicyHistory, SecurityPolicyVersion,
    };
    use crate::values::{Bytes, Symbol};
    use crate::{EpistemicMode, PerspectiveScope};
    use crate::{
        HistorySpaceId, LayerDefinition, LayerId, LayerSchemaSnapshot, LayerSelection,
        RecordedAsOf, ValidatedLayerSelection,
    };
    use std::cell::Cell;

    type TestResult<T> = Result<T, TestError>;

    #[derive(Debug)]
    struct TestError(String);
    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(&self.0)
        }
    }
    impl std::error::Error for TestError {}

    fn check<T, E: std::fmt::Display>(result: Result<T, E>) -> TestResult<T> {
        result.map_err(|error| TestError(error.to_string()))
    }

    fn id<T: DomainId>(last: u8) -> TestResult<T> {
        let mut bytes = [0; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = last;
        check(T::try_from_bytes(bytes))
    }

    fn context() -> TestResult<QueryContext> {
        let revision = check(Revision::new(1))?;
        let schema_revision = crate::SchemaRevision::from_published_revision(revision);
        let layer = LayerDefinition::new(
            id::<LayerId>(1)?,
            check(Symbol::new("world"))?,
            None,
            0,
            Lifecycle::Active,
            schema_revision,
        );
        let schema = check(LayerSchemaSnapshot::new(
            schema_revision,
            vec![layer],
            id::<LayerId>(1)?,
        ))?;
        let layers = check(ValidatedLayerSelection::resolve(
            &schema,
            LayerSelection::BaseOnly,
        ))?;
        let as_of = RecordedAsOf::from_published_revision(revision);
        let mut schema_history = SchemaHistoryReferenceModel::new();
        check(schema_history.publish(revision, vec![SchemaDefinition::LayerSnapshot(schema)]))?;
        let binding = check(crate::HistoricalQueryBinding::bind(
            &schema_history,
            as_of,
            SchemaMode::Historical,
        ))?;
        check(QueryContext::new(QueryContextInput {
            snapshot: SnapshotRef::new(id::<SnapshotId>(2)?),
            snapshot_revision: revision,
            recorded_as_of: as_of,
            history_space: id::<HistorySpaceId>(3)?,
            layers,
            world_time: WorldTimeSelector::AllTimes,
            perspective: PerspectiveScope::World,
            epistemic_mode: EpistemicMode::WorldState,
            schema_binding: binding,
            security: SecurityContext::new(id::<PrincipalId>(4)?, AuthorizationMode::Now),
            budget: check(QueryBudget::new(
                10,
                100,
                100,
                check(QueryBudgetLimits::new(100, 1_000, 1_000))?,
            ))?,
            cancellation: CancellationToken::new(),
        }))
    }

    fn policies(grants: &[Capability]) -> TestResult<SecurityPolicyHistory> {
        let principal = id::<PrincipalId>(4)?;
        let principal_record = Principal::new(principal);
        let rules = grants
            .iter()
            .enumerate()
            .map(|(index, capability)| -> TestResult<_> {
                Ok(CapabilityRule::new(
                    id::<crate::PolicyRuleId>(index as u8 + 1)?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(*capability, GrantEffect::Allow),
                    PolicyScope::project(),
                ))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let snapshot = check(crate::SecurityPolicySnapshot::new(
            vec![principal_record],
            vec![],
            vec![],
            rules,
        ))?;
        check(SecurityPolicyHistory::new(
            revision(1)?,
            vec![
                SecurityPolicyVersion::new(revision(0)?, SecurityEpoch::INITIAL, snapshot.clone()),
                SecurityPolicyVersion::new(revision(1)?, SecurityEpoch::INITIAL, snapshot),
            ],
        ))
    }

    fn revision(value: u64) -> TestResult<Revision> {
        check(Revision::new(value))
    }

    struct Audit {
        called: Cell<bool>,
        fail: bool,
    }
    impl AdminRawAuditAuthorizer for Audit {
        fn authorize(&self, request: &AdminRawAuthorizeRequest) -> Result<(), AdminRawAuditError> {
            self.called.set(true);
            let _safe_scope = (
                request.principal_id(),
                request.scope_fingerprint(),
                request.snapshot_id(),
                request.current_security_epoch(),
                request.evaluated_security_epoch(),
                request.page_ordinal(),
                request.client_request_id(),
            );
            if self.fail {
                Err(AdminRawAuditError)
            } else {
                Ok(())
            }
        }
    }

    fn request_scope() -> TestResult<AuditScopeFingerprint> {
        check(AuditScopeFingerprint::new(Bytes::new(vec![1, 2, 3])))
    }

    #[test]
    fn admin_path_requires_both_raw_capabilities_and_does_not_call_audit_when_denied()
    -> TestResult<()> {
        let audit = Audit {
            called: Cell::new(false),
            fail: false,
        };
        let result = release_admin_raw_page::<u8>(
            &context()?,
            &policies(&[Capability::HistorySpaceRead, Capability::RawHistoryRead])?,
            request_scope()?,
            PageOrdinal::new(0),
            id::<ClientRequestId>(5)?,
            Vec::new(),
            &audit,
        );
        assert!(matches!(result, Err(AdminRawError::Unauthorized)));
        assert!(!audit.called.get());
        Ok(())
    }

    #[test]
    fn audit_failure_fails_closed_before_page_is_returned() -> TestResult<()> {
        let audit = Audit {
            called: Cell::new(false),
            fail: true,
        };
        let result = release_admin_raw_page::<u8>(
            &context()?,
            &policies(&[
                Capability::HistorySpaceRead,
                Capability::RawHistoryRead,
                Capability::AdminRawRead,
            ])?,
            request_scope()?,
            PageOrdinal::new(7),
            id::<ClientRequestId>(8)?,
            Vec::new(),
            &audit,
        );
        assert!(matches!(result, Err(AdminRawError::AuditAuthorization(_))));
        assert!(audit.called.get());
        Ok(())
    }

    #[test]
    fn successful_audit_authorization_is_bound_to_page_scope_context_and_epochs() -> TestResult<()>
    {
        let audit = Audit {
            called: Cell::new(false),
            fail: false,
        };
        let result = check(release_admin_raw_page::<u8>(
            &context()?,
            &policies(&[
                Capability::HistorySpaceRead,
                Capability::RawHistoryRead,
                Capability::AdminRawRead,
            ])?,
            request_scope()?,
            PageOrdinal::new(7),
            id::<ClientRequestId>(9)?,
            Vec::new(),
            &audit,
        ))?;
        assert_eq!(
            result.query_context_binding().snapshot().id(),
            id::<SnapshotId>(2)?
        );
        assert!(audit.called.get());
        Ok(())
    }
}
