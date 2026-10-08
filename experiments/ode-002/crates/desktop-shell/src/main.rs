use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Instant;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
#[cfg(feature = "in-process")]
use worlddb_ode_engine::EngineHost;
#[cfg(feature = "sidecar")]
use worlddb_ode_engine::Request;
use worlddb_ode_engine::{
    BranchLayerCommand, BranchLayerResponse, EntityCommand, EntityModeInput, EntityResponse,
    FactCommand, FactQueryModeInput, FactQuerySchemaModeInput, FactResponse,
    HistorySpaceTransferCommand, HistorySpaceTransferResponse, JobListView, JobShutdownView,
    MaskSelectorInput, PerspectiveCommand, PerspectiveResponse, RecoveryApplyView,
    RecoveryReportView, RecoverySalvageView, ResolutionOutcomeView, ResolutionResultView, Response,
    SchemaCommand, SchemaDefinitionDraft, SchemaFamily, SchemaLifecycle, SchemaResponse,
    SecurityPolicyCommand, SecurityPolicyResponse, SecurityPolicySnapshotView, StreamPlan,
};
#[cfg(feature = "sidecar")]
use worlddb_ode_engine::{MAX_STREAM_BYTES, MAX_STREAM_CHUNK_BYTES, fill_deterministic_chunk};
mod backup;
mod diagnostics;
mod export_import;
mod host_session;
mod migration;
mod purge;
mod transfer;
use host_session::{
    HostCapability, HostIdentity, HostSessionManager, HostSessionTicket, SessionError,
};
use transfer::{
    BeginTransferRequestV1, BeginTransferResponseV1, ChunkAcknowledgementV1,
    FinishTransferRequestV1, IPC_PROTOCOL_VERSION, MAX_TRANSFER_CHUNK_BYTES, TransferCompletionV1,
    TransferError, TransferManager,
};
#[cfg(feature = "in-process")]
use worlddb_core::CancellationRequestDisposition;
use worlddb_core::{JobId, OperationId, PrincipalId};
use worlddb_ode_engine::ProjectError;
use worlddb_storage_file::CurrentPointerFormat;

#[cfg(feature = "sidecar")]
const FRAME_DATA: u8 = 1;
#[cfg(feature = "sidecar")]
const FRAME_END: u8 = 2;
#[cfg(feature = "sidecar")]
const FRAME_CANCEL: u8 = 3;
const STREAM_TEST_BYTES: u64 = 100 * 1024 * 1024;
const STREAM_TEST_CHUNK_BYTES: u32 = 256 * 1024;
const STREAM_CANCEL_AFTER_BYTES: u64 = 8 * 1024 * 1024;
const STREAM_MEASUREMENT_RUNS: usize = 5;
static FACTS_SMOKE_RESULT_LOCK: Mutex<()> = Mutex::new(());
#[cfg(feature = "sidecar")]
const SIDECAR_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    if let Err(error) = run() {
        eprintln!("WorldDB ODE-002 startup failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let database_root = cfg!(debug_assertions)
        .then(|| std::env::var_os("WORLDDB_ODE_DATABASE").map(PathBuf::from))
        .flatten();

    let host_sessions = HostIdentity::current()
        .map(HostSessionManager::new)
        .map_err(|_| "operating system account identity unavailable".to_owned())?;

    let backend = Backend::new(database_root.as_deref())?;

    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(all(feature = "native-e2e", debug_assertions))]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .manage(backend)
        .manage(host_sessions)
        .manage(TransferManager::new())
        .setup(|app| {
            if app.get_webview_window("primary").is_none()
                || app.get_webview_window("secondary").is_none()
            {
                return Err("two native windows were not created".into());
            }
            #[cfg(debug_assertions)]
            if env_enabled("WORLDDB_ODE_SHOW_WINDOWS") {
                for label in ["primary", "secondary"] {
                    app.get_webview_window(label)
                        .ok_or_else(|| format!("missing native window {label}"))?
                        .show()?;
                }
            }
            let state = app.state::<Backend>();
            let engine_at_start = state
                .health()
                .map_err(std::io::Error::other)?
                .map(|health| serde_json::to_value(health).map_err(std::io::Error::other))
                .transpose()?;
            let mut report = serde_json::json!({
                "mode": if cfg!(feature = "sidecar") { "sidecar" } else { "in_process" },
                "application_process_id": std::process::id(),
                "application_build_id": env!("WORLDDB_ODE_APP_BUILD_ID"),
                "windows": ["primary", "secondary"],
                "engine": engine_at_start,
                "engine_at_start": engine_at_start,
            });
            if env_enabled("WORLDDB_ODE_STREAM_TEST") {
                let full_plan = StreamPlan {
                    total_bytes: STREAM_TEST_BYTES,
                    chunk_bytes: STREAM_TEST_CHUNK_BYTES,
                    cancel_after_bytes: None,
                };
                let cancel_plan = StreamPlan {
                    total_bytes: STREAM_TEST_BYTES,
                    chunk_bytes: STREAM_TEST_CHUNK_BYTES,
                    cancel_after_bytes: Some(STREAM_CANCEL_AFTER_BYTES),
                };
                let mut full_runs = Vec::with_capacity(STREAM_MEASUREMENT_RUNS);
                let mut cancel_runs = Vec::with_capacity(STREAM_MEASUREMENT_RUNS);
                for _ in 0..STREAM_MEASUREMENT_RUNS {
                    full_runs.push(state.stream(full_plan).map_err(std::io::Error::other)?);
                    cancel_runs.push(state.stream(cancel_plan).map_err(std::io::Error::other)?);
                }
                report["stream_full"] = full_runs[0].clone();
                report["stream_cancelled"] = cancel_runs[0].clone();
                report["stream_full_runs"] = serde_json::Value::Array(full_runs);
                report["stream_cancelled_runs"] = serde_json::Value::Array(cancel_runs);
            }

            let mut exit_after_report = false;
            if env_enabled("WORLDDB_ODE_PANIC_TEST") {
                let panic_result = state
                    .exercise_engine_panic()
                    .map_err(std::io::Error::other)?;
                exit_after_report = panic_result["application_restart_required"] == true;
                report["engine_panic"] = panic_result;
            }
            if env_enabled("WORLDDB_ODE_UPDATE_TEST") {
                report["engine_update"] = state
                    .exercise_engine_update()
                    .map_err(std::io::Error::other)?;
                exit_after_report |=
                    report["engine_update"]["application_restart_required"] == true;
            }
            report["engine_after_tests"] = match state.health() {
                Ok(health) => serde_json::to_value(health).map_err(std::io::Error::other)?,
                Err(error) => serde_json::json!({ "health_error": error }),
            };

            if let Some(result_path) = std::env::var_os("WORLDDB_ODE_RESULT") {
                use std::io::Write;
                let mut file = std::fs::File::create(result_path)?;
                serde_json::to_writer(&mut file, &report)?;
                file.write_all(b"\n")?;
                file.flush()?;
            }
            if exit_after_report {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    handle.exit(0);
                });
            }
            if let Ok(hold_ms) = std::env::var("WORLDDB_ODE_AUTOCLOSE_MS") {
                let hold_ms = hold_ms.parse::<u64>().unwrap_or(0);
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(hold_ms));
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_host_session,
            security_smoke_mode,
            health,
            begin_transfer,
            push_transfer_chunk,
            finish_transfer,
            project_dialog_mode,
            project_status,
            list_jobs,
            cancel_job,
            inspect_recovery,
            run_journaled_recovery,
            salvage_recovery,
            select_migration_plan,
            migration_status,
            preview_migration,
            resolve_migration_items,
            run_migration,
            resume_migration,
            cancel_migration,
            create_backup,
            verify_backup,
            restore_backup,
            export_data,
            create_import_plan,
            prepare_import,
            export_diagnostics,
            preview_purge,
            execute_purge,
            discard_purge_plan,
            create_project,
            open_project,
            close_project,
            manage_schema,
            manage_entities,
            manage_branch_layers,
            manage_history_space_transfer,
            manage_facts,
            facts_smoke_diagnostic,
            focus_native_window,
            diagnostic_smoke_canary,
            manage_perspectives,
            manage_security_policy
        ])
        .run(tauri::generate_context!())
        .map_err(|error| error.to_string())
}

fn env_enabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
}

fn injected_unknown_commit_response(operation_id: Option<OperationId>) -> Option<OperationId> {
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return None;
    }
    let requested = std::env::var("WORLDDB_ODE_UNKNOWN_COMMIT_OPERATION_ID").ok()?;
    operation_id.filter(|operation_id| operation_id.to_string() == requested)
}

#[tauri::command]
fn open_host_session(
    window: tauri::WebviewWindow,
    sessions: tauri::State<'_, HostSessionManager>,
) -> Result<HostSessionTicket, IpcErrorV1> {
    sessions
        .issue(window.label())
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HealthRequestV1 {
    protocol_version: u16,
    session_id: String,
}

#[derive(Serialize)]
struct HealthResponseV1 {
    protocol_version: u16,
    engine: Option<Response>,
    project: ProjectStatusV1,
}

#[derive(Serialize)]
struct IpcErrorV1 {
    protocol_version: u16,
    code: &'static str,
    message_key: String,
    next_action_key: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    database_id: Option<String>,
}

#[derive(Serialize)]
struct SecuritySmokeModeV1 {
    protocol_version: u16,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticExportRequestV1 {
    protocol_version: u16,
}

#[tauri::command]
fn diagnostic_smoke_canary(
    window: tauri::WebviewWindow,
    session_id: String,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<IpcErrorV1, IpcErrorV1> {
    if !cfg!(debug_assertions)
        || project_smoke_root().is_none()
        || std::env::var_os("WORLDDB_ODE_FACTS_SMOKE_RESULT").is_none()
    {
        return Err(IpcErrorV1::new("unauthorized"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    Ok(backend.record_diagnostic_error(
        "migration_rejected",
        "WDB_INTERNAL_CAUSE_CANARY_93D1".to_owned(),
    ))
}

#[derive(Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ProjectStatusV1 {
    protocol_version: u16,
    window: String,
    project_open: bool,
    project_name: Option<String>,
    database_id: Option<String>,
    revision: Option<u64>,
    role: Option<String>,
    snapshot_id: Option<String>,
    compatibility: Option<ProjectCompatibilityV1>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ProjectStorageFormatV1 {
    CurrentV1,
    CurrentV2,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ProjectMigrationPolicyV1 {
    ExplicitOnly,
}

#[derive(Serialize, Clone, Copy)]
struct ProjectCompatibilityV1 {
    storage_format: ProjectStorageFormatV1,
    format_upgrade_policy: ProjectMigrationPolicyV1,
    schema_migration_policy: ProjectMigrationPolicyV1,
    migration_applied_during_open: bool,
}

impl ProjectCompatibilityV1 {
    const fn from_pointer_format(format: CurrentPointerFormat) -> Self {
        let storage_format = match format {
            CurrentPointerFormat::V1 => ProjectStorageFormatV1::CurrentV1,
            CurrentPointerFormat::V2 => ProjectStorageFormatV1::CurrentV2,
        };
        Self {
            storage_format,
            format_upgrade_policy: ProjectMigrationPolicyV1::ExplicitOnly,
            schema_migration_policy: ProjectMigrationPolicyV1::ExplicitOnly,
            migration_applied_during_open: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProjectRequestV1 {
    protocol_version: u16,
    project_name: String,
    #[serde(default)]
    operation_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenProjectRequestV1 {
    protocol_version: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseProjectRequestV1 {
    protocol_version: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobsRequestV1 {
    protocol_version: u16,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryRequestV1 {
    protocol_version: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryApplyRequestV1 {
    protocol_version: u16,
    confirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverySalvageRequestV1 {
    protocol_version: u16,
    archive_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationRequestV1 {
    protocol_version: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationRunRequestV1 {
    protocol_version: u16,
    confirmed_breaking: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationResolutionRequestV1 {
    protocol_version: u16,
    omitted_record_indexes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelJobRequestV1 {
    protocol_version: u16,
    session_id: String,
    job_id: String,
}

#[derive(Serialize)]
struct JobListResponseV1 {
    protocol_version: u16,
    result: JobListView,
    #[serde(skip_serializing_if = "Option::is_none")]
    cancel_disposition: Option<String>,
}

#[derive(Serialize)]
struct RecoveryReportResponseV1 {
    protocol_version: u16,
    result: RecoveryReportView,
}

#[derive(Serialize)]
struct RecoveryApplyResponseV1 {
    protocol_version: u16,
    result: RecoveryApplyView,
}

#[derive(Serialize)]
struct RecoverySalvageResponseV1 {
    protocol_version: u16,
    result: RecoverySalvageView,
}

#[derive(Serialize)]
struct MigrationPlanResponseV1 {
    protocol_version: u16,
    plan: migration::MigrationPlanView,
}

#[derive(Serialize)]
struct MigrationPanelResponseV1 {
    protocol_version: u16,
    state: Option<migration::MigrationPanelState>,
}

#[derive(Serialize)]
struct MigrationPreviewResponseV1 {
    protocol_version: u16,
    result: migration::MigrationDryRunView,
}

#[derive(Serialize)]
struct MigrationRunResponseV1 {
    protocol_version: u16,
    result: migration::MigrationRunView,
}

#[derive(Serialize)]
struct BackupOperationResponseV1 {
    protocol_version: u16,
    result: backup::BackupOperationView,
}

#[derive(Serialize)]
struct CloseProjectResponseV1 {
    protocol_version: u16,
    project: ProjectStatusV1,
    closed: bool,
    shutdown: Option<JobShutdownView>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: SchemaCommand,
}

#[derive(Serialize)]
struct SchemaResponseV1 {
    protocol_version: u16,
    result: SchemaResponse,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: EntityCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchLayerRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: BranchLayerCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistorySpaceTransferRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: HistorySpaceTransferCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: FactCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PerspectiveRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: PerspectiveCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SecurityPolicyRequestV1 {
    protocol_version: u16,
    session_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    command: SecurityPolicyCommand,
}

#[derive(Serialize)]
struct BranchLayerResponseV1 {
    protocol_version: u16,
    result: BranchLayerResponse,
}

#[derive(Serialize)]
struct HistorySpaceTransferResponseV1 {
    protocol_version: u16,
    result: HistorySpaceTransferResponse,
}

#[derive(Serialize)]
struct FactResponseV1 {
    protocol_version: u16,
    result: FactResponse,
}

#[derive(Serialize)]
struct EntityResponseV1 {
    protocol_version: u16,
    result: EntityResponse,
}

#[derive(Serialize)]
struct PerspectiveResponseV1 {
    protocol_version: u16,
    result: PerspectiveResponse,
}

#[derive(Serialize)]
struct SecurityPolicyResponseV1 {
    protocol_version: u16,
    result: SecurityPolicyResponse,
}

#[derive(Serialize)]
struct ProjectDialogModeV1 {
    protocol_version: u16,
    enabled: bool,
    startup_smoke_enabled: bool,
}

impl IpcErrorV1 {
    fn new(code: &'static str) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            code,
            message_key: diagnostics::localization_key(code),
            next_action_key: diagnostics::next_action_key(code),
            operation_id: None,
            database_id: None,
        }
    }

    fn unknown_commit(operation_id: OperationId, database_id: Option<String>) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            code: "unknown_commit_outcome",
            message_key: diagnostics::localization_key("unknown_commit_outcome"),
            next_action_key: diagnostics::next_action_key("unknown_commit_outcome"),
            operation_id: Some(operation_id.to_string()),
            database_id,
        }
    }
}

impl std::fmt::Debug for IpcErrorV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IpcErrorV1")
            .field("protocol_version", &self.protocol_version)
            .field("code", &self.code)
            .field("message_key", &self.message_key)
            .field("next_action_key", &self.next_action_key)
            .field("operation_id", &self.operation_id)
            .field("database_id", &self.database_id)
            .finish()
    }
}

fn parse_client_operation_id(value: Option<&str>) -> Result<Option<OperationId>, IpcErrorV1> {
    value
        .map(OperationId::from_str)
        .transpose()
        .map_err(|_| IpcErrorV1::new("invalid_operation_id"))
}

#[cfg(feature = "in-process")]
fn cancel_disposition_code(disposition: CancellationRequestDisposition) -> &'static str {
    match disposition {
        CancellationRequestDisposition::Signalled => "signalled",
        CancellationRequestDisposition::AlreadySignalled => "already_signalled",
        CancellationRequestDisposition::TooLate => "too_late",
        CancellationRequestDisposition::AlreadyTerminal => "already_terminal",
        CancellationRequestDisposition::OutcomeUnknown => "outcome_unknown",
    }
}

#[tauri::command]
fn security_smoke_mode() -> SecuritySmokeModeV1 {
    SecuritySmokeModeV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        enabled: cfg!(debug_assertions) && std::env::var_os("WORLDDB_ODE_IPC_RESULT").is_some(),
    }
}

#[tauri::command]
fn health(
    window: tauri::WebviewWindow,
    request: HealthRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    state: tauri::State<'_, Backend>,
) -> Result<HealthResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::HealthRead,
        )
        .map_err(map_session_error)?;
    let engine = state
        .health()
        .map_err(|_| IpcErrorV1::new("engine_unavailable"))?;
    let project = state
        .project_status(window.label())
        .map_err(map_project_error)?;
    record_ipc_probe(window.label())?;
    Ok(HealthResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        engine,
        project,
    })
}

#[tauri::command]
fn project_dialog_mode() -> ProjectDialogModeV1 {
    ProjectDialogModeV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        enabled: cfg!(debug_assertions)
            && std::env::var_os("WORLDDB_ODE_PROJECT_SMOKE_ROOT").is_some(),
        startup_smoke_enabled: cfg!(debug_assertions)
            && std::env::var_os("WORLDDB_ODE_PROJECT_SMOKE_ROOT").is_some()
            && !env_enabled("WORLDDB_ODE_PROJECT_SMOKE_SKIP_AUTORUN"),
    }
}

#[tauri::command]
fn project_status(
    window: tauri::WebviewWindow,
    session_id: String,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<ProjectStatusV1, IpcErrorV1> {
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .project_status(window.label())
        .map_err(map_project_error)
}

#[tauri::command]
fn list_jobs(
    window: tauri::WebviewWindow,
    request: JobsRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<JobListResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let result = backend
        .list_jobs()
        .map_err(|_| IpcErrorV1::new("job_state_unavailable"))?;
    Ok(JobListResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
        cancel_disposition: None,
    })
}

#[tauri::command]
fn cancel_job(
    window: tauri::WebviewWindow,
    request: CancelJobRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<JobListResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let job_id = JobId::from_str(&request.job_id).map_err(|_| IpcErrorV1::new("invalid_job_id"))?;
    let disposition = backend
        .cancel_job(job_id)
        .map_err(|_| IpcErrorV1::new("job_cancel_unavailable"))?;
    let result = backend
        .list_jobs()
        .map_err(|_| IpcErrorV1::new("job_state_unavailable"))?;
    Ok(JobListResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
        cancel_disposition: Some(disposition),
    })
}

#[tauri::command]
async fn inspect_recovery(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: RecoveryRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<RecoveryReportResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let root = if let Some(test_root) = project_smoke_root() {
        test_root
    } else {
        pick_project_folder(app, window, "WorldDB-Projekt read-only prüfen").await?
    };
    let result = backend
        .inspect_recovery(&root)
        .map_err(|_| IpcErrorV1::new("recovery_inspection_unavailable"))?;
    Ok(RecoveryReportResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
fn run_journaled_recovery(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: RecoveryApplyRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<RecoveryApplyResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    if !request.confirmed {
        return Err(IpcErrorV1::new("explicit_confirmation_required"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let result = backend
        .run_journaled_recovery()
        .map_err(|_| IpcErrorV1::new("journaled_recovery_rejected"))?;
    app.emit("recovery-state-changed", ())
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    Ok(RecoveryApplyResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn salvage_recovery(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: RecoverySalvageRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<RecoverySalvageResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let archive_name = validate_project_name(&request.archive_name)?;
    let parent = pick_project_parent(app, window, "Zielordner für das Salvage-Archiv").await?;
    let destination = parent.join(format!("{archive_name}.salvage"));
    let result = backend
        .salvage_recovery(&destination)
        .map_err(|_| IpcErrorV1::new("salvage_rejected"))?;
    Ok(RecoverySalvageResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn select_migration_plan(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: MigrationRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<MigrationPlanResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .begin_migration_plan_selection()
        .map_err(|_| IpcErrorV1::new("migration_unavailable"))?;
    let selection = async {
        let source = pick_project_folder(
            app.clone(),
            window.clone(),
            "Quellprojekt für die Migration",
        )
        .await?;
        let plan = pick_migration_plan_file(app.clone(), window).await?;
        Ok::<_, IpcErrorV1>((source, plan))
    }
    .await;
    let plan = match selection {
        Ok((source, plan_path)) => backend.finish_migration_plan_selection(&source, &plan_path),
        Err(error) => {
            backend.cancel_migration_dialog();
            return Err(error);
        }
    }
    .map_err(|detail| backend.record_diagnostic_error("migration_rejected", detail))?;
    let _ = app.emit("migration-state-changed", ());
    Ok(MigrationPlanResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        plan,
    })
}

#[tauri::command]
fn migration_status(
    window: tauri::WebviewWindow,
    session_id: String,
    request: MigrationRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<MigrationPanelResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    Ok(MigrationPanelResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        state: backend
            .migration_state()
            .map_err(|_| IpcErrorV1::new("host_unavailable"))?,
    })
}

#[tauri::command]
async fn preview_migration(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: MigrationRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<MigrationPreviewResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let steps = backend
        .begin_migration_preview()
        .map_err(|_| IpcErrorV1::new("migration_unavailable"))?;
    let selection = async {
        let mut selected_files = Vec::new();
        selected_files
            .try_reserve_exact(steps.len())
            .map_err(|_| IpcErrorV1::new("migration_unavailable"))?;
        for step_id in steps {
            let title = format!("Quelldatensätze für Migrationsschritt {step_id}");
            selected_files
                .push(pick_migration_record_files(app.clone(), window.clone(), title).await?);
        }
        Ok::<_, IpcErrorV1>(selected_files)
    }
    .await;
    let result = match selection {
        Ok(selected_files) => backend.finish_migration_preview(selected_files),
        Err(error) => {
            backend.cancel_migration_dialog();
            return Err(error);
        }
    }
    .map_err(|detail| backend.record_diagnostic_error("migration_rejected", detail))?;
    let _ = app.emit("migration-state-changed", ());
    Ok(MigrationPreviewResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
fn resolve_migration_items(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: MigrationResolutionRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<MigrationPanelResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let mut indexes = Vec::new();
    indexes
        .try_reserve_exact(request.omitted_record_indexes.len())
        .map_err(|_| IpcErrorV1::new("invalid_request"))?;
    for value in &request.omitted_record_indexes {
        let index = value
            .parse::<u64>()
            .map_err(|_| IpcErrorV1::new("invalid_request"))?;
        if index.to_string() != *value {
            return Err(IpcErrorV1::new("invalid_request"));
        }
        indexes.push(index);
    }
    let state = backend
        .set_migration_omissions(indexes)
        .map_err(|detail| backend.record_diagnostic_error("migration_rejected", detail))?;
    let _ = app.emit("migration-state-changed", ());
    Ok(MigrationPanelResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        state: Some(state),
    })
}

#[tauri::command]
async fn run_migration(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: MigrationRunRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<MigrationRunResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let breaking = backend
        .begin_migration_execution(false)
        .map_err(|_| IpcErrorV1::new("migration_unavailable"))?;
    if breaking != request.confirmed_breaking {
        backend.cancel_migration_dialog();
        return Err(IpcErrorV1::new("explicit_confirmation_required"));
    }
    let destinations = async {
        if breaking {
            let backup_parent = pick_project_parent(
                app.clone(),
                window.clone(),
                "Elternordner für die exakte Migrationssicherung",
            )
            .await?;
            let restore_parent = pick_project_parent(
                app.clone(),
                window,
                "Elternordner für den geprüften Restore-Klon",
            )
            .await?;
            Ok::<_, IpcErrorV1>((Some(backup_parent), Some(restore_parent)))
        } else {
            Ok((None, None))
        }
    }
    .await;
    let (backup_parent, restore_parent) = match destinations {
        Ok(destinations) => destinations,
        Err(error) => {
            backend.cancel_migration_dialog();
            return Err(error);
        }
    };
    let completion = backend.finish_migration_execution(
        request.confirmed_breaking,
        backup_parent,
        restore_parent,
    );
    let _ = app.emit("migration-state-changed", ());
    let result = completion
        .map_err(|detail| backend.record_diagnostic_error("migration_rejected", detail))?;
    Ok(MigrationRunResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn resume_migration(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: MigrationRunRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<MigrationRunResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let breaking = backend
        .begin_migration_execution(true)
        .map_err(|_| IpcErrorV1::new("migration_unavailable"))?;
    if breaking != request.confirmed_breaking {
        backend.cancel_migration_dialog();
        return Err(IpcErrorV1::new("explicit_confirmation_required"));
    }
    let restore_parent = if breaking {
        match pick_project_parent(
            app.clone(),
            window,
            "Elternordner für den neuen Restore-Klon",
        )
        .await
        {
            Ok(parent) => Some(parent),
            Err(error) => {
                backend.cancel_migration_dialog();
                return Err(error);
            }
        }
    } else {
        None
    };
    let completion = backend.finish_migration_resume(request.confirmed_breaking, restore_parent);
    let _ = app.emit("migration-state-changed", ());
    let result = completion
        .map_err(|detail| backend.record_diagnostic_error("migration_rejected", detail))?;
    Ok(MigrationRunResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
fn cancel_migration(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: MigrationRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<(), IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .cancel_migration()
        .map_err(|_| IpcErrorV1::new("migration_unavailable"))?;
    let _ = app.emit("migration-state-changed", ());
    Ok(())
}

#[tauri::command]
async fn create_backup(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: backup::BackupRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<BackupOperationResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let profile = backup::BackupProfile::parse(&request.profile)
        .map_err(|_| IpcErrorV1::new("invalid_request"))?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("backup_unavailable"))?;
    let outcome = async {
        let source = pick_project_folder(
            app.clone(),
            window.clone(),
            "Quellprojekt für die Sicherung auswählen",
        )
        .await?;
        let parent = pick_project_parent(
            app.clone(),
            window.clone(),
            "Elternordner für die neue Sicherung auswählen",
        )
        .await?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            backup::create_backup(&source, &parent, profile)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("backup_rejected", detail))?;
        Ok::<_, IpcErrorV1>(result)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("backup-state-changed", ());
    Ok(BackupOperationResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn verify_backup(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: backup::BackupRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<BackupOperationResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let profile = backup::BackupProfile::parse(&request.profile)
        .map_err(|_| IpcErrorV1::new("invalid_request"))?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("backup_unavailable"))?;
    let outcome = async {
        let backup_path = pick_project_folder(
            app.clone(),
            window.clone(),
            "Zu prüfendes WorldDB-Backup auswählen",
        )
        .await?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            backup::verify_backup(&backup_path, profile)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("backup_rejected", detail))?;
        Ok::<_, IpcErrorV1>(result)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("backup-state-changed", ());
    Ok(BackupOperationResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn restore_backup(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: backup::BackupRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<BackupOperationResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let profile = backup::BackupProfile::parse(&request.profile)
        .map_err(|_| IpcErrorV1::new("invalid_request"))?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("backup_unavailable"))?;
    let outcome = async {
        let authorization = pick_project_folder(
            app.clone(),
            window.clone(),
            "Sauberes Autorisierungsprojekt für dieses Backup auswählen",
        )
        .await?;
        let backup_path = pick_project_folder(
            app.clone(),
            window.clone(),
            "Wiederherzustellendes, zuvor geprüftes Backup auswählen",
        )
        .await?;
        let parent = pick_project_parent(
            app.clone(),
            window.clone(),
            "Elternordner für den neuen Restore-Klon auswählen",
        )
        .await?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            backup::restore_clone(&backup_path, &authorization, &parent, profile)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("backup_rejected", detail))?;
        Ok::<_, IpcErrorV1>(result)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("backup-state-changed", ());
    Ok(BackupOperationResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn export_data(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: export_import::ExportRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<export_import::ExportResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    export_import::validate_export_request(&request)
        .map_err(|_| IpcErrorV1::new("export_import_rejected"))?;
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("export_import_unavailable"))?;
    let outcome = async {
        let source = pick_project_folder(
            app.clone(),
            window.clone(),
            "Quellprojekt für den Export auswählen",
        )
        .await?;
        let (filter, extensions, default_name) = if request.kind == "sharing" {
            (
                "WorldDB Sharing Export",
                &(["wdbshare"][..]),
                "WorldDB-Sharing.wdbshare",
            )
        } else {
            (
                "WorldDB Logical Export",
                &(["wdblex"][..]),
                "WorldDB-Logical.wdblex",
            )
        };
        let output = pick_output_file(
            app.clone(),
            window.clone(),
            "Neues Exportartefakt speichern",
            filter,
            extensions,
            default_name,
        )
        .await?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            export_import::export(&source, &output, request)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("export_import_rejected", detail))?;
        Ok::<_, IpcErrorV1>(result)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("export-import-state-changed", ());
    Ok(export_import::ExportResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn create_import_plan(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: export_import::ImportPlanRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<export_import::ImportPlanResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    export_import::validate_plan_request(&request)
        .map_err(|_| IpcErrorV1::new("export_import_rejected"))?;
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("export_import_unavailable"))?;
    let outcome = async {
        let destination = pick_project_folder(
            app.clone(),
            window.clone(),
            "Zielprojekt für den Importplan auswählen",
        )
        .await?;
        let input = pick_worlddb_file(
            app.clone(),
            window.clone(),
            "Logisches Exportartefakt für den Importplan auswählen",
            "WorldDB Logical Export",
            &["wdblex", "bin"],
        )
        .await?;
        let output = pick_output_file(
            app.clone(),
            window.clone(),
            "Neuen kanonischen Importplan speichern",
            "WorldDB Import Plan",
            &["wdbplan"],
            "WorldDB-Import.wdbplan",
        )
        .await?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            export_import::create_import_plan(&destination, &input, &output, request)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("export_import_rejected", detail))?;
        Ok::<_, IpcErrorV1>(result)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("export-import-state-changed", ());
    Ok(export_import::ImportPlanResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn prepare_import(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: export_import::ImportPrepareRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<export_import::ImportPrepareResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("export_import_unavailable"))?;
    let outcome = async {
        let destination = pick_project_folder(
            app.clone(),
            window.clone(),
            "Zielprojekt für die Importvorbereitung auswählen",
        )
        .await?;
        let input = pick_worlddb_file(
            app.clone(),
            window.clone(),
            "Logisches Exportartefakt zur Vorbereitung auswählen",
            "WorldDB Logical Export",
            &["wdblex", "bin"],
        )
        .await?;
        let plan = pick_worlddb_file(
            app.clone(),
            window.clone(),
            "Zugehörigen kanonischen Importplan auswählen",
            "WorldDB Import Plan",
            &["wdbplan"],
        )
        .await?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            export_import::prepare_import(&destination, &input, &plan, request)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("export_import_rejected", detail))?;
        Ok::<_, IpcErrorV1>(result)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("export-import-state-changed", ());
    Ok(export_import::ImportPrepareResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn export_diagnostics(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: serde_json::Value,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<diagnostics::DiagnosticExportViewV1, IpcErrorV1> {
    record_facts_smoke_diagnostic(
        window.label(),
        "facts-smoke:diagnostic-export:entered".to_owned(),
    )?;
    // Decode the raw request into a strict DTO before authorization, native
    // dialogs, or filesystem access can begin.
    let request: DiagnosticExportRequestV1 = match serde_json::from_value(request) {
        Ok(request) => request,
        Err(_) => {
            record_facts_smoke_diagnostic(
                window.label(),
                "facts-smoke:diagnostic-export:renderer-request-rejected".to_owned(),
            )?;
            return Err(IpcErrorV1::new("invalid_request"));
        }
    };
    record_facts_smoke_diagnostic(
        window.label(),
        "facts-smoke:diagnostic-export:request-accepted".to_owned(),
    )?;
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let (database_id, permit) = backend
        .diagnostic_export_context()
        .map_err(|_| IpcErrorV1::new("unauthorized"))?;
    if permit.is_none() {
        return Err(IpcErrorV1::new("unauthorized"));
    }
    let path = pick_output_file(
        app,
        window.clone(),
        "Technischen WorldDB-Diagnoseexport speichern",
        "WorldDB Diagnosedaten",
        &["json"],
        "WorldDB-Diagnose.json",
    )
    .await?;
    let (current_database_id, current_permit) = backend
        .diagnostic_export_context()
        .map_err(|_| IpcErrorV1::new("unauthorized"))?;
    if current_database_id != database_id || current_permit.is_none() {
        return Err(IpcErrorV1::new("unauthorized"));
    }
    let bundle = backend
        .diagnostic_bundle(
            &database_id,
            current_permit
                .as_ref()
                .expect("authorization permit checked immediately above"),
        )
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let result =
        tauri::async_runtime::spawn_blocking(move || diagnostics::write_export(&path, &bundle))
            .await
            .map_err(|_| IpcErrorV1::new("host_unavailable"))?
            .map_err(|error| {
                IpcErrorV1::new(match error {
                    diagnostics::DiagnosticWriteError::InvalidTarget
                    | diagnostics::DiagnosticWriteError::TargetExists
                    | diagnostics::DiagnosticWriteError::TooLarge => "diagnostic_export_rejected",
                    diagnostics::DiagnosticWriteError::Encode
                    | diagnostics::DiagnosticWriteError::Io => "diagnostic_export_unavailable",
                })
            })?;
    Ok(result)
}

#[tauri::command]
async fn preview_purge(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: purge::PurgeRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<purge::PurgePlanResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    purge::validate_request(&request).map_err(|_| IpcErrorV1::new("purge_rejected"))?;
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .begin_backup_operation()
        .map_err(|_| IpcErrorV1::new("purge_unavailable"))?;
    let clear_result = backend.clear_purge_draft();
    let outcome = async {
        clear_result.map_err(|_| IpcErrorV1::new("host_unavailable"))?;
        let source = pick_project_folder(
            app.clone(),
            window.clone(),
            "Quellprojekt für den Purgeplan auswählen",
        )
        .await?;
        let destination_parent = pick_project_parent(
            app.clone(),
            window.clone(),
            "Elternordner für die neue Purge-Datenbank auswählen",
        )
        .await?;
        let report = pick_output_file(
            app.clone(),
            window.clone(),
            "Neuen Purge-Planbericht speichern",
            "WorldDB Purge Plan Report",
            &["json"],
            "WorldDB-Purge-Plan.json",
        )
        .await?;
        let (view, draft) = tauri::async_runtime::spawn_blocking(move || {
            purge::plan(&source, &destination_parent, &report, request)
        })
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?
        .map_err(|detail| backend.record_diagnostic_error("purge_rejected", detail))?;
        backend
            .stage_purge_draft(draft)
            .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
        Ok::<_, IpcErrorV1>(view)
    }
    .await;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("purge-state-changed", ());
    Ok(purge::PurgePlanResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
async fn execute_purge(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: purge::PurgeRunRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<purge::PurgeRunResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    purge::validate_run_request(&request).map_err(|_| IpcErrorV1::new("purge_rejected"))?;
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let draft = backend
        .begin_purge_execution(&request.plan_fingerprint)
        .map_err(|_| IpcErrorV1::new("purge_unavailable"))?;
    let fingerprint = request.plan_fingerprint.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || purge::run(draft, &fingerprint))
        .await
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
        .and_then(|result| {
            result.map_err(|detail| backend.record_diagnostic_error("purge_rejected", detail))
        });
    backend
        .clear_purge_draft()
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    backend.cancel_migration_dialog();
    let result = outcome?;
    let _ = app.emit("purge-state-changed", ());
    Ok(purge::PurgeRunResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

#[tauri::command]
fn discard_purge_plan(
    window: tauri::WebviewWindow,
    session_id: String,
    request: purge::PurgeDiscardRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<purge::PurgeDiscardResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    backend
        .discard_purge_draft()
        .map_err(|detail| backend.record_diagnostic_error("purge_rejected", detail))?;
    Ok(purge::PurgeDiscardResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        discarded: true,
    })
}

#[tauri::command]
async fn create_project(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: CreateProjectRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<ProjectStatusV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?
        .ok_or_else(|| IpcErrorV1::new("invalid_operation_id"))?;
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectCreate)
        .map_err(map_session_error)?;
    let principal_id = sessions.project_principal().map_err(map_session_error)?;
    let name = validate_project_name(&request.project_name)?;
    let root = if let Some(test_root) = project_smoke_root() {
        test_root
    } else {
        let parent =
            pick_project_parent(app.clone(), window.clone(), "Neues WorldDB-Projekt").await?;
        parent.join(format!("{name}.worlddb"))
    };
    let status = backend
        .create_project(window.label(), &root, principal_id, operation_id)
        .map_err(map_project_error)?;
    let engine = backend
        .health()
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    app.emit("project-state-changed", ())
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    record_project_smoke(window.label(), &status, engine.as_ref())?;
    Ok(status)
}

#[tauri::command]
async fn open_project(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: OpenProjectRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<ProjectStatusV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectOpen)
        .map_err(map_session_error)?;
    let principal_id = sessions.project_principal().map_err(map_session_error)?;
    let root = if let Some(test_root) = project_smoke_root() {
        test_root
    } else {
        pick_project_folder(app.clone(), window.clone(), "WorldDB-Projekt öffnen").await?
    };
    let status = backend
        .open_project(window.label(), &root, principal_id)
        .map_err(map_project_error)?;
    let engine = backend
        .health()
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    app.emit("project-state-changed", ())
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    record_project_smoke(window.label(), &status, engine.as_ref())?;
    Ok(status)
}

#[tauri::command]
fn close_project(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    session_id: String,
    request: CloseProjectRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<CloseProjectResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(window.label(), &session_id, HostCapability::ProjectClose)
        .map_err(map_session_error)?;
    let shutdown = backend.close_project().map_err(map_project_error)?;
    let project = backend
        .project_status(window.label())
        .map_err(map_project_error)?;
    let closed = !project.project_open;
    if closed {
        app.emit("project-state-changed", ())
            .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    }
    Ok(CloseProjectResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        project,
        closed,
        shutdown,
    })
}

#[tauri::command]
async fn manage_schema(
    window: tauri::WebviewWindow,
    request: SchemaRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<SchemaResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    let trace_facts_smoke_schema = window.label() == "primary"
        && std::env::var_os("WORLDDB_ODE_FACTS_SMOKE_RESULT").is_some();
    if trace_facts_smoke_schema {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:schema-ipc:entered".to_owned(),
        )?;
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    if trace_facts_smoke_schema {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:schema-ipc:authorized".to_owned(),
        )?;
    }
    let compact_create_response =
        matches!(&request.command, &SchemaCommand::Create { .. });
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let operation = schema_smoke_operation(&request.command);
    let facts_smoke_predicate = matches!(
        &request.command,
        SchemaCommand::Create {
            definition: SchemaDefinitionDraft::Predicate { symbol, .. },
            ..
        } if symbol == "ipc_smoke_facts"
    );
    let facts_smoke_entity_type_deprecation = matches!(
        &request.command,
        SchemaCommand::SetLifecycle {
            family: SchemaFamily::EntityType,
            lifecycle: SchemaLifecycle::Deprecated,
            ..
        }
    );
    if facts_smoke_predicate {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:predicate-schema-command:entered".to_owned(),
        )?;
    }
    if facts_smoke_entity_type_deprecation {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:entity-type-deprecation:entered".to_owned(),
        )?;
    }
    if trace_facts_smoke_schema {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:schema-ipc:dispatching".to_owned(),
        )?;
    }
    let result = backend.schema_with_operation_id(request.command, operation_id);
    if trace_facts_smoke_schema {
        record_facts_smoke_diagnostic(
            window.label(),
            format!(
                "facts-smoke:schema-ipc:engine-returned:{}",
                if result.is_ok() { "ok" } else { "rejected" }
            ),
        )?;
    }
    if facts_smoke_predicate {
        record_facts_smoke_diagnostic(
            window.label(),
            format!(
                "facts-smoke:predicate-schema-command:{}",
                if result.is_ok() {
                    "returned"
                } else {
                    "rejected"
                }
            ),
        )?;
    }
    if facts_smoke_entity_type_deprecation {
        record_facts_smoke_diagnostic(
            window.label(),
            format!(
                "facts-smoke:entity-type-deprecation:{}",
                if result.is_ok() {
                    "returned"
                } else {
                    "rejected"
                }
            ),
        )?;
    }
    match result {
        Ok(mut result) => {
            record_schema_smoke(window.label(), operation, true, Some(&result), &backend)?;
            if trace_facts_smoke_schema {
                record_facts_smoke_diagnostic(
                    window.label(),
                    "facts-smoke:schema-ipc:recorded".to_owned(),
                )?;
            }
            if compact_create_response {
                if let SchemaResponse::Published(publication) = &mut result {
                    // The renderer already has the preceding snapshot. Return
                    // only the new definition instead of the entire catalogue.
                    let revision = publication.revision;
                    publication
                        .definitions
                        .retain(|definition| definition.created_revision == revision);
                }
            }
            Ok(SchemaResponseV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                result,
            })
        }
        Err(_) => {
            record_schema_smoke(window.label(), operation, false, None, &backend)?;
            Err(IpcErrorV1::new("schema_rejected"))
        }
    }
}

#[tauri::command]
async fn manage_entities(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    request: EntityRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<EntityResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    let trace_facts_smoke_entities = window.label() == "primary"
        && std::env::var_os("WORLDDB_ODE_FACTS_SMOKE_RESULT").is_some();
    if trace_facts_smoke_entities {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:entity-ipc:entered".to_owned(),
        )?;
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    if trace_facts_smoke_entities {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:entity-ipc:authorized".to_owned(),
        )?;
    }
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let operation = entity_smoke_operation(&request.command);
    if trace_facts_smoke_entities {
        record_facts_smoke_diagnostic(
            window.label(),
            "facts-smoke:entity-ipc:dispatching".to_owned(),
        )?;
    }
    let result = backend.entities_with_operation_id(request.command, operation_id);
    if trace_facts_smoke_entities {
        record_facts_smoke_diagnostic(
            window.label(),
            format!(
                "facts-smoke:entity-ipc:engine-returned:{}",
                if result.is_ok() { "ok" } else { "rejected" }
            ),
        )?;
    }
    match result {
        Ok(result) => {
            record_entity_smoke(window.label(), operation, true, Some(&result))?;
            if trace_facts_smoke_entities {
                record_facts_smoke_diagnostic(
                    window.label(),
                    "facts-smoke:entity-ipc:recorded".to_owned(),
                )?;
            }
            if matches!(result, EntityResponse::Published(_)) {
                let _ = app.emit("project-state-changed", ());
            }
            Ok(EntityResponseV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                result,
            })
        }
        Err(_) => {
            record_entity_smoke(window.label(), operation, false, None)?;
            Err(IpcErrorV1::new("entity_rejected"))
        }
    }
}

#[tauri::command]
async fn manage_branch_layers(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    request: BranchLayerRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<BranchLayerResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let operation = branch_layer_smoke_operation(&request.command);
    match backend.branch_layers_with_operation_id(request.command, operation_id) {
        Ok(result) => {
            record_branch_layer_smoke(window.label(), operation, true, Some(&result))?;
            if matches!(result, BranchLayerResponse::Published(_)) {
                let _ = app.emit("project-state-changed", ());
            }
            Ok(BranchLayerResponseV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                result,
            })
        }
        Err(_) => {
            record_branch_layer_smoke(window.label(), operation, false, None)?;
            Err(IpcErrorV1::new("branch_layer_rejected"))
        }
    }
}

#[tauri::command]
async fn manage_history_space_transfer(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    request: HistorySpaceTransferRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<HistorySpaceTransferResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let operation = history_space_transfer_smoke_operation(&request.command);
    match backend.history_space_transfer_with_operation_id(request.command, operation_id) {
        Ok(result) => {
            record_history_space_transfer_smoke(window.label(), operation, true, Some(&result))?;
            if matches!(result, HistorySpaceTransferResponse::Published(_)) {
                let _ = app.emit("project-state-changed", ());
            }
            Ok(HistorySpaceTransferResponseV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                result,
            })
        }
        Err(_) => {
            record_history_space_transfer_smoke(window.label(), operation, false, None)?;
            Err(IpcErrorV1::new("history_space_transfer_rejected"))
        }
    }
}

#[tauri::command]
async fn manage_facts(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    request: FactRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<FactResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let is_write = !matches!(
        &request.command,
        FactCommand::Snapshot
            | FactCommand::Preview { .. }
            | FactCommand::Query { .. }
            | FactCommand::CommitStatus { .. }
    );
    let operation = facts_smoke_operation(&request.command);
    let selector_kind = facts_smoke_selector_kind(&request.command);
    match backend.facts_with_operation_id(request.command, operation_id) {
        Ok(result) => {
            record_facts_smoke(
                window.label(),
                operation,
                selector_kind,
                true,
                Some(&result),
            )?;
            if is_write {
                let _ = app.emit("project-state-changed", ());
            }
            if let Some(operation_id) = injected_unknown_commit_response(operation_id) {
                return Err(IpcErrorV1::unknown_commit(operation_id, None));
            }
            Ok(FactResponseV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                result,
            })
        }
        Err(error) => {
            record_facts_smoke(window.label(), operation, selector_kind, false, None)?;
            let _ = record_facts_smoke_diagnostic(
                window.label(),
                format!("facts_error:{operation}:{error}"),
            );
            Err(IpcErrorV1::new("facts_rejected"))
        }
    }
}

#[tauri::command]
fn facts_smoke_diagnostic(window: tauri::WebviewWindow, details: String) -> Result<(), IpcErrorV1> {
    let close_after_smoke = window.label() == "primary"
        && details == "facts-smoke:recovery-smoke:complete"
        && cfg!(debug_assertions)
        && project_smoke_root().is_some()
        && std::env::var_os("WORLDDB_ODE_FACTS_SMOKE_RESULT").is_some();
    let app = window.app_handle().clone();
    record_facts_smoke_diagnostic(window.label(), details)?;
    if close_after_smoke {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(250));
            app.exit(0);
        });
    }
    Ok(())
}

#[tauri::command]
fn focus_native_window(window: tauri::WebviewWindow) -> Result<(), IpcErrorV1> {
    window
        .set_focus()
        .map_err(|_| IpcErrorV1::new("window_focus_failed"))
}

fn record_facts_smoke_diagnostic(window_label: &str, details: String) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_FACTS_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-facts-{window_label}.jsonl"));
    let record = serde_json::json!({
        "operation": "diagnostic",
        "details": details.chars().take(1024).collect::<String>(),
        "window": window_label,
        "succeeded": true,
    });
    append_facts_smoke_jsonl(&result_path, &record, "ipc_diagnostic_unavailable")
}

fn append_facts_smoke_jsonl(
    path: &Path,
    record: &serde_json::Value,
    error_code: &'static str,
) -> Result<(), IpcErrorV1> {
    let _guard = FACTS_SMOKE_RESULT_LOCK
        .lock()
        .map_err(|_| IpcErrorV1::new(error_code))?;
    let mut encoded = serde_json::to_vec(record).map_err(|_| IpcErrorV1::new(error_code))?;
    encoded.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| IpcErrorV1::new(error_code))?;
    file.write_all(&encoded)
        .map_err(|_| IpcErrorV1::new(error_code))
}

#[tauri::command]
async fn manage_perspectives(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    request: PerspectiveRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<PerspectiveResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let operation = perspective_smoke_operation(&request.command);
    match backend.perspectives_with_operation_id(request.command, operation_id) {
        Ok(result) => {
            record_perspective_smoke(window.label(), operation, true, Some(&result))?;
            if matches!(result, PerspectiveResponse::Published(_)) {
                let _ = app.emit("project-state-changed", ());
            }
            Ok(PerspectiveResponseV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                result,
            })
        }
        Err(_) => {
            record_perspective_smoke(window.label(), operation, false, None)?;
            Err(IpcErrorV1::new("perspective_rejected"))
        }
    }
}

#[tauri::command]
async fn manage_security_policy(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    request: SecurityPolicyRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<SecurityPolicyResponseV1, IpcErrorV1> {
    if request.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    sessions
        .authorize(
            window.label(),
            &request.session_id,
            HostCapability::ProjectOpen,
        )
        .map_err(map_session_error)?;
    let operation_id = parse_client_operation_id(request.operation_id.as_deref())?;
    let is_write = !matches!(&request.command, SecurityPolicyCommand::Snapshot);
    let operation = security_policy_smoke_operation(&request.command);
    let result = match backend.security_policy_with_operation_id(request.command, operation_id) {
        Ok(result) => {
            record_security_policy_smoke(window.label(), operation, true, Some(&result))?;
            result
        }
        Err(_) => {
            record_security_policy_smoke(window.label(), operation, false, None)?;
            return Err(IpcErrorV1::new("security_policy_rejected"));
        }
    };
    if is_write {
        let _ = app.emit("project-state-changed", ());
    }
    Ok(SecurityPolicyResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        result,
    })
}

async fn pick_project_parent(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    title: &'static str,
) -> Result<PathBuf, IpcErrorV1> {
    pick_project_folder(app, window, title).await
}

async fn pick_project_folder(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    title: &'static str,
) -> Result<PathBuf, IpcErrorV1> {
    use tauri_plugin_dialog::DialogExt;

    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title(title)
            .blocking_pick_folders()
            .and_then(|mut paths| paths.pop())
            .and_then(|path| path.into_path().ok())
    })
    .await
    .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    selected.ok_or_else(|| IpcErrorV1::new("selection_cancelled"))
}

async fn pick_worlddb_file(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    title: &'static str,
    filter_name: &'static str,
    extensions: &'static [&'static str],
) -> Result<PathBuf, IpcErrorV1> {
    use tauri_plugin_dialog::DialogExt;

    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title(title)
            .add_filter(filter_name, extensions)
            .blocking_pick_file()
            .and_then(|path| path.into_path().ok())
    })
    .await
    .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    selected.ok_or_else(|| IpcErrorV1::new("selection_cancelled"))
}

async fn pick_output_file(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    title: &'static str,
    filter_name: &'static str,
    extensions: &'static [&'static str],
    default_name: &'static str,
) -> Result<PathBuf, IpcErrorV1> {
    use tauri_plugin_dialog::DialogExt;

    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title(title)
            .set_file_name(default_name)
            .add_filter(filter_name, extensions)
            .blocking_save_file()
            .and_then(|path| path.into_path().ok())
    })
    .await
    .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    selected.ok_or_else(|| IpcErrorV1::new("selection_cancelled"))
}

async fn pick_migration_plan_file(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<PathBuf, IpcErrorV1> {
    use tauri_plugin_dialog::DialogExt;

    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title("Kanonischen MigrationPlan auswählen")
            .add_filter("WorldDB Record", &["record"])
            .blocking_pick_file()
            .and_then(|path| path.into_path().ok())
    })
    .await
    .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    selected.ok_or_else(|| IpcErrorV1::new("selection_cancelled"))
}

async fn pick_migration_record_files(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    title: String,
) -> Result<Vec<PathBuf>, IpcErrorV1> {
    use tauri_plugin_dialog::DialogExt;

    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title(title)
            .add_filter("WorldDB Record", &["record"])
            .blocking_pick_files()
            .map(|paths| {
                paths
                    .into_iter()
                    .map(|path| path.into_path().map_err(|_| ()))
                    .collect::<Result<Vec<_>, _>>()
            })
    })
    .await
    .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    match selected {
        None => Ok(Vec::new()),
        Some(Ok(paths)) => Ok(paths),
        Some(Err(())) => Err(IpcErrorV1::new("host_unavailable")),
    }
}

fn project_smoke_root() -> Option<PathBuf> {
    cfg!(debug_assertions)
        .then(|| std::env::var_os("WORLDDB_ODE_PROJECT_SMOKE_ROOT").map(PathBuf::from))
        .flatten()
}

fn validate_project_name(value: &str) -> Result<String, IpcErrorV1> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed != value
        || trimmed.chars().count() > 80
        || trimmed == "."
        || trimmed == ".."
        || trimmed.ends_with('.')
        || trimmed.ends_with(' ')
        || trimmed
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
    {
        return Err(IpcErrorV1::new("invalid_request"));
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
    {
        return Err(IpcErrorV1::new("invalid_request"));
    }
    Ok(trimmed.to_owned())
}

fn record_project_smoke(
    window_label: &str,
    status: &ProjectStatusV1,
    engine: Option<&Response>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_IPC_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-project-{window_label}.json"));
    let record = serde_json::json!({
        "protocol_version": status.protocol_version,
        "window": window_label,
        "project_open": status.project_open,
        "database_id": status.database_id,
        "revision": status.revision,
        "role": status.role,
        "snapshot_id": status.snapshot_id,
        "compatibility": status.compatibility,
        "engine": engine,
    });
    std::fs::write(
        result_path,
        serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?,
    )
    .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

fn schema_smoke_operation(command: &SchemaCommand) -> &'static str {
    match command {
        SchemaCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Current,
        } => "snapshot_current",
        SchemaCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Historical { .. },
        } => "snapshot_historical",
        SchemaCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Explicit { .. },
        } => "snapshot_explicit",
        SchemaCommand::Create { .. } => "create",
        SchemaCommand::SetLifecycle { .. } => "set_lifecycle",
        SchemaCommand::SetLifecycleBatch { .. } => "set_lifecycle_batch",
    }
}

fn entity_smoke_operation(command: &EntityCommand) -> &'static str {
    match command {
        EntityCommand::Snapshot {
            mode: EntityModeInput::Current,
        } => "snapshot_current",
        EntityCommand::Snapshot {
            mode: EntityModeInput::Historical { .. },
        } => "snapshot_historical",
        EntityCommand::Snapshot {
            mode: EntityModeInput::Explicit { .. },
        } => "snapshot_explicit",
        EntityCommand::Create { .. } => "create",
        EntityCommand::Retire { .. } => "retire",
    }
}

fn branch_layer_smoke_operation(command: &BranchLayerCommand) -> &'static str {
    match command {
        BranchLayerCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Current,
        } => "snapshot_current",
        BranchLayerCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Historical { .. },
        } => "snapshot_historical",
        BranchLayerCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Explicit { .. },
        } => "snapshot_explicit",
        BranchLayerCommand::CreateChild { .. } => "create_child",
        BranchLayerCommand::CreateLayer { .. } => "create_layer",
        BranchLayerCommand::ReviseLayer { .. } => "revise_layer",
    }
}

fn history_space_transfer_smoke_operation(command: &HistorySpaceTransferCommand) -> &'static str {
    match command {
        HistorySpaceTransferCommand::List { .. } => "list",
        HistorySpaceTransferCommand::Preview { .. } => "preview",
        HistorySpaceTransferCommand::Commit { .. } => "commit",
    }
}

fn perspective_smoke_operation(command: &PerspectiveCommand) -> &'static str {
    match command {
        PerspectiveCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Current,
        } => "snapshot_current",
        PerspectiveCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Historical { .. },
        } => "snapshot_historical",
        PerspectiveCommand::Snapshot {
            mode: worlddb_ode_engine::SchemaModeInput::Explicit { .. },
        } => "snapshot_explicit",
        PerspectiveCommand::Create { .. } => "create",
        PerspectiveCommand::Update { .. } => "update",
        PerspectiveCommand::Retire { .. } => "retire",
        PerspectiveCommand::ValidateContext { .. } => "validate_context",
    }
}

fn security_policy_smoke_operation(command: &SecurityPolicyCommand) -> &'static str {
    match command {
        SecurityPolicyCommand::Snapshot => "snapshot",
        SecurityPolicyCommand::SetPrincipalState { .. } => "set_principal_state",
        SecurityPolicyCommand::RegisterRole { .. } => "register_role",
        SecurityPolicyCommand::AssignRole { .. } => "assign_role",
        SecurityPolicyCommand::RevokeRoleAssignment { .. } => "revoke_role_assignment",
        SecurityPolicyCommand::AddCapabilityRule { .. } => "add_capability_rule",
        SecurityPolicyCommand::RevokeCapabilityRule { .. } => "revoke_capability_rule",
    }
}

fn facts_smoke_operation(command: &FactCommand) -> &'static str {
    match command {
        FactCommand::Snapshot => "snapshot",
        FactCommand::CreateSource { .. } => "create_source",
        FactCommand::SupersedeSource { .. } => "supersede_source",
        FactCommand::CreateEvidence { .. } => "create_evidence",
        FactCommand::CreateProvenance { .. } => "create_provenance",
        FactCommand::RetractEvidence { .. } => "retract_evidence",
        FactCommand::RetractProvenance { .. } => "retract_provenance",
        FactCommand::CreateAssertion { .. } => "create_assertion",
        FactCommand::CreateMask { .. } => "create_mask",
        FactCommand::CreateReplacementBoundary { .. } => "create_replacement_boundary",
        FactCommand::CreateEvent { .. } => "create_event",
        FactCommand::CreateEventMask { .. } => "create_event_mask",
        FactCommand::CreateEventRelation { .. } => "create_event_relation",
        FactCommand::CloseEventSpan { .. } => "close_event_span",
        FactCommand::CorrectAssertion { .. } => "correct_assertion",
        FactCommand::CorrectEvent { .. } => "correct_event",
        FactCommand::CommitStatus { .. } => "commit_status",
        FactCommand::Lifecycle { .. } => "lifecycle",
        FactCommand::Query { .. } => "query",
        FactCommand::Preview { .. } => "preview",
    }
}

fn facts_smoke_selector_kind(command: &FactCommand) -> Option<&'static str> {
    match command {
        FactCommand::CreateMask { selector, .. } => Some(match selector {
            MaskSelectorInput::ExactAssertion { .. } => "exact_assertion",
            MaskSelectorInput::Proposition { .. } => "proposition",
            MaskSelectorInput::Slot { .. } => "slot",
        }),
        _ => None,
    }
}

fn record_facts_smoke(
    window_label: &str,
    operation: &str,
    selector_kind: Option<&str>,
    succeeded: bool,
    result: Option<&FactResponse>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_FACTS_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-facts-{window_label}.jsonl"));
    let (kind, revision, family, record_id, result_kind, slice_count, outcome_kind) = result
        .map_or(
            (None, None, None, None, None, None, None),
            |response| match response {
                FactResponse::Catalog(catalog) => (
                    Some("catalog"),
                    Some(catalog.revision),
                    None,
                    None,
                    None,
                    Some(catalog.records.len()),
                    None,
                ),
                FactResponse::Published(publication) => (
                    Some("published"),
                    Some(publication.revision),
                    Some(publication.family.as_str()),
                    Some(publication.record_id.as_str()),
                    None,
                    None,
                    None,
                ),
                FactResponse::Preview(preview) => {
                    let (result_kind, slice_count, outcome_kind) = match &preview.result {
                        ResolutionResultView::Point { outcome, .. } => {
                            ("point", None, Some(resolution_outcome_kind(outcome)))
                        }
                        ResolutionResultView::AllTimes { slices } => (
                            "all_times",
                            Some(slices.len()),
                            slices
                                .first()
                                .map(|slice| resolution_outcome_kind(&slice.outcome)),
                        ),
                        ResolutionResultView::CompleteEmpty => ("complete_empty", Some(0), None),
                    };
                    (
                        Some("preview"),
                        Some(preview.revision),
                        None,
                        None,
                        Some(result_kind),
                        slice_count,
                        outcome_kind,
                    )
                }
                FactResponse::Query(query) => {
                    let (result_kind, item_count, outcome_kind) = match &query.result {
                        worlddb_ode_engine::FactQueryResultView::History { records } => {
                            ("history", Some(records.len()), None)
                        }
                        worlddb_ode_engine::FactQueryResultView::Resolved { result } => {
                            match result {
                                ResolutionResultView::Point { outcome, .. } => (
                                    "resolved_point",
                                    None,
                                    Some(resolution_outcome_kind(outcome)),
                                ),
                                ResolutionResultView::AllTimes { slices } => (
                                    "resolved_all_times",
                                    Some(slices.len()),
                                    slices
                                        .first()
                                        .map(|slice| resolution_outcome_kind(&slice.outcome)),
                                ),
                                ResolutionResultView::CompleteEmpty => {
                                    ("resolved_complete_empty", Some(0), None)
                                }
                            }
                        }
                        worlddb_ode_engine::FactQueryResultView::Explain {
                            outcome,
                            stages,
                            ..
                        } => (
                            "explain",
                            Some(stages.len()),
                            Some(resolution_outcome_kind(outcome)),
                        ),
                        worlddb_ode_engine::FactQueryResultView::TokenSearch {
                            hits,
                            result_complete,
                            ..
                        } => (
                            if *result_complete {
                                "token_search_complete"
                            } else {
                                "token_search_page"
                            },
                            Some(hits.len()),
                            None,
                        ),
                        worlddb_ode_engine::FactQueryResultView::Graph { nodes, edges, .. } => {
                            ("graph", Some(nodes.len() + edges.len()), Some("complete"))
                        }
                        worlddb_ode_engine::FactQueryResultView::Aggregate { result } => match result {
                            worlddb_ode_engine::FactQueryAggregateResultView::Count { value } => (
                                "aggregate_count",
                                value.parse::<usize>().ok(),
                                None,
                            ),
                            worlddb_ode_engine::FactQueryAggregateResultView::Exists { value } => (
                                "aggregate_exists",
                                Some(usize::from(*value)),
                                None,
                            ),
                            worlddb_ode_engine::FactQueryAggregateResultView::GroupedCount { groups } => (
                                "aggregate_grouped_count",
                                Some(groups.len()),
                                None,
                            ),
                        },
                    };
                    (
                        Some("query"),
                        query.snapshot_revision.parse::<u64>().ok(),
                        None,
                        None,
                        Some(result_kind),
                        item_count,
                        outcome_kind,
                    )
                }
                FactResponse::AssertionCorrected(correction) => (
                    Some("assertion_corrected"),
                    Some(correction.revision),
                    Some("assertion_correction"),
                    Some(correction.replacement_assertion_id.as_str()),
                    None,
                    Some(3),
                    None,
                ),
                FactResponse::EventCorrected(correction) => (
                    Some("event_corrected"),
                    Some(correction.revision),
                    Some("event_correction"),
                    Some(correction.replacement_event_id.as_str()),
                    None,
                    Some(2),
                    None,
                ),
                FactResponse::EventGraphConflict(conflict) => (
                    Some("event_graph_conflict"),
                    None,
                    None,
                    None,
                    Some(if conflict.relation_saved {
                        "unexpected_saved"
                    } else {
                        "not_saved"
                    }),
                    None,
                    Some(if conflict.automatic_inference_applied {
                        "inference_applied"
                    } else {
                        "no_automatic_inference"
                    }),
                ),
                FactResponse::OperationStatus(status) => (
                    Some("operation_status"),
                    status.revision,
                    None,
                    None,
                    Some(match status.status {
                        worlddb_ode_engine::FactOperationStatusKind::NotCommitted => {
                            "not_committed"
                        }
                        worlddb_ode_engine::FactOperationStatusKind::Committed => "committed",
                        worlddb_ode_engine::FactOperationStatusKind::Indeterminate => {
                            "indeterminate"
                        }
                    }),
                    None,
                    None,
                ),
                FactResponse::LifecycleChanged(change) => (
                    Some("lifecycle_changed"),
                    Some(change.revision),
                    Some(change.target_family.as_str()),
                    Some(change.record_id.as_str()),
                    Some(change.effect.as_str()),
                    None,
                    None,
                ),
            },
        );
    let (query_mode, recorded_as_of, schema_mode, schema_revision) =
        result.map_or((None, None, None, None), |response| match response {
            FactResponse::Query(query) => (
                Some(match query.query_mode {
                    FactQueryModeInput::History => "history",
                    FactQueryModeInput::Resolved => "resolved",
                    FactQueryModeInput::Explain => "explain",
                    FactQueryModeInput::TokenSearch => "token_search",
                    FactQueryModeInput::Graph => "graph",
                    FactQueryModeInput::Count => "count",
                    FactQueryModeInput::Exists => "exists",
                    FactQueryModeInput::GroupedCount => "grouped_count",
                }),
                Some(query.recorded_as_of.clone()),
                Some(match &query.schema_mode {
                    FactQuerySchemaModeInput::Historical { .. } => "historical",
                    FactQuerySchemaModeInput::Current => "current",
                    FactQuerySchemaModeInput::Explicit { .. } => "explicit",
                }),
                Some(query.schema_revision.clone()),
            ),
            _ => (None, None, None, None),
        });
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "selector_kind": selector_kind,
        "succeeded": succeeded,
        "kind": kind,
        "revision": revision,
        "family": family,
        "record_id": record_id,
        "result_kind": result_kind,
        "slice_count": slice_count,
        "outcome_kind": outcome_kind,
        "query_mode": query_mode,
        "recorded_as_of": recorded_as_of,
        "schema_mode": schema_mode,
        "schema_revision": schema_revision,
        "snapshot_revision_exact": result.and_then(|response| match response {
            FactResponse::Query(query) => Some(query.snapshot_revision.clone()),
            _ => None,
        }),
    });
    append_facts_smoke_jsonl(&result_path, &record, "host_unavailable")
}

fn resolution_outcome_kind(outcome: &ResolutionOutcomeView) -> &'static str {
    match outcome {
        ResolutionOutcomeView::Known { .. } => "known",
        ResolutionOutcomeView::Unknown => "unknown",
        ResolutionOutcomeView::Conflict { .. } => "conflict",
    }
}

fn record_security_policy_smoke(
    window_label: &str,
    operation: &str,
    succeeded: bool,
    result: Option<&SecurityPolicyResponse>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_SECURITY_POLICY_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-security-policy-{window_label}.jsonl"));
    let fields = result.map_or_else(
        SecurityPolicySmokeFields::default,
        |response| match response {
            SecurityPolicyResponse::Snapshot(snapshot) => policy_smoke_snapshot_fields(snapshot),
            SecurityPolicyResponse::Published(publication) => SecurityPolicySmokeFields {
                revision: Some(publication.revision),
                security_epoch: Some(publication.security_epoch),
                ..SecurityPolicySmokeFields::default()
            },
        },
    );
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "succeeded": succeeded,
        "revision": fields.revision,
        "security_epoch": fields.security_epoch,
        "principal_count": fields.principal_count,
        "role_count": fields.role_count,
        "assignment_count": fields.assignment_count,
        "explicit_rule_count": fields.rule_count,
        "gm_admin_raw_allow": fields.gm_admin_raw_allow,
        "gm_raw_history_allow": fields.gm_raw_history_allow,
        "gm_admin_raw_deny": fields.gm_admin_raw_deny,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(result_path)
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let encoded = serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    file.write_all(&encoded)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

#[derive(Default)]
struct SecurityPolicySmokeFields {
    revision: Option<u64>,
    security_epoch: Option<u64>,
    principal_count: Option<usize>,
    role_count: Option<usize>,
    assignment_count: Option<usize>,
    rule_count: Option<usize>,
    gm_admin_raw_allow: Option<bool>,
    gm_raw_history_allow: Option<bool>,
    gm_admin_raw_deny: Option<bool>,
}

fn policy_smoke_snapshot_fields(
    snapshot: &SecurityPolicySnapshotView,
) -> SecurityPolicySmokeFields {
    let gm = snapshot.roles.iter().find(|role| role.symbol == "gm");
    let gm_admin_raw_allow = gm.is_some_and(|role| {
        role.bundle
            .iter()
            .any(|rule| rule.capability == "admin_raw_read" && rule.effect == "allow")
    });
    let gm_raw_history_allow = gm.is_some_and(|role| {
        role.bundle
            .iter()
            .any(|rule| rule.capability == "raw_history_read" && rule.effect == "allow")
    });
    let gm_admin_raw_deny = gm.is_some_and(|role| {
        snapshot.explicit_rules.iter().any(|rule| {
            rule.subject_kind == "role"
                && rule.subject_id == role.role_id
                && rule.capability == "admin_raw_read"
                && rule.effect == "deny"
        })
    });
    SecurityPolicySmokeFields {
        revision: Some(snapshot.revision),
        security_epoch: Some(snapshot.security_epoch),
        principal_count: Some(snapshot.principals.len()),
        role_count: Some(snapshot.roles.len()),
        assignment_count: Some(snapshot.assignments.len()),
        rule_count: Some(snapshot.explicit_rules.len()),
        gm_admin_raw_allow: Some(gm_admin_raw_allow),
        gm_raw_history_allow: Some(gm_raw_history_allow),
        gm_admin_raw_deny: Some(gm_admin_raw_deny),
    }
}

fn record_entity_smoke(
    window_label: &str,
    operation: &str,
    succeeded: bool,
    result: Option<&EntityResponse>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_ENTITY_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-entity-{window_label}.jsonl"));
    let (revision, entity_count, entity_types, warning) = result.map_or(
        (None, None, serde_json::Value::Null, serde_json::Value::Null),
        |response| match response {
            EntityResponse::Snapshot(snapshot) => (
                Some(snapshot.revision),
                Some(snapshot.entities.len()),
                serde_json::Value::Array(
                    snapshot
                        .entity_types
                        .iter()
                        .map(|entity_type| {
                            serde_json::json!({
                                "symbol": entity_type.symbol,
                                "lifecycle": entity_type.lifecycle,
                            })
                        })
                        .collect(),
                ),
                serde_json::Value::Null,
            ),
            EntityResponse::Published(publication) => (
                Some(publication.revision),
                None,
                serde_json::Value::Null,
                publication.warning.as_ref().map_or(
                    serde_json::Value::Null,
                    |warning| serde_json::json!({ "code": warning.code }),
                ),
            ),
        },
    );
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "succeeded": succeeded,
        "revision": revision,
        "entity_count": entity_count,
        "entity_types": entity_types,
        "warning": warning,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(result_path)
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let encoded = serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    file.write_all(&encoded)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

fn record_branch_layer_smoke(
    window_label: &str,
    operation: &str,
    succeeded: bool,
    result: Option<&BranchLayerResponse>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_BRANCH_LAYER_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-branch-layer-{window_label}.jsonl"));
    let (revision, branch_count, layer_count, branch_created, layer_changed) =
        result.map_or((None, None, None, None, None), |response| match response {
            BranchLayerResponse::Snapshot(snapshot) => (
                Some(snapshot.revision),
                Some(snapshot.branches.len()),
                Some(snapshot.layers.len()),
                None,
                None,
            ),
            BranchLayerResponse::Published(publication) => (
                Some(publication.revision),
                None,
                None,
                Some(publication.branch_created),
                Some(publication.layer_changed),
            ),
        });
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "succeeded": succeeded,
        "revision": revision,
        "branch_count": branch_count,
        "layer_count": layer_count,
        "branch_created": branch_created,
        "layer_changed": layer_changed,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(result_path)
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let encoded = serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    file.write_all(&encoded)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

fn record_history_space_transfer_smoke(
    window_label: &str,
    operation: &str,
    succeeded: bool,
    result: Option<&HistorySpaceTransferResponse>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_TRANSFER_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-transfer-{window_label}.jsonl"));
    let (record_count, relation_count, copied_records, copied_relations) =
        result.map_or((None, None, None, None), |response| match response {
            HistorySpaceTransferResponse::Catalog(catalog) => (
                Some(catalog.records.len()),
                Some(catalog.event_relations.len()),
                None,
                None,
            ),
            HistorySpaceTransferResponse::Preview(preview) => (
                None,
                None,
                Some(preview.copied_record_count),
                Some(preview.copied_relation_count),
            ),
            HistorySpaceTransferResponse::Published(receipt) => (
                None,
                None,
                Some(receipt.copied_record_count),
                Some(receipt.copied_relation_count),
            ),
        });
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "succeeded": succeeded,
        "record_count": record_count,
        "relation_count": relation_count,
        "copied_record_count": copied_records,
        "copied_relation_count": copied_relations,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(result_path)
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let encoded = serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    file.write_all(&encoded)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

fn record_perspective_smoke(
    window_label: &str,
    operation: &str,
    succeeded: bool,
    result: Option<&PerspectiveResponse>,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_PERSPECTIVE_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-perspective-{window_label}.jsonl"));
    let (revision, perspective_count, mode) =
        result.map_or((None, None, None), |response| match response {
            PerspectiveResponse::Snapshot(snapshot) => (
                Some(snapshot.revision),
                Some(snapshot.perspectives.len()),
                None,
            ),
            PerspectiveResponse::Published(publication) => (Some(publication.revision), None, None),
            PerspectiveResponse::ContextBound(context) => (
                None,
                None,
                Some(format!("{:?}", context.epistemic_mode).to_ascii_lowercase()),
            ),
        });
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "succeeded": succeeded,
        "revision": revision,
        "perspective_count": perspective_count,
        "epistemic_mode": mode,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(result_path)
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let encoded = serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    file.write_all(&encoded)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

fn record_schema_smoke(
    window_label: &str,
    operation: &str,
    succeeded: bool,
    result: Option<&SchemaResponse>,
    backend: &Backend,
) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_SCHEMA_SMOKE_RESULT") else {
        return Ok(());
    };
    if !cfg!(debug_assertions) || project_smoke_root().is_none() {
        return Ok(());
    }
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path =
        result_prefix.with_file_name(format!("{file_stem}-schema-{window_label}.jsonl"));
    let (revision, definition_count, definitions) =
        result.map_or((None, None, serde_json::Value::Null), |response| {
            let (revision, count, values) = match response {
                SchemaResponse::Snapshot(snapshot) => (
                    snapshot.revision,
                    snapshot.definitions.len(),
                    &snapshot.definitions,
                ),
                SchemaResponse::Published(publication) => (
                    publication.revision,
                    publication.definition_count,
                    &publication.definitions,
                ),
            };
            let definitions = values
                .iter()
                .map(|definition| {
                    serde_json::json!({
                        "family": definition.family,
                        "identity": definition.identity,
                        "symbol": definition.symbol,
                        "lifecycle": definition.lifecycle,
                    })
                })
                .collect::<Vec<_>>();
            (
                Some(revision),
                Some(count),
                serde_json::Value::Array(definitions),
            )
        });
    let project_open = backend
        .project_status(window_label)
        .ok()
        .map(|status| status.project_open);
    let record = serde_json::json!({
        "window": window_label,
        "operation": operation,
        "succeeded": succeeded,
        "project_open": project_open,
        "revision": revision,
        "definition_count": definition_count,
        "definitions": definitions,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(result_path)
        .map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    let encoded = serde_json::to_vec(&record).map_err(|_| IpcErrorV1::new("host_unavailable"))?;
    file.write_all(&encoded)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

fn record_ipc_probe(window_label: &str) -> Result<(), IpcErrorV1> {
    let Some(result_prefix) = std::env::var_os("WORLDDB_ODE_IPC_RESULT") else {
        return Ok(());
    };
    let result_prefix = PathBuf::from(result_prefix);
    let file_stem = result_prefix
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("ipc");
    let result_path = result_prefix.with_file_name(format!("{file_stem}-{window_label}.json"));
    let result = serde_json::json!({
        "protocol_version": IPC_PROTOCOL_VERSION,
        "window": window_label,
        "status": "authorized_health_ok",
        "security_probe_mode": cfg!(debug_assertions) && std::env::var_os("WORLDDB_ODE_IPC_RESULT").is_some(),
    });
    std::fs::write(
        result_path,
        serde_json::to_vec(&result).map_err(|_| IpcErrorV1::new("host_unavailable"))?,
    )
    .map_err(|_| IpcErrorV1::new("host_unavailable"))
}

#[tauri::command]
fn begin_transfer(
    window: tauri::WebviewWindow,
    session_id: String,
    request: BeginTransferRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    transfers: tauri::State<'_, TransferManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<BeginTransferResponseV1, IpcErrorV1> {
    sessions
        .authorize(window.label(), &session_id, HostCapability::TransferWrite)
        .map_err(map_session_error)?;
    transfers
        .begin(
            &session_id,
            request,
            |transfer_id, total_bytes, chunk_bytes| {
                backend.begin_stream(transfer_id, total_bytes, chunk_bytes)
            },
        )
        .map_err(map_transfer_error)
}

struct ChunkMetadata<'a> {
    session_id: &'a str,
    transfer_id: &'a str,
    sequence: u64,
}

fn chunk_metadata(headers: &tauri::http::HeaderMap) -> Result<ChunkMetadata<'_>, IpcErrorV1> {
    fn required<'a>(
        headers: &'a tauri::http::HeaderMap,
        name: &str,
    ) -> Result<&'a str, IpcErrorV1> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| IpcErrorV1::new("invalid_request"))
    }

    let version = required(headers, "x-worlddb-protocol-version")?
        .parse::<u16>()
        .map_err(|_| IpcErrorV1::new("invalid_request"))?;
    if version != IPC_PROTOCOL_VERSION {
        return Err(IpcErrorV1::new("unsupported_protocol"));
    }
    let sequence = required(headers, "x-worlddb-sequence")?
        .parse::<u64>()
        .map_err(|_| IpcErrorV1::new("invalid_request"))?;
    Ok(ChunkMetadata {
        session_id: required(headers, "x-worlddb-session")?,
        transfer_id: required(headers, "x-worlddb-transfer")?,
        sequence,
    })
}

#[cfg(test)]
mod ipc_security_tests {
    use super::{
        CloseProjectRequestV1, CreateProjectRequestV1, DiagnosticExportRequestV1, HealthRequestV1,
        IpcErrorV1, MigrationRequestV1, MigrationResolutionRequestV1, MigrationRunRequestV1,
        OpenProjectRequestV1, RecoveryApplyRequestV1, RecoveryRequestV1, RecoverySalvageRequestV1,
        chunk_metadata, validate_project_name,
    };
    use crate::diagnostics;
    use std::str::FromStr;
    use tauri::http::{HeaderMap, HeaderValue};
    use worlddb_core::{DatabaseId, OperationId};

    fn valid_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-worlddb-protocol-version", HeaderValue::from_static("1"));
        headers.insert(
            "x-worlddb-session",
            HeaderValue::from_static("session-ticket"),
        );
        headers.insert(
            "x-worlddb-transfer",
            HeaderValue::from_static("transfer-ticket"),
        );
        headers.insert("x-worlddb-sequence", HeaderValue::from_static("7"));
        headers
    }

    #[test]
    fn binary_chunk_headers_require_a_versioned_session_transfer_and_sequence() {
        let headers = valid_headers();
        let parsed = chunk_metadata(&headers).expect("valid chunk metadata");
        assert_eq!(parsed.session_id, "session-ticket");
        assert_eq!(parsed.transfer_id, "transfer-ticket");
        assert_eq!(parsed.sequence, 7);

        let mut missing_sequence = valid_headers();
        missing_sequence.remove("x-worlddb-sequence");
        assert!(chunk_metadata(&missing_sequence).is_err());

        let mut unsupported_version = valid_headers();
        unsupported_version.insert("x-worlddb-protocol-version", HeaderValue::from_static("2"));
        assert!(chunk_metadata(&unsupported_version).is_err());

        let mut malformed_sequence = valid_headers();
        malformed_sequence.insert("x-worlddb-sequence", HeaderValue::from_static("-1"));
        assert!(chunk_metadata(&malformed_sequence).is_err());
    }

    #[test]
    fn health_dto_rejects_unversioned_path_and_principal_fields() {
        let invalid = serde_json::json!({
            "protocol_version": 1,
            "session_id": "session-ticket",
            "path": "C:/renderer/chosen/database",
            "principal": "renderer-selected-principal"
        });
        assert!(serde_json::from_value::<HealthRequestV1>(invalid).is_err());
    }

    #[test]
    fn diagnostic_export_request_rejects_renderer_selected_paths() {
        assert!(
            serde_json::from_value::<DiagnosticExportRequestV1>(serde_json::json!({
                "protocol_version": 1,
                "path": "C:/renderer/chosen/diagnostics.json"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DiagnosticExportRequestV1>(serde_json::json!({
                "protocol_version": 1
            }))
            .is_ok()
        );
    }

    #[test]
    fn public_ipc_error_and_debug_never_include_internal_cause_canary() {
        const CANARY: &str = "WDB_INTERNAL_CAUSE_CANARY_93D1";
        let store = diagnostics::DiagnosticStore::new();
        store.record(Some("db-a"), "migration_rejected", CANARY);
        let error = IpcErrorV1::new("migration_rejected");
        let serialized = serde_json::to_string(&error).expect("safe public IPC error");
        let debug = format!("{error:?}");
        let permit =
            diagnostics::authorize_export(&["audit_read".to_owned(), "audit_export".to_owned()])
                .expect("explicit audit export permissions");
        let internal = store
            .export_bundle("db-a", &permit)
            .expect("host diagnostic bundle");
        let authorized_export = serde_json::to_string(&internal).expect("diagnostic export");

        assert!(serialized.contains("migration_rejected"));
        assert!(serialized.contains("worlddb.error.migration_rejected"));
        assert!(serialized.contains("worlddb.error.action.contact_support"));
        assert!(!serialized.contains(CANARY));
        assert!(!debug.contains(CANARY));
        assert!(authorized_export.contains(CANARY));
    }

    #[test]
    fn project_dtos_reject_renderer_selected_paths_and_principals() {
        let invalid = serde_json::json!({
            "protocol_version": 1,
            "project_name": "Campaign",
            "project_path": "C:/renderer/chosen/database",
            "principal": "renderer-selected-principal"
        });
        assert!(serde_json::from_value::<CreateProjectRequestV1>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<OpenProjectRequestV1>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<CloseProjectRequestV1>(invalid).is_err());
    }

    #[test]
    fn recovery_dtos_reject_renderer_paths_and_require_explicit_apply_confirmation() {
        let invalid = serde_json::json!({
            "protocol_version": 1,
            "project_path": "C:/renderer/chosen/database",
        });
        assert!(serde_json::from_value::<RecoveryRequestV1>(invalid).is_err());
        assert!(
            serde_json::from_value::<RecoveryApplyRequestV1>(serde_json::json!({
                "protocol_version": 1,
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RecoverySalvageRequestV1>(serde_json::json!({
                "protocol_version": 1,
                "archive_name": "safe-name",
                "destination": "C:/renderer/chosen/database",
            }))
            .is_err()
        );
    }

    #[test]
    fn migration_dtos_reject_renderer_paths_and_require_a_breaking_decision_field() {
        assert!(
            serde_json::from_value::<MigrationRequestV1>(serde_json::json!({
                "protocol_version": 1,
                "source_path": "C:/renderer/selected/project",
                "plan_path": "C:/renderer/selected/plan.record"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<MigrationRunRequestV1>(serde_json::json!({
                "protocol_version": 1,
                "backup_path": "C:/renderer/selected/backup",
                "restore_path": "C:/renderer/selected/restore"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<MigrationRunRequestV1>(serde_json::json!({
                "protocol_version": 1,
                "confirmed_breaking": true
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<MigrationResolutionRequestV1>(serde_json::json!({
                "protocol_version": 1,
                "omitted_record_indexes": ["0", "2"],
                "replacement_path": "C:/renderer/replacement.record"
            }))
            .is_err()
        );
    }

    #[test]
    fn project_names_are_single_safe_windows_directory_names() {
        assert_eq!(
            validate_project_name("Campaign 2026").unwrap(),
            "Campaign 2026"
        );
        for invalid in ["", ".", "..", "CON", "LPT1", "folder/name", "campaign."] {
            assert!(
                validate_project_name(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn unknown_project_commit_returns_operation_and_database_identity() {
        let operation_id = OperationId::from_str("00000000-0000-7000-8000-000000000031")
            .expect("valid operation ID");
        let database_id = DatabaseId::from_str("00000000-0000-7000-8000-000000000032")
            .expect("valid database ID");
        let error = IpcErrorV1::unknown_commit(operation_id, Some(database_id.to_string()));
        let value = serde_json::to_value(error).expect("serializable project failure");
        assert_eq!(value["code"], "unknown_commit_outcome");
        assert_eq!(value["message_key"], "worlddb.error.unknown_commit_outcome");
        assert_eq!(
            value["next_action_key"],
            "worlddb.error.action.resolve_operation"
        );
        assert_eq!(value["operation_id"], operation_id.to_string());
        assert_eq!(value["database_id"], database_id.to_string());
    }
}

#[tauri::command]
fn push_transfer_chunk(
    window: tauri::WebviewWindow,
    request: tauri::ipc::Request<'_>,
    sessions: tauri::State<'_, HostSessionManager>,
    transfers: tauri::State<'_, TransferManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<ChunkAcknowledgementV1, IpcErrorV1> {
    let metadata = chunk_metadata(request.headers())?;
    sessions
        .authorize(
            window.label(),
            metadata.session_id,
            HostCapability::TransferWrite,
        )
        .map_err(map_session_error)?;
    let tauri::ipc::InvokeBody::Raw(chunk) = request.body() else {
        return Err(IpcErrorV1::new("invalid_request"));
    };
    if chunk.is_empty() || chunk.len() > MAX_TRANSFER_CHUNK_BYTES as usize {
        return Err(IpcErrorV1::new("invalid_request"));
    }
    transfers
        .push_chunk(
            metadata.session_id,
            metadata.transfer_id,
            metadata.sequence,
            chunk,
            |transfer_id, sequence, bytes| backend.push_stream_chunk(transfer_id, sequence, bytes),
        )
        .map_err(map_transfer_error)
}

#[tauri::command]
fn finish_transfer(
    window: tauri::WebviewWindow,
    session_id: String,
    request: FinishTransferRequestV1,
    sessions: tauri::State<'_, HostSessionManager>,
    transfers: tauri::State<'_, TransferManager>,
    backend: tauri::State<'_, Backend>,
) -> Result<TransferCompletionV1, IpcErrorV1> {
    sessions
        .authorize(window.label(), &session_id, HostCapability::TransferWrite)
        .map_err(map_session_error)?;
    transfers
        .finish(&session_id, request, |transfer_id, cancelled| {
            backend.finish_stream(transfer_id, cancelled)
        })
        .map_err(map_transfer_error)
}

fn map_session_error(error: SessionError) -> IpcErrorV1 {
    match error {
        SessionError::Unauthorized => IpcErrorV1::new("unauthorized"),
        SessionError::Unavailable => IpcErrorV1::new("host_unavailable"),
    }
}

fn map_transfer_error(error: TransferError) -> IpcErrorV1 {
    match error {
        TransferError::Unauthorized => IpcErrorV1::new("unauthorized"),
        TransferError::Rejected => IpcErrorV1::new("invalid_request"),
        TransferError::Unavailable => IpcErrorV1::new("engine_unavailable"),
    }
}

struct WindowProjectSnapshot {
    snapshot_id: String,
    revision: u64,
}

struct BackendState {
    engine: Option<EngineBackend>,
    project: Option<worlddb_ode_engine::ProjectAccess>,
    recovery_root: Option<PathBuf>,
    migration_draft: Option<migration::MigrationDraft>,
    purge_draft: Option<purge::PurgeDraft>,
    migration_dialog_active: bool,
    windows: HashMap<String, WindowProjectSnapshot>,
}

struct Backend {
    state: Mutex<BackendState>,
    diagnostics: diagnostics::DiagnosticStore,
}

impl Backend {
    fn new(database_root: Option<&Path>) -> Result<Self, String> {
        let engine = if let Some(database_root) = database_root {
            #[cfg(feature = "sidecar")]
            {
                Some(EngineBackend::Sidecar(Mutex::new(Sidecar::start(
                    database_root,
                )?)))
            }
            #[cfg(feature = "in-process")]
            {
                Some(EngineBackend::InProcess(
                    EngineHost::open(database_root).map_err(|error| error.to_string())?,
                ))
            }
        } else {
            None
        };
        Ok(Self {
            state: Mutex::new(BackendState {
                engine,
                project: None,
                recovery_root: None,
                migration_draft: None,
                purge_draft: None,
                migration_dialog_active: false,
                windows: HashMap::new(),
            }),
            diagnostics: diagnostics::DiagnosticStore::new(),
        })
    }

    fn record_diagnostic_error(&self, code: &'static str, detail: String) -> IpcErrorV1 {
        let database_id = self.state.lock().ok().and_then(|state| {
            state
                .project
                .as_ref()
                .map(|project| project.database_id().to_string())
        });
        self.diagnostics
            .record(database_id.as_deref(), code, &detail);
        IpcErrorV1::new(code)
    }

    fn diagnostic_export_context(
        &self,
    ) -> Result<(String, Option<diagnostics::DiagnosticExportPermit>), String> {
        let database_id = self
            .state
            .lock()
            .map_err(|_| "host project state is unavailable".to_owned())?
            .project
            .as_ref()
            .map(|project| project.database_id().to_string())
            .ok_or_else(|| "no WorldDB project is open".to_owned())?;
        let snapshot =
            match self.security_policy_with_operation_id(SecurityPolicyCommand::Snapshot, None) {
                Ok(SecurityPolicyResponse::Snapshot(snapshot)) => snapshot,
                Ok(SecurityPolicyResponse::Published(_)) => {
                    return Err("security policy snapshot unavailable".to_owned());
                }
                Err(_) => return Err("security policy snapshot unavailable".to_owned()),
            };
        Ok((
            database_id,
            diagnostics::authorize_export(&snapshot.capabilities),
        ))
    }

    fn diagnostic_bundle(
        &self,
        database_id: &str,
        permit: &diagnostics::DiagnosticExportPermit,
    ) -> Result<diagnostics::DiagnosticBundle, diagnostics::DiagnosticStoreError> {
        self.diagnostics.export_bundle(database_id, permit)
    }

    fn health(&self) -> Result<Option<Response>, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        state.engine.as_ref().map(EngineBackend::health).transpose()
    }

    fn list_jobs(&self) -> Result<JobListView, String> {
        self.with_engine(EngineBackend::list_jobs)
    }

    fn cancel_job(&self, job_id: JobId) -> Result<String, String> {
        self.with_engine(|engine| engine.cancel_job(job_id))
    }

    fn project_status(&self, window_label: &str) -> Result<ProjectStatusV1, ProjectError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProjectError::HostUnavailable)?;
        let Some((root, database_id, project_revision, role_symbol, current_pointer_format)) =
            state.project.as_ref().map(|project| {
                (
                    project.canonical_root().to_owned(),
                    project.database_id(),
                    project.revision().value(),
                    project.role_symbol().to_owned(),
                    project.current_pointer_format(),
                )
            })
        else {
            return Ok(ProjectStatusV1 {
                protocol_version: IPC_PROTOCOL_VERSION,
                window: window_label.to_owned(),
                project_open: false,
                project_name: None,
                database_id: None,
                revision: None,
                role: None,
                snapshot_id: None,
                compatibility: None,
            });
        };
        if !state.windows.contains_key(window_label) {
            let snapshot_id = new_window_snapshot_id()?;
            state.windows.insert(
                window_label.to_owned(),
                WindowProjectSnapshot {
                    snapshot_id,
                    revision: project_revision,
                },
            );
        }
        let view = state
            .windows
            .get(window_label)
            .ok_or(ProjectError::HostUnavailable)?;
        let project_name = root
            .file_stem()
            .or_else(|| root.file_name())
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("WorldDB-Projekt")
            .to_owned();
        Ok(ProjectStatusV1 {
            protocol_version: IPC_PROTOCOL_VERSION,
            window: window_label.to_owned(),
            project_open: true,
            project_name: Some(project_name),
            database_id: Some(database_id.to_string()),
            revision: Some(view.revision),
            role: Some(role_symbol),
            snapshot_id: Some(view.snapshot_id.clone()),
            compatibility: Some(ProjectCompatibilityV1::from_pointer_format(
                current_pointer_format,
            )),
        })
    }

    fn create_project(
        &self,
        window_label: &str,
        root: &Path,
        principal_id: PrincipalId,
        operation_id: OperationId,
    ) -> Result<ProjectStatusV1, ProjectError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProjectError::HostUnavailable)?;
        if state.migration_dialog_active || state.migration_draft.is_some() {
            return Err(ProjectError::AlreadyOpen);
        }
        if state.project.is_some() || state.engine.is_some() {
            return Err(ProjectError::AlreadyOpen);
        }
        #[cfg(feature = "in-process")]
        let access =
            worlddb_ode_engine::create_project_with_operation_id(root, principal_id, operation_id)?;
        #[cfg(feature = "sidecar")]
        let access = {
            if root.exists() {
                return Err(ProjectError::AlreadyExists);
            }
            Sidecar::create_project(root, operation_id).map_err(|_| {
                ProjectError::UnknownCommitOutcome {
                    operation_id,
                    database_id: None,
                }
            })?;
            worlddb_ode_engine::open_project(root, principal_id)?
        };
        let database_id = access.database_id();
        let canonical_root = access.canonical_root().to_owned();
        #[cfg(feature = "in-process")]
        let engine = {
            let (engine, opened) = EngineHost::open_authorized(&canonical_root, principal_id)
                .map_err(|_| ProjectError::UnknownCommitOutcome {
                    operation_id,
                    database_id: Some(database_id),
                })?;
            if opened.database_id() != access.database_id() {
                return Err(ProjectError::UnknownCommitOutcome {
                    operation_id,
                    database_id: Some(database_id),
                });
            }
            EngineBackend::InProcess(engine)
        };
        #[cfg(feature = "sidecar")]
        let engine = EngineBackend::Sidecar(Mutex::new(
            Sidecar::start_for_project(&canonical_root).map_err(|_| {
                ProjectError::UnknownCommitOutcome {
                    operation_id,
                    database_id: Some(database_id),
                }
            })?,
        ));
        state.engine = Some(engine);
        state.project = Some(access);
        state.recovery_root = None;
        state.windows.clear();
        attach_window_snapshot(&mut state, window_label).map_err(|_| {
            ProjectError::UnknownCommitOutcome {
                operation_id,
                database_id: Some(database_id),
            }
        })?;
        self.project_status_locked(&mut state, window_label)
            .map_err(|_| ProjectError::UnknownCommitOutcome {
                operation_id,
                database_id: Some(database_id),
            })
    }

    fn open_project(
        &self,
        window_label: &str,
        root: &Path,
        principal_id: PrincipalId,
    ) -> Result<ProjectStatusV1, ProjectError> {
        let metadata = std::fs::symlink_metadata(root).map_err(|_| ProjectError::InvalidProject)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Err(ProjectError::InvalidProject);
        }
        let canonical_root =
            std::fs::canonicalize(root).map_err(|_| ProjectError::InvalidProject)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProjectError::HostUnavailable)?;
        if state.migration_dialog_active || state.migration_draft.is_some() {
            return Err(ProjectError::AlreadyOpen);
        }
        if let Some(project) = state.project.as_ref() {
            if project.canonical_root() != canonical_root || project.principal_id() != principal_id
            {
                return Err(ProjectError::AlreadyOpen);
            }
            attach_window_snapshot(&mut state, window_label)?;
            return self.project_status_locked(&mut state, window_label);
        }
        if state.engine.is_some() {
            return Err(ProjectError::AlreadyOpen);
        }
        #[cfg(feature = "in-process")]
        let (engine, access) = {
            let (engine, access) = EngineHost::open_authorized(&canonical_root, principal_id)?;
            (EngineBackend::InProcess(engine), access)
        };
        #[cfg(feature = "sidecar")]
        let (engine, access) = {
            let access = worlddb_ode_engine::open_project(&canonical_root, principal_id)?;
            let engine = Sidecar::start_for_project(&canonical_root)
                .map_err(|_| ProjectError::HostUnavailable)?;
            (EngineBackend::Sidecar(Mutex::new(engine)), access)
        };
        state.engine = Some(engine);
        state.project = Some(access);
        state.recovery_root = None;
        state.windows.clear();
        attach_window_snapshot(&mut state, window_label)?;
        self.project_status_locked(&mut state, window_label)
    }

    fn inspect_recovery(&self, root: &Path) -> Result<RecoveryReportView, String> {
        let metadata = std::fs::symlink_metadata(root)
            .map_err(|_| "recovery source is unavailable".to_owned())?;
        if !metadata.is_dir() || is_reparse_point(&metadata) {
            return Err("recovery source is unavailable".to_owned());
        }
        let canonical_root =
            std::fs::canonicalize(root).map_err(|_| "recovery source is unavailable".to_owned())?;
        {
            let state = self
                .state
                .lock()
                .map_err(|_| "host project state is poisoned".to_owned())?;
            if state.project.is_some()
                || state.engine.is_some()
                || state.migration_dialog_active
                || state.migration_draft.is_some()
            {
                return Err("another project is open".to_owned());
            }
        }
        let result = recovery_inspect_host(&canonical_root)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some()
            || state.engine.is_some()
            || state.migration_dialog_active
            || state.migration_draft.is_some()
        {
            return Err("another project is open".to_owned());
        }
        state.recovery_root = Some(canonical_root);
        Ok(result)
    }

    fn run_journaled_recovery(&self) -> Result<RecoveryApplyView, String> {
        let root = self.recovery_root()?;
        recovery_run_host(&root)
    }

    fn salvage_recovery(&self, destination: &Path) -> Result<RecoverySalvageView, String> {
        let root = self.recovery_root()?;
        recovery_salvage_host(&root, destination)
    }

    fn recovery_root(&self) -> Result<PathBuf, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some() || state.engine.is_some() || state.migration_dialog_active {
            return Err("another project is open".to_owned());
        }
        state
            .recovery_root
            .clone()
            .ok_or_else(|| "no recovery source is selected".to_owned())
    }

    fn begin_backup_operation(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some()
            || state.engine.is_some()
            || state.migration_dialog_active
            || state.migration_draft.is_some()
        {
            return Err("backup operation is unavailable".to_owned());
        }
        state.migration_dialog_active = true;
        Ok(())
    }

    fn clear_purge_draft(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        state.purge_draft = None;
        Ok(())
    }

    fn discard_purge_draft(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some() || state.engine.is_some() || state.migration_dialog_active {
            return Err("purge plan cannot be discarded during another operation".to_owned());
        }
        state.purge_draft = None;
        Ok(())
    }

    fn stage_purge_draft(&self, draft: purge::PurgeDraft) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if !state.migration_dialog_active || state.project.is_some() || state.engine.is_some() {
            return Err("purge plan staging is unavailable".to_owned());
        }
        state.purge_draft = Some(draft);
        Ok(())
    }

    fn begin_purge_execution(&self, fingerprint: &str) -> Result<purge::PurgeDraft, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some()
            || state.engine.is_some()
            || state.migration_dialog_active
            || state.migration_draft.is_some()
        {
            return Err("purge execution is unavailable".to_owned());
        }
        let draft = state
            .purge_draft
            .as_ref()
            .filter(|draft| draft.matches_fingerprint(fingerprint))
            .cloned()
            .ok_or_else(|| "purge plan is missing or no longer selected".to_owned())?;
        state.migration_dialog_active = true;
        Ok(draft)
    }

    fn begin_migration_plan_selection(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some()
            || state.engine.is_some()
            || state.migration_dialog_active
            || state
                .migration_draft
                .as_ref()
                .is_some_and(migration::MigrationDraft::run_attempted)
        {
            return Err("migration plan selection is unavailable".to_owned());
        }
        state.migration_draft = None;
        state.migration_dialog_active = true;
        Ok(())
    }

    fn finish_migration_plan_selection(
        &self,
        source: &Path,
        plan_path: &Path,
    ) -> Result<migration::MigrationPlanView, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if !state.migration_dialog_active {
            return Err("migration selection is unavailable".to_owned());
        }
        let result = if state.project.is_some() || state.engine.is_some() {
            Err("a project is open".to_owned())
        } else {
            migration::inspect_plan(source, plan_path)
        };
        state.migration_dialog_active = false;
        match result {
            Ok(draft) => {
                let view = draft.plan_view();
                state.migration_draft = Some(draft);
                Ok(view)
            }
            Err(error) => Err(error),
        }
    }

    fn begin_migration_preview(&self) -> Result<Vec<String>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some() || state.engine.is_some() || state.migration_dialog_active {
            return Err("migration preview is unavailable".to_owned());
        }
        let draft = state
            .migration_draft
            .as_ref()
            .ok_or_else(|| "no migration plan is selected".to_owned())?;
        if draft.run_attempted() {
            return Err("an attempted migration must be resumed or inspected".to_owned());
        }
        let steps = draft.step_ids();
        state.migration_dialog_active = true;
        Ok(steps)
    }

    fn finish_migration_preview(
        &self,
        selected_files: Vec<Vec<PathBuf>>,
    ) -> Result<migration::MigrationDryRunView, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if !state.migration_dialog_active {
            return Err("migration preview is unavailable".to_owned());
        }
        let result = if state.project.is_some() || state.engine.is_some() {
            Err("a project is open".to_owned())
        } else {
            state
                .migration_draft
                .as_mut()
                .ok_or_else(|| "no migration plan is selected".to_owned())?
                .stage_inputs(selected_files)
        };
        state.migration_dialog_active = false;
        result
    }

    fn begin_migration_execution(&self, resume: bool) -> Result<bool, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some() || state.engine.is_some() || state.migration_dialog_active {
            return Err("migration execution is unavailable".to_owned());
        }
        let draft = state
            .migration_draft
            .as_ref()
            .ok_or_else(|| "no migration plan is selected".to_owned())?;
        if (resume && !draft.can_resume()) || (!resume && !draft.can_execute()) {
            return Err("migration is not ready for this action".to_owned());
        }
        let breaking = draft.is_breaking();
        state.migration_dialog_active = true;
        Ok(breaking)
    }

    fn finish_migration_execution(
        &self,
        confirmed_breaking: bool,
        backup_parent: Option<PathBuf>,
        restore_parent: Option<PathBuf>,
    ) -> Result<migration::MigrationRunView, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if !state.migration_dialog_active {
            return Err("migration execution is unavailable".to_owned());
        }
        let result = if state.project.is_some() || state.engine.is_some() {
            Err("a project is open".to_owned())
        } else if let Some(draft) = state.migration_draft.as_mut() {
            draft.execute(confirmed_breaking, backup_parent, restore_parent)
        } else {
            Err("no migration plan is selected".to_owned())
        };
        state.migration_dialog_active = false;
        if result.is_ok() {
            state.migration_draft = None;
        }
        result
    }

    fn finish_migration_resume(
        &self,
        confirmed_breaking: bool,
        restore_parent: Option<PathBuf>,
    ) -> Result<migration::MigrationRunView, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if !state.migration_dialog_active {
            return Err("migration resume is unavailable".to_owned());
        }
        let result = if state.project.is_some() || state.engine.is_some() {
            Err("a project is open".to_owned())
        } else if let Some(draft) = state.migration_draft.as_mut() {
            draft.resume(confirmed_breaking, restore_parent)
        } else {
            Err("no migration plan is selected".to_owned())
        };
        state.migration_dialog_active = false;
        if result.is_ok() {
            state.migration_draft = None;
        }
        result
    }

    fn cancel_migration_dialog(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.migration_dialog_active = false;
        }
    }

    fn cancel_migration(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.migration_dialog_active
            || state
                .migration_draft
                .as_ref()
                .is_some_and(migration::MigrationDraft::run_attempted)
        {
            return Err("migration state cannot be discarded yet".to_owned());
        }
        state.migration_draft = None;
        Ok(())
    }

    fn migration_state(&self) -> Result<Option<migration::MigrationPanelState>, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        Ok(state
            .migration_draft
            .as_ref()
            .map(migration::MigrationDraft::panel_state))
    }

    fn set_migration_omissions(
        &self,
        record_indexes: Vec<u64>,
    ) -> Result<migration::MigrationPanelState, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        if state.project.is_some() || state.engine.is_some() || state.migration_dialog_active {
            return Err("migration decisions are unavailable".to_owned());
        }
        let draft = state
            .migration_draft
            .as_mut()
            .ok_or_else(|| "no migration plan is selected".to_owned())?;
        draft.set_omissions(record_indexes)?;
        Ok(draft.panel_state())
    }

    fn close_project(&self) -> Result<Option<JobShutdownView>, ProjectError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProjectError::HostUnavailable)?;
        if state.project.is_some() {
            let shutdown = state
                .engine
                .as_mut()
                .ok_or(ProjectError::HostUnavailable)?
                .shutdown()
                .map_err(|_| ProjectError::HostUnavailable)?;
            if !shutdown.drained {
                return Ok(Some(shutdown));
            }
            state.windows.clear();
            state.project = None;
            state.engine = None;
            return Ok(Some(shutdown));
        }
        Ok(None)
    }

    fn schema_with_operation_id(
        &self,
        command: SchemaCommand,
        operation_id: Option<OperationId>,
    ) -> Result<SchemaResponse, String> {
        self.with_engine(|engine| engine.schema(command, operation_id))
    }

    fn entities_with_operation_id(
        &self,
        command: EntityCommand,
        operation_id: Option<OperationId>,
    ) -> Result<EntityResponse, String> {
        self.with_engine(|engine| engine.entities(command, operation_id))
    }

    fn branch_layers_with_operation_id(
        &self,
        command: BranchLayerCommand,
        operation_id: Option<OperationId>,
    ) -> Result<BranchLayerResponse, String> {
        self.with_engine(|engine| engine.branch_layers(command, operation_id))
    }

    fn history_space_transfer_with_operation_id(
        &self,
        command: HistorySpaceTransferCommand,
        operation_id: Option<OperationId>,
    ) -> Result<HistorySpaceTransferResponse, String> {
        self.with_engine(|engine| engine.history_space_transfer(command, operation_id))
    }

    fn facts_with_operation_id(
        &self,
        command: FactCommand,
        operation_id: Option<OperationId>,
    ) -> Result<FactResponse, String> {
        self.with_engine(|engine| engine.facts(command, operation_id))
    }

    fn perspectives_with_operation_id(
        &self,
        command: PerspectiveCommand,
        operation_id: Option<OperationId>,
    ) -> Result<PerspectiveResponse, String> {
        self.with_engine(|engine| engine.perspectives(command, operation_id))
    }

    fn security_policy_with_operation_id(
        &self,
        command: SecurityPolicyCommand,
        operation_id: Option<OperationId>,
    ) -> Result<SecurityPolicyResponse, String> {
        self.with_engine(|engine| engine.security_policy(command, operation_id))
    }

    fn project_status_locked(
        &self,
        state: &mut BackendState,
        window_label: &str,
    ) -> Result<ProjectStatusV1, ProjectError> {
        if !state.windows.contains_key(window_label) {
            attach_window_snapshot(state, window_label)?;
        }
        let (root, database_id, role_symbol, current_pointer_format) = state
            .project
            .as_ref()
            .map(|project| {
                (
                    project.canonical_root().to_owned(),
                    project.database_id(),
                    project.role_symbol().to_owned(),
                    project.current_pointer_format(),
                )
            })
            .ok_or(ProjectError::HostUnavailable)?;
        let view = state
            .windows
            .get(window_label)
            .map(|view| (view.revision, view.snapshot_id.clone()))
            .ok_or(ProjectError::HostUnavailable)?;
        let project_name = root
            .file_stem()
            .or_else(|| root.file_name())
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("WorldDB-Projekt")
            .to_owned();
        Ok(ProjectStatusV1 {
            protocol_version: IPC_PROTOCOL_VERSION,
            window: window_label.to_owned(),
            project_open: true,
            project_name: Some(project_name),
            database_id: Some(database_id.to_string()),
            revision: Some(view.0),
            role: Some(role_symbol),
            snapshot_id: Some(view.1),
            compatibility: Some(ProjectCompatibilityV1::from_pointer_format(
                current_pointer_format,
            )),
        })
    }

    fn begin_stream(
        &self,
        transfer_id: &str,
        total_bytes: u64,
        chunk_bytes: u32,
    ) -> Result<(), String> {
        self.with_engine(|engine| engine.begin_stream(transfer_id, total_bytes, chunk_bytes))
    }

    fn push_stream_chunk(
        &self,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
    ) -> Result<u64, String> {
        self.with_engine(|engine| engine.push_stream_chunk(transfer_id, sequence, chunk))
    }

    fn finish_stream(
        &self,
        transfer_id: &str,
        cancelled: bool,
    ) -> Result<worlddb_ode_engine::StreamReport, String> {
        self.with_engine(|engine| engine.finish_stream(transfer_id, cancelled))
    }

    fn stream(&self, plan: StreamPlan) -> Result<serde_json::Value, String> {
        self.with_engine(|engine| engine.stream(plan))
    }

    fn exercise_engine_panic(&self) -> Result<serde_json::Value, String> {
        self.with_engine(EngineBackend::exercise_engine_panic)
    }

    fn exercise_engine_update(&self) -> Result<serde_json::Value, String> {
        self.with_engine(EngineBackend::exercise_engine_update)
    }

    fn with_engine<T>(
        &self,
        operation: impl FnOnce(&EngineBackend) -> Result<T, String>,
    ) -> Result<T, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "host project state is poisoned".to_owned())?;
        let engine = state
            .engine
            .as_ref()
            .ok_or_else(|| "no WorldDB project is open".to_owned())?;
        operation(engine)
    }
}

fn attach_window_snapshot(
    state: &mut BackendState,
    window_label: &str,
) -> Result<(), ProjectError> {
    let revision = state
        .project
        .as_ref()
        .ok_or(ProjectError::HostUnavailable)?
        .revision()
        .value();
    if !state.windows.contains_key(window_label) {
        state.windows.insert(
            window_label.to_owned(),
            WindowProjectSnapshot {
                snapshot_id: new_window_snapshot_id()?,
                revision,
            },
        );
    }
    Ok(())
}

fn new_window_snapshot_id() -> Result<String, ProjectError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| ProjectError::HostUnavailable)?;
    let mut id = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut id, "{byte:02x}").map_err(|_| ProjectError::HostUnavailable)?;
    }
    Ok(id)
}

fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn map_project_error(error: worlddb_ode_engine::ProjectError) -> IpcErrorV1 {
    use worlddb_ode_engine::ProjectError;
    match error {
        ProjectError::AlreadyExists => IpcErrorV1::new("project_already_exists"),
        ProjectError::AlreadyOpen => IpcErrorV1::new("project_already_open"),
        ProjectError::AccessDenied => IpcErrorV1::new("project_unavailable"),
        ProjectError::InvalidProject => IpcErrorV1::new("invalid_project"),
        ProjectError::RecoveryRequired => IpcErrorV1::new("recovery_required"),
        ProjectError::UnsupportedIdentity | ProjectError::HostUnavailable => {
            IpcErrorV1::new("host_unavailable")
        }
        ProjectError::UnknownCommitOutcome {
            operation_id,
            database_id,
        } => IpcErrorV1::unknown_commit(operation_id, database_id.map(|id| id.to_string())),
    }
}

#[cfg(feature = "in-process")]
fn recovery_inspect_host(root: &Path) -> Result<RecoveryReportView, String> {
    worlddb_ode_engine::inspect_recovery(root).map_err(|_| "recovery inspection failed".to_owned())
}

#[cfg(feature = "in-process")]
fn recovery_run_host(root: &Path) -> Result<RecoveryApplyView, String> {
    worlddb_ode_engine::run_journaled_recovery(root)
        .map_err(|_| "journaled recovery failed".to_owned())
}

#[cfg(feature = "in-process")]
fn recovery_salvage_host(root: &Path, destination: &Path) -> Result<RecoverySalvageView, String> {
    worlddb_ode_engine::salvage_recovery(root, destination).map_err(|_| "salvage failed".to_owned())
}

#[cfg(feature = "sidecar")]
fn recovery_sidecar_request(root: &Path, request: Request) -> Result<Response, String> {
    let mut sidecar = Sidecar::start_for_recovery(root)?;
    let response = sidecar.request(request);
    let stopped = sidecar.shutdown_child();
    match response {
        Ok(response) => {
            stopped?;
            Ok(response)
        }
        Err(error) => {
            let _ = stopped;
            Err(error)
        }
    }
}

#[cfg(feature = "sidecar")]
fn recovery_inspect_host(root: &Path) -> Result<RecoveryReportView, String> {
    match recovery_sidecar_request(root, Request::RecoveryInspect)? {
        Response::RecoveryReport { result } => Ok(result),
        Response::Error { .. } => Err("recovery inspection failed".to_owned()),
        _ => Err("sidecar returned an unexpected recovery report".to_owned()),
    }
}

#[cfg(feature = "sidecar")]
fn recovery_run_host(root: &Path) -> Result<RecoveryApplyView, String> {
    match recovery_sidecar_request(root, Request::RecoveryRun)? {
        Response::RecoveryApplied { result } => Ok(result),
        Response::Error { .. } => Err("journaled recovery failed".to_owned()),
        _ => Err("sidecar returned an unexpected recovery result".to_owned()),
    }
}

#[cfg(feature = "sidecar")]
fn recovery_salvage_host(root: &Path, destination: &Path) -> Result<RecoverySalvageView, String> {
    match recovery_sidecar_request(
        root,
        Request::RecoverySalvage {
            destination: destination.to_owned(),
        },
    )? {
        Response::RecoverySalvaged { result } => Ok(result),
        Response::Error { .. } => Err("salvage failed".to_owned()),
        _ => Err("sidecar returned an unexpected salvage result".to_owned()),
    }
}

#[cfg(feature = "in-process")]
enum EngineBackend {
    InProcess(EngineHost),
}

#[cfg(feature = "sidecar")]
enum EngineBackend {
    Sidecar(Mutex<Sidecar>),
}

impl EngineBackend {
    fn schema(
        &self,
        command: SchemaCommand,
        operation_id: Option<OperationId>,
    ) -> Result<SchemaResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.schema(command)
                        })
                    }
                    None => engine.schema(command),
                };
                result.map_err(|_| "engine rejected schema operation".to_owned())
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .schema(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn entities(
        &self,
        command: EntityCommand,
        operation_id: Option<OperationId>,
    ) -> Result<EntityResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.entities(command)
                        })
                    }
                    None => engine.entities(command),
                };
                result.map_err(|_| "engine rejected Entity operation".to_owned())
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .entities(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn branch_layers(
        &self,
        command: BranchLayerCommand,
        operation_id: Option<OperationId>,
    ) -> Result<BranchLayerResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.branch_layers(command)
                        })
                    }
                    None => engine.branch_layers(command),
                };
                result.map_err(|_| "engine rejected branch or Layer operation".to_owned())
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .branch_layers(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn history_space_transfer(
        &self,
        command: HistorySpaceTransferCommand,
        operation_id: Option<OperationId>,
    ) -> Result<HistorySpaceTransferResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.history_space_transfer(command)
                        })
                    }
                    None => engine.history_space_transfer(command),
                };
                result.map_err(|_| "engine rejected HistorySpace transfer".to_owned())
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .history_space_transfer(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn facts(
        &self,
        command: FactCommand,
        operation_id: Option<OperationId>,
    ) -> Result<FactResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.facts(command)
                        })
                    }
                    None => engine.facts(command),
                };
                result.map_err(|error| format!("engine rejected factual-record operation: {error}"))
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .facts(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn perspectives(
        &self,
        command: PerspectiveCommand,
        operation_id: Option<OperationId>,
    ) -> Result<PerspectiveResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.perspectives(command)
                        })
                    }
                    None => engine.perspectives(command),
                };
                result.map_err(|_| "engine rejected Perspective operation".to_owned())
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .perspectives(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn security_policy(
        &self,
        command: SecurityPolicyCommand,
        operation_id: Option<OperationId>,
    ) -> Result<SecurityPolicyResponse, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let result = match operation_id {
                    Some(operation_id) => {
                        worlddb_ode_engine::with_operation_id(operation_id, || {
                            engine.security_policy(command)
                        })
                    }
                    None => engine.security_policy(command),
                };
                result.map_err(|_| "engine rejected security policy operation".to_owned())
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .security_policy(command, operation_id.map(|id| id.to_string())),
        }
    }

    fn health(&self) -> Result<Response, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine.health().map_err(|error| error.to_string()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .health(),
        }
    }

    fn list_jobs(&self) -> Result<JobListView, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine
                .list_jobs()
                .map_err(|_| "job state unavailable".to_owned()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .list_jobs(),
        }
    }

    fn cancel_job(&self, job_id: JobId) -> Result<String, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine
                .cancel_job(job_id)
                .map(cancel_disposition_code)
                .map(str::to_owned)
                .map_err(|_| "job cancellation unavailable".to_owned()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .cancel_job(job_id.to_string()),
        }
    }

    fn shutdown(&mut self) -> Result<JobShutdownView, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine
                .shutdown_jobs(Instant::now() + Duration::from_secs(2))
                .map_err(|_| "job shutdown unavailable".to_owned()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .get_mut()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .shutdown_child(),
        }
    }

    fn begin_stream(
        &self,
        transfer_id: &str,
        total_bytes: u64,
        chunk_bytes: u32,
    ) -> Result<(), String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine
                .begin_stream(transfer_id, total_bytes, chunk_bytes)
                .map_err(|_| "engine rejected transfer".to_owned()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .begin_stream(transfer_id, total_bytes, chunk_bytes),
        }
    }

    fn push_stream_chunk(
        &self,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
    ) -> Result<u64, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine
                .push_stream_chunk(transfer_id, sequence, chunk)
                .map_err(|_| "engine rejected transfer".to_owned()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .push_stream_chunk(transfer_id, sequence, chunk),
        }
    }

    fn finish_stream(
        &self,
        transfer_id: &str,
        cancelled: bool,
    ) -> Result<worlddb_ode_engine::StreamReport, String> {
        match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => engine
                .finish_stream(transfer_id, cancelled)
                .map_err(|_| "engine rejected transfer".to_owned()),
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .finish_stream(transfer_id, cancelled),
        }
    }

    fn stream(&self, plan: StreamPlan) -> Result<serde_json::Value, String> {
        let plan = plan.validate().map_err(|error| error.to_string())?;
        let started = Instant::now();
        let response = match self {
            #[cfg(feature = "in-process")]
            Self::InProcess(engine) => {
                let report = engine
                    .stream_synthetic(plan)
                    .map_err(|error| error.to_string())?;
                Response::StreamComplete {
                    bytes_read: report.bytes_read,
                    digest: report.digest,
                    cancelled: report.cancelled,
                }
            }
            #[cfg(feature = "sidecar")]
            Self::Sidecar(engine) => engine
                .lock()
                .map_err(|_| "sidecar lock failed".to_owned())?
                .stream(plan)?,
        };
        match response {
            Response::StreamComplete {
                bytes_read,
                digest,
                cancelled,
            } => Ok(serde_json::json!({
                "bytes_read": bytes_read,
                "digest": digest,
                "cancelled": cancelled,
                "elapsed_microseconds": started.elapsed().as_micros(),
            })),
            Response::Error { code } => Err(format!("engine rejected stream: {code}")),
            _ => Err("engine returned an unexpected stream response".to_owned()),
        }
    }

    #[cfg(feature = "in-process")]
    fn exercise_engine_panic(&self) -> Result<serde_json::Value, String> {
        let Self::InProcess(engine) = self;
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.panic_for_spike();
        }));
        let poisoned = engine.health().is_err();
        if panic_result.is_ok() || !poisoned {
            return Err("in-process panic did not poison the engine state".to_owned());
        }
        Ok(serde_json::json!({
            "engine_process_survived": true,
            "poison_detected": true,
            "application_restart_required": true,
            "result": "host catches panic; poisoned in-process engine requires full app restart",
        }))
    }

    #[cfg(feature = "sidecar")]
    fn exercise_engine_panic(&self) -> Result<serde_json::Value, String> {
        let Self::Sidecar(engine) = self;
        engine
            .lock()
            .map_err(|_| "sidecar lock failed".to_owned())?
            .panic_and_restart()
    }

    #[cfg(feature = "in-process")]
    fn exercise_engine_update(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "application_build_id": env!("WORLDDB_ODE_APP_BUILD_ID"),
            "application_restart_required": true,
            "result": "in-process engine update is coupled to the desktop application update",
        }))
    }

    #[cfg(feature = "sidecar")]
    fn exercise_engine_update(&self) -> Result<serde_json::Value, String> {
        let Self::Sidecar(engine) = self;
        let update_executable = std::env::var_os("WORLDDB_ODE_ENGINE_UPDATE")
            .map(PathBuf::from)
            .ok_or("WORLDDB_ODE_ENGINE_UPDATE must point to the staged updated sidecar")?;
        engine
            .lock()
            .map_err(|_| "sidecar lock failed".to_owned())?
            .update_to(&update_executable)
    }
}

#[cfg(feature = "sidecar")]
struct Sidecar {
    child: std::process::Child,
    input: std::process::ChildStdin,
    responses: std::sync::mpsc::Receiver<Result<Response, String>>,
    response_reader: Option<std::thread::JoinHandle<()>>,
    database_root: PathBuf,
    executable: PathBuf,
    host_authenticated: bool,
    recovery_mode: bool,
    engine_process_id: u32,
    engine_build_id: String,
}

#[cfg(feature = "sidecar")]
fn sidecar_executable() -> Result<PathBuf, String> {
    match std::env::var_os("WORLDDB_ODE_ENGINE_EXECUTABLE") {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(std::env::current_exe()
            .map_err(|_| "application executable path unavailable".to_owned())?
            .with_file_name(if cfg!(windows) {
                "worlddb_ode_engine.exe"
            } else {
                "worlddb_ode_engine"
            })),
    }
}

#[cfg(feature = "sidecar")]
impl Sidecar {
    fn start(database_root: &Path) -> Result<Self, String> {
        let executable = sidecar_executable()?;
        Self::start_with(database_root, &executable)
    }

    fn create_project(database_root: &Path, operation_id: OperationId) -> Result<(), String> {
        use std::process::{Command, Stdio};

        let executable = sidecar_executable()?;
        let status = Command::new(executable)
            .arg(database_root)
            .arg("--host-account")
            .arg("--bootstrap-project")
            .arg(operation_id.to_string())
            .env_remove("WORLDDB_ODE_ENGINE_PRINCIPAL_ID")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "engine sidecar could not bootstrap the project".to_owned())?;
        if status.success() {
            Ok(())
        } else {
            Err("engine sidecar project bootstrap failed".to_owned())
        }
    }

    fn start_for_project(database_root: &Path) -> Result<Self, String> {
        Self::start_with_authentication(database_root, &sidecar_executable()?, true, false)
    }

    fn start_for_recovery(database_root: &Path) -> Result<Self, String> {
        Self::start_with_authentication(database_root, &sidecar_executable()?, true, true)
    }

    fn start_with(database_root: &Path, executable: &Path) -> Result<Self, String> {
        Self::start_with_authentication(database_root, executable, false, false)
    }

    fn start_with_authentication(
        database_root: &Path,
        executable: &Path,
        host_authenticated: bool,
        recovery_only: bool,
    ) -> Result<Self, String> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let mut command = Command::new(executable);
        command
            .arg(database_root)
            .env_remove("WORLDDB_ODE_ENGINE_PRINCIPAL_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if host_authenticated {
            command.arg("--host-account");
        }
        if recovery_only {
            command.arg("--recovery-only");
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("engine sidecar could not start: {error}"))?;
        let input = child.stdin.take().ok_or("sidecar input unavailable")?;
        let output = child.stdout.take().ok_or("sidecar output unavailable")?;
        let (response_sender, responses) = std::sync::mpsc::channel();
        let response_reader = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut line = String::new();
                match output.read_line(&mut line) {
                    Ok(0) => {
                        let _ = response_sender
                            .send(Err("sidecar closed its response pipe".to_owned()));
                        return;
                    }
                    Ok(_) => {
                        let response = serde_json::from_str::<Response>(&line)
                            .map_err(|_| "sidecar response was invalid".to_owned());
                        if response_sender.send(response).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = response_sender
                            .send(Err("sidecar response could not be read".to_owned()));
                        return;
                    }
                }
            }
        });
        let mut sidecar = Self {
            child,
            input,
            responses,
            response_reader: Some(response_reader),
            database_root: database_root.to_owned(),
            executable: executable.to_owned(),
            host_authenticated,
            recovery_mode: recovery_only,
            engine_process_id: 0,
            engine_build_id: String::new(),
        };
        let ready = sidecar.read_response()?;
        let Response::Ready {
            engine_process_id,
            engine_build_id,
            protocol_version: 1,
        } = ready
        else {
            sidecar.stop_after_transport_failure();
            return Err("sidecar protocol handshake was not compatible".to_owned());
        };
        sidecar.engine_process_id = engine_process_id;
        sidecar.engine_build_id = engine_build_id;
        Ok(sidecar)
    }

    fn health(&mut self) -> Result<Response, String> {
        self.request(Request::Health)
    }

    fn list_jobs(&mut self) -> Result<JobListView, String> {
        match self.request(Request::JobsList)? {
            Response::Jobs { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected job status request".to_owned()),
            _ => Err("sidecar returned an unexpected job status response".to_owned()),
        }
    }

    fn cancel_job(&mut self, job_id: String) -> Result<String, String> {
        match self.request(Request::JobCancel { job_id })? {
            Response::JobCancel { disposition, .. } => Ok(disposition),
            Response::Error { .. } => Err("sidecar rejected job cancellation".to_owned()),
            _ => Err("sidecar returned an unexpected job cancellation response".to_owned()),
        }
    }

    fn schema(
        &mut self,
        command: SchemaCommand,
        operation_id: Option<String>,
    ) -> Result<SchemaResponse, String> {
        match self.request(Request::Schema {
            operation_id,
            command,
        })? {
            Response::Schema { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected schema operation".to_owned()),
            _ => Err("sidecar returned an unexpected schema response".to_owned()),
        }
    }

    fn entities(
        &mut self,
        command: EntityCommand,
        operation_id: Option<String>,
    ) -> Result<EntityResponse, String> {
        match self.request(Request::Entities {
            operation_id,
            command,
        })? {
            Response::Entities { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected Entity operation".to_owned()),
            _ => Err("sidecar returned an unexpected Entity response".to_owned()),
        }
    }

    fn branch_layers(
        &mut self,
        command: BranchLayerCommand,
        operation_id: Option<String>,
    ) -> Result<BranchLayerResponse, String> {
        match self.request(Request::BranchLayers {
            operation_id,
            command,
        })? {
            Response::BranchLayers { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected branch or Layer operation".to_owned()),
            _ => Err("sidecar returned an unexpected branch/Layer response".to_owned()),
        }
    }

    fn history_space_transfer(
        &mut self,
        command: HistorySpaceTransferCommand,
        operation_id: Option<String>,
    ) -> Result<HistorySpaceTransferResponse, String> {
        match self.request(Request::HistorySpaceTransfer {
            operation_id,
            command,
        })? {
            Response::HistorySpaceTransfer { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected HistorySpace transfer".to_owned()),
            _ => Err("sidecar returned an unexpected transfer response".to_owned()),
        }
    }

    fn facts(
        &mut self,
        command: FactCommand,
        operation_id: Option<String>,
    ) -> Result<FactResponse, String> {
        match self.request(Request::Facts {
            operation_id,
            command: Box::new(command),
        })? {
            Response::Facts { result } => Ok(*result),
            Response::Error { .. } => Err("sidecar rejected factual-record operation".to_owned()),
            _ => Err("sidecar returned an unexpected factual-record response".to_owned()),
        }
    }

    fn perspectives(
        &mut self,
        command: PerspectiveCommand,
        operation_id: Option<String>,
    ) -> Result<PerspectiveResponse, String> {
        match self.request(Request::Perspectives {
            operation_id,
            command,
        })? {
            Response::Perspectives { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected Perspective operation".to_owned()),
            _ => Err("sidecar returned an unexpected Perspective response".to_owned()),
        }
    }

    fn security_policy(
        &mut self,
        command: SecurityPolicyCommand,
        operation_id: Option<String>,
    ) -> Result<SecurityPolicyResponse, String> {
        match self.request(Request::SecurityPolicy {
            operation_id,
            command,
        })? {
            Response::SecurityPolicy { result } => Ok(result),
            Response::Error { .. } => Err("sidecar rejected security policy operation".to_owned()),
            _ => Err("sidecar returned an unexpected security policy response".to_owned()),
        }
    }

    fn request(&mut self, request: Request) -> Result<Response, String> {
        self.ensure_running()?;
        self.request_without_recovery(request)
    }

    fn request_without_recovery(&mut self, request: Request) -> Result<Response, String> {
        if send_request(&mut self.input, &request).is_err() {
            self.stop_after_transport_failure();
            return Err("sidecar request could not be sent".to_owned());
        }
        self.read_response()
    }

    fn read_response(&mut self) -> Result<Response, String> {
        match self.responses.recv_timeout(SIDECAR_REQUEST_TIMEOUT) {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.stop_after_transport_failure();
                Err("sidecar is unavailable".to_owned())
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.stop_after_transport_failure();
                Err("sidecar response timed out".to_owned())
            }
        }
    }

    fn stop_after_transport_failure(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        self.join_response_reader();
    }

    fn join_response_reader(&mut self) {
        if let Some(response_reader) = self.response_reader.take() {
            let _ = response_reader.join();
        }
    }

    fn ensure_running(&mut self) -> Result<(), String> {
        if self
            .child
            .try_wait()
            .map_err(|_| "sidecar process status could not be read".to_owned())?
            .is_none()
        {
            return Ok(());
        }
        self.join_response_reader();
        let mut replacement = Self::start_with_authentication(
            &self.database_root,
            &self.executable,
            self.host_authenticated,
            self.recovery_mode,
        )?;
        let response = replacement.request_without_recovery(Request::Health)?;
        if !matches!(
            response,
            Response::Health {
                writer_owned: true,
                ..
            }
        ) {
            replacement.stop_after_transport_failure();
            return Err("replacement sidecar could not acquire the writer lock".to_owned());
        }
        *self = replacement;
        Ok(())
    }

    fn begin_stream(
        &mut self,
        transfer_id: &str,
        total_bytes: u64,
        chunk_bytes: u32,
    ) -> Result<(), String> {
        match self.request(Request::StreamStart {
            transfer_id: transfer_id.to_owned(),
            total_bytes,
            chunk_bytes,
        })? {
            Response::StreamStarted { transfer_id: id } if id == transfer_id => Ok(()),
            Response::Error { .. } => Err("sidecar rejected transfer".to_owned()),
            _ => Err("sidecar returned an unexpected transfer response".to_owned()),
        }
    }

    fn push_stream_chunk(
        &mut self,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
    ) -> Result<u64, String> {
        use std::io::Write;

        let chunk_bytes =
            u32::try_from(chunk.len()).map_err(|_| "sidecar rejected transfer chunk".to_owned())?;
        self.ensure_running()?;
        if send_request(
            &mut self.input,
            &Request::StreamChunk {
                transfer_id: transfer_id.to_owned(),
                sequence,
                chunk_bytes,
            },
        )
        .is_err()
        {
            self.stop_after_transport_failure();
            return Err("sidecar request could not be sent".to_owned());
        }
        if write_frame(&mut self.input, FRAME_DATA, chunk)
            .and_then(|()| self.input.flush())
            .is_err()
        {
            self.stop_after_transport_failure();
            return Err("sidecar stream chunk write failed".to_owned());
        }
        match self.read_response()? {
            Response::StreamChunkAccepted {
                transfer_id: id,
                sequence: acknowledged_sequence,
                bytes_received,
            } if id == transfer_id && acknowledged_sequence == sequence => Ok(bytes_received),
            Response::Error { .. } => Err("sidecar rejected transfer chunk".to_owned()),
            _ => Err("sidecar returned an unexpected chunk response".to_owned()),
        }
    }

    fn finish_stream(
        &mut self,
        transfer_id: &str,
        cancelled: bool,
    ) -> Result<worlddb_ode_engine::StreamReport, String> {
        match self.request(Request::StreamFinish {
            transfer_id: transfer_id.to_owned(),
            cancelled,
        })? {
            Response::StreamComplete {
                bytes_read,
                digest,
                cancelled,
            } => Ok(worlddb_ode_engine::StreamReport {
                bytes_read,
                digest,
                cancelled,
            }),
            Response::Error { .. } => Err("sidecar rejected transfer completion".to_owned()),
            _ => Err("sidecar returned an unexpected completion response".to_owned()),
        }
    }

    fn stream(&mut self, plan: StreamPlan) -> Result<Response, String> {
        use std::io::Write;

        let plan = plan.validate().map_err(|error| error.to_string())?;
        self.ensure_running()?;
        if send_request(
            &mut self.input,
            &Request::StreamSink {
                total_bytes: plan.total_bytes,
                chunk_bytes: plan.chunk_bytes,
                cancel_after_bytes: plan.cancel_after_bytes,
            },
        )
        .is_err()
        {
            self.stop_after_transport_failure();
            return Err("sidecar stream request could not be sent".to_owned());
        }

        let mut offset = 0;
        while offset < plan.target_bytes() {
            let length = (plan.target_bytes() - offset).min(plan.chunk_bytes as u64) as usize;
            let mut chunk = vec![0; length];
            fill_deterministic_chunk(&mut chunk, offset);
            if write_frame(&mut self.input, FRAME_DATA, &chunk).is_err() {
                self.stop_after_transport_failure();
                return Err("sidecar stream frame write failed".to_owned());
            }
            offset += length as u64;
        }
        let terminal_frame = if plan.cancel_after_bytes.is_some() {
            FRAME_CANCEL
        } else {
            FRAME_END
        };
        if write_frame(&mut self.input, terminal_frame, &[])
            .and_then(|()| self.input.flush())
            .is_err()
        {
            self.stop_after_transport_failure();
            return Err("sidecar stream completion write failed".to_owned());
        }
        self.read_response()
    }

    fn panic_and_restart(&mut self) -> Result<serde_json::Value, String> {
        let previous_process_id = self.engine_process_id;
        let previous_build_id = self.engine_build_id.clone();
        send_request(&mut self.input, &Request::Panic)?;
        let status = wait_for_child(&mut self.child)?;
        self.join_response_reader();
        if status.success() {
            return Err("injected sidecar panic exited successfully".to_owned());
        }
        let mut replacement = Self::start_with_authentication(
            &self.database_root,
            &self.executable,
            self.host_authenticated,
            self.recovery_mode,
        )?;
        let replacement_health = replacement.health_for_validation()?;
        let replacement_process_id = replacement.engine_process_id;
        let replacement_build_id = replacement.engine_build_id.clone();
        *self = replacement;
        if !matches!(
            replacement_health,
            Response::Health {
                writer_owned: true,
                ..
            }
        ) {
            return Err("replacement sidecar did not reacquire the writer lock".to_owned());
        }
        if previous_process_id == replacement_process_id {
            return Err("sidecar process id did not change after panic".to_owned());
        }
        Ok(serde_json::json!({
            "application_process_survived": true,
            "previous_engine_process_id": previous_process_id,
            "replacement_engine_process_id": replacement_process_id,
            "previous_engine_build_id": previous_build_id,
            "replacement_engine_build_id": replacement_build_id,
            "writer_lock_reacquired": true,
            "application_restart_required": false,
        }))
    }

    fn update_to(&mut self, updated_executable: &Path) -> Result<serde_json::Value, String> {
        let previous_build_id = self.engine_build_id.clone();
        let previous_process_id = self.engine_process_id;
        self.shutdown_child()?;
        let mut replacement = Self::start_with_authentication(
            &self.database_root,
            updated_executable,
            self.host_authenticated,
            self.recovery_mode,
        )?;
        let replacement_health = replacement.health_for_validation()?;
        let replacement_build_id = replacement.engine_build_id.clone();
        let replacement_process_id = replacement.engine_process_id;
        if previous_build_id == replacement_build_id {
            return Err("updated sidecar has the same engine build id".to_owned());
        }
        if !matches!(
            replacement_health,
            Response::Health {
                writer_owned: true,
                ..
            }
        ) {
            return Err("updated sidecar did not reacquire the writer lock".to_owned());
        }
        *self = replacement;
        Ok(serde_json::json!({
            "application_process_id": std::process::id(),
            "previous_engine_process_id": previous_process_id,
            "updated_engine_process_id": replacement_process_id,
            "previous_engine_build_id": previous_build_id,
            "updated_engine_build_id": replacement_build_id,
            "writer_lock_reacquired": true,
            "application_restart_required": false,
        }))
    }

    fn health_for_validation(&mut self) -> Result<Response, String> {
        self.request(Request::Health)
    }

    fn shutdown_child(&mut self) -> Result<JobShutdownView, String> {
        let result = match self.request_without_recovery(Request::Shutdown)? {
            Response::Shutdown { result } => result,
            _ => {
                self.stop_after_transport_failure();
                return Err("sidecar did not acknowledge shutdown".to_owned());
            }
        };
        if !result.drained {
            return Ok(result);
        }
        let status = wait_for_child(&mut self.child)?;
        self.join_response_reader();
        if !status.success() {
            return Err("sidecar did not shut down cleanly".to_owned());
        }
        Ok(result)
    }
}

#[cfg(feature = "sidecar")]
impl Drop for Sidecar {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            self.join_response_reader();
            return;
        }
        let _ = send_request(&mut self.input, &Request::Shutdown);
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                self.join_response_reader();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_response_reader();
    }
}

#[cfg(feature = "sidecar")]
fn send_request(input: &mut std::process::ChildStdin, request: &Request) -> Result<(), String> {
    use std::io::Write;

    serde_json::to_writer(&mut *input, request)
        .map_err(|_| "sidecar request encoding failed".to_owned())?;
    input
        .write_all(b"\n")
        .and_then(|()| input.flush())
        .map_err(|_| "sidecar request write failed".to_owned())
}

#[cfg(feature = "sidecar")]
fn wait_for_child(child: &mut std::process::Child) -> Result<std::process::ExitStatus, String> {
    for _ in 0..100 {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "sidecar process status could not be read".to_owned())?
        {
            return Ok(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let _ = child.kill();
    child
        .wait()
        .map_err(|_| "sidecar did not stop after the process timeout".to_owned())
}

#[cfg(feature = "sidecar")]
fn write_frame<W: std::io::Write>(writer: &mut W, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    writer.write_all(&[kind])?;
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)
}

#[cfg(all(test, feature = "sidecar"))]
mod sidecar_recovery_tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{Request, Response, Sidecar};

    static NEXT_TEST_DATABASE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn hung_sidecar_request_times_out_then_reconnects_and_reacquires_writer_lock() {
        let database_root = std::env::temp_dir().join(format!(
            "worlddb-ode-sidecar-timeout-{}-{}",
            std::process::id(),
            NEXT_TEST_DATABASE.fetch_add(1, Ordering::Relaxed)
        ));
        let current_test_executable = std::env::current_exe().expect("test executable path");
        let engine_executable = current_test_executable
            .parent()
            .and_then(std::path::Path::parent)
            .expect("Cargo target directory")
            .join(if cfg!(windows) {
                "worlddb_ode_engine.exe"
            } else {
                "worlddb_ode_engine"
            });
        let mut sidecar =
            Sidecar::start_with(&database_root, &engine_executable).expect("sidecar starts");
        let previous_process_id = sidecar.engine_process_id;
        let transfer_id = "00000000000000000000000000000001";
        sidecar
            .begin_stream(transfer_id, 10, 4)
            .expect("stream starts");

        let timed_out = sidecar.request(Request::StreamChunk {
            transfer_id: transfer_id.to_owned(),
            sequence: 0,
            chunk_bytes: 4,
        });
        assert!(timed_out.is_err());
        assert!(sidecar.child.try_wait().expect("process state").is_some());

        assert!(matches!(
            sidecar.health().expect("sidecar reconnects"),
            Response::Health {
                writer_owned: true,
                ..
            }
        ));
        assert_ne!(sidecar.engine_process_id, previous_process_id);
        drop(sidecar);
        let _ = std::fs::remove_dir_all(database_root);
    }
}

#[cfg(feature = "sidecar")]
const _: () = {
    assert!(STREAM_TEST_BYTES <= MAX_STREAM_BYTES);
    assert!(STREAM_TEST_CHUNK_BYTES <= MAX_STREAM_CHUNK_BYTES);
};

#[cfg(test)]
fn desktop_fuzz_probe(target: &str, bytes: &[u8]) -> Result<bool, String> {
    let accepted = match target {
        "desktop_sidecar_request_response" => {
            serde_json::from_slice::<Response>(bytes).is_ok()
                || serde_json::from_slice::<worlddb_ode_engine::Request>(bytes).is_ok()
                || std::str::from_utf8(bytes)
                    .ok()
                    .is_some_and(|value| parse_client_operation_id(Some(value.trim())).is_ok())
        }
        "desktop_backup_dto" => backup::fuzz_backup_dto(bytes),
        "desktop_transfer_ids" => {
            transfer::fuzz_transfer_id(bytes) || host_session::fuzz_session_id(bytes)
        }
        "desktop_export_import_dto" => export_import::fuzz_import_dto(bytes),
        "desktop_migration_plan" => migration::fuzz_migration_plan(bytes)?,
        "desktop_purge_report" => purge::fuzz_purge_report(bytes)?,
        _ => return Err(format!("unknown desktop fuzz target: {target}")),
    };
    Ok(accepted)
}

#[cfg(test)]
#[path = "../../../../../tools/fuzz/rust_campaign.rs"]
mod fuzz_campaign_support;

#[cfg(test)]
#[test]
#[ignore = "24-hour fuzz campaign; run through tools/fuzz/run-target.ps1"]
fn desktop_fuzz_campaign() {
    if let Err(error) = fuzz_campaign_support::run_campaign(
        &[
            "desktop_sidecar_request_response",
            "desktop_backup_dto",
            "desktop_transfer_ids",
            "desktop_export_import_dto",
            "desktop_migration_plan",
            "desktop_purge_report",
        ],
        desktop_fuzz_probe,
    ) {
        panic!("desktop fuzz campaign failed: {error}");
    }
}

#[cfg(test)]
mod fuzz_campaign_tests {
    use super::desktop_fuzz_probe;

    const TARGETS: &[&str] = &[
        "desktop_sidecar_request_response",
        "desktop_backup_dto",
        "desktop_transfer_ids",
        "desktop_export_import_dto",
        "desktop_migration_plan",
        "desktop_purge_report",
    ];

    #[test]
    fn fuzz_campaign_dispatch_rejects_unregistered_desktop_targets() {
        assert!(desktop_fuzz_probe("unknown", b"seed").is_err());
        for target in TARGETS {
            assert!(desktop_fuzz_probe(target, b"seed").is_ok());
        }
    }
}
