use std::ffi::OsStr;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

#[cfg(windows)]
use worlddb_core::PrincipalId;
use worlddb_ode_engine::{
    EngineHost, Request, Response, StreamConsumer, StreamPlan, stream_response,
};

const FRAME_DATA: u8 = 1;
const FRAME_END: u8 = 2;
const FRAME_CANCEL: u8 = 3;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let Some(database_root) = arguments.next().map(PathBuf::from) else {
        std::process::exit(64);
    };
    let host_authenticated = match arguments.next() {
        None => false,
        Some(argument)
            if argument == OsStr::new("--host-account") && arguments.next().is_none() =>
        {
            true
        }
        Some(_) => std::process::exit(64),
    };
    let engine = if host_authenticated {
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
            Request::Panic => engine.panic_for_spike(),
            Request::Shutdown => {
                let _ = write_response(&Response::Shutdown);
                return;
            }
        }
    }
}

#[cfg(windows)]
fn current_host_principal() -> Result<PrincipalId, ()> {
    let identity = current_user_sid()?;
    worlddb_ode_engine::derive_host_account_principal(&identity).map_err(|_| ())
}

#[cfg(windows)]
fn current_user_sid() -> Result<Vec<u8>, ()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let mut token_handle: HANDLE = ptr::null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) };
    if opened == 0 || token_handle.is_null() {
        return Err(());
    }
    let token = Token(token_handle);

    let mut required_bytes = 0_u32;
    unsafe {
        let _ = GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required_bytes);
    }
    if required_bytes < size_of::<TOKEN_USER>() as u32 || required_bytes > 4096 {
        return Err(());
    }
    let word_count = (required_bytes as usize).div_ceil(size_of::<usize>());
    let mut aligned_buffer = vec![0_usize; word_count];
    let buffer = aligned_buffer.as_mut_ptr().cast::<c_void>();
    let read_token = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer,
            required_bytes,
            &mut required_bytes,
        )
    };
    if read_token == 0 {
        return Err(());
    }
    let token_user = unsafe { &*buffer.cast::<TOKEN_USER>() };
    if token_user.User.Sid.is_null() {
        return Err(());
    }
    let sid_length = unsafe { GetLengthSid(token_user.User.Sid) } as usize;
    if sid_length == 0 || sid_length > 1024 {
        return Err(());
    }
    let sid = unsafe { std::slice::from_raw_parts(token_user.User.Sid.cast::<u8>(), sid_length) };
    Ok(sid.to_vec())
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
