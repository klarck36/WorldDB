//! Deterministic migration preview through the production transformer.

use crate::{
    AuthorizationDecision, Capability, MigrationCategory, MigrationPlan, MigrationPlanFingerprint,
    MigrationTransformEstimate, MigrationTransformFingerprint, MigrationTransformPreview,
    MigrationTransformRecordError, MigrationTransformer, MigrationTransformerError,
    MigrationTransformerVersion, PolicyTarget, PrincipalId, SchemaRevision, SecurityPolicySnapshot,
};
use std::fmt;

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
        let input_fingerprint = fingerprint_input(records);
        let mut report = MigrationDryRunReport {
            plan_fingerprint: plan.fingerprint(),
            category: plan.category(),
            transformer_version: plan.transformer_version(),
            source_revision,
            source_schema_fingerprint: source_fingerprint,
            input_fingerprint,
            source_record_count,
            source_bytes,
            estimate: None,
            diagnostic_memory_bytes: 0,
            error_count: 0,
            omitted_error_count: 0,
            omitted_unresolved_count: 0,
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

        let mut transform_preview = match transformer.transform_records_for_preview(
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
        report.unresolved = transform_preview.take_unresolved_items();
        report.omitted_unresolved_count = transform_preview.omitted_unresolved_count();
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
    category: MigrationCategory,
    transformer_version: MigrationTransformerVersion,
    source_revision: SchemaRevision,
    source_schema_fingerprint: [u8; 32],
    input_fingerprint: Option<MigrationDryRunInputFingerprint>,
    source_record_count: Option<u64>,
    source_bytes: Option<u64>,
    estimate: Option<MigrationTransformEstimate>,
    diagnostic_memory_bytes: u64,
    error_count: u64,
    omitted_error_count: u64,
    omitted_unresolved_count: u64,
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

    /// Migration category fixed by the immutable plan.
    #[must_use]
    pub const fn category(&self) -> MigrationCategory {
        self.category
    }

    /// Transformer version selected by the immutable plan.
    #[must_use]
    pub const fn transformer_version(&self) -> MigrationTransformerVersion {
        self.transformer_version
    }

    /// Source schema revision checked by this preflight.
    #[must_use]
    pub const fn source_revision(&self) -> SchemaRevision {
        self.source_revision
    }

    /// Source schema fingerprint checked by this preflight.
    #[must_use]
    pub const fn source_schema_fingerprint(&self) -> &[u8; 32] {
        &self.source_schema_fingerprint
    }

    /// Fingerprint of the exact ordered source frames supplied to this preflight.
    #[must_use]
    pub const fn input_fingerprint(&self) -> Option<MigrationDryRunInputFingerprint> {
        self.input_fingerprint
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

    /// Extra bounded memory reserved for retained per-record errors and unresolved items.
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

    /// Number of unresolved item details omitted to stay within the plan memory allowance.
    #[must_use]
    pub const fn omitted_unresolved_item_count(&self) -> u64 {
        self.omitted_unresolved_count
    }

    /// True only when every input passed and a complete shared-transform result exists.
    #[must_use]
    pub fn preflight_complete(&self) -> bool {
        self.output.is_some()
            && self.error_count == 0
            && self.omitted_error_count == 0
            && self.omitted_unresolved_count == 0
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

/// Stable BLAKE3 fingerprint of the exact ordered source frames inspected by a dry run.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationDryRunInputFingerprint([u8; 32]);

impl MigrationDryRunInputFingerprint {
    /// Exact fingerprint bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Fingerprints the exact ordered canonical input frames for resumable execution.
    #[must_use]
    pub fn for_records(records: &[Vec<u8>]) -> Option<Self> {
        fingerprint_input(records)
    }
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
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MigrationDryRunUnresolvedItem {
    record_index: u64,
    reason: MigrationDryRunUnresolvedReason,
    source_record_fingerprint: [u8; 32],
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

    /// Fingerprint of the exact source frame that produced this unresolved item.
    #[must_use]
    pub const fn source_record_fingerprint(&self) -> &[u8; 32] {
        &self.source_record_fingerprint
    }

    #[cfg(test)]
    fn from_record(
        record_index: u64,
        reason: MigrationDryRunUnresolvedReason,
        record: &[u8],
    ) -> Option<Self> {
        Some(Self {
            record_index,
            reason,
            source_record_fingerprint: fingerprint_record(record)?,
        })
    }
}

/// Closed unresolved-item reasons for migration preview.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MigrationDryRunUnresolvedReason {
    /// The selected target schema does not identify one unique transformation.
    AmbiguousTargetMapping,
}

/// Explicit choice for one unresolved source record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationItemResolution {
    /// Supply exact canonical WorldDB record bytes for the target.
    ReplaceRecord(Vec<u8>),
    /// Explicitly omit this source record from the target migration result.
    OmitRecord,
}

/// An administrator's proposed choice, bound to one dry-run report and unresolved item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationAdminDecision {
    plan_fingerprint: MigrationPlanFingerprint,
    transformer_version: MigrationTransformerVersion,
    source_revision: SchemaRevision,
    source_schema_fingerprint: [u8; 32],
    input_fingerprint: MigrationDryRunInputFingerprint,
    item: MigrationDryRunUnresolvedItem,
    resolution: MigrationItemResolution,
}

impl MigrationAdminDecision {
    /// Binds an explicit resolution to an unresolved item in the supplied report.
    pub fn for_item(
        report: &MigrationDryRunReport,
        item: MigrationDryRunUnresolvedItem,
        resolution: MigrationItemResolution,
    ) -> Result<Self, MigrationDecisionError> {
        if !report.unresolved.contains(&item) {
            return Err(MigrationDecisionError::UnknownUnresolvedItem);
        }
        let input_fingerprint = report
            .input_fingerprint
            .ok_or(MigrationDecisionError::ReportIdentityUnavailable)?;
        Ok(Self {
            plan_fingerprint: report.plan_fingerprint,
            transformer_version: report.transformer_version,
            source_revision: report.source_revision,
            source_schema_fingerprint: report.source_schema_fingerprint,
            input_fingerprint,
            item,
            resolution,
        })
    }

    /// The exact unresolved source item selected by this decision.
    #[must_use]
    pub const fn item(&self) -> MigrationDryRunUnresolvedItem {
        self.item
    }

    /// Explicit resolution recorded for the item.
    #[must_use]
    pub fn resolution(&self) -> &MigrationItemResolution {
        &self.resolution
    }
}

/// Validated exact per-item administrator decisions for one dry-run report.
///
/// This validates decision completeness, canonical replacement framing, and the current
/// `MigrationExecute` policy grant. It is not a migration commit permit: the execution path must
/// recheck current authorization, source OCC, target schema semantics, and commit/audit gates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMigrationDecisions {
    plan_fingerprint: MigrationPlanFingerprint,
    transformer_version: MigrationTransformerVersion,
    source_revision: SchemaRevision,
    source_schema_fingerprint: [u8; 32],
    input_fingerprint: MigrationDryRunInputFingerprint,
    actor: Option<PrincipalId>,
    policy_fingerprint: Option<[u8; 32]>,
    target: PolicyTarget,
    decisions: Vec<MigrationAdminDecision>,
}

impl ValidatedMigrationDecisions {
    /// Validates exactly one authorized decision for every unresolved item.
    pub fn validate(
        plan: &MigrationPlan,
        report: &MigrationDryRunReport,
        mut decisions: Vec<MigrationAdminDecision>,
        policy: &SecurityPolicySnapshot,
        actor: PrincipalId,
        target: PolicyTarget,
    ) -> Result<Self, MigrationDecisionError> {
        if report.plan_fingerprint != plan.fingerprint() {
            return Err(MigrationDecisionError::PlanFingerprintMismatch);
        }
        if report.category != plan.category()
            || report.transformer_version != plan.transformer_version()
        {
            return Err(MigrationDecisionError::PlanParametersMismatch);
        }
        let source = plan.source_schema_precondition();
        if report.source_revision != source.revision()
            || report.source_schema_fingerprint != *source.fingerprint()
        {
            return Err(MigrationDecisionError::SourcePreconditionMismatch);
        }
        let input_fingerprint = report
            .input_fingerprint
            .ok_or(MigrationDecisionError::InputFingerprintUnavailable)?;
        if report.fatal_error.is_some() || report.error_count != 0 {
            return Err(MigrationDecisionError::DryRunContainsErrors);
        }
        if report.omitted_unresolved_count != 0 {
            return Err(MigrationDecisionError::UnresolvedItemsOmitted);
        }
        if report.unresolved.is_empty() && report.output.is_none() {
            return Err(MigrationDecisionError::DryRunIncomplete);
        }

        for (index, item) in report.unresolved.iter().enumerate() {
            if report
                .unresolved
                .iter()
                .take(index)
                .any(|prior| prior == item)
            {
                return Err(MigrationDecisionError::DuplicateUnresolvedItem);
            }
        }

        for decision in &decisions {
            if decision.plan_fingerprint != report.plan_fingerprint
                || decision.transformer_version != report.transformer_version
                || decision.source_revision != report.source_revision
                || decision.source_schema_fingerprint != report.source_schema_fingerprint
                || decision.input_fingerprint != input_fingerprint
            {
                return Err(MigrationDecisionError::StaleDecisionContext {
                    record_index: decision.item.record_index,
                });
            }
            if !report.unresolved.contains(&decision.item) {
                return Err(MigrationDecisionError::UnexpectedDecision {
                    record_index: decision.item.record_index,
                });
            }
        }

        for item in &report.unresolved {
            let matches = decisions
                .iter()
                .filter(|decision| decision.item == *item)
                .count();
            match matches {
                0 => {
                    return Err(MigrationDecisionError::MissingDecision {
                        record_index: item.record_index,
                    });
                }
                1 => {}
                _ => {
                    return Err(MigrationDecisionError::DuplicateDecision {
                        record_index: item.record_index,
                    });
                }
            }
        }
        decisions.sort_by_key(|decision| decision.item);

        let transformer = MigrationTransformer::for_version(plan.transformer_version())
            .map_err(MigrationDecisionError::Transformer)?;
        let mut reserved_bytes = 0_u64;
        let decision_slot_bytes = u64::try_from(decisions.capacity())
            .map_err(|_| MigrationDecisionError::SizeOverflow)?
            .checked_mul(
                u64::try_from(std::mem::size_of::<MigrationAdminDecision>())
                    .map_err(|_| MigrationDecisionError::SizeOverflow)?,
            )
            .ok_or(MigrationDecisionError::SizeOverflow)?;
        reserved_bytes = reserved_bytes
            .checked_add(decision_slot_bytes)
            .ok_or(MigrationDecisionError::SizeOverflow)?;
        for decision in &decisions {
            if let MigrationItemResolution::ReplaceRecord(bytes) = &decision.resolution {
                let byte_count = u64::try_from(bytes.capacity())
                    .map_err(|_| MigrationDecisionError::SizeOverflow)?;
                reserved_bytes = reserved_bytes
                    .checked_add(byte_count)
                    .ok_or(MigrationDecisionError::SizeOverflow)?;
                transformer.validate_record(bytes).map_err(|_| {
                    MigrationDecisionError::InvalidReplacementRecord {
                        record_index: decision.item.record_index,
                    }
                })?;
            }
        }
        let estimate_reserved = report
            .estimate
            .ok_or(MigrationDecisionError::DryRunIncomplete)?
            .reserved_memory_bytes();
        let preflight_reserved = estimate_reserved
            .checked_add(report.diagnostic_memory_bytes)
            .ok_or(MigrationDecisionError::SizeOverflow)?;
        let total_reserved = preflight_reserved
            .checked_add(reserved_bytes)
            .ok_or(MigrationDecisionError::SizeOverflow)?;
        if total_reserved > plan.budget().max_memory_bytes() {
            return Err(MigrationDecisionError::MemoryBudgetExceeded {
                requested: total_reserved,
                limit: plan.budget().max_memory_bytes(),
            });
        }

        let (validated_actor, policy_fingerprint) = if decisions.is_empty() {
            (None, None)
        } else {
            if policy.authorize(actor, Capability::MigrationExecute, target)
                != AuthorizationDecision::Allow
            {
                return Err(MigrationDecisionError::Unauthorized);
            }
            (
                Some(actor),
                Some(policy.effective_capability_fingerprint(actor, target)),
            )
        };

        Ok(Self {
            plan_fingerprint: report.plan_fingerprint,
            transformer_version: report.transformer_version,
            source_revision: report.source_revision,
            source_schema_fingerprint: report.source_schema_fingerprint,
            input_fingerprint,
            actor: validated_actor,
            policy_fingerprint,
            target,
            decisions,
        })
    }

    /// Plan identity to which these decisions apply.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> MigrationPlanFingerprint {
        self.plan_fingerprint
    }

    /// Ordered source fingerprint to which these decisions apply.
    #[must_use]
    pub const fn input_fingerprint(&self) -> MigrationDryRunInputFingerprint {
        self.input_fingerprint
    }

    /// Transformer version fixed by the reviewed plan.
    #[must_use]
    pub const fn transformer_version(&self) -> MigrationTransformerVersion {
        self.transformer_version
    }

    /// Source schema revision fixed by the reviewed report.
    #[must_use]
    pub const fn source_revision(&self) -> SchemaRevision {
        self.source_revision
    }

    /// Source schema fingerprint fixed by the reviewed report.
    #[must_use]
    pub const fn source_schema_fingerprint(&self) -> &[u8; 32] {
        &self.source_schema_fingerprint
    }

    /// Authenticated actor supplied to the policy check, when decisions were required.
    #[must_use]
    pub const fn actor(&self) -> Option<PrincipalId> {
        self.actor
    }

    /// Effective-rights fingerprint from the policy snapshot used to validate the decisions.
    #[must_use]
    pub const fn policy_fingerprint(&self) -> Option<&[u8; 32]> {
        self.policy_fingerprint.as_ref()
    }

    /// Target scope used for the administrative policy check.
    #[must_use]
    pub const fn target(&self) -> PolicyTarget {
        self.target
    }

    /// Canonically ordered, exact per-item decisions.
    #[must_use]
    pub fn decisions(&self) -> &[MigrationAdminDecision] {
        &self.decisions
    }
}

/// Failure while binding an administrative decision set to a dry-run report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationDecisionError {
    /// The immutable plan differs from the plan reviewed in the dry run.
    PlanFingerprintMismatch,
    /// Plan category or transformer version differs from the reviewed report.
    PlanParametersMismatch,
    /// The reviewed source schema is stale or does not match the plan.
    SourcePreconditionMismatch,
    /// The report could not fingerprint the exact ordered input frames.
    InputFingerprintUnavailable,
    /// The report contains a fatal preflight or per-record transform error.
    DryRunContainsErrors,
    /// The report stopped before producing a usable transform estimate/result.
    DryRunIncomplete,
    /// Unresolved items were omitted, so a complete decision set cannot be established.
    UnresolvedItemsOmitted,
    /// The report repeats the same unresolved item.
    DuplicateUnresolvedItem,
    /// The decision refers to a different plan, source schema, transformer, or input set.
    StaleDecisionContext { record_index: u64 },
    /// A decision was supplied for an item absent from the reviewed report.
    UnexpectedDecision { record_index: u64 },
    /// An unresolved item has no explicit decision.
    MissingDecision { record_index: u64 },
    /// An unresolved item has more than one explicit decision.
    DuplicateDecision { record_index: u64 },
    /// A replacement is not a valid canonical WorldDB record frame.
    InvalidReplacementRecord { record_index: u64 },
    /// The actor lacks the MigrationExecute right for this exact target scope.
    Unauthorized,
    /// Decision storage or transform admission exceeds the plan memory budget.
    MemoryBudgetExceeded { requested: u64, limit: u64 },
    /// A decision payload or reservation size overflowed the canonical range.
    SizeOverflow,
    /// The supplied unresolved item is not part of the source report.
    UnknownUnresolvedItem,
    /// A decision was requested for a report without a complete source identity.
    ReportIdentityUnavailable,
    /// A replacement validator could not be loaded for the plan version.
    Transformer(MigrationTransformerError),
}

impl fmt::Display for MigrationDecisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanFingerprintMismatch => {
                formatter.write_str("dry-run plan fingerprint mismatch")
            }
            Self::PlanParametersMismatch => formatter.write_str("dry-run plan parameters mismatch"),
            Self::SourcePreconditionMismatch => {
                formatter.write_str("dry-run source precondition mismatch")
            }
            Self::InputFingerprintUnavailable => {
                formatter.write_str("dry-run input fingerprint unavailable")
            }
            Self::DryRunContainsErrors => formatter.write_str("dry-run contains errors"),
            Self::DryRunIncomplete => formatter.write_str("dry-run did not complete"),
            Self::UnresolvedItemsOmitted => formatter.write_str("dry-run omitted unresolved items"),
            Self::DuplicateUnresolvedItem => {
                formatter.write_str("dry-run repeats an unresolved item")
            }
            Self::StaleDecisionContext { record_index } => write!(
                formatter,
                "decision for record {record_index} belongs to another dry-run context"
            ),
            Self::UnexpectedDecision { record_index } => write!(
                formatter,
                "decision for record {record_index} is not in the dry-run report"
            ),
            Self::MissingDecision { record_index } => write!(
                formatter,
                "unresolved record {record_index} has no decision"
            ),
            Self::DuplicateDecision { record_index } => write!(
                formatter,
                "unresolved record {record_index} has duplicate decisions"
            ),
            Self::InvalidReplacementRecord { record_index } => write!(
                formatter,
                "replacement for record {record_index} is not a canonical WorldDB record"
            ),
            Self::Unauthorized => {
                formatter.write_str("MigrationExecute is not authorized for this target")
            }
            Self::MemoryBudgetExceeded { requested, limit } => write!(
                formatter,
                "migration decisions reserve {requested} bytes, limit is {limit}"
            ),
            Self::SizeOverflow => formatter.write_str("migration decision size overflowed"),
            Self::UnknownUnresolvedItem => {
                formatter.write_str("unresolved item is not in the supplied dry-run report")
            }
            Self::ReportIdentityUnavailable => {
                formatter.write_str("dry-run report identity is unavailable")
            }
            Self::Transformer(error) => {
                write!(formatter, "migration transformer unavailable: {error}")
            }
        }
    }
}

impl std::error::Error for MigrationDecisionError {}

fn fingerprint_input(records: &[Vec<u8>]) -> Option<MigrationDryRunInputFingerprint> {
    let count = u64::try_from(records.len()).ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.MigrationDryRunInput.v1\0");
    hasher.update(&count.to_be_bytes());
    for record in records {
        let length = u64::try_from(record.len()).ok()?;
        hasher.update(&length.to_be_bytes());
        hasher.update(record);
    }
    Some(MigrationDryRunInputFingerprint(
        *hasher.finalize().as_bytes(),
    ))
}

#[cfg(test)]
fn fingerprint_record(record: &[u8]) -> Option<[u8; 32]> {
    let length = u64::try_from(record.len()).ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.MigrationDryRunRecord.v1\0");
    hasher.update(&length.to_be_bytes());
    hasher.update(record);
    Some(*hasher.finalize().as_bytes())
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
    use super::{
        MigrationAdminDecision, MigrationDecisionError, MigrationDryRun,
        MigrationDryRunUnresolvedItem, MigrationDryRunUnresolvedReason, MigrationItemResolution,
        ValidatedMigrationDecisions,
    };
    use crate::ids::{
        DomainId, EntityId, EntityTypeId, MigrationId, MigrationStepId, PolicyRuleId, PredicateId,
        PrincipalId, Revision, SchemaRevision,
    };
    use crate::{
        Capability, CapabilityGrant, CapabilityRule, Entity, GrantEffect, JobBudget,
        MigrationCalendarShift, MigrationCategory, MigrationPlan, MigrationPlanSpec,
        MigrationTargetSchema, MigrationTransformer, MigrationTransformerVersion, PolicyScope,
        PolicySubject, Principal, Record, SchemaDefinitionId, SchemaIdentityTransition,
        SecurityPolicySnapshot, SourceSchemaPrecondition, encode_record,
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
            step_targets: None,
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
        let mut records = Vec::new();
        for tail in [5, 6] {
            let entity = Entity::new(
                uuid::<EntityId>(tail)?,
                uuid::<EntityTypeId>(7)?,
                Revision::GENESIS,
            );
            records
                .push(encode_record(&Record::Entity(entity)).map_err(|error| error.to_string())?);
        }
        Ok(records)
    }

    fn migration_policy(
        principal: PrincipalId,
        allow: bool,
    ) -> Result<SecurityPolicySnapshot, String> {
        let rules = if allow {
            vec![CapabilityRule::new(
                uuid::<PolicyRuleId>(11)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::MigrationExecute, GrantEffect::Allow),
                PolicyScope::project(),
            )]
        } else {
            Vec::new()
        };
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(|error| error.to_string())
    }

    fn unresolved_report(
        plan: &MigrationPlan,
        records: &[Vec<u8>],
    ) -> Result<
        (
            super::MigrationDryRunReport,
            Vec<MigrationDryRunUnresolvedItem>,
        ),
        String,
    > {
        let mut report = MigrationDryRun::run(
            plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            records,
        );
        report.output = None;
        let mut items = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let record_index = u64::try_from(index).map_err(|error| error.to_string())?;
            let item = MigrationDryRunUnresolvedItem::from_record(
                record_index,
                MigrationDryRunUnresolvedReason::AmbiguousTargetMapping,
                record,
            )
            .ok_or("could not fingerprint unresolved source record")?;
            report.unresolved.push(item);
            items.push(item);
        }
        Ok((report, items))
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

    #[test]
    fn report_binds_plan_schema_and_exact_ordered_input() -> Result<(), String> {
        let plan = plan(MigrationCategory::Restrictive, 10, 1024 * 1024)?;
        let records = records()?;
        let report = MigrationDryRun::run(
            &plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            &records,
        );
        let input_fingerprint = report
            .input_fingerprint()
            .ok_or("input fingerprint missing")?;
        let mut reversed = Vec::new();
        for record in records.iter().rev() {
            reversed.push(record.clone());
        }
        let reversed_report = MigrationDryRun::run(
            &plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            &reversed,
        );

        assert_eq!(report.category(), MigrationCategory::Restrictive);
        assert_eq!(
            report.source_revision(),
            plan.source_schema_precondition().revision()
        );
        assert_eq!(report.source_schema_fingerprint(), &[0x11; 32]);
        assert_ne!(
            input_fingerprint,
            reversed_report
                .input_fingerprint()
                .ok_or("reversed input fingerprint missing")?
        );
        Ok(())
    }

    #[test]
    fn restrictive_unresolved_items_require_one_authorized_decision_each() -> Result<(), String> {
        let plan = plan(MigrationCategory::Restrictive, 10, 1024 * 1024)?;
        let records = records()?;
        let (report, items) = unresolved_report(&plan, &records)?;
        let first = *items.first().ok_or("first unresolved item missing")?;
        let principal = uuid::<PrincipalId>(12)?;
        let allow = migration_policy(principal, true)?;
        let deny = migration_policy(principal, false)?;
        let replacement = records.get(1).ok_or("replacement record missing")?.clone();
        let decision = MigrationAdminDecision::for_item(
            &report,
            first,
            MigrationItemResolution::ReplaceRecord(replacement),
        )
        .map_err(|error| error.to_string())?;
        let second = *items.get(1).ok_or("second unresolved item missing")?;
        let second_decision =
            MigrationAdminDecision::for_item(&report, second, MigrationItemResolution::OmitRecord)
                .map_err(|error| error.to_string())?;

        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &plan,
                &report,
                vec![decision.clone()],
                &allow,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::MissingDecision { record_index: 1 })
        );
        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &plan,
                &report,
                vec![decision.clone(), decision.clone()],
                &allow,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::DuplicateDecision { record_index: 0 })
        );
        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &plan,
                &report,
                vec![decision.clone(), second_decision.clone()],
                &deny,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::Unauthorized)
        );
        let mut over_budget_decisions = Vec::with_capacity(20_000);
        over_budget_decisions.push(decision.clone());
        over_budget_decisions.push(second_decision.clone());
        assert!(matches!(
            ValidatedMigrationDecisions::validate(
                &plan,
                &report,
                over_budget_decisions,
                &allow,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::MemoryBudgetExceeded { .. })
        ));

        let validated = ValidatedMigrationDecisions::validate(
            &plan,
            &report,
            vec![second_decision, decision],
            &allow,
            principal,
            crate::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(validated.actor(), Some(principal));
        assert!(validated.policy_fingerprint().is_some());
        assert_eq!(validated.decisions().len(), 2);
        assert_eq!(
            validated
                .decisions()
                .first()
                .ok_or("canonical first decision missing")?
                .item()
                .record_index(),
            0
        );
        assert_eq!(
            validated
                .decisions()
                .get(1)
                .ok_or("canonical second decision missing")?
                .item()
                .record_index(),
            1
        );
        Ok(())
    }

    #[test]
    fn stale_or_invalid_admin_decisions_fail_closed() -> Result<(), String> {
        let migration_plan = plan(MigrationCategory::Breaking, 10, 1024 * 1024)?;
        let records = records()?;
        let (report, items) = unresolved_report(&migration_plan, &records)?;
        let item = *items.first().ok_or("unresolved item missing")?;
        let principal = uuid::<PrincipalId>(13)?;
        let allow = migration_policy(principal, true)?;
        let invalid_replacement = MigrationAdminDecision::for_item(
            &report,
            item,
            MigrationItemResolution::ReplaceRecord(vec![0xff]),
        )
        .map_err(|error| error.to_string())?;
        let second_item = *items.get(1).ok_or("second unresolved item missing")?;
        let second_decision = MigrationAdminDecision::for_item(
            &report,
            second_item,
            MigrationItemResolution::OmitRecord,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &migration_plan,
                &report,
                vec![invalid_replacement, second_decision],
                &allow,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::InvalidReplacementRecord { record_index: 0 })
        );

        let changed_records = vec![records.get(1).ok_or("source record missing")?.clone()];
        let (changed_report, changed_items) = unresolved_report(&migration_plan, &changed_records)?;
        let stale =
            MigrationAdminDecision::for_item(&report, item, MigrationItemResolution::OmitRecord)
                .map_err(|error| error.to_string())?;
        let changed_item = *changed_items
            .first()
            .ok_or("changed unresolved item missing")?;
        let current_decision = MigrationAdminDecision::for_item(
            &changed_report,
            changed_item,
            MigrationItemResolution::OmitRecord,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &migration_plan,
                &changed_report,
                vec![stale],
                &allow,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::StaleDecisionContext { record_index: 0 })
        );
        let changed_plan = plan(MigrationCategory::Breaking, 11, 1024 * 1024)?;
        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &changed_plan,
                &changed_report,
                vec![current_decision],
                &allow,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::PlanFingerprintMismatch)
        );
        Ok(())
    }

    #[test]
    fn warning_unresolved_and_error_remain_distinct_states() -> Result<(), String> {
        let plan = plan(MigrationCategory::Restrictive, 10, 1024 * 1024)?;
        let records = records()?;
        let (mut report, items) = unresolved_report(&plan, &records)?;
        report.warnings.push(super::MigrationDryRunWarning {
            code: super::MigrationDryRunWarningCode::DeprecatedTargetDefinition,
            record_index: Some(0),
        });
        let principal = uuid::<PrincipalId>(14)?;
        let policy = migration_policy(principal, true)?;
        let decision = MigrationAdminDecision::for_item(
            &report,
            *items.first().ok_or("unresolved item missing")?,
            MigrationItemResolution::OmitRecord,
        )
        .map_err(|error| error.to_string())?;
        let second_decision = MigrationAdminDecision::for_item(
            &report,
            *items.get(1).ok_or("second unresolved item missing")?,
            MigrationItemResolution::OmitRecord,
        )
        .map_err(|error| error.to_string())?;
        let validated = ValidatedMigrationDecisions::validate(
            &plan,
            &report,
            vec![decision, second_decision],
            &policy,
            principal,
            crate::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;

        assert_eq!(report.warnings().len(), 1);
        assert_eq!(report.unresolved_items().len(), 2);
        assert!(report.errors().is_empty());
        assert!(report.fatal_error().is_none());
        assert!(!report.preflight_complete());
        assert_eq!(validated.decisions().len(), 2);
        Ok(())
    }

    #[test]
    fn transform_errors_cannot_be_overridden_by_admin_decisions() -> Result<(), String> {
        let plan = plan(MigrationCategory::Breaking, 10, 1024 * 1024)?;
        let mut records = records()?;
        *records.first_mut().ok_or("first record missing")? = vec![0xff];
        let report = MigrationDryRun::run(
            &plan,
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [0x11; 32],
            &records,
        );
        let principal = uuid::<PrincipalId>(15)?;
        let policy = migration_policy(principal, true)?;

        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &plan,
                &report,
                Vec::new(),
                &policy,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::DryRunContainsErrors)
        );
        assert_eq!(report.errors().len(), 1);
        assert!(report.unresolved_items().is_empty());
        assert!(report.warnings().is_empty());
        Ok(())
    }

    #[test]
    fn omitted_unresolved_details_never_look_fully_resolved() -> Result<(), String> {
        let plan = plan(MigrationCategory::Breaking, 10, 1024 * 1024)?;
        let records = records()?;
        let (mut report, items) = unresolved_report(&plan, &records)?;
        report.omitted_unresolved_count = 1;
        let principal = uuid::<PrincipalId>(16)?;
        let policy = migration_policy(principal, true)?;
        let mut decisions = Vec::new();
        for item in &items {
            decisions.push(
                MigrationAdminDecision::for_item(
                    &report,
                    *item,
                    MigrationItemResolution::OmitRecord,
                )
                .map_err(|error| error.to_string())?,
            );
        }

        assert!(!report.preflight_complete());
        assert_eq!(report.omitted_unresolved_item_count(), 1);
        assert_eq!(
            ValidatedMigrationDecisions::validate(
                &plan,
                &report,
                decisions,
                &policy,
                principal,
                crate::PolicyTarget::default(),
            ),
            Err(MigrationDecisionError::UnresolvedItemsOmitted)
        );
        Ok(())
    }
}
