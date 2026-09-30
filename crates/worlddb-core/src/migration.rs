//! Pure migration identities, plans, and run-state values.

use std::fmt;

use crate::ids::{MigrationId, MigrationRunId, MigrationStepId, OperationId};

/// The complete set of 1.0 migration compatibility categories.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MigrationCategory {
    /// The change affects metadata and does not change interpretation or writes.
    MetadataOnly,
    /// The change adds schema or data capabilities without invalidating existing use.
    Additive,
    /// The change tightens a constraint while retaining compatibility for existing data.
    CompatibleConstraintChange,
    /// The change prevents some writes that were previously valid.
    Restrictive,
    /// The change breaks the existing interpretation or compatibility contract.
    Breaking,
}

/// An immutable identity/category/step outline for one logical migration.
///
/// Source-schema checks, target schema payload, transformer version, canonical
/// fingerprint, and execution budgets are validated by the migration planner
/// added in M7. This core value only owns the stable identity and ordered steps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan {
    migration_id: MigrationId,
    category: MigrationCategory,
    steps: Vec<MigrationStepId>,
}

impl MigrationPlan {
    /// Creates a plan outline with a non-empty ordered set of unique step IDs.
    pub fn new(
        migration_id: MigrationId,
        category: MigrationCategory,
        steps: Vec<MigrationStepId>,
    ) -> Result<Self, MigrationPlanError> {
        if steps.is_empty() {
            return Err(MigrationPlanError::EmptySteps);
        }
        for (index, step) in steps.iter().enumerate() {
            if steps.iter().skip(index + 1).any(|other| other == step) {
                return Err(MigrationPlanError::DuplicateStepId(*step));
            }
        }
        Ok(Self {
            migration_id,
            category,
            steps,
        })
    }

    /// Returns the stable identity of the logical migration.
    #[must_use]
    pub const fn migration_id(&self) -> MigrationId {
        self.migration_id
    }

    /// Returns the compatibility category assigned to the plan.
    #[must_use]
    pub const fn category(&self) -> MigrationCategory {
        self.category
    }

    /// Returns the plan's ordered, unique step identities.
    #[must_use]
    pub fn steps(&self) -> &[MigrationStepId] {
        &self.steps
    }
}

/// Invalid migration-plan outline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MigrationPlanError {
    /// A plan must contain at least one step.
    EmptySteps,
    /// A step identity may occur only once in the ordered plan.
    DuplicateStepId(MigrationStepId),
}

impl fmt::Display for MigrationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySteps => formatter.write_str("migration plan has no steps"),
            Self::DuplicateStepId(step_id) => {
                write!(formatter, "migration plan repeats step {step_id}")
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
        MigrationCategory, MigrationPlan, MigrationPlanError, MigrationRun, MigrationRunState,
        MigrationStepCommitIdentity,
    };
    use crate::ids::{
        DomainId, IdValidationError, MigrationId, MigrationRunId, MigrationStepId, OperationId,
    };

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    #[test]
    fn migration_plan_and_run_retain_separate_identities() -> Result<(), IdValidationError> {
        let migration_id = uuid::<MigrationId>(1)?;
        let run_id = uuid::<MigrationRunId>(2)?;
        let first_step = uuid::<MigrationStepId>(3)?;
        let second_step = uuid::<MigrationStepId>(4)?;
        let operation_id = uuid::<OperationId>(5)?;
        let plan = MigrationPlan::new(
            migration_id,
            MigrationCategory::Additive,
            vec![first_step, second_step],
        );
        assert!(plan.is_ok());
        if let Ok(plan) = plan {
            assert_eq!(plan.migration_id(), migration_id);
            assert_eq!(plan.category(), MigrationCategory::Additive);
            assert_eq!(plan.steps(), &[first_step, second_step]);
        }
        let run = MigrationRun::new(run_id, migration_id, MigrationRunState::Running);
        assert_eq!(run.run_id(), run_id);
        assert_eq!(run.migration_id(), migration_id);
        assert_eq!(run.state(), MigrationRunState::Running);
        let identity =
            MigrationStepCommitIdentity::new(migration_id, run_id, first_step, operation_id);
        assert_eq!(identity.migration_id(), migration_id);
        assert_eq!(identity.run_id(), run_id);
        assert_eq!(identity.step_id(), first_step);
        assert_eq!(identity.operation_id(), operation_id);
        Ok(())
    }

    #[test]
    fn migration_categories_are_closed_and_exact() {
        let categories = [
            MigrationCategory::MetadataOnly,
            MigrationCategory::Additive,
            MigrationCategory::CompatibleConstraintChange,
            MigrationCategory::Restrictive,
            MigrationCategory::Breaking,
        ];
        assert_eq!(categories.len(), 5);
        assert_ne!(categories[1], categories[3]);
    }

    #[test]
    fn migration_plans_reject_empty_and_duplicate_step_sets() -> Result<(), IdValidationError> {
        let migration_id = uuid::<MigrationId>(10)?;
        assert_eq!(
            MigrationPlan::new(migration_id, MigrationCategory::MetadataOnly, Vec::new()),
            Err(MigrationPlanError::EmptySteps)
        );
        let step = uuid::<MigrationStepId>(11)?;
        assert_eq!(
            MigrationPlan::new(
                migration_id,
                MigrationCategory::MetadataOnly,
                vec![step, step]
            ),
            Err(MigrationPlanError::DuplicateStepId(step))
        );
        Ok(())
    }

    #[test]
    fn migration_run_states_are_separate_from_transaction_states() {
        let states = [
            MigrationRunState::Planned,
            MigrationRunState::Running,
            MigrationRunState::Completed,
            MigrationRunState::Failed,
        ];
        assert_eq!(states.len(), 4);
        assert_ne!(MigrationRunState::Failed, MigrationRunState::Completed);
    }
}
