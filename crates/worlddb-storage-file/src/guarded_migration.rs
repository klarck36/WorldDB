//! Guarded migration execution bound to a recovered file-store and durable run journal.

use std::fmt;

use worlddb_core::{
    AuditRecord, BreakingMigrationAdminAction, CancellablePublishError, CommitCancellation,
    DatabaseId, MigrationAuditCommit, MigrationCommitBackend, MigrationDryRunInputFingerprint,
    MigrationExecutionFailure, MigrationExecutionResult, MigrationPlan, MigrationRunId,
    MigrationRunJournalError, MigrationRunJournalSnapshot, MigrationRunJournalSpec,
    MigrationRunJournalStepSpec, MigrationRunJournalStepState, MigrationRunJournalStore,
    MigrationSafeRestorePoint, MigrationStepInput, MigrationStepTargetSchema,
    MigrationTransformFingerprint, MigrationTransformer, MigrationTransformerVersion, PolicyTarget,
    PrincipalId, Record, Revision, RevisionBackend, RevisionLogError, SecurityEpoch,
    SecurityPolicySnapshot, encode_record, execute_guarded_migration,
};

use crate::{
    DatabaseLayout, FileMigrationBackendError, FileMigrationCommitBackend,
    FileMigrationHistoryRead, MigrationRunJournalFileStore, MigrationRunJournalFileStoreError,
    WriterLock,
};

/// Failure while opening a recovered file-store migration run.
#[derive(Debug)]
pub enum FileStoreGuardedMigrationOpenError {
    /// The persistent migration commit backend could not be opened.
    Backend(FileMigrationBackendError),
    /// The run journal could not be bound to this database and writer lock.
    Journal(MigrationRunJournalFileStoreError),
}

impl fmt::Display for FileStoreGuardedMigrationOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(error) => write!(formatter, "open guarded migration backend: {error}"),
            Self::Journal(error) => write!(formatter, "open guarded migration journal: {error}"),
        }
    }
}

impl std::error::Error for FileStoreGuardedMigrationOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            Self::Journal(error) => Some(error),
        }
    }
}

/// A journal I/O, transition, or binding failure at the guarded migration boundary.
#[derive(Debug)]
pub enum FileStoreGuardedMigrationJournalFailure {
    /// The durable sidecar could not be loaded or saved.
    Store(MigrationRunJournalFileStoreError),
    /// The monotone migration-run state transition was rejected.
    Transition(MigrationRunJournalError),
    /// The commit marker did not match the prepared run and step identity.
    CommitBindingMismatch,
    /// A committed output record could not be canonically encoded for its receipt fingerprint.
    RecordEncoding(worlddb_core::RecordCodecError),
}

impl fmt::Display for FileStoreGuardedMigrationJournalFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "migration journal storage failed: {error}"),
            Self::Transition(error) => {
                write!(formatter, "migration journal transition failed: {error}")
            }
            Self::CommitBindingMismatch => formatter
                .write_str("migration commit does not match the durable run-journal step identity"),
            Self::RecordEncoding(error) => {
                write!(
                    formatter,
                    "migration journal transform fingerprint failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for FileStoreGuardedMigrationJournalFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Transition(error) => Some(error),
            Self::RecordEncoding(error) => Some(error),
            Self::CommitBindingMismatch => None,
        }
    }
}

/// Why a guarded file-store migration call could not complete.
#[derive(Debug)]
pub enum FileStoreGuardedMigrationExecutionError<E> {
    /// The immutable run specification could not be constructed.
    JournalSpec(MigrationRunJournalError),
    /// An existing durable run-journal snapshot could not be read.
    JournalStore(MigrationRunJournalFileStoreError),
    /// An input batch could not be fingerprinted for the durable journal.
    InputFingerprintFailed,
    /// A previous run id is bound to another plan, input, or transformer version.
    JournalIdentityMismatch,
    /// This run already has a committed or prepared prefix; M7-16e adds resume execution.
    ResumeRequired(MigrationRunId),
    /// An already completed run cannot be executed a second time.
    RunAlreadyCompleted(MigrationRunId),
    /// The durable sidecar failed before the step's file-store commit was attempted.
    JournalBeforeCommit(FileStoreGuardedMigrationJournalFailure),
    /// The file-store commit succeeded, but its matching journal update did not become durable.
    JournalAfterCommit {
        /// Operation identity to reconcile from committed history.
        operation_id: worlddb_core::OperationId,
        /// Sidecar failure after the normative commit.
        source: FileStoreGuardedMigrationJournalFailure,
    },
    /// All migration steps committed, but the final Completed journal state could not be saved.
    JournalFinalization {
        /// Final durable WorldDB revision.
        revision: Revision,
        /// Sidecar failure after all normative commits.
        source: FileStoreGuardedMigrationJournalFailure,
    },
    /// Guarded authorization, proof, transform, validation, or commit failed.
    Migration(MigrationExecutionFailure<E>),
}

impl<E: fmt::Debug> fmt::Display for FileStoreGuardedMigrationExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JournalSpec(error) => {
                write!(formatter, "invalid migration run specification: {error}")
            }
            Self::JournalStore(error) => write!(formatter, "load migration run journal: {error}"),
            Self::InputFingerprintFailed => {
                formatter.write_str("migration step input could not be fingerprinted")
            }
            Self::JournalIdentityMismatch => formatter.write_str(
                "persisted migration run belongs to a different plan, input, or transformer",
            ),
            Self::ResumeRequired(run_id) => write!(
                formatter,
                "migration run {run_id} has a durable prefix and must be resumed or reconciled",
            ),
            Self::RunAlreadyCompleted(run_id) => {
                write!(formatter, "migration run {run_id} is already complete")
            }
            Self::JournalBeforeCommit(error) => {
                write!(formatter, "migration stopped before commit because {error}")
            }
            Self::JournalAfterCommit {
                operation_id,
                source,
            } => write!(
                formatter,
                "migration OperationId {operation_id} committed, but its journal update failed: {source}"
            ),
            Self::JournalFinalization { revision, source } => write!(
                formatter,
                "migration committed through revision {revision}, but journal completion failed: {source}"
            ),
            Self::Migration(failure) => write!(formatter, "guarded migration failed: {failure:?}"),
        }
    }
}

impl<E: fmt::Debug + 'static> std::error::Error for FileStoreGuardedMigrationExecutionError<E> {}

enum DeferredJournalFailure {
    BeforeCommit(FileStoreGuardedMigrationJournalFailure),
    AfterCommit {
        operation_id: worlddb_core::OperationId,
        source: FileStoreGuardedMigrationJournalFailure,
    },
}

/// One recovered file-store handle for production Restrictive/Breaking execution.
///
/// Each step's Prepared sidecar snapshot is synced before its WAL commit. The matching committed
/// step state is saved only after the Required Audit action and migration marker are durable.
/// This handle refuses an existing prepared prefix; `M7-16e` owns resuming such a run.
pub struct FileStoreGuardedMigrationRun<'a> {
    backend: FileMigrationCommitBackend<'a>,
    journal: MigrationRunJournalFileStore<'a>,
    journal_spec: Option<MigrationRunJournalSpec>,
    snapshot: Option<MigrationRunJournalSnapshot>,
    deferred_journal_failure: Option<DeferredJournalFailure>,
}

impl<'a> FileStoreGuardedMigrationRun<'a> {
    /// Recovers and opens one live file-store handle while retaining its exclusive writer lock.
    pub fn open(
        layout: DatabaseLayout,
        writer_lock: &'a WriterLock,
    ) -> Result<Self, FileStoreGuardedMigrationOpenError> {
        let backend = FileMigrationCommitBackend::open(layout.clone(), writer_lock)
            .map_err(FileStoreGuardedMigrationOpenError::Backend)?;
        let journal = MigrationRunJournalFileStore::new(&layout, writer_lock)
            .map_err(FileStoreGuardedMigrationOpenError::Journal)?;
        Ok(Self {
            backend,
            journal,
            journal_spec: None,
            snapshot: None,
            deferred_journal_failure: None,
        })
    }

    /// Current verified revision of the opened file-store.
    #[must_use]
    pub fn latest_published(&self) -> Revision {
        self.backend.latest_published()
    }

    /// Current in-process journal view, if this handle has started or loaded the run.
    #[must_use]
    pub fn journal_snapshot(&self) -> Option<&MigrationRunJournalSnapshot> {
        self.snapshot.as_ref()
    }

    /// Loads a durable run-journal snapshot without changing it.
    pub fn load_journal_status(
        &self,
        run_id: MigrationRunId,
    ) -> Result<Option<MigrationRunJournalSnapshot>, MigrationRunJournalFileStoreError> {
        self.journal.load(run_id)
    }

    /// Executes one authorized Restrictive or Breaking run through the file-store and journal.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0003")]
    pub fn execute<E>(
        &mut self,
        plan: &MigrationPlan,
        run_id: MigrationRunId,
        source_database_id: DatabaseId,
        actual_source_schema_fingerprint: [u8; 32],
        transformer: MigrationTransformer,
        step_inputs: Vec<MigrationStepInput>,
        decisions: worlddb_core::ValidatedMigrationDecisions,
        actor: PrincipalId,
        current_policy: &SecurityPolicySnapshot,
        policy_target: PolicyTarget,
        current_security_epoch: SecurityEpoch,
        safe_restore_point: Option<&MigrationSafeRestorePoint>,
        admin_action: Option<&BreakingMigrationAdminAction>,
        required_audit_records: Vec<AuditRecord>,
        validate_step: impl FnMut(
            &Self,
            Revision,
            &[Record],
            MigrationStepTargetSchema,
        ) -> Result<(), E>,
    ) -> Result<MigrationExecutionResult, FileStoreGuardedMigrationExecutionError<E>>
    where
        E: fmt::Debug,
    {
        self.deferred_journal_failure = None;
        let spec = build_journal_spec(plan, run_id, transformer.version(), &step_inputs).map_err(
            |error| match error {
                JournalSpecBuildError::Journal(error) => {
                    FileStoreGuardedMigrationExecutionError::JournalSpec(error)
                }
                JournalSpecBuildError::InputFingerprint => {
                    FileStoreGuardedMigrationExecutionError::InputFingerprintFailed
                }
            },
        )?;
        self.prepare_run_state(spec, run_id)?;

        let result = execute_guarded_migration(
            self,
            plan,
            run_id,
            source_database_id,
            actual_source_schema_fingerprint,
            transformer,
            step_inputs,
            decisions,
            actor,
            current_policy,
            policy_target,
            current_security_epoch,
            safe_restore_point,
            admin_action,
            required_audit_records,
            validate_step,
        );

        if let Some(failure) = self.deferred_journal_failure.take() {
            return Err(match failure {
                DeferredJournalFailure::BeforeCommit(source) => {
                    FileStoreGuardedMigrationExecutionError::JournalBeforeCommit(source)
                }
                DeferredJournalFailure::AfterCommit {
                    operation_id,
                    source,
                } => FileStoreGuardedMigrationExecutionError::JournalAfterCommit {
                    operation_id,
                    source,
                },
            });
        }

        let result = result.map_err(FileStoreGuardedMigrationExecutionError::Migration)?;
        let snapshot = self.snapshot.as_mut().ok_or_else(|| {
            FileStoreGuardedMigrationExecutionError::JournalBeforeCommit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            )
        })?;
        snapshot.mark_completed().map_err(|error| {
            FileStoreGuardedMigrationExecutionError::JournalFinalization {
                revision: result.final_revision(),
                source: FileStoreGuardedMigrationJournalFailure::Transition(error),
            }
        })?;
        if let Err(error) = self.journal.save(snapshot) {
            return Err(
                FileStoreGuardedMigrationExecutionError::JournalFinalization {
                    revision: result.final_revision(),
                    source: FileStoreGuardedMigrationJournalFailure::Store(error),
                },
            );
        }
        Ok(result)
    }

    fn prepare_run_state<E>(
        &mut self,
        spec: MigrationRunJournalSpec,
        run_id: MigrationRunId,
    ) -> Result<(), FileStoreGuardedMigrationExecutionError<E>> {
        if self
            .journal_spec
            .as_ref()
            .is_some_and(|prior| prior != &spec)
        {
            return Err(FileStoreGuardedMigrationExecutionError::JournalIdentityMismatch);
        }
        let loaded = self
            .journal
            .load(run_id)
            .map_err(FileStoreGuardedMigrationExecutionError::JournalStore)?;
        if let Some(snapshot) = loaded {
            if snapshot.spec() != &spec {
                return Err(FileStoreGuardedMigrationExecutionError::JournalIdentityMismatch);
            }
            if snapshot.state() == worlddb_core::MigrationRunJournalState::Completed {
                return Err(FileStoreGuardedMigrationExecutionError::RunAlreadyCompleted(run_id));
            }
            if snapshot
                .steps()
                .iter()
                .any(|step| step.state() != MigrationRunJournalStepState::Pending)
            {
                return Err(FileStoreGuardedMigrationExecutionError::ResumeRequired(
                    run_id,
                ));
            }
            self.snapshot = Some(snapshot);
        } else {
            self.snapshot = None;
        }
        self.journal_spec = Some(spec);
        Ok(())
    }

    fn fail_before_commit(
        &mut self,
        failure: FileStoreGuardedMigrationJournalFailure,
    ) -> CancellablePublishError {
        self.deferred_journal_failure = Some(DeferredJournalFailure::BeforeCommit(failure));
        CancellablePublishError::Publish(RevisionLogError::BackendFailure)
    }

    fn ensure_journal_started(
        &mut self,
        run_id: MigrationRunId,
    ) -> Result<(), FileStoreGuardedMigrationJournalFailure> {
        if self.snapshot.is_none() {
            let spec = self
                .journal_spec
                .clone()
                .ok_or(FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch)?;
            match self
                .journal
                .load(run_id)
                .map_err(FileStoreGuardedMigrationJournalFailure::Store)?
            {
                Some(snapshot) if snapshot.spec() == &spec => {
                    if snapshot.state() != worlddb_core::MigrationRunJournalState::Running
                        || snapshot
                            .steps()
                            .iter()
                            .any(|step| step.state() != MigrationRunJournalStepState::Pending)
                    {
                        return Err(FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch);
                    }
                    self.snapshot = Some(snapshot);
                }
                Some(_) => {
                    return Err(FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch);
                }
                None => {
                    let snapshot = MigrationRunJournalSnapshot::start(spec)
                        .map_err(FileStoreGuardedMigrationJournalFailure::Transition)?;
                    self.journal
                        .save(&snapshot)
                        .map_err(FileStoreGuardedMigrationJournalFailure::Store)?;
                    self.snapshot = Some(snapshot);
                }
            }
        }
        Ok(())
    }
}

impl RevisionBackend<Record> for FileStoreGuardedMigrationRun<'_> {
    type Read<'b>
        = FileMigrationHistoryRead<'b>
    where
        Self: 'b,
        Record: 'b;

    fn latest_published(&self) -> Revision {
        self.backend.latest_published()
    }

    fn publish(&mut self, _entries: Vec<Record>) -> Result<Revision, RevisionLogError> {
        Err(RevisionLogError::BackendFailure)
    }

    fn publish_cancellable(
        &mut self,
        _entries: Vec<Record>,
        _cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        Err(CancellablePublishError::Publish(
            RevisionLogError::BackendFailure,
        ))
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
        self.backend.read_at(revision)
    }
}

impl MigrationCommitBackend for FileStoreGuardedMigrationRun<'_> {
    fn publish_migration_step_with_required_audit(
        &mut self,
        expected_base_revision: Revision,
        entries: Vec<Record>,
        audit_commit: MigrationAuditCommit,
        audit_record: AuditRecord,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        let Some(spec) = self.journal_spec.as_ref() else {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        };
        let spec_plan_fingerprint = spec.plan_fingerprint();
        let spec_migration_id = spec.migration_id();
        let spec_run_id = spec.run_id();
        let transformer_version = spec.transformer_version();
        if audit_commit.plan_fingerprint() != spec_plan_fingerprint
            || audit_commit.identity().migration_id() != spec_migration_id
            || audit_commit.identity().run_id() != spec_run_id
            || Some(audit_commit.commit_revision()) != expected_base_revision.next_commit().ok()
        {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        }

        let mut marker = None;
        for (index, entry) in entries.iter().enumerate() {
            if let Record::MigrationStepCommitIdentity(identity) = entry {
                if marker.replace((index, *identity)).is_some() {
                    return Err(self.fail_before_commit(
                        FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
                    ));
                }
            }
        }
        let Some((marker_index, identity)) = marker else {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        };
        if marker_index + 1 != entries.len() || identity != audit_commit.identity() {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        }
        let Some(step_spec) = spec.steps().iter().copied().find(|step| {
            step.step_id() == identity.step_id() && step.operation_id() == identity.operation_id()
        }) else {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        };
        if identity.input_fingerprint() != Some(step_spec.input_fingerprint())
            || step_spec.target_revision() != audit_commit.commit_revision()
        {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        }

        let Some(transformed_records) = entries.get(..marker_index) else {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        };
        let transform_fingerprint = match calculate_transform_fingerprint(
            audit_commit.plan_fingerprint().as_bytes(),
            transformer_version,
            transformed_records,
        ) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return Err(self.fail_before_commit(
                    FileStoreGuardedMigrationJournalFailure::RecordEncoding(error),
                ));
            }
        };

        if let Err(error) = self.ensure_journal_started(spec_run_id) {
            return Err(self.fail_before_commit(error));
        }
        let Some(snapshot) = self.snapshot.as_mut() else {
            return Err(self.fail_before_commit(
                FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            ));
        };
        if let Err(error) = snapshot.prepare_step(identity.step_id()) {
            return Err(
                self.fail_before_commit(FileStoreGuardedMigrationJournalFailure::Transition(error))
            );
        }
        if let Err(error) = self.journal.save(snapshot) {
            return Err(
                self.fail_before_commit(FileStoreGuardedMigrationJournalFailure::Store(error))
            );
        }

        let operation_id = identity.operation_id();
        let revision = self.backend.publish_migration_step_with_required_audit(
            expected_base_revision,
            entries,
            audit_commit,
            audit_record,
            cancellation,
        )?;
        let Some(snapshot) = self.snapshot.as_mut() else {
            self.deferred_journal_failure = Some(DeferredJournalFailure::AfterCommit {
                operation_id,
                source: FileStoreGuardedMigrationJournalFailure::CommitBindingMismatch,
            });
            return Err(CancellablePublishError::OutcomeUnknown(operation_id));
        };
        if let Err(error) =
            snapshot.mark_step_committed(identity.step_id(), revision, transform_fingerprint)
        {
            self.deferred_journal_failure = Some(DeferredJournalFailure::AfterCommit {
                operation_id,
                source: FileStoreGuardedMigrationJournalFailure::Transition(error),
            });
            return Err(CancellablePublishError::OutcomeUnknown(operation_id));
        }
        if let Err(error) = self.journal.save(snapshot) {
            self.deferred_journal_failure = Some(DeferredJournalFailure::AfterCommit {
                operation_id,
                source: FileStoreGuardedMigrationJournalFailure::Store(error),
            });
            return Err(CancellablePublishError::OutcomeUnknown(operation_id));
        }
        Ok(revision)
    }
}

enum JournalSpecBuildError {
    Journal(MigrationRunJournalError),
    InputFingerprint,
}

fn build_journal_spec(
    plan: &MigrationPlan,
    run_id: MigrationRunId,
    transformer_version: MigrationTransformerVersion,
    step_inputs: &[MigrationStepInput],
) -> Result<MigrationRunJournalSpec, JournalSpecBuildError> {
    let targets = plan.step_targets().ok_or(JournalSpecBuildError::Journal(
        MigrationRunJournalError::StepSetMismatch,
    ))?;
    if step_inputs.len() != plan.steps().len() || step_inputs.len() != targets.len() {
        return Err(JournalSpecBuildError::Journal(
            MigrationRunJournalError::StepSetMismatch,
        ));
    }
    let mut steps = Vec::new();
    steps
        .try_reserve_exact(step_inputs.len())
        .map_err(|_| JournalSpecBuildError::Journal(MigrationRunJournalError::AllocationFailed))?;
    for ((input, planned_step), target) in step_inputs.iter().zip(plan.steps()).zip(targets) {
        if input.step_id() != *planned_step || target.step_id() != *planned_step {
            return Err(JournalSpecBuildError::Journal(
                MigrationRunJournalError::StepSetMismatch,
            ));
        }
        let fingerprint = MigrationDryRunInputFingerprint::for_records(input.records())
            .ok_or(JournalSpecBuildError::InputFingerprint)?;
        steps.push(MigrationRunJournalStepSpec::new(
            input.step_id(),
            input.operation_id(),
            *fingerprint.as_bytes(),
            target.schema().revision().revision(),
        ));
    }
    MigrationRunJournalSpec::new(
        plan.migration_id(),
        run_id,
        plan.fingerprint(),
        plan.source_schema_precondition().revision(),
        *plan.source_schema_precondition().fingerprint(),
        transformer_version,
        steps,
    )
    .map_err(JournalSpecBuildError::Journal)
}

fn calculate_transform_fingerprint(
    plan_fingerprint: &[u8; 32],
    transformer_version: MigrationTransformerVersion,
    records: &[Record],
) -> Result<MigrationTransformFingerprint, worlddb_core::RecordCodecError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.MigrationTransformResult.v2\0");
    hasher.update(plan_fingerprint);
    hasher.update(&transformer_version.value().to_be_bytes());
    hasher.update(
        &u64::try_from(records.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for record in records {
        let bytes = encode_record(record)?;
        hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(&bytes);
    }
    Ok(MigrationTransformFingerprint::from_bytes(
        *hasher.finalize().as_bytes(),
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{
        AuditAction, AuditCommitContext, AuditObjectClass, AuditOperationId, AuditOutcome,
        AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordId,
        AuditRecordIdentity, AuditSequence, AuthorizationMode, BreakingMigrationAdminAction, Bytes,
        Capability, CapabilityGrant, CapabilityRule, DomainId, GrantEffect, JobBudget,
        MigrationCategory, MigrationDryRun, MigrationExecutionError, MigrationId, MigrationPlan,
        MigrationPlanSpec, MigrationRunId, MigrationRunJournalSnapshot, MigrationRunJournalState,
        MigrationRunJournalStepState, MigrationStepInput, MigrationStepTargetSchema,
        MigrationTargetSchema, MigrationTransformer, MigrationTransformerVersion, PolicyRuleId,
        PolicyScope, PolicySubject, PolicyTarget, PredicateId, Principal, PrincipalId, Record,
        Revision, SchemaDefinitionId, SchemaIdentityTransition, SchemaRevision, SecurityEpoch,
        SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
        SourceSchemaPrecondition, ValidatedMigrationDecisions,
    };

    use super::{FileStoreGuardedMigrationExecutionError, FileStoreGuardedMigrationRun};
    use crate::{
        DatabaseLayout, ExactBackupManager, HistorySegmentStore, ManifestSegmentKind,
        ManifestSegmentReference, ManifestStore, MigrationRestorePointError, RecoveryManager,
        WalPrepareLog,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempArea(PathBuf);

    impl TempArea {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("worlddb-m7-16d-{}-{sequence}", std::process::id()));
            std::fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempArea {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn policy(
        actor: PrincipalId,
        migration: bool,
        backup: bool,
    ) -> Result<SecurityPolicySnapshot, String> {
        let capabilities = [
            (Capability::MigrationExecute, migration),
            (Capability::BackupCreate, backup),
            (Capability::BackupRestore, backup),
        ];
        let rules = capabilities
            .into_iter()
            .enumerate()
            .filter(|(_, (_, allowed))| *allowed)
            .map(|(index, (capability, _))| -> Result<_, String> {
                Ok(CapabilityRule::new(
                    id::<PolicyRuleId>(u8::try_from(index + 1).map_err(|e| e.to_string())?)?,
                    PolicySubject::Principal(actor),
                    CapabilityGrant::new(capability, GrantEffect::Allow),
                    PolicyScope::project(),
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        SecurityPolicySnapshot::new(vec![Principal::new(actor)], vec![], vec![], rules)
            .map_err(|error| error.to_string())
    }

    fn plan_and_inputs(
        category: MigrationCategory,
        source_revision: Revision,
        step_count: usize,
    ) -> Result<(MigrationPlan, Vec<MigrationStepInput>), String> {
        let migration_id = id::<MigrationId>(10)?;
        let source_predicate = id::<PredicateId>(11)?;
        let target_predicate = id::<PredicateId>(12)?;
        let transformer_version =
            MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?;
        let mut steps = Vec::new();
        let mut targets = Vec::new();
        let mut inputs = Vec::new();
        for index in 0..step_count {
            let tail = u8::try_from(index).map_err(|error| error.to_string())?;
            let step_id = id(tail.saturating_add(20))?;
            let operation_id = id(tail.saturating_add(30))?;
            let revision = (0..=index).try_fold(source_revision, |revision, _| {
                revision.next_commit().map_err(|error| error.to_string())
            })?;
            let fingerprint = if index + 1 == step_count {
                [0x22; 32]
            } else {
                [0x21; 32]
            };
            let schema = MigrationTargetSchema::new(
                SchemaRevision::from_published_revision(revision),
                fingerprint,
            );
            steps.push(step_id);
            targets.push(MigrationStepTargetSchema::new(step_id, schema));
            inputs.push(MigrationStepInput::new(step_id, operation_id, Vec::new()));
        }
        let final_target = targets
            .last()
            .copied()
            .ok_or_else(|| String::from("plan has no target step"))?
            .schema();
        let transition = SchemaIdentityTransition::new(
            Some(SchemaDefinitionId::Predicate(source_predicate)),
            Some(SchemaDefinitionId::Predicate(
                if category == MigrationCategory::Breaking {
                    target_predicate
                } else {
                    source_predicate
                },
            )),
            category,
        )
        .map_err(|error| error.to_string())?;
        let plan = MigrationPlan::new(MigrationPlanSpec {
            migration_id,
            category,
            source_schema: SourceSchemaPrecondition::new(
                SchemaRevision::from_published_revision(source_revision),
                [0x11; 32],
            ),
            target_schema: final_target,
            steps,
            step_targets: Some(targets),
            schema_changes: vec![transition],
            transformer_version,
            calendar_shift: None,
            budget: JobBudget::new(100, 1024 * 1024).map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())?;
        Ok((plan, inputs))
    }

    fn decisions(
        plan: &MigrationPlan,
        inputs: &[MigrationStepInput],
        actor: PrincipalId,
        policy: &SecurityPolicySnapshot,
    ) -> Result<ValidatedMigrationDecisions, String> {
        let source = plan.source_schema_precondition();
        let records = inputs
            .iter()
            .flat_map(|input| input.records().iter().cloned())
            .collect::<Vec<_>>();
        let dry_run =
            MigrationDryRun::run(plan, source.revision(), *source.fingerprint(), &records);
        ValidatedMigrationDecisions::validate(
            plan,
            &dry_run,
            Vec::new(),
            policy,
            actor,
            PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())
    }

    fn audit_records(
        plan: &MigrationPlan,
        inputs: &[MigrationStepInput],
        actor: PrincipalId,
        policy: &SecurityPolicySnapshot,
        first_record_tail: u8,
        first_operation_tail: u8,
    ) -> Result<Vec<AuditRecord>, String> {
        let policy_fingerprint =
            policy.effective_capability_fingerprint(actor, PolicyTarget::default());
        inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let tail = u8::try_from(index).map_err(|error| error.to_string())?;
                let target = plan
                    .step_targets()
                    .and_then(|targets| targets.get(index))
                    .ok_or_else(|| String::from("migration step target is missing"))?;
                let revision = target.schema().revision().revision();
                Ok(AuditRecord::new(
                    AuditRecordIdentity {
                        record_id: id::<AuditRecordId>(first_record_tail.saturating_add(tail))?,
                        sequence: AuditSequence::new(
                            u64::try_from(index + 1).map_err(|e| e.to_string())?,
                        ),
                        audit_operation_id: id::<AuditOperationId>(
                            first_operation_tail.saturating_add(tail),
                        )?,
                    },
                    AuditRecordDetails {
                        actor,
                        action: AuditAction::Migration,
                        object_class: AuditObjectClass::Migration,
                        outcome: AuditOutcome::Succeeded,
                        commit_context: AuditCommitContext::Committed {
                            revision,
                            operation_id: input.operation_id(),
                        },
                        security_epoch: SecurityEpoch::INITIAL,
                        policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                            policy_fingerprint.to_vec(),
                        ))
                        .map_err(|error| error.to_string())?,
                    },
                ))
            })
            .collect()
    }

    fn install_genesis_history(layout: &DatabaseLayout) -> Result<(), String> {
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let record = Record::HistorySpaceDefinition(
            worlddb_core::HistorySpaceDefinition::new(
                id::<worlddb_core::HistorySpaceId>(70)?,
                None,
                Revision::GENESIS,
            )
            .map_err(|error| error.to_string())?,
        );
        let bytes = worlddb_core::encode_record(&record).map_err(|error| error.to_string())?;
        let decoded = worlddb_core::decode_record(&bytes).map_err(|error| error.to_string())?;
        let receipt = HistorySegmentStore::new(layout.clone())
            .write_decoded_segment(&lock, &[decoded])
            .map_err(|error| error.to_string())?;
        let reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            receipt.id(),
            receipt.content_digest(),
            Revision::FIRST_COMMIT,
        );
        WalPrepareLog::new(layout)
            .commit_manifest_snapshot(
                &lock,
                id::<worlddb_core::OperationId>(71)?,
                vec![reference],
                &[],
            )
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        if ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| manifest.revision())
            != Some(Revision::FIRST_COMMIT)
        {
            return Err(String::from(
                "genesis fixture did not publish its first revision",
            ));
        }
        Ok(())
    }

    fn with_history_records(
        inputs: &[MigrationStepInput],
    ) -> Result<Vec<MigrationStepInput>, String> {
        inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let tail = u8::try_from(index).map_err(|error| error.to_string())?;
                let record = Record::HistorySpaceDefinition(
                    worlddb_core::HistorySpaceDefinition::new(
                        id::<worlddb_core::HistorySpaceId>(tail.saturating_add(80))?,
                        None,
                        Revision::GENESIS,
                    )
                    .map_err(|error| error.to_string())?,
                );
                let bytes =
                    worlddb_core::encode_record(&record).map_err(|error| error.to_string())?;
                Ok(MigrationStepInput::new(
                    input.step_id(),
                    input.operation_id(),
                    vec![bytes],
                ))
            })
            .collect()
    }

    fn security_history(
        actor: PrincipalId,
    ) -> Result<(SecurityPolicyHistory, SecurityPolicySnapshot), String> {
        let policy = policy(actor, true, true)?;
        let history = SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                policy.clone(),
            )],
        )
        .map_err(|error| error.to_string())?;
        Ok((history, policy))
    }

    #[test]
    fn restrictive_run_persists_each_step_journal_and_required_audit_on_file_store()
    -> Result<(), String> {
        let area = TempArea::create()?;
        let layout =
            DatabaseLayout::create(area.path("restrictive")).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let actor = id::<PrincipalId>(1)?;
        let database_id = layout
            .database_id()
            .ok_or_else(|| String::from("database identity is missing"))?;
        let run_id = id::<MigrationRunId>(2)?;
        let target = PolicyTarget::default();
        let policy = policy(actor, true, false)?;
        let (plan, empty_inputs) =
            plan_and_inputs(MigrationCategory::Restrictive, Revision::GENESIS, 2)?;
        let inputs = with_history_records(&empty_inputs)?;
        let validated_decisions = decisions(&plan, &inputs, actor, &policy)?;
        let audits = audit_records(&plan, &inputs, actor, &policy, 3, 5)?;

        let mut run = FileStoreGuardedMigrationRun::open(layout.clone(), &lock)
            .map_err(|error| error.to_string())?;
        let result = run
            .execute(
                &plan,
                run_id,
                database_id,
                [0x11; 32],
                MigrationTransformer::for_version(
                    MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?,
                inputs,
                validated_decisions,
                actor,
                &policy,
                target,
                SecurityEpoch::INITIAL,
                None,
                None,
                audits.clone(),
                |_, _, _, _| Ok::<(), String>(()),
            )
            .map_err(|error| error.to_string())?;

        assert_eq!(
            result.final_revision(),
            Revision::try_from(2).map_err(|e| e.to_string())?
        );
        assert_eq!(run.latest_published(), result.final_revision());
        let snapshot = run
            .journal_snapshot()
            .ok_or_else(|| String::from("successful run has no in-memory journal"))?;
        assert_eq!(snapshot.state(), MigrationRunJournalState::Completed);
        assert_eq!(snapshot.steps().len(), 2);
        let committed_steps = snapshot.steps().to_vec();
        for (step, receipt) in snapshot.steps().iter().zip(result.completed_steps()) {
            assert_eq!(
                step.state(),
                MigrationRunJournalStepState::Committed {
                    revision: receipt.revision(),
                    transform_fingerprint: receipt.transform_fingerprint(),
                }
            );
        }
        let committed_audits = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(committed_audits.len(), 2);
        for (stored, expected) in committed_audits.iter().zip(&audits) {
            assert_eq!(stored.record(), expected);
            assert!(stored.migration_action_payload().is_some());
        }

        drop(run);
        drop(lock);
        let reopened =
            DatabaseLayout::open(area.path("restrictive")).map_err(|error| error.to_string())?;
        let reopened_lock = reopened
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let reopened_run = FileStoreGuardedMigrationRun::open(reopened, &reopened_lock)
            .map_err(|error| error.to_string())?;
        let persisted = reopened_run
            .load_journal_status(run_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| String::from("completed run journal was not persisted"))?;
        assert_eq!(persisted.state(), MigrationRunJournalState::Completed);
        assert_eq!(persisted.steps(), committed_steps.as_slice());
        Ok(())
    }

    #[test]
    fn denied_current_rights_create_no_journal_or_normative_commit() -> Result<(), String> {
        let area = TempArea::create()?;
        let layout =
            DatabaseLayout::create(area.path("denied")).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let actor = id::<PrincipalId>(1)?;
        let database_id = layout
            .database_id()
            .ok_or_else(|| String::from("database identity is missing"))?;
        let run_id = id::<MigrationRunId>(4)?;
        let target = PolicyTarget::default();
        let approved_policy = policy(actor, true, false)?;
        let denied_policy = policy(actor, false, false)?;
        let (plan, inputs) = plan_and_inputs(MigrationCategory::Restrictive, Revision::GENESIS, 1)?;
        let validated_decisions = decisions(&plan, &inputs, actor, &approved_policy)?;
        let audits = audit_records(&plan, &inputs, actor, &approved_policy, 8, 9)?;
        let mut run = FileStoreGuardedMigrationRun::open(layout.clone(), &lock)
            .map_err(|error| error.to_string())?;

        let failure = run.execute(
            &plan,
            run_id,
            database_id,
            [0x11; 32],
            MigrationTransformer::for_version(
                MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
            inputs,
            validated_decisions,
            actor,
            &denied_policy,
            target,
            SecurityEpoch::INITIAL,
            None,
            None,
            audits,
            |_, _, _, _| Ok::<(), String>(()),
        );
        assert!(matches!(
            failure,
            Err(FileStoreGuardedMigrationExecutionError::Migration(failure))
                if matches!(failure.error(), MigrationExecutionError::MigrationUnauthorized)
        ));
        assert_eq!(run.latest_published(), Revision::GENESIS);
        assert!(
            run.load_journal_status(run_id)
                .map_err(|error| error.to_string())?
                .is_none()
        );
        assert!(
            WalPrepareLog::new(&layout)
                .committed_required_audit_records(&lock)
                .map_err(|error| error.to_string())?
                .is_empty()
        );
        assert!(
            ManifestStore::new(layout)
                .read_current()
                .map_err(|error| error.to_string())?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn stale_plan_source_revision_fails_occ_before_journal_or_publication() -> Result<(), String> {
        let area = TempArea::create()?;
        let layout =
            DatabaseLayout::create(area.path("stale-source")).map_err(|error| error.to_string())?;
        install_genesis_history(&layout)?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let actor = id::<PrincipalId>(1)?;
        let database_id = layout
            .database_id()
            .ok_or_else(|| String::from("database identity is missing"))?;
        let run_id = id::<MigrationRunId>(5)?;
        let target = PolicyTarget::default();
        let policy = policy(actor, true, false)?;
        let (plan, inputs) = plan_and_inputs(MigrationCategory::Restrictive, Revision::GENESIS, 1)?;
        let validated_decisions = decisions(&plan, &inputs, actor, &policy)?;
        let audits = audit_records(&plan, &inputs, actor, &policy, 18, 19)?;
        let mut run = FileStoreGuardedMigrationRun::open(layout.clone(), &lock)
            .map_err(|error| error.to_string())?;

        let failure = run.execute(
            &plan,
            run_id,
            database_id,
            [0x11; 32],
            MigrationTransformer::for_version(
                MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
            inputs,
            validated_decisions,
            actor,
            &policy,
            target,
            SecurityEpoch::INITIAL,
            None,
            None,
            audits,
            |_, _, _, _| Ok::<(), String>(()),
        );
        assert!(matches!(
            failure,
            Err(FileStoreGuardedMigrationExecutionError::Migration(failure))
                if matches!(failure.error(), MigrationExecutionError::SourceRevisionDoesNotMatchHead {
                    planned: Revision::GENESIS,
                    actual: Revision::FIRST_COMMIT,
                })
        ));
        assert_eq!(run.latest_published(), Revision::FIRST_COMMIT);
        assert!(
            run.load_journal_status(run_id)
                .map_err(|error| error.to_string())?
                .is_none()
        );
        assert!(
            WalPrepareLog::new(&layout)
                .committed_required_audit_records(&lock)
                .map_err(|error| error.to_string())?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn breaking_run_requires_real_restore_proof_and_explicit_admin_action() -> Result<(), String> {
        let area = TempArea::create()?;
        let layout =
            DatabaseLayout::create(area.path("breaking")).map_err(|error| error.to_string())?;
        install_genesis_history(&layout)?;
        let source_database_id = layout
            .database_id()
            .ok_or_else(|| String::from("source database identity is missing"))?;
        let actor = id::<PrincipalId>(1)?;
        let target = PolicyTarget::default();
        let (policy_history, current_policy) = security_history(actor)?;
        let (plan, inputs) =
            plan_and_inputs(MigrationCategory::Breaking, Revision::FIRST_COMMIT, 1)?;
        let validated_decisions = decisions(&plan, &inputs, actor, &current_policy)?;
        let audits = audit_records(&plan, &inputs, actor, &current_policy, 10, 12)?;
        let (_, retry_inputs) =
            plan_and_inputs(MigrationCategory::Breaking, Revision::FIRST_COMMIT, 1)?;
        let retry_audits = audit_records(&plan, &retry_inputs, actor, &current_policy, 10, 12)?;
        let retry_decisions = decisions(&plan, &retry_inputs, actor, &current_policy)?;
        let view = policy_history
            .select(AuthorizationMode::Now, actor, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        let safe_restore_point = ExactBackupManager::new(layout.clone())
            .create_migration_safe_restore_point(
                &plan,
                area.path("exact-backup"),
                area.path("restore-clone"),
                None,
                view,
                target,
            )
            .map_err(|error: MigrationRestorePointError| error.to_string())?;
        let admin_action = BreakingMigrationAdminAction::confirm(
            &plan,
            source_database_id,
            &[0x11; 32],
            actor,
            &current_policy,
            target,
            &safe_restore_point,
        )
        .map_err(|error| error.to_string())?;
        let run_id = id::<MigrationRunId>(14)?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut run = FileStoreGuardedMigrationRun::open(layout.clone(), &lock)
            .map_err(|error| error.to_string())?;

        let no_admin_confirmation = run.execute(
            &plan,
            run_id,
            source_database_id,
            [0x11; 32],
            MigrationTransformer::for_version(
                MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
            inputs,
            validated_decisions,
            actor,
            &current_policy,
            target,
            SecurityEpoch::INITIAL,
            Some(&safe_restore_point),
            None,
            audits.clone(),
            |_, _, _, _| Ok::<(), String>(()),
        );
        assert!(matches!(
            no_admin_confirmation,
            Err(FileStoreGuardedMigrationExecutionError::Migration(failure))
                if matches!(failure.error(), MigrationExecutionError::MissingBreakingAdminAction)
        ));
        assert_eq!(run.latest_published(), Revision::FIRST_COMMIT);
        assert!(
            run.load_journal_status(run_id)
                .map_err(|error| error.to_string())?
                .is_none()
        );

        let result = run
            .execute(
                &plan,
                run_id,
                source_database_id,
                [0x11; 32],
                MigrationTransformer::for_version(
                    MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?,
                retry_inputs,
                retry_decisions,
                actor,
                &current_policy,
                target,
                SecurityEpoch::INITIAL,
                Some(&safe_restore_point),
                Some(&admin_action),
                retry_audits,
                |_, _, _, _| Ok::<(), String>(()),
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(
            result.final_revision(),
            Revision::try_from(2).map_err(|e| e.to_string())?
        );
        assert_eq!(run.latest_published(), result.final_revision());
        assert_eq!(
            run.journal_snapshot()
                .map(MigrationRunJournalSnapshot::state),
            Some(MigrationRunJournalState::Completed)
        );
        let committed = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(committed.len(), 1);
        assert!(committed.iter().all(|record| {
            record.record().action() == AuditAction::Migration
                && record.migration_action_payload().is_some()
        }));
        Ok(())
    }
}
