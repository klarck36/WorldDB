//! Storage capability declarations and the production durability admission boundary.

use std::fmt;

use crate::ids::Revision;
use crate::revision_backend::{CancellablePublishError, RevisionBackend};

mod sealed {
    /// Prevents downstream crates from implementing the internal storage port.
    pub trait Sealed<T> {}
}

/// Durability level a backend can truthfully provide for an accepted commit.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DurabilityLevel {
    /// Data survives only while the current in-memory process remains alive.
    Memory,
    /// Data survives process termination but not loss of the machine or its storage power.
    Process,
    /// The backend has passed the platform-specific machine-durability profile.
    Machine,
}

/// Closed set of logical and operational storage features.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum StorageFeature {
    /// Reads bind to one committed revision and cannot observe a partial publish.
    ConsistentSnapshots = 0,
    /// A successful publish exposes one complete batch at one new revision.
    AtomicRevisionPublish = 1,
    /// Previously published revisions remain historically readable.
    HistoricalReads = 2,
    /// Active snapshot leases prevent required historical data from reclamation.
    SnapshotPins = 3,
    /// The backend can independently verify its stored representation.
    Verify = 4,
    /// The backend can recover a safe committed prefix after restart.
    Recovery = 5,
}

/// Set of supported storage features. Unknown bits cannot be constructed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StorageFeatureSet(u8);

impl StorageFeatureSet {
    /// Creates an empty feature set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Returns a copy of this set with `feature` included.
    #[must_use]
    pub const fn with(self, feature: StorageFeature) -> Self {
        Self(self.0 | (1_u8 << feature as u8))
    }

    /// Reports whether `feature` is included.
    #[must_use]
    pub const fn contains(self, feature: StorageFeature) -> bool {
        self.0 & (1_u8 << feature as u8) != 0
    }

    /// Reports whether this set contains every feature in `required`.
    #[must_use]
    pub const fn contains_all(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    /// Features expected from a logical reference backend.
    #[must_use]
    pub const fn logical_reference() -> Self {
        Self::empty()
            .with(StorageFeature::ConsistentSnapshots)
            .with(StorageFeature::AtomicRevisionPublish)
            .with(StorageFeature::HistoricalReads)
            .with(StorageFeature::SnapshotPins)
    }

    /// Complete feature set required before production storage is admitted.
    #[must_use]
    pub const fn production_required() -> Self {
        Self::logical_reference()
            .with(StorageFeature::Verify)
            .with(StorageFeature::Recovery)
    }
}

/// Durability and closed feature declaration reported by one backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageCapabilities {
    durability: DurabilityLevel,
    features: StorageFeatureSet,
}

impl StorageCapabilities {
    /// Creates a capability declaration. The backend remains responsible for
    /// supplying evidence that its declaration matches its implementation.
    #[must_use]
    pub const fn new(durability: DurabilityLevel, features: StorageFeatureSet) -> Self {
        Self {
            durability,
            features,
        }
    }

    /// Capabilities of the in-memory logical reference backend.
    #[must_use]
    pub const fn in_memory_reference_model() -> Self {
        Self::new(
            DurabilityLevel::Memory,
            StorageFeatureSet::logical_reference(),
        )
    }

    /// Backend durability level.
    #[must_use]
    pub const fn durability(self) -> DurabilityLevel {
        self.durability
    }

    /// Supported closed feature set.
    #[must_use]
    pub const fn features(self) -> StorageFeatureSet {
        self.features
    }

    /// Checks the full capability contract required for production use.
    pub fn require_production(self) -> Result<(), StorageRequirementError> {
        if self.durability < DurabilityLevel::Machine {
            return Err(StorageRequirementError::MachineDurabilityRequired {
                actual: self.durability,
            });
        }
        for feature in [
            StorageFeature::ConsistentSnapshots,
            StorageFeature::AtomicRevisionPublish,
            StorageFeature::HistoricalReads,
            StorageFeature::SnapshotPins,
            StorageFeature::Verify,
            StorageFeature::Recovery,
        ] {
            if !self.features.contains(feature) {
                return Err(StorageRequirementError::MissingFeature(feature));
            }
        }
        Ok(())
    }
}

/// Storage contract implemented by first-party backend adapters.
///
/// The revision methods provide committed snapshots, atomic publication, and
/// historical reads. Capabilities also declare durability, pin, verification,
/// and recovery support. A capability declaration is not platform evidence;
/// production admission still requires the corresponding implementation tests.
/// The port is sealed so a third-party adapter cannot claim capabilities
/// without the repository's implementation and verification policy.
///
/// ```compile_fail
/// use worlddb_core::{StorageBackend, StorageCapabilities};
/// struct ExternalBackend;
/// impl StorageBackend<u8> for ExternalBackend {
///     fn storage_capabilities(&self) -> StorageCapabilities {
///         StorageCapabilities::in_memory_reference_model()
///     }
/// }
/// ```
pub trait StorageBackend<T>: RevisionBackend<T> + sealed::Sealed<T> {
    /// Returns the capabilities verified for this backend instance.
    fn storage_capabilities(&self) -> StorageCapabilities;
}

/// Backend admitted through the production capability and durability check.
///
/// Production engine construction should accept this wrapper rather than a
/// raw `RevisionBackend`. Model backends remain usable directly by engine
/// contract tests but cannot pass this wrapper without Machine durability.
pub struct ProductionStorage<B> {
    backend: B,
    capabilities: StorageCapabilities,
}

impl<B> ProductionStorage<B> {
    /// Admits a backend only when it declares the complete production contract.
    pub fn try_new<T>(backend: B) -> Result<Self, StorageRequirementError>
    where
        B: StorageBackend<T>,
    {
        let capabilities = backend.storage_capabilities();
        capabilities.require_production()?;
        Ok(Self {
            backend,
            capabilities,
        })
    }

    /// Capabilities checked when this production handle was created.
    #[must_use]
    pub const fn storage_capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }
}

impl<T, B> RevisionBackend<T> for ProductionStorage<B>
where
    B: StorageBackend<T>,
{
    type Read<'a>
        = B::Read<'a>
    where
        Self: 'a,
        T: 'a;

    fn latest_published(&self) -> Revision {
        self.backend.latest_published()
    }

    fn publish(&mut self, entries: Vec<T>) -> Result<Revision, crate::RevisionLogError> {
        self.backend.publish(entries)
    }

    fn publish_cancellable(
        &mut self,
        entries: Vec<T>,
        cancellation: &crate::CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        self.backend.publish_cancellable(entries, cancellation)
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, crate::RevisionLogError> {
        self.backend.read_at(revision)
    }
}

impl<T, B> StorageBackend<T> for ProductionStorage<B>
where
    B: StorageBackend<T>,
{
    fn storage_capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }
}

impl<T, B> sealed::Sealed<T> for ProductionStorage<B> where B: StorageBackend<T> {}

impl<T> sealed::Sealed<T> for crate::InMemoryRevisionBackend<T> {}

impl<T> StorageBackend<T> for crate::InMemoryRevisionBackend<T> {
    fn storage_capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::in_memory_reference_model()
    }
}

/// Why a backend could not be admitted for production writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageRequirementError {
    /// A production commit requires a platform-verified Machine durability profile.
    MachineDurabilityRequired { actual: DurabilityLevel },
    /// A required storage operation is not supported by this backend.
    MissingFeature(StorageFeature),
}

impl fmt::Display for StorageRequirementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MachineDurabilityRequired { actual } => write!(
                formatter,
                "production storage requires Machine durability; backend reports {actual:?}"
            ),
            Self::MissingFeature(feature) => {
                write!(
                    formatter,
                    "production storage lacks required feature {feature:?}"
                )
            }
        }
    }
}

impl std::error::Error for StorageRequirementError {}

#[cfg(test)]
mod tests {
    use super::{DurabilityLevel, StorageCapabilities, StorageFeature, StorageFeatureSet};
    use crate::revision_backend::InMemoryRevisionBackend;
    use crate::storage_contract::{ProductionStorage, StorageBackend, StorageRequirementError};

    #[test]
    fn in_memory_profile_supports_logical_contract_but_not_machine_durability() {
        let backend = InMemoryRevisionBackend::<u8>::new();
        let capabilities = backend.storage_capabilities();
        assert_eq!(capabilities.durability(), DurabilityLevel::Memory);
        assert!(
            capabilities
                .features()
                .contains_all(StorageFeatureSet::logical_reference())
        );
        assert!(!capabilities.features().contains(StorageFeature::Verify));
        assert!(!capabilities.features().contains(StorageFeature::Recovery));
        assert_eq!(
            ProductionStorage::try_new::<u8>(backend).err(),
            Some(StorageRequirementError::MachineDurabilityRequired {
                actual: DurabilityLevel::Memory,
            })
        );
    }

    #[test]
    fn machine_durability_alone_does_not_skip_other_required_features() {
        let capabilities = StorageCapabilities::new(
            DurabilityLevel::Machine,
            StorageFeatureSet::logical_reference().with(StorageFeature::Recovery),
        );
        assert_eq!(
            capabilities.require_production(),
            Err(StorageRequirementError::MissingFeature(
                StorageFeature::Verify
            ))
        );
    }

    #[test]
    fn process_durability_is_not_enough_for_production_commits() {
        let capabilities = StorageCapabilities::new(
            DurabilityLevel::Process,
            StorageFeatureSet::production_required(),
        );
        assert_eq!(
            capabilities.require_production(),
            Err(StorageRequirementError::MachineDurabilityRequired {
                actual: DurabilityLevel::Process,
            })
        );
    }

    #[test]
    fn complete_machine_profile_satisfies_the_admission_predicate() {
        let capabilities = StorageCapabilities::new(
            DurabilityLevel::Machine,
            StorageFeatureSet::production_required(),
        );
        assert_eq!(capabilities.require_production(), Ok(()));
    }
}
