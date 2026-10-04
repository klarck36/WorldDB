//! Bounded CPU and blocking-I/O job pools with typed progress and failures.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::commit_cancellation::{
    CancellationRequestDisposition, CommitCancellation, CommitCancellationState, CommitpointError,
    CommitpointPermit,
};
use crate::ids::{JobId, PrincipalId};
use crate::jobs::{
    JobBudget, JobDescriptor, JobKind, JobPhase, JobProgress, JobStatus, JobTerminalState,
    TaskFailure,
};

/// Bounded worker pool selected for one job.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobPool {
    /// CPU-bound deterministic work.
    Cpu,
    /// Blocking filesystem or other synchronous I/O work.
    BlockingIo,
}

/// Immutable inputs required to admit one job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobSpec {
    job_id: JobId,
    kind: JobKind,
    owner: Option<PrincipalId>,
    budget: JobBudget,
    pool: JobPool,
}

impl JobSpec {
    /// Creates one typed job request with an explicit execution pool.
    #[must_use]
    pub const fn new(
        job_id: JobId,
        kind: JobKind,
        owner: Option<PrincipalId>,
        budget: JobBudget,
        pool: JobPool,
    ) -> Self {
        Self {
            job_id,
            kind,
            owner,
            budget,
            pool,
        }
    }

    /// Stable job identity selected by the caller.
    #[must_use]
    pub const fn job_id(self) -> JobId {
        self.job_id
    }

    /// Kind used for the job's public status snapshot.
    #[must_use]
    pub const fn kind(self) -> JobKind {
        self.kind
    }

    /// Optional authenticated owner assigned at admission.
    #[must_use]
    pub const fn owner(self) -> Option<PrincipalId> {
        self.owner
    }

    /// Finite work and memory ceilings assigned at admission.
    #[must_use]
    pub const fn budget(self) -> JobBudget {
        self.budget
    }

    /// Worker pool assigned at admission.
    #[must_use]
    pub const fn pool(self) -> JobPool {
        self.pool
    }
}

/// Hard limits for tracked jobs, worker counts, queues and resume metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobSupervisorLimits {
    max_tracked_jobs: NonZeroUsize,
    cpu_workers: NonZeroUsize,
    cpu_queue_capacity: NonZeroUsize,
    io_workers: NonZeroUsize,
    io_queue_capacity: NonZeroUsize,
    max_resume_bytes: NonZeroUsize,
}

impl JobSupervisorLimits {
    /// Creates finite positive limits for both worker pools and retained state.
    pub fn new(
        max_tracked_jobs: usize,
        cpu_workers: usize,
        cpu_queue_capacity: usize,
        io_workers: usize,
        io_queue_capacity: usize,
        max_resume_bytes: usize,
    ) -> Result<Self, JobSupervisorLimitError> {
        Ok(Self {
            max_tracked_jobs: positive(max_tracked_jobs, JobSupervisorLimitError::ZeroTrackedJobs)?,
            cpu_workers: positive(cpu_workers, JobSupervisorLimitError::ZeroCpuWorkers)?,
            cpu_queue_capacity: positive(
                cpu_queue_capacity,
                JobSupervisorLimitError::ZeroCpuQueue,
            )?,
            io_workers: positive(io_workers, JobSupervisorLimitError::ZeroIoWorkers)?,
            io_queue_capacity: positive(io_queue_capacity, JobSupervisorLimitError::ZeroIoQueue)?,
            max_resume_bytes: positive(max_resume_bytes, JobSupervisorLimitError::ZeroResumeLimit)?,
        })
    }
}

fn positive(
    value: usize,
    error: JobSupervisorLimitError,
) -> Result<NonZeroUsize, JobSupervisorLimitError> {
    NonZeroUsize::new(value).ok_or(error)
}

/// Invalid worker, queue or retention limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobSupervisorLimitError {
    ZeroTrackedJobs,
    ZeroCpuWorkers,
    ZeroCpuQueue,
    ZeroIoWorkers,
    ZeroIoQueue,
    ZeroResumeLimit,
}

impl fmt::Display for JobSupervisorLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroTrackedJobs => "tracked job limit must be positive",
            Self::ZeroCpuWorkers => "CPU worker count must be positive",
            Self::ZeroCpuQueue => "CPU queue capacity must be positive",
            Self::ZeroIoWorkers => "I/O worker count must be positive",
            Self::ZeroIoQueue => "I/O queue capacity must be positive",
            Self::ZeroResumeLimit => "resume metadata limit must be positive",
        })
    }
}

impl std::error::Error for JobSupervisorLimitError {}

/// Versioned opaque checkpoint bytes for a resumable job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobResumeMetadata {
    format_version: u16,
    payload: Vec<u8>,
}

impl JobResumeMetadata {
    /// Creates bounded checkpoint metadata with a non-zero format version.
    pub fn new(
        format_version: u16,
        payload: Vec<u8>,
        max_bytes: usize,
    ) -> Result<Self, JobStateError> {
        if format_version == 0 {
            return Err(JobStateError::InvalidResumeVersion);
        }
        if payload.len() > max_bytes {
            return Err(JobStateError::ResumeMetadataTooLarge {
                maximum: max_bytes,
                actual: payload.len(),
            });
        }
        Ok(Self {
            format_version,
            payload,
        })
    }

    /// Checkpoint schema version owned by the job kind's adapter.
    #[must_use]
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Opaque bounded checkpoint bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Explicit cooperative result returned by a job closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobCompletion {
    Succeeded,
    Failed,
    Cancelled,
}

/// Value snapshot of registered job state and any safe recovery metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSnapshot {
    descriptor: JobDescriptor,
    pool: JobPool,
    cancellation_state: CommitCancellationState,
    resume_metadata: Option<JobResumeMetadata>,
    failure: Option<TaskFailure>,
    reserved_memory_bytes: u64,
}

/// Bounded result of a deadline-limited supervisor close attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobShutdownReport {
    drained: bool,
    unfinished_jobs: Vec<JobId>,
    unfinished_workers: usize,
    worker_panics: usize,
}

impl JobShutdownReport {
    /// Whether all workers exited and every tracked job reached a terminal state.
    #[must_use]
    pub const fn drained(&self) -> bool {
        self.drained
    }

    /// Job identities still queued or running when this close attempt returned.
    #[must_use]
    pub fn unfinished_jobs(&self) -> &[JobId] {
        &self.unfinished_jobs
    }

    /// Worker threads still running after the deadline.
    #[must_use]
    pub const fn unfinished_workers(&self) -> usize {
        self.unfinished_workers
    }

    /// Worker threads that panicked while being joined.
    #[must_use]
    pub const fn worker_panics(&self) -> usize {
        self.worker_panics
    }
}

/// Shutdown could not safely inspect supervisor state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobShutdownError {
    RegistryPoisoned,
}

impl fmt::Display for JobShutdownError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("job supervisor state is unavailable during shutdown")
    }
}

impl std::error::Error for JobShutdownError {}

impl JobSnapshot {
    /// Typed job identity, owner, state, budget and progress.
    #[must_use]
    pub const fn descriptor(&self) -> JobDescriptor {
        self.descriptor
    }

    /// Worker pool assigned at admission.
    #[must_use]
    pub const fn pool(&self) -> JobPool {
        self.pool
    }

    /// Last linearized cancellation or publication state.
    #[must_use]
    pub const fn cancellation_state(&self) -> CommitCancellationState {
        self.cancellation_state
    }

    /// Latest bounded resumable checkpoint, if one was reported.
    #[must_use]
    pub fn resume_metadata(&self) -> Option<&JobResumeMetadata> {
        self.resume_metadata.as_ref()
    }

    /// Payload-free typed worker failure, if the job panicked.
    #[must_use]
    pub const fn failure(&self) -> Option<TaskFailure> {
        self.failure
    }

    /// Memory explicitly reserved through `JobControl` by the task adapter.
    #[must_use]
    pub const fn reserved_memory_bytes(&self) -> u64 {
        self.reserved_memory_bytes
    }
}

/// Cooperative control available to one running job closure.
#[derive(Clone)]
pub struct JobControl {
    job_id: JobId,
    budget: JobBudget,
    cancelled: Arc<AtomicBool>,
    commit_cancellation: CommitCancellation,
    state: Arc<Mutex<JobState>>,
    max_resume_bytes: usize,
}

impl JobControl {
    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Reports the current user-visible phase for this job.
    pub fn report_phase(&self, phase: JobPhase) -> Result<(), JobStateError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let accepting = state.accepting;
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobStateError::UnknownJob)?;
        if !matches!(record.status, JobStatus::Running) {
            return Err(JobStateError::JobNotRunning);
        }
        record.phase = if accepting {
            phase
        } else {
            JobPhase::ShuttingDown
        };
        Ok(())
    }

    /// Claims the final publication boundary. Cancellation after this point is too late.
    pub fn begin_commitpoint(&self) -> Result<CommitpointPermit, CommitpointError> {
        self.commit_cancellation.begin_commitpoint()
    }

    /// Reports determinate or indeterminate progress within the job's work budget.
    pub fn report_progress(&self, progress: JobProgress) -> Result<(), JobStateError> {
        if let JobProgress::Determinate(determinate) = progress {
            let total = determinate.total();
            if total > self.budget.max_work_units() {
                return Err(JobStateError::ProgressExceedsBudget {
                    total,
                    maximum: self.budget.max_work_units(),
                });
            }
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobStateError::UnknownJob)?;
        if !matches!(record.status, JobStatus::Running) {
            return Err(JobStateError::JobNotRunning);
        }
        record.progress = progress;
        Ok(())
    }

    /// Accounts memory held by this task and rejects reservations above its budget.
    pub fn reserve_memory(&self, bytes: u64) -> Result<(), JobStateError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobStateError::UnknownJob)?;
        if !matches!(record.status, JobStatus::Running) {
            return Err(JobStateError::JobNotRunning);
        }
        let requested = record.reserved_memory_bytes.checked_add(bytes).ok_or(
            JobStateError::MemoryBudgetExceeded {
                requested: u64::MAX,
                maximum: self.budget.max_memory_bytes(),
            },
        )?;
        if requested > self.budget.max_memory_bytes() {
            return Err(JobStateError::MemoryBudgetExceeded {
                requested,
                maximum: self.budget.max_memory_bytes(),
            });
        }
        record.reserved_memory_bytes = requested;
        Ok(())
    }

    /// Releases previously accounted task memory.
    pub fn release_memory(&self, bytes: u64) -> Result<(), JobStateError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobStateError::UnknownJob)?;
        if !matches!(record.status, JobStatus::Running) {
            return Err(JobStateError::JobNotRunning);
        }
        record.reserved_memory_bytes = record
            .reserved_memory_bytes
            .checked_sub(bytes)
            .ok_or(JobStateError::MemoryReleaseExceedsReservation)?;
        Ok(())
    }

    /// Stores the latest bounded checkpoint for a migration, backup or index build.
    pub fn set_resume_metadata(
        &self,
        format_version: u16,
        payload: Vec<u8>,
    ) -> Result<(), JobStateError> {
        let metadata = JobResumeMetadata::new(format_version, payload, self.max_resume_bytes)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobStateError::UnknownJob)?;
        if !matches!(record.status, JobStatus::Running) {
            return Err(JobStateError::JobNotRunning);
        }
        record.resume_metadata = Some(metadata);
        Ok(())
    }
}

type JobWork = Box<dyn FnOnce(JobControl) -> JobCompletion + Send + 'static>;
type WorkItem = (JobSpec, Arc<AtomicBool>, CommitCancellation, JobWork);

struct JobRecord {
    spec: JobSpec,
    status: JobStatus,
    phase: JobPhase,
    progress: JobProgress,
    cancellation: Arc<AtomicBool>,
    commit_cancellation: CommitCancellation,
    resume_metadata: Option<JobResumeMetadata>,
    failure: Option<TaskFailure>,
    reserved_memory_bytes: u64,
}

struct JobState {
    accepting: bool,
    jobs: BTreeMap<JobId, JobRecord>,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            accepting: true,
            jobs: BTreeMap::new(),
        }
    }
}

/// Owns two bounded pools and a bounded in-memory job registry.
pub struct JobSupervisor {
    limits: JobSupervisorLimits,
    state: Arc<Mutex<JobState>>,
    cpu_sender: Option<SyncSender<WorkItem>>,
    io_sender: Option<SyncSender<WorkItem>>,
    workers: Vec<JoinHandle<()>>,
    worker_panics: usize,
}

impl JobSupervisor {
    /// Starts fixed-size CPU and blocking-I/O pools with bounded queues.
    pub fn new(limits: JobSupervisorLimits) -> std::io::Result<Self> {
        let state = Arc::new(Mutex::new(JobState::default()));
        let (cpu_sender, cpu_receiver) = mpsc::sync_channel(limits.cpu_queue_capacity.get());
        let (io_sender, io_receiver) = mpsc::sync_channel(limits.io_queue_capacity.get());
        let mut workers = Vec::new();
        spawn_pool(
            "worlddb-cpu",
            limits.cpu_workers.get(),
            cpu_receiver,
            Arc::clone(&state),
            limits.max_resume_bytes.get(),
            &mut workers,
        )?;
        if let Err(error) = spawn_pool(
            "worlddb-io",
            limits.io_workers.get(),
            io_receiver,
            Arc::clone(&state),
            limits.max_resume_bytes.get(),
            &mut workers,
        ) {
            drop(cpu_sender);
            drop(io_sender);
            for worker in workers {
                let _ = worker.join();
            }
            return Err(error);
        }
        Ok(Self {
            limits,
            state,
            cpu_sender: Some(cpu_sender),
            io_sender: Some(io_sender),
            workers,
            worker_panics: 0,
        })
    }

    /// Admits a job or immediately reports bounded-queue backpressure.
    pub fn submit(
        &self,
        spec: JobSpec,
        work: impl FnOnce(JobControl) -> JobCompletion + Send + 'static,
    ) -> Result<(), JobSubmitError> {
        let cancellation = Arc::new(AtomicBool::new(false));
        let commit_cancellation = CommitCancellation::new();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| JobSubmitError::RegistryPoisoned)?;
            if state.jobs.contains_key(&spec.job_id) {
                return Err(JobSubmitError::DuplicateId);
            }
            if !state.accepting {
                return Err(JobSubmitError::ShuttingDown);
            }
            if state.jobs.len() >= self.limits.max_tracked_jobs.get() {
                return Err(JobSubmitError::AtCapacity);
            }
            state.jobs.insert(
                spec.job_id,
                JobRecord {
                    spec,
                    status: JobStatus::Queued,
                    phase: JobPhase::Queued,
                    progress: JobProgress::Indeterminate,
                    cancellation: Arc::clone(&cancellation),
                    commit_cancellation: commit_cancellation.clone(),
                    resume_metadata: None,
                    failure: None,
                    reserved_memory_bytes: 0,
                },
            );
        }
        let item = (
            spec,
            cancellation,
            commit_cancellation,
            Box::new(work) as JobWork,
        );
        let sender = match spec.pool {
            JobPool::Cpu => self.cpu_sender.as_ref(),
            JobPool::BlockingIo => self.io_sender.as_ref(),
        }
        .ok_or(JobSubmitError::Unavailable)?;
        match sender.try_send(item) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.remove_unaccepted(spec.job_id);
                Err(JobSubmitError::QueueFull(spec.pool))
            }
            Err(TrySendError::Disconnected(_)) => {
                self.remove_unaccepted(spec.job_id);
                Err(JobSubmitError::Unavailable)
            }
        }
    }

    /// Requests cooperative cancellation for a queued or running job.
    pub fn cancel(&self, job_id: JobId) -> Result<bool, JobStateError> {
        let disposition = self.cancel_with_disposition(job_id)?;
        Ok(!matches!(
            disposition,
            CancellationRequestDisposition::AlreadyTerminal
                | CancellationRequestDisposition::OutcomeUnknown
        ))
    }

    /// Requests cancellation and returns the exact commitpoint disposition.
    pub fn cancel_with_disposition(
        &self,
        job_id: JobId,
    ) -> Result<CancellationRequestDisposition, JobStateError> {
        let state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let record = state.jobs.get(&job_id).ok_or(JobStateError::UnknownJob)?;
        if record.status.is_terminal() {
            return Ok(CancellationRequestDisposition::AlreadyTerminal);
        }
        let disposition = record.commit_cancellation.request_cancellation();
        if matches!(
            disposition,
            CancellationRequestDisposition::Signalled
                | CancellationRequestDisposition::AlreadySignalled
        ) {
            record.cancellation.store(true, Ordering::Release);
        }
        Ok(disposition)
    }

    /// Returns an owned, payload-safe snapshot of one job's public state.
    pub fn get(&self, job_id: JobId) -> Result<Option<JobSnapshot>, JobStateError> {
        let state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        Ok(state.jobs.get(&job_id).map(snapshot))
    }

    /// Returns owned, payload-safe snapshots in stable JobId order.
    pub fn list(&self) -> Result<Vec<JobSnapshot>, JobStateError> {
        let state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        Ok(state.jobs.values().map(snapshot).collect())
    }

    /// Drops one terminal job record so bounded registry capacity can be reused.
    pub fn forget_terminal(&self, job_id: JobId) -> Result<(), JobStateError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?;
        let record = state.jobs.get(&job_id).ok_or(JobStateError::UnknownJob)?;
        if !record.status.is_terminal() {
            return Err(JobStateError::JobNotTerminal);
        }
        state.jobs.remove(&job_id);
        Ok(())
    }

    /// Number of retained jobs, bounded by the configured hard maximum.
    pub fn tracked_jobs(&self) -> Result<usize, JobStateError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| JobStateError::RegistryPoisoned)?
            .jobs
            .len())
    }

    fn remove_unaccepted(&self, job_id: JobId) {
        if let Ok(mut state) = self.state.lock() {
            state.jobs.remove(&job_id);
        }
    }

    /// Stops job admission and requests cooperative cancellation without waiting.
    ///
    /// Database shutdown owners can call this for every owned subsystem before
    /// they spend the shared close deadline draining any one subsystem.
    pub fn begin_shutdown(&mut self) -> Result<(), JobShutdownError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| JobShutdownError::RegistryPoisoned)?;
            state.accepting = false;
            for record in state.jobs.values_mut() {
                if !record.status.is_terminal() {
                    let disposition = record.commit_cancellation.request_cancellation();
                    if matches!(
                        disposition,
                        CancellationRequestDisposition::Signalled
                            | CancellationRequestDisposition::AlreadySignalled
                    ) {
                        record.cancellation.store(true, Ordering::Release);
                        record.phase = JobPhase::ShuttingDown;
                    }
                }
            }
        }
        self.cpu_sender.take();
        self.io_sender.take();
        Ok(())
    }

    /// Stops admission, requests cancellation, and drains workers until `deadline`.
    ///
    /// A timed-out call leaves unfinished worker handles and job records owned by
    /// this supervisor. The caller may release blockers and call `close` again.
    /// Dropping the supervisor itself only signals cancellation and detaches any
    /// remaining workers; it never waits or performs a commit.
    pub fn close(&mut self, deadline: Instant) -> Result<JobShutdownReport, JobShutdownError> {
        self.begin_shutdown()?;

        loop {
            let mut index = 0;
            while index < self.workers.len() {
                if self.workers.get(index).is_some_and(JoinHandle::is_finished) {
                    let worker = self.workers.swap_remove(index);
                    if worker.join().is_err() {
                        self.worker_panics = self.worker_panics.saturating_add(1);
                    }
                } else {
                    index += 1;
                }
            }

            let state = self
                .state
                .lock()
                .map_err(|_| JobShutdownError::RegistryPoisoned)?;
            let unfinished_jobs: Vec<_> = state
                .jobs
                .iter()
                .filter_map(|(job_id, record)| (!record.status.is_terminal()).then_some(*job_id))
                .collect();
            let drained = self.workers.is_empty() && unfinished_jobs.is_empty();
            let report = JobShutdownReport {
                drained,
                unfinished_jobs,
                unfinished_workers: self.workers.len(),
                worker_panics: self.worker_panics,
            };
            drop(state);
            if drained || Instant::now() >= deadline {
                return Ok(report);
            }
            thread::park_timeout(std::time::Duration::from_millis(1));
        }
    }
}

impl Drop for JobSupervisor {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.accepting = false;
            for record in state.jobs.values_mut() {
                if !record.status.is_terminal() {
                    let disposition = record.commit_cancellation.request_cancellation();
                    if matches!(
                        disposition,
                        CancellationRequestDisposition::Signalled
                            | CancellationRequestDisposition::AlreadySignalled
                    ) {
                        record.cancellation.store(true, Ordering::Release);
                        record.phase = JobPhase::ShuttingDown;
                    }
                }
            }
        }
        // SyncSenders and JoinHandles drop without waiting. Workers observe the
        // closed queues and cooperative cancellation on their own threads.
    }
}

fn spawn_pool(
    name: &str,
    count: usize,
    receiver: Receiver<WorkItem>,
    state: Arc<Mutex<JobState>>,
    max_resume_bytes: usize,
    workers: &mut Vec<JoinHandle<()>>,
) -> std::io::Result<()> {
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..count {
        let receiver = Arc::clone(&receiver);
        let state = Arc::clone(&state);
        let thread_name = format!("{name}-{index}");
        let worker = thread::Builder::new().name(thread_name).spawn(move || {
            loop {
                let next = match receiver.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => return,
                };
                let Ok((spec, cancellation, commit_cancellation, work)) = next else {
                    return;
                };
                if !matches!(set_running(&state, spec.job_id), Ok(true)) {
                    continue;
                }
                let control = JobControl {
                    job_id: spec.job_id,
                    budget: spec.budget,
                    cancelled: cancellation,
                    commit_cancellation,
                    state: Arc::clone(&state),
                    max_resume_bytes,
                };
                let result = catch_unwind(AssertUnwindSafe(|| work(control)));
                let (terminal, failure) = match result {
                    Ok(JobCompletion::Succeeded) => (JobTerminalState::Succeeded, None),
                    Ok(JobCompletion::Failed) => (JobTerminalState::Failed, None),
                    Ok(JobCompletion::Cancelled) => (JobTerminalState::Cancelled, None),
                    Err(_payload) => (JobTerminalState::Failed, Some(TaskFailure::Panicked)),
                };
                set_terminal(&state, spec.job_id, terminal, failure);
            }
        })?;
        workers.push(worker);
    }
    Ok(())
}

fn set_running(state: &Mutex<JobState>, job_id: JobId) -> Result<bool, JobStateError> {
    let mut state = state.lock().map_err(|_| JobStateError::RegistryPoisoned)?;
    let record = state
        .jobs
        .get_mut(&job_id)
        .ok_or(JobStateError::UnknownJob)?;
    if matches!(record.status, JobStatus::Queued) && record.cancellation.load(Ordering::Acquire) {
        record.status = JobStatus::Terminal(JobTerminalState::Cancelled);
        record.phase = JobPhase::ShuttingDown;
        record.reserved_memory_bytes = 0;
        return Ok(false);
    }
    if matches!(record.status, JobStatus::Queued) {
        record.status = JobStatus::Running;
        record.phase = JobPhase::Starting;
        return Ok(true);
    }
    Ok(false)
}

fn set_terminal(
    state: &Mutex<JobState>,
    job_id: JobId,
    terminal: JobTerminalState,
    failure: Option<TaskFailure>,
) {
    if let Ok(mut state) = state.lock() {
        if let Some(record) = state.jobs.get_mut(&job_id) {
            record.status = JobStatus::Terminal(terminal);
            record.phase = match terminal {
                JobTerminalState::Succeeded | JobTerminalState::Failed => JobPhase::Finalizing,
                JobTerminalState::Cancelled
                | JobTerminalState::NeedsRestart
                | JobTerminalState::Interrupted => JobPhase::ShuttingDown,
            };
            record.failure = failure;
            record.reserved_memory_bytes = 0;
        }
    }
}

fn snapshot(record: &JobRecord) -> JobSnapshot {
    JobSnapshot {
        descriptor: JobDescriptor::new(
            record.spec.job_id,
            record.spec.kind,
            record.spec.owner,
            record.status,
            record.phase,
            record.spec.budget,
            record.progress,
        ),
        pool: record.spec.pool,
        cancellation_state: record.commit_cancellation.state(),
        resume_metadata: record.resume_metadata.clone(),
        failure: record.failure,
        reserved_memory_bytes: record.reserved_memory_bytes,
    }
}

/// Job admission rejected due to duplicate identity, bounded capacity or stopped workers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobSubmitError {
    DuplicateId,
    AtCapacity,
    QueueFull(JobPool),
    Unavailable,
    ShuttingDown,
    RegistryPoisoned,
}

impl fmt::Display for JobSubmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateId => "JobId is already tracked",
            Self::AtCapacity => "job registry reached its configured maximum",
            Self::QueueFull(JobPool::Cpu) => "CPU job queue is full",
            Self::QueueFull(JobPool::BlockingIo) => "blocking-I/O job queue is full",
            Self::Unavailable => "job worker pool is unavailable",
            Self::ShuttingDown => "job supervisor is shutting down",
            Self::RegistryPoisoned => "job registry synchronization failed",
        })
    }
}

impl std::error::Error for JobSubmitError {}

/// Invalid progress, checkpoint, or job-state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobStateError {
    UnknownJob,
    JobNotRunning,
    JobNotTerminal,
    InvalidResumeVersion,
    ResumeMetadataTooLarge { maximum: usize, actual: usize },
    ProgressExceedsBudget { total: u64, maximum: u64 },
    MemoryBudgetExceeded { requested: u64, maximum: u64 },
    MemoryReleaseExceedsReservation,
    RegistryPoisoned,
}

impl fmt::Display for JobStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownJob => "job is not tracked",
            Self::JobNotRunning => "job metadata can only be changed while running",
            Self::JobNotTerminal => "only terminal jobs can be forgotten",
            Self::InvalidResumeVersion => "resume metadata format version must be non-zero",
            Self::ResumeMetadataTooLarge { .. } => "resume metadata exceeds its configured bound",
            Self::ProgressExceedsBudget { .. } => "job progress exceeds its work budget",
            Self::MemoryBudgetExceeded { .. } => "job memory reservation exceeds its budget",
            Self::MemoryReleaseExceedsReservation => "job released more memory than it reserved",
            Self::RegistryPoisoned => "job registry synchronization failed",
        })
    }
}

impl std::error::Error for JobStateError {}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        JobCompletion, JobPool, JobSpec, JobSubmitError, JobSupervisor, JobSupervisorLimits,
    };
    use crate::commit_cancellation::{CancellationRequestDisposition, CommitpointError};
    use crate::ids::{DomainId, JobId};
    use crate::jobs::{
        JobBudget, JobKind, JobPhase, JobProgress, JobStatus, JobTerminalState, TaskFailure,
    };

    fn id(tail: u8) -> Result<JobId, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        JobId::try_from_bytes(bytes)
    }

    fn limits(
        tracked: usize,
        cpu_workers: usize,
        cpu_queue: usize,
        io_workers: usize,
        io_queue: usize,
    ) -> Result<JobSupervisorLimits, String> {
        JobSupervisorLimits::new(tracked, cpu_workers, cpu_queue, io_workers, io_queue, 128)
            .map_err(|error| error.to_string())
    }

    fn spec(job_id: JobId, pool: JobPool) -> Result<JobSpec, String> {
        let budget = JobBudget::new(10, 4096).map_err(|error| error.to_string())?;
        Ok(JobSpec::new(
            job_id,
            JobKind::IndexBuild,
            None,
            budget,
            pool,
        ))
    }

    fn wait_terminal(supervisor: &JobSupervisor, job_id: JobId) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = supervisor
                .get(job_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "job disappeared before completion".to_owned())?;
            if snapshot.descriptor().status().is_terminal() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("job did not reach a terminal state".to_owned());
            }
            thread::yield_now();
        }
    }

    fn close_drained(supervisor: &mut JobSupervisor) -> Result<(), String> {
        let report = supervisor
            .close(Instant::now() + Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        if report.drained() {
            Ok(())
        } else {
            Err(format!("worker shutdown did not drain: {report:?}"))
        }
    }

    #[test]
    fn full_cpu_queue_applies_backpressure_and_keeps_registry_bounded() -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(4, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let first_id = id(1).map_err(|error| error.to_string())?;
        let second_id = id(2).map_err(|error| error.to_string())?;
        let rejected_id = id(3).map_err(|error| error.to_string())?;
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec(first_id, JobPool::Cpu)?, move |_| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        supervisor
            .submit(spec(second_id, JobPool::Cpu)?, |_| JobCompletion::Succeeded)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            supervisor
                .submit(spec(rejected_id, JobPool::Cpu)?, |_| {
                    JobCompletion::Succeeded
                })
                .err(),
            Some(JobSubmitError::QueueFull(JobPool::Cpu))
        );
        assert_eq!(
            supervisor
                .tracked_jobs()
                .map_err(|error| error.to_string())?,
            2
        );
        release_tx.send(()).map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, first_id)?;
        wait_terminal(&supervisor, second_id)?;
        assert_eq!(
            supervisor
                .get(rejected_id)
                .map_err(|error| error.to_string())?,
            None
        );
        close_drained(&mut supervisor)?;
        Ok(())
    }

    #[test]
    fn cpu_and_blocking_io_pools_make_progress_independently() -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(3, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let cpu_id = id(7).map_err(|error| error.to_string())?;
        let io_id = id(8).map_err(|error| error.to_string())?;
        let (cpu_started_tx, cpu_started_rx) = mpsc::sync_channel(1);
        let (cpu_release_tx, cpu_release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec(cpu_id, JobPool::Cpu)?, move |_| {
                let _ = cpu_started_tx.send(());
                let _ = cpu_release_rx.recv();
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;
        cpu_started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        supervisor
            .submit(spec(io_id, JobPool::BlockingIo)?, |_| {
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, io_id)?;
        assert_eq!(
            supervisor
                .get(cpu_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "CPU job disappeared".to_owned())?
                .descriptor()
                .status(),
            JobStatus::Running
        );
        cpu_release_tx.send(()).map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, cpu_id)?;
        close_drained(&mut supervisor)?;
        Ok(())
    }

    #[test]
    fn task_panic_is_recorded_without_exposing_its_payload() -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(2, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let job_id = id(4).map_err(|error| error.to_string())?;
        supervisor
            .submit(spec(job_id, JobPool::BlockingIo)?, |_| {
                std::panic::resume_unwind(Box::new("private panic payload"))
            })
            .map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, job_id)?;
        let snapshot = supervisor
            .get(job_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "panicked job was not retained".to_owned())?;
        assert_eq!(
            snapshot.descriptor().status(),
            JobStatus::Terminal(JobTerminalState::Failed)
        );
        assert_eq!(snapshot.failure(), Some(TaskFailure::Panicked));
        assert!(
            !snapshot
                .failure()
                .map(|failure| failure.to_string())
                .unwrap_or_default()
                .contains("private panic payload")
        );
        close_drained(&mut supervisor)?;
        Ok(())
    }

    #[test]
    fn progress_and_resume_metadata_are_bounded_and_visible() -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(2, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let job_id = id(5).map_err(|error| error.to_string())?;
        let progress = JobProgress::determinate(3, 10).map_err(|error| error.to_string())?;
        supervisor
            .submit(spec(job_id, JobPool::Cpu)?, move |control| {
                if control.set_resume_metadata(1, vec![1, 2, 3]).is_err()
                    || control.report_progress(progress).is_err()
                    || control.reserve_memory(4096).is_err()
                    || control.reserve_memory(1).is_ok()
                    || control.release_memory(2048).is_err()
                {
                    return JobCompletion::Failed;
                }
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, job_id)?;
        let snapshot = supervisor
            .get(job_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "completed job was not retained".to_owned())?;
        assert_eq!(
            snapshot.descriptor().progress(),
            JobProgress::determinate(3, 10).map_err(|e| e.to_string())?
        );
        assert_eq!(snapshot.reserved_memory_bytes(), 0);
        let metadata = snapshot
            .resume_metadata()
            .ok_or_else(|| "checkpoint metadata was not retained".to_owned())?;
        assert_eq!(metadata.format_version(), 1);
        assert_eq!(metadata.payload(), &[1, 2, 3]);
        close_drained(&mut supervisor)?;
        Ok(())
    }

    #[test]
    fn cancellation_is_cooperative_and_terminal_jobs_can_release_registry_capacity()
    -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(1, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let job_id = id(6).map_err(|error| error.to_string())?;
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec(job_id, JobPool::BlockingIo)?, move |control| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                if control.is_cancelled() {
                    JobCompletion::Cancelled
                } else {
                    JobCompletion::Succeeded
                }
            })
            .map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(
            supervisor
                .cancel(job_id)
                .map_err(|error| error.to_string())?
        );
        release_tx.send(()).map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, job_id)?;
        let snapshot = supervisor
            .get(job_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "cancelled job was not retained".to_owned())?;
        assert_eq!(
            snapshot.descriptor().status(),
            JobStatus::Terminal(JobTerminalState::Cancelled)
        );
        assert!(
            !supervisor
                .cancel(job_id)
                .map_err(|error| error.to_string())?
        );
        let next_id = id(7).map_err(|error| error.to_string())?;
        assert_eq!(
            supervisor
                .submit(spec(next_id, JobPool::Cpu)?, |_| JobCompletion::Succeeded)
                .err(),
            Some(JobSubmitError::AtCapacity)
        );
        supervisor
            .forget_terminal(job_id)
            .map_err(|error| error.to_string())?;
        supervisor
            .submit(spec(next_id, JobPool::Cpu)?, |_| JobCompletion::Succeeded)
            .map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, next_id)?;
        assert_eq!(
            supervisor
                .tracked_jobs()
                .map_err(|error| error.to_string())?,
            1
        );
        supervisor
            .forget_terminal(next_id)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            supervisor
                .tracked_jobs()
                .map_err(|error| error.to_string())?,
            0
        );
        close_drained(&mut supervisor)?;
        Ok(())
    }

    #[test]
    fn cancellation_reports_too_late_after_commitpoint_and_keeps_phase_visible()
    -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(2, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let job_id = id(77).map_err(|error| error.to_string())?;
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec(job_id, JobPool::Cpu)?, move |control| {
                if control.report_phase(JobPhase::Committing).is_err() {
                    return JobCompletion::Failed;
                }
                let permit = match control.begin_commitpoint() {
                    Ok(permit) => permit,
                    Err(_) => return JobCompletion::Failed,
                };
                let _ = control_tx.send(control.clone());
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                permit.committed();
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            supervisor
                .cancel_with_disposition(job_id)
                .map_err(|error| error.to_string())?,
            CancellationRequestDisposition::TooLate
        );
        let snapshot = supervisor
            .get(job_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "committing job was not retained".to_owned())?;
        assert_eq!(snapshot.descriptor().phase(), JobPhase::Committing);
        assert_eq!(
            snapshot.cancellation_state(),
            crate::commit_cancellation::CommitCancellationState::CommitpointPassed
        );
        let control = control_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            control.begin_commitpoint().err(),
            Some(CommitpointError::AlreadyStarted)
        );
        release_tx.send(()).map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, job_id)?;
        close_drained(&mut supervisor)?;
        Ok(())
    }

    #[test]
    fn shutdown_cancels_queued_jobs_and_can_retry_after_deadline() -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(3, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let running_id = id(9).map_err(|error| error.to_string())?;
        let queued_id = id(10).map_err(|error| error.to_string())?;
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let queued_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let queued_ran_in_job = std::sync::Arc::clone(&queued_ran);

        supervisor
            .submit(spec(running_id, JobPool::Cpu)?, move |control| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                if control.is_cancelled() {
                    JobCompletion::Cancelled
                } else {
                    JobCompletion::Succeeded
                }
            })
            .map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        supervisor
            .submit(spec(queued_id, JobPool::Cpu)?, move |_| {
                queued_ran_in_job.store(true, std::sync::atomic::Ordering::Release);
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;

        let report = supervisor
            .close(Instant::now() + Duration::from_millis(30))
            .map_err(|error| error.to_string())?;
        assert!(!report.drained());
        assert!(report.unfinished_jobs().contains(&running_id));
        assert!(report.unfinished_jobs().contains(&queued_id));
        assert!(report.unfinished_workers() >= 1);
        assert_eq!(
            supervisor
                .submit(
                    spec(id(11).map_err(|error| error.to_string())?, JobPool::Cpu)?,
                    |_| { JobCompletion::Succeeded }
                )
                .err(),
            Some(JobSubmitError::ShuttingDown)
        );

        release_tx.send(()).map_err(|error| error.to_string())?;
        wait_terminal(&supervisor, running_id)?;
        wait_terminal(&supervisor, queued_id)?;
        assert!(!queued_ran.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            supervisor
                .get(queued_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "queued job disappeared".to_owned())?
                .descriptor()
                .status(),
            JobStatus::Terminal(JobTerminalState::Cancelled)
        );
        let drained = supervisor
            .close(Instant::now() + Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(drained.drained());
        Ok(())
    }

    #[test]
    fn begin_shutdown_closes_admission_before_cooperative_cancel_drains() -> Result<(), String> {
        let mut supervisor =
            JobSupervisor::new(limits(2, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let running_id = id(17).map_err(|error| error.to_string())?;
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (shutdown_state_tx, shutdown_state_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec(running_id, JobPool::Cpu)?, move |control| {
                let _ = started_tx.send(());
                while !control.is_cancelled() {
                    thread::yield_now();
                }
                let phase_result = control.report_phase(JobPhase::Scanning);
                let commit_was_cancelled = matches!(
                    control.begin_commitpoint(),
                    Err(CommitpointError::CancelledBeforeCommitpoint)
                );
                let _ = shutdown_state_tx.send((phase_result.is_ok(), commit_was_cancelled));
                let _ = release_rx.recv();
                JobCompletion::Cancelled
            })
            .map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("running job did not start: {error}"))?;

        supervisor
            .begin_shutdown()
            .map_err(|error| error.to_string())?;
        assert_eq!(
            supervisor
                .submit(
                    spec(id(18).map_err(|error| error.to_string())?, JobPool::Cpu)?,
                    |_| JobCompletion::Succeeded
                )
                .err(),
            Some(JobSubmitError::ShuttingDown)
        );
        assert_eq!(
            shutdown_state_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| format!("running job did not observe cancellation: {error}"))?,
            (true, true)
        );
        let shutting_down = supervisor
            .get(running_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "running job disappeared".to_owned())?;
        assert_eq!(shutting_down.descriptor().phase(), JobPhase::ShuttingDown);
        assert_eq!(
            shutting_down.cancellation_state(),
            crate::commit_cancellation::CommitCancellationState::CancellationRequested
        );
        release_tx.send(()).map_err(|error| error.to_string())?;
        let report = supervisor
            .close(Instant::now() + Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(report.drained());
        Ok(())
    }

    #[test]
    fn dropping_supervisor_does_not_wait_for_uncooperative_work() -> Result<(), String> {
        let supervisor =
            JobSupervisor::new(limits(1, 1, 1, 1, 1)?).map_err(|error| error.to_string())?;
        let job_id = id(12).map_err(|error| error.to_string())?;
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec(job_id, JobPool::BlockingIo)?, move |_| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                JobCompletion::Succeeded
            })
            .map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;

        let (dropped_tx, dropped_rx) = mpsc::sync_channel(1);
        let drop_thread = thread::spawn(move || {
            drop(supervisor);
            let _ = dropped_tx.send(());
        });
        dropped_rx
            .recv_timeout(Duration::from_millis(250))
            .map_err(|_| "dropping supervisor waited for blocked job".to_owned())?;
        release_tx.send(()).map_err(|error| error.to_string())?;
        drop_thread
            .join()
            .map_err(|_| "drop helper thread panicked".to_owned())?;
        Ok(())
    }

    #[test]
    fn invalid_worker_and_resume_bounds_are_rejected() {
        assert!(JobSupervisorLimits::new(1, 0, 1, 1, 1, 1).is_err());
        assert!(super::JobResumeMetadata::new(1, vec![0, 1], 1).is_err());
    }
}
