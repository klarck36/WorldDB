use std::{
    ffi::OsString,
    fs::{self, FileType},
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackupProfile {
    ExactDatabase,
    AuditComplete,
}

impl BackupProfile {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "exact" => Ok(Self::ExactDatabase),
            "audit_complete" => Ok(Self::AuditComplete),
            _ => Err("backup profile is unsupported".to_owned()),
        }
    }

    const fn cli_label(self) -> &'static str {
        match self {
            Self::ExactDatabase => "exact",
            Self::AuditComplete => "audit-complete",
        }
    }

    const fn audit_scope(self) -> &'static str {
        match self {
            Self::ExactDatabase => "excluded",
            Self::AuditComplete => "included",
        }
    }

    const fn display_label(self) -> &'static str {
        match self {
            Self::ExactDatabase => "ExactDatabaseBackup",
            Self::AuditComplete => "AuditCompleteBackup",
        }
    }

    const fn directory_label(self) -> &'static str {
        match self {
            Self::ExactDatabase => "exact",
            Self::AuditComplete => "audit-complete",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupRequestV1 {
    pub(crate) protocol_version: u16,
    pub(crate) profile: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct BackupOperationView {
    pub(crate) action: &'static str,
    pub(crate) profile: &'static str,
    pub(crate) audit_scope: &'static str,
    pub(crate) database_id: Option<String>,
    pub(crate) revision: Option<String>,
    pub(crate) source_database_id: Option<String>,
    pub(crate) source_revision: Option<String>,
    pub(crate) restored_database_id: Option<String>,
    pub(crate) restored_revision: Option<String>,
    pub(crate) item_count: String,
    pub(crate) audit_safe_sequence: Option<String>,
    pub(crate) authenticity: String,
    pub(crate) target_verified: bool,
    pub(crate) source_modified: bool,
    pub(crate) restore_mode: Option<&'static str>,
    pub(crate) folder_name: String,
    pub(crate) clone_verify_disposition: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CliBackupSummary {
    status: String,
    profile: String,
    audit_scope: String,
    database_id: String,
    revision: String,
    item_count: String,
    audit_safe_sequence: Option<String>,
    authenticity: String,
    target_verified: bool,
    source_modified: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliRestoreSummary {
    status: String,
    profile: String,
    audit_scope: String,
    source_database_id: String,
    source_revision: String,
    restored_database_id: String,
    restored_revision: String,
    item_count: String,
    audit_safe_sequence: Option<String>,
    authenticity: String,
    target_verified: bool,
    source_modified: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliVerifySummary {
    status: String,
    safe_revision: String,
    disposition: String,
    finding_count: String,
    source_modified: bool,
}

pub(crate) fn create_backup(
    source_path: &Path,
    parent_path: &Path,
    profile: BackupProfile,
) -> Result<BackupOperationView, String> {
    let source = canonical_directory(source_path, "backup source")?;
    let parent = canonical_directory(parent_path, "backup destination")?;
    let target = new_target(&parent, &source, "backup", profile.directory_label())?;
    let created = backup_command("create", &source, &target, profile)?;
    validate_backup_summary(&created, "created", profile)?;

    let independently_verified =
        backup_command("verify", &target, &target, profile).map_err(|error| {
            format!(
                "backup {} was created, but its independent verification failed ({error})",
                folder_name(&target)
            )
        })?;
    validate_backup_summary(&independently_verified, "verified", profile).map_err(|error| {
        format!(
            "backup {} was created, but its independent verification did not match ({error})",
            folder_name(&target)
        )
    })?;
    if !same_backup(&created, &independently_verified) {
        return Err(format!(
            "backup {} was created, but its independent verification changed the profile or inventory",
            folder_name(&target)
        ));
    }

    Ok(backup_view(
        "created",
        profile,
        &created,
        folder_name(&target),
    ))
}

pub(crate) fn verify_backup(
    backup_path: &Path,
    profile: BackupProfile,
) -> Result<BackupOperationView, String> {
    let backup = canonical_directory(backup_path, "backup")?;
    let verified = backup_command("verify", &backup, &backup, profile)?;
    validate_backup_summary(&verified, "verified", profile)?;
    Ok(backup_view(
        "verified",
        profile,
        &verified,
        folder_name(&backup),
    ))
}

pub(crate) fn restore_clone(
    backup_path: &Path,
    authorization_path: &Path,
    parent_path: &Path,
    profile: BackupProfile,
) -> Result<BackupOperationView, String> {
    let backup = canonical_directory(backup_path, "backup")?;
    let authorization = canonical_directory(authorization_path, "restore authorization project")?;
    let parent = canonical_directory(parent_path, "restore destination")?;

    let backup_verification = backup_command("verify", &backup, &backup, profile)?;
    validate_backup_summary(&backup_verification, "verified", profile)?;
    if backup_verification.database_id.is_empty() {
        return Err("verified backup has no source database identity".to_owned());
    }

    let target = new_target(
        &parent,
        &authorization,
        "restore-clone",
        profile.directory_label(),
    )?;
    ensure_disjoint(
        &target,
        &backup,
        "restore destination overlaps the selected backup",
    )?;
    let restored = restore_command(&backup, &authorization, &target, profile)?;
    if restored.status != "restored"
        || restored.profile != cli_profile(profile)
        || restored.audit_scope != cli_audit_scope(profile)
        || restored.source_database_id != backup_verification.database_id
        || restored.source_revision != backup_verification.revision
        || restored.item_count != backup_verification.item_count
        || restored.audit_safe_sequence != backup_verification.audit_safe_sequence
        || restored.authenticity != backup_verification.authenticity
        || restored.restored_database_id == restored.source_database_id
        || !restored.target_verified
        || restored.source_modified
    {
        return Err(format!(
            "restore into {} completed without a verifiable matching clone",
            folder_name(&target)
        ));
    }

    let clone_verification = verify_project(&target).map_err(|error| {
        format!(
            "restore clone {} was created, but its independent project verification failed ({error})",
            folder_name(&target)
        )
    })?;
    if clone_verification.status != "completed"
        || clone_verification.disposition != "clean"
        || clone_verification.finding_count != "0"
        || clone_verification.safe_revision != restored.restored_revision
        || clone_verification.source_modified
    {
        return Err(format!(
            "restore clone {} was created, but its independent project verification was not clean",
            folder_name(&target)
        ));
    }

    Ok(BackupOperationView {
        action: "restored",
        profile: profile.display_label(),
        audit_scope: display_audit_scope(profile),
        database_id: None,
        revision: None,
        source_database_id: Some(restored.source_database_id),
        source_revision: Some(restored.source_revision),
        restored_database_id: Some(restored.restored_database_id),
        restored_revision: Some(restored.restored_revision),
        item_count: restored.item_count,
        audit_safe_sequence: restored.audit_safe_sequence,
        authenticity: restored.authenticity,
        target_verified: true,
        source_modified: false,
        restore_mode: Some("clone_new_database"),
        folder_name: folder_name(&target),
        clone_verify_disposition: Some(clone_verification.disposition),
    })
}

fn backup_command(
    action: &str,
    input_path: &Path,
    output_path: &Path,
    profile: BackupProfile,
) -> Result<CliBackupSummary, String> {
    let mut arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("backup"),
        OsString::from(action),
        input_path.as_os_str().to_owned(),
    ];
    if action == "create" {
        arguments.push(OsString::from("--output"));
        arguments.push(output_path.as_os_str().to_owned());
    }
    arguments.extend([
        OsString::from("--profile"),
        OsString::from(profile.cli_label()),
        OsString::from("--audit-scope"),
        OsString::from(profile.audit_scope()),
    ]);
    let expected_status = if action == "create" {
        "created"
    } else {
        "verified"
    };
    let data = run_cli(arguments, "backup")?;
    let summary: CliBackupSummary = serde_json::from_value(data)
        .map_err(|_| "backup service returned an unreadable summary".to_owned())?;
    validate_backup_summary(&summary, expected_status, profile)?;
    Ok(summary)
}

fn restore_command(
    backup_path: &Path,
    authorization_path: &Path,
    destination: &Path,
    profile: BackupProfile,
) -> Result<CliRestoreSummary, String> {
    let arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("restore"),
        OsString::from("clone"),
        backup_path.as_os_str().to_owned(),
        OsString::from("--authorize-with"),
        authorization_path.as_os_str().to_owned(),
        OsString::from("--output"),
        destination.as_os_str().to_owned(),
        OsString::from("--profile"),
        OsString::from(profile.cli_label()),
        OsString::from("--audit-scope"),
        OsString::from(profile.audit_scope()),
    ];
    let data = run_cli(arguments, "restore_clone")?;
    serde_json::from_value(data)
        .map_err(|_| "restore service returned an unreadable summary".to_owned())
}

fn verify_project(path: &Path) -> Result<CliVerifySummary, String> {
    let arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("verify"),
        path.as_os_str().to_owned(),
    ];
    let data = run_cli(arguments, "verify")?;
    serde_json::from_value(data)
        .map_err(|_| "restored project verification returned an unreadable summary".to_owned())
}

fn run_cli(arguments: Vec<OsString>, expected_outcome: &str) -> Result<Value, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = worlddb_cli::cli::run_with(arguments, &mut stdout, &mut stderr);
    let response: Value = serde_json::from_slice(&stdout)
        .map_err(|_| "backup service returned an unreadable report".to_owned())?;
    let outcome = response
        .get("outcome")
        .ok_or_else(|| "backup service returned an unreadable report".to_owned())?;
    let outcome_kind = outcome
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "backup service returned an unreadable report".to_owned())?;
    if exit_code != 0 || outcome_kind == "error" {
        let code = outcome
            .get("data")
            .and_then(|data| data.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("backup_rejected");
        return Err(format!("backup command was rejected ({code})"));
    }
    if outcome_kind != expected_outcome {
        return Err("backup service returned an unexpected report".to_owned());
    }
    outcome
        .get("data")
        .cloned()
        .ok_or_else(|| "backup service returned an unreadable report".to_owned())
}

fn validate_backup_summary(
    summary: &CliBackupSummary,
    expected_status: &str,
    profile: BackupProfile,
) -> Result<(), String> {
    if summary.status != expected_status
        || summary.profile != cli_profile(profile)
        || summary.audit_scope != cli_audit_scope(profile)
        || summary.database_id.is_empty()
        || summary.revision.is_empty()
        || !summary.target_verified
        || summary.source_modified
    {
        return Err("backup result did not match the requested profile and audit scope".to_owned());
    }
    Ok(())
}

fn same_backup(left: &CliBackupSummary, right: &CliBackupSummary) -> bool {
    left.profile == right.profile
        && left.audit_scope == right.audit_scope
        && left.database_id == right.database_id
        && left.revision == right.revision
        && left.item_count == right.item_count
        && left.audit_safe_sequence == right.audit_safe_sequence
        && left.authenticity == right.authenticity
        && left.target_verified
        && right.target_verified
        && !left.source_modified
        && !right.source_modified
}

fn backup_view(
    action: &'static str,
    profile: BackupProfile,
    summary: &CliBackupSummary,
    target_name: String,
) -> BackupOperationView {
    BackupOperationView {
        action,
        profile: profile.display_label(),
        audit_scope: display_audit_scope(profile),
        database_id: Some(summary.database_id.clone()),
        revision: Some(summary.revision.clone()),
        source_database_id: None,
        source_revision: None,
        restored_database_id: None,
        restored_revision: None,
        item_count: summary.item_count.clone(),
        audit_safe_sequence: summary.audit_safe_sequence.clone(),
        authenticity: summary.authenticity.clone(),
        target_verified: summary.target_verified,
        source_modified: summary.source_modified,
        restore_mode: None,
        folder_name: target_name,
        clone_verify_disposition: None,
    }
}

fn cli_profile(profile: BackupProfile) -> &'static str {
    match profile {
        BackupProfile::ExactDatabase => "ExactDatabase",
        BackupProfile::AuditComplete => "AuditComplete",
    }
}

fn cli_audit_scope(profile: BackupProfile) -> &'static str {
    match profile {
        BackupProfile::ExactDatabase => "Excluded",
        BackupProfile::AuditComplete => "Included",
    }
}

fn display_audit_scope(profile: BackupProfile) -> &'static str {
    match profile {
        BackupProfile::ExactDatabase => "Excluded",
        BackupProfile::AuditComplete => "Included",
    }
}

fn new_target(
    parent: &Path,
    protected_project: &Path,
    purpose: &str,
    profile_label: &str,
) -> Result<PathBuf, String> {
    let id = worlddb_core::storage_internal::generate_database_id()
        .map_err(|_| "backup destination identity is unavailable".to_owned())?;
    let target = parent.join(format!("worlddb-{purpose}-{profile_label}-{id}"));
    ensure_disjoint(
        &target,
        protected_project,
        "destination overlaps the source project",
    )?;
    match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(target),
        _ => Err("generated backup destination already exists or cannot be inspected".to_owned()),
    }
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| format!("selected {label} folder is unavailable"))?;
    let file_type = metadata.file_type();
    if !metadata.is_dir() || is_reparse_point(&file_type, &metadata) {
        return Err(format!(
            "selected {label} folder is not a regular directory"
        ));
    }
    fs::canonicalize(path).map_err(|_| format!("selected {label} folder is unavailable"))
}

fn ensure_disjoint(left: &Path, right: &Path, message: &str) -> Result<(), String> {
    if left.starts_with(right) || right.starts_with(left) {
        return Err(message.to_owned());
    }
    Ok(())
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("WorldDB-Backup")
        .to_owned()
}

#[cfg(windows)]
fn is_reparse_point(_file_type: &FileType, metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(file_type: &FileType, _metadata: &std::fs::Metadata) -> bool {
    file_type.is_symlink()
}

#[cfg(test)]
mod tests {
    use super::{
        BackupProfile, BackupRequestV1, CliBackupSummary, same_backup, validate_backup_summary,
    };

    #[test]
    fn profile_binds_the_only_supported_audit_scope() {
        assert_eq!(
            BackupProfile::parse("exact").unwrap().audit_scope(),
            "excluded"
        );
        assert_eq!(
            BackupProfile::parse("audit_complete")
                .unwrap()
                .audit_scope(),
            "included"
        );
        assert!(BackupProfile::parse("ExactDatabase").is_err());
    }

    #[test]
    fn backup_request_rejects_renderer_paths_and_unknown_fields() {
        let request: BackupRequestV1 =
            serde_json::from_str(r#"{"protocol_version":1,"profile":"exact"}"#).unwrap();
        assert_eq!(request.profile, "exact");
        assert!(
            serde_json::from_str::<BackupRequestV1>(
                r#"{"protocol_version":1,"profile":"exact","path":"C:/user/chosen"}"#,
            )
            .is_err()
        );
    }

    #[test]
    fn backup_result_must_match_requested_profile_and_scope() {
        let summary: CliBackupSummary = serde_json::from_str(
            r#"{"status":"created","profile":"ExactDatabase","audit_scope":"Excluded","database_id":"00000000-0000-7000-8000-000000000001","revision":"5","item_count":"8","audit_safe_sequence":null,"authenticity":"NotClaimed","target_verified":true,"source_modified":false}"#,
        )
        .unwrap();
        assert!(validate_backup_summary(&summary, "created", BackupProfile::ExactDatabase).is_ok());
        assert!(
            validate_backup_summary(&summary, "created", BackupProfile::AuditComplete).is_err()
        );
        assert!(
            validate_backup_summary(&summary, "verified", BackupProfile::ExactDatabase).is_err()
        );
        assert!(same_backup(&summary, &summary));
    }
}
