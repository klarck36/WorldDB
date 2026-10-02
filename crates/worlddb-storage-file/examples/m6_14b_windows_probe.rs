//! Windows storage probe for the M6-14b small-commit and recovery lanes.

#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        eprintln!("m6-14b Windows probe failed: {error}");
        std::process::exit(2);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("m6-14b Windows probe must run on Windows");
    std::process::exit(2);
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::{self, File};
    use std::io::Read;
    use std::path::PathBuf;
    use std::time::Instant;

    use worlddb_core::{DomainId, OperationId};
    use worlddb_storage_file::{DatabaseLayout, RecoveryManager, WalPrepareLog};

    const SMALL_COMMIT_SAMPLES: u64 = 35;
    const RECOVERY_SAMPLES: u64 = 30;
    const CHUNK_BYTES: usize = 32 * 1024 * 1024;
    let operation_id = |counter: u64| -> Result<OperationId, Box<dyn std::error::Error>> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8..16].copy_from_slice(&counter.to_be_bytes());
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(OperationId::try_from_bytes(bytes)?)
    };

    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(output_root) = arguments.first().map(PathBuf::from) else {
        return Err("usage: m6_14b_windows_probe OUTPUT_ROOT PROVENANCE_EDGES_BIN".into());
    };
    let Some(provenance_path) = arguments.get(1).map(PathBuf::from) else {
        return Err("usage: m6_14b_windows_probe OUTPUT_ROOT PROVENANCE_EDGES_BIN".into());
    };
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is not set")?;
    let local_app_data = fs::canonicalize(local_app_data)?;
    fs::create_dir_all(&output_root)?;
    let output_root = fs::canonicalize(output_root)?;
    let corpus_path = fs::canonicalize(provenance_path)?;
    if !output_root.starts_with(&local_app_data) || !corpus_path.starts_with(&local_app_data) {
        return Err("probe output and corpus must both be under LOCALAPPDATA".into());
    }
    if output_root.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("onedrive")
    }) {
        return Err("probe output must not be stored in OneDrive".into());
    }

    println!("operation,iteration,elapsed_ns,bytes");
    let commit_root = output_root.join("small-commit-db");
    let layout = DatabaseLayout::create(&commit_root)?;
    let lock = layout.try_writer_lock()?;
    let log = WalPrepareLog::new(&layout);
    let small_payload = [0x57_u8; 64];
    for iteration in 0..SMALL_COMMIT_SAMPLES {
        let operation_id = operation_id(iteration + 1)?;
        let start = Instant::now();
        log.commit_operation(&lock, operation_id, &small_payload)?;
        println!(
            "commit_small_warm,{iteration},{},{}",
            start.elapsed().as_nanos(),
            small_payload.len()
        );
    }
    drop(lock);

    let recovery_root = output_root.join("large-recovery-db");
    let recovery_layout = DatabaseLayout::create(&recovery_root)?;
    let recovery_lock = recovery_layout.try_writer_lock()?;
    let recovery_log = WalPrepareLog::new(&recovery_layout);
    let mut corpus = File::open(&corpus_path)?;
    let corpus_bytes = corpus.metadata()?.len();
    let mut remaining = corpus_bytes;
    let mut iteration = 0_u64;
    while remaining > 0 {
        let chunk_size = usize::try_from(remaining.min(CHUNK_BYTES as u64))?;
        let mut payload = Vec::new();
        payload.try_reserve_exact(chunk_size)?;
        payload.resize(chunk_size, 0);
        corpus.read_exact(&mut payload)?;
        let operation_id = operation_id(10_000 + iteration)?;
        recovery_log.commit_operation(&recovery_lock, operation_id, &payload)?;
        iteration = iteration
            .checked_add(1)
            .ok_or("operation counter overflow")?;
        remaining = remaining
            .checked_sub(u64::try_from(chunk_size)?)
            .ok_or("provenance input length underflow")?;
    }
    if corpus.read(&mut [0_u8; 1])? != 0 {
        return Err("provenance corpus changed while the probe was running".into());
    }
    drop(corpus);

    let manager = RecoveryManager::new(recovery_layout);
    for sample in 0..RECOVERY_SAMPLES {
        let start = Instant::now();
        manager.recover(&recovery_lock)?;
        println!(
            "recovery_large_warm,{sample},{},{}",
            start.elapsed().as_nanos(),
            corpus_bytes
        );
    }
    Ok(())
}
