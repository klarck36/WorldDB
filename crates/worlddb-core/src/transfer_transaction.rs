//! Atomic adapter from the transfer reference model to a mixed Record commit.

use std::fmt;

use crate::ids::{PrincipalId, Revision, RevisionError};
use crate::revision_backend::{CancellablePublishError, RevisionBackend};
use crate::security::{Capability, PolicyTarget, SecurityPolicySnapshot};
use crate::transaction_flow::{MixedRecordCommitError, OpenTransaction, commit_mixed_record_batch};
use crate::transfer_model::TransferPlan;
use crate::transfer_reference_model::{
    TransferReceipt, TransferReferenceModel, TransferReferenceModelError,
};
use crate::wire_records::Record;

/// Result of committing transfer Records and the reference-model post-state together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferTransactionOutcome<T> {
    /// The complete Record batch and reference-model candidate share one revision.
    Committed {
        /// The revision assigned by the backend.
        revision: Revision,
        /// Typed identity and lineage receipt returned after publish.
        receipt: TransferReceipt<T>,
    },
    /// A stale database base or destination head requires a fresh transfer plan.
    Conflict {
        /// Revision expected by the operation or plan.
        expected: Revision,
        /// Revision observed during commit validation.
        actual: Revision,
    },
}

/// Validation or publication error for a mixed transfer transaction.
#[derive(Debug)]
pub enum TransferTransactionError<E> {
    /// Current policy denied transfer from or into one HistorySpace.
    Denied {
        history_space_id: crate::ids::HistorySpaceId,
    },
    /// The transfer reference model rejected the candidate plan or records.
    Transfer(TransferReferenceModelError),
    /// Caller-supplied complete post-state validation rejected staged Records.
    Validation(E),
    /// The next shared revision cannot be represented.
    RevisionExhausted(RevisionError),
    /// Backend failed to publish the validated complete batch.
    Publish(CancellablePublishError),
    /// Internal transaction callback completed without returning its receipt.
    MissingReceipt,
}

/// Revalidates and stages a transfer on a cloned reference model, then publishes
/// the complete mixed Record batch once. `validate_records` must compare the
/// receipt's typed copies, lineage and lifecycle artifacts against the complete
/// staged post-state. The original model is replaced only after backend publish.
pub fn commit_transfer_record_batch<B, T, E>(
    transaction: OpenTransaction<'_, B, Record>,
    model: &mut TransferReferenceModel<T>,
    plan: &TransferPlan,
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    prepare: impl FnOnce(
        &mut TransferReferenceModel<T>,
    ) -> Result<TransferReceipt<T>, TransferReferenceModelError>,
    validate_records: impl FnOnce(Revision, &TransferReceipt<T>, &[Record]) -> Result<(), E>,
) -> Result<TransferTransactionOutcome<T>, TransferTransactionError<E>>
where
    B: RevisionBackend<Record>,
    T: Clone + Eq,
{
    let base_revision = transaction.base_revision();
    let backend_head = transaction.latest_published();
    if backend_head != base_revision {
        return Ok(TransferTransactionOutcome::Conflict {
            expected: base_revision,
            actual: backend_head,
        });
    }
    if model.latest_published() != base_revision {
        return Ok(TransferTransactionOutcome::Conflict {
            expected: base_revision,
            actual: model.latest_published(),
        });
    }
    for history_space_id in [
        plan.source_history_space_id(),
        plan.target_history_space_id(),
    ] {
        if !history_space_transfer_allowed(policy, principal, history_space_id) {
            return Err(TransferTransactionError::Denied { history_space_id });
        }
    }
    let expected_revision = base_revision
        .next_commit()
        .map_err(TransferTransactionError::RevisionExhausted)?;
    let mut candidate = model.clone();
    let mut receipt_after_validation = None;

    let result = commit_mixed_record_batch(transaction, |base, records| {
        if base != base_revision {
            return Err(TransferValidationFailure::Conflict {
                expected: base_revision,
                actual: base,
            });
        }
        let receipt = prepare(&mut candidate).map_err(|error| {
            if let TransferReferenceModelError::TargetHeadConflict { expected, actual } = error {
                TransferValidationFailure::Conflict { expected, actual }
            } else {
                TransferValidationFailure::Transfer(error)
            }
        })?;
        if receipt.revision() != expected_revision {
            return Err(TransferValidationFailure::Conflict {
                expected: expected_revision,
                actual: receipt.revision(),
            });
        }
        validate_records(expected_revision, &receipt, records)
            .map_err(TransferValidationFailure::Validation)?;
        receipt_after_validation = Some(receipt);
        Ok(())
    });

    let revision = match result {
        Ok(revision) => revision,
        Err(MixedRecordCommitError::Publish(error)) => {
            return Err(TransferTransactionError::Publish(error));
        }
        Err(MixedRecordCommitError::Validation(TransferValidationFailure::Conflict {
            expected,
            actual,
        })) => return Ok(TransferTransactionOutcome::Conflict { expected, actual }),
        Err(MixedRecordCommitError::Validation(TransferValidationFailure::Transfer(error))) => {
            return Err(TransferTransactionError::Transfer(error));
        }
        Err(MixedRecordCommitError::Validation(TransferValidationFailure::Validation(error))) => {
            return Err(TransferTransactionError::Validation(error));
        }
    };
    if revision != expected_revision {
        // The exclusive backend borrow prevents an intervening writer. Treat a
        // backend that violates this contract as a publish failure after the
        // single atomic write; never advance the reference model out of step.
        return Err(TransferTransactionError::Publish(
            CancellablePublishError::Publish(
                crate::revision_history::RevisionLogError::RevisionGap {
                    expected: expected_revision,
                    actual: revision,
                },
            ),
        ));
    }
    *model = candidate;
    let receipt = receipt_after_validation.ok_or(TransferTransactionError::MissingReceipt)?;
    Ok(TransferTransactionOutcome::Committed { revision, receipt })
}

#[derive(Debug)]
enum TransferValidationFailure<E> {
    Conflict {
        expected: Revision,
        actual: Revision,
    },
    Transfer(TransferReferenceModelError),
    Validation(E),
}

impl<E: fmt::Display> fmt::Display for TransferTransactionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied { history_space_id } => write!(
                formatter,
                "HistorySpace transfer capability denied for {history_space_id}"
            ),
            Self::Transfer(error) => write!(formatter, "transfer candidate failed: {error}"),
            Self::Validation(error) => {
                write!(formatter, "transfer record validation failed: {error}")
            }
            Self::RevisionExhausted(error) => write!(formatter, "revision exhausted: {error}"),
            Self::Publish(error) => write!(formatter, "transfer publication failed: {error}"),
            Self::MissingReceipt => {
                formatter.write_str("transfer receipt missing after publication")
            }
        }
    }
}

impl<E> std::error::Error for TransferTransactionError<E> where E: std::error::Error + 'static {}

fn history_space_transfer_allowed(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    history_space_id: crate::ids::HistorySpaceId,
) -> bool {
    policy.authorize(
        principal,
        Capability::HistorySpaceTransfer,
        PolicyTarget::new(Some(history_space_id), None, None, None, None),
    ) == crate::security::AuthorizationDecision::Allow
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{TransferTransactionOutcome, commit_transfer_record_batch};
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::catalog::HistorySpaceDefinition;
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, DatabaseId, DomainId, EntityId, HistorySpaceId, LayerId, PolicyRuleId,
        PredicateId, PrincipalId, ProvenanceId, Revision, TimelineId,
    };
    use crate::revision_backend::{InMemoryRevisionBackend, RevisionBackend};
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyScope, PolicySubject,
        Principal, SecurityPolicySnapshot,
    };
    use crate::source_provenance::{ProvenanceEdge, ProvenanceEndpointRef, ProvenanceRelation};
    use crate::temporal::{AssertionValidity, TimeInterval, Timeline};
    use crate::transaction_flow::OpenTransaction;
    use crate::transfer_model::{HistorySpaceContentRef, TransferPlan, TransferPlanSpec};
    use crate::transfer_reference_model::{TransferRecord, TransferReferenceModel};
    use crate::values::Value;
    use crate::wire_records::Record;

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    #[test]
    fn transfer_candidate_and_typed_records_publish_at_one_revision() -> Result<(), String> {
        let database = id::<DatabaseId>(1)?;
        let source_space = id::<HistorySpaceId>(2)?;
        let target_space = id::<HistorySpaceId>(3)?;
        let source_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(4)?);
        let target_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(5)?);
        let mut model = TransferReferenceModel::new(
            database,
            vec![
                HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
                HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            ],
        )
        .map_err(|error| error.to_string())?;
        let source_revision = model
            .publish(
                source_space,
                vec![TransferRecord::new(source_id, "claim", vec![])],
            )
            .map_err(|error| error.to_string())?;
        let plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_revision,
            target_history_space_id: target_space,
            expected_target_head: Revision::GENESIS,
            selected_records: BTreeSet::from([source_id]),
            record_id_map: BTreeMap::from([(source_id, target_id)]),
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: vec![],
            lifecycle_policy:
                crate::transfer_model::TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })
        .map_err(|error| error.to_string())?;

        let principal = id::<PrincipalId>(6)?;
        let rule_ids = [id::<PolicyRuleId>(7)?, id::<PolicyRuleId>(8)?];
        let rules = [source_space, target_space]
            .into_iter()
            .zip(rule_ids)
            .map(|(history_space_id, rule_id)| {
                CapabilityRule::new(
                    rule_id,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(Capability::HistorySpaceTransfer, GrantEffect::Allow),
                    PolicyScope::new(Some(history_space_id), None, None, None, None),
                )
            })
            .collect();
        let policy =
            SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
                .map_err(|error| error.to_string())?;

        let mut backend = InMemoryRevisionBackend::<Record>::new();
        assert_eq!(
            backend.publish(vec![]).map_err(|error| error.to_string())?,
            source_revision
        );
        let target_assertion = Assertion::new(
            match target_id {
                HistorySpaceContentRef::Assertion(id) => id,
                _ => return Err("expected assertion target".to_owned()),
            },
            AssertionDraft::new(
                ContextKey::new(
                    target_space,
                    id::<LayerId>(8)?,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState,
                )
                .map_err(|error| error.to_string())?,
                Subject::new(id::<EntityId>(9)?),
                id::<PredicateId>(10)?,
                Value::Bool(true),
                Polarity::Positive,
                AssertionValidity::new(
                    TimeInterval::new(Timeline::new(id::<TimelineId>(12)?), None, None)
                        .map_err(|error| error.to_string())?,
                ),
            ),
            Revision::new(2).map_err(|error| error.to_string())?,
        );
        let provenance_id = id::<ProvenanceId>(11)?;
        let target_record = Record::Assertion(target_assertion);
        let lineage_record = Record::Provenance(
            ProvenanceEdge::new(
                provenance_id,
                ProvenanceEndpointRef::Assertion(match source_id {
                    HistorySpaceContentRef::Assertion(id) => id,
                    _ => return Err("expected assertion source".to_owned()),
                }),
                ProvenanceEndpointRef::Assertion(match target_id {
                    HistorySpaceContentRef::Assertion(id) => id,
                    _ => return Err("expected assertion target".to_owned()),
                }),
                ProvenanceRelation::DerivedFrom,
                Revision::new(2).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
        );
        let mut transaction = OpenTransaction::begin(&mut backend, source_revision)
            .map_err(|error| error.to_string())?;
        transaction.stage(target_record.clone());
        transaction.stage(lineage_record.clone());

        let outcome = commit_transfer_record_batch(
            transaction,
            &mut model,
            &plan,
            &policy,
            principal,
            |candidate| candidate.commit_transfer(&plan, vec![provenance_id], vec![], vec![]),
            |revision, receipt, records| {
                assert_eq!(receipt.revision(), revision);
                assert_eq!(receipt.record_id_map().get(&source_id), Some(&target_id));
                assert_eq!(receipt.lineage().len(), 1);
                let actual = records
                    .iter()
                    .map(crate::wire_records::encode_record)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                let expected = [target_record.clone(), lineage_record.clone()]
                    .iter()
                    .map(crate::wire_records::encode_record)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                assert_eq!(actual, expected);
                Ok::<_, String>(())
            },
        )
        .map_err(|error| error.to_string())?;

        let committed = match outcome {
            TransferTransactionOutcome::Committed { revision, receipt } => {
                Some((revision, receipt))
            }
            TransferTransactionOutcome::Conflict { .. } => None,
        };
        assert!(committed.is_some(), "unexpected transfer conflict");
        if let Some((revision, receipt)) = committed {
            assert_eq!(
                revision,
                Revision::new(2).map_err(|error| error.to_string())?
            );
            assert_eq!(receipt.revision(), revision);
        }
        assert_eq!(model.latest_published(), backend.latest_published());
        assert_eq!(
            backend.latest_published(),
            Revision::new(2).map_err(|error| error.to_string())?
        );
        Ok(())
    }
}
