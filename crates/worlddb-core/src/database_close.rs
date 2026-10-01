//! Explicit owner for the process-level close boundary.
//!
//! The owner retains the non-cloneable job supervisor and writer coordinator.
//! Shared command handles may outlive it, but both intake gates close before
//! either subsystem is drained. Telemetry remains an explicitly borrowed port.
//!
//! ```compile_fail
//! use worlddb_core::DatabaseCloseOwner;
//! fn duplicate_owner<C, R>(owner: DatabaseCloseOwner<C, R>) -> DatabaseCloseOwner<C, R> {
//!     owner.clone()
//! }
//! ```

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use crate::job_supervisor::{JobShutdownError, JobShutdownReport, JobSupervisor};
use crate::jobs::TaskFailure;
use crate::writer::{WriterCloseError, WriterCloseReport, WriterCoordinator};

/// Result of an independently reported best-effort telemetry flush.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelemetryFlushStatus {
    /// No telemetry exporter was configured for this close attempt.
    NotConfigured,
    /// The exporter completed its flush before the shared deadline.
    Complete,
    /// The exporter reached its deadline with telemetry still pending.
    Incomplete,
    /// The exporter reported a flush failure.
    Failed,
    /// The exporter panicked; its payload was discarded.
    Panicked,
}

/// Borrowed boundary for an exporter that flushes under the database close deadline.
pub trait TelemetryFlusher {
    /// Attempts a flush using the same absolute deadline as resource shutdown.
    ///
    /// Implementations must return by `deadline`; core code cannot forcibly
    /// interrupt arbitrary exporter code. A flush failure never changes the
    /// separately reported resource-close result.
    fn flush_until(&mut self, deadline: Instant) -> TelemetryFlushStatus;
}

impl<F> TelemetryFlusher for F
where
    F: FnMut(Instant) -> TelemetryFlushStatus,
{
    fn flush_until(&mut self, deadline: Instant) -> TelemetryFlushStatus {
        self(deadline)
    }
}

/// Explicit no-exporter adapter; its state remains visible in the close report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoTelemetryFlusher;

impl TelemetryFlusher for NoTelemetryFlusher {
    fn flush_until(&mut self, _deadline: Instant) -> TelemetryFlushStatus {
        TelemetryFlushStatus::NotConfigured
    }
}

/// Process owner for the job supervisor and single writer.
///
/// `JobSupervisor` and `WriterCoordinator` are moved into this value. Shared
/// writer handles remain controlled capabilities; telemetry stays explicitly
/// borrowed at each `close` call.
pub struct DatabaseCloseOwner<C, R> {
    jobs: JobSupervisor,
    writer: WriterCoordinator<C, R>,
}

impl<C, R> DatabaseCloseOwner<C, R> {
    /// Takes exclusive process ownership of both shutdown-capable subsystems.
    #[must_use]
    pub fn new(jobs: JobSupervisor, writer: WriterCoordinator<C, R>) -> Self {
        Self { jobs, writer }
    }

    /// Borrows the owned job supervisor for task admission and status reads.
    #[must_use]
    pub const fn job_supervisor(&self) -> &JobSupervisor {
        &self.jobs
    }

    /// Closes both intakes first, then drains each resource to the same deadline.
    ///
    /// The telemetry exporter is borrowed for this attempt and reported
    /// separately from resource completion. Repeated calls can finish a prior
    /// timed-out drain after blockers are released.
    pub fn close<T: TelemetryFlusher>(
        &mut self,
        deadline: Instant,
        telemetry: &mut T,
    ) -> DatabaseCloseReport {
        let _ = self.jobs.begin_shutdown();
        let _ = self.writer.begin_shutdown();

        let jobs = self.jobs.close(deadline);
        let writer = self.writer.close(deadline);
        let telemetry = catch_unwind(AssertUnwindSafe(|| telemetry.flush_until(deadline)))
            .unwrap_or(TelemetryFlushStatus::Panicked);

        DatabaseCloseReport {
            jobs,
            writer,
            telemetry,
        }
    }
}

impl<C, R> Drop for DatabaseCloseOwner<C, R> {
    fn drop(&mut self) {
        let _ = self.jobs.begin_shutdown();
        let _ = self.writer.begin_shutdown();
        // The owned components detach their handles without waiting or executing work here.
    }
}

/// Per-attempt resource and telemetry outcomes from [`DatabaseCloseOwner::close`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseCloseReport {
    jobs: Result<JobShutdownReport, JobShutdownError>,
    writer: Result<WriterCloseReport, WriterCloseError>,
    telemetry: TelemetryFlushStatus,
}

impl DatabaseCloseReport {
    /// Whether both owned subsystems reached a terminal close state.
    #[must_use]
    pub fn resources_complete(&self) -> bool {
        self.jobs.as_ref().is_ok_and(JobShutdownReport::drained)
            && self.writer.as_ref().is_ok_and(|report| report.complete())
    }

    /// Whether a worker panic means this process needs restart or recovery.
    #[must_use]
    pub fn requires_restart(&self) -> bool {
        self.jobs
            .as_ref()
            .is_ok_and(|report| report.worker_panics() > 0)
            || self
                .writer
                .as_ref()
                .is_ok_and(|report| report.worker_failure() == Some(TaskFailure::Panicked))
    }

    /// Job shutdown detail, including unfinished job IDs and worker counts.
    pub fn job_shutdown(&self) -> Result<&JobShutdownReport, JobShutdownError> {
        self.jobs.as_ref().map_err(|error| *error)
    }

    /// Writer shutdown detail, including unfinished handoffs and panic status.
    pub fn writer_shutdown(&self) -> Result<WriterCloseReport, WriterCloseError> {
        self.writer
            .as_ref()
            .map(|report| *report)
            .map_err(|error| *error)
    }

    /// Telemetry flush outcome, independent of resource shutdown completion.
    #[must_use]
    pub const fn telemetry_flush(&self) -> TelemetryFlushStatus {
        self.telemetry
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::ids::{DomainId, JobId};
    use crate::job_supervisor::{
        JobCompletion, JobPool, JobSpec, JobSupervisor, JobSupervisorLimits,
    };
    use crate::jobs::{JobBudget, JobKind};
    use crate::writer::{WriterUnavailable, spawn_single_writer};

    use super::{DatabaseCloseOwner, NoTelemetryFlusher, TelemetryFlushStatus};

    fn job_id(tail: u8) -> Result<JobId, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        JobId::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn supervisor() -> Result<JobSupervisor, String> {
        let limits =
            JobSupervisorLimits::new(4, 1, 1, 1, 1, 128).map_err(|error| error.to_string())?;
        JobSupervisor::new(limits).map_err(|error| error.to_string())
    }

    fn spec(job_id: JobId) -> Result<JobSpec, String> {
        let budget = JobBudget::new(10, 4096).map_err(|error| error.to_string())?;
        Ok(JobSpec::new(
            job_id,
            JobKind::IndexBuild,
            None,
            budget,
            JobPool::Cpu,
        ))
    }

    fn wait_terminal(owner: &DatabaseCloseOwner<u8, u8>, job_id: JobId) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = owner
                .job_supervisor()
                .get(job_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "job disappeared during database close".to_owned())?;
            if snapshot.descriptor().status().is_terminal() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("job did not reach a terminal state".to_owned());
            }
            thread::yield_now();
        }
    }

    #[test]
    fn owner_stops_both_intakes_then_retries_resources_separately_from_telemetry()
    -> Result<(), String> {
        let jobs = supervisor()?;
        let (writer_started_tx, writer_started_rx) = mpsc::sync_channel(1);
        let (writer_release_tx, writer_release_rx) = mpsc::sync_channel(1);
        let (handle, writer) = spawn_single_writer(1, move |command: u8| {
            let _ = writer_started_tx.send(());
            let _ = writer_release_rx.recv();
            command
        })
        .map_err(|error| error.to_string())?;
        let mut owner = DatabaseCloseOwner::new(jobs, writer);

        let job = job_id(1)?;
        let (job_started_tx, job_started_rx) = mpsc::sync_channel(1);
        let (job_release_tx, job_release_rx) = mpsc::sync_channel(1);
        owner
            .job_supervisor()
            .submit(spec(job)?, move |control| {
                let _ = job_started_tx.send(());
                let _ = job_release_rx.recv();
                if control.is_cancelled() {
                    JobCompletion::Cancelled
                } else {
                    JobCompletion::Succeeded
                }
            })
            .map_err(|error| error.to_string())?;
        let accepted_write = handle.submit(1).map_err(|error| error.to_string())?;
        job_started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        writer_started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;

        let telemetry_status = Cell::new(TelemetryFlushStatus::Incomplete);
        let mut flush = |_: Instant| telemetry_status.get();
        let first = owner.close(Instant::now() + Duration::from_millis(30), &mut flush);
        assert!(!first.resources_complete());
        assert_eq!(first.telemetry_flush(), TelemetryFlushStatus::Incomplete);
        assert!(
            !first
                .job_shutdown()
                .map_err(|error| error.to_string())?
                .drained()
        );
        assert!(
            !first
                .writer_shutdown()
                .map_err(|error| error.to_string())?
                .complete()
        );
        assert_eq!(
            owner
                .job_supervisor()
                .submit(spec(job_id(2)?)?, |_| JobCompletion::Succeeded)
                .err(),
            Some(crate::JobSubmitError::ShuttingDown)
        );
        assert_eq!(handle.submit(2).err(), Some(WriterUnavailable));

        job_release_tx.send(()).map_err(|error| error.to_string())?;
        writer_release_tx
            .send(())
            .map_err(|error| error.to_string())?;
        wait_terminal(&owner, job)?;
        assert_eq!(
            accepted_write
                .receive()
                .map_err(|error| error.to_string())?,
            1
        );

        telemetry_status.set(TelemetryFlushStatus::Complete);
        let second = owner.close(Instant::now() + Duration::from_secs(2), &mut flush);
        assert!(second.resources_complete());
        assert!(!second.requires_restart());
        assert_eq!(second.telemetry_flush(), TelemetryFlushStatus::Complete);
        Ok(())
    }

    #[test]
    fn telemetry_failure_is_separate_and_panics_are_payload_free() -> Result<(), String> {
        let jobs = supervisor()?;
        let (_handle, writer) =
            spawn_single_writer(1, |command: u8| command).map_err(|error| error.to_string())?;
        let mut owner = DatabaseCloseOwner::new(jobs, writer);
        let mut flush_failed = |_| TelemetryFlushStatus::Failed;
        let failed = owner.close(Instant::now() + Duration::from_secs(2), &mut flush_failed);
        assert!(failed.resources_complete());
        assert_eq!(failed.telemetry_flush(), TelemetryFlushStatus::Failed);

        let mut flush_panicked = |_| -> TelemetryFlushStatus {
            std::panic::resume_unwind(Box::new("private telemetry detail"));
        };
        let panicked = owner.close(Instant::now() + Duration::from_secs(2), &mut flush_panicked);
        assert!(panicked.resources_complete());
        assert_eq!(panicked.telemetry_flush(), TelemetryFlushStatus::Panicked);
        Ok(())
    }

    #[test]
    fn dropping_database_close_owner_does_not_wait_or_execute_queued_work() -> Result<(), String> {
        let jobs = supervisor()?;
        let job = job_id(3)?;
        let (job_started_tx, job_started_rx) = mpsc::sync_channel(1);
        let (job_release_tx, job_release_rx) = mpsc::sync_channel(1);
        let (job_finished_tx, job_finished_rx) = mpsc::sync_channel(1);
        jobs.submit(spec(job)?, move |_| {
            let _ = job_started_tx.send(());
            let _ = job_release_rx.recv();
            let _ = job_finished_tx.send(());
            JobCompletion::Succeeded
        })
        .map_err(|error| error.to_string())?;

        let executed = Arc::new(AtomicUsize::new(0));
        let writer_executed = Arc::clone(&executed);
        let (writer_started_tx, writer_started_rx) = mpsc::sync_channel(1);
        let (writer_release_tx, writer_release_rx) = mpsc::sync_channel(1);
        let (handle, writer) = spawn_single_writer(1, move |command: u8| {
            let _ = writer_started_tx.send(());
            let _ = writer_release_rx.recv();
            writer_executed.fetch_add(usize::from(command), Ordering::SeqCst);
            command
        })
        .map_err(|error| error.to_string())?;
        let accepted_write = handle.submit(1).map_err(|error| error.to_string())?;
        job_started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        writer_started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        let owner = DatabaseCloseOwner::new(jobs, writer);

        let (dropped_tx, dropped_rx) = mpsc::sync_channel(1);
        let drop_thread = thread::spawn(move || {
            drop(owner);
            let _ = dropped_tx.send(());
        });
        dropped_rx
            .recv_timeout(Duration::from_millis(250))
            .map_err(|_| "dropping database owner waited for blocked work".to_owned())?;
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert_eq!(handle.submit(2).err(), Some(WriterUnavailable));

        job_release_tx.send(()).map_err(|error| error.to_string())?;
        writer_release_tx
            .send(())
            .map_err(|error| error.to_string())?;
        assert_eq!(
            accepted_write
                .receive()
                .map_err(|error| error.to_string())?,
            1
        );
        job_finished_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        drop_thread
            .join()
            .map_err(|_| "drop helper thread panicked".to_owned())?;
        assert_eq!(executed.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn no_telemetry_exporter_is_explicit() -> Result<(), String> {
        let jobs = supervisor()?;
        let (_handle, writer) =
            spawn_single_writer(1, |command: u8| command).map_err(|error| error.to_string())?;
        let mut owner = DatabaseCloseOwner::new(jobs, writer);
        let report = owner.close(
            Instant::now() + Duration::from_secs(2),
            &mut NoTelemetryFlusher,
        );
        assert!(report.resources_complete());
        assert_eq!(
            report.telemetry_flush(),
            TelemetryFlushStatus::NotConfigured
        );
        Ok(())
    }
}
