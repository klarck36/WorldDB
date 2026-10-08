use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_TARGETS: usize = 4096;
const MAX_EXTERNAL_COPIES: usize = 4096;
const MAX_IDENTITY_LENGTH: usize = 128;
const MAX_PREVIEW_ITEMS: usize = 128;
const MAX_REPORT_BYTES: u64 = 128 * 1024 * 1024;

const IDENTITY_FAMILIES: &[&str] = &[
    "history-space",
    "layer",
    "perspective",
    "timeline",
    "entity",
    "entity-type",
    "predicate",
    "event-kind",
    "event-role",
    "event-attribute",
];

const EXTERNAL_COPY_KINDS: &[&str] = &[
    "exact-backup",
    "audit-complete-backup",
    "logical-export",
    "sharing-export",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PurgeRequestV1 {
    pub(crate) protocol_version: u16,
    pub(crate) targets: Vec<String>,
    pub(crate) mode: String,
    pub(crate) external_inventory_complete: bool,
    pub(crate) known_external_artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PurgeRunRequestV1 {
    pub(crate) protocol_version: u16,
    pub(crate) plan_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PurgeDiscardRequestV1 {
    pub(crate) protocol_version: u16,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgeDiscardResponseV1 {
    pub(crate) protocol_version: u16,
    pub(crate) discarded: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgePlanResponseV1 {
    pub(crate) protocol_version: u16,
    pub(crate) result: PurgePlanView,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgeRunResponseV1 {
    pub(crate) protocol_version: u16,
    pub(crate) result: PurgeRunView,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgePlanView {
    pub(crate) action: &'static str,
    pub(crate) source_database_id: String,
    pub(crate) source_revision: String,
    pub(crate) mode: String,
    pub(crate) target_count: String,
    pub(crate) target_record_count: String,
    pub(crate) dependant_count: String,
    pub(crate) approval_possible: bool,
    pub(crate) plan_fingerprint: String,
    pub(crate) report_digest: String,
    pub(crate) external_inventory_complete: bool,
    pub(crate) index_inventory_complete: bool,
    pub(crate) target_identities: Vec<String>,
    pub(crate) target_identities_remaining: String,
    pub(crate) target_records: Vec<PurgeRecordPreview>,
    pub(crate) target_records_remaining: String,
    pub(crate) dependants: Vec<PurgeRecordPreview>,
    pub(crate) dependants_remaining: String,
    pub(crate) affected_record_count: String,
    pub(crate) index_generation_count: String,
    pub(crate) index_generations: Vec<PurgeIndexGenerationView>,
    pub(crate) index_generations_remaining: String,
    pub(crate) known_external_artifacts: Vec<PurgeExternalArtifactView>,
    pub(crate) index_families_to_rebuild: Vec<String>,
    pub(crate) writes_database: bool,
    pub(crate) source_modified: bool,
    pub(crate) secure_erase_claimed: bool,
    pub(crate) report_name: String,
    pub(crate) destination_name: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgeRecordPreview {
    pub(crate) identity: String,
    pub(crate) content_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PurgeExternalArtifactView {
    pub(crate) kind: String,
    pub(crate) digest: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgeIndexGenerationView {
    pub(crate) family: String,
    pub(crate) generation_id: String,
    pub(crate) file_digest: String,
    pub(crate) current: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PurgeRunView {
    pub(crate) action: &'static str,
    pub(crate) source_database_id: String,
    pub(crate) destination_database_id: String,
    pub(crate) source_revision: String,
    pub(crate) destination_revision: String,
    pub(crate) mode: String,
    pub(crate) removed_record_count: String,
    pub(crate) plan_fingerprint: String,
    pub(crate) report_digest: String,
    pub(crate) operation_id: String,
    pub(crate) audit_record_id: String,
    pub(crate) external_inventory_complete: bool,
    pub(crate) target_verified: bool,
    pub(crate) report_persisted: bool,
    pub(crate) source_modified: bool,
    pub(crate) secure_erase_claimed: bool,
    pub(crate) destination_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PurgeDraft {
    source_path: PathBuf,
    destination_path: PathBuf,
    request: PurgeRequestV1,
    view: PurgePlanView,
}

impl PurgeDraft {
    pub(crate) fn matches_fingerprint(&self, fingerprint: &str) -> bool {
        self.view.plan_fingerprint == fingerprint
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliPurgePlanSummary {
    status: String,
    source_database_id: String,
    source_revision: String,
    mode: String,
    target_count: String,
    target_record_count: String,
    dependant_count: String,
    approval_possible: bool,
    plan_fingerprint: String,
    report_digest: String,
    external_inventory_complete: bool,
    writes_database: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliPurgeRunSummary {
    status: String,
    source_database_id: String,
    destination_database_id: String,
    source_revision: String,
    destination_revision: String,
    mode: String,
    removed_record_count: String,
    plan_fingerprint: String,
    report_digest: String,
    operation_id: String,
    audit_record_id: String,
    external_inventory_complete: bool,
    target_verified: bool,
    report_persisted: bool,
    source_modified: bool,
    secure_erase_claimed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PurgePlanReport {
    schema: String,
    source_database_id: String,
    source_revision: String,
    source_artifact_digest: String,
    mode: String,
    plan_fingerprint: String,
    approval_possible: bool,
    external_inventory_complete: bool,
    index_inventory_complete: bool,
    targets: Vec<String>,
    target_records: Vec<CliPurgeRecord>,
    dependants: Vec<CliPurgeRecord>,
    affected_records: Vec<CliPurgeRecord>,
    index_generations: Vec<CliPurgeIndexGeneration>,
    known_external_artifacts: Vec<PurgeExternalArtifactView>,
    index_families_to_rebuild: Vec<String>,
    secure_erase_claimed: bool,
}

#[cfg(test)]
static NEXT_PURGE_FUZZ_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
struct PurgeFuzzFile(PathBuf);

#[cfg(test)]
impl Drop for PurgeFuzzFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
pub(crate) fn fuzz_purge_report(bytes: &[u8]) -> Result<bool, String> {
    let report = serde_json::from_slice::<PurgePlanReport>(bytes).is_ok();
    let summary = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| decode_summary::<PurgePlanReport>(value, "purge").ok())
        .is_some();
    let sequence = NEXT_PURGE_FUZZ_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "worlddb-desktop-purge-fuzz-{}-{sequence}.report",
        std::process::id()
    ));
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let file = PurgeFuzzFile(path);
    let bounded = read_bounded_report(&file.0).is_ok();
    Ok((report || summary) && bounded)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliPurgeRecord {
    identity: String,
    content_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliPurgeIndexGeneration {
    family: String,
    generation_id: String,
    file_digest: String,
    current: bool,
}

pub(crate) fn validate_request(request: &PurgeRequestV1) -> Result<(), String> {
    if request.protocol_version != 1
        || request.targets.is_empty()
        || request.targets.len() > MAX_TARGETS
        || !matches!(request.mode.as_str(), "reject-if-referenced" | "cascade")
        || request
            .targets
            .iter()
            .any(|target| target.len() > MAX_IDENTITY_LENGTH || !is_typed_identity(target))
        || request
            .targets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != request.targets.len()
        || request.known_external_artifacts.len() > MAX_EXTERNAL_COPIES
        || request
            .known_external_artifacts
            .iter()
            .any(|artifact| !is_known_external_artifact(artifact))
        || request
            .known_external_artifacts
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != request.known_external_artifacts.len()
    {
        return Err("invalid purge request".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_run_request(request: &PurgeRunRequestV1) -> Result<(), String> {
    if request.protocol_version != 1 || !is_lower_hex_digest(&request.plan_fingerprint) {
        return Err("invalid purge confirmation request".to_owned());
    }
    Ok(())
}

pub(crate) fn plan(
    source_path: &Path,
    destination_parent: &Path,
    report_path: &Path,
    request: PurgeRequestV1,
) -> Result<(PurgePlanView, PurgeDraft), String> {
    validate_request(&request)?;
    let source = canonical_directory(source_path, "purge source")?;
    let parent = canonical_directory(destination_parent, "purge destination parent")?;
    let destination = new_destination(&parent)?;
    let report_name = safe_file_name(report_path)?;
    let destination_name = safe_file_name(&destination)?;
    let data = run_cli(
        purge_arguments(
            "plan",
            &source,
            &destination,
            Some(report_path),
            &request,
            None,
        ),
        "purge_plan",
    )?;
    let summary: CliPurgePlanSummary = decode_summary(data, "purge plan")?;
    validate_plan_summary(&summary, &request)?;
    let report_bytes = read_bounded_report(report_path)?;
    let report_digest = blake3::hash(&report_bytes).to_hex().to_string();
    if report_digest != summary.report_digest {
        return Err("purge plan report digest does not match the CLI receipt".to_owned());
    }
    let report: PurgePlanReport = serde_json::from_slice(&report_bytes)
        .map_err(|_| "purge plan report is unreadable".to_owned())?;
    validate_plan_report(&report, &summary, &request)?;
    let view = plan_view(summary, report, report_name, destination_name);
    let draft = PurgeDraft {
        source_path: source,
        destination_path: destination,
        request,
        view: view.clone(),
    };
    Ok((view, draft))
}

pub(crate) fn run(draft: PurgeDraft, confirmation: &str) -> Result<PurgeRunView, String> {
    if !is_lower_hex_digest(confirmation) || confirmation != draft.view.plan_fingerprint {
        return Err("purge confirmation does not match the reviewed plan".to_owned());
    }
    let data = run_cli(
        purge_arguments(
            "run",
            &draft.source_path,
            &draft.destination_path,
            None,
            &draft.request,
            Some(confirmation),
        ),
        "purge_run",
    )?;
    let summary: CliPurgeRunSummary = decode_summary(data, "purge run")?;
    if summary.status != "completed"
        || summary.source_database_id != draft.view.source_database_id
        || summary.destination_database_id == summary.source_database_id
        || summary.source_revision != draft.view.source_revision
        || summary.mode != draft.view.mode
        || summary.plan_fingerprint != confirmation
        || summary.external_inventory_complete != draft.request.external_inventory_complete
        || !is_nonempty_public_id(&summary.destination_database_id)
        || !summary.target_verified
        || !summary.report_persisted
        || summary.source_modified
        || summary.secure_erase_claimed
        || !is_canonical_count(&summary.removed_record_count)
        || !is_lower_hex_digest(&summary.report_digest)
        || !is_nonempty_public_id(&summary.operation_id)
        || !is_nonempty_public_id(&summary.audit_record_id)
    {
        return Err(
            "purge result did not match the reviewed plan or target verification".to_owned(),
        );
    }
    let persisted_report = read_bounded_report(&draft.destination_path.join("PURGE_REPORT"))?;
    if blake3::hash(&persisted_report).to_hex().as_str() != summary.report_digest {
        return Err("published purge report digest does not match the CLI receipt".to_owned());
    }
    Ok(PurgeRunView {
        action: "completed",
        source_database_id: summary.source_database_id,
        destination_database_id: summary.destination_database_id,
        source_revision: summary.source_revision,
        destination_revision: summary.destination_revision,
        mode: summary.mode,
        removed_record_count: summary.removed_record_count,
        plan_fingerprint: summary.plan_fingerprint,
        report_digest: summary.report_digest,
        operation_id: summary.operation_id,
        audit_record_id: summary.audit_record_id,
        external_inventory_complete: summary.external_inventory_complete,
        target_verified: summary.target_verified,
        report_persisted: summary.report_persisted,
        source_modified: summary.source_modified,
        secure_erase_claimed: summary.secure_erase_claimed,
        destination_name: draft.view.destination_name,
    })
}

fn validate_plan_summary(
    summary: &CliPurgePlanSummary,
    request: &PurgeRequestV1,
) -> Result<(), String> {
    if summary.status != "previewed"
        || summary.mode != request.mode
        || summary.external_inventory_complete != request.external_inventory_complete
        || summary.writes_database
        || !is_nonempty_public_id(&summary.source_database_id)
        || !is_canonical_count(&summary.source_revision)
        || !is_canonical_count(&summary.target_count)
        || !is_canonical_count(&summary.target_record_count)
        || !is_canonical_count(&summary.dependant_count)
        || summary.target_count != request.targets.len().to_string()
        || !is_lower_hex_digest(&summary.plan_fingerprint)
        || !is_lower_hex_digest(&summary.report_digest)
    {
        return Err("purge plan summary is invalid or incomplete".to_owned());
    }
    Ok(())
}

fn validate_plan_report(
    report: &PurgePlanReport,
    summary: &CliPurgePlanSummary,
    request: &PurgeRequestV1,
) -> Result<(), String> {
    let mut report_targets = report.targets.clone();
    report_targets.sort();
    let mut request_targets = request.targets.clone();
    request_targets.sort();
    let affected_count = report
        .target_records
        .len()
        .saturating_add(report.dependants.len());
    let known_copy_count = report.known_external_artifacts.len();
    let mut report_copies = report
        .known_external_artifacts
        .iter()
        .map(|artifact| format!("{}:{}", artifact.kind, artifact.digest))
        .collect::<Vec<_>>();
    report_copies.sort();
    let mut request_copies = request.known_external_artifacts.clone();
    request_copies.sort();
    if report.schema != "WorldDB.PurgePlanReport.v1"
        || report.source_database_id != summary.source_database_id
        || report.source_revision != summary.source_revision
        || !is_lower_hex_digest(&report.source_artifact_digest)
        || report.mode != summary.mode
        || report.plan_fingerprint != summary.plan_fingerprint
        || report.approval_possible != summary.approval_possible
        || report.external_inventory_complete != summary.external_inventory_complete
        || !report.index_inventory_complete
        || report.secure_erase_claimed
        || report_targets != request_targets
        || report.targets.len().to_string() != summary.target_count
        || report.target_records.len().to_string() != summary.target_record_count
        || report.dependants.len().to_string() != summary.dependant_count
        || report.affected_records.len() != affected_count
        || known_copy_count != request.known_external_artifacts.len()
        || report_copies != request_copies
        || report
            .target_records
            .iter()
            .chain(report.dependants.iter())
            .chain(report.affected_records.iter())
            .any(|record| !is_lower_hex_digest(&record.content_digest))
        || report.index_generations.iter().any(|generation| {
            generation.family.is_empty()
                || generation.generation_id.is_empty()
                || !is_lower_hex_digest(&generation.file_digest)
        })
        || report.known_external_artifacts.iter().any(|artifact| {
            !is_known_external_artifact(&format!("{}:{}", artifact.kind, artifact.digest))
        })
    {
        return Err("purge plan report does not match the approved inventory".to_owned());
    }
    Ok(())
}

fn plan_view(
    summary: CliPurgePlanSummary,
    report: PurgePlanReport,
    report_name: String,
    destination_name: String,
) -> PurgePlanView {
    let target_identities_remaining = report.targets.len().saturating_sub(MAX_PREVIEW_ITEMS);
    let target_records_remaining = report
        .target_records
        .len()
        .saturating_sub(MAX_PREVIEW_ITEMS);
    let dependants_remaining = report.dependants.len().saturating_sub(MAX_PREVIEW_ITEMS);
    let index_generations_remaining = report
        .index_generations
        .len()
        .saturating_sub(MAX_PREVIEW_ITEMS);
    let index_generation_count = report.index_generations.len();
    let index_generations = report
        .index_generations
        .into_iter()
        .take(MAX_PREVIEW_ITEMS)
        .map(|generation| PurgeIndexGenerationView {
            family: generation.family,
            generation_id: generation.generation_id,
            file_digest: generation.file_digest,
            current: generation.current,
        })
        .collect();
    PurgePlanView {
        action: "previewed",
        source_database_id: summary.source_database_id,
        source_revision: summary.source_revision,
        mode: summary.mode,
        target_count: summary.target_count,
        target_record_count: summary.target_record_count,
        dependant_count: summary.dependant_count,
        approval_possible: summary.approval_possible,
        plan_fingerprint: summary.plan_fingerprint,
        report_digest: summary.report_digest,
        external_inventory_complete: summary.external_inventory_complete,
        index_inventory_complete: report.index_inventory_complete,
        target_identities: report.targets.into_iter().take(MAX_PREVIEW_ITEMS).collect(),
        target_identities_remaining: target_identities_remaining.to_string(),
        target_records: preview_records(report.target_records),
        target_records_remaining: target_records_remaining.to_string(),
        dependants: preview_records(report.dependants),
        dependants_remaining: dependants_remaining.to_string(),
        affected_record_count: report.affected_records.len().to_string(),
        index_generation_count: index_generation_count.to_string(),
        index_generations,
        index_generations_remaining: index_generations_remaining.to_string(),
        known_external_artifacts: report.known_external_artifacts,
        index_families_to_rebuild: report.index_families_to_rebuild,
        writes_database: false,
        source_modified: false,
        secure_erase_claimed: false,
        report_name,
        destination_name,
    }
}

fn preview_records(records: Vec<CliPurgeRecord>) -> Vec<PurgeRecordPreview> {
    records
        .into_iter()
        .take(MAX_PREVIEW_ITEMS)
        .map(|record| PurgeRecordPreview {
            identity: record.identity,
            content_digest: record.content_digest,
        })
        .collect()
}

fn purge_arguments(
    action: &str,
    source: &Path,
    destination: &Path,
    report_path: Option<&Path>,
    request: &PurgeRequestV1,
    confirmation: Option<&str>,
) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("purge"),
        OsString::from(action),
        source.as_os_str().to_owned(),
        OsString::from("--destination"),
        destination.as_os_str().to_owned(),
    ];
    if let Some(report_path) = report_path {
        arguments.push(OsString::from("--report"));
        arguments.push(report_path.as_os_str().to_owned());
    }
    if let Some(confirmation) = confirmation {
        arguments.push(OsString::from("--confirm-plan"));
        arguments.push(OsString::from(confirmation));
    }
    for target in &request.targets {
        arguments.push(OsString::from("--target"));
        arguments.push(OsString::from(target));
    }
    arguments.extend([
        OsString::from("--mode"),
        OsString::from(&request.mode),
        OsString::from("--external-inventory"),
        OsString::from(if request.external_inventory_complete {
            "complete"
        } else {
            "incomplete"
        }),
    ]);
    for artifact in &request.known_external_artifacts {
        arguments.push(OsString::from("--known-copy"));
        arguments.push(OsString::from(artifact));
    }
    arguments
}

fn run_cli(arguments: Vec<OsString>, expected_type: &str) -> Result<Value, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = worlddb_cli::cli::run_with(arguments, &mut stdout, &mut stderr);
    let response: Value = serde_json::from_slice(&stdout)
        .map_err(|_| "purge service returned an unreadable report".to_owned())?;
    let outcome = response
        .get("outcome")
        .ok_or_else(|| "purge service returned an unreadable report".to_owned())?;
    let outcome_type = outcome
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "purge service returned an unreadable report".to_owned())?;
    if exit_code != 0 || outcome_type == "error" {
        let code = outcome
            .get("data")
            .and_then(|data| data.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("purge_rejected");
        return Err(format!("purge operation was rejected ({code})"));
    }
    if outcome_type != expected_type {
        return Err("purge service returned an unexpected report".to_owned());
    }
    outcome
        .get("data")
        .cloned()
        .ok_or_else(|| "purge service returned an unreadable report".to_owned())
}

fn decode_summary<T: for<'de> Deserialize<'de>>(data: Value, label: &str) -> Result<T, String> {
    serde_json::from_value(data)
        .map_err(|_| format!("{label} service returned an unreadable summary"))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("{label} must be a real directory"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(format!("{label} cannot be a reparse point"));
        }
    }
    fs::canonicalize(path).map_err(|_| format!("{label} cannot be resolved"))
}

fn new_destination(parent: &Path) -> Result<PathBuf, String> {
    let ticks = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system time is unavailable".to_owned())?
        .as_nanos();
    for suffix in 0..32_u8 {
        let name = format!("WorldDB-purge-{}-{ticks}-{suffix}", std::process::id());
        let candidate = parent.join(name);
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => {}
            Err(_) => return Err("a new purge destination could not be checked".to_owned()),
        }
    }
    Err("a new purge destination could not be reserved".to_owned())
}

fn safe_file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && name.chars().all(|character| !character.is_control()))
        .map(str::to_owned)
        .ok_or_else(|| "selected file name is unavailable".to_owned())
}

fn read_bounded_report(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "purge report is unavailable".to_owned())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_REPORT_BYTES
    {
        return Err("purge report is not a bounded regular file".to_owned());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("purge report is not a bounded regular file".to_owned());
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "purge report could not be read".to_owned())?;
    let file_metadata = file
        .metadata()
        .map_err(|_| "purge report could not be read".to_owned())?;
    if !file_metadata.is_file() || file_metadata.len() > MAX_REPORT_BYTES {
        return Err("purge report is not a bounded regular file".to_owned());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file_metadata.file_attributes() & 0x400 != 0 {
            return Err("purge report is not a bounded regular file".to_owned());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "purge report could not be read".to_owned())?;
    if u64::try_from(bytes.len()).map_err(|_| "purge report exceeds its size limit".to_owned())?
        > MAX_REPORT_BYTES
    {
        return Err("purge report exceeds its size limit".to_owned());
    }
    Ok(bytes)
}

fn is_typed_identity(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [family, uuid] => IDENTITY_FAMILIES.contains(family) && is_canonical_uuid(uuid),
        ["record", tag, uuid] => {
            tag.parse::<u32>()
                .is_ok_and(|parsed| parsed.to_string() == *tag)
                && is_canonical_uuid(uuid)
        }
        _ => false,
    }
}

fn is_known_external_artifact(value: &str) -> bool {
    let Some((kind, digest)) = value.split_once(':') else {
        return false;
    };
    EXTERNAL_COPY_KINDS.contains(&kind) && is_lower_hex_digest(digest)
}

fn is_canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

fn is_canonical_count(value: &str) -> bool {
    value == "0"
        || (!value.is_empty()
            && !value.starts_with('0')
            && value.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_nonempty_public_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| !byte.is_ascii_control())
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_request() -> PurgeRequestV1 {
        PurgeRequestV1 {
            protocol_version: 1,
            targets: vec!["entity:00000000-0000-0000-0000-000000000000".to_owned()],
            mode: "cascade".to_owned(),
            external_inventory_complete: false,
            known_external_artifacts: vec![format!("logical-export:{}", "a".repeat(64))],
        }
    }

    #[test]
    fn purge_request_rejects_renderer_paths() {
        let request = serde_json::from_str::<PurgeRequestV1>(
            r#"{"protocol_version":1,"targets":["entity:00000000-0000-0000-0000-000000000000"],"mode":"cascade","external_inventory_complete":false,"known_external_artifacts":[],"source_path":"C:/secret","destination_path":"C:/secret"}"#,
        );
        assert!(request.is_err());
    }

    #[test]
    fn purge_request_bounds_and_validates_targets_and_copy_digests() {
        assert!(validate_request(&valid_request()).is_ok());
        let invalid_target = PurgeRequestV1 {
            targets: vec!["entity:UPPERCASE".to_owned()],
            ..valid_request()
        };
        assert!(validate_request(&invalid_target).is_err());
        let invalid_copy = PurgeRequestV1 {
            known_external_artifacts: vec!["logical-export:ABC".to_owned()],
            ..valid_request()
        };
        assert!(validate_request(&invalid_copy).is_err());
    }

    #[test]
    fn report_preview_does_not_expose_paths_or_secure_erasure_claims() {
        let report = serde_json::from_str::<PurgePlanReport>(
            r#"{"schema":"WorldDB.PurgePlanReport.v1","source_database_id":"db-a","source_revision":"1","source_artifact_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","mode":"cascade","plan_fingerprint":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","approval_possible":true,"external_inventory_complete":false,"index_inventory_complete":true,"targets":["entity:00000000-0000-0000-0000-000000000000"],"target_records":[],"dependants":[],"affected_records":[],"index_generations":[],"known_external_artifacts":[],"index_families_to_rebuild":[],"secure_erase_claimed":false,"source_path":"C:/secret"}"#,
        );
        assert!(report.is_err());
    }

    #[test]
    fn run_confirmation_must_be_a_lowercase_plan_fingerprint() {
        assert!(
            validate_run_request(&PurgeRunRequestV1 {
                protocol_version: 1,
                plan_fingerprint: "c".repeat(64),
            })
            .is_ok()
        );
        assert!(
            validate_run_request(&PurgeRunRequestV1 {
                protocol_version: 1,
                plan_fingerprint: "C".repeat(64),
            })
            .is_err()
        );
    }

    #[test]
    fn discard_request_rejects_renderer_selected_paths() {
        let request = serde_json::from_str::<PurgeDiscardRequestV1>(
            r#"{"protocol_version":1,"destination_path":"C:/renderer/path"}"#,
        );
        assert!(request.is_err());
    }
}
