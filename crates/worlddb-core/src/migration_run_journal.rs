//! Recoverable coordination state for migration runs.
//!
//! Journal snapshots are deliberately not `Record` variants. Committed schema
//! and data records remain normative; this state only helps a runner resume.

use std::collections::BTreeSet;
use std::fmt;

use crate::ids::{
    DomainId, MigrationId, MigrationRunId, MigrationStepId, OperationId, Revision, SchemaRevision,
};
use crate::migration_transform::MigrationTransformFingerprint;
use crate::{MigrationPlan, MigrationPlanFingerprint, MigrationTransformerVersion};

const JOURNAL_MAGIC: [u8; 8] = *b"WDBMRJ\0\x01";
const JOURNAL_DIGEST_BYTES: usize = 32;
const JOURNAL_HEADER_BYTES: usize = 8 + 16 + 16 + 32 + 8 + 32 + 4 + 1 + 4;
const JOURNAL_STEP_BYTES: usize = 16 + 16 + 32 + 8 + 1;
const JOURNAL_COMMITTED_BYTES: usize = 8 + 32;
/// Maximum encoded migration-run journal snapshot accepted from storage.
pub const MAX_MIGRATION_RUN_JOURNAL_BYTES: usize = 16 * 1024 * 1024;
/// Maximum step count accepted in one migration-run journal snapshot.
pub const MAX_MIGRATION_RUN_JOURNAL_STEPS: usize = 100_000;

/// Immutable per-step identity and input binding persisted for one run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationRunJournalStepSpec {
    step_id: MigrationStepId,
    operation_id: OperationId,
    input_fingerprint: [u8; 32],
    target_revision: Revision,
}

impl MigrationRunJournalStepSpec {
    /// Binds a plan step, unique operation identity, exact input, and target revision.
    #[must_use]
    pub const fn new(
        step_id: MigrationStepId,
        operation_id: OperationId,
        input_fingerprint: [u8; 32],
        target_revision: Revision,
    ) -> Self {
        Self {
            step_id,
            operation_id,
            input_fingerprint,
            target_revision,
        }
    }

    /// Stable migration-plan step identity.
    #[must_use]
    pub const fn step_id(self) -> MigrationStepId {
        self.step_id
    }

    /// Idempotency identity for the step's atomic record batch.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Fingerprint of the exact ordered canonical source frames.
    #[must_use]
    pub const fn input_fingerprint(self) -> [u8; 32] {
        self.input_fingerprint
    }

    /// Revision this step is expected to publish.
    #[must_use]
    pub const fn target_revision(self) -> Revision {
        self.target_revision
    }
}

/// Immutable metadata that must match before a run can start or resume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationRunJournalSpec {
    migration_id: MigrationId,
    run_id: MigrationRunId,
    plan_fingerprint: MigrationPlanFingerprint,
    source_revision: SchemaRevision,
    source_fingerprint: [u8; 32],
    transformer_version: MigrationTransformerVersion,
    steps: Vec<MigrationRunJournalStepSpec>,
}

impl MigrationRunJournalSpec {
    /// Creates a complete immutable run binding and rejects duplicate identities.
    pub fn new(
        migration_id: MigrationId,
        run_id: MigrationRunId,
        plan_fingerprint: MigrationPlanFingerprint,
        source_revision: SchemaRevision,
        source_fingerprint: [u8; 32],
        transformer_version: MigrationTransformerVersion,
        steps: Vec<MigrationRunJournalStepSpec>,
    ) -> Result<Self, MigrationRunJournalError> {
        if steps.is_empty() {
            return Err(MigrationRunJournalError::EmptyRun);
        }
        let mut seen_steps = BTreeSet::new();
        let mut seen_operations = BTreeSet::new();
        for step in &steps {
            if !seen_steps.insert(step.step_id()) {
                return Err(MigrationRunJournalError::DuplicateStepId(step.step_id()));
            }
            if !seen_operations.insert(step.operation_id()) {
                return Err(MigrationRunJournalError::DuplicateOperationId(
                    step.operation_id(),
                ));
            }
        }
        Ok(Self {
            migration_id,
            run_id,
            plan_fingerprint,
            source_revision,
            source_fingerprint,
            transformer_version,
            steps,
        })
    }

    /// Validates the persisted plan identity and transformer version.
    pub fn validate_plan(
        &self,
        plan: &MigrationPlan,
        transformer_version: MigrationTransformerVersion,
    ) -> Result<(), MigrationRunJournalError> {
        if self.migration_id != plan.migration_id() {
            return Err(MigrationRunJournalError::MigrationIdentityMismatch);
        }
        if self.plan_fingerprint != plan.fingerprint() {
            return Err(MigrationRunJournalError::PlanFingerprintMismatch);
        }
        if self.source_revision != plan.source_schema_precondition().revision()
            || self.source_fingerprint != *plan.source_schema_precondition().fingerprint()
        {
            return Err(MigrationRunJournalError::SourcePreconditionMismatch);
        }
        if self.transformer_version != transformer_version
            || self.transformer_version != plan.transformer_version()
        {
            return Err(MigrationRunJournalError::TransformerVersionMismatch);
        }
        let Some(targets) = plan.step_targets() else {
            return Err(MigrationRunJournalError::StepSetMismatch);
        };
        if self.steps.len() != plan.steps().len() || self.steps.len() != targets.len() {
            return Err(MigrationRunJournalError::StepSetMismatch);
        }
        for ((step, planned_id), target) in self.steps.iter().zip(plan.steps()).zip(targets) {
            if step.step_id() != *planned_id
                || target.step_id() != step.step_id()
                || step.target_revision() != target.schema().revision().revision()
            {
                return Err(MigrationRunJournalError::StepSetMismatch);
            }
        }
        Ok(())
    }

    /// Logical migration identity.
    #[must_use]
    pub const fn migration_id(&self) -> MigrationId {
        self.migration_id
    }

    /// One concrete migration execution identity.
    #[must_use]
    pub const fn run_id(&self) -> MigrationRunId {
        self.run_id
    }

    /// Canonical immutable plan fingerprint.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> MigrationPlanFingerprint {
        self.plan_fingerprint
    }

    /// Source schema revision fixed by the immutable plan.
    #[must_use]
    pub const fn source_revision(&self) -> SchemaRevision {
        self.source_revision
    }

    /// Source schema fingerprint fixed by the immutable plan.
    #[must_use]
    pub const fn source_fingerprint(&self) -> [u8; 32] {
        self.source_fingerprint
    }

    /// Version of the implementation allowed to execute this plan.
    #[must_use]
    pub const fn transformer_version(&self) -> MigrationTransformerVersion {
        self.transformer_version
    }

    /// Ordered input and operation bindings for every planned step.
    #[must_use]
    pub fn steps(&self) -> &[MigrationRunJournalStepSpec] {
        &self.steps
    }
}

/// Durable state of one step in the migration coordination journal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationRunJournalStepState {
    /// No commit attempt was durably prepared.
    Pending,
    /// Input and OperationId are durable; resume must query committed history before retry.
    Prepared,
    /// The normative transaction is confirmed by its OperationId marker and revision.
    Committed {
        revision: Revision,
        transform_fingerprint: MigrationTransformFingerprint,
    },
}

/// Lifecycle state of a coordination journal snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationRunJournalState {
    /// At least one planned step remains to execute or reconcile.
    Running,
    /// Every planned step has a confirmed normative commit.
    Completed,
}

/// Immutable step identity plus its latest monotone journal state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationRunJournalStep {
    spec: MigrationRunJournalStepSpec,
    state: MigrationRunJournalStepState,
}

impl MigrationRunJournalStep {
    /// Exact plan/input/operation binding for this step.
    #[must_use]
    pub const fn spec(self) -> MigrationRunJournalStepSpec {
        self.spec
    }

    /// Current recoverable coordination state.
    #[must_use]
    pub const fn state(self) -> MigrationRunJournalStepState {
        self.state
    }
}

/// Recoverable run state stored outside normative WorldDB History.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationRunJournalSnapshot {
    spec: MigrationRunJournalSpec,
    state: MigrationRunJournalState,
    steps: Vec<MigrationRunJournalStep>,
}

impl MigrationRunJournalSnapshot {
    /// Starts a new coordination snapshot with all steps pending.
    pub fn start(spec: MigrationRunJournalSpec) -> Result<Self, MigrationRunJournalError> {
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(spec.steps.len())
            .map_err(|_| MigrationRunJournalError::AllocationFailed)?;
        steps.extend(
            spec.steps
                .iter()
                .copied()
                .map(|spec| MigrationRunJournalStep {
                    spec,
                    state: MigrationRunJournalStepState::Pending,
                }),
        );
        Ok(Self {
            spec,
            state: MigrationRunJournalState::Running,
            steps,
        })
    }

    /// Durably marks the next ordered step prepared before its data commit begins.
    pub fn prepare_step(
        &mut self,
        step_id: MigrationStepId,
    ) -> Result<(), MigrationRunJournalError> {
        if self.state != MigrationRunJournalState::Running {
            return Err(MigrationRunJournalError::RunAlreadyCompleted);
        }
        let next = self
            .steps
            .iter_mut()
            .find(|step| !matches!(step.state, MigrationRunJournalStepState::Committed { .. }))
            .ok_or(MigrationRunJournalError::RunAlreadyCompleted)?;
        if next.spec.step_id() != step_id {
            return Err(MigrationRunJournalError::OutOfOrderStep {
                expected: next.spec.step_id(),
                actual: step_id,
            });
        }
        match next.state {
            MigrationRunJournalStepState::Pending => {
                next.state = MigrationRunJournalStepState::Prepared;
                Ok(())
            }
            MigrationRunJournalStepState::Prepared => Ok(()),
            MigrationRunJournalStepState::Committed { .. } => {
                Err(MigrationRunJournalError::StepAlreadyCommitted(step_id))
            }
        }
    }

    /// Reconciles a prepared step with its committed OperationId marker.
    pub fn mark_step_committed(
        &mut self,
        step_id: MigrationStepId,
        revision: Revision,
        transform_fingerprint: MigrationTransformFingerprint,
    ) -> Result<(), MigrationRunJournalError> {
        if self.state != MigrationRunJournalState::Running {
            return Err(MigrationRunJournalError::RunAlreadyCompleted);
        }
        let step = self
            .steps
            .iter_mut()
            .find(|step| step.spec.step_id() == step_id)
            .ok_or(MigrationRunJournalError::UnknownStep(step_id))?;
        if revision != step.spec.target_revision() {
            return Err(MigrationRunJournalError::CommittedRevisionMismatch {
                expected: step.spec.target_revision(),
                actual: revision,
            });
        }
        match step.state {
            MigrationRunJournalStepState::Prepared => {
                step.state = MigrationRunJournalStepState::Committed {
                    revision,
                    transform_fingerprint,
                };
                Ok(())
            }
            MigrationRunJournalStepState::Committed {
                revision: prior_revision,
                transform_fingerprint: prior_fingerprint,
            } if prior_revision == revision && prior_fingerprint == transform_fingerprint => Ok(()),
            MigrationRunJournalStepState::Committed { .. } => {
                Err(MigrationRunJournalError::CommitReceiptMismatch(step_id))
            }
            MigrationRunJournalStepState::Pending => {
                Err(MigrationRunJournalError::StepNotPrepared(step_id))
            }
        }
    }

    /// Marks the run complete only after every normative step commit is confirmed.
    pub fn mark_completed(&mut self) -> Result<(), MigrationRunJournalError> {
        if self
            .steps
            .iter()
            .any(|step| !matches!(step.state, MigrationRunJournalStepState::Committed { .. }))
        {
            return Err(MigrationRunJournalError::IncompleteRun);
        }
        self.state = MigrationRunJournalState::Completed;
        Ok(())
    }

    /// Checks that this snapshot is an allowed durable successor of an earlier one.
    pub fn validate_successor(&self, previous: &Self) -> Result<(), MigrationRunJournalError> {
        if self.spec != previous.spec || self.steps.len() != previous.steps.len() {
            return Err(MigrationRunJournalError::RunIdentityMismatch);
        }
        if previous
            .steps
            .iter()
            .zip(&self.steps)
            .any(|(prior, next)| prior.spec != next.spec)
        {
            return Err(MigrationRunJournalError::RunIdentityMismatch);
        }
        if previous.state == MigrationRunJournalState::Completed {
            return if self == previous {
                Ok(())
            } else {
                Err(MigrationRunJournalError::InvalidStateTransition)
            };
        }

        let changed_index = previous
            .steps
            .iter()
            .zip(&self.steps)
            .enumerate()
            .filter_map(|(index, (prior, next))| (prior.state != next.state).then_some(index))
            .try_fold(None, |found, index| {
                if found.is_some() {
                    Err(MigrationRunJournalError::InvalidStateTransition)
                } else {
                    Ok(Some(index))
                }
            })?;

        if self.state == MigrationRunJournalState::Completed {
            if changed_index.is_some() {
                return Err(MigrationRunJournalError::InvalidStateTransition);
            }
            let mut expected = previous.clone();
            expected.mark_completed()?;
            return if expected == *self {
                Ok(())
            } else {
                Err(MigrationRunJournalError::InvalidStateTransition)
            };
        }
        if self.state != MigrationRunJournalState::Running {
            return Err(MigrationRunJournalError::InvalidStateTransition);
        }
        let Some(changed_index) = changed_index else {
            return if self == previous {
                Ok(())
            } else {
                Err(MigrationRunJournalError::InvalidStateTransition)
            };
        };
        let next_pending = previous
            .steps
            .iter()
            .position(|step| !matches!(step.state, MigrationRunJournalStepState::Committed { .. }));
        if next_pending != Some(changed_index) {
            return Err(MigrationRunJournalError::InvalidStateTransition);
        }
        let prior = previous
            .steps
            .get(changed_index)
            .ok_or(MigrationRunJournalError::InvalidStateTransition)?;
        let next = self
            .steps
            .get(changed_index)
            .ok_or(MigrationRunJournalError::InvalidStateTransition)?;
        let mut expected = previous.clone();
        match (prior.state, next.state) {
            (MigrationRunJournalStepState::Pending, MigrationRunJournalStepState::Prepared) => {
                expected.prepare_step(prior.spec.step_id())?;
            }
            (
                MigrationRunJournalStepState::Prepared,
                MigrationRunJournalStepState::Committed {
                    revision,
                    transform_fingerprint,
                },
            ) => {
                expected.mark_step_committed(
                    prior.spec.step_id(),
                    revision,
                    transform_fingerprint,
                )?;
            }
            _ => return Err(MigrationRunJournalError::InvalidStateTransition),
        }
        if expected == *self {
            Ok(())
        } else {
            Err(MigrationRunJournalError::InvalidStateTransition)
        }
    }

    /// Run binding whose plan and version are required for every retry.
    #[must_use]
    pub fn spec(&self) -> &MigrationRunJournalSpec {
        &self.spec
    }

    /// Current run state.
    #[must_use]
    pub const fn state(&self) -> MigrationRunJournalState {
        self.state
    }

    /// Ordered per-step state.
    #[must_use]
    pub fn steps(&self) -> &[MigrationRunJournalStep] {
        &self.steps
    }

    /// Encodes the recoverable snapshot with a versioned fixed-field format and checksum.
    pub fn encode(&self) -> Result<Vec<u8>, MigrationRunJournalCodecError> {
        if self.steps.is_empty() || self.steps.len() > MAX_MIGRATION_RUN_JOURNAL_STEPS {
            return Err(MigrationRunJournalCodecError::ResourceLimit);
        }
        let mut encoded_length = JOURNAL_HEADER_BYTES
            .checked_add(JOURNAL_DIGEST_BYTES)
            .and_then(|length| {
                length.checked_add(self.steps.len().checked_mul(JOURNAL_STEP_BYTES)?)
            })
            .ok_or(MigrationRunJournalCodecError::ResourceLimit)?;
        for step in &self.steps {
            if matches!(step.state, MigrationRunJournalStepState::Committed { .. }) {
                encoded_length = encoded_length
                    .checked_add(JOURNAL_COMMITTED_BYTES)
                    .ok_or(MigrationRunJournalCodecError::ResourceLimit)?;
            }
        }
        if encoded_length > MAX_MIGRATION_RUN_JOURNAL_BYTES {
            return Err(MigrationRunJournalCodecError::ResourceLimit);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(encoded_length)
            .map_err(|_| MigrationRunJournalCodecError::AllocationFailed)?;
        bytes.extend_from_slice(&JOURNAL_MAGIC);
        bytes.extend_from_slice(&self.spec.migration_id.to_bytes());
        bytes.extend_from_slice(&self.spec.run_id.to_bytes());
        bytes.extend_from_slice(self.spec.plan_fingerprint.as_bytes());
        bytes.extend_from_slice(&self.spec.source_revision.revision().value().to_le_bytes());
        bytes.extend_from_slice(&self.spec.source_fingerprint);
        bytes.extend_from_slice(&self.spec.transformer_version.value().to_le_bytes());
        bytes.push(match self.state {
            MigrationRunJournalState::Running => 0,
            MigrationRunJournalState::Completed => 1,
        });
        let step_count = u32::try_from(self.steps.len())
            .map_err(|_| MigrationRunJournalCodecError::ResourceLimit)?;
        bytes.extend_from_slice(&step_count.to_le_bytes());
        for step in &self.steps {
            bytes.extend_from_slice(&step.spec.step_id().to_bytes());
            bytes.extend_from_slice(&step.spec.operation_id().to_bytes());
            bytes.extend_from_slice(&step.spec.input_fingerprint());
            bytes.extend_from_slice(&step.spec.target_revision().value().to_le_bytes());
            match step.state {
                MigrationRunJournalStepState::Pending => bytes.push(0),
                MigrationRunJournalStepState::Prepared => bytes.push(1),
                MigrationRunJournalStepState::Committed {
                    revision,
                    transform_fingerprint,
                } => {
                    bytes.push(2);
                    bytes.extend_from_slice(&revision.value().to_le_bytes());
                    bytes.extend_from_slice(transform_fingerprint.as_bytes());
                }
            }
        }
        let digest = blake3::hash(&bytes);
        bytes.extend_from_slice(digest.as_bytes());
        if bytes.len() != encoded_length {
            return Err(MigrationRunJournalCodecError::InvalidEncoding);
        }
        Ok(bytes)
    }

    /// Decodes and validates a complete versioned snapshot, rejecting torn or reordered state.
    pub fn decode(bytes: &[u8]) -> Result<Self, MigrationRunJournalCodecError> {
        if bytes.len() > MAX_MIGRATION_RUN_JOURNAL_BYTES
            || bytes.len() < JOURNAL_HEADER_BYTES + JOURNAL_DIGEST_BYTES
        {
            return Err(MigrationRunJournalCodecError::ResourceLimit);
        }
        if bytes.get(..JOURNAL_MAGIC.len()) != Some(JOURNAL_MAGIC.as_slice()) {
            return Err(MigrationRunJournalCodecError::InvalidMagic);
        }
        let body_length = bytes
            .len()
            .checked_sub(JOURNAL_DIGEST_BYTES)
            .ok_or(MigrationRunJournalCodecError::Truncated)?;
        let (body, digest) = bytes.split_at(body_length);
        if digest != blake3::hash(body).as_bytes() {
            return Err(MigrationRunJournalCodecError::ChecksumMismatch);
        }

        let mut cursor = JournalCursor::new(body);
        if cursor.take::<8>()? != JOURNAL_MAGIC {
            return Err(MigrationRunJournalCodecError::InvalidMagic);
        }
        let migration_id = decode_id::<MigrationId>(&mut cursor)?;
        let run_id = decode_id::<MigrationRunId>(&mut cursor)?;
        let plan_fingerprint = MigrationPlanFingerprint::from_bytes(cursor.take::<32>()?);
        let source_revision = decode_revision(&mut cursor)?;
        let source_fingerprint = cursor.take::<32>()?;
        let transformer_version = MigrationTransformerVersion::new(cursor.read_u32()?)
            .map_err(|_| MigrationRunJournalCodecError::InvalidTransformerVersion)?;
        let run_state = match cursor.read_u8()? {
            0 => MigrationRunJournalState::Running,
            1 => MigrationRunJournalState::Completed,
            _ => return Err(MigrationRunJournalCodecError::InvalidRunState),
        };
        let step_count = usize::try_from(cursor.read_u32()?)
            .map_err(|_| MigrationRunJournalCodecError::ResourceLimit)?;
        if step_count == 0
            || step_count > MAX_MIGRATION_RUN_JOURNAL_STEPS
            || step_count
                .checked_mul(JOURNAL_STEP_BYTES)
                .is_none_or(|minimum| minimum > cursor.remaining())
        {
            return Err(MigrationRunJournalCodecError::ResourceLimit);
        }
        let mut step_specs = Vec::new();
        let mut step_states = Vec::new();
        step_specs
            .try_reserve_exact(step_count)
            .map_err(|_| MigrationRunJournalCodecError::AllocationFailed)?;
        step_states
            .try_reserve_exact(step_count)
            .map_err(|_| MigrationRunJournalCodecError::AllocationFailed)?;
        for _ in 0..step_count {
            let step_id = decode_id::<MigrationStepId>(&mut cursor)?;
            let operation_id = decode_id::<OperationId>(&mut cursor)?;
            let input_fingerprint = cursor.take::<32>()?;
            let target_revision = decode_revision(&mut cursor)?;
            let state = match cursor.read_u8()? {
                0 => MigrationRunJournalStepState::Pending,
                1 => MigrationRunJournalStepState::Prepared,
                2 => MigrationRunJournalStepState::Committed {
                    revision: decode_revision(&mut cursor)?,
                    transform_fingerprint: MigrationTransformFingerprint::from_bytes(
                        cursor.take::<32>()?,
                    ),
                },
                _ => return Err(MigrationRunJournalCodecError::InvalidStepState),
            };
            step_specs.push(MigrationRunJournalStepSpec::new(
                step_id,
                operation_id,
                input_fingerprint,
                target_revision,
            ));
            step_states.push(state);
        }
        if cursor.remaining() != 0 {
            return Err(MigrationRunJournalCodecError::TrailingData);
        }
        let spec = MigrationRunJournalSpec::new(
            migration_id,
            run_id,
            plan_fingerprint,
            SchemaRevision::from_published_revision(source_revision),
            source_fingerprint,
            transformer_version,
            step_specs,
        )
        .map_err(MigrationRunJournalCodecError::InvalidRunSpec)?;
        let mut snapshot =
            Self::start(spec).map_err(MigrationRunJournalCodecError::InvalidRunSpec)?;
        for (index, state) in step_states.into_iter().enumerate() {
            let Some(step_id) = snapshot.steps.get(index).map(|step| step.spec.step_id()) else {
                return Err(MigrationRunJournalCodecError::InvalidEncoding);
            };
            match state {
                MigrationRunJournalStepState::Pending => {}
                MigrationRunJournalStepState::Prepared => snapshot
                    .prepare_step(step_id)
                    .map_err(MigrationRunJournalCodecError::InvalidTransition)?,
                MigrationRunJournalStepState::Committed {
                    revision,
                    transform_fingerprint,
                } => {
                    snapshot
                        .prepare_step(step_id)
                        .map_err(MigrationRunJournalCodecError::InvalidTransition)?;
                    snapshot
                        .mark_step_committed(step_id, revision, transform_fingerprint)
                        .map_err(MigrationRunJournalCodecError::InvalidTransition)?;
                }
            }
        }
        if run_state == MigrationRunJournalState::Completed {
            snapshot
                .mark_completed()
                .map_err(MigrationRunJournalCodecError::InvalidTransition)?;
        }
        Ok(snapshot)
    }
}

fn decode_id<T: DomainId>(
    cursor: &mut JournalCursor<'_>,
) -> Result<T, MigrationRunJournalCodecError> {
    T::try_from_bytes(cursor.take::<16>()?)
        .map_err(|_| MigrationRunJournalCodecError::InvalidIdentity)
}

fn decode_revision(
    cursor: &mut JournalCursor<'_>,
) -> Result<Revision, MigrationRunJournalCodecError> {
    Revision::new(cursor.read_u64()?).map_err(|_| MigrationRunJournalCodecError::InvalidRevision)
}

struct JournalCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> JournalCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], MigrationRunJournalCodecError> {
        let end = self
            .position
            .checked_add(N)
            .ok_or(MigrationRunJournalCodecError::Truncated)?;
        let source = self
            .bytes
            .get(self.position..end)
            .ok_or(MigrationRunJournalCodecError::Truncated)?;
        let mut value = [0_u8; N];
        value.copy_from_slice(source);
        self.position = end;
        Ok(value)
    }

    fn read_u8(&mut self) -> Result<u8, MigrationRunJournalCodecError> {
        Ok(self.take::<1>()?[0])
    }

    fn read_u32(&mut self) -> Result<u32, MigrationRunJournalCodecError> {
        Ok(u32::from_le_bytes(self.take::<4>()?))
    }

    fn read_u64(&mut self) -> Result<u64, MigrationRunJournalCodecError> {
        Ok(u64::from_le_bytes(self.take::<8>()?))
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }
}

/// Checksum, format, identity, or transition error while reading a run-journal snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationRunJournalCodecError {
    /// Snapshot is shorter or longer than the supported bounded format.
    ResourceLimit,
    /// The byte buffer could not be allocated within its declared bound.
    AllocationFailed,
    /// The journal magic or version is not supported.
    InvalidMagic,
    /// A fixed-width field ended before all bytes were present.
    Truncated,
    /// The trailing BLAKE3 digest does not match the snapshot body.
    ChecksumMismatch,
    /// A UUID field is not a valid persistent WorldDB identity.
    InvalidIdentity,
    /// A revision field uses a reserved or otherwise invalid value.
    InvalidRevision,
    /// The transformer version is zero or invalid.
    InvalidTransformerVersion,
    /// The run-state tag is not recognized.
    InvalidRunState,
    /// The step-state tag is not recognized.
    InvalidStepState,
    /// The snapshot fields do not form a valid immutable run specification.
    InvalidRunSpec(MigrationRunJournalError),
    /// The ordered step states violate the journal transition rules.
    InvalidTransition(MigrationRunJournalError),
    /// Bytes remain after the declared steps have been decoded.
    TrailingData,
    /// Internal encoded length did not match the fixed-field calculation.
    InvalidEncoding,
}

impl fmt::Display for MigrationRunJournalCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceLimit => {
                formatter.write_str("migration run journal exceeds codec limits")
            }
            Self::AllocationFailed => {
                formatter.write_str("migration run journal allocation failed")
            }
            Self::InvalidMagic => {
                formatter.write_str("migration run journal magic or version is invalid")
            }
            Self::Truncated => formatter.write_str("migration run journal is truncated"),
            Self::ChecksumMismatch => {
                formatter.write_str("migration run journal checksum does not match")
            }
            Self::InvalidIdentity => {
                formatter.write_str("migration run journal identity is invalid")
            }
            Self::InvalidRevision => {
                formatter.write_str("migration run journal revision is invalid")
            }
            Self::InvalidTransformerVersion => {
                formatter.write_str("migration run journal transformer version is invalid")
            }
            Self::InvalidRunState => {
                formatter.write_str("migration run journal run state is invalid")
            }
            Self::InvalidStepState => {
                formatter.write_str("migration run journal step state is invalid")
            }
            Self::InvalidRunSpec(error) => write!(
                formatter,
                "migration run journal specification is invalid: {error}"
            ),
            Self::InvalidTransition(error) => write!(
                formatter,
                "migration run journal transition is invalid: {error}"
            ),
            Self::TrailingData => {
                formatter.write_str("migration run journal contains trailing bytes")
            }
            Self::InvalidEncoding => {
                formatter.write_str("migration run journal encoded length is inconsistent")
            }
        }
    }
}

impl std::error::Error for MigrationRunJournalCodecError {}

/// Persistence boundary for recoverable coordination snapshots.
pub trait MigrationRunJournalStore {
    /// Backend-specific durable journal failure.
    type Error;

    /// Loads one run snapshot; absence proves the run has not been journaled.
    fn load(
        &self,
        run_id: MigrationRunId,
    ) -> Result<Option<MigrationRunJournalSnapshot>, Self::Error>;

    /// Atomically persists a complete validated snapshot outside normative History.
    fn save(&mut self, snapshot: &MigrationRunJournalSnapshot) -> Result<(), Self::Error>;
}

/// Invalid run identity, step ordering, fingerprint binding, or journal transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationRunJournalError {
    /// A migration run must contain at least one planned step.
    EmptyRun,
    /// A planned step identity appears more than once.
    DuplicateStepId(MigrationStepId),
    /// An OperationId is reused within one run.
    DuplicateOperationId(OperationId),
    /// Allocation of a bounded journal snapshot failed.
    AllocationFailed,
    /// Journal migration identity differs from the supplied plan.
    MigrationIdentityMismatch,
    /// Journal plan fingerprint differs from the supplied immutable plan.
    PlanFingerprintMismatch,
    /// Journal source precondition differs from the supplied immutable plan.
    SourcePreconditionMismatch,
    /// Journal transformer version differs from the plan or implementation.
    TransformerVersionMismatch,
    /// Journal ordered steps or target revisions differ from the plan.
    StepSetMismatch,
    /// A run ID already exists with a different immutable binding.
    RunIdentityMismatch,
    /// The requested step does not match the next ordered pending step.
    OutOfOrderStep {
        expected: MigrationStepId,
        actual: MigrationStepId,
    },
    /// The named step is absent from the immutable run.
    UnknownStep(MigrationStepId),
    /// A committed step marker names an unexpected database revision.
    CommittedRevisionMismatch {
        expected: Revision,
        actual: Revision,
    },
    /// A committed step was not durably marked Prepared first.
    StepNotPrepared(MigrationStepId),
    /// A repeated commit receipt differs from the persisted receipt.
    CommitReceiptMismatch(MigrationStepId),
    /// The caller tried to prepare an already committed step.
    StepAlreadyCommitted(MigrationStepId),
    /// A completed run cannot accept another step transition.
    RunAlreadyCompleted,
    /// Completion was requested while a step is pending or unresolved.
    IncompleteRun,
    /// A persisted state change skips or reverses a legal transition.
    InvalidStateTransition,
}

impl fmt::Display for MigrationRunJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRun => formatter.write_str("migration run journal has no steps"),
            Self::DuplicateStepId(id) => write!(formatter, "duplicate journal step {id}"),
            Self::DuplicateOperationId(id) => {
                write!(formatter, "duplicate journal OperationId {id}")
            }
            Self::AllocationFailed => {
                formatter.write_str("migration run journal allocation failed")
            }
            Self::MigrationIdentityMismatch => {
                formatter.write_str("migration run journal has another MigrationId")
            }
            Self::PlanFingerprintMismatch => {
                formatter.write_str("migration run journal has another plan fingerprint")
            }
            Self::SourcePreconditionMismatch => {
                formatter.write_str("migration run journal has another source precondition")
            }
            Self::TransformerVersionMismatch => {
                formatter.write_str("migration run journal has another transformer version")
            }
            Self::StepSetMismatch => {
                formatter.write_str("migration run journal steps differ from the immutable plan")
            }
            Self::RunIdentityMismatch => {
                formatter.write_str("migration run journal identity or immutable inputs differ")
            }
            Self::OutOfOrderStep { expected, actual } => write!(
                formatter,
                "migration journal expected step {expected}, got {actual}"
            ),
            Self::UnknownStep(id) => write!(formatter, "unknown migration journal step {id}"),
            Self::CommittedRevisionMismatch { expected, actual } => write!(
                formatter,
                "migration journal expected revision {expected}, got {actual}"
            ),
            Self::StepNotPrepared(id) => {
                write!(formatter, "migration journal step {id} was not prepared")
            }
            Self::CommitReceiptMismatch(id) => {
                write!(formatter, "migration journal receipt for step {id} differs")
            }
            Self::StepAlreadyCommitted(id) => {
                write!(
                    formatter,
                    "migration journal step {id} is already committed"
                )
            }
            Self::RunAlreadyCompleted => {
                formatter.write_str("migration run journal is already complete")
            }
            Self::IncompleteRun => {
                formatter.write_str("migration run journal cannot complete with unresolved steps")
            }
            Self::InvalidStateTransition => {
                formatter.write_str("migration run journal state transition is invalid")
            }
        }
    }
}

impl std::error::Error for MigrationRunJournalError {}

#[cfg(test)]
mod tests {
    use super::{
        JOURNAL_DIGEST_BYTES, JOURNAL_HEADER_BYTES, JOURNAL_STEP_BYTES,
        MigrationRunJournalCodecError, MigrationRunJournalError, MigrationRunJournalSnapshot,
        MigrationRunJournalSpec, MigrationRunJournalState, MigrationRunJournalStepSpec,
        MigrationRunJournalStepState,
    };
    use crate::ids::{
        DomainId, MigrationId, MigrationRunId, MigrationStepId, OperationId, Revision,
        SchemaRevision,
    };
    use crate::migration::{MigrationPlanFingerprint, MigrationTransformerVersion};
    use crate::migration_transform::MigrationTransformFingerprint;

    fn id<T: DomainId>(last: u8) -> Result<T, String> {
        let mut bytes = [0; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = last;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn spec() -> Result<MigrationRunJournalSpec, String> {
        MigrationRunJournalSpec::new(
            id::<MigrationId>(1)?,
            id::<MigrationRunId>(2)?,
            MigrationPlanFingerprint::from_bytes([3; 32]),
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [4; 32],
            MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
            vec![
                MigrationRunJournalStepSpec::new(
                    id::<MigrationStepId>(5)?,
                    id::<OperationId>(7)?,
                    [8; 32],
                    Revision::FIRST_COMMIT,
                ),
                MigrationRunJournalStepSpec::new(
                    id::<MigrationStepId>(6)?,
                    id::<OperationId>(9)?,
                    [10; 32],
                    Revision::new(2).map_err(|error| error.to_string())?,
                ),
            ],
        )
        .map_err(|error| error.to_string())
    }

    #[test]
    fn journal_codec_round_trips_pending_prepared_committed_and_completed_states()
    -> Result<(), String> {
        let mut snapshot =
            MigrationRunJournalSnapshot::start(spec()?).map_err(|error| error.to_string())?;
        let first = snapshot
            .spec()
            .steps()
            .first()
            .map(|step| step.step_id())
            .ok_or_else(|| String::from("first step spec is missing"))?;
        let second = snapshot
            .spec()
            .steps()
            .get(1)
            .map(|step| step.step_id())
            .ok_or_else(|| String::from("second step spec is missing"))?;
        snapshot
            .prepare_step(first)
            .map_err(|error| error.to_string())?;
        snapshot
            .mark_step_committed(
                first,
                Revision::FIRST_COMMIT,
                MigrationTransformFingerprint::from_bytes([11; 32]),
            )
            .map_err(|error| error.to_string())?;

        let prepared = MigrationRunJournalSnapshot::decode(
            &snapshot.encode().map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(prepared, snapshot);
        assert_eq!(prepared.state(), MigrationRunJournalState::Running);
        assert_eq!(
            prepared
                .steps()
                .get(1)
                .map(|step| step.state())
                .ok_or_else(|| String::from("second prepared step is missing"))?,
            MigrationRunJournalStepState::Pending
        );

        snapshot
            .prepare_step(second)
            .map_err(|error| error.to_string())?;
        snapshot
            .mark_step_committed(
                second,
                Revision::new(2).map_err(|error| error.to_string())?,
                MigrationTransformFingerprint::from_bytes([12; 32]),
            )
            .map_err(|error| error.to_string())?;
        snapshot
            .mark_completed()
            .map_err(|error| error.to_string())?;
        let decoded = MigrationRunJournalSnapshot::decode(
            &snapshot.encode().map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.state(), MigrationRunJournalState::Completed);
        Ok(())
    }

    #[test]
    fn journal_codec_rejects_checksum_corruption_and_illegal_step_order() -> Result<(), String> {
        let snapshot =
            MigrationRunJournalSnapshot::start(spec()?).map_err(|error| error.to_string())?;
        let mut corrupted = snapshot.encode().map_err(|error| error.to_string())?;
        let body_length = corrupted.len() - JOURNAL_DIGEST_BYTES;
        let last_body_index = body_length
            .checked_sub(1)
            .ok_or_else(|| String::from("encoded journal body is empty"))?;
        let last_body_byte = corrupted
            .get_mut(last_body_index)
            .ok_or_else(|| String::from("encoded journal body byte is missing"))?;
        *last_body_byte ^= 1;
        assert_eq!(
            MigrationRunJournalSnapshot::decode(&corrupted),
            Err(MigrationRunJournalCodecError::ChecksumMismatch)
        );

        let mut reordered = snapshot.encode().map_err(|error| error.to_string())?;
        let second_state = JOURNAL_HEADER_BYTES + JOURNAL_STEP_BYTES + JOURNAL_STEP_BYTES - 1;
        *reordered
            .get_mut(second_state)
            .ok_or_else(|| String::from("second step state byte is missing"))? = 1;
        let body_length = reordered.len() - JOURNAL_DIGEST_BYTES;
        let checksum = blake3::hash(
            reordered
                .get(..body_length)
                .ok_or_else(|| String::from("journal body slice is missing"))?,
        );
        reordered
            .get_mut(body_length..)
            .ok_or_else(|| String::from("journal checksum slice is missing"))?
            .copy_from_slice(checksum.as_bytes());
        assert!(matches!(
            MigrationRunJournalSnapshot::decode(&reordered),
            Err(MigrationRunJournalCodecError::InvalidTransition(
                MigrationRunJournalError::OutOfOrderStep { .. }
            ))
        ));
        Ok(())
    }

    #[test]
    fn journal_successors_are_monotone_and_keep_immutable_inputs() -> Result<(), String> {
        let first =
            MigrationRunJournalSnapshot::start(spec()?).map_err(|error| error.to_string())?;
        let mut prepared = first.clone();
        let first_step_id = prepared
            .spec()
            .steps()
            .first()
            .map(|step| step.step_id())
            .ok_or_else(|| String::from("first step spec is missing"))?;
        prepared
            .prepare_step(first_step_id)
            .map_err(|error| error.to_string())?;
        prepared
            .validate_successor(&first)
            .map_err(|error| error.to_string())?;

        let mut skipped = prepared.clone();
        skipped
            .mark_step_committed(
                first_step_id,
                Revision::FIRST_COMMIT,
                MigrationTransformFingerprint::from_bytes([13; 32]),
            )
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            skipped.validate_successor(&first),
            Err(MigrationRunJournalError::InvalidStateTransition)
        ));
        Ok(())
    }
}
