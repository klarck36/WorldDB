//! Immutable snapshot bindings and explicit reader leases.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::catalog::HistorySpaceDefinition;
use crate::errors::QueryError;
use crate::ids::{DatabaseId, HistorySpaceId, PrincipalId, Revision, SecurityEpoch, SnapshotId};
use crate::layers::{LayerSchemaSnapshot, LayerSelection};
use crate::query_context::AuthorizationMode;
use crate::record_refs::SnapshotRef;
use crate::reference_query::HistoricalQueryBinding;
use crate::temporal::RecordedAsOf;

/// Historical HistorySpace ancestry resolved for one snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySpaceView {
    selected: HistorySpaceId,
    ancestry: Vec<HistorySpaceDefinition>,
}

impl HistorySpaceView {
    /// Creates a validated root-to-selected chain with immutable parent cutoffs.
    pub fn new(ancestry: Vec<HistorySpaceDefinition>) -> Result<Self, SnapshotError> {
        let Some(root) = ancestry.first() else {
            return Err(SnapshotError::InvalidHistorySpaceView);
        };
        let selected = ancestry
            .last()
            .map(|definition| definition.history_space_id())
            .ok_or(SnapshotError::InvalidHistorySpaceView)?;
        if root.parent_history_space_id().is_some()
            || root.base_revision() != Revision::GENESIS
            || ancestry
                .windows(2)
                .any(|pair| match (pair.first(), pair.get(1)) {
                    (Some(parent), Some(child)) => {
                        child.parent_history_space_id() != Some(parent.history_space_id())
                    }
                    _ => true,
                })
        {
            return Err(SnapshotError::InvalidHistorySpaceView);
        }
        let unique: BTreeSet<_> = ancestry
            .iter()
            .map(|definition| definition.history_space_id())
            .collect();
        if unique.len() != ancestry.len() {
            return Err(SnapshotError::InvalidHistorySpaceView);
        }
        Ok(Self { selected, ancestry })
    }

    /// Selected HistorySpace.
    #[must_use]
    pub const fn selected(&self) -> HistorySpaceId {
        self.selected
    }

    /// Root-to-selected HistorySpace ancestry captured at pin time.
    #[must_use]
    pub fn ancestry(&self) -> &[HistorySpaceDefinition] {
        &self.ancestry
    }
}

/// Authorization evaluation inputs frozen into the snapshot binding.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotSecurityBinding {
    principal_id: PrincipalId,
    authorization_mode: AuthorizationMode,
    security_epoch: SecurityEpoch,
}

impl SnapshotSecurityBinding {
    /// Captures principal, policy time basis and evaluated SecurityEpoch.
    #[must_use]
    pub const fn new(
        principal_id: PrincipalId,
        authorization_mode: AuthorizationMode,
        security_epoch: SecurityEpoch,
    ) -> Self {
        Self {
            principal_id,
            authorization_mode,
            security_epoch,
        }
    }

    /// Authenticated principal bound to the evaluation.
    #[must_use]
    pub const fn principal_id(self) -> PrincipalId {
        self.principal_id
    }

    /// Explicit authorization time basis.
    #[must_use]
    pub const fn authorization_mode(self) -> AuthorizationMode {
        self.authorization_mode
    }

    /// Security policy epoch evaluated for this binding.
    #[must_use]
    pub const fn security_epoch(self) -> SecurityEpoch {
        self.security_epoch
    }
}

/// All semantic and storage axes pinned by one snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotBinding {
    database_id: DatabaseId,
    snapshot: SnapshotRef,
    data_revision: Revision,
    recorded_as_of: RecordedAsOf,
    schema: HistoricalQueryBinding,
    history_space: HistorySpaceView,
    layer_schema: LayerSchemaSnapshot,
    layer_selection: LayerSelection,
    security: SnapshotSecurityBinding,
    backend_generation: u64,
}

/// Complete input required to create one coherent snapshot binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotBindingInput {
    /// Database whose immutable history is being read.
    pub database_id: DatabaseId,
    /// Unique session-local snapshot identity.
    pub snapshot_id: SnapshotId,
    /// Published data revision fixed by the snapshot.
    pub data_revision: Revision,
    /// Explicit transaction-time read point.
    pub recorded_as_of: RecordedAsOf,
    /// Schema mode, revision and fingerprint captured at the read point.
    pub schema: HistoricalQueryBinding,
    /// Selected HistorySpace plus its immutable ancestry/cutoffs.
    pub history_space: HistorySpaceView,
    /// Historical layer definitions at the bound schema revision.
    pub layer_schema: LayerSchemaSnapshot,
    /// Requested layer set validated against `layer_schema`.
    pub layer_selection: LayerSelection,
    /// Principal and security evaluation epoch bound to the query.
    pub security: SnapshotSecurityBinding,
    /// Backend generation whose segments this snapshot protects.
    pub backend_generation: u64,
}

impl SnapshotBinding {
    /// Builds and cross-validates the immutable data/schema/query/storage binding.
    pub fn new(input: SnapshotBindingInput) -> Result<Self, SnapshotError> {
        let SnapshotBindingInput {
            database_id,
            snapshot_id,
            data_revision,
            recorded_as_of,
            schema,
            history_space,
            layer_schema,
            layer_selection,
            security,
            backend_generation,
        } = input;
        if recorded_as_of.revision() > data_revision
            || schema.recorded_as_of() != recorded_as_of
            || schema.schema_revision() != layer_schema.revision()
            || history_space
                .ancestry()
                .iter()
                .any(|space| space.base_revision() > data_revision)
        {
            return Err(SnapshotError::InconsistentBinding);
        }
        layer_schema
            .resolve(&layer_selection)
            .map_err(|_| SnapshotError::InvalidLayerSelection)?;
        if let AuthorizationMode::AtRevision(revision) = security.authorization_mode() {
            if revision > data_revision {
                return Err(SnapshotError::InconsistentBinding);
            }
        }
        Ok(Self {
            database_id,
            snapshot: SnapshotRef::new(snapshot_id),
            data_revision,
            recorded_as_of,
            schema,
            history_space,
            layer_schema,
            layer_selection,
            security,
            backend_generation,
        })
    }

    /// Database that owns this immutable snapshot.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Session-local snapshot handle.
    #[must_use]
    pub const fn snapshot(&self) -> SnapshotRef {
        self.snapshot
    }

    /// Published data revision fixed at pin time.
    #[must_use]
    pub const fn data_revision(&self) -> Revision {
        self.data_revision
    }

    /// Explicit transaction-time read point.
    #[must_use]
    pub const fn recorded_as_of(&self) -> RecordedAsOf {
        self.recorded_as_of
    }

    /// Immutable schema mode, revision and fingerprint.
    #[must_use]
    pub const fn schema(&self) -> HistoricalQueryBinding {
        self.schema
    }

    /// HistorySpace and ancestry captured at snapshot construction.
    #[must_use]
    pub const fn history_space(&self) -> &HistorySpaceView {
        &self.history_space
    }

    /// Historical layer definitions used to validate layer selection.
    #[must_use]
    pub const fn layer_schema(&self) -> &LayerSchemaSnapshot {
        &self.layer_schema
    }

    /// Exact requested layer selection, preserved with its resolved schema.
    #[must_use]
    pub const fn layer_selection(&self) -> &LayerSelection {
        &self.layer_selection
    }

    /// Security evaluation binding frozen at construction.
    #[must_use]
    pub const fn security(&self) -> SnapshotSecurityBinding {
        self.security
    }

    /// Backend generation whose segments are held by this snapshot.
    #[must_use]
    pub const fn backend_generation(&self) -> u64 {
        self.backend_generation
    }
}

/// Configured lifetimes for ordinary and administrative snapshot pins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotLifetimeLimits {
    soft_ms: u64,
    hard_ms: u64,
    exact_backup_max_ms: u64,
}

impl SnapshotLifetimeLimits {
    /// Creates positive limits; exact-backup limit may exceed the interactive hard limit.
    pub fn new(
        soft_ms: u64,
        hard_ms: u64,
        exact_backup_max_ms: u64,
    ) -> Result<Self, SnapshotError> {
        if soft_ms == 0 || hard_ms <= soft_ms || exact_backup_max_ms < hard_ms {
            return Err(SnapshotError::InvalidLifetimeLimits);
        }
        Ok(Self {
            soft_ms,
            hard_ms,
            exact_backup_max_ms,
        })
    }
}

/// Purpose determines the hard lifetime applied to one explicit pin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPinPurpose {
    /// Ordinary query or interactive transaction, bounded by `hard_ms`.
    Interactive,
    /// Administrative exact backup with an explicitly selected duration.
    AdminExactBackup { lifetime_ms: u64 },
}

/// Current state relative to one pin's monotonic lifetime budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotLifetimeStatus {
    /// The pin is below its soft-warning threshold.
    WithinBudget,
    /// The pin crossed the soft threshold but remains valid.
    SoftLimitExceeded { age_ms: u64, soft_limit_ms: u64 },
    /// The pin reached its hard lifetime; reads and commit must stop.
    Expired { age_ms: u64, hard_limit_ms: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PinBudget {
    hard_ms: u64,
}

#[derive(Debug)]
struct ActivePin {
    started_at_ms: u64,
    budget: PinBudget,
}

#[derive(Debug)]
struct SnapshotEntry {
    binding: Arc<SnapshotBinding>,
    pins: BTreeMap<u64, ActivePin>,
}

#[derive(Debug, Default)]
struct RegistryState {
    next_lease_id: u64,
    seen_snapshots: BTreeSet<SnapshotId>,
    entries: BTreeMap<SnapshotId, SnapshotEntry>,
}

/// Shared in-memory pin registry used by readers and reclamation checks.
#[derive(Clone, Debug)]
pub struct SnapshotRegistry {
    state: Arc<Mutex<RegistryState>>,
    limits: SnapshotLifetimeLimits,
}

impl SnapshotRegistry {
    /// Creates an empty registry with finite interactive and backup limits.
    #[must_use]
    pub fn new(limits: SnapshotLifetimeLimits) -> Self {
        Self {
            state: Arc::new(Mutex::new(RegistryState::default())),
            limits,
        }
    }

    /// Registers a unique snapshot and returns its first explicit pin.
    pub fn pin(
        &self,
        binding: SnapshotBinding,
        purpose: SnapshotPinPurpose,
        now_ms: u64,
    ) -> Result<SnapshotLease, SnapshotError> {
        let budget = self.budget(purpose)?;
        let snapshot_id = binding.snapshot().id();
        let mut state = self
            .state
            .lock()
            .map_err(|_| SnapshotError::RegistryPoisoned)?;
        if !state.seen_snapshots.insert(snapshot_id) {
            return Err(SnapshotError::SnapshotIdReused);
        }
        let lease_id = next_lease_id(&mut state)?;
        let mut pins = BTreeMap::new();
        pins.insert(
            lease_id,
            ActivePin {
                started_at_ms: now_ms,
                budget,
            },
        );
        state.entries.insert(
            snapshot_id,
            SnapshotEntry {
                binding: Arc::new(binding),
                pins,
            },
        );
        Ok(SnapshotLease {
            registry: self.clone(),
            snapshot_id,
            lease_id,
            soft_ms: self.limits.soft_ms,
            released: false,
        })
    }

    fn budget(&self, purpose: SnapshotPinPurpose) -> Result<PinBudget, SnapshotError> {
        match purpose {
            SnapshotPinPurpose::Interactive => Ok(PinBudget {
                hard_ms: self.limits.hard_ms,
            }),
            SnapshotPinPurpose::AdminExactBackup { lifetime_ms }
                if lifetime_ms > 0 && lifetime_ms <= self.limits.exact_backup_max_ms =>
            {
                Ok(PinBudget {
                    hard_ms: lifetime_ms,
                })
            }
            SnapshotPinPurpose::AdminExactBackup { .. } => {
                Err(SnapshotError::InvalidBackupLifetime)
            }
        }
    }

    /// Returns whether any live lease still pins this backend generation.
    pub fn generation_is_pinned(&self, generation: u64) -> Result<bool, SnapshotError> {
        let state = self
            .state
            .lock()
            .map_err(|_| SnapshotError::RegistryPoisoned)?;
        Ok(state.entries.values().any(|entry| {
            entry.binding.backend_generation() == generation && !entry.pins.is_empty()
        }))
    }

    /// Number of active reader/backup leases for one snapshot.
    pub fn active_leases(&self, snapshot_id: SnapshotId) -> Result<usize, SnapshotError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| SnapshotError::RegistryPoisoned)?
            .entries
            .get(&snapshot_id)
            .map_or(0, |entry| entry.pins.len()))
    }

    fn fork(&self, parent: &SnapshotLease, now_ms: u64) -> Result<SnapshotLease, SnapshotError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SnapshotError::RegistryPoisoned)?;
        let parent_pin = state
            .entries
            .get(&parent.snapshot_id)
            .and_then(|entry| entry.pins.get(&parent.lease_id))
            .ok_or(SnapshotError::LeaseReleased)?;
        if now_ms.saturating_sub(parent_pin.started_at_ms) >= parent_pin.budget.hard_ms {
            return Err(SnapshotError::Expired);
        }
        let budget = parent_pin.budget;
        let lease_id = next_lease_id(&mut state)?;
        let entry = state
            .entries
            .get_mut(&parent.snapshot_id)
            .ok_or(SnapshotError::LeaseReleased)?;
        entry.pins.insert(
            lease_id,
            ActivePin {
                started_at_ms: now_ms,
                budget,
            },
        );
        Ok(SnapshotLease {
            registry: self.clone(),
            snapshot_id: parent.snapshot_id,
            lease_id,
            soft_ms: parent.soft_ms,
            released: false,
        })
    }

    fn release(&self, snapshot_id: SnapshotId, lease_id: u64) {
        if let Ok(mut state) = self.state.lock() {
            let remove_entry = if let Some(entry) = state.entries.get_mut(&snapshot_id) {
                entry.pins.remove(&lease_id);
                entry.pins.is_empty()
            } else {
                false
            };
            if remove_entry {
                state.entries.remove(&snapshot_id);
            }
        }
    }
}

/// Non-cloneable ownership of one registered snapshot pin.
///
/// `fork_reader()` is the only supported way to add another reader pin. Dropping
/// a lease releases in-memory registry state and never performs storage I/O.
///
/// ```compile_fail
/// use worlddb_core::SnapshotLease;
/// fn clone_snapshot(lease: &SnapshotLease) -> SnapshotLease {
///     let copy: SnapshotLease = lease.clone();
///     copy
/// }
/// ```
#[derive(Debug)]
pub struct SnapshotLease {
    registry: SnapshotRegistry,
    snapshot_id: SnapshotId,
    lease_id: u64,
    soft_ms: u64,
    released: bool,
}

impl SnapshotLease {
    /// Immutable binding held for the complete lifetime of this lease.
    pub fn binding(&self) -> Result<Arc<SnapshotBinding>, SnapshotError> {
        self.registry
            .state
            .lock()
            .map_err(|_| SnapshotError::RegistryPoisoned)?
            .entries
            .get(&self.snapshot_id)
            .filter(|entry| entry.pins.contains_key(&self.lease_id))
            .map(|entry| Arc::clone(&entry.binding))
            .ok_or(SnapshotError::LeaseReleased)
    }

    /// Explicitly registers an independent reader pin to the same immutable view.
    pub fn fork_reader(&self, now_ms: u64) -> Result<Self, SnapshotError> {
        self.registry.fork(self, now_ms)
    }

    /// Reports the pin's soft warning or hard expiration using monotonic milliseconds.
    pub fn lifetime_status(&self, now_ms: u64) -> Result<SnapshotLifetimeStatus, SnapshotError> {
        let state = self
            .registry
            .state
            .lock()
            .map_err(|_| SnapshotError::RegistryPoisoned)?;
        let pin = state
            .entries
            .get(&self.snapshot_id)
            .and_then(|entry| entry.pins.get(&self.lease_id))
            .ok_or(SnapshotError::LeaseReleased)?;
        let age_ms = now_ms.saturating_sub(pin.started_at_ms);
        Ok(if age_ms >= pin.budget.hard_ms {
            SnapshotLifetimeStatus::Expired {
                age_ms,
                hard_limit_ms: pin.budget.hard_ms,
            }
        } else if age_ms >= self.soft_ms {
            SnapshotLifetimeStatus::SoftLimitExceeded {
                age_ms,
                soft_limit_ms: self.soft_ms,
            }
        } else {
            SnapshotLifetimeStatus::WithinBudget
        })
    }

    /// Fails closed with `SnapshotExpired` when the hard lifetime is reached.
    pub fn ensure_live(&self, now_ms: u64) -> Result<(), QueryError> {
        match self.lifetime_status(now_ms) {
            Ok(SnapshotLifetimeStatus::Expired { .. }) | Err(SnapshotError::LeaseReleased) => {
                Err(QueryError::SnapshotExpired)
            }
            Ok(
                SnapshotLifetimeStatus::WithinBudget
                | SnapshotLifetimeStatus::SoftLimitExceeded { .. },
            ) => Ok(()),
            Err(_) => Err(QueryError::StorageRead),
        }
    }
}

impl Drop for SnapshotLease {
    fn drop(&mut self) {
        if !self.released {
            self.registry.release(self.snapshot_id, self.lease_id);
            self.released = true;
        }
    }
}

fn next_lease_id(state: &mut RegistryState) -> Result<u64, SnapshotError> {
    let next = state
        .next_lease_id
        .checked_add(1)
        .ok_or(SnapshotError::LeaseIdExhausted)?;
    state.next_lease_id = next;
    Ok(next)
}

/// Snapshot construction, registration or lease-lifetime failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    /// HistorySpace ancestry is empty, duplicated, or does not end at selection.
    InvalidHistorySpaceView,
    /// Data, schema or security revisions do not form one coherent binding.
    InconsistentBinding,
    /// Layer selection is invalid against the captured historical definitions.
    InvalidLayerSelection,
    /// Lifetime thresholds are zero or not strictly ordered.
    InvalidLifetimeLimits,
    /// Exact-backup lifetime is absent or exceeds the configured maximum.
    InvalidBackupLifetime,
    /// Snapshot ID has already been used in this session registry.
    SnapshotIdReused,
    /// A lease was released or is not owned by this registry.
    LeaseReleased,
    /// Snapshot lease reached its hard lifetime.
    Expired,
    /// Session-local lease ID space was exhausted.
    LeaseIdExhausted,
    /// Registry synchronization failed.
    RegistryPoisoned,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHistorySpaceView => "invalid HistorySpace view",
            Self::InconsistentBinding => "inconsistent snapshot binding",
            Self::InvalidLayerSelection => "invalid pinned layer selection",
            Self::InvalidLifetimeLimits => "invalid snapshot lifetime limits",
            Self::InvalidBackupLifetime => "invalid exact-backup lifetime",
            Self::SnapshotIdReused => "SnapshotId was already used in this registry",
            Self::LeaseReleased => "snapshot lease has been released",
            Self::Expired => "snapshot lease expired",
            Self::LeaseIdExhausted => "snapshot lease ID space exhausted",
            Self::RegistryPoisoned => "snapshot registry synchronization failed",
        })
    }
}

impl std::error::Error for SnapshotError {}

#[cfg(test)]
mod tests {
    use super::{
        HistorySpaceView, SnapshotBinding, SnapshotBindingInput, SnapshotError,
        SnapshotLifetimeLimits, SnapshotLifetimeStatus, SnapshotPinPurpose, SnapshotRegistry,
        SnapshotSecurityBinding,
    };
    use crate::catalog::HistorySpaceDefinition;
    use crate::ids::{
        DatabaseId, DomainId, HistorySpaceId, LayerId, PrincipalId, Revision, SchemaRevision,
        SecurityEpoch, SnapshotId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot, LayerSelection};
    use crate::query_context::AuthorizationMode;
    use crate::reference_query::HistoricalQueryBinding;
    use crate::schema::{Lifecycle, NonEmptySet};
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::temporal::RecordedAsOf;
    use crate::values::Symbol;
    use std::sync::{Arc, Barrier, mpsc};
    use std::thread;
    use std::time::Duration;

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn binding(
        snapshot_tail: u8,
        revision: u64,
        generation: u64,
    ) -> Result<SnapshotBinding, String> {
        let revision = Revision::new(revision).map_err(|error| error.to_string())?;
        let layer_id = id::<LayerId>(4).map_err(|error| error.to_string())?;
        let schema_revision = SchemaRevision::from_published_revision(revision);
        let definition = LayerDefinition::new(
            layer_id,
            Symbol::new("world").map_err(|error| error.to_string())?,
            None,
            0,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(Revision::GENESIS),
        );
        let layer_schema = LayerSchemaSnapshot::new(schema_revision, vec![definition], layer_id)
            .map_err(|error| error.to_string())?;
        let recorded_as_of = RecordedAsOf::from_published_revision(revision);
        let mut history = SchemaHistoryReferenceModel::new();
        history
            .publish(
                revision,
                vec![SchemaDefinition::LayerSnapshot(layer_schema.clone())],
            )
            .map_err(|error| error.to_string())?;
        let schema = HistoricalQueryBinding::bind(&history, recorded_as_of, SchemaMode::Historical)
            .map_err(|error| error.to_string())?;
        SnapshotBinding::new(SnapshotBindingInput {
            database_id: id::<DatabaseId>(1).map_err(|error| error.to_string())?,
            snapshot_id: id::<SnapshotId>(snapshot_tail).map_err(|error| error.to_string())?,
            data_revision: revision,
            recorded_as_of,
            schema,
            history_space: HistorySpaceView::new(vec![
                HistorySpaceDefinition::new(
                    id::<HistorySpaceId>(2).map_err(|error| error.to_string())?,
                    None,
                    Revision::GENESIS,
                )
                .map_err(|error| error.to_string())?,
                HistorySpaceDefinition::new(
                    id::<HistorySpaceId>(3).map_err(|error| error.to_string())?,
                    Some(id::<HistorySpaceId>(2).map_err(|error| error.to_string())?),
                    revision,
                )
                .map_err(|error| error.to_string())?,
            ])
            .map_err(|error| error.to_string())?,
            layer_schema,
            layer_selection: LayerSelection::BaseOnly,
            security: SnapshotSecurityBinding::new(
                id::<PrincipalId>(5).map_err(|error| error.to_string())?,
                AuthorizationMode::Now,
                SecurityEpoch::new(1),
            ),
            backend_generation: generation,
        })
        .map_err(|error| error.to_string())
    }

    fn limits() -> Result<SnapshotLifetimeLimits, SnapshotError> {
        SnapshotLifetimeLimits::new(10, 20, 100)
    }

    #[test]
    fn hard_expiry_stops_reader_and_drop_releases_generation_pin() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let lease = registry
            .pin(binding(2, 1, 9)?, SnapshotPinPurpose::Interactive, 100)
            .map_err(|error| error.to_string())?;
        assert!(
            registry
                .generation_is_pinned(9)
                .map_err(|error| error.to_string())?
        );
        assert_eq!(
            lease
                .lifetime_status(110)
                .map_err(|error| error.to_string())?,
            SnapshotLifetimeStatus::SoftLimitExceeded {
                age_ms: 10,
                soft_limit_ms: 10,
            }
        );
        assert_eq!(
            lease.ensure_live(120),
            Err(crate::QueryError::SnapshotExpired)
        );
        assert!(
            registry
                .generation_is_pinned(9)
                .map_err(|error| error.to_string())?
        );
        drop(lease);
        assert!(
            !registry
                .generation_is_pinned(9)
                .map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn fork_registers_an_independent_pin_and_releases_only_its_own() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let lease = registry
            .pin(binding(6, 1, 12)?, SnapshotPinPurpose::Interactive, 100)
            .map_err(|error| error.to_string())?;
        let reader = lease.fork_reader(105).map_err(|error| error.to_string())?;
        assert_eq!(
            registry
                .active_leases(id::<SnapshotId>(6).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            2
        );
        drop(reader);
        assert_eq!(
            registry
                .active_leases(id::<SnapshotId>(6).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            1
        );
        assert!(
            registry
                .generation_is_pinned(12)
                .map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn exact_backup_can_explicitly_exceed_interactive_hard_limit() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let backup = registry
            .pin(
                binding(8, 1, 15)?,
                SnapshotPinPurpose::AdminExactBackup { lifetime_ms: 80 },
                100,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(
            backup
                .lifetime_status(150)
                .map_err(|error| error.to_string())?,
            SnapshotLifetimeStatus::SoftLimitExceeded {
                age_ms: 50,
                soft_limit_ms: 10,
            }
        );
        assert_eq!(backup.ensure_live(150), Ok(()));
        assert_eq!(
            backup
                .lifetime_status(180)
                .map_err(|error| error.to_string())?,
            SnapshotLifetimeStatus::Expired {
                age_ms: 80,
                hard_limit_ms: 80,
            }
        );
        assert_eq!(
            backup.ensure_live(180),
            Err(crate::QueryError::SnapshotExpired)
        );
        Ok(())
    }

    #[test]
    fn expired_lease_cannot_fork_a_fresh_reader_pin() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let lease = registry
            .pin(binding(9, 1, 16)?, SnapshotPinPurpose::Interactive, 100)
            .map_err(|error| error.to_string())?;
        assert_eq!(lease.fork_reader(120).err(), Some(SnapshotError::Expired));
        assert_eq!(
            registry
                .active_leases(id::<SnapshotId>(9).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            1
        );
        assert!(
            registry
                .generation_is_pinned(16)
                .map_err(|error| error.to_string())?
        );
        drop(lease);
        assert!(
            !registry
                .generation_is_pinned(16)
                .map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn scheduled_fork_then_parent_release_keeps_the_same_snapshot_pinned() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let snapshot_id = id::<SnapshotId>(24).map_err(|error| error.to_string())?;
        let parent = registry
            .pin(binding(24, 1, 34)?, SnapshotPinPurpose::Interactive, 100)
            .map_err(|error| error.to_string())?;
        let parent_binding = parent.binding().map_err(|error| error.to_string())?;
        let child = parent.fork_reader(105).map_err(|error| error.to_string())?;
        let child_binding = child.binding().map_err(|error| error.to_string())?;
        assert_eq!(child_binding.as_ref(), parent_binding.as_ref());
        drop(parent);
        assert_eq!(
            registry
                .active_leases(snapshot_id)
                .map_err(|e| e.to_string())?,
            1
        );
        assert!(
            registry
                .generation_is_pinned(34)
                .map_err(|error| error.to_string())?
        );
        assert_eq!(child.ensure_live(124), Ok(()));
        drop(child);
        assert!(
            !registry
                .generation_is_pinned(34)
                .map_err(|error| error.to_string())?
        );
        assert_eq!(
            registry
                .active_leases(snapshot_id)
                .map_err(|e| e.to_string())?,
            0
        );
        Ok(())
    }

    #[test]
    fn concurrent_duplicate_pins_are_linearizable_and_do_not_deadlock() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let snapshot_id = id::<SnapshotId>(25).map_err(|error| error.to_string())?;
        let binding = binding(25, 1, 35)?;
        let barrier = Arc::new(Barrier::new(3));
        let (sender, receiver) = mpsc::channel();
        let mut workers = Vec::new();

        for _ in 0..2 {
            let worker_registry = registry.clone();
            let worker_binding = binding.clone();
            let worker_barrier = Arc::clone(&barrier);
            let worker_sender = sender.clone();
            workers.push(thread::spawn(move || {
                worker_barrier.wait();
                let result = worker_registry
                    .pin(worker_binding, SnapshotPinPurpose::Interactive, 100)
                    .map(|lease| {
                        drop(lease);
                        true
                    });
                let _ = worker_sender.send(result);
            }));
        }
        drop(sender);
        barrier.wait();

        let first = receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|error| format!("first concurrent pin did not finish: {error}"))?;
        let second = receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|error| format!("second concurrent pin did not finish: {error}"))?;
        for worker in workers {
            worker
                .join()
                .map_err(|_| "concurrent pin worker panicked".to_owned())?;
        }

        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let errors = [first.err(), second.err()];
        assert!(
            errors
                .iter()
                .flatten()
                .all(|error| *error == SnapshotError::SnapshotIdReused)
        );
        assert_eq!(
            registry
                .active_leases(snapshot_id)
                .map_err(|e| e.to_string())?,
            0
        );
        assert!(
            !registry
                .generation_is_pinned(35)
                .map_err(|error| error.to_string())?
        );
        assert_eq!(
            registry
                .pin(binding, SnapshotPinPurpose::Interactive, 101)
                .err(),
            Some(SnapshotError::SnapshotIdReused)
        );
        Ok(())
    }

    #[test]
    fn concurrent_forks_and_registry_reads_keep_one_immutable_binding() -> Result<(), String> {
        const WORKERS: usize = 4;
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let parent = Arc::new(
            registry
                .pin(binding(26, 2, 36)?, SnapshotPinPurpose::Interactive, 100)
                .map_err(|error| error.to_string())?,
        );
        let expected_binding = parent.binding().map_err(|error| error.to_string())?;
        let barrier = Arc::new(Barrier::new(WORKERS + 1));
        let (sender, receiver) = mpsc::channel();
        let mut workers = Vec::new();

        for _ in 0..WORKERS {
            let worker_parent = Arc::clone(&parent);
            let worker_expected_binding = Arc::clone(&expected_binding);
            let worker_barrier = Arc::clone(&barrier);
            let worker_sender = sender.clone();
            workers.push(thread::spawn(move || {
                worker_barrier.wait();
                let result = worker_parent.fork_reader(105).and_then(|child| {
                    let child_binding = child.binding()?;
                    let lifetime = child.lifetime_status(106)?;
                    let immutable = child_binding.as_ref() == worker_expected_binding.as_ref()
                        && lifetime == SnapshotLifetimeStatus::WithinBudget;
                    drop(child);
                    Ok(immutable)
                });
                let _ = worker_sender.send(result);
            }));
        }
        drop(sender);
        barrier.wait();

        for index in 0..WORKERS {
            let result = receiver
                .recv_timeout(Duration::from_secs(3))
                .map_err(|error| {
                    format!("fork/registry operation {index} did not finish: {error}")
                })?;
            assert_eq!(result, Ok(true));
        }
        for worker in workers {
            worker
                .join()
                .map_err(|_| "concurrent fork worker panicked".to_owned())?;
        }

        let snapshot_id = expected_binding.snapshot().id();
        assert_eq!(
            registry
                .active_leases(snapshot_id)
                .map_err(|e| e.to_string())?,
            1
        );
        assert!(
            registry
                .generation_is_pinned(expected_binding.backend_generation())
                .map_err(|error| error.to_string())?
        );
        drop(parent);
        assert!(
            !registry
                .generation_is_pinned(expected_binding.backend_generation())
                .map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn duplicate_snapshot_id_and_invalid_backup_duration_fail_closed() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let _first = registry
            .pin(binding(10, 1, 20)?, SnapshotPinPurpose::Interactive, 0)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            registry
                .pin(binding(10, 1, 20)?, SnapshotPinPurpose::Interactive, 1)
                .err(),
            Some(SnapshotError::SnapshotIdReused)
        );
        assert_eq!(
            registry
                .pin(
                    binding(11, 1, 21)?,
                    SnapshotPinPurpose::AdminExactBackup { lifetime_ms: 101 },
                    0,
                )
                .err(),
            Some(SnapshotError::InvalidBackupLifetime)
        );
        Ok(())
    }

    #[test]
    fn descriptor_captures_all_axes_without_revision_drift() -> Result<(), String> {
        let pinned = binding(12, 1, 22)?;
        assert_eq!(
            pinned.data_revision(),
            Revision::new(1).map_err(|e| e.to_string())?
        );
        assert_eq!(pinned.schema().schema_mode(), SchemaMode::Historical);
        assert_eq!(pinned.history_space().ancestry().len(), 2);
        assert_eq!(pinned.layer_selection(), &LayerSelection::BaseOnly);
        assert_eq!(
            pinned.security().authorization_mode(),
            AuthorizationMode::Now
        );
        assert_eq!(pinned.backend_generation(), 22);
        Ok(())
    }

    #[test]
    fn explicit_layer_selection_is_bound_to_historical_layer_definitions() -> Result<(), String> {
        let pinned = binding(13, 1, 23)?;
        let layer_id = pinned.layer_schema().base_layer_id();
        let explicit = LayerSelection::Explicit(
            NonEmptySet::new(vec![layer_id]).map_err(|error| error.to_string())?,
        );
        assert!(
            SnapshotBinding::new(SnapshotBindingInput {
                database_id: pinned.database_id(),
                snapshot_id: pinned.snapshot().id(),
                data_revision: pinned.data_revision(),
                recorded_as_of: pinned.recorded_as_of(),
                schema: pinned.schema(),
                history_space: pinned.history_space().clone(),
                layer_schema: pinned.layer_schema().clone(),
                layer_selection: explicit,
                security: pinned.security(),
                backend_generation: pinned.backend_generation(),
            })
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn newer_snapshot_does_not_move_an_existing_lease() -> Result<(), String> {
        let registry = SnapshotRegistry::new(limits().map_err(|error| error.to_string())?);
        let first = registry
            .pin(binding(14, 1, 30)?, SnapshotPinPurpose::Interactive, 0)
            .map_err(|error| error.to_string())?;
        let _second = registry
            .pin(binding(15, 2, 31)?, SnapshotPinPurpose::Interactive, 1)
            .map_err(|error| error.to_string())?;

        let pinned = first.binding().map_err(|error| error.to_string())?;
        assert_eq!(
            pinned.data_revision(),
            Revision::new(1).map_err(|e| e.to_string())?
        );
        assert_eq!(pinned.backend_generation(), 30);
        Ok(())
    }
}
