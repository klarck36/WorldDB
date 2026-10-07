#![deny(unsafe_code)]

//! CLI-side process boundaries for WorldDB extension adapters.

pub mod adapter_protocol;
pub mod cli;

#[cfg(test)]
#[path = "../../../tools/fuzz/rust_campaign.rs"]
mod fuzz_campaign_support;

#[cfg(test)]
fn fuzz_campaign_probe(target: &str, bytes: &[u8]) -> Result<bool, String> {
    let accepted = match target {
        "cli_arguments" => cli::fuzz_arguments(bytes),
        "cli_policy_history" => cli::fuzz_policy_history(bytes)?,
        "cli_adapter_manifest"
        | "cli_adapter_payload"
        | "cli_adapter_handshake"
        | "cli_adapter_output"
        | "cli_adapter_frame" => adapter_protocol::fuzz_probe(target, bytes),
        "cli_import_mapping" => cli::fuzz_import_mapping(bytes),
        "cli_migration_plan_json" => cli::fuzz_migration_plan_json(bytes)?,
        "cli_migration_step_records" => cli::fuzz_migration_step_records(bytes)?,
        _ => return Err(format!("unknown CLI fuzz target: {target}")),
    };
    Ok(accepted)
}

#[cfg(test)]
#[test]
#[ignore = "24-hour fuzz campaign; run through tools/fuzz/run-target.ps1"]
fn cli_fuzz_campaign() -> Result<(), String> {
    let campaign = fuzz_campaign_support::run_campaign(
        &[
            "cli_arguments",
            "cli_policy_history",
            "cli_adapter_manifest",
            "cli_adapter_payload",
            "cli_adapter_handshake",
            "cli_adapter_output",
            "cli_adapter_frame",
            "cli_import_mapping",
            "cli_migration_plan_json",
            "cli_migration_step_records",
        ],
        fuzz_campaign_probe,
    );
    let cleanup = cli::cleanup_policy_history_fuzz_fixture();
    campaign.map_err(|error| format!("CLI fuzz campaign failed: {error}"))?;
    cleanup.map_err(|error| format!("CLI fuzz fixture cleanup failed: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod fuzz_campaign_tests {
    use super::fuzz_campaign_probe;

    const TARGETS: &[&str] = &[
        "cli_arguments",
        "cli_policy_history",
        "cli_adapter_manifest",
        "cli_adapter_payload",
        "cli_adapter_handshake",
        "cli_adapter_output",
        "cli_adapter_frame",
        "cli_import_mapping",
        "cli_migration_plan_json",
        "cli_migration_step_records",
    ];

    #[test]
    fn fuzz_campaign_dispatch_rejects_unregistered_cli_targets() {
        assert!(fuzz_campaign_probe("unknown", b"seed").is_err());
        for target in TARGETS {
            assert!(fuzz_campaign_probe(target, b"seed").is_ok());
        }
        assert!(super::cli::cleanup_policy_history_fuzz_fixture().is_ok());
    }
}
