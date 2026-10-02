//! Canonical migration-plan, run, and step-commit wire payloads.

use crate::wire::DecoderLimits;
use crate::wire::{decode_id, encode_id};
use crate::{
    CalendarPeriod, JobBudget, MigrationCalendarDirection, MigrationCalendarShift,
    MigrationCategory, MigrationPlan, MigrationPlanSpec, MigrationRun, MigrationRunState,
    MigrationStepCommitIdentity, MigrationStepTargetSchema, MigrationTargetSchema,
    MigrationTransformerVersion, RecordCodecError, RecordKind, SchemaIdentityTransition,
    SourceSchemaPrecondition, TimelineId,
};

use super::{
    collect_results_limited, decode_array_with_limits, decode_fields_with_limits,
    decode_schema_revision, encode_array, encode_fields, encode_schema_revision, invalid_field,
    required_field,
};

const PLAN_FIELDS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];

pub(super) fn encode_plan(value: &MigrationPlan) -> Result<Vec<u8>, RecordCodecError> {
    let steps = value
        .steps()
        .iter()
        .map(|step| encode_id(*step).to_vec())
        .collect::<Vec<_>>();
    let schema_changes = value
        .schema_changes()
        .iter()
        .map(encode_schema_identity_transition)
        .collect::<Result<Vec<_>, _>>()?;
    let source = value.source_schema_precondition();
    let target = value.target_schema();
    let mut fields = vec![
        (1, encode_id(value.migration_id()).to_vec()),
        (2, vec![category_tag(value.category())]),
        (3, encode_array(&steps)),
        (4, encode_schema_revision(source.revision())),
        (5, source.fingerprint().to_vec()),
        (6, encode_schema_revision(target.revision())),
        (7, target.fingerprint().to_vec()),
        (
            8,
            encode_u64(u64::from(value.transformer_version().value())),
        ),
        (9, encode_u64(value.budget().max_work_units())),
        (10, encode_u64(value.budget().max_memory_bytes())),
        (11, value.fingerprint().as_bytes().to_vec()),
        (12, encode_array(&schema_changes)),
    ];
    if let Some(calendar_shift) = value.calendar_shift() {
        fields.push((13, encode_calendar_shift(calendar_shift)?));
    }
    if let Some(step_targets) = value.step_targets() {
        let encoded_targets = step_targets
            .iter()
            .map(encode_step_target)
            .collect::<Result<Vec<_>, _>>()?;
        fields.push((14, encode_array(&encoded_targets)));
    }
    encode_fields(RecordKind::MigrationPlan, fields)
}

pub(super) fn decode_plan(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationPlan, RecordCodecError> {
    let kind = RecordKind::MigrationPlan;
    let fields = decode_fields_with_limits(kind, bytes, PLAN_FIELDS, limits)?;
    let migration_id =
        decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let category = decode_category(kind, 2, required_field(kind, &fields, 2)?)?;
    let step_bytes = decode_array_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let steps = collect_results_limited(
        step_bytes
            .into_iter()
            .map(|bytes| decode_id(bytes).map_err(RecordCodecError::Wire)),
        limits,
    )?;
    let source_revision = decode_schema_revision(kind, 4, required_field(kind, &fields, 4)?)?;
    let source_fingerprint = decode_fingerprint(kind, 5, required_field(kind, &fields, 5)?)?;
    let target_revision = decode_schema_revision(kind, 6, required_field(kind, &fields, 6)?)?;
    let target_fingerprint = decode_fingerprint(kind, 7, required_field(kind, &fields, 7)?)?;
    let transformer_version =
        u32::try_from(decode_u64(kind, 8, required_field(kind, &fields, 8)?)?)
            .map_err(|_| invalid_field(kind, 8))?;
    let transformer_version = MigrationTransformerVersion::new(transformer_version)
        .map_err(|_| invalid_field(kind, 8))?;
    let max_work_units = decode_u64(kind, 9, required_field(kind, &fields, 9)?)?;
    let max_memory_bytes = decode_u64(kind, 10, required_field(kind, &fields, 10)?)?;
    let budget = JobBudget::new(max_work_units, max_memory_bytes).map_err(|error| match error {
        crate::JobBudgetError::ZeroWorkUnits => invalid_field(kind, 9),
        crate::JobBudgetError::ZeroMemoryBytes => invalid_field(kind, 10),
    })?;
    let expected_fingerprint = decode_fingerprint(kind, 11, required_field(kind, &fields, 11)?)?;
    let schema_change_bytes = decode_array_with_limits(required_field(kind, &fields, 12)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let schema_changes = collect_results_limited(
        schema_change_bytes
            .into_iter()
            .map(|bytes| decode_schema_identity_transition(bytes, limits)),
        limits,
    )?;
    let calendar_shift = fields
        .iter()
        .find(|(tag, _)| *tag == 13)
        .map(|(_, bytes)| decode_calendar_shift(kind, bytes, limits))
        .transpose()?;
    let step_targets = fields
        .iter()
        .find(|(tag, _)| *tag == 14)
        .map(|(_, bytes)| {
            let encoded_targets =
                decode_array_with_limits(bytes, limits).map_err(RecordCodecError::Wire)?;
            collect_results_limited(
                encoded_targets
                    .into_iter()
                    .map(|bytes| decode_step_target(bytes, limits)),
                limits,
            )
        })
        .transpose()?;

    let plan = MigrationPlan::new(MigrationPlanSpec {
        migration_id,
        category,
        source_schema: SourceSchemaPrecondition::new(source_revision, source_fingerprint),
        target_schema: MigrationTargetSchema::new(target_revision, target_fingerprint),
        steps,
        step_targets,
        schema_changes,
        transformer_version,
        calendar_shift,
        budget,
    })
    .map_err(|error| match error {
        crate::MigrationPlanError::EmptySteps | crate::MigrationPlanError::DuplicateStepId(_) => {
            invalid_field(kind, 3)
        }
        crate::MigrationPlanError::TargetSchemaNotLater => invalid_field(kind, 6),
        crate::MigrationPlanError::CategoryDoesNotMatchChanges => invalid_field(kind, 2),
        crate::MigrationPlanError::EmptySchemaChanges
        | crate::MigrationPlanError::DuplicateSchemaChange => invalid_field(kind, 12),
        crate::MigrationPlanError::StepTargetsDoNotMatchSteps
        | crate::MigrationPlanError::StepTargetRevisionNotContiguous
        | crate::MigrationPlanError::StepTargetRevisionOverflow
        | crate::MigrationPlanError::StepTargetFinalSchemaMismatch => invalid_field(kind, 14),
        crate::MigrationPlanError::ZeroTransformerVersion => invalid_field(kind, 8),
        crate::MigrationPlanError::TransformerVersionMismatch { .. } => invalid_field(kind, 8),
        crate::MigrationPlanError::RunPlanIdentityMismatch => invalid_field(kind, 1),
        crate::MigrationPlanError::SourceSchemaPreconditionMismatch
        | crate::MigrationPlanError::FingerprintMismatch => invalid_field(kind, 11),
    })?;
    if plan.fingerprint().as_bytes() != &expected_fingerprint {
        return Err(invalid_field(kind, 11));
    }
    Ok(plan)
}

fn encode_step_target(target: &MigrationStepTargetSchema) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::MigrationPlan,
        vec![
            (1, encode_id(target.step_id()).to_vec()),
            (2, encode_schema_revision(target.schema().revision())),
            (3, target.schema().fingerprint().to_vec()),
        ],
    )
}

fn decode_step_target(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationStepTargetSchema, RecordCodecError> {
    let kind = RecordKind::MigrationPlan;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)
        .map_err(|_| invalid_field(kind, 14))?;
    let step_id = decode_id(required_field(kind, &fields, 1).map_err(|_| invalid_field(kind, 14))?)
        .map_err(|_| invalid_field(kind, 14))?;
    let revision = decode_schema_revision(
        kind,
        2,
        required_field(kind, &fields, 2).map_err(|_| invalid_field(kind, 14))?,
    )
    .map_err(|_| invalid_field(kind, 14))?;
    let fingerprint = decode_fingerprint(
        kind,
        3,
        required_field(kind, &fields, 3).map_err(|_| invalid_field(kind, 14))?,
    )
    .map_err(|_| invalid_field(kind, 14))?;
    Ok(MigrationStepTargetSchema::new(
        step_id,
        MigrationTargetSchema::new(revision, fingerprint),
    ))
}

fn encode_calendar_shift(shift: MigrationCalendarShift) -> Result<Vec<u8>, RecordCodecError> {
    let profile_tag = 1_u8; // ProlepticGregorianUtc, the only supported profile.
    let nested = encode_fields(
        RecordKind::MigrationPlan,
        vec![
            (1, encode_id(shift.timeline_id()).to_vec()),
            (2, vec![profile_tag]),
            (3, shift.epoch_unix_nanoseconds().to_be_bytes().to_vec()),
            (4, encode_u64(u64::from(shift.period().years()))),
            (5, vec![shift.period().months()]),
            (6, encode_u64(u64::from(shift.period().days()))),
            (7, vec![shift.direction().wire_tag()]),
        ],
    )?;
    Ok(nested)
}

fn decode_calendar_shift(
    kind: RecordKind,
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationCalendarShift, RecordCodecError> {
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5, 6, 7], limits)
        .map_err(|_| invalid_field(kind, 13))?;
    let timeline_id = decode_id::<TimelineId>(required_shift_field(kind, &fields, 1)?)
        .map_err(|_| invalid_field(kind, 13))?;
    match required_shift_field(kind, &fields, 2)? {
        [1] => {}
        _ => return Err(invalid_field(kind, 13)),
    }
    let epoch_unix_nanoseconds = required_shift_field(kind, &fields, 3)?
        .try_into()
        .map(i128::from_be_bytes)
        .map_err(|_| invalid_field(kind, 13))?;
    let years = u32::try_from(decode_u64(
        kind,
        13,
        required_shift_field(kind, &fields, 4)?,
    )?)
    .map_err(|_| invalid_field(kind, 13))?;
    let months = match required_shift_field(kind, &fields, 5)? {
        [months] => *months,
        _ => return Err(invalid_field(kind, 13)),
    };
    let days = u32::try_from(decode_u64(
        kind,
        13,
        required_shift_field(kind, &fields, 6)?,
    )?)
    .map_err(|_| invalid_field(kind, 13))?;
    let period = CalendarPeriod::new(years, months, days).map_err(|_| invalid_field(kind, 13))?;
    let direction = match required_shift_field(kind, &fields, 7)? {
        [1] => MigrationCalendarDirection::Past,
        [2] => MigrationCalendarDirection::Future,
        _ => return Err(invalid_field(kind, 13)),
    };
    Ok(MigrationCalendarShift::new(
        timeline_id,
        epoch_unix_nanoseconds,
        period,
        direction,
    ))
}

fn required_shift_field<'a>(
    kind: RecordKind,
    fields: &[(u32, &'a [u8])],
    tag: u32,
) -> Result<&'a [u8], RecordCodecError> {
    required_field(kind, fields, tag).map_err(|_| invalid_field(kind, 13))
}

pub(super) fn encode_run(value: &MigrationRun) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::MigrationRun,
        vec![
            (1, encode_id(value.run_id()).to_vec()),
            (2, encode_id(value.migration_id()).to_vec()),
            (
                3,
                vec![match value.state() {
                    MigrationRunState::Planned => 1,
                    MigrationRunState::Running => 2,
                    MigrationRunState::Completed => 3,
                    MigrationRunState::Failed => 4,
                }],
            ),
        ],
    )
}

pub(super) fn decode_run(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationRun, RecordCodecError> {
    let kind = RecordKind::MigrationRun;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let run_id = decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let migration_id =
        decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let state = match required_field(kind, &fields, 3)? {
        [1] => MigrationRunState::Planned,
        [2] => MigrationRunState::Running,
        [3] => MigrationRunState::Completed,
        [4] => MigrationRunState::Failed,
        _ => return Err(invalid_field(kind, 3)),
    };
    Ok(MigrationRun::new(run_id, migration_id, state))
}

pub(super) fn encode_step_commit(
    value: &MigrationStepCommitIdentity,
) -> Result<Vec<u8>, RecordCodecError> {
    let mut fields = vec![
        (1, encode_id(value.migration_id()).to_vec()),
        (2, encode_id(value.run_id()).to_vec()),
        (3, encode_id(value.step_id()).to_vec()),
        (4, encode_id(value.operation_id()).to_vec()),
    ];
    if let Some(input_fingerprint) = value.input_fingerprint() {
        fields.push((5, input_fingerprint.to_vec()));
    }
    encode_fields(RecordKind::MigrationStepCommitIdentity, fields)
}

pub(super) fn decode_step_commit(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationStepCommitIdentity, RecordCodecError> {
    let kind = RecordKind::MigrationStepCommitIdentity;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4, 5], limits)?;
    let migration_id =
        decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let run_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let step_id = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let operation_id =
        decode_id(required_field(kind, &fields, 4)?).map_err(RecordCodecError::Wire)?;
    let input_fingerprint = fields
        .iter()
        .find(|(tag, _)| *tag == 5)
        .map(|(_, value)| decode_fingerprint(kind, 5, value))
        .transpose()?;
    Ok(match input_fingerprint {
        Some(fingerprint) => MigrationStepCommitIdentity::with_input_fingerprint(
            migration_id,
            run_id,
            step_id,
            operation_id,
            fingerprint,
        ),
        None => MigrationStepCommitIdentity::new(migration_id, run_id, step_id, operation_id),
    })
}

fn encode_schema_identity_transition(
    value: &SchemaIdentityTransition,
) -> Result<Vec<u8>, RecordCodecError> {
    encode_fields(
        RecordKind::MigrationPlan,
        vec![
            (1, encode_optional_identity(value.source())),
            (2, encode_optional_identity(value.target())),
            (3, vec![category_tag(value.category())]),
        ],
    )
}

fn decode_schema_identity_transition(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<SchemaIdentityTransition, RecordCodecError> {
    let kind = RecordKind::MigrationPlan;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let source = decode_optional_identity(required_field(kind, &fields, 1)?)
        .map_err(|_| invalid_field(kind, 12))?;
    let target = decode_optional_identity(required_field(kind, &fields, 2)?)
        .map_err(|_| invalid_field(kind, 12))?;
    let category = decode_category(kind, 3, required_field(kind, &fields, 3)?)?;
    SchemaIdentityTransition::new(source, target, category).map_err(|_| invalid_field(kind, 12))
}

fn encode_optional_identity(identity: Option<crate::SchemaDefinitionId>) -> Vec<u8> {
    match identity {
        None => vec![0],
        Some(identity) => {
            let mut bytes = Vec::with_capacity(18);
            bytes.push(1);
            bytes.extend_from_slice(&identity.to_wire_bytes());
            bytes
        }
    }
}

fn decode_optional_identity(bytes: &[u8]) -> Result<Option<crate::SchemaDefinitionId>, ()> {
    match bytes {
        [0] => Ok(None),
        [1, identity @ ..] => crate::SchemaDefinitionId::from_wire_bytes(identity)
            .map(Some)
            .ok_or(()),
        _ => Err(()),
    }
}

fn category_tag(category: MigrationCategory) -> u8 {
    match category {
        MigrationCategory::MetadataOnly => 1,
        MigrationCategory::Additive => 2,
        MigrationCategory::CompatibleConstraintChange => 3,
        MigrationCategory::Restrictive => 4,
        MigrationCategory::Breaking => 5,
    }
}

fn decode_category(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<MigrationCategory, RecordCodecError> {
    match bytes {
        [1] => Ok(MigrationCategory::MetadataOnly),
        [2] => Ok(MigrationCategory::Additive),
        [3] => Ok(MigrationCategory::CompatibleConstraintChange),
        [4] => Ok(MigrationCategory::Restrictive),
        [5] => Ok(MigrationCategory::Breaking),
        _ => Err(invalid_field(kind, field)),
    }
}

fn encode_u64(value: u64) -> Vec<u8> {
    crate::numbers::encode_u128_varint(u128::from(value))
}

fn decode_u64(kind: RecordKind, field: u32, bytes: &[u8]) -> Result<u64, RecordCodecError> {
    crate::UInt::from_canonical_bytes(bytes)
        .map_err(|_| invalid_field(kind, field))
        .and_then(|value| u64::try_from(value.value()).map_err(|_| invalid_field(kind, field)))
}

fn decode_fingerprint(
    kind: RecordKind,
    field: u32,
    bytes: &[u8],
) -> Result<[u8; 32], RecordCodecError> {
    bytes.try_into().map_err(|_| invalid_field(kind, field))
}
