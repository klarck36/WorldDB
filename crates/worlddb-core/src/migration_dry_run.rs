//! Deterministic migration preview through the production transformer.

use crate::{
    MigrationPlan, MigrationPlanFingerprint, MigrationTransformEstimate,
    MigrationTransformFingerprint, MigrationTransformPreview, MigrationTransformRecordError,
    MigrationTransformer, MigrationTransformerError, MigrationTransformerVersion,
};

/// Runs a migration plan through the same versioned transform used by execution.
pub struct MigrationDryRun;

impl MigrationDryRun {
    /// Admits and transforms every source frame, collecting its result and validation summary.
    ///
    /// Invalid frames are reported with stable input positions within the plan's memory budget.
    /// The preview never publishes or persists transformed data.
    #[must_use]
    pub fn run(
        plan: &MigrationPlan,
        source_revision: crate::SchemaRevision,
        source_fingerprint: [u8; 32],
        records: &[Vec<u8>],
    ) -> MigrationDryRunReport {
        let source_record_count = u64::try_from(records.len()).ok();
        let source_bytes = records.iter().try_fold(0_u64, |total, record| {
            u64::try_from(record.len())
                .ok()
                .and_then(|length| total.checked_add(length))
        });
        let mut report = MigrationDryRunReport {
            plan_fingerprint: plan.fingerprint(),
            transformer_version: plan.transformer_version(),
            source_record_count,
            source_bytes,
            estimate: None,
            diagnostic_memory_bytes: 0,
            error_count: 0,
            omitted_error_count: 0,
            output: None,
            warnings: Vec::new(),
            unresolved: Vec::new(),
            fatal_error: None,
            transform_preview: None,
        };

        if source_record_count.is_none() || source_bytes.is_none() {
            report.push_fatal_error(MigrationDryRunErrorKind::InputSizeOverflow);
            return report;
        }

        let transformer = match MigrationTransformer::for_version(plan.transformer_version()) {
            Ok(transformer) => transformer,
            Err(error) => {
                report.push_transformer_error(error);
                return report;
            }
        };

        let transform_preview = match transformer.transform_records_for_preview(
            plan,
            source_revision,
            source_fingerprint,
            records,
        ) {
            Ok(preview) => preview,
            Err(error) => {
                report.push_transformer_error(error);
                return report;
            }
        };
        report.estimate = Some(transform_preview.estimate());
        report.diagnostic_memory_bytes = transform_preview.diagnostic_memory_bytes();
        report.error_count = transform_preview.error_count();
        report.omitted_error_count = transform_preview.omitted_error_count();
        if let Some(fingerprint) = transform_preview.fingerprint() {
            report.output = Some(MigrationDryRunOutput {
                record_count: transform_preview.estimate().record_count(),
                encoded_bytes: transform_preview.estimate().output_bytes(),
                fingerprint,
            });
        }
        report.transform_preview = Some(transform_preview);
        report
    }
}

/// Complete, immutable summary of one migration dry run.
pub struct MigrationDryRunReport {
    plan_fingerprint: MigrationPlanFingerprint,
    transformer_version: MigrationTransformerVersion,
    source_record_count: Option<u64>,
    source_bytes: Option<u64>,
    estimate: Option<MigrationTransformEstimate>,
    diagnostic_memory_bytes: u64,
    error_count: u64,
    omitted_error_count: u64,
    output: Option<MigrationDryRunOutput>,
    warnings: Vec<MigrationDryRunWarning>,
    unresolved: Vec<MigrationDryRunUnresolvedItem>,
    fatal_error: Option<MigrationDryRunError>,
    transform_preview: Option<MigrationTransformPreview>,
}

impl MigrationDryRunReport {
    /// Plan identity whose exact fingerprint was previewed.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> MigrationPlanFingerprint {
        self.plan_fingerprint
    }

    /// Transformer version selected by the immutable plan.
    #[must_use]
    pub const fn transformer_version(&self) -> MigrationTransformerVersion {
        self.transformer_version
    }

    /// Number of supplied source frames, or `None` if it cannot be represented.
    #[must_use]
    pub const fn source_record_count(&self) -> Option<u64> {
        self.source_record_count
    }

    /// Total encoded source bytes, or `None` if the sum overflowed.
    #[must_use]
    pub const fn source_bytes(&self) -> Option<u64> {
        self.source_bytes
    }

    /// Work and conservative copy-memory admission estimate, when available.
    #[must_use]
    pub const fn estimate(&self) -> Option<MigrationTransformEstimate> {
        self.estimate
    }

    /// Extra bounded memory reserved for retained per-record error details.
    #[must_use]
    pub const fn diagnostic_memory_bytes(&self) -> u64 {
        self.diagnostic_memory_bytes
    }

    /// Result of the shared transformer. `None` means preflight stopped before transformation.
    #[must_use]
    pub const fn output(&self) -> Option<&MigrationDryRunOutput> {
        self.output.as_ref()
    }

    /// Deterministic non-blocking findings. Transformer version 1 emits none.
    #[must_use]
    pub fn warnings(&self) -> &[MigrationDryRunWarning] {
        &self.warnings
    }

    /// Items that require an explicit later decision. Transformer version 1 emits none.
    #[must_use]
    pub fn unresolved_items(&self) -> &[MigrationDryRunUnresolvedItem] {
        &self.unresolved
    }

    /// Plan, version, budget, or input-size failure, if any.
    #[must_use]
    pub const fn fatal_error(&self) -> Option<MigrationDryRunError> {
        self.fatal_error
    }

    /// Per-record canonical frame failures retained by the shared transform, subject to budget.
    #[must_use]
    pub fn errors(&self) -> &[MigrationTransformRecordError] {
        self.transform_preview
            .as_ref()
            .map(MigrationTransformPreview::record_errors)
            .unwrap_or(&[])
    }

    /// Total number of errors, including errors whose detail exceeded the memory bound.
    #[must_use]
    pub const fn error_count(&self) -> u64 {
        self.error_count
    }

    /// Number of errors summarized by count only to stay within the plan memory budget.
    #[must_use]
    pub const fn omitted_error_count(&self) -> u64 {
        self.omitted_error_count
    }

    /// True only when every input passed and a complete shared-transform result exists.
    #[must_use]
    pub fn preflight_complete(&self) -> bool {
        self.output.is_some()
            && self.error_count == 0
            && self.omitted_error_count == 0
            && self.unresolved.is_empty()
    }

    fn push_transformer_error(&mut self, error: MigrationTransformerError) {
        self.push_fatal_error(MigrationDryRunErrorKind::Transformer(error));
    }

    fn push_fatal_error(&mut self, kind: MigrationDryRunErrorKind) {
        self.error_count = 1;
        self.fatal_error = Some(MigrationDryRunError { kind });
    }
}

/// Summary of the exact result produced by the shared transformer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationDryRunOutput {
    record_count: u64,
    encoded_bytes: u64,
    fingerprint: MigrationTransformFingerprint,
}

impl MigrationDryRunOutput {
    /// Number of result frames.
    #[must_use]
    pub const fn record_count(self) -> u64 {
        self.record_count
    }

    /// Total encoded bytes in the result frames.
    #[must_use]
    pub const fn encoded_bytes(self) -> u64 {
        self.encoded_bytes
    }

    /// Fingerprint of the exact ordered output bytes.
    #[must_use]
    pub const fn fingerprint(self) -> MigrationTransformFingerprint {
        self.fingerprint
    }
}

/// Non-blocking finding emitted by an implemented transformer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationDryRunWarning {
    code: MigrationDryRunWarningCode,
    record_index: Option<u64>,
}

impl MigrationDryRunWarning {
    /// Stable warning classification.
    #[must_use]
    pub const fn code(self) -> MigrationDryRunWarningCode {
        self.code
    }

    /// Source record position associated with this warning, if any.
    #[must_use]
    pub const fn record_index(self) -> Option<u64> {
        self.record_index
    }
}

/// Closed warning classifications for migration preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationDryRunWarningCode {
    /// A transformed value uses a deprecated but accepted target definition.
    DeprecatedTargetDefinition,
}

/// One source item that cannot be transformed without an explicit decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationDryRunUnresolvedItem {
    record_index: u64,
    reason: MigrationDryRunUnresolvedReason,
}

impl MigrationDryRunUnresolvedItem {
    /// Source record position requiring an explicit decision.
    #[must_use]
    pub const fn record_index(self) -> u64 {
        self.record_index
    }

    /// Closed reason that made the record unresolved.
    #[must_use]
    pub const fn reason(self) -> MigrationDryRunUnresolvedReason {
        self.reason
    }
}

/// Closed unresolved-item reasons for migration preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationDryRunUnresolvedReason {
    /// The selected target schema does not identify one unique transformation.
    AmbiguousTargetMapping,
}

/// One preflight error and its optional stable source position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationDryRunError {
    kind: MigrationDryRunErrorKind,
}

impl MigrationDryRunError {
    /// Stable error classification and underlying deterministic cause.
    #[must_use]
    pub const fn kind(self) -> MigrationDryRunErrorKind {
        self.kind
    }
}

/// Closed dry-run failure classifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationDryRunErrorKind {
    /// A deterministic source, version, codec, or resource failure.
    Transformer(MigrationTransformerError),
    /// Summing the input count or encoded bytes exceeded the fixed integer range.
    InputSizeOverflow,
}

#[cfg(test)]
mod tests {
    use super::MigrationDryRun;
    use crate::ids::{
        DomainId, EntityId, EntityTypeId, MigrationId, MigrationStepId, PredicateId, Revision,
        SchemaRevision,
    };
    use crate::{
        Entity, JobBudget, MigrationCalendarShift, MigrationCategory, MigrationPlan,
        MigrationPlanSpec, MigrationTargetSchema, MigrationTransformer,
        MigrationTransformerVersion, Record, SchemaDefinitionId, SchemaIdentityTransition,
        SourceSchemaPrecondition, encode_record,
    };

    fn uuid<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn plan(
        category: MigrationCategory,
        max_work_units: u64,
        max_memory_bytes: u64,
    ) -> Result<MigrationPlan, String> {
        let source_id = SchemaDefinitionId::Predicate(uuid::<PredicateId>(3)?);
        let target_id = if category == MigrationCategory::Breaking {
            Some(SchemaDefinitionId::Predicate(uuid::<PredicateId>(4)?))
        } else {
            Some(source_id)
        };
        let source_id = if category == MigrationCategory::Additive {
            None
        } else {
            Some(source_id)
        };
        let transition = SchemaIdentityTransition::new(source_id, target_id, category)
            .map_err(|error| error.to_string())?;
        MigrationPlan::new(MigrationPlanSpec {
            migration_id: uuid::<MigrationId>(1)?,
            category,
            source_schema: SourceSchemaPrecondition::new(
                SchemaRevision::from_published_revision(Revision::GENESIS),
                [0x11; 32],
            ),
            target_schema: MigrationTargetSchema::new(
                SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
                [0x22; 32],
            ),
            steps: vec![uuid::<MigrationStepId>(2)?],
            schema_changes: vec![transition],
            transformer_version: MigrationTransformerVersion::new(1)
                .map_err(|error| error.to_string())?,
            calendar_shift: Option::<MigrationCalendarShift>::None,
            budget: JobBudget::new(max_work_units, max_memory_bytes)
                .map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())
    }

    fn records() -> Result<Vec<Vec<u8>>, String> {
        [5, 6]
            .into_iter()
            .map(|tail| {
                let entity = Entity::new(
                    uuid::<EntityId>(tail)?,
                    uuid::<EntityTypeId>(7)?,
                    Revision::GENESIS,
                );
                encode_record(&Record::Entity(entity)).map_err(|error| error.to_string())
            })
            .collect()
    }

    #[test]
    fn restrictive_and_breaking_dry_runs_use_the_shared_transformer() -> Result<(), String> {
        for category in [MigrationCategory::Restrictive, MigrationCategory::Breaking] {
            let plan = plan(category, 10, 1024 * 1024)?;
            let records = records()?;
            let report = MigrationDryRun::run(
                &plan,
                SchemaRevision::from_published_revision(Revision::GENESIS),
                [0x11; 32],
                &records,
            );
            let output = report.output().ok_or("dry run has no output")?;
            let estimate = report.estimate().ok_or("dry run has no estimate")?;
            let transformer = MigrationTransformer::for_version(plan.transformer_version())
                .map_err(|error| error.to_string())?;
            let transformed = transformer
                .transform_records(
                    &plan,
                    SchemaRevision::from_published_revision(Revision::GENESIS),
                    [0x11; 32],
                    &records,
                )
                .map_err(|error| error.to_string())?;

            assert!(report.preflight_complete());
            assert_eq!(report.plan_fingerprint(), plan.fingerprint());
            assert_eq!(report.source_record_count(), Some(2));
            assert_eq!(output.record_count(), 2);
            assert_eq!(output.encoded_bytes(), estimate.output_bytes());
            assert_eq!(output.fingerprint(), transformed.fingerprint());
            assert_eq!(estimate.record_count(), 2);
            assert_eq!(estimate.input_bytes(), estimate.output_bytes());
            assert!(estimate.reserved_memory_bytes() >= estimate.input_bytes() * 2);
            assert!(
                report.diagnostic_memory_bytes()
                    <= plan
                        .budget()
                        .max_memory_bytes()
                        .saturating_sub(estimate.reserved_memory_bytes())
            );
            assert!(report.warnings().is_empty());
            assert!(report.unresolved_items().is_empty());
            assert!(report.errors().is_empty());
        }
        Ok(())
    }

    #[test]
    fn dry_run_collects_every_invalid_record_position_without_a_partial_output()
    -> Result<(), String> {
        let plan = plan(MigrationCategory::Restrictive, 10, 1024 * 1024)?;
        let mut records = records()?;
        *records.first_mut().ok_or("first record missing")? = vec![0xff];
        records.push(Vec::new());
        let report = MigrationDryRun::run(
            &plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            &records,
        );

        assert!(!report.preflight_complete());
        assert!(report.output().is_none());
        assert_eq!(report.errors().len(), 2);
        assert_eq!(
            report
                .errors()
                .first()
                .ok_or("first record error missing")?
                .record_index(),
            0
        );
        assert_eq!(
            report
                .errors()
                .get(1)
                .ok_or("second record error missing")?
                .record_index(),
            2
        );
        assert_eq!(report.error_count(), 2);
        assert_eq!(report.omitted_error_count(), 0);
        Ok(())
    }

    #[test]
    fn stale_source_and_exceeded_work_budget_block_preflight() -> Result<(), String> {
        let records = records()?;
        let limited_plan = plan(MigrationCategory::Restrictive, 1, 1024 * 1024)?;
        let budget_report = MigrationDryRun::run(
            &limited_plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            &records,
        );
        assert!(!budget_report.preflight_complete());
        assert!(budget_report.output().is_none());
        assert!(budget_report.fatal_error().is_some());
        assert_eq!(budget_report.error_count(), 1);

        let full_plan = plan(MigrationCategory::Restrictive, 10, 1024 * 1024)?;
        let stale_report = MigrationDryRun::run(
            &full_plan,
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            [0x11; 32],
            &records,
        );
        assert!(!stale_report.preflight_complete());
        assert!(stale_report.output().is_none());
        assert!(stale_report.fatal_error().is_some());
        assert_eq!(stale_report.error_count(), 1);
        Ok(())
    }

    #[test]
    fn diagnostic_details_stay_within_remaining_plan_memory() -> Result<(), String> {
        let mut records = records()?;
        *records.first_mut().ok_or("first record missing")? = vec![0xff];
        *records.get_mut(1).ok_or("second record missing")? = Vec::new();
        let input_bytes = records
            .iter()
            .map(|record| record.len() as u64)
            .sum::<u64>();
        let transform_memory = input_bytes * 2 + records.len() as u64 * 32;
        let plan = plan(MigrationCategory::Breaking, 10, transform_memory.max(1))?;
        let report = MigrationDryRun::run(
            &plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            &records,
        );

        assert!(!report.preflight_complete());
        assert_eq!(report.error_count(), 2);
        assert_eq!(report.omitted_error_count(), 2);
        assert!(report.errors().is_empty());
        assert_eq!(report.diagnostic_memory_bytes(), 0);
        Ok(())
    }
}
