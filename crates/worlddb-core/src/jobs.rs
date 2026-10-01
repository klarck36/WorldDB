//! Pure job identity, status, progress, and resource-budget values.

use std::fmt;
use std::num::NonZeroU64;
use std::thread;

use crate::ids::{JobId, PrincipalId};

/// The closed set of job kinds named by the 1.0 master plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobKind {
    /// Executes a versioned migration plan.
    Migration,
    /// Creates or verifies a backup.
    Backup,
    /// Builds an internal derived index.
    IndexBuild,
}

/// A terminal result of a job.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobTerminalState {
    /// The requested work completed successfully.
    Succeeded,
    /// The requested work stopped because it failed.
    Failed,
    /// The requested work stopped after accepted cancellation.
    Cancelled,
    /// A worker panic requires the engine to restart before work can continue.
    NeedsRestart,
}

/// Classified panic observed when joining a worker task.
///
/// Panic payloads are deliberately discarded: they may contain secrets or
/// implementation details and are not a recovery instruction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskFailure {
    /// The worker unwound before returning its result.
    Panicked,
}

/// Engine role of a worker that panicked.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskRole {
    /// Single writer whose interrupted mutation requires restart and recovery.
    Writer,
    /// Background task that does not own an in-progress publication.
    Background,
}

impl TaskFailure {
    /// Maps a worker panic to the safe terminal state for its role.
    ///
    /// Writer panics require database restart and recovery. Other worker
    /// panics fail only that job; the caller remains responsible for recording
    /// the failure and applying its retry policy.
    #[must_use]
    pub const fn terminal_state(self, role: TaskRole) -> JobTerminalState {
        match role {
            TaskRole::Writer => JobTerminalState::NeedsRestart,
            TaskRole::Background => JobTerminalState::Failed,
        }
    }
}

impl fmt::Display for TaskFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panicked => formatter.write_str("background task panicked"),
        }
    }
}

impl std::error::Error for TaskFailure {}

/// Converts a joined worker panic to a payload-free typed failure.
///
/// This observes the panic after the worker has unwound; it does not catch a
/// panic inside the worker or claim that the writer can continue safely.
pub fn observe_task_join<T>(joined: thread::Result<T>) -> Result<T, TaskFailure> {
    joined.map_err(|_payload| TaskFailure::Panicked)
}

/// Observed state of one job; terminal results remain explicit values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobStatus {
    /// Accepted into a bounded queue but not running yet.
    Queued,
    /// Currently being executed by a worker.
    Running,
    /// Finished with a typed terminal result.
    Terminal(JobTerminalState),
}

impl JobStatus {
    /// Returns whether the observed status is terminal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

/// A validated determinate progress count with a non-zero denominator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DeterminateJobProgress {
    completed: u64,
    total: NonZeroU64,
}

impl DeterminateJobProgress {
    /// Creates progress only when `total` is positive and `completed <= total`.
    pub fn new(completed: u64, total: u64) -> Result<Self, JobProgressError> {
        let total = NonZeroU64::new(total).ok_or(JobProgressError::ZeroTotal)?;
        if completed > total.get() {
            return Err(JobProgressError::CompletedExceedsTotal {
                completed,
                total: total.get(),
            });
        }
        Ok(Self { completed, total })
    }

    /// Returns the completed work units.
    #[must_use]
    pub const fn completed(self) -> u64 {
        self.completed
    }

    /// Returns the non-zero total work units.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.total.get()
    }
}

/// Progress is either indeterminate or has a validated determinate denominator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobProgress {
    /// The job has no meaningful stable work-unit denominator.
    Indeterminate,
    /// The job reports a validated completed/total pair.
    Determinate(DeterminateJobProgress),
}

impl JobProgress {
    /// Constructs determinate progress through its validating constructor.
    pub fn determinate(completed: u64, total: u64) -> Result<Self, JobProgressError> {
        DeterminateJobProgress::new(completed, total).map(Self::Determinate)
    }
}

/// Invalid determinate job progress.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobProgressError {
    /// Determinate progress cannot have a zero denominator.
    ZeroTotal,
    /// Completed work cannot exceed the reported total.
    CompletedExceedsTotal { completed: u64, total: u64 },
}

impl fmt::Display for JobProgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroTotal => {
                formatter.write_str("determinate job progress total must be positive")
            }
            Self::CompletedExceedsTotal { completed, total } => {
                write!(
                    formatter,
                    "completed job work {completed} exceeds total {total}"
                )
            }
        }
    }
}

impl std::error::Error for JobProgressError {}

/// Finite per-job work and memory ceilings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JobBudget {
    max_work_units: NonZeroU64,
    max_memory_bytes: NonZeroU64,
}

impl JobBudget {
    /// Creates a finite budget with positive work and memory limits.
    pub fn new(max_work_units: u64, max_memory_bytes: u64) -> Result<Self, JobBudgetError> {
        let max_work_units =
            NonZeroU64::new(max_work_units).ok_or(JobBudgetError::ZeroWorkUnits)?;
        let max_memory_bytes =
            NonZeroU64::new(max_memory_bytes).ok_or(JobBudgetError::ZeroMemoryBytes)?;
        Ok(Self {
            max_work_units,
            max_memory_bytes,
        })
    }

    /// Returns the maximum permitted work units.
    #[must_use]
    pub const fn max_work_units(self) -> u64 {
        self.max_work_units.get()
    }

    /// Returns the maximum permitted memory in bytes.
    #[must_use]
    pub const fn max_memory_bytes(self) -> u64 {
        self.max_memory_bytes.get()
    }
}

/// Invalid finite job budget.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobBudgetError {
    /// A job budget must permit a positive number of work units.
    ZeroWorkUnits,
    /// A job budget must permit a positive amount of memory.
    ZeroMemoryBytes,
}

impl fmt::Display for JobBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroWorkUnits => formatter.write_str("job work-unit budget must be positive"),
            Self::ZeroMemoryBytes => formatter.write_str("job memory budget must be positive"),
        }
    }
}

impl std::error::Error for JobBudgetError {}

/// A value snapshot of the public job metadata; it owns no worker or cancel token.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JobDescriptor {
    job_id: JobId,
    kind: JobKind,
    owner: Option<PrincipalId>,
    status: JobStatus,
    budget: JobBudget,
    progress: JobProgress,
}

impl JobDescriptor {
    /// Creates a typed job metadata snapshot.
    #[must_use]
    pub const fn new(
        job_id: JobId,
        kind: JobKind,
        owner: Option<PrincipalId>,
        status: JobStatus,
        budget: JobBudget,
        progress: JobProgress,
    ) -> Self {
        Self {
            job_id,
            kind,
            owner,
            status,
            budget,
            progress,
        }
    }

    /// Returns the job identity.
    #[must_use]
    pub const fn job_id(self) -> JobId {
        self.job_id
    }

    /// Returns the job kind.
    #[must_use]
    pub const fn kind(self) -> JobKind {
        self.kind
    }

    /// Returns the optional typed owner.
    #[must_use]
    pub const fn owner(self) -> Option<PrincipalId> {
        self.owner
    }

    /// Returns the observed job status.
    #[must_use]
    pub const fn status(self) -> JobStatus {
        self.status
    }

    /// Returns the finite resource ceilings.
    #[must_use]
    pub const fn budget(self) -> JobBudget {
        self.budget
    }

    /// Returns determinate or indeterminate progress.
    #[must_use]
    pub const fn progress(self) -> JobProgress {
        self.progress
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JobBudget, JobBudgetError, JobDescriptor, JobKind, JobProgress, JobProgressError,
        JobStatus, JobTerminalState, TaskFailure, TaskRole, observe_task_join,
    };
    use crate::ids::{DomainId, IdValidationError, JobId, PrincipalId};
    use std::thread;

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    #[test]
    fn job_descriptor_keeps_identity_owner_status_budget_and_progress_typed()
    -> Result<(), IdValidationError> {
        let job_id = uuid::<JobId>(1)?;
        let owner = uuid::<PrincipalId>(2)?;
        let budget = JobBudget::new(100, 4096);
        assert!(budget.is_ok());
        if let Ok(budget) = budget {
            let progress = JobProgress::determinate(3, 10);
            assert!(progress.is_ok());
            if let Ok(progress) = progress {
                let descriptor = JobDescriptor::new(
                    job_id,
                    JobKind::Migration,
                    Some(owner),
                    JobStatus::Running,
                    budget,
                    progress,
                );
                assert_eq!(descriptor.job_id(), job_id);
                assert_eq!(descriptor.owner(), Some(owner));
                assert_eq!(descriptor.status(), JobStatus::Running);
                assert_eq!(descriptor.budget().max_work_units(), 100);
                assert_eq!(descriptor.progress(), progress);
            }
        }
        Ok(())
    }

    #[test]
    fn job_status_and_terminal_outcomes_are_separate_values() {
        assert!(!JobStatus::Queued.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        for state in [
            JobTerminalState::Succeeded,
            JobTerminalState::Failed,
            JobTerminalState::Cancelled,
            JobTerminalState::NeedsRestart,
        ] {
            assert!(JobStatus::Terminal(state).is_terminal());
        }
        assert_ne!(JobTerminalState::Failed, JobTerminalState::Cancelled);
    }

    #[test]
    fn joined_panics_are_payload_free_and_writer_panics_require_restart() {
        let joined: thread::Result<()> = Err(Box::new("secret panic detail"));
        let failure = TaskFailure::Panicked;
        assert_eq!(observe_task_join(joined), Err(failure));
        assert_eq!(failure, TaskFailure::Panicked);
        assert_eq!(failure.to_string(), "background task panicked");
        assert_eq!(
            failure.terminal_state(TaskRole::Background),
            JobTerminalState::Failed
        );
        assert_eq!(
            failure.terminal_state(TaskRole::Writer),
            JobTerminalState::NeedsRestart
        );
    }

    #[test]
    fn progress_rejects_zero_denominators_and_counts_over_total() {
        assert_eq!(
            JobProgress::determinate(0, 0),
            Err(JobProgressError::ZeroTotal)
        );
        assert_eq!(
            JobProgress::determinate(11, 10),
            Err(JobProgressError::CompletedExceedsTotal {
                completed: 11,
                total: 10,
            })
        );
        assert!(JobProgress::determinate(10, 10).is_ok());
        assert_eq!(JobProgress::Indeterminate, JobProgress::Indeterminate);
    }

    #[test]
    fn budgets_are_finite_and_reject_zero_limits() {
        assert_eq!(JobBudget::new(0, 1), Err(JobBudgetError::ZeroWorkUnits));
        assert_eq!(JobBudget::new(1, 0), Err(JobBudgetError::ZeroMemoryBytes));
        let maximum = JobBudget::new(u64::MAX, u64::MAX);
        assert!(maximum.is_ok());
    }
}
