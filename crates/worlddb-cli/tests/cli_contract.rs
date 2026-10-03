use std::process::{Command, Output};

fn run(arguments: &[&str]) -> Option<Output> {
    Command::new(env!("CARGO_BIN_EXE_worlddb-cli"))
        .args(arguments)
        .output()
        .ok()
}

#[test]
fn machine_help_and_version_are_single_versioned_json_lines() {
    let help = run(&["--format=jsonl", "v1", "--help"]);
    assert!(help.is_some());
    let Some(help) = help else {
        return;
    };
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.ends_with('\n'));
    assert_eq!(help_text.lines().count(), 1);
    assert!(help_text.contains("\"cli_protocol\":{\"major\":1,\"minor\":0}"));
    assert!(help_text.contains("\"type\":\"help\""));
    assert!(help_text.contains("\"scope\":\"root\""));
    assert!(help_text.contains("\"request_id\":\""));

    let version = run(&["--format", "jsonl", "v1", "version"]);
    assert!(version.is_some());
    let Some(version) = version else {
        return;
    };
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    let version_text = String::from_utf8_lossy(&version.stdout);
    assert_eq!(version_text.lines().count(), 1);
    assert!(version_text.contains("\"type\":\"version\""));
    assert!(version_text.contains("\"protocol\":{\"major\":1,\"minor\":0}"));
}

#[test]
fn unsupported_commands_use_public_code_and_do_not_echo_arguments() {
    let secret = "WDB_SECRET_ARGUMENT_CANARY";
    let output = run(&["--format=jsonl", "v1", "not-a-command", secret]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"type\":\"error\""));
    assert!(stdout.contains("\"code\":\"UnsupportedOperation\""));
    assert!(!stdout.contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[test]
fn adapter_io_errors_use_public_code_without_exposing_paths_or_causes() {
    let secret = "WDB_SECRET_PATH_CANARY";
    let output = run(&[
        "--format=jsonl",
        "v1",
        "adapter",
        "run",
        "--manifest",
        secret,
        "--input",
        "WDB_SECRET_INPUT_CANARY",
        "--output",
        "WDB_SECRET_OUTPUT_CANARY",
        "--",
        "WDB_SECRET_ADAPTER_CANARY",
    ]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(8));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"code\":\"StorageRead\""));
    for canary in [
        secret,
        "WDB_SECRET_INPUT_CANARY",
        "WDB_SECRET_OUTPUT_CANARY",
        "WDB_SECRET_ADAPTER_CANARY",
    ] {
        assert!(!stdout.contains(canary));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(canary));
    }
}

#[test]
fn human_errors_are_stderr_only_and_contain_no_user_arguments() {
    let secret = "WDB_SECRET_ARGUMENT_CANARY";
    let output = run(&["v1", "not-a-command", secret]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[UnsupportedOperation]"));
    assert!(!stderr.contains(secret));
}

#[test]
fn unsupported_cli_versions_have_a_distinct_public_code() {
    let output = run(&["--format=jsonl", "v2", "help"]);
    assert!(output.is_some());
    let Some(output) = output else {
        return;
    };

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"UnsupportedProtocolVersion\""));
}
