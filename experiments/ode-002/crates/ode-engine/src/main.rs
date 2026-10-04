use std::ffi::OsStr;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::str::FromStr;

use std::time::{Duration, Instant};

#[cfg(windows)]
use worlddb_core::PrincipalId;
use worlddb_core::{CancellationRequestDisposition, JobId, OperationId};
use worlddb_ode_engine::{
    EngineHost, JobJournalViewStatus, JobListView, JobShutdownView, RecoveryHost, Request,
    Response, StreamConsumer, StreamPlan, stream_response, with_operation_id,
};

const FRAME_DATA: u8 = 1;
const FRAME_END: u8 = 2;
const FRAME_CANCEL: u8 = 3;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let Some(database_root) = arguments.next().map(PathBuf::from) else {
        std::process::exit(64);
    };
    let mut host_authenticated = false;
    let mut recovery_only = false;
    for argument in arguments {
        if argument == OsStr::new("--host-account") && !host_authenticated {
            host_authenticated = true;
        } else if argument == OsStr::new("--recovery-only") && !recovery_only {
            recovery_only = true;
        } else {
            std::process::exit(64);
        }
    }
    if recovery_only {
        if !host_authenticated {
            std::process::exit(64);
        }
        #[cfg(windows)]
        {
            let _principal = current_host_principal().unwrap_or_else(|_| std::process::exit(73));
            run_recovery_service(&database_root);
            return;
        }
        #[cfg(not(windows))]
        std::process::exit(73);
    }
    let mut engine = if host_authenticated {
        #[cfg(windows)]
        {
            let principal_id = current_host_principal().unwrap_or_else(|_| std::process::exit(73));
            match EngineHost::open_authorized(&database_root, principal_id) {
                Ok((engine, _access)) => engine,
                Err(_) => std::process::exit(73),
            }
        }
        #[cfg(not(windows))]
        {
            std::process::exit(73)
        }
    } else if cfg!(debug_assertions) {
        match EngineHost::open(&database_root) {
            Ok(engine) => engine,
            Err(_) => std::process::exit(73),
        }
    } else {
        std::process::exit(73)
    };
    if write_response(&Response::Ready {
        engine_process_id: std::process::id(),
        engine_build_id: worlddb_ode_engine::ENGINE_BUILD_ID.to_owned(),
        protocol_version: 1,
    })
    .is_err()
    {
        std::process::exit(74);
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {}
            Err(_) => std::process::exit(74),
        }
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(_) => {
                if write_response(&Response::Error {
                    code: "invalid_request".to_owned(),
                })
                .is_err()
                {
                    std::process::exit(74);
                }
                continue;
            }
        };
        match request {
            Request::Health => {
                let response = engine.health().unwrap_or_else(|_| Response::Error {
                    code: "engine_failed".to_owned(),
                });
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::JobsList => {
                let response = engine
                    .list_jobs()
                    .map(|result| Response::Jobs { result })
                    .unwrap_or_else(|_| Response::Error {
                        code: "job_state_unavailable".to_owned(),
                    });
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::JobCancel { job_id } => {
                let response = match JobId::from_str(&job_id) {
                    Ok(job_id) => match engine.cancel_job(job_id) {
                        Ok(disposition) => {
                            let name = match disposition {
                                CancellationRequestDisposition::Signalled => "signalled",
                                CancellationRequestDisposition::AlreadySignalled => {
                                    "already_signalled"
                                }
                                CancellationRequestDisposition::TooLate => "too_late",
                                CancellationRequestDisposition::AlreadyTerminal => {
                                    "already_terminal"
                                }
                                CancellationRequestDisposition::OutcomeUnknown => "outcome_unknown",
                            };
                            let jobs = engine.list_jobs().unwrap_or(JobListView {
                                status: JobJournalViewStatus::Unavailable,
                                jobs: Vec::new(),
                            });
                            Response::JobCancel {
                                disposition: name.to_owned(),
                                jobs,
                            }
                        }
                        Err(_) => Response::Error {
                            code: "job_cancel_unavailable".to_owned(),
                        },
                    },
                    Err(_) => Response::Error {
                        code: "invalid_job_id".to_owned(),
                    },
                };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::RecoveryInspect | Request::RecoveryRun | Request::RecoverySalvage { .. } => {
                if write_response(&Response::Error {
                    code: "recovery_mode_required".to_owned(),
                })
                .is_err()
                {
                    std::process::exit(74);
                }
            }
            Request::StreamSink {
                total_bytes,
                chunk_bytes,
                cancel_after_bytes,
            } => {
                let plan = StreamPlan {
                    total_bytes,
                    chunk_bytes,
                    cancel_after_bytes,
                };
                let response = consume_framed_stream(&mut reader, plan)
                    .map(stream_response)
                    .unwrap_or_else(|_| std::process::exit(65));
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::StreamStart {
                transfer_id,
                total_bytes,
                chunk_bytes,
            } => {
                let response = match engine.begin_stream(&transfer_id, total_bytes, chunk_bytes) {
                    Ok(()) => Response::StreamStarted { transfer_id },
                    Err(_) => Response::Error {
                        code: "stream_rejected".to_owned(),
                    },
                };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::StreamChunk {
                transfer_id,
                sequence,
                chunk_bytes,
            } => {
                let chunk = read_binary_chunk(&mut reader, chunk_bytes)
                    .unwrap_or_else(|_| std::process::exit(65));
                let response = match engine.push_stream_chunk(&transfer_id, sequence, &chunk) {
                    Ok(bytes_received) => Response::StreamChunkAccepted {
                        transfer_id,
                        sequence,
                        bytes_received,
                    },
                    Err(_) => Response::Error {
                        code: "stream_rejected".to_owned(),
                    },
                };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::StreamFinish {
                transfer_id,
                cancelled,
            } => {
                let response = engine
                    .finish_stream(&transfer_id, cancelled)
                    .map(stream_response)
                    .unwrap_or_else(|_| Response::Error {
                        code: "stream_rejected".to_owned(),
                    });
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::Schema {
                operation_id,
                command,
            } => {
                let response =
                    match with_request_operation_id(operation_id, || engine.schema(command)) {
                        Ok(Ok(result)) => Response::Schema { result },
                        Ok(Err(worlddb_ode_engine::EngineError::Schema(_))) => Response::Error {
                            code: "schema_rejected".to_owned(),
                        },
                        Ok(Err(_)) => Response::Error {
                            code: "engine_failed".to_owned(),
                        },
                        Err(()) => Response::Error {
                            code: "invalid_operation_id".to_owned(),
                        },
                    };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::Entities {
                operation_id,
                command,
            } => {
                let response =
                    match with_request_operation_id(operation_id, || engine.entities(command)) {
                        Ok(Ok(result)) => Response::Entities { result },
                        Ok(Err(worlddb_ode_engine::EngineError::Entity(_))) => Response::Error {
                            code: "entity_rejected".to_owned(),
                        },
                        Ok(Err(_)) => Response::Error {
                            code: "engine_failed".to_owned(),
                        },
                        Err(()) => Response::Error {
                            code: "invalid_operation_id".to_owned(),
                        },
                    };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::BranchLayers {
                operation_id,
                command,
            } => {
                let response =
                    match with_request_operation_id(operation_id, || engine.branch_layers(command))
                    {
                        Ok(Ok(result)) => Response::BranchLayers { result },
                        Ok(Err(worlddb_ode_engine::EngineError::BranchLayer(_))) => {
                            Response::Error {
                                code: "branch_layer_rejected".to_owned(),
                            }
                        }
                        Ok(Err(_)) => Response::Error {
                            code: "engine_failed".to_owned(),
                        },
                        Err(()) => Response::Error {
                            code: "invalid_operation_id".to_owned(),
                        },
                    };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::HistorySpaceTransfer {
                operation_id,
                command,
            } => {
                let response = match with_request_operation_id(operation_id, || {
                    engine.history_space_transfer(command)
                }) {
                    Ok(Ok(result)) => Response::HistorySpaceTransfer { result },
                    Ok(Err(worlddb_ode_engine::EngineError::HistorySpaceTransfer(_))) => {
                        Response::Error {
                            code: "history_space_transfer_rejected".to_owned(),
                        }
                    }
                    Ok(Err(_)) => Response::Error {
                        code: "engine_failed".to_owned(),
                    },
                    Err(()) => Response::Error {
                        code: "invalid_operation_id".to_owned(),
                    },
                };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::Facts {
                operation_id,
                command,
            } => {
                let response =
                    match with_request_operation_id(operation_id, || engine.facts(*command)) {
                        Ok(Ok(result)) => Response::Facts {
                            result: Box::new(result),
                        },
                        Ok(Err(worlddb_ode_engine::EngineError::Fact(_))) => Response::Error {
                            code: "facts_rejected".to_owned(),
                        },
                        Ok(Err(_)) => Response::Error {
                            code: "engine_failed".to_owned(),
                        },
                        Err(()) => Response::Error {
                            code: "invalid_operation_id".to_owned(),
                        },
                    };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::Perspectives {
                operation_id,
                command,
            } => {
                let response = match with_request_operation_id(operation_id, || {
                    engine.perspectives(command)
                }) {
                    Ok(Ok(result)) => Response::Perspectives { result },
                    Ok(Err(worlddb_ode_engine::EngineError::Perspective(_))) => Response::Error {
                        code: "perspective_rejected".to_owned(),
                    },
                    Ok(Err(_)) => Response::Error {
                        code: "engine_failed".to_owned(),
                    },
                    Err(()) => Response::Error {
                        code: "invalid_operation_id".to_owned(),
                    },
                };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::SecurityPolicy {
                operation_id,
                command,
            } => {
                let response = match with_request_operation_id(operation_id, || {
                    engine.security_policy(command)
                }) {
                    Ok(Ok(result)) => Response::SecurityPolicy { result },
                    Ok(Err(worlddb_ode_engine::EngineError::SecurityPolicy(_))) => {
                        Response::Error {
                            code: "security_policy_rejected".to_owned(),
                        }
                    }
                    Ok(Err(_)) => Response::Error {
                        code: "engine_failed".to_owned(),
                    },
                    Err(()) => Response::Error {
                        code: "invalid_operation_id".to_owned(),
                    },
                };
                if write_response(&response).is_err() {
                    std::process::exit(74);
                }
            }
            Request::Panic => engine.panic_for_spike(),
            Request::Shutdown => {
                let result = engine
                    .shutdown_jobs(Instant::now() + Duration::from_secs(2))
                    .unwrap_or(JobShutdownView {
                        drained: false,
                        unfinished_job_ids: Vec::new(),
                        unfinished_workers: 0,
                        worker_panics: 0,
                    });
                let drained = result.drained;
                if write_response(&Response::Shutdown { result }).is_err() {
                    std::process::exit(74);
                }
                if drained {
                    return;
                }
            }
        }
    }
}

fn run_recovery_service(database_root: &std::path::Path) {
    let recovery = RecoveryHost::open(database_root).unwrap_or_else(|_| std::process::exit(73));
    if write_response(&Response::Ready {
        engine_process_id: std::process::id(),
        engine_build_id: worlddb_ode_engine::ENGINE_BUILD_ID.to_owned(),
        protocol_version: 1,
    })
    .is_err()
    {
        std::process::exit(74);
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {}
            Err(_) => std::process::exit(74),
        }
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(_) => {
                if write_response(&Response::Error {
                    code: "invalid_request".to_owned(),
                })
                .is_err()
                {
                    std::process::exit(74);
                }
                continue;
            }
        };
        let (response, shutdown) = match request {
            Request::RecoveryInspect => (
                recovery
                    .inspect()
                    .map(|result| Response::RecoveryReport { result })
                    .unwrap_or_else(recovery_error_response),
                false,
            ),
            Request::RecoveryRun => (
                recovery
                    .run_journaled_recovery()
                    .map(|result| Response::RecoveryApplied { result })
                    .unwrap_or_else(recovery_error_response),
                false,
            ),
            Request::RecoverySalvage { destination } => (
                recovery
                    .salvage_to(&destination)
                    .map(|result| Response::RecoverySalvaged { result })
                    .unwrap_or_else(recovery_error_response),
                false,
            ),
            Request::Shutdown => (
                Response::Shutdown {
                    result: JobShutdownView {
                        drained: true,
                        unfinished_job_ids: Vec::new(),
                        unfinished_workers: 0,
                        worker_panics: 0,
                    },
                },
                true,
            ),
            _ => (
                Response::Error {
                    code: "recovery_only_mode".to_owned(),
                },
                false,
            ),
        };
        if write_response(&response).is_err() {
            std::process::exit(74);
        }
        if shutdown {
            return;
        }
    }
}

fn recovery_error_response(error: worlddb_ode_engine::EngineError) -> Response {
    let code = match error {
        worlddb_ode_engine::EngineError::Recovery(code) => code,
        _ => "recovery_action_failed",
    };
    Response::Error {
        code: code.to_owned(),
    }
}

fn with_request_operation_id<T>(
    operation_id: Option<String>,
    operation: impl FnOnce() -> T,
) -> Result<T, ()> {
    match operation_id {
        Some(value) => {
            let operation_id = OperationId::from_str(&value).map_err(|_| ())?;
            Ok(with_operation_id(operation_id, operation))
        }
        None => Ok(operation()),
    }
}

#[cfg(windows)]
fn current_host_principal() -> Result<PrincipalId, ()> {
    let identity = worlddb_process_adapter::current_process_identity_bytes().map_err(|_| ())?;
    worlddb_core::derive_host_account_principal(&identity).map_err(|_| ())
}

fn read_binary_chunk<R: Read>(reader: &mut R, expected_length: u32) -> io::Result<Vec<u8>> {
    if expected_length == 0 || expected_length > worlddb_ode_engine::MAX_STREAM_CHUNK_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid stream chunk length",
        ));
    }
    let mut header = [0_u8; 5];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header[1..5].try_into().map_err(invalid_data)?);
    if header[0] != FRAME_DATA || length != expected_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid stream frame",
        ));
    }
    let mut chunk = vec![0; length as usize];
    reader.read_exact(&mut chunk)?;
    Ok(chunk)
}

fn consume_framed_stream<R: Read>(
    reader: &mut R,
    plan: StreamPlan,
) -> io::Result<worlddb_ode_engine::StreamReport> {
    let plan = plan.validate().map_err(invalid_data)?;
    let mut consumer = StreamConsumer::new(plan).map_err(invalid_data)?;
    let mut header = [0_u8; 5];
    loop {
        reader.read_exact(&mut header)?;
        let length = u32::from_le_bytes(header[1..5].try_into().map_err(invalid_data)?) as usize;
        match header[0] {
            FRAME_DATA if length > 0 && length <= plan.chunk_bytes as usize => {
                let mut chunk = vec![0; length];
                reader.read_exact(&mut chunk)?;
                consumer.push_chunk(&chunk).map_err(invalid_data)?;
            }
            FRAME_END if length == 0 => return consumer.finish(false).map_err(invalid_data),
            FRAME_CANCEL if length == 0 => return consumer.finish(true).map_err(invalid_data),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid frame")),
        }
    }
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn write_response(response: &Response) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let bytes = serde_json::to_vec(response).map_err(io::Error::other)?;
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
