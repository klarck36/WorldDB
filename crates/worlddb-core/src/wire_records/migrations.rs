//! Migration record wire payloads.

use crate::wire::DecoderLimits;
use crate::wire::{decode_id, encode_id};
use crate::{
    MigrationCategory, MigrationPlan, MigrationRun, MigrationRunState, MigrationStepCommitIdentity,
    RecordCodecError, RecordKind,
};

use super::{
    collect_results_limited, decode_array_with_limits, decode_fields_with_limits, encode_array,
    encode_fields, invalid_field, required_field,
};

pub(super) fn encode_plan(value: &MigrationPlan) -> Result<Vec<u8>, RecordCodecError> {
    let steps = value
        .steps()
        .iter()
        .map(|step| encode_id(*step).to_vec())
        .collect::<Vec<_>>();
    encode_fields(
        RecordKind::MigrationPlan,
        vec![
            (1, encode_id(value.migration_id()).to_vec()),
            (
                2,
                vec![match value.category() {
                    MigrationCategory::MetadataOnly => 1,
                    MigrationCategory::Additive => 2,
                    MigrationCategory::CompatibleConstraintChange => 3,
                    MigrationCategory::Restrictive => 4,
                    MigrationCategory::Breaking => 5,
                }],
            ),
            (3, encode_array(&steps)),
        ],
    )
}

pub(super) fn decode_plan(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationPlan, RecordCodecError> {
    let kind = RecordKind::MigrationPlan;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3], limits)?;
    let migration_id =
        decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let category = match required_field(kind, &fields, 2)? {
        [1] => MigrationCategory::MetadataOnly,
        [2] => MigrationCategory::Additive,
        [3] => MigrationCategory::CompatibleConstraintChange,
        [4] => MigrationCategory::Restrictive,
        [5] => MigrationCategory::Breaking,
        _ => return Err(invalid_field(kind, 2)),
    };
    let step_bytes = decode_array_with_limits(required_field(kind, &fields, 3)?, limits)
        .map_err(RecordCodecError::Wire)?;
    let steps = collect_results_limited(
        step_bytes
            .into_iter()
            .map(|bytes| decode_id(bytes).map_err(RecordCodecError::Wire)),
        limits,
    )?;
    MigrationPlan::new(migration_id, category, steps).map_err(|_| invalid_field(kind, 3))
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
    encode_fields(
        RecordKind::MigrationStepCommitIdentity,
        vec![
            (1, encode_id(value.migration_id()).to_vec()),
            (2, encode_id(value.run_id()).to_vec()),
            (3, encode_id(value.step_id()).to_vec()),
            (4, encode_id(value.operation_id()).to_vec()),
        ],
    )
}

pub(super) fn decode_step_commit(
    bytes: &[u8],
    limits: &DecoderLimits,
) -> Result<MigrationStepCommitIdentity, RecordCodecError> {
    let kind = RecordKind::MigrationStepCommitIdentity;
    let fields = decode_fields_with_limits(kind, bytes, &[1, 2, 3, 4], limits)?;
    let migration_id =
        decode_id(required_field(kind, &fields, 1)?).map_err(RecordCodecError::Wire)?;
    let run_id = decode_id(required_field(kind, &fields, 2)?).map_err(RecordCodecError::Wire)?;
    let step_id = decode_id(required_field(kind, &fields, 3)?).map_err(RecordCodecError::Wire)?;
    let operation_id =
        decode_id(required_field(kind, &fields, 4)?).map_err(RecordCodecError::Wire)?;
    Ok(MigrationStepCommitIdentity::new(
        migration_id,
        run_id,
        step_id,
        operation_id,
    ))
}
