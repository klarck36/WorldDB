//! Ordered shape and post-transaction schema validation for assertion writes.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::assertions::AssertionDraft;
use crate::context::ContextKey;
use crate::ids::{PredicateId, PrincipalId, Revision};
use crate::schema::{ConstraintSet, InclusiveRange, Lifecycle, ValueConstraint, ValueKind};
use crate::schema_history::SchemaDefinition;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicySnapshot,
};
use crate::temporal::{TemporalError, WorldTime};
use crate::values::{Time, Value};
use crate::wire::{DecodeResource, DecoderLimits};
use crate::{SchemaSnapshot, UInt};

/// Typed warning attached to a successful write against a Deprecated predicate.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeprecatedSchemaWriteWarning {
    predicate_id: PredicateId,
    schema_revision: Revision,
    capability: Capability,
}

impl DeprecatedSchemaWriteWarning {
    /// Predicate whose deprecated schema accepted the write.
    #[must_use]
    pub const fn predicate_id(self) -> PredicateId {
        self.predicate_id
    }

    /// Schema revision that was validated.
    #[must_use]
    pub const fn schema_revision(self) -> Revision {
        self.schema_revision
    }

    /// Exact capability that authorized the write.
    #[must_use]
    pub const fn capability(self) -> Capability {
        self.capability
    }
}

/// Fully validated assertion drafts and their typed schema warnings.
pub struct ValidatedAssertionBatch {
    drafts: Vec<AssertionDraft>,
    warnings: Vec<DeprecatedSchemaWriteWarning>,
    schema_revision: Revision,
}

impl ValidatedAssertionBatch {
    /// Drafts validated as one all-or-nothing batch.
    #[must_use]
    pub fn drafts(&self) -> &[AssertionDraft] {
        &self.drafts
    }

    /// Typed warnings to carry forward into the commit result.
    #[must_use]
    pub fn warnings(&self) -> &[DeprecatedSchemaWriteWarning] {
        &self.warnings
    }

    /// Post-transaction schema revision used to validate this batch.
    #[must_use]
    pub const fn schema_revision(&self) -> Revision {
        self.schema_revision
    }

    /// Consumes the validated batch into its drafts and typed warnings.
    #[must_use]
    pub fn into_parts(self) -> (Vec<AssertionDraft>, Vec<DeprecatedSchemaWriteWarning>) {
        (self.drafts, self.warnings)
    }
}

/// Fixed-order shape, constraint, schema lifecycle, or capability rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaWriteValidationError {
    /// A batch or scalar exceeded the supplied finite resource policy.
    ResourceLimit {
        /// Which configured limit was exceeded.
        resource: DecodeResource,
        /// Configured maximum.
        limit: usize,
        /// Observed size.
        actual: usize,
    },
    /// No predicate exists in the post-transaction schema snapshot.
    PredicateNotFound(PredicateId),
    /// The assertion value's closed scalar variant differs from the schema.
    ValueKindMismatch {
        /// Value family declared by the schema.
        expected: ValueKind,
        /// Actual closed value family.
        actual: ValueKind,
    },
    /// A value did not satisfy one of the predicate's typed constraints.
    ConstraintViolation {
        /// Predicate whose constraint rejected the value.
        predicate_id: PredicateId,
        /// Value family of the failing rule.
        value_kind: ValueKind,
    },
    /// A stored time value or range endpoint could not be resolved by schema.
    TimeResolution(TemporalError),
    /// The predicate is retired and cannot accept a new assertion.
    PredicateRetired(PredicateId),
    /// The deprecated-schema opt-in was not explicitly supplied.
    DeprecatedOptInRequired(PredicateId),
    /// The scoped operation capability did not authorize the deprecated write.
    DeprecatedCapabilityDenied {
        /// Predicate whose write was denied.
        predicate_id: PredicateId,
        /// Required operation capability.
        capability: Capability,
    },
}

impl std::fmt::Display for SchemaWriteValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceLimit {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "write resource {resource:?} is {actual}; configured limit is {limit}"
            ),
            Self::PredicateNotFound(predicate_id) => {
                write!(
                    formatter,
                    "predicate {predicate_id} is absent from the selected schema"
                )
            }
            Self::ValueKindMismatch { expected, actual } => write!(
                formatter,
                "schema accepts {expected:?}; assertion value is {actual:?}"
            ),
            Self::ConstraintViolation {
                predicate_id,
                value_kind,
            } => write!(
                formatter,
                "value for predicate {predicate_id} violates its {value_kind:?} constraint"
            ),
            Self::TimeResolution(error) => {
                write!(formatter, "time value could not be resolved: {error}")
            }
            Self::PredicateRetired(predicate_id) => {
                write!(
                    formatter,
                    "retired predicate {predicate_id} rejects new writes"
                )
            }
            Self::DeprecatedOptInRequired(predicate_id) => write!(
                formatter,
                "predicate {predicate_id} requires explicit deprecated-schema opt-in"
            ),
            Self::DeprecatedCapabilityDenied {
                predicate_id,
                capability,
            } => write!(
                formatter,
                "{capability:?} does not authorize a write to deprecated predicate {predicate_id}"
            ),
        }
    }
}

impl std::error::Error for SchemaWriteValidationError {}

/// Validates assertion shape and values against a post-transaction schema.
///
/// The order is fixed: resource limits, predicate lookup in the supplied
/// post-transaction schema, exact ValueKind, typed constraints, then lifecycle
/// opt-in and scoped capability. No drafts or warning escape on an error.
pub fn validate_assertion_batch(
    drafts: Vec<AssertionDraft>,
    post_transaction_schema: &SchemaSnapshot,
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    allow_deprecated_schema: bool,
    limits: DecoderLimits,
    mut resolve_time: impl FnMut(&Time) -> Result<WorldTime, TemporalError>,
) -> Result<ValidatedAssertionBatch, SchemaWriteValidationError> {
    validate_write_limits(&drafts, limits)?;
    let schema_revision = post_transaction_schema.schema_revision().revision();
    let mut warnings = BTreeSet::new();

    for draft in &drafts {
        let predicate = post_transaction_schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::Predicate(value)
                    if value.predicate_id() == draft.predicate_id() =>
                {
                    Some(value)
                }
                _ => None,
            })
            .ok_or(SchemaWriteValidationError::PredicateNotFound(
                draft.predicate_id(),
            ))?;

        let actual_kind = ValueKind::of(draft.value());
        if predicate.value_kind() != actual_kind {
            return Err(SchemaWriteValidationError::ValueKindMismatch {
                expected: predicate.value_kind(),
                actual: actual_kind,
            });
        }
        validate_constraints(
            predicate.predicate_id(),
            predicate.constraints(),
            draft.value(),
            &mut resolve_time,
        )?;

        match predicate.lifecycle() {
            Lifecycle::Active => {}
            Lifecycle::Retired => {
                return Err(SchemaWriteValidationError::PredicateRetired(
                    predicate.predicate_id(),
                ));
            }
            Lifecycle::Deprecated => {
                if !allow_deprecated_schema {
                    return Err(SchemaWriteValidationError::DeprecatedOptInRequired(
                        predicate.predicate_id(),
                    ));
                }
                let capability = Capability::AssertionCreate;
                let context: ContextKey = draft.context();
                let target = PolicyTarget::new(
                    Some(context.history_space_id()),
                    Some(context.layer_id()),
                    None,
                    Some(FieldSelector::AssertionValue(predicate.predicate_id())),
                    None,
                );
                if policy.authorize(principal, capability, target) != AuthorizationDecision::Allow {
                    return Err(SchemaWriteValidationError::DeprecatedCapabilityDenied {
                        predicate_id: predicate.predicate_id(),
                        capability,
                    });
                }
                warnings.insert(DeprecatedSchemaWriteWarning {
                    predicate_id: predicate.predicate_id(),
                    schema_revision,
                    capability,
                });
            }
        }
    }

    Ok(ValidatedAssertionBatch {
        drafts,
        warnings: warnings.into_iter().collect(),
        schema_revision,
    })
}

fn validate_write_limits(
    drafts: &[AssertionDraft],
    limits: DecoderLimits,
) -> Result<(), SchemaWriteValidationError> {
    if drafts.len() > limits.max_records_per_batch {
        return Err(SchemaWriteValidationError::ResourceLimit {
            resource: DecodeResource::BatchRecords,
            limit: limits.max_records_per_batch,
            actual: drafts.len(),
        });
    }

    let mut collection_bytes = 0_usize;
    for draft in drafts {
        let length = value_variable_length(draft.value());
        if length > limits.max_string_or_bytes {
            return Err(SchemaWriteValidationError::ResourceLimit {
                resource: DecodeResource::StringOrBytes,
                limit: limits.max_string_or_bytes,
                actual: length,
            });
        }
        collection_bytes = collection_bytes.checked_add(length).ok_or(
            SchemaWriteValidationError::ResourceLimit {
                resource: DecodeResource::CollectionBytes,
                limit: limits.max_collection_bytes,
                actual: usize::MAX,
            },
        )?;
        if collection_bytes > limits.max_collection_bytes {
            return Err(SchemaWriteValidationError::ResourceLimit {
                resource: DecodeResource::CollectionBytes,
                limit: limits.max_collection_bytes,
                actual: collection_bytes,
            });
        }
    }
    Ok(())
}

fn value_variable_length(value: &Value) -> usize {
    match value {
        Value::String(value) => value.len(),
        Value::Symbol(value) => value.as_str().len(),
        Value::Time(value) => value.unit().as_str().len(),
        Value::Bytes(value) => value.len(),
        Value::Bool(_)
        | Value::Int(_)
        | Value::UInt(_)
        | Value::Decimal(_)
        | Value::Entity(_)
        | Value::Duration(_) => 0,
    }
}

fn validate_constraints(
    predicate_id: PredicateId,
    constraints: &ConstraintSet,
    value: &Value,
    resolve_time: &mut impl FnMut(&Time) -> Result<WorldTime, TemporalError>,
) -> Result<(), SchemaWriteValidationError> {
    for constraint in constraints.rules() {
        let matches = match (constraint, value) {
            (ValueConstraint::BoolSet(allowed), Value::Bool(value)) => {
                allowed.as_slice().contains(value)
            }
            (ValueConstraint::IntRange(range), Value::Int(value)) => in_range(value, range),
            (ValueConstraint::UIntRange(range), Value::UInt(value)) => in_range(value, range),
            (ValueConstraint::DecimalRange(range), Value::Decimal(value)) => in_range(value, range),
            (ValueConstraint::StringByteLength(range), Value::String(value)) => {
                in_range(&UInt::new(value.len() as u128), range)
            }
            (ValueConstraint::SymbolSet(allowed), Value::Symbol(value)) => {
                allowed.as_slice().contains(value)
            }
            (ValueConstraint::TimeRange(range), Value::Time(value)) => {
                let actual =
                    resolve_time(value).map_err(SchemaWriteValidationError::TimeResolution)?;
                let minimum_ok = match range.min() {
                    Some(minimum) => {
                        actual
                            .checked_cmp(
                                resolve_time(minimum)
                                    .map_err(SchemaWriteValidationError::TimeResolution)?,
                            )
                            .map_err(SchemaWriteValidationError::TimeResolution)?
                            != Ordering::Less
                    }
                    None => true,
                };
                let maximum_ok = match range.max() {
                    Some(maximum) => {
                        actual
                            .checked_cmp(
                                resolve_time(maximum)
                                    .map_err(SchemaWriteValidationError::TimeResolution)?,
                            )
                            .map_err(SchemaWriteValidationError::TimeResolution)?
                            != Ordering::Greater
                    }
                    None => true,
                };
                minimum_ok && maximum_ok
            }
            (ValueConstraint::DurationRange(range), Value::Duration(value)) => {
                in_range(value, range)
            }
            (ValueConstraint::BytesLength(range), Value::Bytes(value)) => {
                in_range(&UInt::new(value.len() as u128), range)
            }
            _ => false,
        };
        if !matches {
            return Err(SchemaWriteValidationError::ConstraintViolation {
                predicate_id,
                value_kind: constraint.value_kind(),
            });
        }
    }
    Ok(())
}

fn in_range<T: Ord>(value: &T, range: &InclusiveRange<T>) -> bool {
    let above_minimum = range.min().is_none_or(|minimum| value >= minimum);
    let below_maximum = range.max().is_none_or(|maximum| value <= maximum);
    above_minimum && below_maximum
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        DeprecatedSchemaWriteWarning, SchemaWriteValidationError, validate_assertion_batch,
    };
    use crate::assertions::{AssertionDraft, Polarity, Subject};
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        DomainId, EntityId, HistorySpaceId, IdValidationError, LayerId, OperationId, PolicyRuleId,
        PredicateId, PrincipalId, Revision, TimelineId,
    };
    use crate::schema::{
        Cardinality, ConstraintSet, InclusiveRange, Lifecycle, PredicateDefinition,
        PredicateDefinitionSpec, ResolutionPolicy, TimeRange, ValueConstraint, ValueKind,
    };
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, FieldSelector, GrantEffect, PolicyScope,
        PolicySubject, SecurityPolicySnapshot,
    };
    use crate::temporal::{AssertionValidity, TimeInterval, Timeline, WorldTime};
    use crate::values::{Bytes, Symbol, Time, Value};
    use crate::wire::{DecodeResource, DecoderLimits};
    use crate::{Decimal, Int, UInt};

    type TestResult<T> = Result<T, String>;

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    struct Fixture {
        schema: crate::SchemaSnapshot,
        principal: PrincipalId,
        predicate: PredicateId,
        context: ContextKey,
        entity: EntityId,
    }

    fn fixture(lifecycle: Lifecycle, constraints: ConstraintSet) -> TestResult<Fixture> {
        let predicate = uuid::<PredicateId>(1).map_err(|error| error.to_string())?;
        let space = uuid::<HistorySpaceId>(2).map_err(|error| error.to_string())?;
        let layer = uuid::<LayerId>(3).map_err(|error| error.to_string())?;
        let principal = uuid::<PrincipalId>(4).map_err(|error| error.to_string())?;
        let entity = uuid::<EntityId>(5).map_err(|error| error.to_string())?;
        let definition = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: predicate,
            symbol: Symbol::new("score").map_err(|error| error.to_string())?,
            subject_constraint: crate::EntityTypeConstraint::AnyEntity,
            value_kind: constraints
                .rules()
                .first()
                .map(ValueConstraint::value_kind)
                .unwrap_or(ValueKind::Int),
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints,
            decimal_metadata: None,
            lifecycle,
            created_revision: Revision::FIRST_COMMIT,
        })
        .map_err(|error| error.to_string())?;
        let mut history = SchemaHistoryReferenceModel::new();
        history
            .publish(
                Revision::FIRST_COMMIT,
                vec![SchemaDefinition::Predicate(definition)],
            )
            .map_err(|error| error.to_string())?;
        let schema = history
            .schema_at(SchemaMode::Current, Revision::FIRST_COMMIT)
            .map_err(|error| error.to_string())?;
        let context = ContextKey::new(
            space,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )
        .map_err(|error| error.to_string())?;
        Ok(Fixture {
            schema,
            principal,
            predicate,
            context,
            entity,
        })
    }

    fn draft(fixture: &Fixture, value: Value) -> TestResult<AssertionDraft> {
        let timeline = Timeline::new(uuid::<TimelineId>(6).map_err(|error| error.to_string())?);
        let validity = AssertionValidity::new(
            TimeInterval::new(timeline, None, None).map_err(|error| error.to_string())?,
        );
        Ok(AssertionDraft::new(
            fixture.context,
            Subject::new(fixture.entity),
            fixture.predicate,
            value,
            Polarity::Positive,
            validity,
        ))
    }

    fn policy(fixture: &Fixture, allowed: bool) -> TestResult<SecurityPolicySnapshot> {
        let rule = CapabilityRule::new(
            uuid::<PolicyRuleId>(7).map_err(|error| error.to_string())?,
            PolicySubject::Principal(fixture.principal),
            CapabilityGrant::new(
                Capability::AssertionCreate,
                if allowed {
                    GrantEffect::Allow
                } else {
                    GrantEffect::Deny
                },
            ),
            PolicyScope::new(
                Some(fixture.context.history_space_id()),
                Some(fixture.context.layer_id()),
                None,
                Some(FieldSelector::AssertionValue(fixture.predicate)),
                None,
            ),
        );
        SecurityPolicySnapshot::new(
            vec![crate::Principal::new(fixture.principal)],
            vec![],
            vec![],
            vec![rule],
        )
        .map_err(|error| error.to_string())
    }

    fn resolve_same_unit_time(value: &Time) -> Result<WorldTime, crate::TemporalError> {
        Ok(WorldTime::from_nanoseconds(
            Timeline::new(value.timeline_id()),
            value.ticks(),
        ))
    }

    fn unconstrained() -> ConstraintSet {
        ConstraintSet::unconstrained()
    }

    #[test]
    fn limits_precede_schema_lookup_and_reject_without_a_partial_result() -> TestResult<()> {
        let fixture = fixture(Lifecycle::Active, unconstrained())?;
        let value = draft(&fixture, Value::String("oversized".to_owned()))?;
        let policy = policy(&fixture, true)?;
        let limits = DecoderLimits {
            max_string_or_bytes: 3,
            ..DecoderLimits::DEFAULT
        };
        let result = validate_assertion_batch(
            vec![value.clone()],
            &fixture.schema,
            &policy,
            fixture.principal,
            false,
            limits,
            resolve_same_unit_time,
        );
        assert_eq!(
            result.err(),
            Some(SchemaWriteValidationError::ResourceLimit {
                resource: DecodeResource::StringOrBytes,
                limit: 3,
                actual: 9,
            })
        );

        let unknown_schema = SchemaHistoryReferenceModel::new()
            .schema_at(SchemaMode::Historical, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            validate_assertion_batch(
                vec![value.clone()],
                &unknown_schema,
                &policy,
                fixture.principal,
                false,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            ),
            Err(SchemaWriteValidationError::PredicateNotFound(_))
        ));
        Ok(())
    }

    #[test]
    fn record_count_and_combined_value_memory_limits_are_enforced() -> TestResult<()> {
        let fixture = fixture(Lifecycle::Active, unconstrained())?;
        let policy = policy(&fixture, true)?;
        let first = draft(&fixture, Value::String("ab".to_owned()))?;
        let second = draft(&fixture, Value::String("cd".to_owned()))?;
        let too_many = DecoderLimits {
            max_records_per_batch: 1,
            ..DecoderLimits::DEFAULT
        };
        assert_eq!(
            validate_assertion_batch(
                vec![first.clone(), second.clone()],
                &fixture.schema,
                &policy,
                fixture.principal,
                false,
                too_many,
                resolve_same_unit_time,
            )
            .err(),
            Some(SchemaWriteValidationError::ResourceLimit {
                resource: DecodeResource::BatchRecords,
                limit: 1,
                actual: 2,
            })
        );

        let too_much_memory = DecoderLimits {
            max_collection_bytes: 3,
            ..DecoderLimits::DEFAULT
        };
        assert_eq!(
            validate_assertion_batch(
                vec![first, second],
                &fixture.schema,
                &policy,
                fixture.principal,
                false,
                too_much_memory,
                resolve_same_unit_time,
            )
            .err(),
            Some(SchemaWriteValidationError::ResourceLimit {
                resource: DecodeResource::CollectionBytes,
                limit: 3,
                actual: 4,
            })
        );
        Ok(())
    }

    #[test]
    fn exact_value_kind_and_constraints_are_checked_against_post_schema() -> TestResult<()> {
        let range = InclusiveRange::new(Some(Int::new(1)), Some(Int::new(5)))
            .map_err(|error| error.to_string())?;
        let fixture = fixture(
            Lifecycle::Active,
            ConstraintSet::new(vec![ValueConstraint::IntRange(range)])
                .map_err(|error| error.to_string())?,
        )?;
        let policy = policy(&fixture, true)?;
        let valid = validate_assertion_batch(
            vec![draft(&fixture, Value::Int(Int::new(3)))?],
            &fixture.schema,
            &policy,
            fixture.principal,
            false,
            DecoderLimits::DEFAULT,
            resolve_same_unit_time,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(valid.drafts().len(), 1);
        assert!(valid.warnings().is_empty());

        assert!(matches!(
            validate_assertion_batch(
                vec![draft(&fixture, Value::String("wrong kind".to_owned()))?],
                &fixture.schema,
                &policy,
                fixture.principal,
                false,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            ),
            Err(SchemaWriteValidationError::ValueKindMismatch { .. })
        ));
        assert!(matches!(
            validate_assertion_batch(
                vec![draft(&fixture, Value::Int(Int::new(9)))?],
                &fixture.schema,
                &policy,
                fixture.principal,
                false,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            ),
            Err(SchemaWriteValidationError::ConstraintViolation { .. })
        ));
        Ok(())
    }

    #[test]
    fn single_cardinality_does_not_suppress_conflicting_assertion_drafts() -> TestResult<()> {
        let fixture = fixture(Lifecycle::Active, unconstrained())?;
        let policy = policy(&fixture, true)?;
        let batch = validate_assertion_batch(
            vec![
                draft(&fixture, Value::Int(Int::new(3)))?,
                draft(&fixture, Value::Int(Int::new(4)))?,
            ],
            &fixture.schema,
            &policy,
            fixture.principal,
            false,
            DecoderLimits::DEFAULT,
            resolve_same_unit_time,
        )
        .map_err(|error| error.to_string())?;

        assert_eq!(batch.drafts().len(), 2);
        assert!(batch.warnings().is_empty());
        Ok(())
    }

    #[test]
    fn deprecated_opt_in_and_exact_scoped_capability_return_typed_warning() -> TestResult<()> {
        let fixture = fixture(Lifecycle::Deprecated, unconstrained())?;
        let allowed_policy = policy(&fixture, true)?;
        let rejected = validate_assertion_batch(
            vec![draft(&fixture, Value::Int(Int::new(2)))?],
            &fixture.schema,
            &allowed_policy,
            fixture.principal,
            false,
            DecoderLimits::DEFAULT,
            resolve_same_unit_time,
        );
        assert_eq!(
            rejected.err(),
            Some(SchemaWriteValidationError::DeprecatedOptInRequired(
                fixture.predicate
            ))
        );

        let denied_policy = policy(&fixture, false)?;
        assert!(matches!(
            validate_assertion_batch(
                vec![draft(&fixture, Value::Int(Int::new(2)))?],
                &fixture.schema,
                &denied_policy,
                fixture.principal,
                true,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            ),
            Err(SchemaWriteValidationError::DeprecatedCapabilityDenied {
                capability: Capability::AssertionCreate,
                ..
            })
        ));

        let accepted = validate_assertion_batch(
            vec![draft(&fixture, Value::Int(Int::new(2)))?],
            &fixture.schema,
            &allowed_policy,
            fixture.principal,
            true,
            DecoderLimits::DEFAULT,
            resolve_same_unit_time,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            accepted.warnings(),
            &[DeprecatedSchemaWriteWarning {
                predicate_id: fixture.predicate,
                schema_revision: Revision::FIRST_COMMIT,
                capability: Capability::AssertionCreate,
            }]
        );
        let operation_id = uuid::<OperationId>(9).map_err(|error| error.to_string())?;
        let receipt = crate::CommitReceipt::with_warnings(
            operation_id,
            Revision::FIRST_COMMIT,
            accepted.warnings().to_vec(),
        );
        assert_eq!(receipt.warnings(), accepted.warnings());
        Ok(())
    }

    #[test]
    fn retired_predicate_never_accepts_a_new_write() -> TestResult<()> {
        let fixture = fixture(Lifecycle::Retired, unconstrained())?;
        let policy = policy(&fixture, true)?;
        assert_eq!(
            validate_assertion_batch(
                vec![draft(&fixture, Value::Int(Int::new(2)))?],
                &fixture.schema,
                &policy,
                fixture.principal,
                true,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            )
            .err(),
            Some(SchemaWriteValidationError::PredicateRetired(
                fixture.predicate
            ))
        );
        Ok(())
    }

    #[test]
    fn all_value_constraint_families_have_positive_and_negative_checks() -> TestResult<()> {
        let within = |value: Value, constraint: ValueConstraint| -> TestResult<()> {
            let fixture = fixture(
                Lifecycle::Active,
                ConstraintSet::new(vec![constraint]).map_err(|error| error.to_string())?,
            )?;
            let policy = policy(&fixture, true)?;
            let result = validate_assertion_batch(
                vec![draft(&fixture, value)?],
                &fixture.schema,
                &policy,
                fixture.principal,
                false,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            );
            if result.is_err() {
                return Err("value expected inside schema constraint was rejected".to_owned());
            }
            Ok(())
        };

        within(
            Value::Bool(true),
            ValueConstraint::BoolSet(
                crate::NonEmptySet::new(vec![true]).map_err(|error| error.to_string())?,
            ),
        )?;
        within(
            Value::UInt(UInt::new(4)),
            ValueConstraint::UIntRange(
                InclusiveRange::new(Some(UInt::new(2)), Some(UInt::new(6)))
                    .map_err(|error| error.to_string())?,
            ),
        )?;
        within(
            Value::Decimal(Decimal::from_str("1.25").map_err(|error| error.to_string())?),
            ValueConstraint::DecimalRange(
                InclusiveRange::new(
                    Some(Decimal::from_str("1").map_err(|error| error.to_string())?),
                    Some(Decimal::from_str("2").map_err(|error| error.to_string())?),
                )
                .map_err(|error| error.to_string())?,
            ),
        )?;
        within(
            Value::String("four".to_owned()),
            ValueConstraint::StringByteLength(
                InclusiveRange::new(Some(UInt::new(4)), Some(UInt::new(4)))
                    .map_err(|error| error.to_string())?,
            ),
        )?;
        within(
            Value::Symbol(Symbol::new("good").map_err(|error| error.to_string())?),
            ValueConstraint::SymbolSet(
                crate::NonEmptySet::new(vec![
                    Symbol::new("good").map_err(|error| error.to_string())?,
                ])
                .map_err(|error| error.to_string())?,
            ),
        )?;
        let time_min = Time::new(
            uuid::<TimelineId>(6).map_err(|error| error.to_string())?,
            1,
            Symbol::new("tick").map_err(|error| error.to_string())?,
        );
        let time_max = Time::new(
            uuid::<TimelineId>(6).map_err(|error| error.to_string())?,
            5,
            Symbol::new("tick").map_err(|error| error.to_string())?,
        );
        within(
            Value::Time(Time::new(
                uuid::<TimelineId>(6).map_err(|error| error.to_string())?,
                3,
                Symbol::new("tick").map_err(|error| error.to_string())?,
            )),
            ValueConstraint::TimeRange(
                TimeRange::new(Some(time_min), Some(time_max))
                    .map_err(|error| error.to_string())?,
            ),
        )?;
        within(
            Value::Duration(crate::Duration::from_nanoseconds(10)),
            ValueConstraint::DurationRange(
                InclusiveRange::new(
                    Some(crate::Duration::from_nanoseconds(5)),
                    Some(crate::Duration::from_nanoseconds(15)),
                )
                .map_err(|error| error.to_string())?,
            ),
        )?;
        within(
            Value::Bytes(Bytes::new(vec![1, 2, 3])),
            ValueConstraint::BytesLength(
                InclusiveRange::new(Some(UInt::new(3)), Some(UInt::new(3)))
                    .map_err(|error| error.to_string())?,
            ),
        )?;

        let int_range = InclusiveRange::new(Some(Int::new(1)), Some(Int::new(5)))
            .map_err(|error| error.to_string())?;
        let int_fixture = fixture(
            Lifecycle::Active,
            ConstraintSet::new(vec![ValueConstraint::IntRange(int_range)])
                .map_err(|error| error.to_string())?,
        )?;
        let policy = policy(&int_fixture, true)?;
        assert!(matches!(
            validate_assertion_batch(
                vec![draft(&int_fixture, Value::Int(Int::new(0)))?],
                &int_fixture.schema,
                &policy,
                int_fixture.principal,
                false,
                DecoderLimits::DEFAULT,
                resolve_same_unit_time,
            ),
            Err(SchemaWriteValidationError::ConstraintViolation { .. })
        ));
        Ok(())
    }
}
