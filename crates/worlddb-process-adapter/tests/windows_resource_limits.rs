#![deny(unsafe_code)]

#[cfg(windows)]
mod windows_tests {
    use std::fs;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use worlddb_process_adapter::IsolatedChild;

    fn fixture(mode: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_worlddb_process_fixture"));
        command.arg(mode);
        command
    }

    fn wait_with_deadline(child: &mut IsolatedChild) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Err(_) => return None,
            }
        }
    }

    #[test]
    fn memory_exhaustion_is_rejected_by_the_job_limit() {
        let mut command = fixture("memory");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let spawned = IsolatedChild::spawn(&mut command, 32 * 1_024 * 1_024);
        assert!(spawned.is_ok());
        let Ok(mut child) = spawned else {
            return;
        };
        let status = wait_with_deadline(&mut child);
        assert!(status.is_some());
        let Some(status) = status else {
            return;
        };
        assert_eq!(status.code(), Some(42));
    }

    #[test]
    fn adapter_runs_in_a_distinct_os_process() {
        let mut command = fixture("identity");
        command
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped());
        let spawned = IsolatedChild::spawn(&mut command, 64 * 1_024 * 1_024);
        assert!(spawned.is_ok());
        let Ok(mut child) = spawned else {
            return;
        };
        let process_id = child.id();
        let stdout = child.take_stdout();
        assert!(stdout.is_some());
        let Some(stdout) = stdout else {
            return;
        };
        let mut output = String::new();
        let read = BufReader::new(stdout).read_line(&mut output);
        assert!(read.is_ok());
        let status = wait_with_deadline(&mut child);
        assert!(status.is_some());
        let Some(status) = status else {
            return;
        };
        assert!(status.success());
        assert_ne!(output.trim(), std::process::id().to_string());
        assert_eq!(output.trim(), process_id.to_string());
    }

    #[test]
    fn dropping_or_finishing_kills_remaining_descendants() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let marker_dir = std::env::temp_dir().join(format!(
            "worlddb-process-adapter-{}-{nonce}",
            std::process::id()
        ));
        assert!(fs::create_dir(&marker_dir).is_ok());

        let mut command = fixture("tree");
        command
            .arg(&marker_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let spawned = IsolatedChild::spawn(&mut command, 64 * 1_024 * 1_024);
        assert!(spawned.is_ok());
        let Ok(mut child) = spawned else {
            let _ = fs::remove_dir_all(&marker_dir);
            return;
        };
        let stdout = child.take_stdout();
        assert!(stdout.is_some());
        let Some(stdout) = stdout else {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&marker_dir);
            return;
        };
        let mut output = String::new();
        let read = BufReader::new(stdout).read_line(&mut output);
        assert!(read.is_ok());
        assert_eq!(output.trim(), "ready");
        let status = wait_with_deadline(&mut child);
        assert!(status.is_some());
        let Some(status) = status else {
            let _ = fs::remove_dir_all(&marker_dir);
            return;
        };
        assert!(status.success());
        assert!(marker_dir.join("started").exists());
        let terminated = child.terminate_process_tree();
        assert!(terminated.is_ok());
        thread::sleep(Duration::from_millis(2_300));
        assert!(!marker_dir.join("late").exists());
        drop(child);
        let removed = fs::remove_dir_all(&marker_dir);
        assert!(removed.is_ok());
    }
}
