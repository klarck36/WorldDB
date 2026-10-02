//! Per-step execution of compatible schema migrations through ordinary OCC commits.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const OPERATION_INDEX_MEMORY_RESERVATION_BYTES: u64 = 64;

use crate::ids::{MigrationRunId, MigrationStepId, OperationId, Revision, SchemaRevision};
use crate::migration_transform::{MigrationTransformFingerprint, MigrationTransformer};
use crate::revision_backend::{CancellablePublishError, RevisionBackend};
use crate::revision_history::RevisionLogError;
use crate::transaction_flow::{OpenTransaction, TransactionBeginError};
use crate::wire_records::{Record, RecordCodecError, decode_record_with_limits};
use crate::{
    DecoderLimits, MigrationCategory, MigrationDryRunInputFingerprint, MigrationPlan,
    MigrationPlanError, MigrationRunJournalError, MigrationRunJournalSnapshot,
    MigrationRunJournalSpec, MigrationRunJournalStep, MigrationRunJournalStepSpec,
    MigrationRunJournalStepState, MigrationRunJournalStore, MigrationStepCommitIdentity,
    MigrationStepTargetSchema, MigrationTransformerError,
};

/// Canonical records prepared for one stable migration-plan step.
#[derive(Debug)]
pub struct MigrationStepInput {
    step_id: MigrationStepId,
    operation_id: OperationId,
    records: Vec<Vec<u8>>,
}

/// Immutable plan, source precondition, run identity, and transformer for one execution attempt.
#[derive(Clone, Copy, Debug)]
pub struct MigrationExecutionContext<'a> {
    plan: &'a MigrationPlan,
    run_id: MigrationRunId,
    actual_source_schema_fingerprint: [u8; 32],
    transformer: MigrationTransformer,
}

impl<'a> MigrationExecutionContext<'a> {
    /// Binds the original source-schema evidence and implementation to one run identity.
    #[must_use]
    pub const fn new(
        plan: &'a MigrationPlan,
        run_id: MigrationRunId,
        actual_source_schema_fingerprint: [u8; 32],
        transformer: MigrationTransformer,
    ) -> Self {
        Self {
            plan,
            run_id,
            actual_source_schema_fingerprint,
            transformer,
        }
    }
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

/// Failure while reconciling or resuming a journaled compatible migration.
#[derive(Debug)]
pub struct MigrationResumeFailure<E, J> {
    error: MigrationResumeError<E, J>,
    completed_steps: Vec<MigrationStepCommitReceipt>,
}

impl<E, J> MigrationResumeFailure<E, J> {
    /// First failure that stopped migration resume.
    #[must_use]
    pub const fn error(&self) -> &MigrationResumeError<E, J> {
        &self.error
    }

    /// Exact committed prefix confirmed in normative history.
    #[must_use]
    pub fn completed_steps(&self) -> &[MigrationStepCommitReceipt] {
        &self.completed_steps
    }
}

/// Plan, journal, marker, head, or normal execution failure during resume.
#[derive(Debug)]
pub enum MigrationResumeError<E, J> {
    /// Existing migration validation or step execution failed.
    Execution(MigrationExecutionError<E>),
    /// Durable coordination metadata could not be read or written.
    JournalStore(J),
    /// Journal state or persisted plan binding failed validation.
    Journal(MigrationRunJournalError),
    /// The run has a published marker whose operation identity conflicts with the plan.
    ConflictingCommitMarker { operation_id: OperationId },
    /// A journal-committed step has no matching marker in normative history.
    CommittedMarkerMissing { step_id: MigrationStepId },
    /// A normative run marker exists without its expected prepared journal entry.
    OrphanedRunMarker { operation_id: OperationId },
    /// Current history head differs from the exact committed prefix described by the journal.
    ResumeHeadMismatch {
        expected: Revision,
        actual: Revision,
    },
}

/// Normative-history result for a migration-step OperationId status query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationStepCommitStatus {
    /// No matching step-commit identity is present in published history.
    NotCommitted,
    /// A unique matching operation marker was published at this revision.
    Committed {
        revision: Revision,
        identity: MigrationStepCommitIdentity,
    },
}

/// History read failure or duplicate OperationId result for a status query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationStepStatusError {
    /// The backend could not read its published full-history view.
    History(RevisionLogError),
    /// More than one migration-step marker claims the requested OperationId.
    DuplicateOperationId(OperationId),
}

impl fmt::Display for MigrationStepStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::History(error) => write!(formatter, "could not query migration history: {error}"),
            Self::DuplicateOperationId(operation_id) => {
                write!(
                    formatter,
                    "multiple migration commits use OperationId {operation_id}"
                )
            }
        }
    }
}

impl std::error::Error for MigrationStepStatusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::History(error) => Some(error),
            Self::DuplicateOperationId(_) => None,
        }
    }
}

/// Queries normative history for the unique commit marker associated with one OperationId.
pub fn query_migration_step_status<B>(
    backend: &B,
    operation_id: OperationId,
) -> Result<MigrationStepCommitStatus, MigrationStepStatusError>
where
    B: RevisionBackend<Record>,
{
    let mut found = None;
    let history = backend
        .read_at(backend.latest_published())
        .map_err(MigrationStepStatusError::History)?;
    for (revision, record) in history {
        if let Record::MigrationStepCommitIdentity(identity) = record {
            if identity.operation_id() == operation_id {
                if found.is_some() {
                    return Err(MigrationStepStatusError::DuplicateOperationId(operation_id));
                }
                found = Some(MigrationStepCommitStatus::Committed {
                    revision,
                    identity: *identity,
                });
            }
        }
    }
    Ok(found.unwrap_or(MigrationStepCommitStatus::NotCommitted))
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
    /// The ordered input could not be represented by the canonical fingerprint format.
    InputFingerprintFailed,
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

    let input_fingerprint_bytes = u64::try_from(step_inputs.len())
        .ok()
        .and_then(|count| count.checked_mul(32));
    let Some(input_fingerprint_bytes) = input_fingerprint_bytes else {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    let fingerprint_admission = operation_index_memory.checked_add(input_fingerprint_bytes);
    let Some(fingerprint_admission) = fingerprint_admission else {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    if fingerprint_admission > plan.budget().max_memory_bytes() {
        return Err(fail(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: fingerprint_admission,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    }
    for input in &step_inputs {
        if MigrationDryRunInputFingerprint::for_records(&input.records).is_none() {
            return Err(fail(
                MigrationExecutionError::InputFingerprintFailed,
                completed_steps,
            ));
        }
    }

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
        .and_then(|total| total.checked_add(input_fingerprint_bytes))
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
    let mut input_fingerprints = Vec::new();
    if input_fingerprints
        .try_reserve_exact(step_inputs.len())
        .is_err()
    {
        return Err(fail(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }
    for input in &step_inputs {
        let Some(fingerprint) = MigrationDryRunInputFingerprint::for_records(&input.records) else {
            return Err(fail(
                MigrationExecutionError::InputFingerprintFailed,
                completed_steps,
            ));
        };
        input_fingerprints.push(*fingerprint.as_bytes());
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

    for ((input, step_target), input_fingerprint) in step_inputs
        .into_iter()
        .zip(step_targets.iter().copied())
        .zip(input_fingerprints)
    {
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
            MigrationStepCommitIdentity::with_input_fingerprint(
                plan.migration_id(),
                run_id,
                step_id,
                operation_id,
                input_fingerprint,
            ),
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

/// Executes or resumes a compatible migration using a durable sidecar run journal.
///
/// Before each normative commit the exact step input and OperationId are saved as `Prepared`.
/// Resume scans full committed history for the OperationId marker. A marker after a crash is
/// reconciled into the journal; an absent marker at the expected head retries the same step.
/// A mismatched marker, input fingerprint, plan, transformer version, or history head fails
/// closed. Published steps are never undone; semantic compensation must be represented by a new
/// migration plan with its own MigrationId and RunId.
pub fn execute_or_resume_compatible_migration<B, J, E>(
    backend: &mut B,
    journal: &mut J,
    context: MigrationExecutionContext<'_>,
    step_inputs: Vec<MigrationStepInput>,
    mut validate_step: impl FnMut(&B, Revision, &[Record], MigrationStepTargetSchema) -> Result<(), E>,
) -> Result<MigrationExecutionResult, MigrationResumeFailure<E, J::Error>>
where
    B: RevisionBackend<Record>,
    J: MigrationRunJournalStore,
{
    let MigrationExecutionContext {
        plan,
        run_id,
        actual_source_schema_fingerprint,
        transformer,
    } = context;
    let mut completed_steps = Vec::new();
    let fail = |error, completed_steps| MigrationResumeFailure {
        error,
        completed_steps,
    };
    let fail_execution =
        |error, completed_steps| fail(MigrationResumeError::Execution(error), completed_steps);

    if !matches!(
        plan.category(),
        MigrationCategory::MetadataOnly
            | MigrationCategory::Additive
            | MigrationCategory::CompatibleConstraintChange
    ) {
        return Err(fail_execution(
            MigrationExecutionError::IncompatibleCategory(plan.category()),
            completed_steps,
        ));
    }
    let Some(step_targets) = plan.step_targets() else {
        return Err(fail_execution(
            MigrationExecutionError::MissingStepTargets,
            completed_steps,
        ));
    };
    if let Err(error) = plan.validate_start(
        plan.source_schema_precondition().revision(),
        actual_source_schema_fingerprint,
        transformer.version(),
    ) {
        return Err(fail_execution(
            MigrationExecutionError::Plan(error),
            completed_steps,
        ));
    }
    if step_inputs.len() != plan.steps().len() || step_targets.len() != plan.steps().len() {
        return Err(fail_execution(
            MigrationExecutionError::StepInputCountMismatch,
            completed_steps,
        ));
    }
    for ((input, planned_step), step_target) in
        step_inputs.iter().zip(plan.steps()).zip(step_targets)
    {
        if input.step_id() != *planned_step || step_target.step_id() != *planned_step {
            return Err(fail_execution(
                MigrationExecutionError::StepInputOrderMismatch {
                    expected: *planned_step,
                    actual: input.step_id(),
                },
                completed_steps,
            ));
        }
    }

    let step_count = step_inputs.len();
    let input_fingerprint_bytes = u64::try_from(step_count)
        .ok()
        .and_then(|count| count.checked_mul(32));
    let Some(input_fingerprint_bytes) = input_fingerprint_bytes else {
        return Err(fail_execution(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    let operation_index_memory = u64::try_from(step_count)
        .ok()
        .and_then(|count| count.checked_mul(OPERATION_INDEX_MEMORY_RESERVATION_BYTES));
    let Some(operation_index_memory) = operation_index_memory else {
        return Err(fail_execution(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    let journal_working_bytes = u64::try_from(step_count).ok().and_then(|count| {
        let per_step = std::mem::size_of::<MigrationRunJournalStepSpec>()
            .checked_add(std::mem::size_of::<MigrationRunJournalStep>())?
            .checked_add(32)?;
        count.checked_mul(u64::try_from(per_step).ok()?)
    });
    let initial_admission = journal_working_bytes
        .and_then(|bytes| bytes.checked_add(input_fingerprint_bytes))
        .and_then(|bytes| bytes.checked_add(operation_index_memory));
    let Some(initial_admission) = initial_admission else {
        return Err(fail_execution(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    if initial_admission > plan.budget().max_memory_bytes() {
        return Err(fail_execution(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: initial_admission,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    }
    let mut operation_ids = BTreeSet::new();
    for input in &step_inputs {
        if !operation_ids.insert(input.operation_id()) {
            return Err(fail_execution(
                MigrationExecutionError::DuplicateOperationId(input.operation_id()),
                completed_steps,
            ));
        }
    }
    drop(operation_ids);
    let mut input_fingerprints = Vec::new();
    if input_fingerprints.try_reserve_exact(step_count).is_err() {
        return Err(fail_execution(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }
    for input in &step_inputs {
        let Some(fingerprint) = MigrationDryRunInputFingerprint::for_records(&input.records) else {
            return Err(fail_execution(
                MigrationExecutionError::InputFingerprintFailed,
                completed_steps,
            ));
        };
        input_fingerprints.push(*fingerprint.as_bytes());
    }

    let mut journal_step_specs = Vec::new();
    if journal_step_specs.try_reserve_exact(step_count).is_err() {
        return Err(fail_execution(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }
    for ((input, fingerprint), target) in step_inputs
        .iter()
        .zip(&input_fingerprints)
        .zip(step_targets)
    {
        journal_step_specs.push(MigrationRunJournalStepSpec::new(
            input.step_id(),
            input.operation_id(),
            *fingerprint,
            target.schema().revision().revision(),
        ));
    }
    let journal_spec = match MigrationRunJournalSpec::new(
        plan.migration_id(),
        run_id,
        plan.fingerprint(),
        plan.source_schema_precondition().revision(),
        *plan.source_schema_precondition().fingerprint(),
        transformer.version(),
        journal_step_specs,
    ) {
        Ok(spec) => spec,
        Err(error) => {
            return Err(fail(MigrationResumeError::Journal(error), completed_steps));
        }
    };
    let loaded_snapshot = match journal.load(run_id) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(fail(
                MigrationResumeError::JournalStore(error),
                completed_steps,
            ));
        }
    };
    let is_new_run = loaded_snapshot.is_none();
    let mut snapshot = match loaded_snapshot {
        Some(snapshot) => {
            if snapshot.spec().run_id() != run_id || snapshot.spec() != &journal_spec {
                return Err(fail(
                    MigrationResumeError::Journal(MigrationRunJournalError::RunIdentityMismatch),
                    completed_steps,
                ));
            }
            if let Err(error) = snapshot.spec().validate_plan(plan, transformer.version()) {
                return Err(fail(MigrationResumeError::Journal(error), completed_steps));
            }
            snapshot
        }
        None => match MigrationRunJournalSnapshot::start(journal_spec) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Err(fail(MigrationResumeError::Journal(error), completed_steps));
            }
        },
    };

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
                return Err(fail_execution(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        total_work_units = match total_work_units.checked_add(estimate.record_count().max(1)) {
            Some(total) if total <= plan.budget().max_work_units() => total,
            Some(requested) => {
                return Err(fail_execution(
                    MigrationExecutionError::WorkBudgetExceeded {
                        requested,
                        limit: plan.budget().max_work_units(),
                    },
                    completed_steps,
                ));
            }
            None => {
                return Err(fail_execution(
                    MigrationExecutionError::WorkBudgetExceeded {
                        requested: u64::MAX,
                        limit: plan.budget().max_work_units(),
                    },
                    completed_steps,
                ));
            }
        };
        total_input_bytes = match total_input_bytes.checked_add(estimate.input_bytes()) {
            Some(total) => total,
            None => {
                return Err(fail_execution(
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
                return Err(fail_execution(
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
                return Err(fail_execution(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        if let Some(error) = preview.first_record_error() {
            return Err(fail_execution(
                MigrationExecutionError::Transformer(error.cause()),
                completed_steps,
            ));
        }
        if preview.error_count() != 0 || preview.fingerprint().is_none() {
            return Err(fail_execution(
                MigrationExecutionError::InvalidTransformPreview,
                completed_steps,
            ));
        }
        if preview.omitted_unresolved_count() != 0 || !preview.unresolved_items().is_empty() {
            return Err(fail_execution(
                MigrationExecutionError::UnresolvedItemsRemain,
                completed_steps,
            ));
        }
    }
    let input_slot_bytes = total_record_count.checked_mul(32);
    let step_slot_bytes = max_step_record_count.checked_mul(
        32_u64.saturating_add(u64::try_from(std::mem::size_of::<Record>()).unwrap_or(u64::MAX)),
    );
    let receipt_bytes = u64::try_from(step_count).ok().and_then(|count| {
        count.checked_mul(u64::try_from(std::mem::size_of::<MigrationStepCommitReceipt>()).ok()?)
    });
    let step_input_headers = u64::try_from(step_count).ok().and_then(|count| {
        count.checked_mul(u64::try_from(std::mem::size_of::<MigrationStepInput>()).ok()?)
    });
    let working_memory = input_slot_bytes
        .and_then(|slots| total_input_bytes.checked_add(slots))
        .and_then(|total| total.checked_add(max_step_output_bytes.checked_mul(2)?))
        .and_then(|total| total.checked_add(step_slot_bytes?))
        .and_then(|total| total.checked_add(receipt_bytes?))
        .and_then(|total| total.checked_add(step_input_headers?))
        .and_then(|total| total.checked_add(input_fingerprint_bytes))
        .and_then(|total| total.checked_add(operation_index_memory))
        .and_then(|total| total.checked_add(journal_working_bytes?));
    let Some(working_memory) = working_memory else {
        return Err(fail_execution(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: u64::MAX,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    };
    if working_memory > plan.budget().max_memory_bytes() {
        return Err(fail_execution(
            MigrationExecutionError::MemoryBudgetExceeded {
                requested: working_memory,
                limit: plan.budget().max_memory_bytes(),
            },
            completed_steps,
        ));
    }

    let mut receipt_fingerprints = Vec::new();
    if receipt_fingerprints.try_reserve_exact(step_count).is_err() {
        return Err(fail_execution(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }
    for input in &step_inputs {
        let batch = match transformer.transform_records(
            plan,
            plan.source_schema_precondition().revision(),
            *plan.source_schema_precondition().fingerprint(),
            &input.records,
        ) {
            Ok(batch) => batch,
            Err(error) => {
                return Err(fail_execution(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        receipt_fingerprints.push(batch.fingerprint());
    }
    if completed_steps.try_reserve_exact(step_count).is_err() {
        return Err(fail_execution(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }

    let latest = backend.latest_published();
    let mut markers = BTreeMap::new();
    let Some(first_step) = plan.steps().first().copied() else {
        return Err(fail_execution(
            MigrationExecutionError::StepInputCountMismatch,
            completed_steps,
        ));
    };
    let historical = match backend.read_at(latest) {
        Ok(historical) => historical,
        Err(error) => {
            return Err(fail_execution(
                MigrationExecutionError::StepValidation {
                    step_id: first_step,
                    error: MigrationStepValidationError::History(error),
                },
                completed_steps,
            ));
        }
    };
    for (revision, record) in historical {
        let Record::MigrationStepCommitIdentity(identity) = record else {
            continue;
        };
        let expected = snapshot
            .spec()
            .steps()
            .iter()
            .find(|step| step.operation_id() == identity.operation_id());
        if let Some(expected) = expected {
            let exact = identity.migration_id() == plan.migration_id()
                && identity.run_id() == run_id
                && identity.step_id() == expected.step_id()
                && identity.input_fingerprint() == Some(expected.input_fingerprint())
                && revision == expected.target_revision();
            if !exact
                || markers
                    .insert(identity.operation_id(), (revision, *identity))
                    .is_some()
            {
                return Err(fail(
                    MigrationResumeError::ConflictingCommitMarker {
                        operation_id: identity.operation_id(),
                    },
                    completed_steps,
                ));
            }
        } else if identity.migration_id() == plan.migration_id() && identity.run_id() == run_id {
            return Err(fail(
                MigrationResumeError::OrphanedRunMarker {
                    operation_id: identity.operation_id(),
                },
                completed_steps,
            ));
        }
    }

    if is_new_run {
        if let Some((operation_id, _)) = markers.first_key_value() {
            return Err(fail(
                MigrationResumeError::OrphanedRunMarker {
                    operation_id: *operation_id,
                },
                completed_steps,
            ));
        }
        let source_revision = plan.source_schema_precondition().revision().revision();
        if latest != source_revision {
            return Err(fail_execution(
                MigrationExecutionError::SourceRevisionDoesNotMatchHead {
                    planned: source_revision,
                    actual: latest,
                },
                completed_steps,
            ));
        }
        if let Err(error) = journal.save(&snapshot) {
            return Err(fail(
                MigrationResumeError::JournalStore(error),
                completed_steps,
            ));
        }
    }

    let mut committed_count = 0_usize;
    let mut journal_steps = Vec::new();
    if journal_steps
        .try_reserve_exact(snapshot.steps().len())
        .is_err()
    {
        return Err(fail_execution(
            MigrationExecutionError::AllocationFailed,
            completed_steps,
        ));
    }
    journal_steps.extend_from_slice(snapshot.steps());
    for (index, step) in journal_steps.into_iter().enumerate() {
        if step_inputs.get(index).is_none() {
            return Err(fail_execution(
                MigrationExecutionError::StepInputCountMismatch,
                completed_steps,
            ));
        }
        let Some(target_schema) = step_targets.get(index).copied() else {
            return Err(fail_execution(
                MigrationExecutionError::MissingStepTargets,
                completed_steps,
            ));
        };
        let Some(receipt_fingerprint) = receipt_fingerprints.get(index).copied() else {
            return Err(fail_execution(
                MigrationExecutionError::AllocationFailed,
                completed_steps,
            ));
        };
        let spec = step.spec();
        let marker = markers.get(&spec.operation_id());
        match step.state() {
            MigrationRunJournalStepState::Pending => {
                if marker.is_some() {
                    return Err(fail(
                        MigrationResumeError::OrphanedRunMarker {
                            operation_id: spec.operation_id(),
                        },
                        completed_steps,
                    ));
                }
                break;
            }
            MigrationRunJournalStepState::Prepared => {
                if let Some((revision, _identity)) = marker {
                    if *revision != spec.target_revision() {
                        return Err(fail(
                            MigrationResumeError::ConflictingCommitMarker {
                                operation_id: spec.operation_id(),
                            },
                            completed_steps,
                        ));
                    }
                    if let Err(error) =
                        snapshot.mark_step_committed(spec.step_id(), *revision, receipt_fingerprint)
                    {
                        return Err(fail(MigrationResumeError::Journal(error), completed_steps));
                    }
                    completed_steps.push(MigrationStepCommitReceipt {
                        step_id: spec.step_id(),
                        operation_id: spec.operation_id(),
                        revision: *revision,
                        target_schema,
                        transform_fingerprint: receipt_fingerprint,
                    });
                    if let Err(error) = journal.save(&snapshot) {
                        return Err(fail(
                            MigrationResumeError::JournalStore(error),
                            completed_steps,
                        ));
                    }
                    committed_count += 1;
                } else {
                    break;
                }
            }
            MigrationRunJournalStepState::Committed {
                revision,
                transform_fingerprint,
            } => {
                let Some((marker_revision, _)) = marker else {
                    return Err(fail(
                        MigrationResumeError::CommittedMarkerMissing {
                            step_id: spec.step_id(),
                        },
                        completed_steps,
                    ));
                };
                if *marker_revision != revision || revision != spec.target_revision() {
                    return Err(fail(
                        MigrationResumeError::ConflictingCommitMarker {
                            operation_id: spec.operation_id(),
                        },
                        completed_steps,
                    ));
                }
                if receipt_fingerprint != transform_fingerprint {
                    return Err(fail(
                        MigrationResumeError::ConflictingCommitMarker {
                            operation_id: spec.operation_id(),
                        },
                        completed_steps,
                    ));
                }
                completed_steps.push(MigrationStepCommitReceipt {
                    step_id: spec.step_id(),
                    operation_id: spec.operation_id(),
                    revision,
                    target_schema,
                    transform_fingerprint: receipt_fingerprint,
                });
                committed_count += 1;
            }
        }
    }
    let expected_head = match committed_count.checked_sub(1) {
        Some(index) => match step_targets.get(index) {
            Some(target) => target.schema().revision().revision(),
            None => {
                return Err(fail_execution(
                    MigrationExecutionError::MissingStepTargets,
                    completed_steps,
                ));
            }
        },
        None => plan.source_schema_precondition().revision().revision(),
    };
    if latest != expected_head {
        return Err(fail(
            MigrationResumeError::ResumeHeadMismatch {
                expected: expected_head,
                actual: latest,
            },
            completed_steps,
        ));
    }

    if snapshot.state() == crate::MigrationRunJournalState::Completed {
        return Ok(MigrationExecutionResult {
            completed_steps,
            final_revision: latest,
            total_work_units,
            total_input_bytes,
        });
    }

    for (index, input_fingerprint) in input_fingerprints
        .iter()
        .copied()
        .enumerate()
        .skip(committed_count)
    {
        let Some(input) = step_inputs.get(index) else {
            return Err(fail_execution(
                MigrationExecutionError::StepInputCountMismatch,
                completed_steps,
            ));
        };
        let Some(step_target) = step_targets.get(index).copied() else {
            return Err(fail_execution(
                MigrationExecutionError::MissingStepTargets,
                completed_steps,
            ));
        };
        let Some(expected_transform_fingerprint) = receipt_fingerprints.get(index).copied() else {
            return Err(fail_execution(
                MigrationExecutionError::AllocationFailed,
                completed_steps,
            ));
        };
        let step_id = input.step_id();
        let operation_id = input.operation_id();
        if let Err(error) = snapshot.prepare_step(step_id) {
            return Err(fail(MigrationResumeError::Journal(error), completed_steps));
        }
        if let Err(error) = journal.save(&snapshot) {
            return Err(fail(
                MigrationResumeError::JournalStore(error),
                completed_steps,
            ));
        }
        let batch = match transformer.transform_records(
            plan,
            plan.source_schema_precondition().revision(),
            *plan.source_schema_precondition().fingerprint(),
            &input.records,
        ) {
            Ok(batch) => batch,
            Err(error) => {
                return Err(fail_execution(
                    MigrationExecutionError::Transformer(error),
                    completed_steps,
                ));
            }
        };
        let transform_fingerprint = batch.fingerprint();
        if transform_fingerprint != expected_transform_fingerprint {
            return Err(fail(
                MigrationResumeError::ConflictingCommitMarker { operation_id },
                completed_steps,
            ));
        }
        let mut staged_records = Vec::new();
        if staged_records
            .try_reserve_exact(batch.records().len().saturating_add(1))
            .is_err()
        {
            return Err(fail_execution(
                MigrationExecutionError::AllocationFailed,
                completed_steps,
            ));
        }
        for frame in batch.records() {
            let record = match decode_record_with_limits(frame, &DecoderLimits::DEFAULT) {
                Ok(record) => record.into_record(),
                Err(error) => {
                    return Err(fail_execution(
                        MigrationExecutionError::RecordCodec(error),
                        completed_steps,
                    ));
                }
            };
            staged_records.push(record);
        }
        drop(batch);
        staged_records.push(Record::MigrationStepCommitIdentity(
            MigrationStepCommitIdentity::with_input_fingerprint(
                plan.migration_id(),
                run_id,
                step_id,
                operation_id,
                input_fingerprint,
            ),
        ));

        let base_revision = backend.latest_published();
        let expected_revision = match base_revision.next_commit() {
            Ok(revision) => revision,
            Err(_) => {
                return Err(fail_execution(
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
            return Err(fail_execution(
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
                return Err(fail_execution(
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
                return Err(fail_execution(
                    MigrationExecutionError::StepValidation { step_id, error },
                    completed_steps,
                ));
            }
        };
        let revision = match validated.commit() {
            Ok(revision) => revision,
            Err(error) => {
                return Err(fail_execution(
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
            return Err(fail_execution(
                MigrationExecutionError::UnexpectedPublishedRevision {
                    step_id,
                    expected: expected_revision,
                    actual: revision,
                },
                completed_steps,
            ));
        }
        if let Err(error) = snapshot.mark_step_committed(step_id, revision, transform_fingerprint) {
            return Err(fail(MigrationResumeError::Journal(error), completed_steps));
        }
        if let Err(error) = journal.save(&snapshot) {
            return Err(fail(
                MigrationResumeError::JournalStore(error),
                completed_steps,
            ));
        }
    }

    if let Err(error) = snapshot.mark_completed() {
        return Err(fail(MigrationResumeError::Journal(error), completed_steps));
    }
    if let Err(error) = journal.save(&snapshot) {
        return Err(fail(
            MigrationResumeError::JournalStore(error),
            completed_steps,
        ));
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
        MigrationExecutionContext, MigrationExecutionError, MigrationResumeError,
        MigrationStepCommitStatus, MigrationStepInput, MigrationStepStatusError,
        MigrationStepValidationError, execute_compatible_migration,
        execute_or_resume_compatible_migration, query_migration_step_status,
    };
    use crate::ids::{
        DomainId, IdValidationError, MigrationId, MigrationRunId, MigrationStepId, OperationId,
        PredicateId, Revision, SchemaRevision,
    };
    use crate::revision_backend::{InMemoryRevisionBackend, RevisionBackend};
    use crate::wire_records::{Record, encode_record};
    use crate::{
        Cardinality, ConstraintSet, EntityTypeConstraint, JobBudget, Lifecycle, MigrationCategory,
        MigrationPlan, MigrationPlanSpec, MigrationRunJournalSnapshot, MigrationRunJournalState,
        MigrationRunJournalStepState, MigrationRunJournalStore, MigrationStepTargetSchema,
        MigrationTargetSchema, MigrationTransformer, MigrationTransformerVersion, NonEmptySet,
        PredicateDefinition, PredicateDefinitionSpec, ResolutionPolicy, SchemaDefinition,
        SchemaDefinitionId, SchemaHistoryReferenceModel, SchemaIdentityTransition, SchemaMode,
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
        two_step_plan_with_fingerprints_and_version(
            category,
            max_work_units,
            source_fingerprint,
            first_target_fingerprint,
            final_target_fingerprint,
            max_memory_bytes,
            1,
        )
    }

    fn two_step_plan_with_fingerprints_and_version(
        category: MigrationCategory,
        max_work_units: u64,
        source_fingerprint: [u8; 32],
        first_target_fingerprint: [u8; 32],
        final_target_fingerprint: [u8; 32],
        max_memory_bytes: u64,
        transformer_version: u32,
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
            transformer_version: MigrationTransformerVersion::new(transformer_version)
                .map_err(|error| error.to_string())?,
            calendar_shift: None,
            budget: JobBudget::new(max_work_units, max_memory_bytes)
                .map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())?;
        Ok((plan, steps))
    }

    fn compensation_plan() -> Result<(MigrationPlan, MigrationStepId), String> {
        let migration_id = uuid::<MigrationId>(11).map_err(|error| error.to_string())?;
        let step_id = uuid::<MigrationStepId>(12).map_err(|error| error.to_string())?;
        let predicate_id = uuid::<PredicateId>(4).map_err(|error| error.to_string())?;
        let target = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(
                Revision::new(2).map_err(|error| error.to_string())?,
            ),
            [0x44; 32],
        );
        let step_target = MigrationStepTargetSchema::new(step_id, target);
        let plan = MigrationPlan::new(MigrationPlanSpec {
            migration_id,
            category: MigrationCategory::MetadataOnly,
            source_schema: SourceSchemaPrecondition::new(
                SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
                [0x22; 32],
            ),
            target_schema: target,
            steps: vec![step_id],
            step_targets: Some(vec![step_target]),
            schema_changes: vec![
                SchemaIdentityTransition::new(
                    Some(SchemaDefinitionId::Predicate(predicate_id)),
                    Some(SchemaDefinitionId::Predicate(predicate_id)),
                    MigrationCategory::MetadataOnly,
                )
                .map_err(|error| error.to_string())?,
            ],
            transformer_version: MigrationTransformerVersion::new(1)
                .map_err(|error| error.to_string())?,
            calendar_shift: None,
            budget: JobBudget::new(10, 1024 * 1024).map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())?;
        Ok((plan, step_id))
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

    struct MemoryJournalStore {
        snapshot: Option<MigrationRunJournalSnapshot>,
        fail_after_commit_once: bool,
    }

    impl MemoryJournalStore {
        fn new() -> Self {
            Self {
                snapshot: None,
                fail_after_commit_once: false,
            }
        }

        fn failing_after_first_commit() -> Self {
            Self {
                snapshot: None,
                fail_after_commit_once: true,
            }
        }
    }

    impl MigrationRunJournalStore for MemoryJournalStore {
        type Error = &'static str;

        fn load(
            &self,
            run_id: MigrationRunId,
        ) -> Result<Option<MigrationRunJournalSnapshot>, Self::Error> {
            Ok(self
                .snapshot
                .as_ref()
                .filter(|snapshot| snapshot.spec().run_id() == run_id)
                .cloned())
        }

        fn save(&mut self, snapshot: &MigrationRunJournalSnapshot) -> Result<(), Self::Error> {
            let contains_commit = snapshot
                .steps()
                .iter()
                .any(|step| matches!(step.state(), MigrationRunJournalStepState::Committed { .. }));
            if self.fail_after_commit_once && contains_commit {
                self.fail_after_commit_once = false;
                return Err("injected journal write failure after normative commit");
            }
            self.snapshot = Some(snapshot.clone());
            Ok(())
        }
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

    #[test]
    fn resume_reconciles_commit_when_journal_update_fails_after_publication() -> Result<(), String>
    {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let run_id = run_id()?;
        let inputs = prepared_steps(step_ids)?;
        let first_operation_id = inputs
            .first()
            .map(MigrationStepInput::operation_id)
            .ok_or_else(|| String::from("first migration input is missing"))?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut journal = MemoryJournalStore::failing_after_first_commit();
        assert_eq!(
            query_migration_step_status(&backend, first_operation_id)
                .map_err(|error| format!("status query failed: {error:?}"))?,
            MigrationStepCommitStatus::NotCommitted
        );
        let failed = match execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x11; 32], transformer()?),
            inputs,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => return Err(String::from("the injected journal write must fail")),
            Err(failure) => failure,
        };
        assert!(matches!(
            failed.error(),
            MigrationResumeError::JournalStore(
                "injected journal write failure after normative commit"
            )
        ));
        assert_eq!(failed.completed_steps().len(), 1);
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);
        assert!(matches!(
            query_migration_step_status(&backend, first_operation_id)
                .map_err(|error| format!("status query failed: {error:?}"))?,
            MigrationStepCommitStatus::Committed { revision, identity }
                if revision == Revision::FIRST_COMMIT
                    && identity.operation_id() == first_operation_id
                    && identity.input_fingerprint().is_some()
        ));
        assert_eq!(
            journal
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.steps().first().map(|step| step.state())),
            Some(MigrationRunJournalStepState::Prepared)
        );
        assert_eq!(
            backend
                .read_at(backend.latest_published())
                .map_err(|error| error.to_string())?
                .filter(|(_, record)| matches!(record, Record::MigrationStepCommitIdentity(_)))
                .count(),
            1
        );

        let resumed = execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x11; 32], transformer()?),
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        )
        .map_err(|failure| format!("resume failed: {:?}", failure.error()))?;
        assert_eq!(resumed.completed_steps().len(), 2);
        assert_eq!(resumed.final_revision().value(), 2);
        assert_eq!(backend.latest_published(), resumed.final_revision());
        assert_eq!(
            backend
                .read_at(backend.latest_published())
                .map_err(|error| error.to_string())?
                .filter(|(_, record)| matches!(record, Record::MigrationStepCommitIdentity(_)))
                .count(),
            2
        );
        assert_eq!(
            journal
                .snapshot
                .as_ref()
                .map(MigrationRunJournalSnapshot::state),
            Some(MigrationRunJournalState::Completed)
        );
        Ok(())
    }

    #[test]
    fn resume_rejects_changed_input_fingerprint_before_another_commit() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let run_id = run_id()?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut journal = MemoryJournalStore::failing_after_first_commit();
        let _ = execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x11; 32], transformer()?),
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        );
        let mut changed_inputs = prepared_steps(step_ids)?;
        changed_inputs
            .get_mut(1)
            .ok_or_else(|| String::from("second migration input is missing"))?
            .records = vec![vec![0x01]];
        let failure = match execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x11; 32], transformer()?),
            changed_inputs,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => return Err(String::from("changed step input must be rejected")),
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationResumeError::Journal(crate::MigrationRunJournalError::RunIdentityMismatch)
        ));
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);
        assert_eq!(
            backend
                .read_at(backend.latest_published())
                .map_err(|error| error.to_string())?
                .filter(|(_, record)| matches!(record, Record::MigrationStepCommitIdentity(_)))
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn start_rejects_source_and_transformer_version_mismatch_before_journaling()
    -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let run_id = run_id()?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut journal = MemoryJournalStore::new();
        let source_failure = match execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x99; 32], transformer()?),
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => return Err(String::from("changed source fingerprint must fail")),
            Err(failure) => failure,
        };
        assert!(matches!(
            source_failure.error(),
            MigrationResumeError::Execution(MigrationExecutionError::Plan(
                crate::MigrationPlanError::SourceSchemaPreconditionMismatch
            ))
        ));
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(journal.snapshot.is_none());

        let (versioned_plan, versioned_steps) = two_step_plan_with_fingerprints_and_version(
            MigrationCategory::Additive,
            10,
            [0x11; 32],
            [0x22; 32],
            [0x33; 32],
            1024 * 1024,
            2,
        )?;
        let version_failure = match execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(
                &versioned_plan,
                uuid::<MigrationRunId>(15).map_err(|error| error.to_string())?,
                [0x11; 32],
                transformer()?,
            ),
            prepared_steps(versioned_steps)?,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => return Err(String::from("transformer version mismatch must fail")),
            Err(failure) => failure,
        };
        assert!(matches!(
            version_failure.error(),
            MigrationResumeError::Execution(MigrationExecutionError::Plan(
                crate::MigrationPlanError::TransformerVersionMismatch {
                    planned,
                    actual
                }
            )) if planned.value() == 2 && actual.value() == 1
        ));
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(journal.snapshot.is_none());
        Ok(())
    }

    #[test]
    fn resume_rejects_changed_source_fingerprint_before_another_commit() -> Result<(), String> {
        let (plan, step_ids) = two_step_plan(MigrationCategory::Additive, 10)?;
        let run_id = run_id()?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut journal = MemoryJournalStore::failing_after_first_commit();
        let interrupted = execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x11; 32], transformer()?),
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        );
        assert!(interrupted.is_err());
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);

        let failure = match execute_or_resume_compatible_migration(
            &mut backend,
            &mut journal,
            MigrationExecutionContext::new(&plan, run_id, [0x99; 32], transformer()?),
            prepared_steps(step_ids)?,
            |_, _, _, _| Ok::<(), String>(()),
        ) {
            Ok(_) => {
                return Err(String::from(
                    "changed source fingerprint must fail on resume",
                ));
            }
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            MigrationResumeError::Execution(MigrationExecutionError::Plan(
                crate::MigrationPlanError::SourceSchemaPreconditionMismatch
            ))
        ));
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);
        assert_eq!(
            backend
                .read_at(backend.latest_published())
                .map_err(|error| error.to_string())?
                .filter(|(_, record)| matches!(record, Record::MigrationStepCommitIdentity(_)))
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn status_query_rejects_duplicate_operation_markers() -> Result<(), String> {
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let identity = crate::MigrationStepCommitIdentity::new(
            uuid::<MigrationId>(1).map_err(|error| error.to_string())?,
            run_id()?,
            uuid::<MigrationStepId>(2).map_err(|error| error.to_string())?,
            uuid::<OperationId>(3).map_err(|error| error.to_string())?,
        );
        backend
            .publish(vec![Record::MigrationStepCommitIdentity(identity)])
            .map_err(|error| error.to_string())?;
        backend
            .publish(vec![Record::MigrationStepCommitIdentity(identity)])
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            query_migration_step_status(&backend, identity.operation_id()),
            Err(MigrationStepStatusError::DuplicateOperationId(operation_id))
                if operation_id == identity.operation_id()
        ));
        Ok(())
    }

    #[test]
    fn compensation_is_a_new_migration_appended_after_the_committed_prefix() -> Result<(), String> {
        let (original_plan, original_steps) = two_step_plan(MigrationCategory::Additive, 10)?;
        let original_second_step = original_steps
            .get(1)
            .copied()
            .ok_or_else(|| String::from("original second step is missing"))?;
        let original_run_id = run_id()?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut original_journal = MemoryJournalStore::new();
        let partial = match execute_or_resume_compatible_migration(
            &mut backend,
            &mut original_journal,
            MigrationExecutionContext::new(
                &original_plan,
                original_run_id,
                [0x11; 32],
                transformer()?,
            ),
            prepared_steps(original_steps)?,
            |_, _, _, target| {
                if target.step_id() == original_second_step {
                    Err("stop after the first original step")
                } else {
                    Ok(())
                }
            },
        ) {
            Ok(_) => return Err(String::from("the original second step must stop")),
            Err(failure) => failure,
        };
        assert_eq!(partial.completed_steps().len(), 1);
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);

        let (compensation, compensation_step) = compensation_plan()?;
        let compensation_run_id = uuid::<MigrationRunId>(14).map_err(|error| error.to_string())?;
        let compensation_operation_id =
            uuid::<OperationId>(13).map_err(|error| error.to_string())?;
        let mut compensation_journal = MemoryJournalStore::new();
        let result = execute_or_resume_compatible_migration(
            &mut backend,
            &mut compensation_journal,
            MigrationExecutionContext::new(
                &compensation,
                compensation_run_id,
                [0x22; 32],
                transformer()?,
            ),
            vec![MigrationStepInput::new(
                compensation_step,
                compensation_operation_id,
                Vec::new(),
            )],
            |_, _, _, _| Ok::<(), String>(()),
        )
        .map_err(|failure| format!("compensation plan failed: {:?}", failure.error()))?;

        assert_eq!(
            result.final_revision(),
            Revision::new(2).map_err(|e| e.to_string())?
        );
        assert_ne!(compensation.migration_id(), original_plan.migration_id());
        let markers = backend
            .read_at(result.final_revision())
            .map_err(|error| error.to_string())?
            .filter_map(|(revision, record)| match record {
                Record::MigrationStepCommitIdentity(identity) => Some((revision, *identity)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(markers.len(), 2);
        let first_marker = markers
            .first()
            .ok_or_else(|| String::from("first committed marker is missing"))?;
        let second_marker = markers
            .get(1)
            .ok_or_else(|| String::from("second committed marker is missing"))?;
        assert_eq!(first_marker.0, Revision::FIRST_COMMIT);
        assert_eq!(first_marker.1.migration_id(), original_plan.migration_id());
        assert_eq!(
            second_marker.0,
            Revision::new(2).map_err(|e| e.to_string())?
        );
        assert_eq!(second_marker.1.migration_id(), compensation.migration_id());
        Ok(())
    }
}
