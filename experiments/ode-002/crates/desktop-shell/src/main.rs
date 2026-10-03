use std::path::PathBuf;
use std::time::Instant;

#[cfg(feature = "sidecar")]
use std::time::Duration;

use serde::{Deserialize, Serialize};
#[cfg(feature = "sidecar")]
use std::path::Path;
use tauri::Manager;
#[cfg(feature = "in-process")]
use worlddb_ode_engine::EngineHost;
#[cfg(feature = "sidecar")]
use worlddb_ode_engine::Request;
#[cfg(feature = "sidecar")]
use worlddb_ode_engine::{MAX_STREAM_BYTES, MAX_STREAM_CHUNK_BYTES, fill_deterministic_chunk};
use worlddb_ode_engine::{Response, StreamPlan};
mod host_session;
mod transfer;
use host_session::{
    HostCapability, HostIdentity, HostSessionManager, HostSessionTicket, SessionError,
};
use transfer::{
    BeginTransferRequestV1, BeginTransferResponseV1, ChunkAcknowledgementV1,
    FinishTransferRequestV1, IPC_PROTOCOL_VERSION, MAX_TRANSFER_CHUNK_BYTES, TransferCompletionV1,
    TransferError, TransferManager,
};

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
#[cfg(feature = "sidecar")]
const SIDECAR_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    if let Err(error) = run() {
        eprintln!("WorldDB ODE-002 startup failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let database_root = std::env::var_os("WORLDDB_ODE_DATABASE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("worlddb-ode-002-{}", std::process::id()))
        });

    let host_sessions = HostIdentity::current()
        .map(HostSessionManager::new)
        .map_err(|_| "operating system account identity unavailable".to_owned())?;

    #[cfg(feature = "sidecar")]
    let backend = Backend::Sidecar(std::sync::Mutex::new(Sidecar::start(&database_root)?));
    #[cfg(feature = "in-process")]
    let backend =
        Backend::InProcess(EngineHost::open(&database_root).map_err(|error| error.to_string())?);

    tauri::Builder::default()
        .manage(backend)
        .manage(host_sessions)
        .manage(TransferManager::new())
        .setup(|app| {
            if app.get_webview_window("primary").is_none()
                || app.get_webview_window("secondary").is_none()
            {
                return Err("two native windows were not created".into());
            }
            let state = app.state::<Backend>();
            let engine_at_start = state.health().map_err(std::io::Error::other)?;
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
            finish_transfer
        ])
        .run(tauri::generate_context!())
        .map_err(|error| error.to_string())
}

fn env_enabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
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
    engine: Response,
}

#[derive(Debug, Serialize)]
struct IpcErrorV1 {
    protocol_version: u16,
    code: &'static str,
}

#[derive(Serialize)]
struct SecuritySmokeModeV1 {
    protocol_version: u16,
    enabled: bool,
}

impl IpcErrorV1 {
    fn new(code: &'static str) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            code,
        }
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
    record_ipc_probe(window.label())?;
    Ok(HealthResponseV1 {
        protocol_version: IPC_PROTOCOL_VERSION,
        engine,
    })
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
    use super::{HealthRequestV1, chunk_metadata};
    use tauri::http::{HeaderMap, HeaderValue};

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

#[cfg(feature = "in-process")]
enum Backend {
    InProcess(EngineHost),
}

#[cfg(feature = "sidecar")]
enum Backend {
    Sidecar(std::sync::Mutex<Sidecar>),
}

impl Backend {
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
    engine_process_id: u32,
    engine_build_id: String,
}

#[cfg(feature = "sidecar")]
impl Sidecar {
    fn start(database_root: &Path) -> Result<Self, String> {
        let executable = match std::env::var_os("WORLDDB_ODE_ENGINE_EXECUTABLE") {
            Some(path) => PathBuf::from(path),
            None => std::env::current_exe()
                .map_err(|_| "application executable path unavailable".to_owned())?
                .with_file_name(if cfg!(windows) {
                    "worlddb_ode_engine.exe"
                } else {
                    "worlddb_ode_engine"
                }),
        };
        Self::start_with(database_root, &executable)
    }

    fn start_with(database_root: &Path, executable: &Path) -> Result<Self, String> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let mut child = Command::new(executable)
            .arg(database_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
        let mut replacement = Self::start_with(&self.database_root, &self.executable)?;
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
        let mut replacement = Self::start_with(&self.database_root, &self.executable)?;
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
        let mut replacement = Self::start_with(&self.database_root, updated_executable)?;
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

    fn shutdown_child(&mut self) -> Result<(), String> {
        if !matches!(
            self.request_without_recovery(Request::Shutdown)?,
            Response::Shutdown
        ) {
            self.stop_after_transport_failure();
            return Err("sidecar did not acknowledge shutdown".to_owned());
        }
        let status = wait_for_child(&mut self.child)?;
        self.join_response_reader();
        if !status.success() {
            return Err("sidecar did not shut down cleanly".to_owned());
        }
        Ok(())
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
