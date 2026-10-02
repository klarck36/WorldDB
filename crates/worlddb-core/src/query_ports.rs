//! Owned, snapshot-bound outputs for the public query boundary.

use std::collections::BTreeSet;
use std::fmt;

use crate::history_model::HistorySpaceReferenceModel;
use crate::ids::{AssertionId, SecurityEpoch};
use crate::query_context::{QueryContext, QueryContextBinding};
use crate::record_refs::RecordRef;
use crate::reference_query::{
    ExplainStage, HistoricalQueryBinding, RawHistoryError, RawHistoryRow, ReferenceExplain,
    ResolvedView, full_scan_authorized_raw_history,
};
use crate::resource_profile::MemoryReservation;
use crate::security::{SecurityPolicyHistory, SecurityPolicyHistoryError};

/// Query value whose contents are owned and tied to one immutable data/schema/security view.
///
/// The `'static` bound prevents a result from borrowing from a lock guard, memory map,
/// or other request-scoped storage view.
///
/// ```compile_fail
/// use worlddb_core::OwnedQueryResult;
/// fn cannot_store_local_borrow<'a>(value: &'a str) {
///     let _: Option<OwnedQueryResult<&'a str>> = None;
/// }
/// ```
#[derive(Clone, Debug)]
pub struct OwnedQueryResult<T: 'static> {
    query_context_binding: QueryContextBinding,
    binding: HistoricalQueryBinding,
    current_security_epoch: SecurityEpoch,
    evaluated_security_epoch: SecurityEpoch,
    value: T,
    _memory_reservations: Vec<MemoryReservation>,
}

impl<T: 'static> OwnedQueryResult<T> {
    pub(crate) fn bind(
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
        value: T,
    ) -> Result<Self, SecurityPolicyHistoryError> {
        Self::bind_with_memory_reservations(context, policies, value, Vec::new())
    }

    pub(crate) fn bind_with_memory_reservations(
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
        value: T,
        memory_reservations: Vec<MemoryReservation>,
    ) -> Result<Self, SecurityPolicyHistoryError> {
        let security = policies.resolve(context)?;
        Ok(Self {
            query_context_binding: context.binding(),
            binding: context.schema_binding(),
            current_security_epoch: security.current_epoch(),
            evaluated_security_epoch: security.evaluated_epoch(),
            value,
            _memory_reservations: memory_reservations,
        })
    }

    /// Data revision and schema interpretation used by this output.
    #[must_use]
    pub const fn binding(&self) -> HistoricalQueryBinding {
        self.binding
    }

    /// All semantic query axes used by this output.
    #[must_use]
    pub const fn query_context_binding(&self) -> &QueryContextBinding {
        &self.query_context_binding
    }

    /// Current epoch used for administrative gating and reauthorization.
    #[must_use]
    pub const fn current_security_epoch(&self) -> SecurityEpoch {
        self.current_security_epoch
    }

    /// Epoch of the policy actually used to authorize this output.
    #[must_use]
    pub const fn evaluated_security_epoch(&self) -> SecurityEpoch {
        self.evaluated_security_epoch
    }

    /// Owned result payload, borrowed only from this result object.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

/// Executes the raw HistorySpace query and returns owned rows with their full query binding.
pub fn full_scan_owned_authorized_raw_history<T: Clone + 'static>(
    history: &HistorySpaceReferenceModel<T>,
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
    record_ref: impl Fn(&T) -> RecordRef,
) -> Result<OwnedQueryResult<Vec<RawHistoryRow<T>>>, RawHistoryError> {
    let rows = full_scan_authorized_raw_history(history, context, policies, record_ref)?;
    OwnedQueryResult::bind(context, policies, rows).map_err(RawHistoryError::from)
}

/// Binds a resolved outcome only when all of its contributors are in the visible candidate set.
pub fn bind_authorized_resolved_view(
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
    visible_assertions: &[AssertionId],
    view: ResolvedView,
) -> Result<OwnedQueryResult<ResolvedView>, QueryPortError> {
    validate_resolved_visibility(&view, visible_assertions)?;
    OwnedQueryResult::bind(context, policies, view).map_err(QueryPortError::Security)
}

/// Binds an Explain trace only when every candidate and applied record is caller-visible.
pub fn bind_authorized_explain(
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
    visible_assertions: &[AssertionId],
    visible_records: &[RecordRef],
    explain: ReferenceExplain,
) -> Result<OwnedQueryResult<ReferenceExplain>, QueryPortError> {
    if explain.binding() != context.schema_binding() {
        return Err(QueryPortError::QueryBindingMismatch);
    }
    validate_explain_visibility(explain.stages(), visible_assertions, visible_records)?;
    validate_resolved_visibility(explain.resolved_view(), visible_assertions)?;
    OwnedQueryResult::bind(context, policies, explain).map_err(QueryPortError::Security)
}

fn validate_resolved_visibility(
    view: &ResolvedView,
    visible_assertions: &[AssertionId],
) -> Result<(), QueryPortError> {
    let visible = visible_assertions.iter().copied().collect::<BTreeSet<_>>();
    if view
        .contributors()
        .iter()
        .any(|assertion_id| !visible.contains(assertion_id))
    {
        return Err(QueryPortError::HiddenAssertionContributor);
    }
    Ok(())
}

fn validate_explain_visibility(
    stages: &[ExplainStage],
    visible_assertions: &[AssertionId],
    visible_records: &[RecordRef],
) -> Result<(), QueryPortError> {
    let assertions = visible_assertions.iter().copied().collect::<BTreeSet<_>>();
    for stage in stages {
        if stage
            .input_assertions()
            .iter()
            .chain(stage.output_assertions())
            .any(|assertion_id| !assertions.contains(assertion_id))
        {
            return Err(QueryPortError::HiddenAssertionContributor);
        }
        if stage
            .applied_records()
            .iter()
            .any(|record_ref| !visible_records.contains(record_ref))
        {
            return Err(QueryPortError::HiddenExplainRecord);
        }
    }
    Ok(())
}

/// An assertion contributor or Explain record failed visibility validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryPortError {
    /// A resolved outcome refers to an assertion outside the visible candidate set.
    HiddenAssertionContributor,
    /// Explain refers to a record outside the visible set.
    HiddenExplainRecord,
    /// Explain and QueryContext use different data/schema bindings.
    QueryBindingMismatch,
    /// Security time selection could not be authorized or resolved.
    Security(SecurityPolicyHistoryError),
}

impl fmt::Display for QueryPortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HiddenAssertionContributor => {
                formatter.write_str("query result contains a hidden assertion contributor")
            }
            Self::HiddenExplainRecord => {
                formatter.write_str("query explanation contains a hidden record")
            }
            Self::QueryBindingMismatch => {
                formatter.write_str("query explanation binding does not match its context")
            }
            Self::Security(error) => write!(formatter, "query security binding failed: {error}"),
        }
    }
}

impl std::error::Error for QueryPortError {}

#[cfg(test)]
mod tests {
    use super::{QueryPortError, validate_explain_visibility, validate_resolved_visibility};
    use crate::ids::{AssertionId, DomainId};
    use crate::record_refs::RecordRef;
    use crate::reference_query::{ExplainStage, ExplainStageKind, ResolvedView};
    use crate::single_value_resolution::SingleValueOutcome;

    fn id<T: DomainId>(tail: u8) -> T {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).unwrap_or_else(|_| unreachable!("test ID is valid"))
    }

    #[test]
    fn resolved_view_cannot_expose_a_hidden_contributor() {
        let hidden = id::<AssertionId>(2);
        let visible = id::<AssertionId>(1);
        let view = ResolvedView::from_single(SingleValueOutcome::Conflict {
            contributors: vec![visible, hidden],
        });
        assert!(view.is_ok());
        if let Ok(view) = view {
            assert_eq!(
                validate_resolved_visibility(&view, &[visible, hidden]),
                Ok(())
            );
            assert_eq!(
                validate_resolved_visibility(&view, &[visible]),
                Err(QueryPortError::HiddenAssertionContributor)
            );
        }
    }

    #[test]
    fn explain_boundary_rejects_hidden_candidates_and_applied_records() {
        let assertion = id::<AssertionId>(1);
        let hidden_assertion = id::<AssertionId>(2);
        let hidden_mask = RecordRef::Mask(id::<crate::MaskId>(3));
        let candidate = ExplainStage::new(
            ExplainStageKind::CandidateScan,
            vec![],
            vec![assertion, hidden_assertion],
            vec![],
        );
        assert!(candidate.is_ok());
        if let Ok(candidate) = candidate {
            assert_eq!(
                validate_explain_visibility(
                    &[candidate.clone()],
                    &[assertion, hidden_assertion],
                    &[]
                ),
                Ok(())
            );
            assert_eq!(
                validate_explain_visibility(&[candidate], &[assertion], &[]),
                Err(QueryPortError::HiddenAssertionContributor)
            );
        }
        let mask_stage = ExplainStage::new(
            ExplainStageKind::MaskProjection,
            vec![assertion],
            vec![assertion],
            vec![hidden_mask],
        );
        assert!(mask_stage.is_ok());
        if let Ok(mask_stage) = mask_stage {
            assert_eq!(
                validate_explain_visibility(&[mask_stage.clone()], &[assertion], &[hidden_mask]),
                Ok(())
            );
            assert_eq!(
                validate_explain_visibility(&[mask_stage], &[assertion], &[]),
                Err(QueryPortError::HiddenExplainRecord)
            );
        }
    }
}
