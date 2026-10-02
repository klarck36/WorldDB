//! Immutable migration plans, schema impact classifications, and run identities.

use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU32;

use crate::ids::{
    DomainId, EntityTypeId, EventKindId, LayerId, MigrationId, MigrationRunId, MigrationStepId,
    OperationId, PredicateId, SchemaRevision, TimelineId,
};
use crate::{CalendarPeriod, JobBudget};

/// The complete, closed set of migration compatibility categories.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MigrationCategory {
    /// The change affects descriptive metadata only.
    MetadataOnly,
    /// The change adds a definition or capability without invalidating existing use.
    Additive,
    /// The change widens a constraint while preserving existing meaning.
    CompatibleConstraintChange,
    /// The change prevents writes that were previously valid.
    Restrictive,
    /// The change cannot preserve the old meaning under the same schema identity.
    Breaking,
}

impl MigrationCategory {
    /// Effect class used by the schema-change contract.
    #[must_use]
    pub const fn impact(self) -> SchemaChangeImpact {
        match self {
            Self::MetadataOnly | Self::Additive | Self::CompatibleConstraintChange => {
                SchemaChangeImpact::Compatible
            }
            Self::Restrictive => SchemaChangeImpact::Restrictive,
            Self::Breaking => SchemaChangeImpact::Breaking,
        }
    }

    const fn wire_tag(self) -> u8 {
        match self {
            Self::MetadataOnly => 1,
            Self::Additive => 2,
            Self::CompatibleConstraintChange => 3,
            Self::Restrictive => 4,
            Self::Breaking => 5,
        }
    }

    const fn rank(self) -> u8 {
        self.wire_tag()
    }
}

/// Coarse semantic effect of a schema change, independent of its migration mechanism.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SchemaChangeImpact {
    /// Existing reads, writes, and interpretations remain valid.
    Compatible,
    /// Some previously valid new writes or references become disallowed.
    Restrictive,
    /// Existing meaning cannot remain attached to the same stable identity.
    Breaking,
}

/// Stable identity of a schema definition that can be classified in a migration plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SchemaDefinitionId {
    /// Identity of a revisioned layer definition.
    Layer(LayerId),
    /// Identity of a revisioned entity type definition.
    EntityType(EntityTypeId),
    /// Identity of a revisioned predicate definition.
    Predicate(PredicateId),
    /// Identity of a revisioned event-kind definition.
    EventKind(EventKindId),
}

impl SchemaDefinitionId {
    const fn family_tag(self) -> u8 {
        match self {
            Self::Layer(_) => 1,
            Self::EntityType(_) => 2,
            Self::Predicate(_) => 3,
            Self::EventKind(_) => 4,
        }
    }

    fn update_fingerprint(self, hasher: &mut blake3::Hasher) {
        hasher.update(&[self.family_tag()]);
        match self {
            Self::Layer(id) => hasher.update(id.as_bytes()),
            Self::EntityType(id) => hasher.update(id.as_bytes()),
            Self::Predicate(id) => hasher.update(id.as_bytes()),
            Self::EventKind(id) => hasher.update(id.as_bytes()),
        };
    }

    /// The wire variant followed by the typed identity's canonical UUID bytes.
    pub(crate) fn to_wire_bytes(self) -> [u8; 17] {
        let mut bytes = [0; 17];
        bytes[0] = self.family_tag();
        match self {
            Self::Layer(id) => bytes[1..].copy_from_slice(&id.to_bytes()),
            Self::EntityType(id) => bytes[1..].copy_from_slice(&id.to_bytes()),
            Self::Predicate(id) => bytes[1..].copy_from_slice(&id.to_bytes()),
            Self::EventKind(id) => bytes[1..].copy_from_slice(&id.to_bytes()),
        }
        bytes
    }

    /// Decodes one closed schema-identity wire value.
    pub(crate) fn from_wire_bytes(bytes: &[u8]) -> Option<Self> {
        let (tag, raw_id) = bytes.split_first()?;
        let raw_id: [u8; 16] = raw_id.try_into().ok()?;
        match tag {
            1 => LayerId::try_from_bytes(raw_id).ok().map(Self::Layer),
            2 => EntityTypeId::try_from_bytes(raw_id)
                .ok()
                .map(Self::EntityType),
            3 => PredicateId::try_from_bytes(raw_id)
                .ok()
                .map(Self::Predicate),
            4 => EventKindId::try_from_bytes(raw_id)
                .ok()
                .map(Self::EventKind),
            _ => None,
        }
    }
}

/// One explicitly classified change to a stable schema-definition identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaIdentityTransition {
    source: Option<SchemaDefinitionId>,
    target: Option<SchemaDefinitionId>,
    category: MigrationCategory,
}

impl SchemaIdentityTransition {
    /// Validates a typed source/target identity transition against its category.
    pub fn new(
        source: Option<SchemaDefinitionId>,
        target: Option<SchemaDefinitionId>,
        category: MigrationCategory,
    ) -> Result<Self, SchemaIdentityTransitionError> {
        if source.is_none() && target.is_none() {
            return Err(SchemaIdentityTransitionError::MissingIdentities);
        }
        if let (Some(source), Some(target)) = (source, target) {
            if source.family_tag() != target.family_tag() {
                return Err(SchemaIdentityTransitionError::IdentityFamilyMismatch);
            }
        }

        let valid_shape = match (category, source, target) {
            (MigrationCategory::MetadataOnly, Some(source), Some(target)) => source == target,
            (MigrationCategory::Additive, None, Some(_)) => true,
            (MigrationCategory::Additive, Some(source), Some(target)) => source == target,
            (MigrationCategory::CompatibleConstraintChange, Some(source), Some(target)) => {
                source == target
            }
            (MigrationCategory::Restrictive, Some(source), Some(target)) => source == target,
            (MigrationCategory::Breaking, Some(source), Some(target)) => source != target,
            _ => false,
        };
        if !valid_shape {
            return Err(if category == MigrationCategory::Breaking {
                SchemaIdentityTransitionError::BreakingRequiresNewSchemaId
            } else {
                SchemaIdentityTransitionError::IdentityShapeDoesNotMatchCategory
            });
        }

        Ok(Self {
            source,
            target,
            category,
        })
    }

    /// Stable identity before the migration, or `None` for a newly added definition.
    #[must_use]
    pub const fn source(self) -> Option<SchemaDefinitionId> {
        self.source
    }

    /// Stable identity after the migration, or `None` for a removed definition.
    #[must_use]
    pub const fn target(self) -> Option<SchemaDefinitionId> {
        self.target
    }

    /// Classification assigned to this schema-definition change.
    #[must_use]
    pub const fn category(self) -> MigrationCategory {
        self.category
    }
}

/// Invalid schema identity transition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SchemaIdentityTransitionError {
    /// At least one source or target identity is required.
    MissingIdentities,
    /// Source and target identities belong to different schema-definition families.
    IdentityFamilyMismatch,
    /// A breaking semantic change must use a new identity in the same family.
    BreakingRequiresNewSchemaId,
    /// The source/target presence or identity reuse does not fit its category.
    IdentityShapeDoesNotMatchCategory,
}

impl fmt::Display for SchemaIdentityTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingIdentities => formatter.write_str("schema change has no identities"),
            Self::IdentityFamilyMismatch => {
                formatter.write_str("schema change switches identity families")
            }
            Self::BreakingRequiresNewSchemaId => {
                formatter.write_str("breaking schema change requires a new schema identity")
            }
            Self::IdentityShapeDoesNotMatchCategory => {
                formatter.write_str("schema identity transition does not match its category")
            }
        }
    }
}

impl std::error::Error for SchemaIdentityTransitionError {}

/// Exact source-schema revision and fingerprint required before a migration starts or resumes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceSchemaPrecondition {
    revision: SchemaRevision,
    fingerprint: [u8; 32],
}

impl SourceSchemaPrecondition {
    /// Binds the source revision to its canonical schema fingerprint.
    #[must_use]
    pub const fn new(revision: SchemaRevision, fingerprint: [u8; 32]) -> Self {
        Self {
            revision,
            fingerprint,
        }
    }

    /// Source schema revision required by the immutable plan.
    #[must_use]
    pub const fn revision(self) -> SchemaRevision {
        self.revision
    }

    /// Source schema fingerprint required by the immutable plan.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

/// Target schema revision and canonical fingerprint produced by a migration plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationTargetSchema {
    revision: SchemaRevision,
    fingerprint: [u8; 32],
}

impl MigrationTargetSchema {
    /// Binds the target revision to its canonical schema fingerprint.
    #[must_use]
    pub const fn new(revision: SchemaRevision, fingerprint: [u8; 32]) -> Self {
        Self {
            revision,
            fingerprint,
        }
    }

    /// Target schema revision.
    #[must_use]
    pub const fn revision(self) -> SchemaRevision {
        self.revision
    }

    /// Target schema fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

/// Fingerprinted schema state published by one ordered migration step.
///
/// Plans may omit these checkpoints for wire compatibility, but migration
/// execution requires a checkpoint for every step.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationStepTargetSchema {
    step_id: MigrationStepId,
    schema: MigrationTargetSchema,
}

impl MigrationStepTargetSchema {
    /// Binds one stable plan step to the exact schema state it publishes.
    #[must_use]
    pub const fn new(step_id: MigrationStepId, schema: MigrationTargetSchema) -> Self {
        Self { step_id, schema }
    }

    /// Stable step identity from the ordered migration plan.
    #[must_use]
    pub const fn step_id(self) -> MigrationStepId {
        self.step_id
    }

    /// Exact intermediate schema revision and fingerprint.
    #[must_use]
    pub const fn schema(self) -> MigrationTargetSchema {
        self.schema
    }
}

/// Positive, explicit version for the migration transformer's implementation contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationTransformerVersion(NonZeroU32);

impl MigrationTransformerVersion {
    /// Creates a non-zero transformer version.
    pub fn new(version: u32) -> Result<Self, MigrationPlanError> {
        NonZeroU32::new(version)
            .map(Self)
            .ok_or(MigrationPlanError::ZeroTransformerVersion)
    }

    /// Numeric version stored in the immutable plan.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0.get()
    }
}

/// Direction of an explicit Gregorian calendar-period migration shift.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MigrationCalendarDirection {
    /// Move the value backward by the plan's calendar period.
    Past,
    /// Move the value forward by the plan's calendar period.
    Future,
}

impl MigrationCalendarDirection {
    pub(crate) const fn wire_tag(self) -> u8 {
        match self {
            Self::Past => 1,
            Self::Future => 2,
        }
    }
}

/// Explicit, fingerprint-bound calendar transformation parameters.
///
/// The timeline ID and UTC epoch come from the plan's pinned source schema.
/// The migration uses the project's closed proleptic-Gregorian UTC profile and
/// never consults a host clock, timezone, locale, or time-unit inference.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationCalendarShift {
    timeline_id: TimelineId,
    epoch_unix_nanoseconds: i128,
    period: CalendarPeriod,
    direction: MigrationCalendarDirection,
}

impl MigrationCalendarShift {
    /// Creates an explicit shift bound to one registered timeline profile.
    #[must_use]
    pub const fn new(
        timeline_id: TimelineId,
        epoch_unix_nanoseconds: i128,
        period: CalendarPeriod,
        direction: MigrationCalendarDirection,
    ) -> Self {
        Self {
            timeline_id,
            epoch_unix_nanoseconds,
            period,
            direction,
        }
    }

    /// Stable timeline identity whose source schema supplies the calendar profile.
    #[must_use]
    pub const fn timeline_id(self) -> TimelineId {
        self.timeline_id
    }

    /// UTC epoch offset for the timeline's ProlepticGregorianUtc profile.
    #[must_use]
    pub const fn epoch_unix_nanoseconds(self) -> i128 {
        self.epoch_unix_nanoseconds
    }

    /// Explicit canonical calendar period.
    #[must_use]
    pub const fn period(self) -> CalendarPeriod {
        self.period
    }

    /// Explicit direction; no direction is inferred from source values.
    #[must_use]
    pub const fn direction(self) -> MigrationCalendarDirection {
        self.direction
    }
}

/// Stable BLAKE3 digest of every semantic field in an immutable migration plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationPlanFingerprint([u8; 32]);

impl MigrationPlanFingerprint {
    /// Returns the exact fingerprint bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Inputs consumed when constructing an immutable migration plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlanSpec {
    /// Stable identity for the logical migration.
    pub migration_id: MigrationId,
    /// Exact effect category for the planned schema changes.
    pub category: MigrationCategory,
    /// Required source revision and fingerprint.
    pub source_schema: SourceSchemaPrecondition,
    /// Exact target revision and fingerprint.
    pub target_schema: MigrationTargetSchema,
    /// Ordered, unique step identities.
    pub steps: Vec<MigrationStepId>,
    /// Optional exact schema checkpoint after every step; required by execution.
    pub step_targets: Option<Vec<MigrationStepTargetSchema>>,
    /// Canonically ordered schema identity changes represented by the plan.
    pub schema_changes: Vec<SchemaIdentityTransition>,
    /// Version of the deterministic transformer to be used by later run tasks.
    pub transformer_version: MigrationTransformerVersion,
    /// Optional explicit time shift, including every typed parameter it uses.
    pub calendar_shift: Option<MigrationCalendarShift>,
    /// Finite maximum work and memory allowance for the migration.
    pub budget: JobBudget,
}

/// Immutable, canonical plan for one logical schema migration.
///
/// Its fields stay private after validation, so callers cannot change a plan without rebuilding
/// and refingerprinting it.
///
/// ```compile_fail
/// use worlddb_core::MigrationPlan;
///
/// fn mutate_plan(plan: &mut MigrationPlan) {
///     plan.steps.clear();
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan {
    migration_id: MigrationId,
    category: MigrationCategory,
    source_schema: SourceSchemaPrecondition,
    target_schema: MigrationTargetSchema,
    steps: Vec<MigrationStepId>,
    step_targets: Option<Vec<MigrationStepTargetSchema>>,
    schema_changes: Vec<SchemaIdentityTransition>,
    transformer_version: MigrationTransformerVersion,
    calendar_shift: Option<MigrationCalendarShift>,
    budget: JobBudget,
    fingerprint: MigrationPlanFingerprint,
}

impl MigrationPlan {
    /// Builds a validated immutable plan and computes its canonical fingerprint.
    pub fn new(mut spec: MigrationPlanSpec) -> Result<Self, MigrationPlanError> {
        if spec.steps.is_empty() {
            return Err(MigrationPlanError::EmptySteps);
        }
        let mut unique_steps = BTreeSet::new();
        for step in &spec.steps {
            if !unique_steps.insert(*step) {
                return Err(MigrationPlanError::DuplicateStepId(*step));
            }
        }
        if let Some(step_targets) = &spec.step_targets {
            validate_step_targets(
                &spec.steps,
                spec.source_schema,
                spec.target_schema,
                step_targets,
            )?;
        }
        if spec.schema_changes.is_empty() {
            return Err(MigrationPlanError::EmptySchemaChanges);
        }
        spec.schema_changes.sort_unstable();
        if spec.schema_changes.windows(2).any(|pair| match pair {
            [first, second] => first.source == second.source && first.target == second.target,
            _ => false,
        }) {
            return Err(MigrationPlanError::DuplicateSchemaChange);
        }
        if spec.target_schema.revision() <= spec.source_schema.revision() {
            return Err(MigrationPlanError::TargetSchemaNotLater);
        }
        let highest_change_rank = spec
            .schema_changes
            .iter()
            .map(|change| change.category().rank())
            .max()
            .unwrap_or(0);
        if highest_change_rank != spec.category.rank() {
            return Err(MigrationPlanError::CategoryDoesNotMatchChanges);
        }

        let fingerprint = compute_plan_fingerprint(&spec);
        Ok(Self {
            migration_id: spec.migration_id,
            category: spec.category,
            source_schema: spec.source_schema,
            target_schema: spec.target_schema,
            steps: spec.steps,
            step_targets: spec.step_targets,
            schema_changes: spec.schema_changes,
            transformer_version: spec.transformer_version,
            calendar_shift: spec.calendar_shift,
            budget: spec.budget,
            fingerprint,
        })
    }

    /// Stable identity of the logical migration.
    #[must_use]
    pub const fn migration_id(&self) -> MigrationId {
        self.migration_id
    }

    /// Exact effect category assigned to the plan.
    #[must_use]
    pub const fn category(&self) -> MigrationCategory {
        self.category
    }

    /// Exact schema revision and fingerprint required to start or resume.
    #[must_use]
    pub const fn source_schema_precondition(&self) -> SourceSchemaPrecondition {
        self.source_schema
    }

    /// Target schema revision and fingerprint declared by the plan.
    #[must_use]
    pub const fn target_schema(&self) -> MigrationTargetSchema {
        self.target_schema
    }

    /// Ordered steps of the plan.
    #[must_use]
    pub fn steps(&self) -> &[MigrationStepId] {
        &self.steps
    }

    /// Exact schema checkpoint after each ordered step, when present.
    #[must_use]
    pub fn step_targets(&self) -> Option<&[MigrationStepTargetSchema]> {
        self.step_targets.as_deref()
    }

    /// Canonically ordered schema identity transitions classified by this plan.
    #[must_use]
    pub fn schema_changes(&self) -> &[SchemaIdentityTransition] {
        &self.schema_changes
    }

    /// Transformer implementation version fixed by the plan.
    #[must_use]
    pub const fn transformer_version(&self) -> MigrationTransformerVersion {
        self.transformer_version
    }

    /// Optional explicit calendar transformation committed by this plan.
    #[must_use]
    pub const fn calendar_shift(&self) -> Option<MigrationCalendarShift> {
        self.calendar_shift
    }

    /// Finite work and memory budget fixed by the plan.
    #[must_use]
    pub const fn budget(&self) -> JobBudget {
        self.budget
    }

    /// Canonical plan fingerprint computed from all plan fields.
    #[must_use]
    pub const fn fingerprint(&self) -> MigrationPlanFingerprint {
        self.fingerprint
    }

    /// Rejects start/resume when either source revision or fingerprint changed.
    pub fn validate_source_schema(
        &self,
        actual_revision: SchemaRevision,
        actual_fingerprint: [u8; 32],
    ) -> Result<(), MigrationPlanError> {
        if actual_revision != self.source_schema.revision
            || actual_fingerprint != self.source_schema.fingerprint
        {
            return Err(MigrationPlanError::SourceSchemaPreconditionMismatch);
        }
        Ok(())
    }

    /// Checks both the exact source precondition and the available transformer version at start.
    pub fn validate_start(
        &self,
        actual_revision: SchemaRevision,
        actual_fingerprint: [u8; 32],
        transformer_version: MigrationTransformerVersion,
    ) -> Result<(), MigrationPlanError> {
        self.validate_source_schema(actual_revision, actual_fingerprint)?;
        self.validate_transformer_version(transformer_version)
    }

    /// Checks a resumed run's plan identity, source precondition, and transformer version.
    pub fn validate_resume(
        &self,
        run: MigrationRun,
        actual_revision: SchemaRevision,
        actual_fingerprint: [u8; 32],
        transformer_version: MigrationTransformerVersion,
    ) -> Result<(), MigrationPlanError> {
        if run.migration_id() != self.migration_id {
            return Err(MigrationPlanError::RunPlanIdentityMismatch);
        }
        self.validate_start(actual_revision, actual_fingerprint, transformer_version)
    }

    /// Rejects execution when the available implementation differs from the plan's version.
    pub fn validate_transformer_version(
        &self,
        actual: MigrationTransformerVersion,
    ) -> Result<(), MigrationPlanError> {
        if actual != self.transformer_version {
            return Err(MigrationPlanError::TransformerVersionMismatch {
                planned: self.transformer_version,
                actual,
            });
        }
        Ok(())
    }

    /// Verifies an embedded or persisted fingerprint against the complete plan.
    pub fn verify_fingerprint(
        &self,
        expected: MigrationPlanFingerprint,
    ) -> Result<(), MigrationPlanError> {
        if self.fingerprint != expected {
            return Err(MigrationPlanError::FingerprintMismatch);
        }
        Ok(())
    }
}

fn compute_plan_fingerprint(spec: &MigrationPlanSpec) -> MigrationPlanFingerprint {
    let mut hasher = blake3::Hasher::new();
    if spec.step_targets.is_some() {
        hasher.update(b"WorldDB.MigrationPlan.v3\0");
    } else if spec.calendar_shift.is_some() {
        hasher.update(b"WorldDB.MigrationPlan.v2\0");
    } else {
        // Plans without a calendar transform retain the M7-01 fingerprint contract.
        hasher.update(b"WorldDB.MigrationPlan.v1\0");
    }
    hasher.update(&spec.migration_id.to_bytes());
    hasher.update(&[spec.category.wire_tag()]);
    hasher.update(&spec.source_schema.revision.revision().value().to_be_bytes());
    hasher.update(&spec.source_schema.fingerprint);
    hasher.update(&spec.target_schema.revision.revision().value().to_be_bytes());
    hasher.update(&spec.target_schema.fingerprint);
    hasher.update(
        &u64::try_from(spec.steps.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for step in &spec.steps {
        hasher.update(&step.to_bytes());
    }
    hasher.update(
        &u64::try_from(spec.schema_changes.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for change in &spec.schema_changes {
        update_optional_schema_id(&mut hasher, change.source);
        update_optional_schema_id(&mut hasher, change.target);
        hasher.update(&[change.category.wire_tag()]);
    }
    hasher.update(&spec.transformer_version.value().to_be_bytes());
    if let Some(shift) = spec.calendar_shift {
        hasher.update(&[1]);
        // Profile tag 1 is the closed ProlepticGregorianUtc calendar profile.
        hasher.update(&[1]);
        hasher.update(&shift.timeline_id.to_bytes());
        hasher.update(&shift.epoch_unix_nanoseconds.to_be_bytes());
        hasher.update(&shift.period.years().to_be_bytes());
        hasher.update(&[shift.period.months()]);
        hasher.update(&shift.period.days().to_be_bytes());
        hasher.update(&[shift.direction.wire_tag()]);
    }
    if let Some(step_targets) = &spec.step_targets {
        hasher.update(
            &u64::try_from(step_targets.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for target in step_targets {
            hasher.update(&target.step_id.to_bytes());
            hasher.update(&target.schema.revision.revision().value().to_be_bytes());
            hasher.update(&target.schema.fingerprint);
        }
    }
    hasher.update(&spec.budget.max_work_units().to_be_bytes());
    hasher.update(&spec.budget.max_memory_bytes().to_be_bytes());
    MigrationPlanFingerprint(*hasher.finalize().as_bytes())
}

fn validate_step_targets(
    steps: &[MigrationStepId],
    source: SourceSchemaPrecondition,
    target: MigrationTargetSchema,
    step_targets: &[MigrationStepTargetSchema],
) -> Result<(), MigrationPlanError> {
    if steps.len() != step_targets.len() {
        return Err(MigrationPlanError::StepTargetsDoNotMatchSteps);
    }

    let mut previous_revision = source.revision();
    for (step_id, step_target) in steps.iter().zip(step_targets) {
        if *step_id != step_target.step_id() {
            return Err(MigrationPlanError::StepTargetsDoNotMatchSteps);
        }
        let expected_revision = previous_revision
            .revision()
            .next_commit()
            .map_err(|_| MigrationPlanError::StepTargetRevisionOverflow)?;
        if step_target.schema().revision()
            != SchemaRevision::from_published_revision(expected_revision)
        {
            return Err(MigrationPlanError::StepTargetRevisionNotContiguous);
        }
        previous_revision = step_target.schema().revision();
    }

    let Some(last) = step_targets.last() else {
        return Err(MigrationPlanError::StepTargetsDoNotMatchSteps);
    };
    if last.schema() != target {
        return Err(MigrationPlanError::StepTargetFinalSchemaMismatch);
    }
    Ok(())
}

fn update_optional_schema_id(hasher: &mut blake3::Hasher, identity: Option<SchemaDefinitionId>) {
    match identity {
        None => {
            hasher.update(&[0]);
        }
        Some(identity) => {
            hasher.update(&[1]);
            identity.update_fingerprint(hasher);
        }
    }
}

/// Invalid migration-plan content or precondition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MigrationPlanError {
    /// A plan must contain at least one step.
    EmptySteps,
    /// A step identity may occur only once in the ordered plan.
    DuplicateStepId(MigrationStepId),
    /// At least one schema identity transition must be classified.
    EmptySchemaChanges,
    /// A schema identity transition may occur only once.
    DuplicateSchemaChange,
    /// Plan category must equal the highest impact among its schema changes.
    CategoryDoesNotMatchChanges,
    /// Target schema revision must follow the exact source precondition revision.
    TargetSchemaNotLater,
    /// Step schema checkpoints must align exactly with the ordered plan steps.
    StepTargetsDoNotMatchSteps,
    /// Every step schema checkpoint must advance one published transaction revision.
    StepTargetRevisionNotContiguous,
    /// A step schema checkpoint would overflow the published revision space.
    StepTargetRevisionOverflow,
    /// The final step checkpoint must equal the plan's declared target schema.
    StepTargetFinalSchemaMismatch,
    /// Transformer versions start at 1.
    ZeroTransformerVersion,
    /// The implementation available at start/resume differs from the planned version.
    TransformerVersionMismatch {
        /// Version fixed into the plan.
        planned: MigrationTransformerVersion,
        /// Version offered by the current execution environment.
        actual: MigrationTransformerVersion,
    },
    /// A run attempted to resume a different logical migration plan.
    RunPlanIdentityMismatch,
    /// Current schema revision or fingerprint differs from the plan precondition.
    SourceSchemaPreconditionMismatch,
    /// Embedded fingerprint does not match the plan's canonical content.
    FingerprintMismatch,
}

impl fmt::Display for MigrationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySteps => formatter.write_str("migration plan has no steps"),
            Self::DuplicateStepId(step_id) => {
                write!(formatter, "migration plan repeats step {step_id}")
            }
            Self::EmptySchemaChanges => {
                formatter.write_str("migration plan has no schema identity changes")
            }
            Self::DuplicateSchemaChange => {
                formatter.write_str("migration plan repeats a schema identity change")
            }
            Self::CategoryDoesNotMatchChanges => {
                formatter.write_str("migration category does not match its schema changes")
            }
            Self::TargetSchemaNotLater => {
                formatter.write_str("migration target schema does not follow its source")
            }
            Self::StepTargetsDoNotMatchSteps => {
                formatter.write_str("migration step schema checkpoints do not match its steps")
            }
            Self::StepTargetRevisionNotContiguous => {
                formatter.write_str("migration step schema revisions are not contiguous")
            }
            Self::StepTargetRevisionOverflow => {
                formatter.write_str("migration step schema revision space is exhausted")
            }
            Self::StepTargetFinalSchemaMismatch => {
                formatter.write_str("final migration step schema differs from plan target")
            }
            Self::ZeroTransformerVersion => {
                formatter.write_str("migration transformer version must be positive")
            }
            Self::TransformerVersionMismatch { planned, actual } => write!(
                formatter,
                "migration transformer version {} does not match planned version {}",
                actual.value(),
                planned.value()
            ),
            Self::RunPlanIdentityMismatch => {
                formatter.write_str("migration run refers to a different plan identity")
            }
            Self::SourceSchemaPreconditionMismatch => {
                formatter.write_str("migration source schema precondition does not match")
            }
            Self::FingerprintMismatch => {
                formatter.write_str("migration plan fingerprint does not match its content")
            }
        }
    }
}

impl std::error::Error for MigrationPlanError {}

/// State of one concrete execution of a migration plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MigrationRunState {
    /// The run exists but has not started applying steps.
    Planned,
    /// The run is applying steps at transaction boundaries.
    Running,
    /// Every required step completed.
    Completed,
    /// The run stopped with a failure requiring explicit resolution.
    Failed,
}

/// A value snapshot of one migration execution, separate from its plan.
///
/// A run identity cannot be substituted for a plan identity:
///
/// ```compile_fail
/// use worlddb_core::{MigrationId, MigrationRun, MigrationRunState};
///
/// fn use_plan_id_as_run_id(id: MigrationId) {
///     MigrationRun::new(id, id, MigrationRunState::Planned);
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationRun {
    run_id: MigrationRunId,
    migration_id: MigrationId,
    state: MigrationRunState,
}

impl MigrationRun {
    /// Associates one concrete run identity with its logical plan and state.
    #[must_use]
    pub const fn new(
        run_id: MigrationRunId,
        migration_id: MigrationId,
        state: MigrationRunState,
    ) -> Self {
        Self {
            run_id,
            migration_id,
            state,
        }
    }

    /// Returns this concrete execution's identity.
    #[must_use]
    pub const fn run_id(self) -> MigrationRunId {
        self.run_id
    }

    /// Returns the logical plan identity being executed.
    #[must_use]
    pub const fn migration_id(self) -> MigrationId {
        self.migration_id
    }

    /// Returns this run's observed state.
    #[must_use]
    pub const fn state(self) -> MigrationRunState {
        self.state
    }
}

/// The four independent identities needed to identify one migration step commit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationStepCommitIdentity {
    migration_id: MigrationId,
    run_id: MigrationRunId,
    step_id: MigrationStepId,
    operation_id: OperationId,
}

impl MigrationStepCommitIdentity {
    /// Identifies a stable plan step within one run and its idempotent commit.
    #[must_use]
    pub const fn new(
        migration_id: MigrationId,
        run_id: MigrationRunId,
        step_id: MigrationStepId,
        operation_id: OperationId,
    ) -> Self {
        Self {
            migration_id,
            run_id,
            step_id,
            operation_id,
        }
    }

    /// Returns the logical migration identity.
    #[must_use]
    pub const fn migration_id(self) -> MigrationId {
        self.migration_id
    }

    /// Returns the concrete run identity.
    #[must_use]
    pub const fn run_id(self) -> MigrationRunId {
        self.run_id
    }

    /// Returns the stable plan-step identity.
    #[must_use]
    pub const fn step_id(self) -> MigrationStepId {
        self.step_id
    }

    /// Returns the idempotent commit identity for this step attempt.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MigrationCategory, MigrationPlan, MigrationPlanError, MigrationPlanSpec, MigrationRun,
        MigrationRunState, MigrationStepCommitIdentity, MigrationStepTargetSchema,
        MigrationTargetSchema, MigrationTransformerVersion, SchemaChangeImpact, SchemaDefinitionId,
        SchemaIdentityTransition, SchemaIdentityTransitionError, SourceSchemaPrecondition,
    };
    use crate::JobBudget;
    use crate::ids::{
        DomainId, IdValidationError, MigrationId, MigrationRunId, MigrationStepId, OperationId,
        PredicateId, Revision, SchemaRevision,
    };

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn additive_spec() -> Result<MigrationPlanSpec, String> {
        let migration_id = uuid::<MigrationId>(1).map_err(|error| error.to_string())?;
        let step = uuid::<MigrationStepId>(2).map_err(|error| error.to_string())?;
        let predicate = uuid::<PredicateId>(3).map_err(|error| error.to_string())?;
        let change = SchemaIdentityTransition::new(
            None,
            Some(SchemaDefinitionId::Predicate(predicate)),
            MigrationCategory::Additive,
        )
        .map_err(|error| error.to_string())?;
        Ok(MigrationPlanSpec {
            migration_id,
            category: MigrationCategory::Additive,
            source_schema: SourceSchemaPrecondition::new(
                SchemaRevision::from_published_revision(Revision::GENESIS),
                [0x11; 32],
            ),
            target_schema: MigrationTargetSchema::new(
                SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
                [0x22; 32],
            ),
            steps: vec![step],
            step_targets: None,
            schema_changes: vec![change],
            transformer_version: MigrationTransformerVersion::new(1)
                .map_err(|error| error.to_string())?,
            calendar_shift: None,
            budget: JobBudget::new(1_000, 1024 * 1024).map_err(|error| error.to_string())?,
        })
    }

    #[test]
    fn migration_plan_binds_source_target_steps_fingerprint_transformer_and_budget()
    -> Result<(), String> {
        let first = MigrationPlan::new(additive_spec()?).map_err(|error| error.to_string())?;
        let second = MigrationPlan::new(additive_spec()?).map_err(|error| error.to_string())?;
        assert_eq!(first, second);
        assert_eq!(first.category(), MigrationCategory::Additive);
        assert_eq!(first.steps().len(), 1);
        assert_eq!(
            first.source_schema_precondition().fingerprint(),
            &[0x11; 32]
        );
        assert_eq!(first.target_schema().fingerprint(), &[0x22; 32]);
        assert_eq!(first.transformer_version().value(), 1);
        assert_eq!(first.budget().max_work_units(), 1_000);
        assert_eq!(first.budget().max_memory_bytes(), 1024 * 1024);
        assert_eq!(first.fingerprint(), second.fingerprint());
        first
            .verify_fingerprint(first.fingerprint())
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn migration_step_targets_bind_every_contiguous_intermediate_schema() -> Result<(), String> {
        let mut spec = additive_spec()?;
        let second_step = uuid::<MigrationStepId>(4).map_err(|error| error.to_string())?;
        spec.steps.push(second_step);
        spec.target_schema = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(
                Revision::new(2).map_err(|error| error.to_string())?,
            ),
            [0x33; 32],
        );
        let first_schema = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            [0x22; 32],
        );
        let first_step = spec
            .steps
            .first()
            .copied()
            .ok_or_else(|| String::from("first step is missing"))?;
        let second_step = spec
            .steps
            .get(1)
            .copied()
            .ok_or_else(|| String::from("second step is missing"))?;
        spec.step_targets = Some(vec![
            MigrationStepTargetSchema::new(first_step, first_schema),
            MigrationStepTargetSchema::new(second_step, spec.target_schema),
        ]);

        let plan = MigrationPlan::new(spec.clone()).map_err(|error| error.to_string())?;
        assert_eq!(plan.step_targets().map(<[_]>::len), Some(2));

        let mut changed_intermediate = spec.clone();
        changed_intermediate
            .step_targets
            .as_mut()
            .and_then(|targets| targets.first_mut())
            .ok_or_else(|| String::from("first step target is missing"))?
            .schema = MigrationTargetSchema::new(first_schema.revision(), [0x23; 32]);
        let changed_plan =
            MigrationPlan::new(changed_intermediate).map_err(|error| error.to_string())?;
        assert_ne!(plan.fingerprint(), changed_plan.fingerprint());

        let mut skipped_revision = spec.clone();
        skipped_revision
            .step_targets
            .as_mut()
            .and_then(|targets| targets.first_mut())
            .ok_or_else(|| String::from("first step target is missing"))?
            .schema = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(
                Revision::new(2).map_err(|error| error.to_string())?,
            ),
            [0x22; 32],
        );
        assert_eq!(
            MigrationPlan::new(skipped_revision).err(),
            Some(MigrationPlanError::StepTargetRevisionNotContiguous)
        );

        let mut wrong_final = spec;
        wrong_final
            .step_targets
            .as_mut()
            .and_then(|targets| targets.last_mut())
            .ok_or_else(|| String::from("last step target is missing"))?
            .schema = MigrationTargetSchema::new(wrong_final.target_schema.revision(), [0x34; 32]);
        assert_eq!(
            MigrationPlan::new(wrong_final).err(),
            Some(MigrationPlanError::StepTargetFinalSchemaMismatch)
        );
        Ok(())
    }

    #[test]
    fn migration_fingerprint_binds_steps_schema_transformer_and_budget() -> Result<(), String> {
        let first_spec = additive_spec()?;
        let first = MigrationPlan::new(first_spec.clone()).map_err(|error| error.to_string())?;

        let mut changed_schema = first_spec.clone();
        changed_schema.target_schema =
            MigrationTargetSchema::new(changed_schema.target_schema.revision(), [0x23; 32]);
        let changed_schema =
            MigrationPlan::new(changed_schema).map_err(|error| error.to_string())?;
        assert_ne!(first.fingerprint(), changed_schema.fingerprint());

        let mut changed_budget = first_spec.clone();
        changed_budget.budget =
            JobBudget::new(1_001, 1024 * 1024).map_err(|error| error.to_string())?;
        let changed_budget =
            MigrationPlan::new(changed_budget).map_err(|error| error.to_string())?;
        assert_ne!(first.fingerprint(), changed_budget.fingerprint());

        let mut changed_step = first_spec;
        let step = changed_step
            .steps
            .first_mut()
            .ok_or_else(|| String::from("additive migration plan lost its step"))?;
        *step = uuid::<MigrationStepId>(4).map_err(|error| error.to_string())?;
        let changed_step = MigrationPlan::new(changed_step).map_err(|error| error.to_string())?;
        assert_ne!(first.fingerprint(), changed_step.fingerprint());
        Ok(())
    }

    #[test]
    fn migration_categories_are_closed_and_match_schema_impact() {
        let categories = [
            MigrationCategory::MetadataOnly,
            MigrationCategory::Additive,
            MigrationCategory::CompatibleConstraintChange,
            MigrationCategory::Restrictive,
            MigrationCategory::Breaking,
        ];
        assert_eq!(categories.len(), 5);
        assert_eq!(
            MigrationCategory::MetadataOnly.impact(),
            SchemaChangeImpact::Compatible
        );
        assert_eq!(
            MigrationCategory::Additive.impact(),
            SchemaChangeImpact::Compatible
        );
        assert_eq!(
            MigrationCategory::CompatibleConstraintChange.impact(),
            SchemaChangeImpact::Compatible
        );
        assert_eq!(
            MigrationCategory::Restrictive.impact(),
            SchemaChangeImpact::Restrictive
        );
        assert_eq!(
            MigrationCategory::Breaking.impact(),
            SchemaChangeImpact::Breaking
        );
    }

    #[test]
    fn breaking_schema_changes_require_a_new_id_of_the_same_family() -> Result<(), IdValidationError>
    {
        let old = uuid::<PredicateId>(10)?;
        let new = uuid::<PredicateId>(11)?;
        assert_eq!(
            SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(old)),
                Some(SchemaDefinitionId::Predicate(old)),
                MigrationCategory::Breaking,
            ),
            Err(SchemaIdentityTransitionError::BreakingRequiresNewSchemaId)
        );
        assert!(
            SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(old)),
                Some(SchemaDefinitionId::Predicate(new)),
                MigrationCategory::Breaking,
            )
            .is_ok()
        );
        let event_kind = uuid::<crate::EventKindId>(12)?;
        assert_eq!(
            SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(old)),
                Some(SchemaDefinitionId::EventKind(event_kind)),
                MigrationCategory::Breaking,
            ),
            Err(SchemaIdentityTransitionError::IdentityFamilyMismatch)
        );
        Ok(())
    }

    #[test]
    fn compatible_and_metadata_changes_do_not_require_new_schema_identity()
    -> Result<(), IdValidationError> {
        let predicate = uuid::<PredicateId>(20)?;
        for category in [
            MigrationCategory::MetadataOnly,
            MigrationCategory::Additive,
            MigrationCategory::CompatibleConstraintChange,
        ] {
            let transition = SchemaIdentityTransition::new(
                Some(SchemaDefinitionId::Predicate(predicate)),
                Some(SchemaDefinitionId::Predicate(predicate)),
                category,
            );
            assert!(transition.is_ok());
        }
        Ok(())
    }

    #[test]
    fn restrictive_change_preserves_the_schema_identity() -> Result<(), IdValidationError> {
        let predicate = uuid::<PredicateId>(21)?;
        let identity = SchemaDefinitionId::Predicate(predicate);
        assert!(
            SchemaIdentityTransition::new(
                Some(identity),
                Some(identity),
                MigrationCategory::Restrictive,
            )
            .is_ok()
        );
        assert_eq!(
            SchemaIdentityTransition::new(Some(identity), None, MigrationCategory::Restrictive,),
            Err(SchemaIdentityTransitionError::IdentityShapeDoesNotMatchCategory)
        );
        Ok(())
    }

    #[test]
    fn source_schema_precondition_checks_revision_and_fingerprint() -> Result<(), String> {
        let plan = MigrationPlan::new(additive_spec()?).map_err(|error| error.to_string())?;
        let source_revision = plan.source_schema_precondition().revision();
        plan.validate_source_schema(source_revision, [0x11; 32])
            .map_err(|error| error.to_string())?;
        assert_eq!(
            plan.validate_source_schema(source_revision, [0x12; 32]),
            Err(MigrationPlanError::SourceSchemaPreconditionMismatch)
        );
        assert_eq!(
            plan.validate_source_schema(
                SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
                [0x11; 32]
            ),
            Err(MigrationPlanError::SourceSchemaPreconditionMismatch)
        );
        Ok(())
    }

    #[test]
    fn migration_plans_reject_empty_duplicate_and_misclassified_changes() -> Result<(), String> {
        let mut spec = additive_spec()?;
        spec.steps.clear();
        assert_eq!(
            MigrationPlan::new(spec),
            Err(MigrationPlanError::EmptySteps)
        );

        let mut spec = additive_spec()?;
        let first_step = spec
            .steps
            .first()
            .copied()
            .ok_or_else(|| String::from("additive migration plan lost its step"))?;
        spec.steps.push(first_step);
        assert!(matches!(
            MigrationPlan::new(spec),
            Err(MigrationPlanError::DuplicateStepId(_))
        ));

        let predicate = uuid::<PredicateId>(34).map_err(|error| error.to_string())?;
        let identity = SchemaDefinitionId::Predicate(predicate);
        let metadata_change = SchemaIdentityTransition::new(
            Some(identity),
            Some(identity),
            MigrationCategory::MetadataOnly,
        )
        .map_err(|error| error.to_string())?;
        let additive_change = SchemaIdentityTransition::new(
            Some(identity),
            Some(identity),
            MigrationCategory::Additive,
        )
        .map_err(|error| error.to_string())?;
        let mut spec = additive_spec()?;
        spec.schema_changes = vec![metadata_change, additive_change];
        assert_eq!(
            MigrationPlan::new(spec),
            Err(MigrationPlanError::DuplicateSchemaChange)
        );

        let mut spec = additive_spec()?;
        spec.category = MigrationCategory::Breaking;
        assert_eq!(
            MigrationPlan::new(spec),
            Err(MigrationPlanError::CategoryDoesNotMatchChanges)
        );

        let mut spec = additive_spec()?;
        spec.target_schema = MigrationTargetSchema::new(spec.source_schema.revision(), [0x22; 32]);
        assert_eq!(
            MigrationPlan::new(spec),
            Err(MigrationPlanError::TargetSchemaNotLater)
        );
        Ok(())
    }

    #[test]
    fn migration_plan_rejects_zero_transformer_version() {
        assert_eq!(
            MigrationTransformerVersion::new(0),
            Err(MigrationPlanError::ZeroTransformerVersion)
        );
    }

    #[test]
    fn migration_plan_and_run_retain_separate_identities() -> Result<(), String> {
        let spec = additive_spec()?;
        let migration_id = spec.migration_id;
        let step_id = spec
            .steps
            .first()
            .copied()
            .ok_or_else(|| String::from("additive migration plan lost its step"))?;
        let plan = MigrationPlan::new(spec).map_err(|error| error.to_string())?;
        let run_id = uuid::<MigrationRunId>(32).map_err(|error| error.to_string())?;
        let operation_id = uuid::<OperationId>(33).map_err(|error| error.to_string())?;
        assert_eq!(plan.migration_id(), migration_id);
        let run = MigrationRun::new(run_id, migration_id, MigrationRunState::Running);
        assert_eq!(run.run_id(), run_id);
        assert_eq!(run.migration_id(), migration_id);
        assert_eq!(run.state(), MigrationRunState::Running);
        let identity =
            MigrationStepCommitIdentity::new(migration_id, run_id, step_id, operation_id);
        assert_eq!(identity.migration_id(), migration_id);
        assert_eq!(identity.run_id(), run_id);
        assert_eq!(identity.step_id(), step_id);
        assert_eq!(identity.operation_id(), operation_id);
        Ok(())
    }

    #[test]
    fn schema_definition_wire_identity_keeps_its_typed_family() -> Result<(), IdValidationError> {
        let id = uuid::<PredicateId>(40)?;
        let identity = SchemaDefinitionId::Predicate(id);
        assert_eq!(
            SchemaDefinitionId::from_wire_bytes(&identity.to_wire_bytes()),
            Some(identity)
        );
        let mut invalid = identity.to_wire_bytes();
        invalid[0] = 5;
        assert_eq!(SchemaDefinitionId::from_wire_bytes(&invalid), None);
        Ok(())
    }
}
