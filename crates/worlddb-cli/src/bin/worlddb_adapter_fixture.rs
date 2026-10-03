#![deny(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::process;
use std::thread;
use std::time::Duration;

use worlddb_cli::adapter_protocol::{
    AdapterHandshake, AdapterOperation, ProtocolVersion, read_adapter_manifest,
    read_adapter_payload, write_adapter_handshake, write_adapter_output,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("adapter fixture failed: {error}");
        process::exit(80);
    }
}

fn run() -> Result<(), String> {
    let mode = env::args().nth(1).unwrap_or_default();
    if mode == "sleep-before-handshake" {
        thread::sleep(Duration::from_secs(10));
        return Ok(());
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let manifest = read_adapter_manifest(&mut input).map_err(|error| error.to_string())?;
    let capabilities = if mode == "missing-capability" {
        Vec::new()
    } else {
        manifest.required_capabilities().to_vec()
    };
    let handshake = AdapterHandshake::new(ProtocolVersion::new(1, 0), capabilities)
        .map_err(|error| error.to_string())?;
    write_adapter_handshake(&mut output, &handshake).map_err(|error| error.to_string())?;
    if mode == "missing-capability" {
        let _ = read_adapter_payload(&mut input, &manifest);
        return Ok(());
    }
    if mode == "write-then-crash" {
        let payload =
            read_adapter_payload(&mut input, &manifest).map_err(|error| error.to_string())?;
        write_adapter_output(&mut output, &payload).map_err(|error| error.to_string())?;
        process::exit(73);
    }
    if mode == "invalid-output" {
        let _ = read_adapter_payload(&mut input, &manifest).map_err(|error| error.to_string())?;
        output
            .write_all(b"not-an-adapter-frame")
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let payload = read_adapter_payload(&mut input, &manifest).map_err(|error| error.to_string())?;
    if manifest.operation() != AdapterOperation::Import {
        return Err(String::from("fixture only accepts import manifests"));
    }
    if mode == "memory" {
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(96 * 1_024 * 1_024).is_err() {
            process::exit(42);
        }
        bytes.resize(96 * 1_024 * 1_024, 1_u8);
        return Err(format!("unexpectedly allocated {} bytes", bytes.len()));
    }
    if mode != "echo" {
        return Err(String::from("unknown fixture mode"));
    }
    write_adapter_output(&mut output, &payload).map_err(|error| error.to_string())
}
