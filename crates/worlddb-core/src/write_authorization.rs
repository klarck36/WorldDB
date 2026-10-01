//! Commit-time capability checks for validated assertion and event writes.

use std::fmt;

use crate::context::PerspectiveScope;
use crate::ids::PrincipalId;
use crate::schema_write_validation::ValidatedAssertionBatch;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicySnapshot,
};
use crate::values::Value;
use crate::write_reference_validation::ValidatedWriteReferenceBatch;

/// Rechecks every operation, field, layer, Entity-reference, and Perspective-use
/// permission for the authenticated Principal against the current policy.
///
/// All checks use exact source HistorySpace and Layer coordinates. No permission
/// is inferred from another capability, a Perspective, or an earlier preview.
pub fn authorize_validated_write_batch(
    batch: &ValidatedWriteReferenceBatch,
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
) -> Result<(), WriteAuthorizationError> {
    for assertion in batch.assertions() {
        let context = assertion.context();
        let value_field = FieldSelector::AssertionValue(assertion.predicate_id());
        let target = PolicyTarget::new(
            Some(context.history_space_id()),
            Some(context.layer_id()),
            None,
            None,
            None,
        );
        require(
            policy,
            principal,
            Capability::AssertionCreate,
            PolicyTarget::new(
                Some(context.history_space_id()),
                Some(context.layer_id()),
                None,
                Some(value_field),
                None,
            ),
        )?;
        require(policy, principal, Capability::LayerWrite, target)?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            value_field,
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            require(
                policy,
                principal,
                Capability::FieldWrite,
                PolicyTarget::new(
                    Some(context.history_space_id()),
                    Some(context.layer_id()),
                    None,
                    Some(field),
                    None,
                ),
            )?;
        }
        require(policy, principal, Capability::EntityReference, target)?;
        if let Value::Entity(_) = assertion.value() {
            require(policy, principal, Capability::EntityReference, target)?;
        }
        if matches!(
            context.perspective_scope(),
            PerspectiveScope::Perspective(_)
        ) {
            require(policy, principal, Capability::PerspectiveUse, target)?;
        }
    }

    for event in batch.events() {
        let target = PolicyTarget::new(
            Some(event.history_space_id()),
            Some(event.layer_id()),
            None,
            None,
            None,
        );
        require(policy, principal, Capability::EventCreate, target)?;
        require(policy, principal, Capability::LayerWrite, target)?;
        let event_kind = event.event_kind_id();
        for field in [
            FieldSelector::EventKind,
            FieldSelector::EventTime(event_kind),
        ] {
            require(
                policy,
                principal,
                Capability::FieldWrite,
                PolicyTarget::new(
                    Some(event.history_space_id()),
                    Some(event.layer_id()),
                    None,
                    Some(field),
                    None,
                ),
            )?;
        }
        for participant in event.participants().as_slice() {
            require(policy, principal, Capability::EntityReference, target)?;
            require(
                policy,
                principal,
                Capability::FieldWrite,
                PolicyTarget::new(
                    Some(event.history_space_id()),
                    Some(event.layer_id()),
                    None,
                    Some(FieldSelector::EventParticipant(
                        event_kind,
                        participant.role_id(),
                    )),
                    None,
                ),
            )?;
        }
        for attribute in event.attributes().as_slice() {
            if matches!(attribute.value(), Value::Entity(_)) {
                require(policy, principal, Capability::EntityReference, target)?;
            }
            require(
                policy,
                principal,
                Capability::FieldWrite,
                PolicyTarget::new(
                    Some(event.history_space_id()),
                    Some(event.layer_id()),
                    None,
                    Some(FieldSelector::EventAttribute(
                        event_kind,
                        attribute.attribute_id(),
                    )),
                    None,
                ),
            )?;
        }
    }
    Ok(())
}

/// Rechecks exact deprecated-schema warning capabilities at the commit boundary.
///
/// The schema-validated drafts must be identical and in the same order as the
/// reference-validated assertion drafts. Every warning is then checked against
/// the exact predicate value field and the draft's HistorySpace and Layer.
pub fn authorize_deprecated_schema_warnings(
    references: &ValidatedWriteReferenceBatch,
    schema_assertions: &ValidatedAssertionBatch,
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
) -> Result<(), WriteAuthorizationError> {
    if references.assertions().len() != schema_assertions.drafts().len()
        || references
            .assertions()
            .iter()
            .zip(schema_assertions.drafts())
            .any(|(reference, schema)| !same_assertion_draft(reference, schema))
    {
        return Err(WriteAuthorizationError::AssertionSchemaBatchMismatch);
    }
    for warning in schema_assertions.warnings() {
        let mut found_draft = false;
        for assertion in references
            .assertions()
            .iter()
            .filter(|draft| draft.predicate_id() == warning.predicate_id())
        {
            found_draft = true;
            let context = assertion.context();
            require(
                policy,
                principal,
                warning.capability(),
                PolicyTarget::new(
                    Some(context.history_space_id()),
                    Some(context.layer_id()),
                    None,
                    Some(FieldSelector::AssertionValue(warning.predicate_id())),
                    None,
                ),
            )?;
        }
        if !found_draft {
            return Err(WriteAuthorizationError::AssertionSchemaBatchMismatch);
        }
    }
    Ok(())
}

fn same_assertion_draft(
    left: &crate::assertions::AssertionDraft,
    right: &crate::assertions::AssertionDraft,
) -> bool {
    use crate::values::Value;

    let same_value = match (left.value(), right.value()) {
        (Value::Time(left), Value::Time(right)) => {
            left.timeline_id() == right.timeline_id()
                && left.ticks() == right.ticks()
                && left.unit() == right.unit()
        }
        (left, right) => crate::values::canonical_value_equality(left, right) == Some(true),
    };
    left.context() == right.context()
        && left.subject() == right.subject()
        && left.predicate_id() == right.predicate_id()
        && same_value
        && left.polarity() == right.polarity()
        && left.validity() == right.validity()
}

fn require(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    capability: Capability,
    target: PolicyTarget,
) -> Result<(), WriteAuthorizationError> {
    if policy.authorize(principal, capability, target) == AuthorizationDecision::Allow {
        Ok(())
    } else {
        Err(WriteAuthorizationError::Denied { capability, target })
    }
}

/// A required write permission did not match the current active policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteAuthorizationError {
    /// The operation was denied for its exact resource or field target.
    Denied {
        /// Required independent capability.
        capability: Capability,
        /// Exact HistorySpace, Layer, field, and relationship coordinates.
        target: PolicyTarget,
    },
    /// Schema and reference validation did not cover identical assertion drafts.
    AssertionSchemaBatchMismatch,
}

impl fmt::Display for WriteAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied { capability, .. } => {
                write!(
                    formatter,
                    "write requires an ungranted {capability:?} capability"
                )
            }
            Self::AssertionSchemaBatchMismatch => {
                formatter.write_str("schema and reference validated different assertion batches")
            }
        }
    }
}

impl std::error::Error for WriteAuthorizationError {}
