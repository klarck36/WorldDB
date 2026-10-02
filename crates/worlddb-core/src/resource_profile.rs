//! Versioned process resource defaults and cooperative memory admission.
//!
//! The memory ledger tracks only allocations that a component explicitly reserves.
//! It is not a platform RSS limit; decoders, queries, and index builders still need
//! their operation-specific finite limits.

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

/// The current process resource profile format.
pub const RESOURCE_PROFILE_VERSION_V1: u16 = 1;
/// Absolute ceiling accepted for a configured process hard memory budget.
pub const MAX_PROCESS_HARD_MEMORY_BYTES: u64 = 16 * 1024 * 1024 * 1024;

const MIB: u64 = 1024 * 1024;

static PROCESS_MEMORY_BUDGET: OnceLock<ProcessMemoryBudget> = OnceLock::new();

/// Returns the shared process ledger, installing the approved defaults on first use.
///
/// Applications that need custom process limits must call
/// [`configure_process_resource_profile`] before constructing a query context or
/// building a tracked index.
#[must_use]
pub fn process_memory_budget() -> &'static ProcessMemoryBudget {
    PROCESS_MEMORY_BUDGET.get_or_init(|| ProcessResourceProfile::DEFAULT.memory_budget())
}

/// Installs one versioned process profile before any tracked allocation starts.
///
/// The returned ledger is shared by query contexts and tracked index generations.
/// Configuration is immutable after first use so active reservations cannot be
/// reinterpreted under different limits.
pub fn configure_process_resource_profile(
    profile: ProcessResourceProfile,
) -> Result<&'static ProcessMemoryBudget, ResourceBudgetError> {
    PROCESS_MEMORY_BUDGET
        .set(profile.memory_budget())
        .map_err(|_| ResourceBudgetError::ProcessProfileAlreadyInitialized)?;
    PROCESS_MEMORY_BUDGET
        .get()
        .ok_or(ResourceBudgetError::LedgerUnavailable)
}

pub(crate) fn reserve_index_memory_with_budget(
    budget: &ProcessMemoryBudget,
    bytes: u64,
) -> Result<MemoryReservation, ResourceBudgetError> {
    budget.reserve(ResourceClass::Index, bytes)
}

pub(crate) fn estimated_index_memory_bytes(
    entries: usize,
    bytes_per_entry: u64,
) -> Result<u64, ResourceBudgetError> {
    u64::try_from(entries)
        .map_err(|_| ResourceBudgetError::ArithmeticOverflow)?
        .checked_mul(bytes_per_entry)
        .ok_or(ResourceBudgetError::ArithmeticOverflow)
}

/// Independently accounted memory user under one process profile.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResourceClass {
    Query,
    Parser,
    Index,
    Job,
    Other,
}

/// Finite reservation ceilings assigned to each resource class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceClassLimits {
    query_memory_bytes: u64,
    parser_memory_bytes: u64,
    index_memory_bytes: u64,
    job_memory_bytes: u64,
    other_memory_bytes: u64,
}

impl ResourceClassLimits {
    /// Creates a finite limit for each process resource class.
    #[must_use]
    pub const fn new(
        query_memory_bytes: u64,
        parser_memory_bytes: u64,
        index_memory_bytes: u64,
        job_memory_bytes: u64,
        other_memory_bytes: u64,
    ) -> Self {
        Self {
            query_memory_bytes,
            parser_memory_bytes,
            index_memory_bytes,
            job_memory_bytes,
            other_memory_bytes,
        }
    }

    /// Initial profile's per-class limits: Query 256 MiB, Parser 256 MiB,
    /// Index 512 MiB, Job 256 MiB, and Other 256 MiB.
    pub const DEFAULT: Self = Self::new(256 * MIB, 256 * MIB, 512 * MIB, 256 * MIB, 256 * MIB);

    const fn limit_bytes(self, class: ResourceClass) -> u64 {
        match class {
            ResourceClass::Query => self.query_memory_bytes,
            ResourceClass::Parser => self.parser_memory_bytes,
            ResourceClass::Index => self.index_memory_bytes,
            ResourceClass::Job => self.job_memory_bytes,
            ResourceClass::Other => self.other_memory_bytes,
        }
    }
}

/// Versioned process defaults and finite per-component memory reservation limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessResourceProfile {
    version: u16,
    soft_memory_bytes: u64,
    hard_memory_bytes: u64,
    class_limits: ResourceClassLimits,
}

impl ProcessResourceProfile {
    /// The initial 1.0 profile: 512 MiB soft and 1 GiB hard.
    pub const DEFAULT: Self = Self {
        version: RESOURCE_PROFILE_VERSION_V1,
        soft_memory_bytes: 512 * MIB,
        hard_memory_bytes: 1024 * MIB,
        class_limits: ResourceClassLimits::DEFAULT,
    };

    /// Creates a configurable profile with finite, positive memory limits.
    pub fn new(
        version: u16,
        soft_memory_bytes: u64,
        hard_memory_bytes: u64,
        class_limits: ResourceClassLimits,
    ) -> Result<Self, ResourceBudgetError> {
        if version != RESOURCE_PROFILE_VERSION_V1 {
            return Err(ResourceBudgetError::UnsupportedProfileVersion(version));
        }
        if soft_memory_bytes == 0 {
            return Err(ResourceBudgetError::ZeroSoftMemoryLimit);
        }
        if hard_memory_bytes == 0 {
            return Err(ResourceBudgetError::ZeroHardMemoryLimit);
        }
        if hard_memory_bytes > MAX_PROCESS_HARD_MEMORY_BYTES {
            return Err(ResourceBudgetError::HardMemoryLimitAboveAbsoluteMaximum);
        }
        if soft_memory_bytes > hard_memory_bytes {
            return Err(ResourceBudgetError::SoftMemoryLimitExceedsHardLimit);
        }

        for class in [
            ResourceClass::Query,
            ResourceClass::Parser,
            ResourceClass::Index,
            ResourceClass::Job,
            ResourceClass::Other,
        ] {
            let limit = class_limits.limit_bytes(class);
            if limit == 0 {
                return Err(ResourceBudgetError::ZeroClassLimit(class));
            }
            if limit > hard_memory_bytes {
                return Err(ResourceBudgetError::ClassLimitExceedsHardLimit(class));
            }
        }

        Ok(Self {
            version,
            soft_memory_bytes,
            hard_memory_bytes,
            class_limits,
        })
    }

    /// Stable profile format version.
    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    /// Advisory process soft memory threshold.
    #[must_use]
    pub const fn soft_memory_bytes(self) -> u64 {
        self.soft_memory_bytes
    }

    /// Hard ceiling for explicitly admitted memory reservations.
    #[must_use]
    pub const fn hard_memory_bytes(self) -> u64 {
        self.hard_memory_bytes
    }

    /// Maximum explicitly reserved bytes for one component class.
    #[must_use]
    pub const fn class_limit_bytes(self, class: ResourceClass) -> u64 {
        self.class_limits.limit_bytes(class)
    }

    /// Creates a shared ledger that applies these limits across its clones.
    #[must_use]
    pub fn memory_budget(self) -> ProcessMemoryBudget {
        ProcessMemoryBudget {
            profile: self,
            usage: Arc::new(Mutex::new(MemoryUsage::default())),
        }
    }
}

impl Default for ProcessResourceProfile {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Shared hard admission ledger for explicitly tracked process allocations.
#[derive(Clone, Debug)]
pub struct ProcessMemoryBudget {
    profile: ProcessResourceProfile,
    usage: Arc<Mutex<MemoryUsage>>,
}

impl ProcessMemoryBudget {
    /// Profile governing this shared ledger.
    #[must_use]
    pub const fn profile(&self) -> ProcessResourceProfile {
        self.profile
    }

    /// Reserves bytes before a bounded allocation and fails before crossing a hard limit.
    pub fn reserve(
        &self,
        class: ResourceClass,
        bytes: u64,
    ) -> Result<MemoryReservation, ResourceBudgetError> {
        let mut usage = self
            .usage
            .lock()
            .map_err(|_| ResourceBudgetError::LedgerUnavailable)?;
        let class_total = usage
            .class_bytes(class)
            .checked_add(bytes)
            .ok_or(ResourceBudgetError::ArithmeticOverflow)?;
        if class_total > self.profile.class_limit_bytes(class) {
            return Err(ResourceBudgetError::ClassBudgetExceeded {
                class,
                requested_total: class_total,
                limit: self.profile.class_limit_bytes(class),
            });
        }
        let process_total = usage
            .total_bytes
            .checked_add(bytes)
            .ok_or(ResourceBudgetError::ArithmeticOverflow)?;
        if process_total > self.profile.hard_memory_bytes {
            return Err(ResourceBudgetError::ProcessBudgetExceeded {
                requested_total: process_total,
                limit: self.profile.hard_memory_bytes,
            });
        }

        usage.total_bytes = process_total;
        usage.set_class_bytes(class, class_total);
        let soft_limit_exceeded = process_total > self.profile.soft_memory_bytes;
        drop(usage);

        Ok(MemoryReservation {
            inner: Arc::new(MemoryReservationInner {
                budget: self.clone(),
                class,
                bytes,
                soft_limit_exceeded,
            }),
        })
    }

    /// Current explicitly reserved bytes across the shared ledger.
    pub fn reserved_bytes(&self) -> Result<u64, ResourceBudgetError> {
        self.usage
            .lock()
            .map(|usage| usage.total_bytes)
            .map_err(|_| ResourceBudgetError::LedgerUnavailable)
    }

    /// Whether tracked reservations currently exceed the advisory soft threshold.
    pub fn soft_limit_exceeded(&self) -> Result<bool, ResourceBudgetError> {
        Ok(self.reserved_bytes()? > self.profile.soft_memory_bytes)
    }
}

#[derive(Debug, Default)]
struct MemoryUsage {
    total_bytes: u64,
    query_bytes: u64,
    parser_bytes: u64,
    index_bytes: u64,
    job_bytes: u64,
    other_bytes: u64,
}

impl MemoryUsage {
    const fn class_bytes(&self, class: ResourceClass) -> u64 {
        match class {
            ResourceClass::Query => self.query_bytes,
            ResourceClass::Parser => self.parser_bytes,
            ResourceClass::Index => self.index_bytes,
            ResourceClass::Job => self.job_bytes,
            ResourceClass::Other => self.other_bytes,
        }
    }

    fn set_class_bytes(&mut self, class: ResourceClass, bytes: u64) {
        match class {
            ResourceClass::Query => self.query_bytes = bytes,
            ResourceClass::Parser => self.parser_bytes = bytes,
            ResourceClass::Index => self.index_bytes = bytes,
            ResourceClass::Job => self.job_bytes = bytes,
            ResourceClass::Other => self.other_bytes = bytes,
        }
    }
}

/// RAII token that releases its admitted memory when the last clone is dropped.
#[derive(Clone, Debug)]
pub struct MemoryReservation {
    inner: Arc<MemoryReservationInner>,
}

impl MemoryReservation {
    /// Number of bytes held by this reservation.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.inner.bytes
    }

    /// Whether acquiring this reservation crossed the soft threshold.
    #[must_use]
    pub fn soft_limit_exceeded_at_acquisition(&self) -> bool {
        self.inner.soft_limit_exceeded
    }
}

#[derive(Debug)]
struct MemoryReservationInner {
    budget: ProcessMemoryBudget,
    class: ResourceClass,
    bytes: u64,
    soft_limit_exceeded: bool,
}

impl Drop for MemoryReservationInner {
    fn drop(&mut self) {
        let mut usage = match self.budget.usage.lock() {
            Ok(usage) => usage,
            Err(poisoned) => poisoned.into_inner(),
        };
        debug_assert!(usage.total_bytes >= self.bytes);
        debug_assert!(usage.class_bytes(self.class) >= self.bytes);
        let class_bytes = usage.class_bytes(self.class).saturating_sub(self.bytes);
        usage.total_bytes = usage.total_bytes.saturating_sub(self.bytes);
        usage.set_class_bytes(self.class, class_bytes);
    }
}

/// Invalid profile or rejected memory admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceBudgetError {
    UnsupportedProfileVersion(u16),
    ZeroSoftMemoryLimit,
    ZeroHardMemoryLimit,
    HardMemoryLimitAboveAbsoluteMaximum,
    SoftMemoryLimitExceedsHardLimit,
    ZeroClassLimit(ResourceClass),
    ClassLimitExceedsHardLimit(ResourceClass),
    ClassBudgetExceeded {
        class: ResourceClass,
        requested_total: u64,
        limit: u64,
    },
    ProcessBudgetExceeded {
        requested_total: u64,
        limit: u64,
    },
    ArithmeticOverflow,
    LedgerUnavailable,
    ProcessProfileAlreadyInitialized,
}

impl fmt::Display for ResourceBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProfileVersion(version) => {
                write!(
                    formatter,
                    "unsupported process resource profile version {version}"
                )
            }
            Self::ZeroSoftMemoryLimit => formatter.write_str("soft memory limit must be positive"),
            Self::ZeroHardMemoryLimit => formatter.write_str("hard memory limit must be positive"),
            Self::HardMemoryLimitAboveAbsoluteMaximum => {
                formatter.write_str("hard memory limit exceeds the absolute maximum")
            }
            Self::SoftMemoryLimitExceedsHardLimit => {
                formatter.write_str("soft memory limit exceeds the hard limit")
            }
            Self::ZeroClassLimit(class) => {
                write!(formatter, "{class:?} memory limit must be positive")
            }
            Self::ClassLimitExceedsHardLimit(class) => {
                write!(
                    formatter,
                    "{class:?} memory limit exceeds the process hard limit"
                )
            }
            Self::ClassBudgetExceeded { class, .. } => {
                write!(formatter, "{class:?} memory budget exceeded")
            }
            Self::ProcessBudgetExceeded { .. } => {
                formatter.write_str("process memory budget exceeded")
            }
            Self::ArithmeticOverflow => formatter.write_str("memory budget arithmetic overflowed"),
            Self::LedgerUnavailable => formatter.write_str("process memory budget is unavailable"),
            Self::ProcessProfileAlreadyInitialized => formatter.write_str(
                "process resource profile was already initialized and cannot be changed",
            ),
        }
    }
}

impl std::error::Error for ResourceBudgetError {}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROCESS_HARD_MEMORY_BYTES, ProcessResourceProfile, RESOURCE_PROFILE_VERSION_V1,
        ResourceBudgetError, ResourceClass, ResourceClassLimits,
    };

    fn limits(query: u64, parser: u64, index: u64, job: u64, other: u64) -> ResourceClassLimits {
        ResourceClassLimits::new(query, parser, index, job, other)
    }

    #[test]
    fn default_profile_is_versioned_and_matches_the_approved_process_thresholds() {
        let profile = ProcessResourceProfile::default();
        assert_eq!(profile.version(), RESOURCE_PROFILE_VERSION_V1);
        assert_eq!(profile.soft_memory_bytes(), 512 * 1024 * 1024);
        assert_eq!(profile.hard_memory_bytes(), 1024 * 1024 * 1024);
        assert_eq!(
            profile.class_limit_bytes(ResourceClass::Query),
            256 * 1024 * 1024
        );
        assert_eq!(
            profile.class_limit_bytes(ResourceClass::Parser),
            256 * 1024 * 1024
        );
        assert_eq!(
            profile.class_limit_bytes(ResourceClass::Index),
            512 * 1024 * 1024
        );
    }

    #[test]
    fn profile_rejects_unknown_versions_unbounded_values_and_inconsistent_limits() {
        assert_eq!(
            ProcessResourceProfile::new(2, 8, 16, limits(8, 8, 8, 8, 8)),
            Err(ResourceBudgetError::UnsupportedProfileVersion(2))
        );
        assert_eq!(
            ProcessResourceProfile::new(1, 17, 16, limits(8, 8, 8, 8, 8)),
            Err(ResourceBudgetError::SoftMemoryLimitExceedsHardLimit)
        );
        assert_eq!(
            ProcessResourceProfile::new(1, 8, 16, limits(17, 8, 8, 8, 8)),
            Err(ResourceBudgetError::ClassLimitExceedsHardLimit(
                ResourceClass::Query
            ))
        );
        assert_eq!(
            ProcessResourceProfile::new(
                1,
                8,
                MAX_PROCESS_HARD_MEMORY_BYTES + 1,
                limits(8, 8, 8, 8, 8),
            ),
            Err(ResourceBudgetError::HardMemoryLimitAboveAbsoluteMaximum)
        );
    }

    #[test]
    fn shared_memory_ledger_reports_soft_pressure_and_rejects_hard_overcommit() {
        let profile = ProcessResourceProfile::new(1, 8, 16, limits(16, 16, 16, 16, 16));
        assert!(profile.is_ok());
        let Ok(profile) = profile else {
            return;
        };
        let budget = profile.memory_budget();
        let first = budget.reserve(ResourceClass::Query, 6);
        assert!(first.is_ok());
        let Ok(first) = first else {
            return;
        };
        assert!(!first.soft_limit_exceeded_at_acquisition());

        let second = budget.reserve(ResourceClass::Parser, 3);
        assert!(second.is_ok());
        let Ok(second) = second else {
            return;
        };
        assert!(second.soft_limit_exceeded_at_acquisition());
        assert_eq!(budget.reserved_bytes(), Ok(9));
        assert_eq!(
            budget.reserve(ResourceClass::Index, 8).err(),
            Some(ResourceBudgetError::ProcessBudgetExceeded {
                requested_total: 17,
                limit: 16,
            })
        );
        drop(second);
        assert_eq!(budget.reserved_bytes(), Ok(6));
        drop(first);
        assert_eq!(budget.reserved_bytes(), Ok(0));
    }

    #[test]
    fn shared_memory_ledger_enforces_component_limits() {
        let profile = ProcessResourceProfile::new(1, 8, 16, limits(4, 16, 16, 16, 16));
        assert!(profile.is_ok());
        let Ok(profile) = profile else {
            return;
        };
        let budget = profile.memory_budget();
        assert_eq!(
            budget.reserve(ResourceClass::Query, 5).err(),
            Some(ResourceBudgetError::ClassBudgetExceeded {
                class: ResourceClass::Query,
                requested_total: 5,
                limit: 4,
            })
        );
    }
}
