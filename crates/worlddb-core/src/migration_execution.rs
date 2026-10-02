//! Per-step execution of compatible schema migrations through ordinary OCC commits.

use std::collections::BTreeSet;
use std::fmt;

const OPERATION_INDEX_MEMORY_RESERVATION_BYTES: u64 = 64;

use crate::ids::{MigrationRunId, MigrationStepId, OperationId, Revision, SchemaRevision};
use crate::migration_transform::{MigrationTransformFingerprint, MigrationTransformer};
use crate::revision_backend::{CancellablePublishError, RevisionBackend};
use crate::revision_history::RevisionLogError;
use crate::transaction_flow::{OpenTransaction, TransactionBeginError};
use crate::wire_records::{Record, RecordCodecError, decode_record_with_limits};
use crate::{
    DecoderLimits, MigrationCategory, MigrationPlan, MigrationPlanError,
    MigrationStepCommitIdentity, MigrationStepTargetSchema, MigrationTransformerError,
};

/// Canonical records prepared for one stable migration-plan step.
#[derive(Debug)]
pub struct MigrationStepInput {
    step_id: MigrationStepId,
    operation_id: OperationId,
    records: Vec<Vec<u8>>,
}

impl MigrationStepInput {
    /// Associates already prepared canonical record frames with one plan step and operation.
    #[must_use]
    pub fn new(step_id: MigrationStepId, operation_id: OperationId, records: Vec<Vec<u8>>) -> Self {
        Self {
            step_id,
            operation_id,
            records,
        }
    }

    /// Stable plan-step identity.
    #[must_use]
    pub const fn step_id(&self) -> MigrationStepId {
        self.step_id
    }

    /// Idempotency identity persisted with the step's ordinary record batch.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }
}

/// Evidence returned for one atomically published migration step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationStepCommitReceipt {
    step_id: MigrationStepId,
    operation_id: OperationId,
    revision: Revision,
    target_schema: MigrationStepTargetSchema,
    transform_fingerprint: MigrationTransformFingerprint,
}

impl MigrationStepCommitReceipt {
    /// Stable plan-step identity.
    #[must_use]
    pub const fn step_id(self) -> MigrationStepId {
        self.step_id
    }

    /// Idempotency identity stored in this commit.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Exact published database revision.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Fingerprinted schema state validated before publication.
    #[must_use]
    pub const fn target_schema(self) -> MigrationStepTargetSchema {
        self.target_schema
    }

    /// Shared transform's digest for this exact step input and output.
    #[must_use]
    pub const fn transform_fingerprint(self) -> MigrationTransformFingerprint {
        self.transform_fingerprint
    }
}

/// Completed compatible migration whose steps each occupy a separate commit revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationExecutionResult {
    completed_steps: Vec<MigrationStepCommitReceipt>,
    final_revision: Revision,
    total_work_units: u64,
    total_input_bytes: u64,
}

impl MigrationExecutionResult {
    /// Ordered receipts for every committed plan step.
    #[must_use]
    pub fn completed_steps(&self) -> &[MigrationStepCommitReceipt] {
        &self.completed_steps
    }

    /// Revision published by the final step.
    #[must_use]
    pub const fn final_revision(&self) -> Revision {
        self.final_revision
    }

    /// Aggregate record work admitted across all steps.
    #[must_use]
    pub const fn total_work_units(&self) -> u64 {
        self.total_work_units
    }

    /// Aggregate input-frame bytes admitted across all steps.
    #[must_use]
    pub const fn total_input_bytes(&self) -> u64 {
        self.total_input_bytes
    }
}

/// Failure returned with the exact prefix of steps that already committed.
#[derive(Debug)]
pub struct MigrationExecutionFailure<E> {
    error: MigrationExecutionError<E>,
    completed_steps: Vec<MigrationStepCommitReceipt>,
}

impl<E> MigrationExecutionFailure<E> {
    /// First error that stopped execution.
    #[must_use]
    pub const fn error(&self) -> &MigrationExecutionError<E> {
        &self.error
    }

    /// Earlier valid commits; these are never rolled back by this API.
    #[must_use]
    pub fn completed_steps(&self) -> &[MigrationStepCommitReceipt] {
        &self.completed_steps
    }
}

/// Validation, admission, transform, or ordinary transaction failure.
#[derive(Debug)]
pub enum MigrationExecutionError<E> {
    /// The migration plan's source precondition or transformer version failed.
    Plan(MigrationPlanError),
    /// Only MetadataOnly, Additive, and CompatibleConstraintChange plans execute here.
    IncompatibleCategory(MigrationCategory),
    /// One exact intermediate schema target is required for each step.
    MissingStepTargets,
    /// The backend head differs from the plan's source schema revision.
    SourceRevisionDoesNotMatchHead { planned: Revision, actual: Revision },
    /// There must be exactly one prepared input for every plan step.
    StepInputCountMismatch,
    /// Prepared step inputs must use the exact order and identities in the immutable plan.
    StepInputOrderMismatch {
        expected: MigrationStepId,
        actual: MigrationStepId,
    },
    /// A migration cannot reuse an OperationId for more than one step.
    DuplicateOperationId(OperationId),
    /// The aggregate step work exceeds the immutable plan's limit.
    WorkBudgetExceeded { requested: u64, limit: u64 },
    /// The admitted working set exceeds the immutable plan's limit.
    MemoryBudgetExceeded { requested: u64, limit: u64 },
    /// A bounded reservation failed before publishing a step.
    AllocationFailed,
    /// The shared transformer rejected a step input.
    Transformer(MigrationTransformerError),
    /// A transformer preview had an invalid or omitted item.
    InvalidTransformPreview,
    /// The shared transformer left unresolved items for a compatible migration.
    UnresolvedItemsRemain,
    /// A transformed canonical frame could not be decoded to a typed record.
    RecordCodec(RecordCodecError),
    /// The ordinary OCC transaction could not begin at the pinned backend head.
    TransactionBegin(TransactionBeginError),
    /// Full historical-state validation or duplicate-identity checks rejected one step.
    StepValidation {
        step_id: MigrationStepId,
        error: MigrationStepValidationError<E>,
    },
    /// The backend failed while atomically publishing one complete step batch.
    Publish {
        step_id: MigrationStepId,
        error: CancellablePublishError,
    },
    /// A backend published a revision different from the plan-fingerprinted step target.
    UnexpectedPublishedRevision {
        step_id: MigrationStepId,
        expected: Revision,
        actual: Revision,
    },
}

/// Reason a historical intermediate state was rejected before a step commit.
#[derive(Debug)]
pub enum MigrationStepValidationError<E> {
    /// The pinned historical revision could not be read.
    History(RevisionLogError),
    /// This run already committed the same stable step identity.
    StepAlreadyCommitted,
    /// The operation identity already exists in committed migration history.
    OperationIdAlreadyCommitted,
    /// The caller's full engine validation rejected the proposed intermediate state.
    Engine(E),
}

/// Executes the complete immutable compatible plan as one normal OCC transaction per step.
///
/// `step_inputs` contain the canonical records to add at each step, not a copy of the full
/// historical database. Before the first commit, every input is previewed and aggregate work and
/// working-memory budgets are checked. The shared versioned transformer then produces each
/// step's exact records. Before publication, `validate_step` can read the pinned backend history
/// and must verify that the staged records form a valid intermediate schema identified by the
/// plan's step target. A later rejection leaves earlier commits intact and returns their receipts.
///
/// M7-06 adds durable run-journal recovery and unknown-outcome resolution. This function provides
/// only the per-step atomic commit and input/operation identities needed by that later task.
pub fn execute_compatible_migration<B, E>(
    backend: &mut B,
    plan: &MigrationPlan,
    run_id: MigrationRunId,
    actual_source_schema_fingerprint: [u8; 32],
    transformer: MigrationTransformer,
    step_inputs: Vec<MigrationStepInput>,
    mut validate_step: impl FnMut(&B, Revision, &[Record], MigrationStepTargetSchema) -> Result<(), E>,
) -> Result<MigrationExecutionResult, MigrationExecutionFailure<E>>
where
    B: RevisionBackend<Record>,
{
    let mut completed_steps = Vec::new();
    let fail = |error, completed_steps| MigrationExecutionFailure {
        error,
        completed_steps,
    };

    if !matches!(
        plan.category(),
        MigrationCategory::MetadataOnly
            | MigrationCategory::Additive
            | MigrationCategory::CompatibleConstraintChange
    ) {
        return Err(fail(
            MigrationExecutionError::IncompatibleCategory(plan.category()),
            completed_steps,
        ));
    }
    let Some(step_targets) = plan.step_targets() else {
        return Err(fail(
            MigrationExecutionError::MissingStepTargets,
            completed_steps,
        ));
    };

    let latest = backend.latest_published();
    let actual_source_revision = SchemaRevision::from_published_revision(latest);
    if latest != plan.source_schema_precondition().revision().revision() {
        return Err(fail(
            MigrationExecutionError::SourceRevisionDoesNotMatchHead {
                planned: plan.source_schema_precondition().revision().revision(),
                actual: latest,
            },
            completed_steps,
        ));
    }
    if let Err(error) = plan.validate_start(
        actual_source_revision,
        actual_source_schema_fingerprint,
        transformer.version(),
    ) {
        return Err(fail(MigrationExecutionError::Plan(error), completed_steps));
    }
    if step_inputs.len() != plan.steps().len() || step_targets.len() != plan.steps().len() {
        return Err(fail(
            MigrationExecutionError::StepInputCountMismatch,
            completed_steps,
        ));
    }
    let operation_index_memory = u64::try_from(step_inputs.len())
        .ok()
        .and_then(|count| count.checked_mul(OPERATION_INDEX_MEMORY_RESERVATION_BYTES));
    let Some(operation_index_memory) = operation_index_memory else {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    if operation_index_memory > plan.budget().max_memory_bytes() {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: operation_index_memory,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    }

    for ((input, planned_step), step_target) in
        step_inputs.iter().zip(plan.steps()).zip(step_targets)
    {
        if input.step_id() != *planned_step || step_target.step_id() != *planned_step {
            return Err(fail(
                MigrationExecutionError::StepInputOrderMismatch {
                    expected: *planned_step,
                    actual: input.step_id(),
                },
                completed_steps,
            ));
        }
    }
    let mut operation_ids = BTreeSet::new();
    for input in &step_inputs {
        if !operation_ids.insert(input.operation_id()) {
            return Err(fail(
                MigrationExecutionError::DuplicateOperationId(input.operation_id()),
                completed_steps,
            ));
        }
    }
    drop(operation_ids);

    let mut total_work_units = 0_u64;
    let mut total_input_bytes = 0_u64;
    let mut total_record_count = 0_u64;
    let mut max_step_output_bytes = 0_u64;
    let mut max_step_record_count = 0_u64;
    for input in &step_inputs {
        let estimate = match transformer.estimate_records(
            plan,
            plan.source_schema_precondition().revision(),
            *plan.source_schema_precondition().fingerprint(),
            &input.records,
        ) {
            Ok(estimate) => estimate,
            Err(error) => {
                return Err(fail(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        let step_work_units = estimate.record_count().max(1);
        total_work_units = match total_work_units.checked_add(step_work_units) {
            Some(total) => total,
            None => {
                return Err(fail(
                    MigrationExecutionError::WorkBudgetExceeded {
                        requested: u64::MAX,
                        limit: plan.budget().max_work_units(),
                    },
                    completed_steps,
                ));
            }
        };
        if total_work_units > plan.budget().max_work_units() {
            return Err(fail(
                MigrationExecutionError::WorkBudgetExceeded {
                    requested: total_work_units,
                    limit: plan.budget().max_work_units(),
                },
                completed_steps,
            ));
        }
        total_input_bytes = match total_input_bytes.checked_add(estimate.input_bytes()) {
            Some(total) => total,
            None => {
                return Err(fail(
                    MigrationExecutionError::MemoryBudgetExceeded {
                        requested: u64::MAX,
                        limit: plan.budget().max_memory_bytes(),
                    },
                    completed_steps,
                ));
            }
        };
        total_record_count = match total_record_count.checked_add(estimate.record_count()) {
            Some(total) => total,
            None => {
                return Err(fail(
                    MigrationExecutionError::MemoryBudgetExceeded {
                        requested: u64::MAX,
                        limit: plan.budget().max_memory_bytes(),
                    },
                    completed_steps,
                ));
            }
        };
        max_step_output_bytes = max_step_output_bytes.max(estimate.output_bytes());
        max_step_record_count = max_step_record_count.max(estimate.record_count());

        let preview = match transformer.transform_records_for_preview(
            plan,
            plan.source_schema_precondition().revision(),
            *plan.source_schema_precondition().fingerprint(),
            &input.records,
        ) {
            Ok(preview) => preview,
            Err(error) => {
                return Err(fail(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        if let Some(error) = preview.first_record_error() {
            return Err(fail(
                MigrationExecutionError::Transformer(error.cause()),
                completed_steps,
            ));
        }
        if preview.error_count() != 0 || preview.fingerprint().is_none() {
            return Err(fail(
                MigrationExecutionError::InvalidTransformPreview,
                completed_steps,
            ));
        }
        if preview.omitted_unresolved_count() != 0 || !preview.unresolved_items().is_empty() {
            return Err(fail(
                MigrationExecutionError::UnresolvedItemsRemain,
                completed_steps,
            ));
        }
    }

    let input_slot_bytes = total_record_count.checked_mul(32);
    let step_slot_bytes = max_step_record_count.checked_mul(
        32_u64.saturating_add(u64::try_from(std::mem::size_of::<Record>()).unwrap_or(u64::MAX)),
    );
    let receipt_bytes = u64::try_from(step_inputs.len()).ok().and_then(|count| {
        count.checked_mul(u64::try_from(std::mem::size_of::<MigrationStepCommitReceipt>()).ok()?)
    });
    let step_input_headers = u64::try_from(step_inputs.len()).ok().and_then(|count| {
        count.checked_mul(u64::try_from(std::mem::size_of::<MigrationStepInput>()).ok()?)
    });
    let working_memory = input_slot_bytes
        .and_then(|slots| total_input_bytes.checked_add(slots))
        .and_then(|total| total.checked_add(max_step_output_bytes.checked_mul(2)?))
        .and_then(|total| total.checked_add(step_slot_bytes?))
        .and_then(|total| total.checked_add(receipt_bytes?))
        .and_then(|total| total.checked_add(step_input_headers?))
        .and_then(|total| total.checked_add(operation_index_memory));
    let Some(working_memory) = working_memory else {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    if working_memory > plan.budget().max_memory_bytes() {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: working_memory,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    }
    if completed_steps
        .try_reserve_exact(step_inputs.len())
        .is_err()
    {
        return Err(fail(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }

    for (input, step_target) in step_inputs.into_iter().zip(step_targets.iter().copied()) {
        let step_id = input.step_id();
        let operation_id = input.operation_id();
        let batch = match transformer.transform_records(
            plan,
            plan.source_schema_precondition().revision(),
            *plan.source_schema_precondition().fingerprint(),
            &input.records,
        ) {
            Ok(batch) => batch,
            Err(error) => {
                return Err(fail(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        let transform_fingerprint = batch.fingerprint();
        let mut staged_records = Vec::new();
        if staged_records
            .try_reserve_exact(batch.records().len().saturating_add(1))
            .is_err()
        {
            return Err(fail(
                MigrationExecutionError::AllocationFailed,
                completed_steps,
            ));
        }
        for frame in batch.records() {
            let record = match decode_record_with_limits(frame, &DecoderLimits::DEFAULT) {
                Ok(record) => record.into_record(),
                Err(error) => {
                    return Err(fail(
                        MigrationExecutionError::RecordCodec(error),
                        completed_steps,
                    ));
                }
            };
            staged_records.push(record);
        }
        drop(batch);
        staged_records.push(Record::MigrationStepCommitIdentity(
            MigrationStepCommitIdentity::new(plan.migration_id(), run_id, step_id, operation_id),
        ));

        let base_revision = backend.latest_published();
        let expected_revision = match base_revision.next_commit() {
            Ok(revision) => revision,
            Err(_) => {
                return Err(fail(
                    MigrationExecutionError::UnexpectedPublishedRevision {
                        step_id,
                        expected: step_target.schema().revision().revision(),
                        actual: base_revision,
                    },
                    completed_steps,
                ));
            }
        };
        if expected_revision != step_target.schema().revision().revision() {
            return Err(fail(
                MigrationExecutionError::UnexpectedPublishedRevision {
                    step_id,
                    expected: step_target.schema().revision().revision(),
                    actual: expected_revision,
                },
                completed_steps,
            ));
        }

        let mut transaction = match OpenTransaction::begin(backend, base_revision) {
            Ok(transaction) => transaction,
            Err(error) => {
                return Err(fail(
                    MigrationExecutionError::TransactionBegin(error),
                    completed_steps,
                ));
            }
        };
        for record in staged_records {
            transaction.stage(record);
        }
        let validated = transaction.validate_with_backend(|backend, base, staged| {
            let history = backend
                .read_at(base)
                .map_err(MigrationStepValidationError::History)?;
            for (_, record) in history {
                if let Record::MigrationStepCommitIdentity(identity) = record {
                    if identity.operation_id() == operation_id {
                        return Err(MigrationStepValidationError::OperationIdAlreadyCommitted);
                    }
                    if identity.migration_id() == plan.migration_id()
                        && identity.run_id() == run_id
                        && identity.step_id() == step_id
                    {
                        return Err(MigrationStepValidationError::StepAlreadyCommitted);
                    }
                }
            }
            validate_step(backend, base, staged, step_target)
                .map_err(MigrationStepValidationError::Engine)
        });
        let validated = match validated {
            Ok(validated) => validated,
            Err(error) => {
                return Err(fail(
                    MigrationExecutionError::StepValidation { step_id, error },
                    completed_steps,
                ));
            }
        };
        let revision = match validated.commit() {
            Ok(revision) => revision,
            Err(error) => {
                return Err(fail(
                    MigrationExecutionError::Publish { step_id, error },
                    completed_steps,
                ));
            }
        };
        completed_steps.push(MigrationStepCommitReceipt {
            step_id,
            operation_id,
            revision,
            target_schema: step_target,
            transform_fingerprint,
        });
        if revision != expected_revision {
            return Err(fail(
                MigrationExecutionError::UnexpectedPublishedRevision {
                    step_id,
                    expected: expected_revision,
                    actual: revision,
                },
                completed_steps,
            ));
        }
    }

    let final_revision = backend.latest_published();
    Ok(MigrationExecutionResult {
        completed_steps,
        final_revision,
        total_work_units,
        total_input_bytes,
    })
}

impl<E> fmt::Display for MigrationExecutionFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "migration stopped after {} committed step(s)",
            self.completed_steps.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MigrationExecutionError, MigrationStepInput, MigrationStepValidationError,
        execute_compatible_migration,
    };
    use crate::ids::{
        DomainId, IdValidationError, MigrationId, MigrationRunId, MigrationStepId, OperationId,
        PredicateId, Revision, SchemaRevision,
    };
    use crate::revision_backend::{InMemoryRevisionBackend, RevisionBackend};
    use crate::wire_records::{Record, encode_record};
    use crate::{
        Cardinality, ConstraintSet, EntityTypeConstraint, JobBudget, Lifecycle, MigrationCategory,
        MigrationPlan, MigrationPlanSpec, MigrationStepTargetSchema, MigrationTargetSchema,
        MigrationTransformer, MigrationTransformerVersion, NonEmptySet, PredicateDefinition,
        PredicateDefinitionSpec, ResolutionPolicy, SchemaDefinition, SchemaDefinitionId,
        SchemaHistoryReferenceModel, SchemaIdentityTransition, SchemaMode,
        SourceSchemaPrecondition, Symbol, ValueConstraint, ValueKind,
    };

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn two_step_plan(
        category: MigrationCategory,
        max_work_units: u64,
    ) -> Result<(MigrationPlan, [MigrationStepId; 2]), String> {
        two_step_plan_with_fingerprints(
            category,
            max_work_units,
            [0x11; 32],
            [0x22; 32],
            [0x33; 32],
            1024 * 1024,
        )
    }

    fn two_step_plan_with_fingerprints(
        category: MigrationCategory,
        max_work_units: u64,
        source_fingerprint: [u8; 32],
        first_target_fingerprint: [u8; 32],
        final_target_fingerprint: [u8; 32],
        max_memory_bytes: u64,
    ) -> Result<(MigrationPlan, [MigrationStepId; 2]), String> {
        let migration_id = uuid::<MigrationId>(1).map_err(|error| error.to_string())?;
        let steps = [uuid::<MigrationStepId>(2), uuid::<MigrationStepId>(3)]
            .map(|result| result.map_err(|error| error.to_string()))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let steps: [MigrationStepId; 2] = steps
            .try_into()
            .map_err(|_| String::from("expected two migration steps"))?;
        let predicate = uuid::<PredicateId>(4).map_err(|error| error.to_string())?;
        let transition = match category {
            MigrationCategory::MetadataOnly
            | MigrationCategory::CompatibleConstraintChange
            | MigrationCategory::Restrictive => SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(predicate)),
                Some(SchemaDefinitionId::Predicate(predicate)),
                category,
            ),
            MigrationCategory::Additive => SchemaIdentityTransition::new(
                None,
                Some(SchemaDefinitionId::Predicate(predicate)),
                category,
            ),
            MigrationCategory::Breaking => SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(predicate)),
                Some(SchemaDefinitionId::Predicate(
                    uuid::<PredicateId>(5).map_err(|error| error.to_string())?,
                )),
                category,
            ),
        }
        .map_err(|error| error.to_string())?;
        let first_target = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            first_target_fingerprint,
        );
        let final_target = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(Revision::new(2).map_err(|e| e.to_string())?),
            final_target_fingerprint,
        );
        let plan = MigrationPlan::new(MigrationPlanSpec {
            migration_id,
            category,
            source_schema: SourceSchemaPrecondition::new(
                SchemaRevision::from_published_revision(Revision::GENESIS),
                source_fingerprint,
            ),
            target_schema: final_target,
            steps: steps.to_vec(),
            step_targets: Some(vec![
                MigrationStepTargetSchema::new(steps[0], first_target),
                MigrationStepTargetSchema::new(steps[1], final_target),
            ]),
            schema_changes: vec![transition],
            transformer_version: MigrationTransformerVersion::new(1)
                .map_err(|error| error.to_string())?,
            calendar_shift: None,
            budget: JobBudget::new(max_work_units, max_memory_bytes)
                .map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())?;
        Ok((plan, steps))
    }

    fn prepared_steps(step_ids: [MigrationStepId; 2]) -> Result<Vec<MigrationStepInput>, String> {
        Ok(vec![
            MigrationStepInput::new(
                step_ids[0],
                uuid::<OperationId>(6).map_err(|error| error.to_string())?,
                Vec::new(),
            ),
            MigrationStepInput::new(
                step_ids[1],
                uuid::<OperationId>(7).map_err(|error| error.to_string())?,
                Vec::new(),
            ),
        ])
    }

    fn run_id() -> Result<MigrationRunId, String> {
        uuid::<MigrationRunId>(8).map_err(|error| error.to_string())
    }

    fn transformer() -> Result<MigrationTransformer, String> {
        let version = MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?;
        MigrationTransformer::for_version(version).map_err(|error| error.to_string())
    }

    fn schema_definition(record: &Record) -> Option<SchemaDefinition> {
        match record {
            Record::LayerDefinition(value) => Some(SchemaDefinition::Layer(value.clone())),
            Record::LayerSchemaSnapshot(value) => {
                Some(SchemaDefinition::LayerSnapshot(value.clone()))
            }
            Record::EntityTypeDefinition(value) => {
                Some(SchemaDefinition::EntityType(value.clone()))
            }
            Record::PredicateDefinition(value) => Some(SchemaDefinition::Predicate(value.clone())),
            Record::EventKindDefinition(value) => Some(SchemaDefinition::EventKind(value.clone())),
            _ => None,
        }
    }

    #[test]
    fn compatible_steps_are_separate_occ_commits_and_keep_old_history_readable()
    -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let result = execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            [0x11; 32],
            transformer()?,
            prepared_steps(step_ids)?,
            |backend, base_revision, staged, target| {
                if backend.latest_published() != base_revision {
                    return Err(String::from("validator observed a stale OCC base"));
                }
                if target.schema().revision().revision()
                    != base_revision
                        .next_commit()
                        .map_err(|error| error.to_string())?
                {
                    return Err(String::from("step target does not follow its OCC base"));
                }
                if staged.len() != 1
                    || !matches!(staged.first(), Some(Record::MigrationStepCommitIdentity(_)))
                {
                    return Err(String::from("step marker was not atomic with its batch"));
                }
                let history = backend
                    .read_at(base_revision)
                    .map_err(|error| error.to_string())?
                    .collect::<Vec<_>>();
                if history.len() != usize::try_from(base_revision.value()).unwrap_or(usize::MAX) {
                    return Err(String::from("prior step history is incomplete"));
                }
                Ok(())
            },
        )
        .map_err(|failure| failure.to_string())?;

        assert_eq!(result.completed_steps().len(), 2);
        assert_eq!(
            result
                .completed_steps()
                .first()
                .map(|receipt| receipt.revision()),
            Some(Revision::FIRST_COMMIT)
        );
        assert_eq!(result.final_revision().value(), 2);
        assert_eq!(backend.latest_published(), result.final_revision());
        assert_eq!(
            backend
                .read_at(Revision::GENESIS)
                .map_err(|e| e.to_string())?
                .count(),
            0
        );
        assert_eq!(
            backend
                .read_at(result.final_revision())
                .map_err(|e| e.to_string())?
                .count(),
            2
        );
        Ok(())
    }

    #[test]
    fn rejecting_a_later_intermediate_state_does_not_roll_back_earlier_commits()
    -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let run_id = run_id()?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let failure = match execute_compatible_migration(
            &mut backend,
            &plan,
            run_id,
            [0x11; 32],
            transformer()?,
            prepared_steps(step_ids)?,
            |_, _, _, target| {
                if target.step_id() == step_ids[1] {
                    Err(String::from("second intermediate schema rejected"))
                } else {
                    Ok(())
                }
            },
        ) {
            Ok(_) => return Err(String::from("the second step validation must fail")),
            Err(failure) => failure,
        };

        assert_eq!(failure.completed_steps().len(), 1);
        assert_eq!(
            failure
                .completed_steps()
                .first()
                .map(|receipt| receipt.revision()),
            Some(Revision::FIRST_COMMIT)
        );
        assert!(matches!(
            failure.error(),
            MigrationExecutionError::StepValidation {
                error: MigrationStepValidationError::Engine(message),
                ..
            } if message == "second intermediate schema rejected"
        ));
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);
        assert_eq!(
            backend
                .read_at(Revision::GENESIS)
                .map_err(|e| e.to_string())?
                .count(),
            0
        );
        assert_eq!(
            backend
                .read_at(Revision::FIRST_COMMIT)
                .map_err(|e| e.to_string())?
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn restrictive_migrations_are_rejected_before_any_occ_commit() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Restrictive, 10)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let failure = match execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            [0x11; 32],
            transformer()?,
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => {
                return Err(String::from(
                    "restrictive work belongs to the later guarded executor",
                ));
            }
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationExecutionError::IncompatibleCategory(MigrationCategory::Restrictive)
        ));
        assert!(failure.completed_steps().is_empty());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[test]
    fn all_step_work_is_admitted_before_the_first_commit() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 1)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let failure = match execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            [0x11; 32],
            transformer()?,
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => {
                return Err(String::from(
                    "the aggregate two-step work must exceed budget one",
                ));
            }
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationExecutionError::WorkBudgetExceeded {
                requested: 2,
                limit: 1
            }
        ));
        assert!(failure.completed_steps().is_empty());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[test]
    fn aggregate_memory_admission_includes_operation_identity_index() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan_with_fingerprints(
            MigrationCategory::Additive,
            10,
            [0x11; 32],
            [0x22; 32],
            [0x33; 32],
            400,
        )?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let failure = match execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            [0x11; 32],
            transformer()?,
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => {
                return Err(String::from(
                    "combined migration memory must exceed 400 bytes",
                ));
            }
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationExecutionError::MemoryBudgetExceeded { limit: 400, .. }
        ));
        assert!(failure.completed_steps().is_empty());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[test]
    fn duplicate_operation_ids_are_rejected_before_any_commit() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let mut steps = prepared_steps(step_ids)?;
        let first_operation_id = steps
            .first()
            .map(MigrationStepInput::operation_id)
            .ok_or_else(|| String::from("first migration input is missing"))?;
        let second = steps
            .get_mut(1)
            .ok_or_else(|| String::from("second migration input is missing"))?;
        second.operation_id = first_operation_id;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let failure = match execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            [0x11; 32],
            transformer()?,
            steps,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => {
                return Err(String::from(
                    "duplicate operation identity must fail closed",
                ));
            }
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationExecutionError::DuplicateOperationId(id) if *id == first_operation_id
        ));
        assert!(failure.completed_steps().is_empty());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[test]
    fn malformed_later_step_is_rejected_before_any_commit() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let mut steps = prepared_steps(step_ids)?;
        let second = steps
            .get_mut(1)
            .ok_or_else(|| String::from("second migration input is missing"))?;
        second.records = vec![vec![0_u8]];
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let failure = match execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            [0x11; 32],
            transformer()?,
            steps,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => return Err(String::from("the invalid second frame must fail preflight")),
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationExecutionError::Transformer(_)
        ));
        assert!(failure.completed_steps().is_empty());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[test]
    fn compatible_step_publishes_a_fingerprint_validated_intermediate_schema() -> Result<(), String>
    {
        let predicate_id = uuid::<PredicateId>(4).map_err(|error| error.to_string())?;
        let constraints = ConstraintSet::new(vec![ValueConstraint::BoolSet(
            NonEmptySet::new(vec![true]).map_err(|error| error.to_string())?,
        )])
        .map_err(|error| error.to_string())?;
        let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: Symbol::new("enabled").map_err(|error| error.to_string())?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::Bool,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: Revision::FIRST_COMMIT,
        })
        .map_err(|error| error.to_string())?;
        let record = Record::PredicateDefinition(predicate);
        let encoded = encode_record(&record).map_err(|error| error.to_string())?;
        let mut expected_schema = SchemaHistoryReferenceModel::new();
        let source_fingerprint = expected_schema
            .schema_at(SchemaMode::Current, Revision::GENESIS)
            .map_err(|error| error.to_string())?
            .fingerprint();
        expected_schema
            .publish(
                Revision::FIRST_COMMIT,
                vec![
                    schema_definition(&record)
                        .ok_or_else(|| String::from("predicate schema definition was lost"))?,
                ],
            )
            .map_err(|error| error.to_string())?;
        let first_target_fingerprint = expected_schema
            .schema_at(SchemaMode::Current, Revision::FIRST_COMMIT)
            .map_err(|error| error.to_string())?
            .fingerprint();
        let final_revision = Revision::new(2).map_err(|error| error.to_string())?;
        expected_schema
            .publish(final_revision, Vec::new())
            .map_err(|error| error.to_string())?;
        let final_target_fingerprint = expected_schema
            .schema_at(SchemaMode::Current, final_revision)
            .map_err(|error| error.to_string())?
            .fingerprint();
        let (plan, step_ids) = two_step_plan_with_fingerprints(
            MigrationCategory::Additive,
            10,
            source_fingerprint,
            first_target_fingerprint,
            final_target_fingerprint,
            1024 * 1024,
        )?;
        let steps = vec![
            MigrationStepInput::new(
                step_ids[0],
                uuid::<OperationId>(9).map_err(|error| error.to_string())?,
                vec![encoded],
            ),
            MigrationStepInput::new(
                step_ids[1],
                uuid::<OperationId>(10).map_err(|error| error.to_string())?,
                Vec::new(),
            ),
        ];
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let result = execute_compatible_migration(
            &mut backend,
            &plan,
            run_id()?,
            source_fingerprint,
            transformer()?,
            steps,
            |backend, base_revision, staged, target| {
                let current_history = backend
                    .read_at(base_revision)
                    .map_err(|error| error.to_string())?
                    .collect::<Vec<_>>();
                let mut candidate = SchemaHistoryReferenceModel::new();
                for value in 1..=base_revision.value() {
                    let revision = Revision::new(value).map_err(|error| error.to_string())?;
                    let mut definitions = Vec::new();
                    for (record_revision, record) in &current_history {
                        if *record_revision == revision {
                            if let Some(definition) = schema_definition(record) {
                                definitions.push(definition);
                            }
                        }
                    }
                    candidate
                        .publish(revision, definitions)
                        .map_err(|error| error.to_string())?;
                }
                let mut definitions = Vec::new();
                for staged_record in staged {
                    if let Some(definition) = schema_definition(staged_record) {
                        definitions.push(definition);
                    }
                }
                candidate
                    .publish(target.schema().revision().revision(), definitions)
                    .map_err(|error| error.to_string())?;
                let snapshot = candidate
                    .schema_at(SchemaMode::Current, target.schema().revision().revision())
                    .map_err(|error| error.to_string())?;
                if snapshot.fingerprint() != *target.schema().fingerprint() {
                    return Err(String::from(
                        "intermediate schema fingerprint differs from plan",
                    ));
                }
                if target.step_id() == step_ids[0]
                    && !matches!(staged.first(), Some(Record::PredicateDefinition(_)))
                {
                    return Err(String::from(
                        "new schema record did not reach the step sink",
                    ));
                }
                Ok(())
            },
        )
        .map_err(|failure| failure.to_string())?;
        assert_eq!(result.completed_steps().len(), 2);
        assert_eq!(result.final_revision(), final_revision);
        assert_eq!(
            backend
                .read_at(Revision::FIRST_COMMIT)
                .map_err(|e| e.to_string())?
                .count(),
            2
        );
        Ok(())
    }
}
