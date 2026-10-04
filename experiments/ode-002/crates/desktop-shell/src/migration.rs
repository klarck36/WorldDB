use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use worlddb_core::{
    DomainId, MigrationCategory, MigrationDryRun, MigrationDryRunErrorKind,
    MigrationDryRunUnresolvedReason, MigrationPlan, MigrationRunId, MigrationTransformerError,
    OperationId, Record, decode_record,
};

const MAX_PLAN_BYTES: u64 = 16 * 1024 * 1024;
type StagedMigrationInputs = (Vec<Vec<PathBuf>>, Vec<Vec<u8>>);

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationPlanView {
    pub database_id: String,
    pub migration_id: String,
    pub category: &'static str,
    pub plan_fingerprint: String,
    pub source_revision: String,
    pub target_revision: String,
    pub step_ids: Vec<String>,
    pub transformer_version: String,
    pub max_work_units: String,
    pub max_memory_bytes: String,
    pub breaking_confirmation_required: bool,
    pub restorepoint_required: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationInputErrorView {
    pub record_index: String,
    pub cause: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationUnresolvedItemView {
    pub record_index: String,
    pub reason: &'static str,
    pub source_record_fingerprint: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationDryRunView {
    pub plan: MigrationPlanView,
    pub status: &'static str,
    pub source_record_count: String,
    pub input_bytes: String,
    pub estimated_output_bytes: Option<String>,
    pub estimated_output_records: Option<String>,
    pub reserved_memory_bytes: Option<String>,
    pub diagnostic_memory_bytes: String,
    pub error_count: String,
    pub omitted_error_count: String,
    pub fatal_error: Option<&'static str>,
    pub transform_complete: bool,
    pub errors: Vec<MigrationInputErrorView>,
    pub unresolved_items: Vec<MigrationUnresolvedItemView>,
    pub omitted_unresolved_count: String,
    pub warnings: Vec<String>,
    pub input_fingerprint: Option<String>,
    pub output_fingerprint: Option<String>,
    pub preflight_complete: bool,
    pub contract_phase: &'static str,
    pub restorepoint_status: &'static str,
    pub breaking_confirmation_required: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationRunView {
    pub plan: MigrationPlanView,
    pub status: &'static str,
    pub run_id: String,
    pub completed_step_count: String,
    pub final_revision: String,
    pub restorepoint_status: &'static str,
    pub contract_phase: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationPanelState {
    pub plan: MigrationPlanView,
    pub dry_run: Option<MigrationDryRunView>,
    pub run_attempted: bool,
    pub attempted_run_id: Option<String>,
    pub omitted_record_indexes: Vec<String>,
    pub can_execute: bool,
    pub can_resume: bool,
}

pub(crate) struct MigrationDraft {
    database_root: PathBuf,
    staging_root: PathBuf,
    plan_path: PathBuf,
    plan: MigrationPlan,
    plan_view: MigrationPlanView,
    staged_step_files: Option<Vec<Vec<PathBuf>>>,
    dry_run: Option<MigrationDryRunView>,
    run_attempted: bool,
    run_id: Option<MigrationRunId>,
    operation_ids: Vec<OperationId>,
    backup_target: Option<PathBuf>,
    omitted_record_indexes: Vec<u64>,
}

impl MigrationDraft {
    pub(crate) fn plan_view(&self) -> MigrationPlanView {
        self.plan_view.clone()
    }

    pub(crate) fn is_breaking(&self) -> bool {
        self.plan.category() == MigrationCategory::Breaking
    }

    pub(crate) fn step_ids(&self) -> Vec<String> {
        self.plan.steps().iter().map(ToString::to_string).collect()
    }

    pub(crate) fn run_attempted(&self) -> bool {
        self.run_attempted
    }

    pub(crate) fn attempted_run_id(&self) -> Option<String> {
        self.run_id.map(|run_id| run_id.to_string())
    }

    pub(crate) fn can_execute(&self) -> bool {
        self.dry_run.as_ref().is_some_and(|preview| {
            let unresolved_resolved = if preview.unresolved_items.is_empty() {
                preview.transform_complete
            } else {
                preview.unresolved_items.iter().all(|item| {
                    self.omitted_record_indexes
                        .iter()
                        .any(|index| index.to_string() == item.record_index)
                })
            };
            preview.error_count == "0"
                && preview.omitted_error_count == "0"
                && preview.omitted_unresolved_count == "0"
                && unresolved_resolved
        }) && !self.run_attempted
    }

    pub(crate) fn set_omissions(&mut self, record_indexes: Vec<u64>) -> Result<(), String> {
        if self.run_attempted {
            return Err("an attempted migration run cannot change its decisions".to_owned());
        }
        let preview = self
            .dry_run
            .as_ref()
            .ok_or_else(|| "run a migration dry run before resolving items".to_owned())?;
        if preview.omitted_unresolved_count != "0" {
            return Err("the bounded preview omitted unresolved item details".to_owned());
        }
        let selected = record_indexes.iter().copied().collect::<BTreeSet<_>>();
        if selected.len() != record_indexes.len() {
            return Err("migration decisions contain duplicate records".to_owned());
        }
        let known = preview
            .unresolved_items
            .iter()
            .map(|item| {
                item.record_index
                    .parse::<u64>()
                    .map_err(|_| "migration preview contains an invalid record index".to_owned())
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if selected.iter().any(|index| !known.contains(index)) {
            return Err("migration decisions do not match the current unresolved items".to_owned());
        }
        self.omitted_record_indexes = selected.into_iter().collect();
        Ok(())
    }

    pub(crate) fn dry_run_view(&self) -> Option<MigrationDryRunView> {
        self.dry_run.clone()
    }

    pub(crate) fn panel_state(&self) -> MigrationPanelState {
        MigrationPanelState {
            plan: self.plan_view(),
            dry_run: self.dry_run_view(),
            run_attempted: self.run_attempted,
            attempted_run_id: self.attempted_run_id(),
            omitted_record_indexes: self
                .omitted_record_indexes
                .iter()
                .map(ToString::to_string)
                .collect(),
            can_execute: self.can_execute(),
            can_resume: self.can_resume(),
        }
    }

    pub(crate) fn stage_inputs(
        &mut self,
        selected_files: Vec<Vec<PathBuf>>,
    ) -> Result<MigrationDryRunView, String> {
        if self.run_attempted || selected_files.len() != self.plan.steps().len() {
            return Err("migration inputs do not match the selected plan".to_owned());
        }
        self.staged_step_files = None;
        self.dry_run = None;
        self.omitted_record_indexes.clear();
        let input_root = create_private_child(&self.staging_root, "inputs")?;
        let (staged_step_files, flat_records) =
            stage_input_files(&self.plan, &input_root, selected_files)?;
        let source = self.plan.source_schema_precondition();
        let report = MigrationDryRun::run(
            &self.plan,
            source.revision(),
            *source.fingerprint(),
            &flat_records,
        );
        let cli_summary = run_cli_migration(migration_args(
            "dry-run",
            &self.database_root,
            &self.plan_path,
            &self.plan,
            Some(&staged_step_files),
            None,
            None,
            None,
            &[],
        )?)?;
        validate_dry_run_summary(&self.plan, &self.plan_view, &report, &cli_summary)?;

        let estimate = report.estimate();
        let unresolved_items = report
            .unresolved_items()
            .iter()
            .map(|item| MigrationUnresolvedItemView {
                record_index: item.record_index().to_string(),
                reason: match item.reason() {
                    MigrationDryRunUnresolvedReason::AmbiguousTargetMapping => {
                        "ambiguous_target_mapping"
                    }
                },
                source_record_fingerprint: hex(item.source_record_fingerprint()),
            })
            .collect::<Vec<_>>();
        let errors = report
            .errors()
            .iter()
            .map(|error| MigrationInputErrorView {
                record_index: error.record_index().to_string(),
                cause: transformer_error_label(error.cause()),
            })
            .collect::<Vec<_>>();
        let warnings = report
            .warnings()
            .iter()
            .map(|warning| {
                let label = match warning.code() {
                    worlddb_core::MigrationDryRunWarningCode::DeprecatedTargetDefinition => {
                        "deprecated_target_definition"
                    }
                };
                warning
                    .record_index()
                    .map_or_else(|| label.to_owned(), |index| format!("{label}:{index}"))
            })
            .collect();
        let view = MigrationDryRunView {
            plan: self.plan_view.clone(),
            status: "previewed",
            source_record_count: report.source_record_count().unwrap_or(0).to_string(),
            input_bytes: report.source_bytes().unwrap_or(0).to_string(),
            estimated_output_bytes: estimate.map(|estimate| estimate.output_bytes().to_string()),
            estimated_output_records: estimate.map(|estimate| estimate.record_count().to_string()),
            reserved_memory_bytes: estimate
                .map(|estimate| estimate.reserved_memory_bytes().to_string()),
            diagnostic_memory_bytes: report.diagnostic_memory_bytes().to_string(),
            error_count: report.error_count().to_string(),
            omitted_error_count: report.omitted_error_count().to_string(),
            fatal_error: report
                .fatal_error()
                .map(|error| stable_error_kind(error.kind())),
            transform_complete: report.output().is_some(),
            errors,
            unresolved_items,
            omitted_unresolved_count: report.omitted_unresolved_item_count().to_string(),
            warnings,
            input_fingerprint: report
                .input_fingerprint()
                .map(|fingerprint| hex(fingerprint.as_bytes())),
            output_fingerprint: report
                .output()
                .map(|output| hex(output.fingerprint().as_bytes())),
            preflight_complete: report.preflight_complete(),
            contract_phase: "preview_only_no_commit",
            restorepoint_status: if self.is_breaking() {
                "required_before_run_not_created"
            } else {
                "not_required"
            },
            breaking_confirmation_required: self.is_breaking(),
        };
        self.staged_step_files = Some(staged_step_files);
        self.dry_run = Some(view.clone());
        Ok(view)
    }

    pub(crate) fn execute(
        &mut self,
        confirmed_breaking: bool,
        backup_parent: Option<PathBuf>,
        restore_parent: Option<PathBuf>,
    ) -> Result<MigrationRunView, String> {
        if self.run_attempted {
            return Err("this migration run already has an attempted outcome; inspect or resume it before retrying".to_owned());
        }
        if self.dry_run.is_none() {
            return Err("run a fresh migration dry run before execution".to_owned());
        }
        if !self.can_execute() {
            return Err("the migration preview contains errors or unresolved items".to_owned());
        }
        let breaking = self.is_breaking();
        if breaking != confirmed_breaking
            || breaking != backup_parent.is_some()
            || breaking != restore_parent.is_some()
        {
            return Err("breaking migrations require a separate explicit confirmation and restorepoint targets".to_owned());
        }
        let staged_step_files = self
            .staged_step_files
            .as_ref()
            .ok_or_else(|| "migration inputs are no longer available".to_owned())?;
        let run_id = generated_run_id()?;
        let mut operation_ids = Vec::new();
        operation_ids
            .try_reserve_exact(self.plan.steps().len())
            .map_err(|_| "migration operation identity is unavailable".to_owned())?;
        for _ in self.plan.steps() {
            operation_ids.push(
                worlddb_core::storage_internal::generate_schema_management_operation_id()
                    .map_err(|_| "migration operation identity is unavailable".to_owned())?,
            );
        }
        let (backup_target, restore_target) = if breaking {
            let backup_parent = backup_parent
                .as_deref()
                .ok_or_else(|| "backup destination is missing".to_owned())?;
            let restore_parent = restore_parent
                .as_deref()
                .ok_or_else(|| "restore destination is missing".to_owned())?;
            (
                Some(migration_target_directory(
                    backup_parent,
                    &self.database_root,
                    "exact-backup",
                    run_id,
                )?),
                Some(migration_target_directory(
                    restore_parent,
                    &self.database_root,
                    "restore-clone",
                    run_id,
                )?),
            )
        } else {
            (None, None)
        };
        self.run_id = Some(run_id);
        self.operation_ids.clone_from(&operation_ids);
        self.backup_target.clone_from(&backup_target);
        self.run_attempted = true;
        let args = migration_args(
            "run",
            &self.database_root,
            &self.plan_path,
            &self.plan,
            Some(staged_step_files),
            Some((run_id, &self.operation_ids)),
            backup_target.as_deref(),
            restore_target.as_deref(),
            &self.omitted_record_indexes,
        )?;
        let summary = run_cli_migration(args).map_err(|error| {
            format!(
                "{error}; migration run {} must not be started again",
                run_id
            )
        })?;
        if summary.status != "completed"
            || summary.plan_fingerprint != self.plan_view.plan_fingerprint
            || summary.category != self.plan_view.category
            || summary.database_id != self.plan_view.database_id
            || summary.migration_id != self.plan_view.migration_id
            || summary.completed_step_count != self.plan.steps().len().to_string()
            || summary.final_revision.is_none()
        {
            return Err(format!(
                "migration completion could not be verified; migration run {} must not be started again",
                run_id
            ));
        }
        Ok(MigrationRunView {
            plan: self.plan_view.clone(),
            status: "completed",
            run_id: run_id.to_string(),
            completed_step_count: summary.completed_step_count,
            final_revision: summary
                .final_revision
                .ok_or_else(|| "migration completion revision is missing".to_owned())?,
            restorepoint_status: if breaking {
                "created_and_verified"
            } else {
                "not_required"
            },
            contract_phase: "committed",
        })
    }

    pub(crate) fn can_resume(&self) -> bool {
        self.run_attempted
            && self.run_id.is_some()
            && self.operation_ids.len() == self.plan.steps().len()
            && self.staged_step_files.is_some()
    }

    pub(crate) fn resume(
        &mut self,
        confirmed_breaking: bool,
        restore_parent: Option<PathBuf>,
    ) -> Result<MigrationRunView, String> {
        if !self.can_resume() {
            return Err("no migration run is available to resume".to_owned());
        }
        let breaking = self.is_breaking();
        if breaking != confirmed_breaking || breaking != restore_parent.is_some() {
            return Err("breaking migration resume requires explicit confirmation and a new restore destination".to_owned());
        }
        let run_id = self
            .run_id
            .ok_or_else(|| "migration run identity is unavailable".to_owned())?;
        let restore_target = if breaking {
            let restore_parent = restore_parent
                .as_deref()
                .ok_or_else(|| "restore destination is missing".to_owned())?;
            let restore_id = generated_run_id()?;
            Some(migration_target_directory(
                restore_parent,
                &self.database_root,
                "restore-clone-resume",
                restore_id,
            )?)
        } else {
            None
        };
        let staged_step_files = self
            .staged_step_files
            .as_ref()
            .ok_or_else(|| "migration inputs are no longer available".to_owned())?;
        let backup_target = self.backup_target.as_deref();
        let args = migration_args(
            "resume",
            &self.database_root,
            &self.plan_path,
            &self.plan,
            Some(staged_step_files),
            Some((run_id, &self.operation_ids)),
            backup_target,
            restore_target.as_deref(),
            &self.omitted_record_indexes,
        )?;
        let summary = match run_cli_migration(args) {
            Ok(summary) => summary,
            Err(error) if error.contains("(not_found)") => {
                self.run_attempted = false;
                self.run_id = None;
                self.operation_ids.clear();
                self.backup_target = None;
                return Err(format!(
                    "no resumable migration journal was found for run {run_id}; the source has no committed migration step"
                ));
            }
            Err(error) => {
                return Err(format!(
                    "{error}; migration run {} remains available for inspection",
                    run_id
                ));
            }
        };
        if summary.status != "resumed"
            || summary.plan_fingerprint != self.plan_view.plan_fingerprint
            || summary.category != self.plan_view.category
            || summary.database_id != self.plan_view.database_id
            || summary.migration_id != self.plan_view.migration_id
            || summary.completed_step_count != self.plan.steps().len().to_string()
        {
            return Err(format!(
                "migration resume completion could not be verified; run {} remains available for inspection",
                run_id
            ));
        }
        let final_revision = summary
            .final_revision
            .ok_or_else(|| "migration resume revision is missing".to_owned())?;
        Ok(MigrationRunView {
            plan: self.plan_view.clone(),
            status: "resumed",
            run_id: run_id.to_string(),
            completed_step_count: summary.completed_step_count,
            final_revision,
            restorepoint_status: if breaking {
                "created_and_verified"
            } else {
                "not_required"
            },
            contract_phase: "committed",
        })
    }
}

impl Drop for MigrationDraft {
    fn drop(&mut self) {
        remove_private_tree(&self.staging_root);
    }
}

pub(crate) fn inspect_plan(
    database_root: &Path,
    selected_plan_path: &Path,
) -> Result<MigrationDraft, String> {
    let database_root = canonical_directory(database_root)?;
    let plan_bytes = read_bounded_regular_file(selected_plan_path, MAX_PLAN_BYTES)?;
    let plan = decode_migration_plan(&plan_bytes)?;
    let staging_root = create_private_staging_root()?;
    let plan_path = write_staged_file(&staging_root, "migration-plan.record", &plan_bytes)?;
    let summary = run_cli_migration(migration_args(
        "plan",
        &database_root,
        &plan_path,
        &plan,
        None,
        None,
        None,
        None,
        &[],
    )?)?;
    let plan_view = make_plan_view(&plan, &summary)?;
    Ok(MigrationDraft {
        database_root,
        staging_root,
        plan_path,
        plan,
        plan_view,
        staged_step_files: None,
        dry_run: None,
        run_attempted: false,
        run_id: None,
        operation_ids: Vec::new(),
        backup_target: None,
        omitted_record_indexes: Vec::new(),
    })
}

fn migration_target_directory(
    selected_parent: &Path,
    source_root: &Path,
    purpose: &str,
    run_id: MigrationRunId,
) -> Result<PathBuf, String> {
    let parent = canonical_directory(selected_parent)?;
    if parent == source_root || parent.starts_with(source_root) {
        return Err(
            "backup and restore destinations must be outside the source project".to_owned(),
        );
    }
    let target = parent.join(format!("worlddb-migration-{run_id}-{purpose}"));
    if fs::symlink_metadata(&target).is_ok() {
        return Err("migration destination already exists".to_owned());
    }
    Ok(target)
}

fn stage_input_files(
    plan: &MigrationPlan,
    input_root: &Path,
    selected_files: Vec<Vec<PathBuf>>,
) -> Result<StagedMigrationInputs, String> {
    let mut total_bytes = 0_u64;
    let mut total_records = 0_u64;
    let mut staged_steps = Vec::new();
    let mut flat_records = Vec::new();
    staged_steps
        .try_reserve_exact(selected_files.len())
        .map_err(|_| "migration input exceeds its resource budget".to_owned())?;
    for (step_index, (step_id, selected_step_files)) in
        plan.steps().iter().zip(selected_files).enumerate()
    {
        let mut staged_files = Vec::new();
        staged_files
            .try_reserve_exact(selected_step_files.len())
            .map_err(|_| "migration input exceeds its resource budget".to_owned())?;
        for (record_index, source_path) in selected_step_files.iter().enumerate() {
            let remaining = plan
                .budget()
                .max_memory_bytes()
                .checked_sub(total_bytes)
                .ok_or_else(|| "migration input exceeds its memory budget".to_owned())?;
            let bytes = read_bounded_regular_file(source_path, remaining)?;
            total_bytes = total_bytes
                .checked_add(
                    u64::try_from(bytes.len())
                        .map_err(|_| "migration input exceeds its memory budget".to_owned())?,
                )
                .ok_or_else(|| "migration input exceeds its memory budget".to_owned())?;
            total_records = total_records
                .checked_add(1)
                .ok_or_else(|| "migration input count overflowed".to_owned())?;
            if total_records > plan.budget().max_work_units() {
                return Err("migration input exceeds its work budget".to_owned());
            }
            let file_name = format!("step-{step_index}-{}-record-{record_index}.record", step_id);
            let staged_path = write_staged_file(input_root, &file_name, &bytes)?;
            staged_files.push(staged_path);
            flat_records.push(bytes);
        }
        staged_steps.push(staged_files);
    }
    Ok((staged_steps, flat_records))
}

fn validate_dry_run_summary(
    plan: &MigrationPlan,
    plan_view: &MigrationPlanView,
    report: &worlddb_core::MigrationDryRunReport,
    summary: &CliMigrationSummary,
) -> Result<(), String> {
    let retained_unresolved = u64::try_from(report.unresolved_items().len())
        .map_err(|_| "migration unresolved count overflowed".to_owned())?;
    let unresolved_count = retained_unresolved
        .checked_add(report.omitted_unresolved_item_count())
        .ok_or_else(|| "migration unresolved count overflowed".to_owned())?;
    if summary.status != "previewed"
        || summary.category != category_label(plan.category())
        || summary.database_id != plan_view.database_id
        || summary.migration_id != plan_view.migration_id
        || summary.plan_fingerprint != hex(plan.fingerprint().as_bytes())
        || summary.source_revision
            != plan
                .source_schema_precondition()
                .revision()
                .revision()
                .value()
                .to_string()
        || summary.target_revision
            != plan
                .target_schema()
                .revision()
                .revision()
                .value()
                .to_string()
        || summary.input_record_count != report.source_record_count().unwrap_or(0).to_string()
        || summary.input_bytes != report.source_bytes().unwrap_or(0).to_string()
        || summary.error_count != report.error_count().to_string()
        || summary.unresolved_count != unresolved_count.to_string()
        || summary.estimate_output_bytes
            != report
                .output()
                .map(|output| output.encoded_bytes().to_string())
    {
        return Err("migration dry-run did not match the exact staged input".to_owned());
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "closed migration CLI arguments are validated as one command"
)]
fn migration_args(
    action: &str,
    database_root: &Path,
    plan_path: &Path,
    plan: &MigrationPlan,
    step_files: Option<&[Vec<PathBuf>]>,
    execution_ids: Option<(MigrationRunId, &[OperationId])>,
    backup_target: Option<&Path>,
    restore_target: Option<&Path>,
    omitted_record_indexes: &[u64],
) -> Result<Vec<OsString>, String> {
    let mut args = vec![
        OsString::from("--format"),
        OsString::from("jsonl"),
        OsString::from("v1"),
        OsString::from("migration"),
        OsString::from(action),
        database_root.as_os_str().to_owned(),
        OsString::from("--plan-file"),
        plan_path.as_os_str().to_owned(),
    ];
    if let Some(step_files) = step_files {
        if step_files.len() != plan.steps().len() {
            return Err("migration input steps do not match the selected plan".to_owned());
        }
        if execution_ids.is_some_and(|(_, operation_ids)| operation_ids.len() != plan.steps().len())
        {
            return Err("migration operation identity is unavailable".to_owned());
        }
        for (index, (step_id, files)) in plan.steps().iter().zip(step_files).enumerate() {
            args.push(OsString::from("--step"));
            args.push(OsString::from(step_id.to_string()));
            if let Some((_, operation_ids)) = execution_ids {
                let operation_id = operation_ids
                    .get(index)
                    .ok_or_else(|| "migration operation identity is unavailable".to_owned())?;
                args.push(OsString::from("--operation-id"));
                args.push(OsString::from(operation_id.to_string()));
            }
            for path in files {
                args.push(OsString::from("--record"));
                args.push(path.as_os_str().to_owned());
            }
        }
    }
    if let Some((run_id, _)) = execution_ids {
        args.push(OsString::from("--run-id"));
        args.push(OsString::from(run_id.to_string()));
    }
    if let Some(path) = backup_target {
        args.push(OsString::from("--backup"));
        args.push(path.as_os_str().to_owned());
    }
    if let Some(path) = restore_target {
        args.push(OsString::from("--restore"));
        args.push(path.as_os_str().to_owned());
    }
    for record_index in omitted_record_indexes {
        args.push(OsString::from("--omit"));
        args.push(OsString::from(record_index.to_string()));
    }
    if matches!(action, "run" | "resume") && plan.category() == MigrationCategory::Breaking {
        args.push(OsString::from("--confirm-breaking"));
    }
    Ok(args)
}

fn run_cli_migration(arguments: Vec<OsString>) -> Result<CliMigrationSummary, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = worlddb_cli::cli::run_with(arguments, &mut stdout, &mut stderr);
    let response: serde_json::Value = serde_json::from_slice(&stdout)
        .map_err(|_| "migration service returned an unreadable report".to_owned())?;
    let outcome = response
        .get("outcome")
        .ok_or_else(|| "migration service returned an unreadable report".to_owned())?;
    let outcome_kind = outcome
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "migration service returned an unreadable report".to_owned())?;
    if exit_code != 0 || outcome_kind == "error" {
        let code = outcome
            .get("data")
            .and_then(|data| data.get("code"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("migration_rejected");
        return Err(format!("migration command was rejected ({code})"));
    }
    if outcome_kind != "migration" {
        return Err("migration service returned an unexpected report".to_owned());
    }
    serde_json::from_value(
        outcome
            .get("data")
            .cloned()
            .ok_or_else(|| "migration service returned an unreadable report".to_owned())?,
    )
    .map_err(|_| "migration service returned an unreadable report".to_owned())
}

#[derive(Debug, serde::Deserialize)]
struct CliMigrationSummary {
    status: String,
    database_id: String,
    migration_id: String,
    category: String,
    plan_fingerprint: String,
    source_revision: String,
    target_revision: String,
    step_count: String,
    input_record_count: String,
    input_bytes: String,
    estimate_output_bytes: Option<String>,
    error_count: String,
    unresolved_count: String,
    completed_step_count: String,
    final_revision: Option<String>,
}

fn make_plan_view(
    plan: &MigrationPlan,
    summary: &CliMigrationSummary,
) -> Result<MigrationPlanView, String> {
    if summary.status != "planned"
        || summary.category != category_label(plan.category())
        || summary.plan_fingerprint != hex(plan.fingerprint().as_bytes())
        || summary.source_revision
            != plan
                .source_schema_precondition()
                .revision()
                .revision()
                .value()
                .to_string()
        || summary.target_revision
            != plan
                .target_schema()
                .revision()
                .revision()
                .value()
                .to_string()
        || summary.step_count != plan.steps().len().to_string()
    {
        return Err("migration plan does not match the selected source project".to_owned());
    }
    Ok(MigrationPlanView {
        database_id: summary.database_id.clone(),
        migration_id: summary.migration_id.clone(),
        category: category_label(plan.category()),
        plan_fingerprint: summary.plan_fingerprint.clone(),
        source_revision: summary.source_revision.clone(),
        target_revision: summary.target_revision.clone(),
        step_ids: plan.steps().iter().map(ToString::to_string).collect(),
        transformer_version: plan.transformer_version().value().to_string(),
        max_work_units: plan.budget().max_work_units().to_string(),
        max_memory_bytes: plan.budget().max_memory_bytes().to_string(),
        breaking_confirmation_required: plan.category() == MigrationCategory::Breaking,
        restorepoint_required: plan.category() == MigrationCategory::Breaking,
    })
}

fn decode_migration_plan(bytes: &[u8]) -> Result<MigrationPlan, String> {
    let record = decode_record(bytes)
        .map_err(|_| "selected file is not a canonical migration plan".to_owned())?
        .into_record();
    match record {
        Record::MigrationPlan(plan) => Ok(plan),
        _ => Err("selected file does not contain a migration plan".to_owned()),
    }
}

fn read_bounded_regular_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "selected input is unavailable".to_owned())?;
    if !metadata.is_file() || is_reparse_point(&metadata) || metadata.len() > max_bytes {
        return Err("selected input is not a bounded regular file".to_owned());
    }
    let mut file = File::open(path).map_err(|_| "selected input is unavailable".to_owned())?;
    let limit = max_bytes
        .checked_add(1)
        .ok_or_else(|| "selected input exceeds its byte limit".to_owned())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| "selected input could not be read".to_owned())?;
    if u64::try_from(bytes.len()).map_err(|_| "selected input exceeds its byte limit".to_owned())?
        > max_bytes
    {
        return Err("selected input exceeds its byte limit".to_owned());
    }
    Ok(bytes)
}

fn write_staged_file(root: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = root.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "migration input could not be staged".to_owned())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "migration input could not be staged".to_owned())?;
    fs::canonicalize(path).map_err(|_| "migration input could not be staged".to_owned())
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "source project is unavailable".to_owned())?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err("source project is unavailable".to_owned());
    }
    fs::canonicalize(path).map_err(|_| "source project is unavailable".to_owned())
}

fn create_private_staging_root() -> Result<PathBuf, String> {
    let parent = fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "migration staging is unavailable".to_owned())?;
    let id = worlddb_core::storage_internal::generate_database_id()
        .map_err(|_| "migration staging identity is unavailable".to_owned())?;
    let root = parent.join(format!("worlddb-migration-{}", id));
    fs::create_dir(&root).map_err(|_| "migration staging is unavailable".to_owned())?;
    let canonical =
        fs::canonicalize(&root).map_err(|_| "migration staging is unavailable".to_owned())?;
    if !canonical.starts_with(&parent) || canonical == parent {
        return Err("migration staging is unavailable".to_owned());
    }
    Ok(canonical)
}

fn create_private_child(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let id = worlddb_core::storage_internal::generate_database_id()
        .map_err(|_| "migration staging identity is unavailable".to_owned())?;
    let child = parent.join(format!("{name}-{}", id));
    fs::create_dir(&child).map_err(|_| "migration staging is unavailable".to_owned())?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| "migration staging is unavailable".to_owned())?;
    let canonical_child =
        fs::canonicalize(&child).map_err(|_| "migration staging is unavailable".to_owned())?;
    if !canonical_child.starts_with(&canonical_parent) || canonical_child == canonical_parent {
        return Err("migration staging is unavailable".to_owned());
    }
    Ok(canonical_child)
}

fn remove_private_tree(path: &Path) {
    let Ok(temp_root) = fs::canonicalize(std::env::temp_dir()) else {
        return;
    };
    let Ok(target) = fs::canonicalize(path) else {
        return;
    };
    let Ok(metadata) = fs::symlink_metadata(&target) else {
        return;
    };
    if target.starts_with(&temp_root)
        && target != temp_root
        && metadata.is_dir()
        && !is_reparse_point(&metadata)
    {
        let _ = fs::remove_dir_all(target);
    }
}

fn generated_run_id() -> Result<MigrationRunId, String> {
    let database_id = worlddb_core::storage_internal::generate_database_id()
        .map_err(|_| "migration run identity is unavailable".to_owned())?;
    MigrationRunId::try_from_bytes(database_id.to_bytes())
        .map_err(|_| "migration run identity is unavailable".to_owned())
}

fn category_label(category: MigrationCategory) -> &'static str {
    match category {
        MigrationCategory::MetadataOnly => "MetadataOnly",
        MigrationCategory::Additive => "Additive",
        MigrationCategory::CompatibleConstraintChange => "CompatibleConstraintChange",
        MigrationCategory::Restrictive => "Restrictive",
        MigrationCategory::Breaking => "Breaking",
    }
}

fn transformer_error_label(error: MigrationTransformerError) -> &'static str {
    match error {
        MigrationTransformerError::UnsupportedVersion(_) => "unsupported_transformer",
        MigrationTransformerError::Plan(_) => "plan_precondition_failed",
        MigrationTransformerError::RecordCodec(_) => "invalid_record_frame",
        MigrationTransformerError::WorkBudgetExceeded { .. } => "work_budget_exceeded",
        MigrationTransformerError::MemoryBudgetExceeded { .. } => "memory_budget_exceeded",
        MigrationTransformerError::AllocationFailed => "allocation_failed",
        MigrationTransformerError::SizeOverflow => "size_overflow",
        MigrationTransformerError::CalendarShiftNotPlanned => "calendar_shift_missing",
        MigrationTransformerError::TimelineMismatch { .. } => "timeline_mismatch",
        MigrationTransformerError::CalendarArithmeticOverflow => "calendar_arithmetic_overflow",
    }
}

fn stable_error_kind(error: MigrationDryRunErrorKind) -> &'static str {
    match error {
        MigrationDryRunErrorKind::InputSizeOverflow => "input_size_overflow",
        MigrationDryRunErrorKind::Transformer(error) => transformer_error_label(error),
    }
}

fn hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(TABLE[usize::from(byte >> 4)] as char);
        encoded.push(TABLE[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
