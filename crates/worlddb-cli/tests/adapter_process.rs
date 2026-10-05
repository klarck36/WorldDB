#![deny(unsafe_code)]

#[cfg(windows)]
mod windows_tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use worlddb_cli::adapter_protocol::{
        AdapterBudget, AdapterCapability, AdapterManifest, AdapterOperation,
    };

    static TEMPORARY_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_manifest(timeout_millis: u64, memory_limit_bytes: u64) -> Option<AdapterManifest> {
        let budget = AdapterBudget::new(timeout_millis, memory_limit_bytes, 1_024, 1_024).ok()?;
        let capability = AdapterCapability::new("logical_records_v1").ok()?;
        AdapterManifest::new(
            AdapterOperation::Import,
            [7; 32],
            b"canonical-id-map".to_vec(),
            budget,
            vec![capability],
        )
        .ok()
    }

    fn temporary_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let sequence = TEMPORARY_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "worlddb-adapter-e2e-{}-{nonce}-{sequence}",
            std::process::id(),
        ))
    }

    fn run_cli(
        mode: &str,
        manifest_path: &Path,
        input_path: &Path,
        output_path: &Path,
    ) -> std::io::Result<std::process::Output> {
        Command::new(env!("CARGO_BIN_EXE_worlddb-cli"))
            .arg("adapter")
            .arg("run")
            .arg("--manifest")
            .arg(manifest_path)
            .arg("--input")
            .arg(input_path)
            .arg("--output")
            .arg(output_path)
            .arg("--")
            .arg(env!("CARGO_BIN_EXE_worlddb_adapter_fixture"))
            .arg(mode)
            .output()
    }

    fn write_request(
        directory: &Path,
        timeout_millis: u64,
        memory_limit_bytes: u64,
    ) -> Option<(PathBuf, PathBuf, PathBuf)> {
        let manifest = test_manifest(timeout_millis, memory_limit_bytes)?;
        let manifest_bytes = manifest.encode().ok()?;
        let manifest_path = directory.join("manifest.bin");
        let input_path = directory.join("input.bin");
        let output_path = directory.join("output.bin");
        fs::write(&manifest_path, manifest_bytes).ok()?;
        fs::write(&input_path, b"canonical-input-records").ok()?;
        Some((manifest_path, input_path, output_path))
    }

    #[test]
    fn cli_import_adapter_roundtrip_uses_a_separate_process() {
        let directory = temporary_directory();
        assert!(fs::create_dir(&directory).is_ok());
        let request = write_request(&directory, 5_000, 64 * 1_024 * 1_024);
        assert!(request.is_some());
        let Some((manifest_path, input_path, output_path)) = request else {
            let _ = fs::remove_dir_all(&directory);
            return;
        };
        let result = run_cli("echo", &manifest_path, &input_path, &output_path);
        assert!(result.is_ok());
        let Some(result) = result.ok() else {
            let _ = fs::remove_dir_all(&directory);
            return;
        };
        assert!(result.status.success());
        assert_eq!(
            fs::read(&output_path).ok(),
            Some(b"canonical-input-records".to_vec())
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("adapter completed"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn missing_capability_and_crash_never_replace_the_previous_output() {
        for mode in ["missing-capability", "write-then-crash"] {
            let directory = temporary_directory().join(mode);
            assert!(fs::create_dir_all(&directory).is_ok());
            let request = write_request(&directory, 5_000, 64 * 1_024 * 1_024);
            assert!(request.is_some());
            let Some((manifest_path, input_path, output_path)) = request else {
                let _ = fs::remove_dir_all(directory.parent().unwrap_or(&directory));
                continue;
            };
            assert!(fs::write(&output_path, b"previous-good-output").is_ok());
            let result = run_cli(mode, &manifest_path, &input_path, &output_path);
            assert!(result.is_ok());
            let Some(result) = result.ok() else {
                let _ = fs::remove_dir_all(directory.parent().unwrap_or(&directory));
                continue;
            };
            assert!(!result.status.success());
            assert_eq!(
                fs::read(&output_path).ok(),
                Some(b"previous-good-output".to_vec())
            );
            let _ = fs::remove_dir_all(directory.parent().unwrap_or(&directory));
        }
    }

    #[test]
    fn adapter_timeout_kills_the_child_and_returns_without_output() {
        let directory = temporary_directory();
        assert!(fs::create_dir(&directory).is_ok());
        let request = write_request(&directory, 250, 64 * 1_024 * 1_024);
        assert!(request.is_some());
        let Some((manifest_path, input_path, output_path)) = request else {
            let _ = fs::remove_dir_all(&directory);
            return;
        };
        let start = Instant::now();
        let result = run_cli(
            "sleep-before-handshake",
            &manifest_path,
            &input_path,
            &output_path,
        );
        assert!(result.is_ok());
        let Some(result) = result.ok() else {
            let _ = fs::remove_dir_all(&directory);
            return;
        };
        assert!(!result.status.success());
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(!output_path.exists());
        thread::sleep(Duration::from_millis(50));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn adapter_memory_limit_stops_a_child_that_exceeds_its_budget() {
        let directory = temporary_directory();
        assert!(fs::create_dir(&directory).is_ok());
        let request = write_request(&directory, 5_000, 32 * 1_024 * 1_024);
        assert!(request.is_some());
        let Some((manifest_path, input_path, output_path)) = request else {
            let _ = fs::remove_dir_all(&directory);
            return;
        };
        let result = run_cli("memory", &manifest_path, &input_path, &output_path);
        assert!(result.is_ok());
        let Some(result) = result.ok() else {
            let _ = fs::remove_dir_all(&directory);
            return;
        };
        assert!(!result.status.success());
        assert!(!output_path.exists());
        let _ = fs::remove_dir_all(&directory);
    }
}
