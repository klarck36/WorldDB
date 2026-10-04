//! Durable job state and safe, process-restart recovery views.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use worlddb_core::{
    CommitCancellationState, JobKind, JobPhase, JobPool, JobProgress, JobSnapshot, JobSpec,
    JobStatus, JobSupervisor, JobSupervisorLimits, JobTerminalState,
};
use worlddb_storage_file::DatabaseLayout;

const JOURNAL_VERSION: u16 = 1;
const MAX_JOURNAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_JOBS: usize = 256;
const MAX_JOURNAL_FILES: usize = 1024;
const MAX_RESUME_BYTES: usize = 64 * 1024;
const JOURNAL_PREFIX: &str = "JOB_STATE_";
static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

/// User-safe value snapshot shown by the desktop job and shutdown panel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobView {
    pub job_id: String,
    pub kind: JobKindView,
    pub owner: Option<String>,
    pub status: JobStatusView,
    pub phase: JobPhaseView,
    pub progress: JobProgressView,
    pub max_work_units: u64,
    pub max_memory_bytes: u64,
    pub reserved_memory_bytes: u64,
    pub pool: JobPoolView,
    pub cancellation_state: JobCancellationView,
    pub resume_metadata_version: Option<u16>,
    pub resume_metadata_bytes: usize,
    pub failure: Option<String>,
    pub observed_at_unix_ms: u64,
    pub recovered_after_restart: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKindView {
    Migration,
    Backup,
    IndexBuild,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatusView {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    NeedsRestart,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhaseView {
    Queued,
    Starting,
    Scanning,
    Preparing,
    Committing,
    Finalizing,
    Recovering,
    ShuttingDown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobProgressView {
    Indeterminate,
    Determinate { completed: u64, total: u64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPoolView {
    Cpu,
    BlockingIo,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCancellationView {
    Ready,
    CancellationRequested,
    CommitpointPassed,
    Committed,
    NotCommitted,
    OutcomeUnknown,
    InterruptedAfterRestart,
}

/// Durable status of the job history shown to the renderer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobJournalViewStatus {
    Available,
    RecoveredOlderSnapshot,
    WriteFailed,
    Unavailable,
}

/// Bounded list response; journal health is separate from the job outcomes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobListView {
    pub status: JobJournalViewStatus,
    pub jobs: Vec<JobView>,
}

/// Deadline-limited job shutdown result suitable for a versioned desktop response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobShutdownView {
    pub drained: bool,
    pub unfinished_job_ids: Vec<String>,
    pub unfinished_workers: usize,
    pub worker_panics: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct JournalBody {
    version: u16,
    database_id: String,
    generation: u64,
    jobs: BTreeMap<String, JournalEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct JournalEnvelope {
    body: JournalBody,
    checksum: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    view: JobView,
    resume_metadata: Option<Vec<u8>>,
}

pub(crate) struct JobJournal {
    staging: PathBuf,
    database_id: String,
    entries: BTreeMap<String, JournalEntry>,
    generation: u64,
    status: JobJournalViewStatus,
}

impl JobJournal {
    pub(crate) fn open(layout: &DatabaseLayout) -> Result<Self, &'static str> {
        let staging = layout.root().join("staging");
        let metadata = fs::symlink_metadata(&staging).map_err(|_| "journal_unavailable")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("journal_unavailable");
        }
        let canonical = fs::canonicalize(&staging).map_err(|_| "journal_unavailable")?;
        if canonical.parent() != Some(layout.root()) {
            return Err("journal_unavailable");
        }
        let database_id = layout
            .database_id()
            .map(|database_id| database_id.to_string())
            .ok_or("journal_unavailable")?;
        let mut candidates = Vec::new();
        let mut observed_bad = false;
        let mut file_count = 0;
        let mut highest_generation = 0;
        for entry in fs::read_dir(&staging).map_err(|_| "journal_unavailable")? {
            let entry = entry.map_err(|_| "journal_unavailable")?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(generation) = parse_generation(&name) else {
                continue;
            };
            highest_generation = highest_generation.max(generation);
            file_count += 1;
            if file_count > MAX_JOURNAL_FILES {
                return Err("journal_unavailable");
            }
            let path = entry.path();
            match read_envelope(&path, &database_id) {
                Ok(envelope) if envelope.body.generation == generation => {
                    candidates.push((generation, path, envelope.body));
                }
                _ => observed_bad = true,
            }
        }
        candidates.sort_by_key(|(generation, _, _)| *generation);
        let Some((_snapshot_generation, _current_path, mut body)) = candidates.pop() else {
            if observed_bad {
                return Err("journal_unavailable");
            }
            return Ok(Self {
                staging,
                database_id,
                entries: BTreeMap::new(),
                generation: 0,
                status: JobJournalViewStatus::Available,
            });
        };
        let recovered_older = observed_bad;
        for entry in body.jobs.values_mut() {
            if matches!(
                entry.view.status,
                JobStatusView::Queued | JobStatusView::Running
            ) {
                entry.view.status = JobStatusView::Interrupted;
                entry.view.recovered_after_restart = true;
                entry.view.cancellation_state = match entry.view.cancellation_state {
                    JobCancellationView::CancellationRequested => {
                        JobCancellationView::CancellationRequested
                    }
                    _ => JobCancellationView::InterruptedAfterRestart,
                };
            }
        }
        let mut journal = Self {
            staging,
            database_id,
            entries: body.jobs,
            generation: highest_generation,
            status: if recovered_older {
                JobJournalViewStatus::RecoveredOlderSnapshot
            } else {
                JobJournalViewStatus::Available
            },
        };
        if journal
            .entries
            .values()
            .any(|entry| entry.view.recovered_after_restart)
        {
            journal.save()?;
            if recovered_older {
                journal.status = JobJournalViewStatus::RecoveredOlderSnapshot;
            }
        }
        Ok(journal)
    }

    pub(crate) fn merge(&mut self, snapshots: &[JobSnapshot]) -> Result<(), &'static str> {
        let mut changed = false;
        let mut previous_entries = BTreeMap::<String, Option<JournalEntry>>::new();
        for snapshot in snapshots {
            let mut entry = entry_from_snapshot(snapshot);
            let job_id = entry.view.job_id.clone();
            if let Some(previous) = self.entries.get(&job_id) {
                entry.view.recovered_after_restart = previous.view.recovered_after_restart;
                let mut comparable = entry.clone();
                comparable.view.observed_at_unix_ms = previous.view.observed_at_unix_ms;
                if &comparable == previous {
                    continue;
                }
            }
            previous_entries
                .entry(job_id.clone())
                .or_insert_with(|| self.entries.get(&job_id).cloned());
            self.entries.insert(job_id, entry);
            changed = true;
        }
        while self.entries.len() > MAX_JOBS {
            let oldest_terminal = self
                .entries
                .iter()
                .filter(|(_, entry)| {
                    !matches!(
                        entry.view.status,
                        JobStatusView::Queued | JobStatusView::Running
                    )
                })
                .min_by_key(|(_, entry)| entry.view.observed_at_unix_ms)
                .map(|(job_id, _)| job_id.clone());
            let Some(oldest_terminal) = oldest_terminal else {
                self.restore_entries(previous_entries);
                return Err("job_registry_full");
            };
            previous_entries
                .entry(oldest_terminal.clone())
                .or_insert_with(|| self.entries.get(&oldest_terminal).cloned());
            self.entries.remove(&oldest_terminal);
            changed = true;
        }
        if changed {
            if let Err(error) = self.save() {
                self.restore_entries(previous_entries);
                return Err(error);
            }
        }
        Ok(())
    }

    fn restore_entries(&mut self, previous_entries: BTreeMap<String, Option<JournalEntry>>) {
        for (job_id, previous) in previous_entries {
            if let Some(previous) = previous {
                self.entries.insert(job_id, previous);
            } else {
                self.entries.remove(&job_id);
            }
        }
    }

    pub(crate) fn insert_queued(&mut self, spec: JobSpec) -> Result<(), &'static str> {
        let job_id = spec.job_id().to_string();
        if self.entries.contains_key(&job_id) {
            return Err("duplicate_job_id");
        }
        if self.entries.len() >= MAX_JOBS {
            return Err("job_registry_full");
        }
        let now = now_unix_ms();
        self.entries.insert(
            job_id.clone(),
            JournalEntry {
                view: JobView {
                    job_id: job_id.clone(),
                    kind: match spec.kind() {
                        JobKind::Migration => JobKindView::Migration,
                        JobKind::Backup => JobKindView::Backup,
                        JobKind::IndexBuild => JobKindView::IndexBuild,
                    },
                    owner: spec.owner().map(|owner| owner.to_string()),
                    status: JobStatusView::Queued,
                    phase: JobPhaseView::Queued,
                    progress: JobProgressView::Indeterminate,
                    max_work_units: spec.budget().max_work_units(),
                    max_memory_bytes: spec.budget().max_memory_bytes(),
                    reserved_memory_bytes: 0,
                    pool: match spec.pool() {
                        JobPool::Cpu => JobPoolView::Cpu,
                        JobPool::BlockingIo => JobPoolView::BlockingIo,
                    },
                    cancellation_state: JobCancellationView::Ready,
                    resume_metadata_version: None,
                    resume_metadata_bytes: 0,
                    failure: None,
                    observed_at_unix_ms: now,
                    recovered_after_restart: false,
                },
                resume_metadata: None,
            },
        );
        if let Err(error) = self.save() {
            self.entries.remove(&job_id);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn remove_unaccepted(&mut self, job_id: &str) -> Result<(), &'static str> {
        let Some(previous) = self.entries.remove(job_id) else {
            return Ok(());
        };
        if let Err(error) = self.save() {
            self.entries.insert(job_id.to_owned(), previous);
            self.status = JobJournalViewStatus::WriteFailed;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn save(&mut self) -> Result<(), &'static str> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("journal_unavailable")?;
        let body = JournalBody {
            version: JOURNAL_VERSION,
            database_id: self.database_id.clone(),
            generation,
            jobs: self.entries.clone(),
        };
        let canonical = serde_json::to_vec(&body).map_err(|_| "journal_unavailable")?;
        let envelope = JournalEnvelope {
            checksum: blake3::hash(&canonical).to_hex().to_string(),
            body,
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|_| "journal_unavailable")?;
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err("journal_unavailable");
        }
        let target = self
            .staging
            .join(format!("{JOURNAL_PREFIX}{generation:016}.json"));
        if target.exists() {
            return Err("journal_unavailable");
        }
        let (stage, mut file) = self.create_stage_file()?;
        let mut guard = StageGuard {
            path: stage.clone(),
            published: false,
        };
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| "journal_unavailable")?;
        drop(file);
        fs::rename(&stage, &target).map_err(|_| "journal_unavailable")?;
        guard.published = true;
        self.generation = generation;
        self.status = JobJournalViewStatus::Available;
        self.prune(&target);
        Ok(())
    }

    fn create_stage_file(&self) -> Result<(PathBuf, File), &'static str> {
        for _ in 0..16 {
            let sequence = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let path = self
                .staging
                .join(format!(".jobs-{}-{sequence}.tmp", std::process::id()));
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("journal_unavailable"),
            }
        }
        Err("journal_unavailable")
    }

    fn prune(&self, current: &Path) {
        let mut paths = fs::read_dir(&self.staging)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path != current
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| parse_generation(name).is_some())
            })
            .collect::<Vec<_>>();
        paths.sort();
        while paths.len() > 1 {
            if let Some(path) = paths.first().cloned() {
                let _ = fs::remove_file(path);
                paths.remove(0);
            }
        }
    }

    pub(crate) fn response(&self) -> JobListView {
        let mut jobs = self
            .entries
            .values()
            .map(|entry| entry.view.clone())
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| {
            left.observed_at_unix_ms
                .cmp(&right.observed_at_unix_ms)
                .then_with(|| left.job_id.cmp(&right.job_id))
        });
        JobListView {
            status: self.status,
            jobs,
        }
    }

    pub(crate) fn mark_write_failed(&mut self) {
        self.status = JobJournalViewStatus::WriteFailed;
    }
}

struct StageGuard {
    path: PathBuf,
    published: bool,
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn parse_generation(name: &str) -> Option<u64> {
    name.strip_prefix(JOURNAL_PREFIX)?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

fn read_envelope(path: &Path, database_id: &str) -> Result<JournalEnvelope, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "journal_unavailable")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() as usize > MAX_JOURNAL_BYTES
    {
        return Err("journal_unavailable");
    }
    let parent = path.parent().ok_or("journal_unavailable")?;
    if fs::canonicalize(path)
        .map_err(|_| "journal_unavailable")?
        .parent()
        != Some(parent)
    {
        return Err("journal_unavailable");
    }
    let file = File::open(path).map_err(|_| "journal_unavailable")?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_JOURNAL_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "journal_unavailable")?;
    if bytes.len() != metadata.len() as usize || bytes.len() > MAX_JOURNAL_BYTES {
        return Err("journal_unavailable");
    }
    let envelope: JournalEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| "journal_unavailable")?;
    if envelope.body.version != JOURNAL_VERSION || envelope.body.database_id != database_id {
        return Err("journal_unavailable");
    }
    let canonical = serde_json::to_vec(&envelope.body).map_err(|_| "journal_unavailable")?;
    if blake3::hash(&canonical).to_hex().as_str() != envelope.checksum {
        return Err("journal_unavailable");
    }
    for (job_id, entry) in &envelope.body.jobs {
        if job_id != &entry.view.job_id
            || entry
                .resume_metadata
                .as_ref()
                .is_some_and(|bytes| bytes.len() > MAX_RESUME_BYTES)
        {
            return Err("journal_unavailable");
        }
    }
    Ok(envelope)
}

fn entry_from_snapshot(snapshot: &JobSnapshot) -> JournalEntry {
    let descriptor = snapshot.descriptor();
    let progress = match descriptor.progress() {
        JobProgress::Indeterminate => JobProgressView::Indeterminate,
        JobProgress::Determinate(progress) => JobProgressView::Determinate {
            completed: progress.completed(),
            total: progress.total(),
        },
    };
    let resume_metadata = snapshot
        .resume_metadata()
        .map(|metadata| metadata.payload().to_vec());
    let resume_metadata_version = snapshot
        .resume_metadata()
        .map(|metadata| metadata.format_version());
    let resume_metadata_bytes = resume_metadata.as_ref().map_or(0, Vec::len);
    JournalEntry {
        view: JobView {
            job_id: descriptor.job_id().to_string(),
            kind: match descriptor.kind() {
                JobKind::Migration => JobKindView::Migration,
                JobKind::Backup => JobKindView::Backup,
                JobKind::IndexBuild => JobKindView::IndexBuild,
            },
            owner: descriptor.owner().map(|owner| owner.to_string()),
            status: status_view(descriptor.status()),
            phase: phase_view(descriptor.phase()),
            progress,
            max_work_units: descriptor.budget().max_work_units(),
            max_memory_bytes: descriptor.budget().max_memory_bytes(),
            reserved_memory_bytes: snapshot.reserved_memory_bytes(),
            pool: match snapshot.pool() {
                JobPool::Cpu => JobPoolView::Cpu,
                JobPool::BlockingIo => JobPoolView::BlockingIo,
            },
            cancellation_state: cancellation_state_view(snapshot.cancellation_state()),
            resume_metadata_version,
            resume_metadata_bytes,
            failure: snapshot.failure().map(|_| "worker_panicked".to_owned()),
            observed_at_unix_ms: now_unix_ms(),
            recovered_after_restart: false,
        },
        resume_metadata,
    }
}

fn status_view(status: JobStatus) -> JobStatusView {
    match status {
        JobStatus::Queued => JobStatusView::Queued,
        JobStatus::Running => JobStatusView::Running,
        JobStatus::Terminal(terminal) => match terminal {
            JobTerminalState::Succeeded => JobStatusView::Succeeded,
            JobTerminalState::Failed => JobStatusView::Failed,
            JobTerminalState::Cancelled => JobStatusView::Cancelled,
            JobTerminalState::NeedsRestart => JobStatusView::NeedsRestart,
            JobTerminalState::Interrupted => JobStatusView::Interrupted,
        },
    }
}

fn phase_view(phase: JobPhase) -> JobPhaseView {
    match phase {
        JobPhase::Queued => JobPhaseView::Queued,
        JobPhase::Starting => JobPhaseView::Starting,
        JobPhase::Scanning => JobPhaseView::Scanning,
        JobPhase::Preparing => JobPhaseView::Preparing,
        JobPhase::Committing => JobPhaseView::Committing,
        JobPhase::Finalizing => JobPhaseView::Finalizing,
        JobPhase::Recovering => JobPhaseView::Recovering,
        JobPhase::ShuttingDown => JobPhaseView::ShuttingDown,
    }
}

fn cancellation_state_view(state: CommitCancellationState) -> JobCancellationView {
    match state {
        CommitCancellationState::Ready => JobCancellationView::Ready,
        CommitCancellationState::CancellationRequested => {
            JobCancellationView::CancellationRequested
        }
        CommitCancellationState::CommitpointPassed => JobCancellationView::CommitpointPassed,
        CommitCancellationState::Committed => JobCancellationView::Committed,
        CommitCancellationState::NotCommitted => JobCancellationView::NotCommitted,
        CommitCancellationState::UnknownOutcome => JobCancellationView::OutcomeUnknown,
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Creates bounded worker pools for one open engine process.
pub(crate) fn supervisor() -> Result<JobSupervisor, std::io::Error> {
    let limits = JobSupervisorLimits::new(MAX_JOBS, 2, 16, 1, 16, MAX_RESUME_BYTES)
        .map_err(|_| std::io::Error::other("invalid fixed job limits"))?;
    JobSupervisor::new(limits)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{JobJournal, JobJournalViewStatus};
    use worlddb_core::{
        JobBudget, JobCompletion, JobId, JobKind, JobPool, JobSpec, JobSupervisor,
        JobSupervisorLimits,
    };

    static NEXT_JOURNAL_TEST: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn live_job_changes_roll_back_when_the_durable_checkpoint_fails() {
        let root = std::env::temp_dir().join(format!(
            "worlddb-ode-job-journal-write-failure-{}-{}",
            std::process::id(),
            NEXT_JOURNAL_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("test root created");
        let staging_file = root.join("staging-is-a-file");
        fs::write(&staging_file, b"block journal child creation").expect("blocking file created");
        let mut journal = JobJournal {
            staging: staging_file,
            database_id: "test-database".to_owned(),
            entries: BTreeMap::new(),
            generation: 0,
            status: JobJournalViewStatus::Available,
        };
        let mut supervisor = JobSupervisor::new(
            JobSupervisorLimits::new(2, 1, 1, 1, 1, 64).expect("valid supervisor limits"),
        )
        .expect("job supervisor starts");
        let job_id = JobId::from_str("00000000-0000-7000-8000-000000000081").expect("valid job id");
        let spec = JobSpec::new(
            job_id,
            JobKind::Backup,
            None,
            JobBudget::new(10, 4096).expect("finite budget"),
            JobPool::BlockingIo,
        );
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        supervisor
            .submit(spec, move |_| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                JobCompletion::Succeeded
            })
            .expect("job accepted");
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("job started");

        let snapshots = supervisor.list().expect("live snapshots readable");
        assert_eq!(journal.merge(&snapshots), Err("journal_unavailable"));
        let view = journal.response();
        assert_eq!(view.status, JobJournalViewStatus::Available);
        assert!(view.jobs.is_empty(), "unwritten state must not be exposed");
        journal.mark_write_failed();
        assert_eq!(journal.response().status, JobJournalViewStatus::WriteFailed);

        release_tx.send(()).expect("job released");
        let report = supervisor
            .close(std::time::Instant::now() + Duration::from_secs(2))
            .expect("job pool drained");
        assert!(report.drained());
        drop(supervisor);
        fs::remove_dir_all(root).expect("test root removed");
    }
}
