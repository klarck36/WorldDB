use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use worlddb_core::{
    CursorStateStore, CursorStoreLimits, JobCompletion, JobControl, JobId, JobSpec, JobSupervisor,
    OperationId, PrincipalId, QueryHash,
};
use worlddb_storage_file::{DatabaseLayout, FactTokenSearchSession, WriterLock};

mod project;
pub use project::{
    ProjectAccess, ProjectError, create_project, create_project_with_operation_id, open_project,
};
mod entity;
pub use entity::{
    EntityCommand, EntityModeInput, EntityPublicationView, EntityResponse, EntitySnapshotView,
    EntityTypeView, EntityView, EntityWarningView,
};
mod branch_layer;
pub use branch_layer::{
    BranchLayerCommand, BranchLayerPublicationView, BranchLayerResponse, BranchLayerSnapshotView,
    BranchView, LayerView,
};
mod perspective;
pub use perspective::{
    EpistemicModeInput, PerspectiveCommand, PerspectiveContextView, PerspectiveDefinitionView,
    PerspectivePublicationView, PerspectiveResponse, PerspectiveSnapshotView, PerspectiveView,
};
mod security_policy;
pub use security_policy::{
    CapabilityBundleEntryView, CapabilityRuleView, GrantEffectInput, PolicySubjectKindInput,
    PrincipalStateInput, PrincipalView, RoleAssignmentView, RoleView, SecurityPolicyCommand,
    SecurityPolicyPublicationView, SecurityPolicyResponse, SecurityPolicySnapshotView,
};
mod history_space_transfer;
pub use history_space_transfer::{
    ContentIdentityInput, ExternalReferencePolicyInput, HistorySpaceTransferCommand,
    HistorySpaceTransferResponse, TransferCatalogView, TransferContentView, TransferPreviewView,
    TransferPublishedView, TransferRelationView,
};
mod facts;
pub use facts::{
    AssertionCorrectionView, AssertionDraftInput, EventAttributeInput, EventCorrectionView,
    EventDraftInput, EventGraphConflictView, EventParticipantInput, EventRelationKindInput,
    EventTimeInput, FactCatalogRecordView, FactCatalogView, FactCommand, FactConflictFactView,
    FactConflictReportView, FactContextInput, FactGraphCyclePolicyInput, FactGraphDirectionInput,
    FactGraphInput, FactGraphRelationshipInput, FactGraphRootFamilyInput, FactLifecycleActionInput,
    FactLifecycleView, FactOperationStatusKind, FactOperationStatusView, FactPublicationView,
    FactQueryAggregateResultView, FactQueryBudgetView, FactQueryExplainStageView,
    FactQueryGraphEdgeView, FactQueryHistoryDetailView, FactQueryHistoryRecordView,
    FactQueryMaskSelectorView, FactQueryModeInput, FactQueryPolarityGroupView,
    FactQueryRecordContextView, FactQueryRecordRefView, FactQueryResultView,
    FactQuerySchemaModeInput, FactQuerySearchHitView, FactQueryValidityView, FactQueryView,
    FactResponse, FactSearchMatchInput, FactTargetInput, FactValueInput, MaskSelectorInput,
    PolarityInput, ResolutionConflictView, ResolutionOutcomeView, ResolutionPreviewView,
    ResolutionResultView, ResolutionSliceView, ResolutionValueView, ValidityInput,
    WorldTimeSelectorInput,
};
mod schema;
pub use schema::{
    CalendarPeriodDraft, CardinalityDraft, ConstraintDraft, DecimalMetadataDraft,
    EntityConstraintDraft, EventAttributeDraft, EventRoleDraft, EventTimeDraft, EventTimeFormDraft,
    ResolutionPolicyDraft, SchemaCommand, SchemaDefinitionDraft, SchemaDefinitionView,
    SchemaFamily, SchemaLifecycle, SchemaLifecycleUpdateDraft, SchemaModeInput,
    SchemaPublicationView, SchemaResponse, SchemaSnapshotView, TimeBoundDraft, ValueKindDraft,
};
mod jobs;
pub use jobs::{
    JobCancellationView, JobJournalViewStatus, JobKindView, JobListView, JobPhaseView, JobPoolView,
    JobProgressView, JobShutdownView, JobStatusView, JobView,
};
mod recovery;
pub use recovery::{
    RecoveryApplyView, RecoveryFindingView, RecoveryHost, RecoveryInventoryView,
    RecoveryReportView, RecoverySalvageView, inspect_recovery, run_journaled_recovery,
    salvage_recovery,
};

pub const MAX_STREAM_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_STREAM_CHUNK_BYTES: u32 = 1024 * 1024;
const MAX_ACTIVE_STREAMS: usize = 16;
const STREAM_LIFETIME: Duration = Duration::from_secs(5 * 60);
pub const ENGINE_BUILD_ID: &str = env!("WORLDDB_ODE_ENGINE_BUILD_ID");

thread_local! {
    static REQUESTED_OPERATION_ID: Cell<Option<OperationId>> = const { Cell::new(None) };
}

/// Runs one synchronous engine operation with the caller's durable OperationId.
/// The scope is thread-local so concurrent windows cannot exchange identities.
pub fn with_operation_id<T>(operation_id: OperationId, operation: impl FnOnce() -> T) -> T {
    struct RestoreOperationId(Option<OperationId>);

    impl Drop for RestoreOperationId {
        fn drop(&mut self) {
            REQUESTED_OPERATION_ID.with(|current| current.set(self.0));
        }
    }

    let previous = REQUESTED_OPERATION_ID.with(|current| current.replace(Some(operation_id)));
    let _restore = RestoreOperationId(previous);
    operation()
}

pub(crate) fn requested_operation_id_or<E>(
    generate: impl FnOnce() -> Result<OperationId, E>,
) -> Result<OperationId, E> {
    requested_operation_id().map_or_else(generate, Ok)
}

pub(crate) fn requested_operation_id() -> Option<OperationId> {
    REQUESTED_OPERATION_ID.with(Cell::get)
}

/// Derives the project Principal from authenticated host-account identity bytes.
/// Callers must obtain these bytes from the operating-system process token.
pub fn derive_host_account_principal(identity: &[u8]) -> Result<PrincipalId, ProjectError> {
    worlddb_core::derive_host_account_principal(identity)
        .map_err(|_| ProjectError::UnsupportedIdentity)
}

pub struct EngineHost {
    _layout: DatabaseLayout,
    _writer_lock: WriterLock,
    principal_id: Option<PrincipalId>,
    health: Mutex<()>,
    schema_management: Mutex<()>,
    streams: Mutex<HashMap<[u8; 16], ActiveStream>>,
    transfer_previews: Mutex<HashMap<String, history_space_transfer::PendingTransferPreview>>,
    query_cursors: Mutex<CursorStateStore>,
    fact_search_sessions: Mutex<HashMap<QueryHash, FactTokenSearchSession>>,
    jobs: JobSupervisor,
    job_journal: Mutex<Option<jobs::JobJournal>>,
    clock_origin: Instant,
}

fn new_query_cursor_store() -> Result<CursorStateStore, String> {
    let limits =
        CursorStoreLimits::new(32, 1_048_576, 60_000).map_err(|error| error.to_string())?;
    CursorStateStore::new(limits).map_err(|error| error.to_string())
}

struct ActiveStream {
    expires_at: Instant,
    next_sequence: u64,
    bytes_received: u64,
    chunk_bytes: u32,
    consumer: StreamConsumer,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Request {
    Health,
    JobsList,
    JobCancel {
        job_id: String,
    },
    RecoveryInspect,
    RecoveryRun,
    RecoverySalvage {
        destination: PathBuf,
    },
    StreamSink {
        total_bytes: u64,
        chunk_bytes: u32,
        cancel_after_bytes: Option<u64>,
    },
    StreamStart {
        transfer_id: String,
        total_bytes: u64,
        chunk_bytes: u32,
    },
    StreamChunk {
        transfer_id: String,
        sequence: u64,
        chunk_bytes: u32,
    },
    StreamFinish {
        transfer_id: String,
        cancelled: bool,
    },
    Schema {
        #[serde(default)]
        operation_id: Option<String>,
        command: SchemaCommand,
    },
    BranchLayers {
        #[serde(default)]
        operation_id: Option<String>,
        command: BranchLayerCommand,
    },
    Entities {
        #[serde(default)]
        operation_id: Option<String>,
        command: EntityCommand,
    },
    Perspectives {
        #[serde(default)]
        operation_id: Option<String>,
        command: PerspectiveCommand,
    },
    SecurityPolicy {
        #[serde(default)]
        operation_id: Option<String>,
        command: SecurityPolicyCommand,
    },
    HistorySpaceTransfer {
        #[serde(default)]
        operation_id: Option<String>,
        command: HistorySpaceTransferCommand,
    },
    Facts {
        #[serde(default)]
        operation_id: Option<String>,
        command: Box<FactCommand>,
    },
    Panic,
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ready {
        engine_process_id: u32,
        engine_build_id: String,
        protocol_version: u16,
    },
    Health {
        engine_process_id: u32,
        writer_owned: bool,
    },
    Jobs {
        result: JobListView,
    },
    JobCancel {
        disposition: String,
        jobs: JobListView,
    },
    RecoveryReport {
        result: RecoveryReportView,
    },
    RecoveryApplied {
        result: RecoveryApplyView,
    },
    RecoverySalvaged {
        result: RecoverySalvageView,
    },
    StreamComplete {
        bytes_read: u64,
        digest: String,
        cancelled: bool,
    },
    StreamStarted {
        transfer_id: String,
    },
    StreamChunkAccepted {
        transfer_id: String,
        sequence: u64,
        bytes_received: u64,
    },
    Schema {
        result: SchemaResponse,
    },
    BranchLayers {
        result: BranchLayerResponse,
    },
    Entities {
        result: EntityResponse,
    },
    Perspectives {
        result: PerspectiveResponse,
    },
    SecurityPolicy {
        result: SecurityPolicyResponse,
    },
    HistorySpaceTransfer {
        result: HistorySpaceTransferResponse,
    },
    Facts {
        result: Box<FactResponse>,
    },
    Shutdown {
        result: JobShutdownView,
    },
    Error {
        code: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct StreamPlan {
    pub total_bytes: u64,
    pub chunk_bytes: u32,
    pub cancel_after_bytes: Option<u64>,
}

impl StreamPlan {
    pub fn validate(self) -> Result<Self, EngineError> {
        if self.total_bytes == 0 || self.total_bytes > MAX_STREAM_BYTES {
            return Err(EngineError::Stream("invalid_total_bytes"));
        }
        if self.chunk_bytes == 0 || self.chunk_bytes > MAX_STREAM_CHUNK_BYTES {
            return Err(EngineError::Stream("invalid_chunk_bytes"));
        }
        if self
            .cancel_after_bytes
            .is_some_and(|value| value == 0 || value >= self.total_bytes)
        {
            return Err(EngineError::Stream("invalid_cancel_after_bytes"));
        }
        Ok(self)
    }

    pub fn target_bytes(self) -> u64 {
        self.cancel_after_bytes.unwrap_or(self.total_bytes)
    }
}

pub struct StreamConsumer {
    plan: StreamPlan,
    bytes_read: u64,
    hasher: blake3::Hasher,
}

impl StreamConsumer {
    pub fn new(plan: StreamPlan) -> Result<Self, EngineError> {
        Ok(Self {
            plan: plan.validate()?,
            bytes_read: 0,
            hasher: blake3::Hasher::new(),
        })
    }

    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), EngineError> {
        if chunk.is_empty()
            || chunk.len() > self.plan.chunk_bytes as usize
            || self.bytes_read + chunk.len() as u64 > self.plan.target_bytes()
        {
            return Err(EngineError::Stream("invalid_data_frame"));
        }
        self.hasher.update(chunk);
        self.bytes_read += chunk.len() as u64;
        Ok(())
    }

    pub fn finish(self, cancelled: bool) -> Result<StreamReport, EngineError> {
        if cancelled {
            let cancellation_boundary_matches = match self.plan.cancel_after_bytes {
                Some(configured) => self.bytes_read == configured,
                None => self.bytes_read < self.plan.total_bytes,
            };
            if !cancellation_boundary_matches {
                return Err(EngineError::Stream("invalid_cancel_frame"));
            }
        } else if self.bytes_read != self.plan.total_bytes {
            return Err(EngineError::Stream("incomplete_stream"));
        }

        Ok(StreamReport {
            bytes_read: self.bytes_read,
            digest: self.hasher.finalize().to_hex().to_string(),
            cancelled,
        })
    }
}

#[derive(Clone, Debug)]
pub struct StreamReport {
    pub bytes_read: u64,
    pub digest: String,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum EngineError {
    Storage(String),
    Stream(&'static str),
    Schema(String),
    Entity(String),
    BranchLayer(String),
    Perspective(String),
    SecurityPolicy(String),
    Jobs(String),
    Recovery(&'static str),
    HistorySpaceTransfer(String),
    Fact(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(message) => formatter.write_str(message),
            Self::Stream(code) => write!(formatter, "stream protocol error: {code}"),
            Self::Schema(message) => formatter.write_str(message),
            Self::Entity(message) => formatter.write_str(message),
            Self::BranchLayer(message) => formatter.write_str(message),
            Self::Perspective(message) => formatter.write_str(message),
            Self::SecurityPolicy(message) => formatter.write_str(message),
            Self::Jobs(message) => formatter.write_str(message),
            Self::Recovery(code) => write!(formatter, "recovery operation failed: {code}"),
            Self::HistorySpaceTransfer(message) => formatter.write_str(message),
            Self::Fact(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for EngineError {}

impl EngineHost {
    pub fn open(database_root: &Path) -> Result<Self, EngineError> {
        let layout = if database_root.exists() {
            DatabaseLayout::open(database_root)
        } else {
            DatabaseLayout::create(database_root)
        }
        .map_err(|_| EngineError::Storage("database open failed".to_owned()))?;
        let writer_lock = layout
            .try_writer_lock()
            .map_err(|_| EngineError::Storage("database writer lock unavailable".to_owned()))?;
        let query_cursors =
            new_query_cursor_store().map_err(|error| EngineError::Storage(error.to_owned()))?;
        let job_supervisor = jobs::supervisor()
            .map_err(|_| EngineError::Storage("job worker pool unavailable".to_owned()))?;
        let job_journal = jobs::JobJournal::open(&layout).ok();
        Ok(Self {
            _layout: layout,
            _writer_lock: writer_lock,
            principal_id: None,
            health: Mutex::new(()),
            schema_management: Mutex::new(()),
            streams: Mutex::new(HashMap::new()),
            transfer_previews: Mutex::new(HashMap::new()),
            query_cursors: Mutex::new(query_cursors),
            fact_search_sessions: Mutex::new(HashMap::new()),
            jobs: job_supervisor,
            job_journal: Mutex::new(job_journal),
            clock_origin: Instant::now(),
        })
    }

    /// Opens a verified project only when the authenticated host Principal has current
    /// `ProjectRead` permission. The Principal is supplied by the trusted desktop host.
    pub fn open_authorized(
        project_root: &Path,
        principal_id: PrincipalId,
    ) -> Result<(Self, ProjectAccess), ProjectError> {
        let layout =
            DatabaseLayout::open(project_root).map_err(|_| ProjectError::InvalidProject)?;
        let writer_lock = layout.try_writer_lock().map_err(|error| match error {
            worlddb_storage_file::WriterLockError::AlreadyHeld => ProjectError::AlreadyOpen,
            worlddb_storage_file::WriterLockError::LockFileMissing
            | worlddb_storage_file::WriterLockError::Io(_) => ProjectError::HostUnavailable,
        })?;
        let access = project::resolve_open_access(&layout, &writer_lock, principal_id)?;
        let query_cursors = new_query_cursor_store().map_err(|_| ProjectError::HostUnavailable)?;
        let job_supervisor = jobs::supervisor().map_err(|_| ProjectError::HostUnavailable)?;
        let job_journal = jobs::JobJournal::open(&layout).ok();
        Ok((
            Self {
                _layout: layout,
                _writer_lock: writer_lock,
                principal_id: Some(principal_id),
                health: Mutex::new(()),
                schema_management: Mutex::new(()),
                streams: Mutex::new(HashMap::new()),
                transfer_previews: Mutex::new(HashMap::new()),
                query_cursors: Mutex::new(query_cursors),
                fact_search_sessions: Mutex::new(HashMap::new()),
                jobs: job_supervisor,
                job_journal: Mutex::new(job_journal),
                clock_origin: Instant::now(),
            },
            access,
        ))
    }

    /// The internally bound host Principal, if this host opened an authorized project.
    #[must_use]
    pub const fn principal_id(&self) -> Option<PrincipalId> {
        self.principal_id
    }

    /// Lists live and restart-recovered jobs from the checksummed durable journal.
    pub fn list_jobs(&self) -> Result<JobListView, EngineError> {
        let snapshots = self
            .jobs
            .list()
            .map_err(|_| EngineError::Jobs("job state unavailable".to_owned()))?;
        let mut journal = self
            .job_journal
            .lock()
            .map_err(|_| EngineError::Jobs("job journal unavailable".to_owned()))?;
        let Some(journal) = journal.as_mut() else {
            return Ok(JobListView {
                status: jobs::JobJournalViewStatus::Unavailable,
                jobs: Vec::new(),
            });
        };
        if journal.merge(&snapshots).is_err() {
            journal.mark_write_failed();
        }
        Ok(journal.response())
    }

    /// Admits one bounded background task only after its queued state is durable.
    pub fn submit_job(
        &self,
        spec: JobSpec,
        work: impl FnOnce(JobControl) -> JobCompletion + Send + 'static,
    ) -> Result<(), EngineError> {
        let job_id = spec.job_id().to_string();
        {
            let mut journal = self
                .job_journal
                .lock()
                .map_err(|_| EngineError::Jobs("job journal unavailable".to_owned()))?;
            let Some(journal) = journal.as_mut() else {
                return Err(EngineError::Jobs("job journal unavailable".to_owned()));
            };
            journal
                .insert_queued(spec)
                .map_err(|_| EngineError::Jobs("job journal unavailable".to_owned()))?;
        }
        if let Err(error) = self.jobs.submit(spec, work) {
            if let Ok(mut journal) = self.job_journal.lock() {
                if let Some(journal) = journal.as_mut() {
                    let _ = journal.remove_unaccepted(&job_id);
                }
            }
            return Err(EngineError::Jobs(error.to_string()));
        }
        let _ = self.list_jobs();
        Ok(())
    }

    /// Requests cooperative cancellation and reports whether publication made it too late.
    pub fn cancel_job(
        &self,
        job_id: JobId,
    ) -> Result<worlddb_core::CancellationRequestDisposition, EngineError> {
        let disposition = self
            .jobs
            .cancel_with_disposition(job_id)
            .map_err(|_| EngineError::Jobs("job cancellation unavailable".to_owned()))?;
        let _ = self.list_jobs();
        Ok(disposition)
    }

    /// Stops job intake and drains workers to one bounded deadline.
    pub fn shutdown_jobs(&mut self, deadline: Instant) -> Result<JobShutdownView, EngineError> {
        let report = self
            .jobs
            .close(deadline)
            .map_err(|_| EngineError::Jobs("job shutdown state unavailable".to_owned()))?;
        let _ = self.list_jobs();
        Ok(JobShutdownView {
            drained: report.drained(),
            unfinished_job_ids: report
                .unfinished_jobs()
                .iter()
                .map(ToString::to_string)
                .collect(),
            unfinished_workers: report.unfinished_workers(),
            worker_panics: report.worker_panics(),
        })
    }

    pub(crate) fn query_now_ms(&self) -> u64 {
        u64::try_from(self.clock_origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub fn health(&self) -> Result<Response, EngineError> {
        let _guard = self
            .health
            .lock()
            .map_err(|_| EngineError::Storage("engine state is poisoned".to_owned()))?;
        Ok(Response::Health {
            engine_process_id: std::process::id(),
            writer_owned: true,
        })
    }

    pub fn stream_synthetic(&self, plan: StreamPlan) -> Result<StreamReport, EngineError> {
        let _ = self.health()?;
        let plan = plan.validate()?;
        let mut consumer = StreamConsumer::new(plan)?;
        let mut offset = 0;
        while offset < plan.target_bytes() {
            let length = (plan.target_bytes() - offset).min(plan.chunk_bytes as u64) as usize;
            let mut chunk = vec![0; length];
            fill_deterministic_chunk(&mut chunk, offset);
            consumer.push_chunk(&chunk)?;
            offset += length as u64;
        }
        consumer.finish(plan.cancel_after_bytes.is_some())
    }

    pub fn begin_stream(
        &self,
        transfer_id: &str,
        total_bytes: u64,
        chunk_bytes: u32,
    ) -> Result<(), EngineError> {
        let _ = self.health()?;
        let transfer_key = decode_stream_id(transfer_id)?;
        let consumer = StreamConsumer::new(StreamPlan {
            total_bytes,
            chunk_bytes,
            cancel_after_bytes: None,
        })?;
        let now = Instant::now();
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| EngineError::Storage("engine state is poisoned".to_owned()))?;
        streams.retain(|_, stream| stream.expires_at > now);
        if streams.len() >= MAX_ACTIVE_STREAMS || streams.contains_key(&transfer_key) {
            return Err(EngineError::Stream("too_many_active_streams"));
        }
        streams.insert(
            transfer_key,
            ActiveStream {
                expires_at: now + STREAM_LIFETIME,
                next_sequence: 0,
                bytes_received: 0,
                chunk_bytes,
                consumer,
            },
        );
        Ok(())
    }

    pub fn push_stream_chunk(
        &self,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
    ) -> Result<u64, EngineError> {
        let _ = self.health()?;
        let transfer_key = decode_stream_id(transfer_id)?;
        let now = Instant::now();
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| EngineError::Storage("engine state is poisoned".to_owned()))?;
        let Some(stream) = streams.get_mut(&transfer_key) else {
            return Err(EngineError::Stream("unknown_transfer"));
        };
        if stream.expires_at <= now {
            streams.remove(&transfer_key);
            return Err(EngineError::Stream("expired_transfer"));
        }
        if sequence != stream.next_sequence
            || chunk.is_empty()
            || chunk.len() > stream.chunk_bytes as usize
        {
            return Err(EngineError::Stream("invalid_data_frame"));
        }
        stream.consumer.push_chunk(chunk)?;
        stream.bytes_received += chunk.len() as u64;
        stream.next_sequence += 1;
        Ok(stream.bytes_received)
    }

    pub fn finish_stream(
        &self,
        transfer_id: &str,
        cancelled: bool,
    ) -> Result<StreamReport, EngineError> {
        let _ = self.health()?;
        let transfer_key = decode_stream_id(transfer_id)?;
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| EngineError::Storage("engine state is poisoned".to_owned()))?;
        let Some(stream) = streams.get(&transfer_key) else {
            return Err(EngineError::Stream("unknown_transfer"));
        };
        if stream.expires_at <= Instant::now() {
            streams.remove(&transfer_key);
            return Err(EngineError::Stream("expired_transfer"));
        }
        let stream = streams
            .remove(&transfer_key)
            .ok_or(EngineError::Stream("unknown_transfer"))?;
        stream.consumer.finish(cancelled)
    }

    pub fn panic_for_spike(&self) {
        let _health = self
            .health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        panic!("ODE-002 injected engine panic");
    }
}

fn decode_stream_id(encoded: &str) -> Result<[u8; 16], EngineError> {
    if encoded.len() != 32 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(EngineError::Stream("invalid_transfer_id"));
    }
    let mut id = [0_u8; 16];
    for (index, byte) in id.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&encoded[start..start + 2], 16)
            .map_err(|_| EngineError::Stream("invalid_transfer_id"))?;
    }
    Ok(id)
}

#[cfg(test)]
fn fuzz_campaign_probe(target: &str, bytes: &[u8]) -> Result<bool, String> {
    let accepted = match target {
        "engine_ipc_request" => serde_json::from_slice::<Request>(bytes).is_ok(),
        "engine_fact_command" => serde_json::from_slice::<FactCommand>(bytes).is_ok(),
        "engine_schema_command" => serde_json::from_slice::<SchemaCommand>(bytes).is_ok(),
        "engine_branch_layer_command" => {
            serde_json::from_slice::<BranchLayerCommand>(bytes).is_ok()
        }
        "engine_entity_command" => serde_json::from_slice::<EntityCommand>(bytes).is_ok(),
        "engine_perspective_command" => serde_json::from_slice::<PerspectiveCommand>(bytes).is_ok(),
        "engine_security_policy_command" => {
            serde_json::from_slice::<SecurityPolicyCommand>(bytes).is_ok()
        }
        "engine_history_space_transfer_command" => {
            serde_json::from_slice::<HistorySpaceTransferCommand>(bytes).is_ok()
        }
        "engine_query_cursor" => facts::fuzz_cursor(bytes),
        "engine_job_journal" => jobs::fuzz_job_journal(bytes),
        "engine_stream_id" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| decode_stream_id(value.trim()).is_ok()),
        _ => return Err(format!("unknown engine fuzz target: {target}")),
    };
    Ok(accepted)
}

#[cfg(test)]
#[path = "../../../../../tools/fuzz/rust_campaign.rs"]
mod fuzz_campaign_support;

#[cfg(test)]
#[test]
#[ignore = "24-hour fuzz campaign; run through tools/fuzz/run-target.ps1"]
fn ode_fuzz_campaign() {
    if let Err(error) = fuzz_campaign_support::run_campaign(
        &[
            "engine_ipc_request",
            "engine_fact_command",
            "engine_schema_command",
            "engine_branch_layer_command",
            "engine_entity_command",
            "engine_perspective_command",
            "engine_security_policy_command",
            "engine_history_space_transfer_command",
            "engine_query_cursor",
            "engine_job_journal",
            "engine_stream_id",
        ],
        fuzz_campaign_probe,
    ) {
        panic!("ODE fuzz campaign failed: {error}");
    }
}

#[cfg(test)]
mod fuzz_campaign_tests {
    use super::fuzz_campaign_probe;

    const TARGETS: &[&str] = &[
        "engine_ipc_request",
        "engine_fact_command",
        "engine_schema_command",
        "engine_branch_layer_command",
        "engine_entity_command",
        "engine_perspective_command",
        "engine_security_policy_command",
        "engine_history_space_transfer_command",
        "engine_query_cursor",
        "engine_job_journal",
        "engine_stream_id",
    ];

    #[test]
    fn fuzz_campaign_dispatch_rejects_unregistered_engine_targets() {
        assert!(fuzz_campaign_probe("unknown", b"seed").is_err());
        for target in TARGETS {
            assert!(fuzz_campaign_probe(target, b"seed").is_ok());
        }
    }
}

pub fn fill_deterministic_chunk(chunk: &mut [u8], offset: u64) {
    for (index, byte) in chunk.iter_mut().enumerate() {
        *byte = ((offset
            .wrapping_add(index as u64)
            .wrapping_mul(31)
            .wrapping_add(17))
            % 251) as u8;
    }
}

pub fn stream_response(report: StreamReport) -> Response {
    Response::StreamComplete {
        bytes_read: report.bytes_read,
        digest: report.digest,
        cancelled: report.cancelled,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{
        EngineHost, JobCancellationView, JobJournalViewStatus, JobStatusView, OperationId,
        StreamConsumer, StreamPlan, fill_deterministic_chunk, requested_operation_id_or,
        with_operation_id,
    };
    use worlddb_core::{
        CancellationRequestDisposition, JobBudget, JobCompletion, JobId, JobKind, JobPhase,
        JobPool, JobProgress, JobSpec,
    };

    static NEXT_TEST_DATABASE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn client_operation_id_is_scoped_to_one_synchronous_engine_call() {
        let requested = "00000000-0000-7000-8000-000000000001"
            .parse::<OperationId>()
            .expect("canonical OperationId");
        let generated = "00000000-0000-7000-8000-000000000002"
            .parse::<OperationId>()
            .expect("canonical OperationId");

        let used = with_operation_id(requested, || {
            requested_operation_id_or(|| Ok::<_, std::convert::Infallible>(generated))
                .expect("requested identity")
        });
        assert_eq!(used, requested);

        let outside_scope =
            requested_operation_id_or(|| Ok::<_, std::convert::Infallible>(generated))
                .expect("fallback identity");
        assert_eq!(outside_scope, generated);
    }

    #[test]
    fn full_stream_digest_is_deterministic_across_chunk_boundaries() {
        let plan = StreamPlan {
            total_bytes: 17,
            chunk_bytes: 8,
            cancel_after_bytes: None,
        };
        let mut consumer = StreamConsumer::new(plan).expect("valid plan");
        let mut expected = Vec::new();
        let mut offset = 0;
        while offset < plan.total_bytes {
            let length = (plan.total_bytes - offset).min(plan.chunk_bytes as u64) as usize;
            let mut chunk = vec![0; length];
            fill_deterministic_chunk(&mut chunk, offset);
            expected.extend_from_slice(&chunk);
            consumer.push_chunk(&chunk).expect("valid chunk");
            offset += length as u64;
        }
        let report = consumer.finish(false).expect("complete stream");
        assert_eq!(report.bytes_read, 17);
        assert!(!report.cancelled);
        assert_eq!(report.digest, blake3::hash(&expected).to_hex().to_string());
    }

    #[test]
    fn cancellation_accepts_only_the_exact_requested_prefix() {
        let plan = StreamPlan {
            total_bytes: 17,
            chunk_bytes: 8,
            cancel_after_bytes: Some(10),
        };
        let mut consumer = StreamConsumer::new(plan).expect("valid plan");
        consumer.push_chunk(&[1; 8]).expect("first frame");
        consumer.push_chunk(&[2; 2]).expect("last partial frame");
        let report = consumer
            .finish(true)
            .expect("requested cancellation boundary");
        assert_eq!(report.bytes_read, 10);
        assert!(report.cancelled);

        let mut incomplete = StreamConsumer::new(plan).expect("valid plan");
        incomplete.push_chunk(&[1; 8]).expect("first frame");
        assert!(incomplete.finish(true).is_err());
    }

    #[test]
    fn interactive_cancellation_accepts_any_prefix_before_completion() {
        let plan = StreamPlan {
            total_bytes: 17,
            chunk_bytes: 8,
            cancel_after_bytes: None,
        };
        let mut consumer = StreamConsumer::new(plan).expect("valid plan");
        consumer.push_chunk(&[1; 8]).expect("first frame");
        consumer.push_chunk(&[2; 3]).expect("partial frame");
        let report = consumer.finish(true).expect("arbitrary cancel point");
        assert_eq!(report.bytes_read, 11);
        assert!(report.cancelled);

        let empty = StreamConsumer::new(plan).expect("valid plan");
        assert_eq!(
            empty
                .finish(true)
                .expect("cancel before first chunk")
                .bytes_read,
            0
        );

        let mut completed = StreamConsumer::new(plan).expect("valid plan");
        completed.push_chunk(&[3; 8]).expect("first frame");
        completed.push_chunk(&[4; 8]).expect("second frame");
        completed.push_chunk(&[5]).expect("last frame");
        assert!(completed.finish(true).is_err());
    }

    #[test]
    fn stream_plan_rejects_unbounded_chunks_and_bad_cancellation_points() {
        assert!(
            StreamPlan {
                total_bytes: 17,
                chunk_bytes: 0,
                cancel_after_bytes: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            StreamPlan {
                total_bytes: 17,
                chunk_bytes: 1,
                cancel_after_bytes: Some(17),
            }
            .validate()
            .is_err()
        );
        assert!(
            StreamPlan {
                total_bytes: super::MAX_STREAM_BYTES + 1,
                chunk_bytes: 1,
                cancel_after_bytes: None,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn engine_stream_requires_ordered_chunks_and_supports_interactive_cancellation() {
        let database_root = std::env::temp_dir().join(format!(
            "worlddb-ode-engine-test-{}-{}",
            std::process::id(),
            NEXT_TEST_DATABASE.fetch_add(1, Ordering::Relaxed)
        ));
        let engine = super::EngineHost::open(&database_root).expect("test engine open");

        let full_id = "00000000000000000000000000000001";
        engine.begin_stream(full_id, 10, 4).expect("stream starts");
        assert!(engine.push_stream_chunk(full_id, 1, &[1; 4]).is_err());
        assert_eq!(engine.push_stream_chunk(full_id, 0, &[1; 4]).unwrap(), 4);
        assert_eq!(engine.push_stream_chunk(full_id, 1, &[2; 4]).unwrap(), 8);
        assert_eq!(engine.push_stream_chunk(full_id, 2, &[3; 2]).unwrap(), 10);
        let full = engine
            .finish_stream(full_id, false)
            .expect("stream completes");
        assert_eq!(full.bytes_read, 10);
        assert!(!full.cancelled);

        let cancel_id = "00000000000000000000000000000002";
        engine
            .begin_stream(cancel_id, 10, 4)
            .expect("stream starts");
        engine
            .push_stream_chunk(cancel_id, 0, &[4; 3])
            .expect("partial chunk accepted");
        let cancelled = engine
            .finish_stream(cancel_id, true)
            .expect("interactive cancellation");
        assert_eq!(cancelled.bytes_read, 3);
        assert!(cancelled.cancelled);

        drop(engine);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn durable_job_journal_shows_last_evidence_as_interrupted_after_restart() {
        let database_root = std::env::temp_dir().join(format!(
            "worlddb-ode-job-restart-{}-{}",
            std::process::id(),
            NEXT_TEST_DATABASE.fetch_add(1, Ordering::Relaxed)
        ));
        let engine = EngineHost::open(&database_root).expect("test engine open");
        let job_id = JobId::from_str("00000000-0000-7000-8000-000000000077").expect("valid job id");
        let budget = JobBudget::new(100, 4096).expect("finite job budget");
        let spec = JobSpec::new(job_id, JobKind::Backup, None, budget, JobPool::BlockingIo);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        engine
            .submit_job(spec, move |control| {
                if control.report_phase(JobPhase::Scanning).is_err()
                    || control
                        .report_progress(JobProgress::determinate(2, 10).expect("valid progress"))
                        .is_err()
                    || control.set_resume_metadata(1, vec![7, 8, 9]).is_err()
                {
                    return JobCompletion::Failed;
                }
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                JobCompletion::Succeeded
            })
            .expect("job accepted after its initial state was durable");
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("job reached its recorded phase");
        let before_restart = engine.list_jobs().expect("running job is readable");
        assert_eq!(before_restart.status, JobJournalViewStatus::Available);
        assert_eq!(before_restart.jobs.len(), 1);
        assert_eq!(before_restart.jobs[0].status, JobStatusView::Running);
        assert_eq!(before_restart.jobs[0].phase, super::JobPhaseView::Scanning);
        assert_eq!(before_restart.jobs[0].resume_metadata_version, Some(1));
        drop(engine);

        let restarted = EngineHost::open(&database_root).expect("database reopens");
        let after_restart = restarted.list_jobs().expect("recovered job is readable");
        assert_eq!(after_restart.jobs.len(), 1);
        assert_eq!(after_restart.jobs[0].status, JobStatusView::Interrupted);
        assert_eq!(after_restart.jobs[0].phase, super::JobPhaseView::Scanning);
        assert_eq!(
            after_restart.jobs[0].progress,
            super::JobProgressView::Determinate {
                completed: 2,
                total: 10,
            }
        );
        assert_eq!(after_restart.jobs[0].resume_metadata_version, Some(1));
        assert!(after_restart.jobs[0].recovered_after_restart);
        assert_ne!(after_restart.status, JobJournalViewStatus::Unavailable);
        release_tx.send(()).expect("detached test worker released");
        drop(restarted);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn engine_cancel_reports_too_late_and_persists_commitpoint_evidence() {
        let database_root = std::env::temp_dir().join(format!(
            "worlddb-ode-job-cancel-{}-{}",
            std::process::id(),
            NEXT_TEST_DATABASE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut engine = EngineHost::open(&database_root).expect("test engine open");
        let job_id = JobId::from_str("00000000-0000-7000-8000-000000000078").expect("valid job id");
        let budget = JobBudget::new(100, 4096).expect("finite job budget");
        let spec = JobSpec::new(
            job_id,
            JobKind::Migration,
            None,
            budget,
            JobPool::BlockingIo,
        );
        let (commitpoint_tx, commitpoint_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        engine
            .submit_job(spec, move |control| {
                if control.report_phase(JobPhase::Committing).is_err() {
                    return JobCompletion::Failed;
                }
                let permit = match control.begin_commitpoint() {
                    Ok(permit) => permit,
                    Err(_) => return JobCompletion::Failed,
                };
                let _ = commitpoint_tx.send(());
                if release_rx.recv().is_err() {
                    return JobCompletion::Failed;
                }
                permit.committed();
                JobCompletion::Succeeded
            })
            .expect("job accepted");
        commitpoint_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("job entered the commitpoint");

        assert_eq!(
            engine.cancel_job(job_id).expect("cancellation reported"),
            CancellationRequestDisposition::TooLate
        );
        let after_cancel = engine.list_jobs().expect("job state is readable");
        assert_eq!(after_cancel.jobs.len(), 1);
        assert_eq!(after_cancel.jobs[0].status, JobStatusView::Running);
        assert_eq!(after_cancel.jobs[0].phase, super::JobPhaseView::Committing);
        assert_eq!(
            after_cancel.jobs[0].cancellation_state,
            JobCancellationView::CommitpointPassed
        );

        release_tx.send(()).expect("commit released");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let completed = loop {
            let jobs = engine.list_jobs().expect("terminal job is readable");
            let job = jobs.jobs.first().expect("job retained");
            if job.status == JobStatusView::Succeeded {
                break job.clone();
            }
            assert!(std::time::Instant::now() < deadline, "job did not finish");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(completed.cancellation_state, JobCancellationView::Committed);
        let shutdown = engine
            .shutdown_jobs(std::time::Instant::now() + Duration::from_secs(2))
            .expect("job pool drained");
        assert!(shutdown.drained);
        drop(engine);
        let _ = std::fs::remove_dir_all(database_root);
    }
}
