use std::{ffi::OsString, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_MAPPING_ROWS: usize = 4096;
const MAX_MAPPING_LENGTH: usize = 256;
const MAX_REVISION_LENGTH: usize = 20;

const EXPORTABLE_RECORD_CLASSES: &[&str] = &[
    "HistorySpaceDefinition",
    "Entity",
    "EntityRetirement",
    "PerspectiveDefinitionRevision",
    "PerspectiveRetirement",
    "LayerDefinition",
    "LayerSchemaSnapshot",
    "EntityTypeDefinition",
    "PredicateDefinition",
    "EventKindDefinition",
    "TimelineDefinition",
    "TimeUnitDefinition",
    "Assertion",
    "AssertionValidityClosure",
    "AssertionRetraction",
    "Mask",
    "MaskValidityClosure",
    "MaskRetraction",
    "ReplacementBoundary",
    "ReplacementBoundaryValidityClosure",
    "ReplacementBoundaryRetraction",
    "ArchiveTransition",
    "Event",
    "EventMask",
    "EventSpanClosure",
    "EventRetraction",
    "EventMaskRetraction",
    "EventRelation",
    "EventRelationRetraction",
    "Source",
    "Evidence",
    "Provenance",
    "EvidenceRetraction",
    "ProvenanceRetraction",
    "TransferLineage",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportRequestV1 {
    pub(crate) protocol_version: u16,
    pub(crate) kind: String,
    pub(crate) from_revision: String,
    pub(crate) through_revision: String,
    pub(crate) history_spaces: Vec<String>,
    pub(crate) record_classes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportPlanRequestV1 {
    pub(crate) protocol_version: u16,
    pub(crate) mappings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportPrepareRequestV1 {
    pub(crate) protocol_version: u16,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ExportResponseV1 {
    pub(crate) protocol_version: u16,
    pub(crate) result: ExportOperationView,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImportPlanResponseV1 {
    pub(crate) protocol_version: u16,
    pub(crate) result: ImportPlanView,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImportPrepareResponseV1 {
    pub(crate) protocol_version: u16,
    pub(crate) result: ImportPrepareView,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ExportOperationView {
    pub(crate) action: &'static str,
    pub(crate) format: &'static str,
    pub(crate) from_revision: String,
    pub(crate) through_revision: String,
    pub(crate) history_spaces: Vec<String>,
    pub(crate) record_classes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) database_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snapshot_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) record_count: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) included_record_count: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) omission_manifest: Option<OmissionManifestView>,
    pub(crate) omission_counts_disclosed: bool,
    pub(crate) source_audit_committed: bool,
    pub(crate) source_modified: bool,
    pub(crate) output_name: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImportPlanView {
    pub(crate) action: &'static str,
    pub(crate) source_database_id: String,
    pub(crate) destination_database_id: String,
    pub(crate) mapping_count: String,
    pub(crate) plan_digest: String,
    pub(crate) writes_database: bool,
    pub(crate) plan_name: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImportPrepareView {
    pub(crate) action: &'static str,
    pub(crate) source_database_id: String,
    pub(crate) destination_database_id: String,
    pub(crate) stream_fingerprint: String,
    pub(crate) mapping_count: String,
    pub(crate) record_count: String,
    pub(crate) from_revision: String,
    pub(crate) through_revision: String,
    pub(crate) history_space_count: String,
    pub(crate) record_class_count: String,
    pub(crate) omission_manifest: OmissionManifestView,
    pub(crate) writes_database: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OmissionManifestView {
    pub(crate) complete: bool,
    pub(crate) record_classes_omitted: String,
    pub(crate) storage_classes_omitted: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliExportScope {
    from_revision: String,
    through_revision: String,
    history_space_count: String,
    history_spaces: Vec<String>,
    record_class_count: String,
    record_classes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliOmissionManifest {
    complete: bool,
    record_classes_omitted: String,
    storage_classes_omitted: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliLogicalExportSummary {
    status: String,
    database_id: String,
    snapshot_revision: String,
    scope: CliExportScope,
    record_count: String,
    omission_manifest: CliOmissionManifest,
    source_modified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliSharingExportSummary {
    status: String,
    scope: CliExportScope,
    included_record_count: String,
    omission_counts_disclosed: bool,
    source_audit_committed: bool,
    source_modified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliImportPlanSummary {
    status: String,
    source_database_id: String,
    destination_database_id: String,
    mapping_count: String,
    plan_digest: String,
    writes_database: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliImportPrepareSummary {
    status: String,
    source_database_id: String,
    destination_database_id: String,
    stream_fingerprint: String,
    mapping_count: String,
    record_count: String,
    scope: CliImportScope,
    omission_manifest: CliOmissionManifest,
    writes_database: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CliImportScope {
    from_revision: String,
    through_revision: String,
    history_space_count: String,
    record_class_count: String,
}

pub(crate) fn export(
    source_path: &Path,
    output_path: &Path,
    request: ExportRequestV1,
) -> Result<ExportOperationView, String> {
    validate_export_request(&request)?;
    let sharing = request.kind == "sharing";
    let expected_from_revision = request.from_revision.clone();
    let expected_through_revision = request.through_revision.clone();
    let format = if sharing {
        "SharingExport"
    } else {
        "LogicalExport"
    };
    let cli_kind = if sharing { "share" } else { "logical" };
    let mut arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("export"),
        OsString::from(cli_kind),
        source_path.as_os_str().to_owned(),
        OsString::from("--output"),
        output_path.as_os_str().to_owned(),
        OsString::from("--from"),
        OsString::from(&request.from_revision),
        OsString::from("--through"),
        OsString::from(&request.through_revision),
    ];
    for id in &request.history_spaces {
        arguments.push(OsString::from("--history-space"));
        arguments.push(OsString::from(id));
    }
    for class in &request.record_classes {
        arguments.push(OsString::from("--class"));
        arguments.push(OsString::from(class));
    }

    let output_name = safe_file_name(output_path)?;
    let expected_type = if sharing {
        "sharing_export"
    } else {
        "logical_export"
    };
    let data = run_cli(arguments, expected_type)?;
    let mut selected_history_spaces = request.history_spaces;
    selected_history_spaces.sort();
    let mut selected_record_classes = request.record_classes;
    selected_record_classes.sort();

    if sharing {
        let summary: CliSharingExportSummary = decode_summary(data, "sharing export")?;
        if summary.status != "completed"
            || summary.omission_counts_disclosed
            || !summary.source_audit_committed
            || !summary.source_modified
            || !is_canonical_count(&summary.included_record_count)
        {
            return Err("sharing export returned an invalid or incomplete result".to_owned());
        }
        validate_scope(
            &summary.scope,
            &expected_from_revision,
            &expected_through_revision,
            &selected_history_spaces,
            &selected_record_classes,
        )?;
        return Ok(ExportOperationView {
            action: "completed",
            format,
            from_revision: summary.scope.from_revision,
            through_revision: summary.scope.through_revision,
            history_spaces: summary.scope.history_spaces,
            record_classes: summary.scope.record_classes,
            database_id: None,
            snapshot_revision: None,
            record_count: None,
            included_record_count: Some(summary.included_record_count),
            omission_manifest: None,
            omission_counts_disclosed: false,
            source_audit_committed: true,
            source_modified: true,
            output_name,
        });
    }

    let summary: CliLogicalExportSummary = decode_summary(data, "logical export")?;
    if summary.status != "completed"
        || !summary.omission_manifest.complete
        || summary.source_modified
        || !is_canonical_count(&summary.record_count)
    {
        return Err("logical export returned an invalid or incomplete result".to_owned());
    }
    validate_scope(
        &summary.scope,
        &request.from_revision,
        &request.through_revision,
        &selected_history_spaces,
        &selected_record_classes,
    )?;
    Ok(ExportOperationView {
        action: "completed",
        format,
        from_revision: summary.scope.from_revision,
        through_revision: summary.scope.through_revision,
        history_spaces: summary.scope.history_spaces,
        record_classes: summary.scope.record_classes,
        database_id: Some(summary.database_id),
        snapshot_revision: Some(summary.snapshot_revision),
        record_count: Some(summary.record_count),
        included_record_count: None,
        omission_manifest: Some(omission_view(summary.omission_manifest)),
        omission_counts_disclosed: true,
        source_audit_committed: false,
        source_modified: false,
        output_name,
    })
}

pub(crate) fn create_import_plan(
    destination_path: &Path,
    input_path: &Path,
    output_path: &Path,
    request: ImportPlanRequestV1,
) -> Result<ImportPlanView, String> {
    validate_plan_request(&request)?;
    let mut arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("import"),
        OsString::from("plan"),
        destination_path.as_os_str().to_owned(),
        OsString::from("--input"),
        input_path.as_os_str().to_owned(),
        OsString::from("--output"),
        output_path.as_os_str().to_owned(),
    ];
    for mapping in request.mappings {
        arguments.push(OsString::from("--map"));
        arguments.push(OsString::from(mapping));
    }
    let output_name = safe_file_name(output_path)?;
    let data = run_cli(arguments, "import_plan")?;
    let summary: CliImportPlanSummary = decode_summary(data, "import plan")?;
    if summary.status != "created"
        || summary.writes_database
        || !is_canonical_count(&summary.mapping_count)
        || !is_lower_hex_digest(&summary.plan_digest)
        || !is_nonempty_public_id(&summary.source_database_id)
        || !is_nonempty_public_id(&summary.destination_database_id)
    {
        return Err("import plan returned an invalid or incomplete result".to_owned());
    }
    Ok(ImportPlanView {
        action: "plan_created",
        source_database_id: summary.source_database_id,
        destination_database_id: summary.destination_database_id,
        mapping_count: summary.mapping_count,
        plan_digest: summary.plan_digest,
        writes_database: false,
        plan_name: output_name,
    })
}

pub(crate) fn prepare_import(
    destination_path: &Path,
    input_path: &Path,
    plan_path: &Path,
    request: ImportPrepareRequestV1,
) -> Result<ImportPrepareView, String> {
    if request.protocol_version != 1 {
        return Err("unsupported IPC protocol".to_owned());
    }
    let arguments = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("import"),
        OsString::from("prepare"),
        destination_path.as_os_str().to_owned(),
        OsString::from("--input"),
        input_path.as_os_str().to_owned(),
        OsString::from("--plan-file"),
        plan_path.as_os_str().to_owned(),
    ];
    let data = run_cli(arguments, "import_prepare")?;
    let summary: CliImportPrepareSummary = decode_summary(data, "import prepare")?;
    if summary.status != "prepared"
        || summary.writes_database
        || !summary.omission_manifest.complete
        || !is_canonical_count(&summary.mapping_count)
        || !is_canonical_count(&summary.record_count)
        || !is_canonical_revision(&summary.scope.from_revision)
        || !is_canonical_revision(&summary.scope.through_revision)
        || !is_canonical_count(&summary.scope.history_space_count)
        || !is_canonical_count(&summary.scope.record_class_count)
        || !is_lower_hex_digest(&summary.stream_fingerprint)
        || !is_nonempty_public_id(&summary.source_database_id)
        || !is_nonempty_public_id(&summary.destination_database_id)
    {
        return Err("import preparation returned an invalid or incomplete result".to_owned());
    }
    Ok(ImportPrepareView {
        action: "prepared",
        source_database_id: summary.source_database_id,
        destination_database_id: summary.destination_database_id,
        stream_fingerprint: summary.stream_fingerprint,
        mapping_count: summary.mapping_count,
        record_count: summary.record_count,
        from_revision: summary.scope.from_revision,
        through_revision: summary.scope.through_revision,
        history_space_count: summary.scope.history_space_count,
        record_class_count: summary.scope.record_class_count,
        omission_manifest: omission_view(summary.omission_manifest),
        writes_database: false,
    })
}

pub(crate) fn validate_export_request(request: &ExportRequestV1) -> Result<(), String> {
    let from_revision = request.from_revision.parse::<u64>().ok();
    let through_revision = request.through_revision.parse::<u64>().ok();
    if request.protocol_version != 1
        || !matches!(request.kind.as_str(), "logical" | "sharing")
        || !is_canonical_revision(&request.from_revision)
        || !is_canonical_revision(&request.through_revision)
        || from_revision
            .zip(through_revision)
            .is_none_or(|(from, through)| from > through)
        || request.history_spaces.is_empty()
        || request.history_spaces.len() > 65_536
        || request.record_classes.is_empty()
        || request.record_classes.len() > EXPORTABLE_RECORD_CLASSES.len()
        || request
            .history_spaces
            .iter()
            .any(|value| !is_canonical_worlddb_uuid(value))
        || request
            .history_spaces
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != request.history_spaces.len()
        || request
            .record_classes
            .iter()
            .any(|value| !EXPORTABLE_RECORD_CLASSES.contains(&value.as_str()))
        || request
            .record_classes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != request.record_classes.len()
        || (request.kind == "logical"
            && !request
                .record_classes
                .iter()
                .any(|kind| kind == "HistorySpaceDefinition"))
    {
        return Err("invalid export request".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_plan_request(request: &ImportPlanRequestV1) -> Result<(), String> {
    if request.protocol_version != 1
        || request.mappings.len() > MAX_MAPPING_ROWS
        || request.mappings.iter().any(|mapping| {
            mapping.is_empty()
                || mapping.len() > MAX_MAPPING_LENGTH
                || mapping.chars().any(char::is_control)
                || mapping.matches('=').count() != 1
        })
    {
        return Err("invalid import plan request".to_owned());
    }
    Ok(())
}

fn validate_scope(
    actual: &CliExportScope,
    expected_from_revision: &str,
    expected_through_revision: &str,
    expected_history_spaces: &[String],
    expected_record_classes: &[String],
) -> Result<(), String> {
    let mut actual_history_spaces = actual.history_spaces.clone();
    actual_history_spaces.sort();
    let mut actual_record_classes = actual.record_classes.clone();
    actual_record_classes.sort();
    if actual.from_revision != expected_from_revision
        || actual.through_revision != expected_through_revision
        || actual.history_space_count != expected_history_spaces.len().to_string()
        || actual_history_spaces != expected_history_spaces
        || actual.record_class_count != expected_record_classes.len().to_string()
        || actual_record_classes != expected_record_classes
    {
        return Err("export result did not match the requested scope".to_owned());
    }
    Ok(())
}

fn omission_view(manifest: CliOmissionManifest) -> OmissionManifestView {
    OmissionManifestView {
        complete: manifest.complete,
        record_classes_omitted: manifest.record_classes_omitted,
        storage_classes_omitted: manifest.storage_classes_omitted,
    }
}

fn run_cli(arguments: Vec<OsString>, expected_type: &str) -> Result<Value, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = worlddb_cli::cli::run_with(arguments, &mut stdout, &mut stderr);
    let response: Value = serde_json::from_slice(&stdout)
        .map_err(|_| "export/import service returned an unreadable report".to_owned())?;
    let outcome = response
        .get("outcome")
        .ok_or_else(|| "export/import service returned an unreadable report".to_owned())?;
    let outcome_type = outcome
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "export/import service returned an unreadable report".to_owned())?;
    if exit_code != 0 || outcome_type == "error" {
        let code = outcome
            .get("data")
            .and_then(|data| data.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("export_import_rejected");
        return Err(format!("export/import operation was rejected ({code})"));
    }
    if outcome_type != expected_type {
        return Err("export/import service returned an unexpected report".to_owned());
    }
    outcome
        .get("data")
        .cloned()
        .ok_or_else(|| "export/import service returned an unreadable report".to_owned())
}

fn decode_summary<T: for<'de> Deserialize<'de>>(data: Value, label: &str) -> Result<T, String> {
    serde_json::from_value(data)
        .map_err(|_| format!("{label} service returned an unreadable summary"))
}

fn safe_file_name(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && name.chars().all(|character| !character.is_control()))
        .ok_or_else(|| "selected file name is unavailable".to_owned())?;
    Ok(name.to_owned())
}

fn is_canonical_revision(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REVISION_LENGTH
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn is_canonical_count(value: &str) -> bool {
    value == "0"
        || (!value.is_empty()
            && !value.starts_with('0')
            && value.bytes().all(|b| b.is_ascii_digit()))
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_nonempty_public_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| !byte.is_ascii_control())
}

fn is_canonical_worlddb_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_request_rejects_renderer_paths() {
        let request = serde_json::from_str::<ExportRequestV1>(
            r#"{"protocol_version":1,"kind":"logical","from_revision":"1","through_revision":"2","history_spaces":["00000000-0000-0000-0000-000000000000"],"record_classes":["HistorySpaceDefinition"],"source_path":"C:/secret","output_path":"C:/secret"}"#,
        );
        assert!(request.is_err());
    }

    #[test]
    fn import_requests_reject_renderer_paths() {
        let plan = serde_json::from_str::<ImportPlanRequestV1>(
            r#"{"protocol_version":1,"mappings":[],"destination_path":"C:/secret","input_path":"C:/secret","output_path":"C:/secret"}"#,
        );
        let prepare = serde_json::from_str::<ImportPrepareRequestV1>(
            r#"{"protocol_version":1,"destination_path":"C:/secret","input_path":"C:/secret","plan_path":"C:/secret"}"#,
        );
        assert!(plan.is_err());
        assert!(prepare.is_err());
    }

    #[test]
    fn sharing_export_summary_cannot_disclose_omission_counts() {
        let result = serde_json::from_str::<CliSharingExportSummary>(
            r#"{"status":"completed","scope":{"from_revision":"1","through_revision":"2","history_space_count":"1","history_spaces":["00000000-0000-0000-0000-000000000000"],"record_class_count":"1","record_classes":["Source"]},"included_record_count":"3","omission_counts_disclosed":false,"source_audit_committed":true,"source_modified":true,"omitted_record_count":"9"}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn export_scope_validation_enforces_ancestry_and_revision_order() {
        let request = ExportRequestV1 {
            protocol_version: 1,
            kind: "logical".to_owned(),
            from_revision: "1".to_owned(),
            through_revision: "2".to_owned(),
            history_spaces: vec!["00000000-0000-0000-0000-000000000000".to_owned()],
            record_classes: vec!["HistorySpaceDefinition".to_owned(), "Source".to_owned()],
        };
        assert!(validate_export_request(&request).is_ok());
        let share = ExportRequestV1 {
            kind: "sharing".to_owned(),
            record_classes: vec!["Source".to_owned()],
            ..request.clone()
        };
        assert!(validate_export_request(&share).is_ok());
        let missing_history_space_definitions = ExportRequestV1 {
            record_classes: vec!["Source".to_owned()],
            ..request.clone()
        };
        assert!(validate_export_request(&missing_history_space_definitions).is_err());
        let reversed_range = ExportRequestV1 {
            from_revision: "3".to_owned(),
            ..request
        };
        assert!(validate_export_request(&reversed_range).is_err());
        assert!(is_canonical_count("34"));
        assert!(!is_canonical_count("01"));
        assert!(is_canonical_revision("0"));
        assert!(!is_canonical_revision("01"));
    }

    #[test]
    fn import_plan_mapping_rows_are_bounded_and_control_free() {
        let valid = ImportPlanRequestV1 {
            protocol_version: 1,
            mappings: vec!["entity:00000000-0000-0000-0000-000000000000=entity:00000000-0000-0000-0000-000000000001".to_owned()],
        };
        assert!(validate_plan_request(&valid).is_ok());
        let invalid = ImportPlanRequestV1 {
            protocol_version: 1,
            mappings: vec!["entity:a=entity:b\n--output C:/outside".to_owned()],
        };
        assert!(validate_plan_request(&invalid).is_err());
    }
}
