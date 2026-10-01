//! Single-writer command coordination and immutable snapshot publication.
//!
//! Sharing is performed on `Arc<DatabaseHandle>` so the contained database
//! handle itself remains an owned, non-cloneable process resource.
//!
//! ```compile_fail
//! use worlddb_core::DatabaseHandle;
//! fn duplicate_the_database_resource(
//!     handle: &DatabaseHandle<u8, u8>,
//! ) -> DatabaseHandle<u8, u8> {
//!     handle.clone()
//! }
//! ```

use std::io;
use std::ops::Deref;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::jobs::{TaskFailure, observe_task_join};

enum WriterMessage<C, R> {
    Execute(C, SyncSender<R>),
}

/// A narrow, explicitly cloneable command handle to the single writer.
///
/// The handle exposes no engine state or lock. Submission uses a bounded
/// mailbox and applies backpressure while it is full. Once the coordinator
/// closes intake, new submissions fail; submitters already admitted finish
/// their mailbox handoff and are drained in mailbox order.
pub struct DatabaseHandle<C, R> {
    intake: Arc<WriterIntake<C, R>>,
}

impl<C, R> DatabaseHandle<C, R>
where
    C: Send + 'static,
    R: Send + 'static,
{
    /// Sends one owned command to the writer and returns its one-shot result.
    pub fn submit(&self, command: C) -> Result<WriterReply<R>, WriterUnavailable> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        let sender = {
            let mut state = self.intake.state.lock().map_err(|_| WriterUnavailable)?;
            if !state.accepting {
                return Err(WriterUnavailable);
            }
            let sender = state.sender.as_ref().ok_or(WriterUnavailable)?.clone();
            state.submissions_in_flight = state
                .submissions_in_flight
                .checked_add(1)
                .ok_or(WriterUnavailable)?;
            sender
        };
        let send_result = sender
            .send(WriterMessage::Execute(command, reply_sender))
            .map_err(|_| WriterUnavailable);
        drop(sender);
        self.intake.finish_submission()?;
        send_result?;
        Ok(WriterReply {
            receiver: reply_receiver,
        })
    }
}

struct WriterIntakeState<C, R> {
    accepting: bool,
    sender: Option<SyncSender<WriterMessage<C, R>>>,
    submissions_in_flight: usize,
}

struct WriterIntake<C, R> {
    state: Mutex<WriterIntakeState<C, R>>,
    changed: Condvar,
}

impl<C, R> WriterIntake<C, R> {
    fn finish_submission(&self) -> Result<(), WriterUnavailable> {
        let mut state = self.state.lock().map_err(|_| WriterUnavailable)?;
        if state.submissions_in_flight == 0 {
            return Err(WriterUnavailable);
        }
        state.submissions_in_flight -= 1;
        self.changed.notify_all();
        Ok(())
    }
}

/// One-shot result of a command accepted by the writer mailbox.
pub struct WriterReply<R> {
    receiver: Receiver<R>,
}

impl<R> WriterReply<R> {
    /// Waits for the accepted command's result.
    pub fn receive(self) -> Result<R, WriterUnavailable> {
        self.receiver.recv().map_err(|_| WriterUnavailable)
    }
}

/// The command's writer stopped before it could accept or answer the request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriterUnavailable;

impl std::fmt::Display for WriterUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("single writer is unavailable")
    }
}

impl std::error::Error for WriterUnavailable {}

/// Process-owned shutdown and join handle for the unique writer thread.
pub struct WriterCoordinator<C, R> {
    intake: Arc<WriterIntake<C, R>>,
    thread: Option<JoinHandle<()>>,
    worker_failure: Option<TaskFailure>,
}

/// The shared command handle and its unique writer coordinator.
pub type SingleWriter<C, R> = (Arc<DatabaseHandle<C, R>>, WriterCoordinator<C, R>);

impl<C, R> WriterCoordinator<C, R> {
    /// Stops new submissions and drains accepted submissions until `deadline`.
    ///
    /// A timed-out report leaves the writer thread owned by this coordinator;
    /// callers may release blockers and call `close` again. A completed report
    /// with a worker failure requires restart or recovery.
    pub fn close(&mut self, deadline: Instant) -> Result<WriterCloseReport, WriterCloseError> {
        self.begin_shutdown()?;

        loop {
            let pending = {
                let mut state = self
                    .intake
                    .state
                    .lock()
                    .map_err(|_| WriterCloseError::IntakePoisoned)?;
                while state.submissions_in_flight != 0 {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Ok(self.report(state.submissions_in_flight));
                    }
                    let (next, timeout) = self
                        .intake
                        .changed
                        .wait_timeout(state, remaining)
                        .map_err(|_| WriterCloseError::IntakePoisoned)?;
                    state = next;
                    if timeout.timed_out() && state.submissions_in_flight != 0 {
                        return Ok(self.report(state.submissions_in_flight));
                    }
                }
                state.submissions_in_flight
            };

            if self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
                if let Some(thread) = self.thread.take() {
                    self.worker_failure = observe_task_join(thread.join()).err();
                }
                return Ok(self.report(pending));
            }
            if self.thread.is_none() || Instant::now() >= deadline {
                return Ok(self.report(pending));
            }
            thread::park_timeout(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(1)),
            );
        }
    }

    /// Closes intake and waits without a deadline for the writer to finish.
    /// Prefer [`WriterCoordinator::close`] when the caller needs a bounded wait.
    pub fn join(mut self) -> Result<(), WriterCloseError> {
        self.begin_shutdown()?;
        {
            let mut state = self
                .intake
                .state
                .lock()
                .map_err(|_| WriterCloseError::IntakePoisoned)?;
            while state.submissions_in_flight != 0 {
                state = self
                    .intake
                    .changed
                    .wait(state)
                    .map_err(|_| WriterCloseError::IntakePoisoned)?;
            }
        }
        if let Some(thread) = self.thread.take() {
            self.worker_failure = observe_task_join(thread.join()).err();
        }
        match self.worker_failure {
            Some(failure) => Err(WriterCloseError::WorkerFailed(failure)),
            None => Ok(()),
        }
    }

    /// Stops accepting new writer submissions without waiting for accepted work.
    pub fn begin_shutdown(&self) -> Result<(), WriterCloseError> {
        let sender = {
            let mut state = self
                .intake
                .state
                .lock()
                .map_err(|_| WriterCloseError::IntakePoisoned)?;
            state.accepting = false;
            state.sender.take()
        };
        drop(sender);
        Ok(())
    }

    fn report(&self, unfinished_submissions: usize) -> WriterCloseReport {
        WriterCloseReport {
            complete: self.thread.is_none() && unfinished_submissions == 0,
            worker_unfinished: self.thread.is_some(),
            unfinished_submissions,
            worker_failure: self.worker_failure,
        }
    }
}

impl<C, R> Drop for WriterCoordinator<C, R> {
    fn drop(&mut self) {
        let _ = self.begin_shutdown();
        // Dropping JoinHandle detaches the worker; Drop never waits for it.
    }
}

/// Outcome of a deadline-bounded writer close.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriterCloseReport {
    complete: bool,
    worker_unfinished: bool,
    unfinished_submissions: usize,
    worker_failure: Option<TaskFailure>,
}

impl WriterCloseReport {
    /// Whether intake is closed, submissions are handed off, and the writer is joined.
    #[must_use]
    pub const fn complete(self) -> bool {
        self.complete
    }

    /// Whether the writer thread is still owned and has not yet been joined.
    #[must_use]
    pub const fn worker_unfinished(self) -> bool {
        self.worker_unfinished
    }

    /// Number of admitted submit calls that have not yet completed mailbox handoff.
    #[must_use]
    pub const fn unfinished_submissions(self) -> usize {
        self.unfinished_submissions
    }

    /// Payload-free panic classification, if the writer unwound before exit.
    #[must_use]
    pub const fn worker_failure(self) -> Option<TaskFailure> {
        self.worker_failure
    }
}

/// Writer shutdown failed before its state could be established or the worker panicked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriterCloseError {
    IntakePoisoned,
    WorkerFailed(TaskFailure),
}

impl std::fmt::Display for WriterCloseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IntakePoisoned => formatter.write_str("writer intake state is poisoned"),
            Self::WorkerFailed(failure) => write!(formatter, "writer failed: {failure}"),
        }
    }
}

impl std::error::Error for WriterCloseError {}

/// Starts exactly one thread to execute commands in mailbox order.
///
/// The engine state belongs to `execute` on this thread. The returned handles
/// own the bounded command channel and one-shot replies, while the coordinator
/// owns the intake gate and writer shutdown handle.
pub fn spawn_single_writer<C, R, F>(
    mailbox_capacity: usize,
    mut execute: F,
) -> io::Result<SingleWriter<C, R>>
where
    C: Send + 'static,
    R: Send + 'static,
    F: FnMut(C) -> R + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(mailbox_capacity);
    let intake = Arc::new(WriterIntake {
        state: Mutex::new(WriterIntakeState {
            accepting: true,
            sender: Some(sender),
            submissions_in_flight: 0,
        }),
        changed: Condvar::new(),
    });
    let thread = thread::Builder::new()
        .name("worlddb-single-writer".to_owned())
        .spawn(move || run_writer(receiver, &mut execute))?;
    Ok((
        Arc::new(DatabaseHandle {
            intake: Arc::clone(&intake),
        }),
        WriterCoordinator {
            intake,
            thread: Some(thread),
            worker_failure: None,
        },
    ))
}

fn run_writer<C, R, F>(receiver: Receiver<WriterMessage<C, R>>, execute: &mut F)
where
    F: FnMut(C) -> R,
{
    while let Ok(WriterMessage::Execute(command, reply)) = receiver.recv() {
        let result = execute(command);
        let _ = reply.send(result);
    }
}

/// Shared pointer to the current immutable snapshot.
///
/// The lock protects only the current `Arc` pointer. A reader clones the
/// pointer under the read lock and then reads the snapshot without holding any
/// publication or writer lock.
pub struct SnapshotPublisher<S> {
    current: RwLock<Arc<S>>,
}

impl<S> SnapshotPublisher<S> {
    /// Creates the current published snapshot.
    #[must_use]
    pub fn new(initial: S) -> Self {
        Self {
            current: RwLock::new(Arc::new(initial)),
        }
    }

    /// Pins the current immutable value and releases the pointer lock.
    pub fn read(&self) -> Result<SnapshotRead<S>, SnapshotPublicationError> {
        let current = self
            .current
            .read()
            .map_err(|_| SnapshotPublicationError::Poisoned)?;
        Ok(SnapshotRead {
            value: Arc::clone(&current),
        })
    }

    /// Atomically switches the current pointer to one complete new snapshot.
    pub fn publish(&self, snapshot: S) -> Result<(), SnapshotPublicationError> {
        let replacement = Arc::new(snapshot);
        let mut current = self
            .current
            .write()
            .map_err(|_| SnapshotPublicationError::Poisoned)?;
        *current = replacement;
        Ok(())
    }
}

/// An immutable snapshot pointer pinned independently of future publication.
pub struct SnapshotRead<S> {
    value: Arc<S>,
}

impl<S> SnapshotRead<S> {
    /// Returns the immutable snapshot value.
    #[must_use]
    pub fn value(&self) -> &S {
        &self.value
    }
}

impl<S> Deref for SnapshotRead<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

/// Snapshot pointer locking was poisoned; the state was not silently reused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPublicationError {
    /// A publisher or reader panicked while holding the small pointer lock.
    Poisoned,
}

impl std::fmt::Display for SnapshotPublicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => formatter.write_str("snapshot publication lock is poisoned"),
        }
    }
}

impl std::error::Error for SnapshotPublicationError {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{SnapshotPublisher, WriterCloseError, WriterUnavailable, spawn_single_writer};

    #[test]
    fn cloned_handles_submit_to_one_sequential_writer() -> Result<(), String> {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum_active = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let writer_active = Arc::clone(&active);
        let writer_maximum = Arc::clone(&maximum_active);
        let writer_completed = Arc::clone(&completed);
        let (handle, coordinator) = spawn_single_writer(32, move |increment: usize| {
            let now_active = writer_active.fetch_add(1, Ordering::SeqCst) + 1;
            writer_maximum.fetch_max(now_active, Ordering::SeqCst);
            thread::yield_now();
            writer_completed.fetch_add(1, Ordering::SeqCst);
            let result = increment;
            writer_active.fetch_sub(1, Ordering::SeqCst);
            result
        })
        .map_err(|error| error.to_string())?;

        let producer_count = 8;
        let commands_per_producer = 32;
        let start = Arc::new(Barrier::new(producer_count + 1));
        let mut producers = Vec::with_capacity(producer_count);
        for _ in 0..producer_count {
            let producer = Arc::clone(&handle);
            let start = Arc::clone(&start);
            producers.push(thread::spawn(move || -> Result<(), String> {
                start.wait();
                for _ in 0..commands_per_producer {
                    producer
                        .submit(1)
                        .map_err(|error| error.to_string())?
                        .receive()
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            }));
        }
        drop(handle);
        start.wait();
        for producer in producers {
            producer
                .join()
                .map_err(|_| "command producer thread panicked".to_owned())??;
        }
        coordinator.join().map_err(|failure| failure.to_string())?;
        assert_eq!(maximum_active.load(Ordering::SeqCst), 1);
        assert_eq!(
            completed.load(Ordering::SeqCst),
            producer_count * commands_per_producer
        );
        Ok(())
    }

    #[test]
    fn actual_writer_panic_is_observed_and_requires_restart() -> Result<(), String> {
        use crate::jobs::TaskRole;

        let (handle, mut coordinator) = spawn_single_writer::<(), (), _>(1, move |()| {
            std::panic::resume_unwind(Box::new("private writer panic detail"));
        })
        .map_err(|error| error.to_string())?;
        let reply = handle.submit(()).map_err(|error| error.to_string())?;
        drop(handle);

        assert_eq!(reply.receive().err(), Some(WriterUnavailable));
        let report = coordinator
            .close(Instant::now() + Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(report.complete());
        assert_eq!(report.worker_failure(), Some(crate::TaskFailure::Panicked));
        let failure = coordinator
            .join()
            .err()
            .ok_or_else(|| "writer panic was not observed".to_owned())?;
        assert_eq!(
            failure,
            WriterCloseError::WorkerFailed(crate::TaskFailure::Panicked)
        );
        let WriterCloseError::WorkerFailed(failure) = failure else {
            return Err("unexpected writer shutdown error".to_owned());
        };
        assert_eq!(
            failure.terminal_state(TaskRole::Writer),
            crate::JobTerminalState::NeedsRestart
        );
        Ok(())
    }

    #[test]
    fn bounded_close_reports_pending_handoff_then_drains_in_order() -> Result<(), String> {
        let executed = Arc::new(Mutex::new(Vec::new()));
        let writer_executed = Arc::clone(&executed);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (handle, mut coordinator) = spawn_single_writer(1, move |command: u8| {
            if command == 1 {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            }
            if let Ok(mut commands) = writer_executed.lock() {
                commands.push(command);
            }
            command
        })
        .map_err(|error| error.to_string())?;

        let first = handle.submit(1).map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        let second = handle.submit(2).map_err(|error| error.to_string())?;
        let blocked_submitter = Arc::clone(&handle);
        let third = thread::spawn(move || -> Result<u8, String> {
            blocked_submitter
                .submit(3)
                .map_err(|error| error.to_string())?
                .receive()
                .map_err(|error| error.to_string())
        });

        let pending_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let pending = coordinator
                .intake
                .state
                .lock()
                .map_err(|_| "writer intake state was poisoned".to_owned())?
                .submissions_in_flight;
            if pending == 1 {
                break;
            }
            if Instant::now() >= pending_deadline {
                return Err("blocked submission did not enter the mailbox handoff".to_owned());
            }
            thread::yield_now();
        }

        let report = coordinator
            .close(Instant::now() + Duration::from_millis(30))
            .map_err(|error| error.to_string())?;
        assert!(!report.complete());
        assert!(report.worker_unfinished());
        assert_eq!(report.unfinished_submissions(), 1);
        assert_eq!(handle.submit(4).err(), Some(WriterUnavailable));

        release_tx.send(()).map_err(|error| error.to_string())?;
        assert_eq!(first.receive().map_err(|error| error.to_string())?, 1);
        assert_eq!(second.receive().map_err(|error| error.to_string())?, 2);
        assert_eq!(
            third
                .join()
                .map_err(|_| "submitter thread panicked".to_owned())??,
            3
        );
        let report = coordinator
            .close(Instant::now() + Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(report.complete());
        assert_eq!(report.worker_failure(), None);
        assert_eq!(
            *executed
                .lock()
                .map_err(|_| "writer execution log was poisoned".to_owned())?,
            vec![1, 2, 3]
        );
        Ok(())
    }

    #[test]
    fn begin_shutdown_rejects_new_commands_and_drains_accepted_fifo() -> Result<(), String> {
        let executed = Arc::new(Mutex::new(Vec::new()));
        let writer_executed = Arc::clone(&executed);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (handle, mut coordinator) = spawn_single_writer(1, move |command: u8| {
            if command == 1 {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            }
            if let Ok(mut commands) = writer_executed.lock() {
                commands.push(command);
            }
            command
        })
        .map_err(|error| error.to_string())?;

        let first = handle.submit(1).map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("first writer command did not start: {error}"))?;
        let second = handle.submit(2).map_err(|error| error.to_string())?;
        coordinator
            .begin_shutdown()
            .map_err(|error| error.to_string())?;
        assert_eq!(handle.submit(3).err(), Some(WriterUnavailable));

        release_tx.send(()).map_err(|error| error.to_string())?;
        assert_eq!(first.receive().map_err(|error| error.to_string())?, 1);
        assert_eq!(second.receive().map_err(|error| error.to_string())?, 2);
        let report = coordinator
            .close(Instant::now() + Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(report.complete());
        assert_eq!(
            *executed
                .lock()
                .map_err(|_| "writer execution log was poisoned".to_owned())?,
            vec![1, 2]
        );
        Ok(())
    }

    #[test]
    fn dropping_writer_coordinator_is_nonblocking_and_stops_intake() -> Result<(), String> {
        let executed = Arc::new(AtomicUsize::new(0));
        let writer_executed = Arc::clone(&executed);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (handle, coordinator) = spawn_single_writer(1, move |command: u8| {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            writer_executed.fetch_add(usize::from(command), Ordering::SeqCst);
            command
        })
        .map_err(|error| error.to_string())?;
        let accepted = handle.submit(1).map_err(|error| error.to_string())?;
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;

        let (dropped_tx, dropped_rx) = mpsc::sync_channel(1);
        let drop_thread = thread::spawn(move || {
            drop(coordinator);
            let _ = dropped_tx.send(());
        });
        dropped_rx
            .recv_timeout(Duration::from_millis(250))
            .map_err(|_| "dropping writer coordinator waited for active work".to_owned())?;
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert_eq!(handle.submit(2).err(), Some(WriterUnavailable));
        release_tx.send(()).map_err(|error| error.to_string())?;
        assert_eq!(accepted.receive().map_err(|error| error.to_string())?, 1);
        drop_thread
            .join()
            .map_err(|_| "drop helper thread panicked".to_owned())?;
        assert_eq!(executed.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn concurrent_snapshot_readers_hold_old_values_without_blocking_publish() -> Result<(), String>
    {
        let publisher = Arc::new(SnapshotPublisher::new(vec![1_u64, 2, 3]));
        let reader_count = 8;
        let barrier = Arc::new(Barrier::new(reader_count + 1));
        let mut readers = Vec::with_capacity(reader_count);

        for _ in 0..reader_count {
            let reader_publisher = Arc::clone(&publisher);
            let reader_barrier = Arc::clone(&barrier);
            readers.push(thread::spawn(move || -> Result<(), String> {
                let pinned = reader_publisher.read().map_err(|error| error.to_string())?;
                reader_barrier.wait();
                if pinned.value() != &[1, 2, 3] {
                    return Err("reader observed a partial initial snapshot".to_owned());
                }
                reader_barrier.wait();
                if pinned.value() != &[1, 2, 3] {
                    return Err("pinned snapshot changed after publication".to_owned());
                }
                Ok(())
            }));
        }

        barrier.wait();
        publisher
            .publish(vec![4, 5, 6, 7])
            .map_err(|error| error.to_string())?;
        assert_eq!(
            publisher.read().map_err(|error| error.to_string())?.value(),
            &[4, 5, 6, 7]
        );
        barrier.wait();

        for reader in readers {
            reader
                .join()
                .map_err(|_| "snapshot reader thread panicked".to_owned())??;
        }
        Ok(())
    }
}
