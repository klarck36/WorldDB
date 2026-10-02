//! Closed, versioned migration transforms with no ambient runtime capabilities.

use crate::ids::{SchemaRevision, TimelineId};
use crate::{
    CalendarPeriod, DecoderLimits, MigrationCalendarDirection, MigrationPlan, MigrationPlanError,
    MigrationTransformerVersion, RecordCodecError, Timeline, WorldTime, decode_record_with_limits,
};
use std::fmt;

const NANOS_PER_DAY: i128 = 86_400_000_000_000;
const UNIX_EPOCH_DAY_OFFSET: i128 = 719_468;
const RECORD_VECTOR_SLOT_RESERVATION_BYTES: u64 = 32;

/// The implemented deterministic migration interpreter.
///
/// Version 1 performs an exact canonical-record pass-through and an explicit
/// plan-bound Gregorian calendar shift for supplied `WorldTime` coordinates.
/// Its closed API accepts data and typed plan parameters only; callers cannot
/// inject callbacks or host services into the commit transform.
/// Calendar components apply in order: years, then months, then civil days;
/// a year or month move clamps the day to the destination month's final day.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationTransformer {
    version: MigrationTransformerVersion,
}

impl MigrationTransformer {
    /// Loads a supported, immutable transformer implementation by version.
    pub fn for_version(
        version: MigrationTransformerVersion,
    ) -> Result<Self, MigrationTransformerError> {
        if version.value() != 1 {
            return Err(MigrationTransformerError::UnsupportedVersion(version));
        }
        Ok(Self { version })
    }

    /// Version implemented by this transformer.
    #[must_use]
    pub const fn version(self) -> MigrationTransformerVersion {
        self.version
    }

    /// Canonically validates and copies a batch of encoded records.
    ///
    /// Output preserves source order, since record order can carry history
    /// semantics. Work is counted per record and memory admission counts the
    /// input and canonical output frame bytes plus a fixed 32-byte slot for
    /// each output vector. The record parser uses the format's fixed limits,
    /// not process configuration.
    /// No partial output is returned after a validation or budget error.
    pub fn transform_records(
        &self,
        plan: &MigrationPlan,
        source_revision: SchemaRevision,
        source_fingerprint: [u8; 32],
        records: &[Vec<u8>],
    ) -> Result<MigrationTransformBatch, MigrationTransformerError> {
        plan.validate_start(source_revision, source_fingerprint, self.version)
            .map_err(MigrationTransformerError::Plan)?;

        let work_units =
            u64::try_from(records.len()).map_err(|_| MigrationTransformerError::SizeOverflow)?;
        if work_units > plan.budget().max_work_units() {
            return Err(MigrationTransformerError::WorkBudgetExceeded {
                requested: work_units,
                limit: plan.budget().max_work_units(),
            });
        }

        let input_bytes = records.iter().try_fold(0_u64, |total, record| {
            let record_len =
                u64::try_from(record.len()).map_err(|_| MigrationTransformerError::SizeOverflow)?;
            total
                .checked_add(record_len)
                .ok_or(MigrationTransformerError::SizeOverflow)
        })?;
        let vector_overhead = work_units
            .checked_mul(RECORD_VECTOR_SLOT_RESERVATION_BYTES)
            .ok_or(MigrationTransformerError::SizeOverflow)?;
        let memory_required = input_bytes
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(vector_overhead))
            .ok_or(MigrationTransformerError::SizeOverflow)?;
        if memory_required > plan.budget().max_memory_bytes() {
            return Err(MigrationTransformerError::MemoryBudgetExceeded {
                requested: memory_required,
                limit: plan.budget().max_memory_bytes(),
            });
        }

        let mut output = Vec::new();
        output
            .try_reserve_exact(records.len())
            .map_err(|_| MigrationTransformerError::AllocationFailed)?;
        for record in records {
            // The bounded decoder verifies the exact canonical re-encoding.
            decode_record_with_limits(record, &DecoderLimits::DEFAULT)
                .map_err(MigrationTransformerError::RecordCodec)?;
            output.push(record.clone());
        }
        let fingerprint = fingerprint_records(plan, self.version, &output)?;
        Ok(MigrationTransformBatch {
            records: output,
            fingerprint,
        })
    }

    /// Applies the plan's exact calendar shift to one value on its declared timeline.
    pub fn transform_world_time(
        &self,
        plan: &MigrationPlan,
        source_revision: SchemaRevision,
        source_fingerprint: [u8; 32],
        time: WorldTime,
    ) -> Result<WorldTime, MigrationTransformerError> {
        plan.validate_start(source_revision, source_fingerprint, self.version)
            .map_err(MigrationTransformerError::Plan)?;
        let shift = plan
            .calendar_shift()
            .ok_or(MigrationTransformerError::CalendarShiftNotPlanned)?;
        if time.timeline().id() != shift.timeline_id() {
            return Err(MigrationTransformerError::TimelineMismatch {
                planned: shift.timeline_id(),
                actual: time.timeline().id(),
            });
        }
        let utc_nanoseconds = shift
            .epoch_unix_nanoseconds()
            .checked_add(time.nanoseconds())
            .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
        let shifted_utc = shift_calendar_utc(utc_nanoseconds, shift.period(), shift.direction())?;
        let shifted_coordinate = shifted_utc
            .checked_sub(shift.epoch_unix_nanoseconds())
            .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
        Ok(WorldTime::from_nanoseconds(
            Timeline::new(shift.timeline_id()),
            shifted_coordinate,
        ))
    }
}

/// Canonical, all-or-nothing result of one deterministic record transform batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationTransformBatch {
    records: Vec<Vec<u8>>,
    fingerprint: MigrationTransformFingerprint,
}

impl MigrationTransformBatch {
    /// Canonically ordered, validated output record frames.
    #[must_use]
    pub fn records(&self) -> &[Vec<u8>] {
        &self.records
    }

    /// BLAKE3 fingerprint over the exact ordered result bytes.
    #[must_use]
    pub const fn fingerprint(&self) -> MigrationTransformFingerprint {
        self.fingerprint
    }
}

/// Stable BLAKE3 digest of one canonical logical transform result.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MigrationTransformFingerprint([u8; 32]);

impl MigrationTransformFingerprint {
    /// Returns the exact fingerprint bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

fn fingerprint_records(
    plan: &MigrationPlan,
    version: MigrationTransformerVersion,
    records: &[Vec<u8>],
) -> Result<MigrationTransformFingerprint, MigrationTransformerError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.MigrationTransformResult.v2\0");
    hasher.update(plan.fingerprint().as_bytes());
    hasher.update(&version.value().to_be_bytes());
    hasher.update(
        &u64::try_from(records.len())
            .map_err(|_| MigrationTransformerError::SizeOverflow)?
            .to_be_bytes(),
    );
    for record in records {
        hasher.update(
            &u64::try_from(record.len())
                .map_err(|_| MigrationTransformerError::SizeOverflow)?
                .to_be_bytes(),
        );
        hasher.update(record);
    }
    Ok(MigrationTransformFingerprint(*hasher.finalize().as_bytes()))
}

fn shift_calendar_utc(
    utc_nanoseconds: i128,
    period: CalendarPeriod,
    direction: MigrationCalendarDirection,
) -> Result<i128, MigrationTransformerError> {
    let (year_delta, month_delta, day_delta) = match direction {
        MigrationCalendarDirection::Past => (
            -i128::from(period.years()),
            -i128::from(period.months()),
            -i128::from(period.days()),
        ),
        MigrationCalendarDirection::Future => (
            i128::from(period.years()),
            i128::from(period.months()),
            i128::from(period.days()),
        ),
    };

    let day_number = utc_nanoseconds.div_euclid(NANOS_PER_DAY);
    let time_of_day = utc_nanoseconds.rem_euclid(NANOS_PER_DAY);
    let (mut year, mut month, mut day) = civil_from_days(day_number)?;

    year = year
        .checked_add(year_delta)
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
    day = day.min(days_in_month(year, month));
    (year, month) = add_months(year, month, month_delta)?;
    day = day.min(days_in_month(year, month));

    let days = days_from_civil(year, month, day)?
        .checked_add(day_delta)
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
    days.checked_mul(NANOS_PER_DAY)
        .and_then(|start| start.checked_add(time_of_day))
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)
}

fn civil_from_days(days: i128) -> Result<(i128, u8, u8), MigrationTransformerError> {
    let shifted = days
        .checked_add(UNIX_EPOCH_DAY_OFFSET)
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era
        .checked_add(
            era.checked_mul(400)
                .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?,
        )
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 {
        year.checked_add(1)
            .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?
    } else {
        year
    };
    let month =
        u8::try_from(month).map_err(|_| MigrationTransformerError::CalendarArithmeticOverflow)?;
    let day =
        u8::try_from(day).map_err(|_| MigrationTransformerError::CalendarArithmeticOverflow)?;
    Ok((year, month, day))
}

fn days_from_civil(year: i128, month: u8, day: u8) -> Result<i128, MigrationTransformerError> {
    let adjusted_year = if month <= 2 {
        year.checked_sub(1)
            .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?
    } else {
        year
    };
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year.rem_euclid(400);
    let month_prime = i128::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + i128::from(day) - 1;
    let day_of_era = 365 * year_of_era + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)
        .and_then(|days| days.checked_add(day_of_era))
        .and_then(|days| days.checked_sub(UNIX_EPOCH_DAY_OFFSET))
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)
}

fn add_months(
    year: i128,
    month: u8,
    months: i128,
) -> Result<(i128, u8), MigrationTransformerError> {
    let total_month = year
        .checked_mul(12)
        .and_then(|value| value.checked_add(i128::from(month) - 1))
        .and_then(|value| value.checked_add(months))
        .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
    let next_year = total_month.div_euclid(12);
    let next_month = u8::try_from(total_month.rem_euclid(12) + 1)
        .map_err(|_| MigrationTransformerError::CalendarArithmeticOverflow)?;
    Ok((next_year, next_month))
}

fn days_in_month(year: i128, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) =>
        {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Deterministic transform failure, including explicit budget and timeline rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationTransformerError {
    /// The requested implementation version is not compiled into this closed registry.
    UnsupportedVersion(MigrationTransformerVersion),
    /// Plan precondition or version validation failed.
    Plan(MigrationPlanError),
    /// An input is not a valid canonical WorldDB record frame.
    RecordCodec(RecordCodecError),
    /// The batch's record count exceeds its planned work units.
    WorkBudgetExceeded { requested: u64, limit: u64 },
    /// The batch's estimated copy/decode memory exceeds its planned memory bytes.
    MemoryBudgetExceeded { requested: u64, limit: u64 },
    /// A reserved output buffer could not be allocated.
    AllocationFailed,
    /// A size cannot be represented by the canonical fingerprint format.
    SizeOverflow,
    /// The plan contains no calendar shift parameter.
    CalendarShiftNotPlanned,
    /// The supplied WorldTime belongs to a different TimelineId than the plan.
    TimelineMismatch {
        /// Timeline required by the plan.
        planned: TimelineId,
        /// Timeline found in the input value.
        actual: TimelineId,
    },
    /// Checked calendar or UTC-coordinate arithmetic overflowed.
    CalendarArithmeticOverflow,
}

impl fmt::Display for MigrationTransformerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "migration transformer version {} is not implemented",
                version.value()
            ),
            Self::Plan(error) => write!(formatter, "migration plan validation failed: {error}"),
            Self::RecordCodec(error) => write!(formatter, "migration record is invalid: {error}"),
            Self::WorkBudgetExceeded { requested, limit } => {
                write!(
                    formatter,
                    "migration work requires {requested} units, limit is {limit}"
                )
            }
            Self::MemoryBudgetExceeded { requested, limit } => write!(
                formatter,
                "migration transform reserves {requested} bytes, limit is {limit}"
            ),
            Self::AllocationFailed => formatter.write_str("migration transform allocation failed"),
            Self::SizeOverflow => formatter.write_str("migration transform size overflowed"),
            Self::CalendarShiftNotPlanned => {
                formatter.write_str("migration plan has no calendar shift")
            }
            Self::TimelineMismatch { planned, actual } => write!(
                formatter,
                "migration calendar shift expects timeline {planned}, got {actual}"
            ),
            Self::CalendarArithmeticOverflow => {
                formatter.write_str("migration calendar arithmetic overflowed")
            }
        }
    }
}

impl std::error::Error for MigrationTransformerError {}

#[cfg(test)]
mod tests {
    use super::{MigrationTransformer, MigrationTransformerError, NANOS_PER_DAY, days_from_civil};
    use crate::ids::{
        DomainId, EntityId, EntityTypeId, IdValidationError, MigrationId, MigrationRunId,
        MigrationStepId, PredicateId, Revision, SchemaRevision, TimelineId,
    };
    use crate::{
        CalendarPeriod, DecoderLimits, Entity, JobBudget, MigrationCalendarDirection,
        MigrationCalendarShift, MigrationCategory, MigrationPlan, MigrationPlanError,
        MigrationPlanSpec, MigrationRun, MigrationRunState, MigrationTargetSchema,
        MigrationTransformerVersion, Record, SchemaDefinitionId, SchemaIdentityTransition,
        SourceSchemaPrecondition, Timeline, WorldTime, decode_record_with_limits, encode_record,
    };

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn plan(calendar_shift: Option<MigrationCalendarShift>) -> Result<MigrationPlan, String> {
        plan_with_budget(calendar_shift, 10, 1024 * 1024)
    }

    fn plan_with_budget(
        calendar_shift: Option<MigrationCalendarShift>,
        max_work_units: u64,
        max_memory_bytes: u64,
    ) -> Result<MigrationPlan, String> {
        let migration_id = uuid::<MigrationId>(1).map_err(|error| error.to_string())?;
        let step_id = uuid::<MigrationStepId>(2).map_err(|error| error.to_string())?;
        let predicate_id = uuid::<PredicateId>(3).map_err(|error| error.to_string())?;
        let change = SchemaIdentityTransition::new(
            None,
            Some(SchemaDefinitionId::Predicate(predicate_id)),
            MigrationCategory::Additive,
        )
        .map_err(|error| error.to_string())?;
        MigrationPlan::new(MigrationPlanSpec {
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
            steps: vec![step_id],
            schema_changes: vec![change],
            transformer_version: MigrationTransformerVersion::new(1)
                .map_err(|error| error.to_string())?,
            calendar_shift,
            budget: JobBudget::new(max_work_units, max_memory_bytes)
                .map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())
    }

    fn time(
        timeline_id: TimelineId,
        year: i128,
        month: u8,
        day: u8,
        time_of_day: i128,
    ) -> Result<WorldTime, MigrationTransformerError> {
        let days = days_from_civil(year, month, day)?;
        let nanoseconds = days
            .checked_mul(super::NANOS_PER_DAY)
            .and_then(|value| value.checked_add(time_of_day))
            .ok_or(MigrationTransformerError::CalendarArithmeticOverflow)?;
        Ok(WorldTime::from_nanoseconds(
            Timeline::new(timeline_id),
            nanoseconds,
        ))
    }

    fn calendar_plan(
        timeline: TimelineId,
        period: CalendarPeriod,
        direction: MigrationCalendarDirection,
        epoch: i128,
    ) -> Result<MigrationPlan, String> {
        plan(Some(MigrationCalendarShift::new(
            timeline, epoch, period, direction,
        )))
    }

    #[test]
    fn same_plan_and_record_inputs_produce_identical_canonical_result_bytes() -> Result<(), String>
    {
        let plan = plan(None)?;
        let transformer = MigrationTransformer::for_version(plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let entity = Entity::new(
            uuid::<EntityId>(4).map_err(|error| error.to_string())?,
            uuid::<EntityTypeId>(5).map_err(|error| error.to_string())?,
            Revision::GENESIS,
        );
        let input = encode_record(&Record::Entity(entity)).map_err(|error| error.to_string())?;
        let source = plan.source_schema_precondition();
        let first = transformer
            .transform_records(
                &plan,
                source.revision(),
                *source.fingerprint(),
                &[input.clone()],
            )
            .map_err(|error| error.to_string())?;
        let second = transformer
            .transform_records(&plan, source.revision(), *source.fingerprint(), &[input])
            .map_err(|error| error.to_string())?;
        assert_eq!(first, second);
        assert_eq!(first.records().len(), 1);
        Ok(())
    }

    #[test]
    fn transform_preserves_record_order_and_frame_bytes() -> Result<(), String> {
        let plan = plan(None)?;
        let transformer = MigrationTransformer::for_version(plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let first = encode_record(&Record::Entity(Entity::new(
            uuid::<EntityId>(6).map_err(|error| error.to_string())?,
            uuid::<EntityTypeId>(7).map_err(|error| error.to_string())?,
            Revision::GENESIS,
        )))
        .map_err(|error| error.to_string())?;
        let second = encode_record(&Record::Entity(Entity::new(
            uuid::<EntityId>(8).map_err(|error| error.to_string())?,
            uuid::<EntityTypeId>(9).map_err(|error| error.to_string())?,
            Revision::GENESIS,
        )))
        .map_err(|error| error.to_string())?;
        let source = plan.source_schema_precondition();
        let forward = transformer
            .transform_records(
                &plan,
                source.revision(),
                *source.fingerprint(),
                &[first.clone(), second.clone()],
            )
            .map_err(|error| error.to_string())?;
        let reverse = transformer
            .transform_records(
                &plan,
                source.revision(),
                *source.fingerprint(),
                &[second.clone(), first.clone()],
            )
            .map_err(|error| error.to_string())?;
        assert_ne!(forward, reverse);
        assert_eq!(forward.records(), &[first.clone(), second.clone()]);
        assert_eq!(reverse.records(), &[second, first]);
        Ok(())
    }

    #[test]
    fn transform_result_fingerprint_binds_the_plan_and_version() -> Result<(), String> {
        let timeline_id = uuid::<TimelineId>(18).map_err(|error| error.to_string())?;
        let period = CalendarPeriod::new(0, 1, 0).map_err(|error| error.to_string())?;
        let first_plan = calendar_plan(timeline_id, period, MigrationCalendarDirection::Future, 0)?;
        let second_plan =
            calendar_plan(timeline_id, period, MigrationCalendarDirection::Future, 1)?;
        let transformer = MigrationTransformer::for_version(first_plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let entity = Entity::new(
            uuid::<EntityId>(19).map_err(|error| error.to_string())?,
            uuid::<EntityTypeId>(20).map_err(|error| error.to_string())?,
            Revision::GENESIS,
        );
        let input = encode_record(&Record::Entity(entity)).map_err(|error| error.to_string())?;
        let first_source = first_plan.source_schema_precondition();
        let second_source = second_plan.source_schema_precondition();
        let first = transformer
            .transform_records(
                &first_plan,
                first_source.revision(),
                *first_source.fingerprint(),
                &[input.clone()],
            )
            .map_err(|error| error.to_string())?;
        let second = transformer
            .transform_records(
                &second_plan,
                second_source.revision(),
                *second_source.fingerprint(),
                &[input],
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(first.records(), second.records());
        assert_ne!(first.fingerprint(), second.fingerprint());
        Ok(())
    }

    #[test]
    fn canonical_wire_plan_drives_the_versioned_transform() -> Result<(), String> {
        let timeline_id = uuid::<TimelineId>(24).map_err(|error| error.to_string())?;
        let period = CalendarPeriod::new(0, 1, 0).map_err(|error| error.to_string())?;
        let plan = calendar_plan(timeline_id, period, MigrationCalendarDirection::Future, 0)?;
        let plan_bytes = encode_record(&Record::MigrationPlan(plan.clone()))
            .map_err(|error| error.to_string())?;
        let decoded_plan = decode_record_with_limits(&plan_bytes, &DecoderLimits::DEFAULT)
            .map_err(|error| error.to_string())?
            .into_record();
        let Record::MigrationPlan(decoded_plan) = decoded_plan else {
            return Err(String::from(
                "migration-plan wire record decoded to another kind",
            ));
        };
        assert_eq!(decoded_plan, plan);

        let entity = Entity::new(
            uuid::<EntityId>(25).map_err(|error| error.to_string())?,
            uuid::<EntityTypeId>(26).map_err(|error| error.to_string())?,
            Revision::GENESIS,
        );
        let input = encode_record(&Record::Entity(entity)).map_err(|error| error.to_string())?;
        let source = decoded_plan.source_schema_precondition();
        let transformer = MigrationTransformer::for_version(decoded_plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let batch = transformer
            .transform_records(
                &decoded_plan,
                source.revision(),
                *source.fingerprint(),
                &[input.clone()],
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(batch.records(), &[input]);

        let input_time = time(timeline_id, 2021, 1, 31, 0).map_err(|error| error.to_string())?;
        assert_eq!(
            transformer
                .transform_world_time(
                    &decoded_plan,
                    source.revision(),
                    *source.fingerprint(),
                    input_time,
                )
                .map_err(|error| error.to_string())?,
            time(timeline_id, 2021, 2, 28, 0).map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn transform_enforces_work_memory_and_canonical_input_budgets() -> Result<(), String> {
        let entity = Entity::new(
            uuid::<EntityId>(21).map_err(|error| error.to_string())?,
            uuid::<EntityTypeId>(22).map_err(|error| error.to_string())?,
            Revision::GENESIS,
        );
        let input = encode_record(&Record::Entity(entity)).map_err(|error| error.to_string())?;
        let work_plan = plan_with_budget(None, 1, 1024 * 1024)?;
        let work_transformer = MigrationTransformer::for_version(work_plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let work_source = work_plan.source_schema_precondition();
        assert!(matches!(
            work_transformer.transform_records(
                &work_plan,
                work_source.revision(),
                *work_source.fingerprint(),
                &[input.clone(), input.clone()],
            ),
            Err(MigrationTransformerError::WorkBudgetExceeded {
                requested: 2,
                limit: 1
            })
        ));

        let memory_plan = plan_with_budget(None, 10, 1)?;
        let memory_transformer =
            MigrationTransformer::for_version(memory_plan.transformer_version())
                .map_err(|error| error.to_string())?;
        let memory_source = memory_plan.source_schema_precondition();
        assert!(matches!(
            memory_transformer.transform_records(
                &memory_plan,
                memory_source.revision(),
                *memory_source.fingerprint(),
                &[input.clone()],
            ),
            Err(MigrationTransformerError::MemoryBudgetExceeded { limit: 1, .. })
        ));

        let regular_plan = plan(None)?;
        let regular_transformer =
            MigrationTransformer::for_version(regular_plan.transformer_version())
                .map_err(|error| error.to_string())?;
        let regular_source = regular_plan.source_schema_precondition();
        assert!(matches!(
            regular_transformer.transform_records(
                &regular_plan,
                regular_source.revision(),
                *regular_source.fingerprint(),
                &[vec![0; 12]],
            ),
            Err(MigrationTransformerError::RecordCodec(_))
        ));
        Ok(())
    }

    fn reference_leap_year(year: i64) -> bool {
        year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
    }

    fn reference_days_in_month(year: i64, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if reference_leap_year(year) => 29,
            2 => 28,
            _ => 0,
        }
    }

    fn reference_days_since_epoch(year: i64, month: u8, day: u8) -> i128 {
        let mut days = 0_i128;
        if year >= 1970 {
            for prior_year in 1970..year {
                days += if reference_leap_year(prior_year) {
                    366
                } else {
                    365
                };
            }
        } else {
            for prior_year in year..1970 {
                days -= if reference_leap_year(prior_year) {
                    366
                } else {
                    365
                };
            }
        }
        for prior_month in 1..month {
            days += i128::from(reference_days_in_month(year, prior_month));
        }
        days + i128::from(day) - 1
    }

    fn reference_shift_date(
        mut year: i64,
        mut month: u8,
        mut day: u8,
        period: CalendarPeriod,
        direction: MigrationCalendarDirection,
    ) -> Result<(i64, u8, u8), String> {
        let sign = match direction {
            MigrationCalendarDirection::Past => -1_i64,
            MigrationCalendarDirection::Future => 1_i64,
        };
        year += sign * i64::from(period.years());
        day = day.min(reference_days_in_month(year, month));

        let month_index = i64::from(month) - 1 + sign * i64::from(period.months());
        year += month_index.div_euclid(12);
        month = u8::try_from(month_index.rem_euclid(12) + 1).map_err(|error| error.to_string())?;
        day = day.min(reference_days_in_month(year, month));

        for _ in 0..period.days() {
            if sign > 0 {
                day += 1;
                if day > reference_days_in_month(year, month) {
                    day = 1;
                    month += 1;
                    if month > 12 {
                        month = 1;
                        year += 1;
                    }
                }
            } else if day > 1 {
                day -= 1;
            } else if month > 1 {
                month -= 1;
                day = reference_days_in_month(year, month);
            } else {
                year -= 1;
                month = 12;
                day = 31;
            }
        }
        Ok((year, month, day))
    }

    #[test]
    fn calendar_transform_matches_independent_gregorian_oracle() -> Result<(), String> {
        let timeline_id = uuid::<TimelineId>(23).map_err(|error| error.to_string())?;
        let time_of_day = 43_210_123_456_789_i128;
        let periods = [
            CalendarPeriod::new(0, 1, 0).map_err(|error| error.to_string())?,
            CalendarPeriod::new(1, 11, 1).map_err(|error| error.to_string())?,
            CalendarPeriod::new(0, 0, 31).map_err(|error| error.to_string())?,
        ];
        for period in periods {
            for direction in [
                MigrationCalendarDirection::Past,
                MigrationCalendarDirection::Future,
            ] {
                let plan = calendar_plan(timeline_id, period, direction, 0)?;
                let transformer = MigrationTransformer::for_version(plan.transformer_version())
                    .map_err(|error| error.to_string())?;
                let source = plan.source_schema_precondition();
                for year in [1899_i64, 1900, 1901, 1999, 2000, 2001] {
                    for month in 1..=12 {
                        let last_day = reference_days_in_month(year, month);
                        let days = [1, 15, last_day];
                        for day in days {
                            let input_nanoseconds = reference_days_since_epoch(year, month, day)
                                .checked_mul(NANOS_PER_DAY)
                                .and_then(|value| value.checked_add(time_of_day))
                                .ok_or_else(|| String::from("oracle input overflowed"))?;
                            let input = WorldTime::from_nanoseconds(
                                Timeline::new(timeline_id),
                                input_nanoseconds,
                            );
                            let (expected_year, expected_month, expected_day) =
                                reference_shift_date(year, month, day, period, direction)?;
                            let expected_nanoseconds = reference_days_since_epoch(
                                expected_year,
                                expected_month,
                                expected_day,
                            )
                            .checked_mul(NANOS_PER_DAY)
                            .and_then(|value| value.checked_add(time_of_day))
                            .ok_or_else(|| String::from("oracle output overflowed"))?;
                            let actual = transformer
                                .transform_world_time(
                                    &plan,
                                    source.revision(),
                                    *source.fingerprint(),
                                    input,
                                )
                                .map_err(|error| error.to_string())?;
                            assert_eq!(actual.nanoseconds(), expected_nanoseconds);
                            assert_eq!(actual.timeline().id(), timeline_id);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn transformer_version_mismatch_rejects_start_and_resume() -> Result<(), String> {
        let plan = plan(None)?;
        let run = MigrationRun::new(
            uuid::<MigrationRunId>(10).map_err(|error| error.to_string())?,
            plan.migration_id(),
            MigrationRunState::Running,
        );
        let source = plan.source_schema_precondition();
        let wrong_version =
            MigrationTransformerVersion::new(2).map_err(|error| error.to_string())?;
        let expected_error = MigrationPlanError::TransformerVersionMismatch {
            planned: plan.transformer_version(),
            actual: wrong_version,
        };
        assert_eq!(
            plan.validate_start(source.revision(), *source.fingerprint(), wrong_version),
            Err(expected_error)
        );
        assert_eq!(
            plan.validate_resume(run, source.revision(), *source.fingerprint(), wrong_version),
            Err(expected_error)
        );
        assert!(matches!(
            MigrationTransformer::for_version(wrong_version),
            Err(MigrationTransformerError::UnsupportedVersion(_))
        ));
        Ok(())
    }

    #[test]
    fn resume_rejects_a_run_for_another_plan() -> Result<(), String> {
        let plan = plan(None)?;
        let run = MigrationRun::new(
            uuid::<MigrationRunId>(11).map_err(|error| error.to_string())?,
            uuid::<MigrationId>(12).map_err(|error| error.to_string())?,
            MigrationRunState::Running,
        );
        let source = plan.source_schema_precondition();
        assert_eq!(
            plan.validate_resume(
                run,
                source.revision(),
                *source.fingerprint(),
                plan.transformer_version(),
            ),
            Err(MigrationPlanError::RunPlanIdentityMismatch)
        );
        Ok(())
    }

    #[test]
    fn calendar_shift_clamps_month_end_and_preserves_time_of_day() -> Result<(), String> {
        let timeline_id = uuid::<TimelineId>(13).map_err(|error| error.to_string())?;
        let period = CalendarPeriod::new(0, 1, 0).map_err(|error| error.to_string())?;
        let plan = calendar_plan(timeline_id, period, MigrationCalendarDirection::Future, 0)?;
        let transformer = MigrationTransformer::for_version(plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let time_of_day = 12 * 3_600_000_000_000_i128 + 123_456_789;
        let input =
            time(timeline_id, 2021, 1, 31, time_of_day).map_err(|error| error.to_string())?;
        let source = plan.source_schema_precondition();
        let output = transformer
            .transform_world_time(&plan, source.revision(), *source.fingerprint(), input)
            .map_err(|error| error.to_string())?;
        let expected =
            time(timeline_id, 2021, 2, 28, time_of_day).map_err(|error| error.to_string())?;
        assert_eq!(output, expected);
        Ok(())
    }

    #[test]
    fn calendar_shift_uses_gregorian_leap_rules_and_explicit_past_direction() -> Result<(), String>
    {
        let timeline_id = uuid::<TimelineId>(14).map_err(|error| error.to_string())?;
        let one_year = CalendarPeriod::new(1, 0, 0).map_err(|error| error.to_string())?;
        let future = calendar_plan(timeline_id, one_year, MigrationCalendarDirection::Future, 0)?;
        let past = calendar_plan(
            timeline_id,
            CalendarPeriod::new(0, 1, 0).map_err(|error| error.to_string())?,
            MigrationCalendarDirection::Past,
            0,
        )?;
        let transformer = MigrationTransformer::for_version(future.transformer_version())
            .map_err(|error| error.to_string())?;
        let future_source = future.source_schema_precondition();
        let leap_day = time(timeline_id, 2020, 2, 29, 0).map_err(|error| error.to_string())?;
        assert_eq!(
            transformer
                .transform_world_time(
                    &future,
                    future_source.revision(),
                    *future_source.fingerprint(),
                    leap_day,
                )
                .map_err(|error| error.to_string())?,
            time(timeline_id, 2021, 2, 28, 0).map_err(|error| error.to_string())?
        );
        let past_source = past.source_schema_precondition();
        let march_end = time(timeline_id, 2021, 3, 31, 0).map_err(|error| error.to_string())?;
        assert_eq!(
            transformer
                .transform_world_time(
                    &past,
                    past_source.revision(),
                    *past_source.fingerprint(),
                    march_end,
                )
                .map_err(|error| error.to_string())?,
            time(timeline_id, 2021, 2, 28, 0).map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn calendar_shift_rejects_wrong_timeline_and_checked_overflow() -> Result<(), String> {
        let timeline_id = uuid::<TimelineId>(15).map_err(|error| error.to_string())?;
        let other_timeline = uuid::<TimelineId>(16).map_err(|error| error.to_string())?;
        let plan = calendar_plan(
            timeline_id,
            CalendarPeriod::new(0, 0, 1).map_err(|error| error.to_string())?,
            MigrationCalendarDirection::Future,
            0,
        )?;
        let transformer = MigrationTransformer::for_version(plan.transformer_version())
            .map_err(|error| error.to_string())?;
        let source = plan.source_schema_precondition();
        assert!(matches!(
            transformer.transform_world_time(
                &plan,
                source.revision(),
                *source.fingerprint(),
                WorldTime::from_nanoseconds(Timeline::new(other_timeline), 0),
            ),
            Err(MigrationTransformerError::TimelineMismatch { .. })
        ));
        assert_eq!(
            transformer.transform_world_time(
                &plan,
                source.revision(),
                *source.fingerprint(),
                WorldTime::from_nanoseconds(Timeline::new(timeline_id), i128::MAX),
            ),
            Err(MigrationTransformerError::CalendarArithmeticOverflow)
        );
        Ok(())
    }

    #[test]
    fn calendar_shift_parameters_change_the_plan_fingerprint() -> Result<(), String> {
        let timeline_id = uuid::<TimelineId>(17).map_err(|error| error.to_string())?;
        let period = CalendarPeriod::new(0, 1, 0).map_err(|error| error.to_string())?;
        let future = calendar_plan(timeline_id, period, MigrationCalendarDirection::Future, 0)?;
        let past = calendar_plan(timeline_id, period, MigrationCalendarDirection::Past, 0)?;
        let shifted_epoch =
            calendar_plan(timeline_id, period, MigrationCalendarDirection::Future, 1)?;
        assert_ne!(future.fingerprint(), past.fingerprint());
        assert_ne!(future.fingerprint(), shifted_epoch.fingerprint());
        Ok(())
    }
}
