//! Explicit, separately authorized storage-format upgrade from `CURRENT` v1 to v2.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use worlddb_core::{
    AuditPolicyFingerprint, AuthorizationDecision, Bytes, Capability, DatabaseId, DomainId,
    IdGenerationError, PolicyTarget, PrincipalId, Revision, SecurityPolicyView, UpgradePlanId,
    UpgradeRunId,
};

use crate::manifest::{
    CurrentPointer, CurrentPointerVersion, ManifestError, ManifestStore, encode_current_v2,
};
use crate::{
    BackupError, BackupMacKey, BackupProfile, DatabaseLayout, ExactBackupManager,
    FormatCapabilities, RestoreError, RestoreManager, StorageFileError, StorageVerifier,
    StorageVerifyError, WalError, WalPrepareLog, WriterLock, WriterLockError,
};

const UPGRADE_DOMAIN: &[u8] = b"WorldDB.StorageFormatUpgradePlan.v1\0";
const PROFILE_DOMAIN: &[u8] = b"WorldDB.StorageFormatProfile.v1\0";
const RESTORE_PROOF_DOMAIN: &[u8] = b"WorldDB.StorageUpgradeRestorePoint.v1\0";
const ACTION_DOMAIN: &[u8] = b"WorldDB.StorageUpgradeAdminAction.v1\0";
const CURRENT_V1_BYTES: u64 = 80;
const CURRENT_V2_BYTES: u64 = 112;
const MAX_PROFILE_SCAN_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
const JOURNAL_MAGIC: [u8; 8] = *b"WDBSUP\0\x01";
const JOURNAL_RECORD_BYTES: usize = 8 + 8 + 1 + 16 + 16 + (32 * 5) + 32;

/// Version of the `CURRENT` component in a recognized storage profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentPointerFormat {
    /// Original generation and manifest-digest pointer.
    V1,
    /// Pointer bound to the complete known storage-component profile.
    V2,
}

/// Recognized versions and root capabilities for one exact supported profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageFormatProfile {
    current_pointer: CurrentPointerFormat,
    required_capabilities: u64,
    optional_capabilities: u64,
    fingerprint: [u8; 32],
    compatibility_fingerprint: [u8; 32],
}

impl StorageFormatProfile {
    fn new(
        current_pointer: CurrentPointerFormat,
        capabilities: FormatCapabilities,
        component_inventory: &[ComponentInventoryEntry],
    ) -> Self {
        let pointer_version = match current_pointer {
            CurrentPointerFormat::V1 => CurrentPointerVersion::V1,
            CurrentPointerFormat::V2 => CurrentPointerVersion::V2,
        };
        let compatibility_fingerprint = profile_fingerprint(pointer_version, capabilities);
        let fingerprint =
            exact_profile_fingerprint(&compatibility_fingerprint, component_inventory);
        Self {
            current_pointer,
            required_capabilities: capabilities.required_flags(),
            optional_capabilities: capabilities.optional_flags(),
            fingerprint,
            compatibility_fingerprint,
        }
    }

    /// `CURRENT` version in this component vector.
    #[must_use]
    pub const fn current_pointer(self) -> CurrentPointerFormat {
        self.current_pointer
    }

    /// Exact required capability bits from `FORMAT`.
    #[must_use]
    pub const fn required_capabilities(self) -> u64 {
        self.required_capabilities
    }

    /// Exact optional capability bits from `FORMAT`.
    #[must_use]
    pub const fn optional_capabilities(self) -> u64 {
        self.optional_capabilities
    }

    /// Fingerprint of the closed versioned component vector and root capabilities.
    #[must_use]
    pub const fn fingerprint(self) -> [u8; 32] {
        self.fingerprint
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComponentInventoryEntry {
    relative_path: String,
    entry_kind: u8,
    length: u64,
}

/// One closed, versioned storage-format transformation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageUpgradeTransform {
    /// Re-encode the complete current manifest selection as `CURRENT` v2.
    CurrentPointerV1ToV2,
}

/// Finite transformation, memory, and staging limits for one upgrade plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageUpgradeBudget {
    max_transform_work_bytes: u64,
    max_memory_bytes: u64,
    max_staging_bytes: u64,
}

impl StorageUpgradeBudget {
    /// Creates an explicitly finite budget. Zero limits are rejected.
    pub fn new(
        max_transform_work_bytes: u64,
        max_memory_bytes: u64,
        max_staging_bytes: u64,
    ) -> Result<Self, StorageUpgradeError> {
        if max_transform_work_bytes == 0 || max_memory_bytes == 0 || max_staging_bytes == 0 {
            return Err(StorageUpgradeError::InvalidBudget);
        }
        Ok(Self {
            max_transform_work_bytes,
            max_memory_bytes,
            max_staging_bytes,
        })
    }

    /// Smallest budget accepted by the pointer-only v1 to v2 transformer.
    #[must_use]
    pub const fn current_pointer_v1_to_v2() -> Self {
        Self {
            max_transform_work_bytes: CURRENT_V1_BYTES,
            max_memory_bytes: MAX_PROFILE_SCAN_MEMORY_BYTES,
            max_staging_bytes: CURRENT_V2_BYTES,
        }
    }

    /// Maximum source bytes the transform may read.
    #[must_use]
    pub const fn max_transform_work_bytes(self) -> u64 {
        self.max_transform_work_bytes
    }

    /// Maximum in-memory working set the transform may use.
    #[must_use]
    pub const fn max_memory_bytes(self) -> u64 {
        self.max_memory_bytes
    }

    /// Maximum bytes the transform may write into staging.
    #[must_use]
    pub const fn max_staging_bytes(self) -> u64 {
        self.max_staging_bytes
    }
}

/// Caller-selected directories for the exact backup and its isolated restored clone.
#[derive(Clone, Copy, Debug)]
pub struct StorageUpgradeRestoreTargets<'a> {
    backup: &'a Path,
    restore: &'a Path,
}

impl<'a> StorageUpgradeRestoreTargets<'a> {
    /// Binds the two distinct destinations required by a storage-upgrade restore proof.
    #[must_use]
    pub const fn new(backup: &'a Path, restore: &'a Path) -> Self {
        Self { backup, restore }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceBinding {
    database_id: DatabaseId,
    revision: Revision,
    commit_hash: [u8; 32],
    format_digest: [u8; 32],
    current_digest: Option<[u8; 32]>,
}

/// Immutable plan for one exact source and target storage profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageUpgradePlan {
    id: UpgradePlanId,
    source_profile: StorageFormatProfile,
    target_profile: StorageFormatProfile,
    source: SourceBinding,
    transform: StorageUpgradeTransform,
    transformer_version: u16,
    verifier_version: u16,
    budget: StorageUpgradeBudget,
    fingerprint: [u8; 32],
}

impl StorageUpgradePlan {
    /// Unique storage-specific identity, separate from schema migration IDs.
    #[must_use]
    pub const fn id(&self) -> UpgradePlanId {
        self.id
    }

    /// Exact source storage profile.
    #[must_use]
    pub const fn source_profile(&self) -> &StorageFormatProfile {
        &self.source_profile
    }

    /// Exact target storage profile.
    #[must_use]
    pub const fn target_profile(&self) -> &StorageFormatProfile {
        &self.target_profile
    }

    /// Closed ordered transformation selected by this plan.
    #[must_use]
    pub const fn transform(&self) -> StorageUpgradeTransform {
        self.transform
    }

    /// Stable implementation version of the transformation.
    #[must_use]
    pub const fn transformer_version(&self) -> u16 {
        self.transformer_version
    }

    /// Stable implementation version of the target verifier.
    #[must_use]
    pub const fn verifier_version(&self) -> u16 {
        self.verifier_version
    }

    /// Finite resource limits bound by the plan.
    #[must_use]
    pub const fn budget(&self) -> StorageUpgradeBudget {
        self.budget
    }

    /// Canonical plan fingerprint, excluding the random plan identity.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

/// Opaque proof that the exact source was backed up, independently verified, and truly restored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageUpgradeSafeRestorePoint {
    plan_id: UpgradePlanId,
    plan_fingerprint: [u8; 32],
    source_database_id: DatabaseId,
    source_revision: Revision,
    source_commit_hash: [u8; 32],
    source_profile_fingerprint: [u8; 32],
    target_profile_fingerprint: [u8; 32],
    backup_inventory_digest: [u8; 32],
    backup_manifest_digest: [u8; 32],
    restored_database_id: DatabaseId,
    restored_revision: Revision,
    restore_destination_fingerprint: [u8; 32],
    verification_fingerprint: [u8; 32],
    operation_id: [u8; 16],
    audit_record_id: [u8; 16],
    fingerprint: [u8; 32],
}

impl StorageUpgradeSafeRestorePoint {
    /// Canonical proof fingerprint bound to the exact plan and restore evidence.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Identity of the source database proven restorable.
    #[must_use]
    pub const fn source_database_id(&self) -> DatabaseId {
        self.source_database_id
    }

    /// Exact source revision proven restorable.
    #[must_use]
    pub const fn source_revision(&self) -> Revision {
        self.source_revision
    }

    /// Identity assigned to the independently restored clone.
    #[must_use]
    pub const fn restored_database_id(&self) -> DatabaseId {
        self.restored_database_id
    }

    /// Restored clone revision after its audited RestorePublication commit.
    #[must_use]
    pub const fn restored_revision(&self) -> Revision {
        self.restored_revision
    }
}

/// Explicit, plan-bound administrator approval for one unique upgrade run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageUpgradeAdminAction {
    run_id: UpgradeRunId,
    plan_id: UpgradePlanId,
    plan_fingerprint: [u8; 32],
    restore_point_fingerprint: [u8; 32],
    actor: PrincipalId,
    rights_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

impl StorageUpgradeAdminAction {
    /// Unique run identity approved by this explicit action.
    #[must_use]
    pub const fn run_id(&self) -> UpgradeRunId {
        self.run_id
    }

    /// Fingerprint of this administrator action.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

/// Successful storage-format upgrade or reconciliation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageUpgradeReceipt {
    run_id: UpgradeRunId,
    plan_id: UpgradePlanId,
    source_revision: Revision,
    target_profile_fingerprint: [u8; 32],
    current_pointer_digest: [u8; 32],
    resumed: bool,
}

impl StorageUpgradeReceipt {
    /// Unique storage-specific run identity.
    #[must_use]
    pub const fn run_id(&self) -> UpgradeRunId {
        self.run_id
    }

    /// Plan identity completed by this run.
    #[must_use]
    pub const fn plan_id(&self) -> UpgradePlanId {
        self.plan_id
    }

    /// Source logical revision preserved by the storage-only operation.
    #[must_use]
    pub const fn source_revision(&self) -> Revision {
        self.source_revision
    }

    /// Target storage profile activated by the operation.
    #[must_use]
    pub const fn target_profile_fingerprint(&self) -> &[u8; 32] {
        &self.target_profile_fingerprint
    }

    /// Digest of the published `CURRENT` bytes observed when this run completed.
    #[must_use]
    pub const fn current_pointer_digest(&self) -> &[u8; 32] {
        &self.current_pointer_digest
    }

    /// Whether this call reconciled a previously journaled run.
    #[must_use]
    pub const fn resumed(&self) -> bool {
        self.resumed
    }
}

/// Why a storage-format upgrade could not be safely prepared, approved, or resumed.
#[derive(Debug)]
pub enum StorageUpgradeError {
    /// The source layout could not be opened or validated.
    StorageFile(StorageFileError),
    /// The manifest pointer or selected manifest was invalid.
    Manifest(ManifestError),
    /// WAL verification failed.
    Wal(WalError),
    /// The writer lock could not be acquired or was foreign.
    WriterLock(WriterLockError),
    /// Independent storage verification failed.
    StorageVerify(StorageVerifyError),
    /// Exact backup creation or verification failed.
    Backup(BackupError),
    /// The actual clone restore failed.
    Restore(RestoreError),
    /// A filesystem operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// The requested resource limits are not finite positive limits.
    InvalidBudget,
    /// The plan source is already upgraded or outside the supported v1 profile.
    UnsupportedSourceProfile,
    /// A source component, checkpoint, or logical history changed since preflight.
    SourceBindingMismatch,
    /// The source has outstanding recovery work or failed independent verification.
    SourceNotClean,
    /// Backup or real restore did not match the immutable plan and source.
    RestoreBindingMismatch,
    /// The required current permission is denied.
    AuthorizationDenied { capability: Capability },
    /// The effective rights changed after the administrator approved the action.
    AuthorizationChanged,
    /// The action, plan, proof, and run IDs do not form one exact authorization tuple.
    ActionBindingMismatch,
    /// The upgrade journal is corrupt, reversed, or bound to different immutable inputs.
    InvalidJournal,
    /// The run has no journal and cannot be resumed.
    JournalMissing,
    /// A bounded input or staging output exceeds its declared plan budget.
    BudgetExceeded,
    /// UUIDv7 identity generation failed.
    IdGeneration(IdGenerationError),
    /// Test-only injected process interruption at a durable boundary.
    InjectedFailure {
        run_id: UpgradeRunId,
        point: StorageUpgradeFaultPoint,
    },
}

impl fmt::Display for StorageUpgradeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StorageFile(error) => {
                write!(formatter, "storage layout validation failed: {error}")
            }
            Self::Manifest(error) => write!(
                formatter,
                "CURRENT or manifest verification failed: {error}"
            ),
            Self::Wal(error) => write!(formatter, "WAL verification failed: {error}"),
            Self::WriterLock(error) => write!(formatter, "writer lock failed: {error}"),
            Self::StorageVerify(error) => write!(formatter, "storage verification failed: {error}"),
            Self::Backup(error) => write!(formatter, "exact backup failed: {error}"),
            Self::Restore(error) => write!(formatter, "real clone restore failed: {error}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::InvalidBudget => {
                formatter.write_str("storage-upgrade budgets must be finite and nonzero")
            }
            Self::UnsupportedSourceProfile => formatter
                .write_str("storage upgrade requires the supported CURRENT v1 source profile"),
            Self::SourceBindingMismatch => formatter
                .write_str("database source binding changed since storage-upgrade preflight"),
            Self::SourceNotClean => {
                formatter.write_str("storage upgrade requires a clean, fully verified source")
            }
            Self::RestoreBindingMismatch => formatter
                .write_str("exact backup or real restore does not match the storage-upgrade plan"),
            Self::AuthorizationDenied { capability } => {
                write!(formatter, "current {capability:?} permission is denied")
            }
            Self::AuthorizationChanged => {
                formatter.write_str("effective rights changed after upgrade approval")
            }
            Self::ActionBindingMismatch => formatter.write_str(
                "administrator action is not bound to this exact plan and restore proof",
            ),
            Self::InvalidJournal => formatter
                .write_str("storage-upgrade journal is corrupt or has a non-monotone state"),
            Self::JournalMissing => {
                formatter.write_str("no durable storage-upgrade journal exists for this run")
            }
            Self::BudgetExceeded => {
                formatter.write_str("storage-format transformation exceeded its finite plan budget")
            }
            Self::IdGeneration(error) => write!(
                formatter,
                "storage-upgrade identity generation failed: {error}"
            ),
            Self::InjectedFailure { run_id, point } => write!(
                formatter,
                "injected interruption for storage-upgrade run {run_id} at {point:?}"
            ),
        }
    }
}

impl std::error::Error for StorageUpgradeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StorageFile(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            Self::StorageVerify(error) => Some(error),
            Self::Backup(error) => Some(error),
            Self::Restore(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::IdGeneration(error) => Some(error),
            Self::InvalidBudget
            | Self::UnsupportedSourceProfile
            | Self::SourceBindingMismatch
            | Self::SourceNotClean
            | Self::RestoreBindingMismatch
            | Self::AuthorizationDenied { .. }
            | Self::AuthorizationChanged
            | Self::ActionBindingMismatch
            | Self::InvalidJournal
            | Self::JournalMissing
            | Self::BudgetExceeded
            | Self::InjectedFailure { .. } => None,
        }
    }
}

/// Process interruption points used by storage-upgrade crash-contract tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum StorageUpgradeFaultPoint {
    BeforeJournal,
    AfterPrepared,
    AfterStaged,
    BeforeCurrentPublish,
    AfterCurrentReplace,
}

/// Coordinates preflight, restore proof, explicit authorization, execution, and recovery.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageUpgradeManager;

struct UpgradeExecutionRequest<'a> {
    layout: &'a DatabaseLayout,
    plan: &'a StorageUpgradePlan,
    restore_point: &'a StorageUpgradeSafeRestorePoint,
    action: &'a StorageUpgradeAdminAction,
    policy: SecurityPolicyView<'a>,
    policy_target: PolicyTarget,
}

impl StorageUpgradeManager {
    /// Constructs the stateless storage-upgrade coordinator.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Performs a read-only preflight and creates an immutable v1-to-v2 plan.
    pub fn prepare(
        &self,
        layout: &DatabaseLayout,
        budget: StorageUpgradeBudget,
    ) -> Result<StorageUpgradePlan, StorageUpgradeError> {
        if budget.max_transform_work_bytes < CURRENT_V1_BYTES
            || budget.max_memory_bytes < MAX_PROFILE_SCAN_MEMORY_BYTES
            || budget.max_staging_bytes < CURRENT_V2_BYTES
        {
            return Err(StorageUpgradeError::BudgetExceeded);
        }
        let opened =
            DatabaseLayout::open(layout.root()).map_err(StorageUpgradeError::StorageFile)?;
        let lock = opened
            .try_writer_lock()
            .map_err(StorageUpgradeError::WriterLock)?;
        if !lock.require_write_access() {
            return Err(StorageUpgradeError::SourceNotClean);
        }
        let (source, source_profile, pointer, mut inventory) =
            inspect_source_locked(&opened, &lock)?;
        if pointer
            .as_ref()
            .is_some_and(|pointer| pointer.version != CurrentPointerVersion::V1)
            || source_profile.current_pointer != CurrentPointerFormat::V1
        {
            return Err(StorageUpgradeError::UnsupportedSourceProfile);
        }
        target_component_inventory(&mut inventory)?;
        let target_profile = StorageFormatProfile::new(
            CurrentPointerFormat::V2,
            opened.format_capabilities(),
            &inventory,
        );
        let id = worlddb_core::storage_internal::generate_storage_upgrade_plan_id()
            .map_err(StorageUpgradeError::IdGeneration)?;
        let transform = StorageUpgradeTransform::CurrentPointerV1ToV2;
        let transformer_version = 1;
        let verifier_version = 1;
        let fingerprint = fingerprint_plan(
            &source_profile,
            &target_profile,
            &source,
            transform,
            transformer_version,
            verifier_version,
            budget,
        );
        Ok(StorageUpgradePlan {
            id,
            source_profile,
            target_profile,
            source,
            transform,
            transformer_version,
            verifier_version,
            budget,
            fingerprint,
        })
    }

    /// Creates an exact backup and performs an actual verified clone restore for this plan.
    pub fn create_safe_restore_point(
        &self,
        layout: &DatabaseLayout,
        plan: &StorageUpgradePlan,
        targets: StorageUpgradeRestoreTargets<'_>,
        key: Option<&BackupMacKey>,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
    ) -> Result<StorageUpgradeSafeRestorePoint, StorageUpgradeError> {
        for capability in [Capability::BackupCreate, Capability::BackupRestore] {
            require_capability(policy, capability, policy_target)?;
        }

        let source_layout =
            DatabaseLayout::open(layout.root()).map_err(StorageUpgradeError::StorageFile)?;
        {
            let lock = source_layout
                .try_writer_lock()
                .map_err(StorageUpgradeError::WriterLock)?;
            validate_plan_source(plan, &source_layout, &lock)?;
        }

        let backup = ExactBackupManager::new(source_layout.clone())
            .create_exact_backup(targets.backup, key)
            .map_err(StorageUpgradeError::Backup)?;
        if backup.profile() != BackupProfile::ExactDatabase
            || backup.database_id() != plan.source.database_id
            || backup.revision() != plan.source.revision
            || backup.commit_hash().as_bytes() != &plan.source.commit_hash
            || backup.inventory_digest().iter().all(|byte| *byte == 0)
            || backup.manifest_digest().iter().all(|byte| *byte == 0)
            || !backup.storage_report().is_clean()
        {
            return Err(StorageUpgradeError::RestoreBindingMismatch);
        }

        let rights = policy
            .current_snapshot()
            .effective_capability_fingerprint(policy.principal_id(), policy_target);
        let audit_fingerprint = AuditPolicyFingerprint::new(Bytes::new(rights.to_vec()))
            .map_err(|_| StorageUpgradeError::RestoreBindingMismatch)?;
        let restore = RestoreManager::new()
            .restore_clone(
                targets.backup,
                targets.restore,
                key,
                policy,
                policy_target,
                audit_fingerprint,
            )
            .map_err(StorageUpgradeError::Restore)?;
        if restore.profile() != BackupProfile::ExactDatabase
            || restore.source_database_id() != backup.database_id()
            || restore.source_revision() != backup.revision()
            || restore.restored_database_id() == backup.database_id()
            || restore.restored_revision() <= backup.revision()
        {
            return Err(StorageUpgradeError::RestoreBindingMismatch);
        }

        let restored_layout = DatabaseLayout::open(restore.destination())
            .map_err(StorageUpgradeError::StorageFile)?;
        if restored_layout.database_id() != Some(restore.restored_database_id()) {
            return Err(StorageUpgradeError::RestoreBindingMismatch);
        }
        let restored_lock = restored_layout
            .try_writer_lock()
            .map_err(StorageUpgradeError::WriterLock)?;
        let restored_report = StorageVerifier::new(restored_layout.clone())
            .verify(&restored_lock)
            .map_err(StorageUpgradeError::StorageVerify)?;
        if !restored_report.is_clean()
            || restored_report.safe_revision() != restore.restored_revision()
        {
            return Err(StorageUpgradeError::RestoreBindingMismatch);
        }

        let destination =
            fs::canonicalize(restore.destination()).map_err(|source| StorageUpgradeError::Io {
                operation: "canonicalize independently verified restore destination",
                source,
            })?;
        let destination_fingerprint = fingerprint_path(&destination);
        let verification_fingerprint = fingerprint_restore_verification(
            plan,
            &backup,
            &restore,
            &restored_report,
            &destination,
        );
        let mut proof = StorageUpgradeSafeRestorePoint {
            plan_id: plan.id,
            plan_fingerprint: plan.fingerprint,
            source_database_id: backup.database_id(),
            source_revision: backup.revision(),
            source_commit_hash: *backup.commit_hash().as_bytes(),
            source_profile_fingerprint: plan.source_profile.fingerprint,
            target_profile_fingerprint: plan.target_profile.fingerprint,
            backup_inventory_digest: *backup.inventory_digest(),
            backup_manifest_digest: *backup.manifest_digest(),
            restored_database_id: restore.restored_database_id(),
            restored_revision: restore.restored_revision(),
            restore_destination_fingerprint: destination_fingerprint,
            verification_fingerprint,
            operation_id: restore.operation_id().to_bytes(),
            audit_record_id: restore.audit_record_id().to_bytes(),
            fingerprint: [0; 32],
        };
        proof.fingerprint = fingerprint_restore_point(&proof);
        Ok(proof)
    }

    /// Confirms one exact plan and restore proof under the actor's current upgrade permission.
    pub fn confirm(
        &self,
        plan: &StorageUpgradePlan,
        restore_point: &StorageUpgradeSafeRestorePoint,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
    ) -> Result<StorageUpgradeAdminAction, StorageUpgradeError> {
        if !restore_point_matches(plan, restore_point) {
            return Err(StorageUpgradeError::ActionBindingMismatch);
        }
        require_capability(policy, Capability::StorageFormatUpgrade, policy_target)?;
        let run_id = worlddb_core::storage_internal::generate_storage_upgrade_run_id()
            .map_err(StorageUpgradeError::IdGeneration)?;
        let actor = policy.principal_id();
        let rights_fingerprint = policy
            .current_snapshot()
            .effective_capability_fingerprint(actor, policy_target);
        let fingerprint = fingerprint_action(
            run_id,
            plan.id,
            &plan.fingerprint,
            &restore_point.fingerprint,
            actor,
            &rights_fingerprint,
        );
        Ok(StorageUpgradeAdminAction {
            run_id,
            plan_id: plan.id,
            plan_fingerprint: plan.fingerprint,
            restore_point_fingerprint: restore_point.fingerprint,
            actor,
            rights_fingerprint,
            fingerprint,
        })
    }

    /// Begins the explicitly approved upgrade and publishes `CURRENT` atomically.
    pub fn execute(
        &self,
        layout: &DatabaseLayout,
        plan: &StorageUpgradePlan,
        restore_point: &StorageUpgradeSafeRestorePoint,
        action: &StorageUpgradeAdminAction,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
    ) -> Result<StorageUpgradeReceipt, StorageUpgradeError> {
        self.execute_inner(
            UpgradeExecutionRequest {
                layout,
                plan,
                restore_point,
                action,
                policy,
                policy_target,
            },
            None,
            false,
        )
    }

    /// Resumes or reconciles one journaled run after interruption at any durable boundary.
    pub fn resume(
        &self,
        layout: &DatabaseLayout,
        plan: &StorageUpgradePlan,
        restore_point: &StorageUpgradeSafeRestorePoint,
        action: &StorageUpgradeAdminAction,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
    ) -> Result<StorageUpgradeReceipt, StorageUpgradeError> {
        self.execute_inner(
            UpgradeExecutionRequest {
                layout,
                plan,
                restore_point,
                action,
                policy,
                policy_target,
            },
            None,
            true,
        )
    }
}

impl StorageUpgradeManager {
    fn execute_inner(
        &self,
        request: UpgradeExecutionRequest<'_>,
        fault: Option<StorageUpgradeFaultPoint>,
        resume_requested: bool,
    ) -> Result<StorageUpgradeReceipt, StorageUpgradeError> {
        let UpgradeExecutionRequest {
            layout,
            plan,
            restore_point,
            action,
            policy,
            policy_target,
        } = request;
        if !restore_point_matches(plan, restore_point)
            || !action_matches(plan, restore_point, action)
        {
            return Err(StorageUpgradeError::ActionBindingMismatch);
        }
        let opened =
            DatabaseLayout::open(layout.root()).map_err(StorageUpgradeError::StorageFile)?;
        let lock = opened
            .try_writer_lock()
            .map_err(StorageUpgradeError::WriterLock)?;
        if !lock.require_write_access() {
            return Err(StorageUpgradeError::SourceNotClean);
        }
        validate_current_action(action, policy, policy_target)?;

        let store = ManifestStore::new(opened.clone());
        let (active_source, active_profile, active_pointer, _) =
            inspect_source_locked(&opened, &lock)?;
        let journal_path = journal_path(&opened, action.run_id);
        let has_journal = journal_path.exists();
        let resume = resume_requested || has_journal;
        if resume_requested && !has_journal {
            return Err(StorageUpgradeError::JournalMissing);
        }
        let stored_journal = if has_journal {
            let (binding, phase, valid_length) = read_journal(&journal_path)?;
            validate_journal_base(&binding, action, plan, restore_point)?;
            repair_torn_journal_tail(&journal_path, valid_length)?;
            Some((binding, phase))
        } else {
            None
        };
        if active_profile == plan.target_profile {
            if !resume {
                return Err(StorageUpgradeError::UnsupportedSourceProfile);
            }
            let (binding, phase) = stored_journal.ok_or(StorageUpgradeError::JournalMissing)?;
            validate_recovery_phase(active_profile, plan, phase)?;
            validate_target_locked(&opened, &lock, plan)?;
            let target_digest = read_file_digest(&opened.current_file(), CURRENT_V2_BYTES)?
                .ok_or(StorageUpgradeError::SourceBindingMismatch)?;
            finish_journal(&journal_path, binding, phase)?;
            return Ok(receipt(plan, action, target_digest, true));
        }
        if active_source != plan.source || active_profile != plan.source_profile {
            return Err(StorageUpgradeError::SourceBindingMismatch);
        }
        if let Some((_, phase)) = stored_journal {
            validate_recovery_phase(active_profile, plan, phase)?;
        }

        let pointer_bytes = target_pointer_bytes(&opened, active_pointer.as_ref(), plan)?;
        if u64::try_from(pointer_bytes.len()).map_err(|_| StorageUpgradeError::BudgetExceeded)?
            > plan.budget.max_staging_bytes
        {
            return Err(StorageUpgradeError::BudgetExceeded);
        }
        let target_pointer_digest = *blake3::hash(&pointer_bytes).as_bytes();
        let binding = journal_binding(action, plan, restore_point, target_pointer_digest);

        let mut phase = if let Some((stored_binding, phase)) = stored_journal {
            if stored_binding != binding {
                return Err(StorageUpgradeError::InvalidJournal);
            }
            phase
        } else {
            if fault == Some(StorageUpgradeFaultPoint::BeforeJournal) {
                return Err(StorageUpgradeError::InjectedFailure {
                    run_id: action.run_id,
                    point: StorageUpgradeFaultPoint::BeforeJournal,
                });
            }
            create_journal(&journal_path, binding)?;
            JournalPhase::Prepared
        };

        if phase == JournalPhase::Prepared && fault == Some(StorageUpgradeFaultPoint::AfterPrepared)
        {
            return Err(StorageUpgradeError::InjectedFailure {
                run_id: action.run_id,
                point: StorageUpgradeFaultPoint::AfterPrepared,
            });
        }

        let staged = staged_pointer_path(&opened, action.run_id);
        ensure_staged_pointer(&opened, &staged, &pointer_bytes)?;
        if phase == JournalPhase::Prepared {
            phase = append_journal_phase(&journal_path, binding, phase, JournalPhase::Staged)?;
        }
        if phase == JournalPhase::Staged && fault == Some(StorageUpgradeFaultPoint::AfterStaged) {
            return Err(StorageUpgradeError::InjectedFailure {
                run_id: action.run_id,
                point: StorageUpgradeFaultPoint::AfterStaged,
            });
        }
        if phase == JournalPhase::Staged
            && fault == Some(StorageUpgradeFaultPoint::BeforeCurrentPublish)
        {
            return Err(StorageUpgradeError::InjectedFailure {
                run_id: action.run_id,
                point: StorageUpgradeFaultPoint::BeforeCurrentPublish,
            });
        }

        store
            .replace_staged_current_v2(&lock, &staged)
            .map_err(StorageUpgradeError::Manifest)?;
        if fault == Some(StorageUpgradeFaultPoint::AfterCurrentReplace) {
            return Err(StorageUpgradeError::InjectedFailure {
                run_id: action.run_id,
                point: StorageUpgradeFaultPoint::AfterCurrentReplace,
            });
        }
        store
            .sync_current_root()
            .map_err(StorageUpgradeError::Manifest)?;
        let (_, target_profile, _, _) = inspect_source_locked(&opened, &lock)?;
        validate_target_locked(&opened, &lock, plan)?;
        if target_profile != plan.target_profile {
            return Err(StorageUpgradeError::SourceBindingMismatch);
        }
        let target_digest = read_file_digest(&opened.current_file(), CURRENT_V2_BYTES)?
            .ok_or(StorageUpgradeError::SourceBindingMismatch)?;
        if phase < JournalPhase::Published {
            phase = append_journal_phase(&journal_path, binding, phase, JournalPhase::Published)?;
        }
        finish_journal(&journal_path, binding, phase)?;
        Ok(receipt(plan, action, target_digest, resume))
    }
}

fn require_capability(
    policy: SecurityPolicyView<'_>,
    capability: Capability,
    target: PolicyTarget,
) -> Result<(), StorageUpgradeError> {
    if policy
        .current_snapshot()
        .authorize(policy.principal_id(), capability, target)
        == AuthorizationDecision::Allow
    {
        Ok(())
    } else {
        Err(StorageUpgradeError::AuthorizationDenied { capability })
    }
}

fn validate_current_action(
    action: &StorageUpgradeAdminAction,
    policy: SecurityPolicyView<'_>,
    target: PolicyTarget,
) -> Result<(), StorageUpgradeError> {
    require_capability(policy, Capability::StorageFormatUpgrade, target)?;
    let actor = policy.principal_id();
    let current_rights = policy
        .current_snapshot()
        .effective_capability_fingerprint(actor, target);
    if actor != action.actor || current_rights != action.rights_fingerprint {
        return Err(StorageUpgradeError::AuthorizationChanged);
    }
    Ok(())
}

fn action_matches(
    plan: &StorageUpgradePlan,
    proof: &StorageUpgradeSafeRestorePoint,
    action: &StorageUpgradeAdminAction,
) -> bool {
    action.plan_id == plan.id
        && action.plan_fingerprint == plan.fingerprint
        && action.restore_point_fingerprint == proof.fingerprint
        && action.fingerprint
            == fingerprint_action(
                action.run_id,
                plan.id,
                &plan.fingerprint,
                &proof.fingerprint,
                action.actor,
                &action.rights_fingerprint,
            )
}

fn restore_point_matches(
    plan: &StorageUpgradePlan,
    proof: &StorageUpgradeSafeRestorePoint,
) -> bool {
    proof.plan_id == plan.id
        && proof.plan_fingerprint == plan.fingerprint
        && proof.source_database_id == plan.source.database_id
        && proof.source_revision == plan.source.revision
        && proof.source_commit_hash == plan.source.commit_hash
        && proof.source_profile_fingerprint == plan.source_profile.fingerprint
        && proof.target_profile_fingerprint == plan.target_profile.fingerprint
        && proof.fingerprint == fingerprint_restore_point(proof)
}

fn receipt(
    plan: &StorageUpgradePlan,
    action: &StorageUpgradeAdminAction,
    current_pointer_digest: [u8; 32],
    resumed: bool,
) -> StorageUpgradeReceipt {
    StorageUpgradeReceipt {
        run_id: action.run_id,
        plan_id: plan.id,
        source_revision: plan.source.revision,
        target_profile_fingerprint: plan.target_profile.fingerprint,
        current_pointer_digest,
        resumed,
    }
}

fn inspect_source_locked(
    layout: &DatabaseLayout,
    lock: &WriterLock,
) -> Result<
    (
        SourceBinding,
        StorageFormatProfile,
        Option<CurrentPointer>,
        Vec<ComponentInventoryEntry>,
    ),
    StorageUpgradeError,
> {
    if !lock.belongs_to_database_root(layout.root()) || !lock.require_write_access() {
        return Err(StorageUpgradeError::SourceNotClean);
    }
    let database_id = layout
        .database_id()
        .ok_or(StorageUpgradeError::SourceBindingMismatch)?;
    let current_store = ManifestStore::new(layout.clone());
    current_store
        .validate_directories()
        .map_err(StorageUpgradeError::Manifest)?;
    let pointer = current_store
        .read_current_pointer_for_upgrade()
        .map_err(StorageUpgradeError::Manifest)?;
    let current_version =
        pointer
            .as_ref()
            .map_or(CurrentPointerFormat::V1, |pointer| match pointer.version {
                CurrentPointerVersion::V1 => CurrentPointerFormat::V1,
                CurrentPointerVersion::V2 => CurrentPointerFormat::V2,
            });
    let inventory = scan_component_inventory(layout.root())?;
    let profile =
        StorageFormatProfile::new(current_version, layout.format_capabilities(), &inventory);

    let format_bytes = read_small_file(&layout.format_file(), 256)?;
    let format_digest = *blake3::hash(&format_bytes).as_bytes();
    let current_bytes = read_optional_small_file(&layout.current_file(), CURRENT_V2_BYTES)?;
    let current_digest = current_bytes
        .as_deref()
        .map(|bytes| *blake3::hash(bytes).as_bytes());

    let wal = WalPrepareLog::new(layout);
    let head = wal.commit_head(lock).map_err(StorageUpgradeError::Wal)?;
    let verification = StorageVerifier::new(layout.clone())
        .verify(lock)
        .map_err(StorageUpgradeError::StorageVerify)?;
    if !verification.is_clean() || verification.safe_revision() != head.revision() {
        return Err(StorageUpgradeError::SourceNotClean);
    }
    let manifest = current_store
        .read_current()
        .map_err(StorageUpgradeError::Manifest)?;
    match manifest.as_ref() {
        Some(manifest) if manifest.revision() <= head.revision() => {
            let manifest_head = wal
                .verified_head_at(lock, manifest.revision())
                .map_err(StorageUpgradeError::Wal)?
                .ok_or(StorageUpgradeError::SourceBindingMismatch)?;
            if manifest_head.commit_hash() != manifest.commit_hash() {
                return Err(StorageUpgradeError::SourceBindingMismatch);
            }
        }
        None if head.revision() == Revision::GENESIS => {}
        None => return Err(StorageUpgradeError::SourceBindingMismatch),
        Some(_) => return Err(StorageUpgradeError::SourceBindingMismatch),
    }
    let binding = SourceBinding {
        database_id,
        revision: head.revision(),
        commit_hash: *head.commit_hash().as_bytes(),
        format_digest,
        current_digest,
    };
    Ok((binding, profile, pointer, inventory))
}

fn validate_plan_source(
    plan: &StorageUpgradePlan,
    layout: &DatabaseLayout,
    lock: &WriterLock,
) -> Result<(), StorageUpgradeError> {
    let (source, profile, _, _) = inspect_source_locked(layout, lock)?;
    if source != plan.source || profile != plan.source_profile {
        return Err(StorageUpgradeError::SourceBindingMismatch);
    }
    Ok(())
}

fn validate_recovery_phase(
    active_profile: StorageFormatProfile,
    plan: &StorageUpgradePlan,
    phase: JournalPhase,
) -> Result<(), StorageUpgradeError> {
    if active_profile == plan.source_profile {
        if phase <= JournalPhase::Staged {
            return Ok(());
        }
        return Err(StorageUpgradeError::InvalidJournal);
    }
    if active_profile == plan.target_profile {
        if phase >= JournalPhase::Staged {
            return Ok(());
        }
        return Err(StorageUpgradeError::InvalidJournal);
    }
    Err(StorageUpgradeError::SourceBindingMismatch)
}

fn validate_target_locked(
    layout: &DatabaseLayout,
    lock: &WriterLock,
    plan: &StorageUpgradePlan,
) -> Result<(), StorageUpgradeError> {
    let (target_binding, profile, pointer, _) = inspect_source_locked(layout, lock)?;
    if profile != plan.target_profile
        || !matches!(
            pointer.as_ref(),
            Some(pointer)
                if pointer.version == CurrentPointerVersion::V2
                    && pointer.profile_fingerprint
                        == plan.target_profile.compatibility_fingerprint
        )
    {
        return Err(StorageUpgradeError::SourceBindingMismatch);
    }
    if target_binding.database_id != plan.source.database_id
        || target_binding.format_digest != plan.source.format_digest
        || target_binding.revision < plan.source.revision
    {
        return Err(StorageUpgradeError::SourceBindingMismatch);
    }
    let wal = WalPrepareLog::new(layout);
    let source_head = wal
        .verified_head_at(lock, plan.source.revision)
        .map_err(StorageUpgradeError::Wal)?
        .ok_or(StorageUpgradeError::SourceBindingMismatch)?;
    if source_head.commit_hash().as_bytes() != &plan.source.commit_hash {
        return Err(StorageUpgradeError::SourceBindingMismatch);
    }
    if let Some(pointer) = pointer {
        if pointer.generation > 0 {
            let current = ManifestStore::new(layout.clone())
                .read_current()
                .map_err(StorageUpgradeError::Manifest)?
                .ok_or(StorageUpgradeError::SourceBindingMismatch)?;
            if current.generation() != pointer.generation {
                return Err(StorageUpgradeError::SourceBindingMismatch);
            }
        }
    }
    Ok(())
}

fn target_pointer_bytes(
    layout: &DatabaseLayout,
    source_pointer: Option<&CurrentPointer>,
    plan: &StorageUpgradePlan,
) -> Result<Vec<u8>, StorageUpgradeError> {
    let source_bytes = read_optional_small_file(&layout.current_file(), CURRENT_V2_BYTES)?;
    if let Some(bytes) = &source_bytes {
        if u64::try_from(bytes.len()).map_err(|_| StorageUpgradeError::BudgetExceeded)?
            > plan.budget.max_transform_work_bytes
        {
            return Err(StorageUpgradeError::BudgetExceeded);
        }
    }
    let generation = source_pointer.map_or(0, |pointer| pointer.generation);
    let manifest_digest = source_pointer.map_or_else(
        || crate::ContentDigest::from_bytes([0; 32]),
        |pointer| pointer.manifest_digest,
    );
    let target = CurrentPointer {
        generation,
        manifest_digest,
        version: CurrentPointerVersion::V2,
        profile_fingerprint: plan.target_profile.compatibility_fingerprint,
    };
    let encoded = encode_current_v2(target).map_err(StorageUpgradeError::Manifest)?;
    Ok(encoded.to_vec())
}

fn read_small_file(path: &Path, maximum: u64) -> Result<Vec<u8>, StorageUpgradeError> {
    read_optional_small_file(path, maximum)?.ok_or(StorageUpgradeError::SourceBindingMismatch)
}

fn read_optional_small_file(
    path: &Path,
    maximum: u64,
) -> Result<Option<Vec<u8>>, StorageUpgradeError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageUpgradeError::Io {
                operation: "inspect bounded storage-upgrade input",
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum {
        return Err(StorageUpgradeError::SourceBindingMismatch);
    }
    let bytes = fs::read(path).map_err(|source| StorageUpgradeError::Io {
        operation: "read bounded storage-upgrade input",
        source,
    })?;
    if u64::try_from(bytes.len()).map_err(|_| StorageUpgradeError::BudgetExceeded)?
        != metadata.len()
    {
        return Err(StorageUpgradeError::SourceBindingMismatch);
    }
    Ok(Some(bytes))
}

fn read_file_digest(path: &Path, maximum: u64) -> Result<Option<[u8; 32]>, StorageUpgradeError> {
    Ok(read_optional_small_file(path, maximum)?.map(|bytes| *blake3::hash(&bytes).as_bytes()))
}

fn fingerprint_plan(
    source_profile: &StorageFormatProfile,
    target_profile: &StorageFormatProfile,
    source: &SourceBinding,
    transform: StorageUpgradeTransform,
    transformer_version: u16,
    verifier_version: u16,
    budget: StorageUpgradeBudget,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(UPGRADE_DOMAIN);
    hasher.update(&source_profile.fingerprint);
    hasher.update(&target_profile.fingerprint);
    hasher.update(&source.database_id.to_bytes());
    hasher.update(&source.revision.value().to_be_bytes());
    hasher.update(&source.commit_hash);
    hasher.update(&source.format_digest);
    match source.current_digest {
        Some(digest) => {
            hasher.update(&[1]);
            hasher.update(&digest);
        }
        None => {
            hasher.update(&[0]);
        }
    };
    match transform {
        StorageUpgradeTransform::CurrentPointerV1ToV2 => hasher.update(&[1]),
    };
    hasher.update(&transformer_version.to_be_bytes());
    hasher.update(&verifier_version.to_be_bytes());
    hasher.update(&budget.max_transform_work_bytes.to_be_bytes());
    hasher.update(&budget.max_memory_bytes.to_be_bytes());
    hasher.update(&budget.max_staging_bytes.to_be_bytes());
    *hasher.finalize().as_bytes()
}

fn fingerprint_path(path: &Path) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.StorageUpgradeRestoreDestination.v1\0");
    hasher.update(path.to_string_lossy().as_bytes());
    *hasher.finalize().as_bytes()
}

fn fingerprint_restore_verification(
    plan: &StorageUpgradePlan,
    backup: &crate::BackupVerification,
    restore: &crate::RestoreReport,
    report: &crate::StorageVerifyReport,
    destination: &Path,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.StorageUpgradeRestoreVerification.v1\0");
    hasher.update(&plan.fingerprint);
    hasher.update(backup.inventory_digest());
    hasher.update(backup.manifest_digest());
    hasher.update(backup.commit_hash().as_bytes());
    hasher.update(&backup.revision().value().to_be_bytes());
    hasher.update(&[u8::from(backup.storage_report().is_clean())]);
    hasher.update(&restore.source_database_id().to_bytes());
    hasher.update(&restore.source_revision().value().to_be_bytes());
    hasher.update(&restore.restored_database_id().to_bytes());
    hasher.update(&restore.restored_revision().value().to_be_bytes());
    hasher.update(&restore.operation_id().to_bytes());
    hasher.update(&restore.audit_record_id().to_bytes());
    hasher.update(&report.safe_revision().value().to_be_bytes());
    hasher.update(&[u8::from(report.is_clean())]);
    hasher.update(&fingerprint_path(destination));
    *hasher.finalize().as_bytes()
}

fn fingerprint_restore_point(proof: &StorageUpgradeSafeRestorePoint) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(RESTORE_PROOF_DOMAIN);
    hasher.update(&proof.plan_id.to_bytes());
    hasher.update(&proof.plan_fingerprint);
    hasher.update(&proof.source_database_id.to_bytes());
    hasher.update(&proof.source_revision.value().to_be_bytes());
    hasher.update(&proof.source_commit_hash);
    hasher.update(&proof.source_profile_fingerprint);
    hasher.update(&proof.target_profile_fingerprint);
    hasher.update(&proof.backup_inventory_digest);
    hasher.update(&proof.backup_manifest_digest);
    hasher.update(&proof.restored_database_id.to_bytes());
    hasher.update(&proof.restored_revision.value().to_be_bytes());
    hasher.update(&proof.restore_destination_fingerprint);
    hasher.update(&proof.verification_fingerprint);
    hasher.update(&proof.operation_id);
    hasher.update(&proof.audit_record_id);
    *hasher.finalize().as_bytes()
}

fn fingerprint_action(
    run_id: UpgradeRunId,
    plan_id: UpgradePlanId,
    plan_fingerprint: &[u8; 32],
    restore_point_fingerprint: &[u8; 32],
    actor: PrincipalId,
    rights_fingerprint: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(ACTION_DOMAIN);
    hasher.update(&run_id.to_bytes());
    hasher.update(&plan_id.to_bytes());
    hasher.update(plan_fingerprint);
    hasher.update(restore_point_fingerprint);
    hasher.update(&actor.to_bytes());
    hasher.update(rights_fingerprint);
    *hasher.finalize().as_bytes()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
enum JournalPhase {
    Prepared = 1,
    Staged = 2,
    Published = 3,
    Complete = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JournalBinding {
    run_id: UpgradeRunId,
    plan_id: UpgradePlanId,
    plan_fingerprint: [u8; 32],
    proof_fingerprint: [u8; 32],
    action_fingerprint: [u8; 32],
    source_binding_fingerprint: [u8; 32],
    target_pointer_digest: [u8; 32],
}

fn journal_binding(
    action: &StorageUpgradeAdminAction,
    plan: &StorageUpgradePlan,
    proof: &StorageUpgradeSafeRestorePoint,
    target_pointer_digest: [u8; 32],
) -> JournalBinding {
    JournalBinding {
        run_id: action.run_id,
        plan_id: plan.id,
        plan_fingerprint: plan.fingerprint,
        proof_fingerprint: proof.fingerprint,
        action_fingerprint: action.fingerprint,
        source_binding_fingerprint: fingerprint_source_binding(&plan.source),
        target_pointer_digest,
    }
}

fn fingerprint_source_binding(source: &SourceBinding) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.StorageUpgradeSourceBinding.v1\0");
    hasher.update(&source.database_id.to_bytes());
    hasher.update(&source.revision.value().to_be_bytes());
    hasher.update(&source.commit_hash);
    hasher.update(&source.format_digest);
    match source.current_digest {
        Some(digest) => {
            hasher.update(&[1]);
            hasher.update(&digest);
        }
        None => {
            hasher.update(&[0]);
        }
    };
    *hasher.finalize().as_bytes()
}

fn validate_journal_base(
    binding: &JournalBinding,
    action: &StorageUpgradeAdminAction,
    plan: &StorageUpgradePlan,
    proof: &StorageUpgradeSafeRestorePoint,
) -> Result<(), StorageUpgradeError> {
    if binding.run_id != action.run_id
        || binding.plan_id != plan.id
        || binding.plan_fingerprint != plan.fingerprint
        || binding.proof_fingerprint != proof.fingerprint
        || binding.action_fingerprint != action.fingerprint
        || binding.source_binding_fingerprint != fingerprint_source_binding(&plan.source)
    {
        return Err(StorageUpgradeError::InvalidJournal);
    }
    Ok(())
}

fn journal_path(layout: &DatabaseLayout, run_id: UpgradeRunId) -> PathBuf {
    layout
        .staging_directory()
        .join(format!("storage-upgrade-{run_id}.journal"))
}

fn staged_pointer_path(layout: &DatabaseLayout, run_id: UpgradeRunId) -> PathBuf {
    layout
        .staging_directory()
        .join(format!("storage-upgrade-{run_id}.current"))
}

fn create_journal(path: &Path, binding: JournalBinding) -> Result<(), StorageUpgradeError> {
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "create storage-upgrade journal",
            source,
        })?;
    file.sync_all().map_err(|source| StorageUpgradeError::Io {
        operation: "sync new storage-upgrade journal",
        source,
    })?;
    crate::manifest::sync_directory(path.parent().ok_or(StorageUpgradeError::InvalidJournal)?)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "sync storage-upgrade journal directory",
            source,
        })?;
    append_journal_record(path, binding, JournalPhase::Prepared)
}

fn append_journal_phase(
    path: &Path,
    binding: JournalBinding,
    current: JournalPhase,
    next: JournalPhase,
) -> Result<JournalPhase, StorageUpgradeError> {
    let expected = match current {
        JournalPhase::Prepared => JournalPhase::Staged,
        JournalPhase::Staged => JournalPhase::Published,
        JournalPhase::Published => JournalPhase::Complete,
        JournalPhase::Complete => return Err(StorageUpgradeError::InvalidJournal),
    };
    if next != expected {
        return Err(StorageUpgradeError::InvalidJournal);
    }
    append_journal_record(path, binding, next)?;
    Ok(next)
}

fn append_journal_record(
    path: &Path,
    binding: JournalBinding,
    phase: JournalPhase,
) -> Result<(), StorageUpgradeError> {
    let bytes = encode_journal_record(binding, phase);
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "open storage-upgrade journal for append",
            source,
        })?;
    file.write_all(&bytes)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "append storage-upgrade journal state",
            source,
        })?;
    file.sync_all().map_err(|source| StorageUpgradeError::Io {
        operation: "sync storage-upgrade journal state",
        source,
    })
}

fn encode_journal_record(binding: JournalBinding, phase: JournalPhase) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(JOURNAL_RECORD_BYTES);
    bytes.extend_from_slice(&JOURNAL_MAGIC);
    bytes.extend_from_slice(&(phase as u64).to_le_bytes());
    bytes.push(phase as u8);
    bytes.extend_from_slice(&binding.run_id.to_bytes());
    bytes.extend_from_slice(&binding.plan_id.to_bytes());
    bytes.extend_from_slice(&binding.plan_fingerprint);
    bytes.extend_from_slice(&binding.proof_fingerprint);
    bytes.extend_from_slice(&binding.action_fingerprint);
    bytes.extend_from_slice(&binding.source_binding_fingerprint);
    bytes.extend_from_slice(&binding.target_pointer_digest);
    let checksum = *blake3::hash(&bytes).as_bytes();
    bytes.extend_from_slice(&checksum);
    bytes
}

fn read_journal(path: &Path) -> Result<(JournalBinding, JournalPhase, usize), StorageUpgradeError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageUpgradeError::Io {
        operation: "inspect storage-upgrade journal",
        source,
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > (JOURNAL_RECORD_BYTES * 5) as u64
    {
        return Err(StorageUpgradeError::InvalidJournal);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "open storage-upgrade journal",
            source,
        })?
        .take((JOURNAL_RECORD_BYTES * 5 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "read storage-upgrade journal",
            source,
        })?;
    let complete_records = bytes.len() / JOURNAL_RECORD_BYTES;
    let valid_length = complete_records
        .checked_mul(JOURNAL_RECORD_BYTES)
        .ok_or(StorageUpgradeError::InvalidJournal)?;
    if complete_records == 0 || complete_records > 4 {
        return Err(StorageUpgradeError::InvalidJournal);
    }
    let mut latest: Option<(JournalBinding, JournalPhase)> = None;
    let complete_bytes = bytes
        .get(..valid_length)
        .ok_or(StorageUpgradeError::InvalidJournal)?;
    for (index, record) in complete_bytes
        .chunks_exact(JOURNAL_RECORD_BYTES)
        .enumerate()
    {
        let (binding, phase, sequence) = decode_journal_record(record)?;
        let expected_sequence =
            u64::try_from(index + 1).map_err(|_| StorageUpgradeError::InvalidJournal)?;
        if sequence != expected_sequence {
            return Err(StorageUpgradeError::InvalidJournal);
        }
        match latest {
            None if phase == JournalPhase::Prepared => latest = Some((binding, phase)),
            Some((prior_binding, prior_phase))
                if prior_binding == binding && next_journal_phase(prior_phase) == Some(phase) =>
            {
                latest = Some((binding, phase));
            }
            _ => return Err(StorageUpgradeError::InvalidJournal),
        }
    }
    let (binding, phase) = latest.ok_or(StorageUpgradeError::InvalidJournal)?;
    Ok((binding, phase, valid_length))
}

fn decode_journal_record(
    bytes: &[u8],
) -> Result<(JournalBinding, JournalPhase, u64), StorageUpgradeError> {
    if bytes.len() != JOURNAL_RECORD_BYTES || bytes.get(..8) != Some(&JOURNAL_MAGIC) {
        return Err(StorageUpgradeError::InvalidJournal);
    }
    let checksum_start = JOURNAL_RECORD_BYTES - 32;
    let expected_checksum = blake3::hash(
        bytes
            .get(..checksum_start)
            .ok_or(StorageUpgradeError::InvalidJournal)?,
    );
    if expected_checksum.as_bytes()
        != bytes
            .get(checksum_start..)
            .ok_or(StorageUpgradeError::InvalidJournal)?
    {
        return Err(StorageUpgradeError::InvalidJournal);
    }
    let sequence = u64::from_le_bytes(
        bytes
            .get(8..16)
            .ok_or(StorageUpgradeError::InvalidJournal)?
            .try_into()
            .map_err(|_| StorageUpgradeError::InvalidJournal)?,
    );
    let phase = match bytes.get(16).copied() {
        Some(1) => JournalPhase::Prepared,
        Some(2) => JournalPhase::Staged,
        Some(3) => JournalPhase::Published,
        Some(4) => JournalPhase::Complete,
        _ => return Err(StorageUpgradeError::InvalidJournal),
    };
    let run_id = UpgradeRunId::try_from_bytes(
        bytes
            .get(17..33)
            .ok_or(StorageUpgradeError::InvalidJournal)?
            .try_into()
            .map_err(|_| StorageUpgradeError::InvalidJournal)?,
    )
    .map_err(|_| StorageUpgradeError::InvalidJournal)?;
    let plan_id = UpgradePlanId::try_from_bytes(
        bytes
            .get(33..49)
            .ok_or(StorageUpgradeError::InvalidJournal)?
            .try_into()
            .map_err(|_| StorageUpgradeError::InvalidJournal)?,
    )
    .map_err(|_| StorageUpgradeError::InvalidJournal)?;
    let plan_fingerprint = read_digest(bytes, 49)?;
    let proof_fingerprint = read_digest(bytes, 81)?;
    let action_fingerprint = read_digest(bytes, 113)?;
    let source_binding_fingerprint = read_digest(bytes, 145)?;
    let target_pointer_digest = read_digest(bytes, 177)?;
    let binding = JournalBinding {
        run_id,
        plan_id,
        plan_fingerprint,
        proof_fingerprint,
        action_fingerprint,
        source_binding_fingerprint,
        target_pointer_digest,
    };
    Ok((binding, phase, sequence))
}

fn read_digest(bytes: &[u8], start: usize) -> Result<[u8; 32], StorageUpgradeError> {
    bytes
        .get(start..start + 32)
        .ok_or(StorageUpgradeError::InvalidJournal)?
        .try_into()
        .map_err(|_| StorageUpgradeError::InvalidJournal)
}

fn next_journal_phase(phase: JournalPhase) -> Option<JournalPhase> {
    match phase {
        JournalPhase::Prepared => Some(JournalPhase::Staged),
        JournalPhase::Staged => Some(JournalPhase::Published),
        JournalPhase::Published => Some(JournalPhase::Complete),
        JournalPhase::Complete => None,
    }
}

fn repair_torn_journal_tail(path: &Path, valid_length: usize) -> Result<(), StorageUpgradeError> {
    let length = fs::metadata(path)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "inspect torn storage-upgrade journal tail",
            source,
        })?
        .len();
    let valid = u64::try_from(valid_length).map_err(|_| StorageUpgradeError::InvalidJournal)?;
    if length > valid {
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|source| StorageUpgradeError::Io {
                operation: "open torn storage-upgrade journal for recovery",
                source,
            })?;
        file.set_len(valid)
            .map_err(|source| StorageUpgradeError::Io {
                operation: "truncate torn storage-upgrade journal tail",
                source,
            })?;
        file.sync_all().map_err(|source| StorageUpgradeError::Io {
            operation: "sync repaired storage-upgrade journal",
            source,
        })?;
    }
    Ok(())
}

fn ensure_staged_pointer(
    layout: &DatabaseLayout,
    path: &Path,
    bytes: &[u8],
) -> Result<(), StorageUpgradeError> {
    match read_optional_small_file(path, CURRENT_V2_BYTES) {
        Ok(Some(existing)) if existing == bytes => return Ok(()),
        Ok(Some(_)) => return Err(StorageUpgradeError::InvalidJournal),
        Ok(None) => {}
        Err(StorageUpgradeError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
        }
        Err(error) => return Err(error),
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "create staged CURRENT v2 pointer",
            source,
        })?;
    file.write_all(bytes)
        .map_err(|source| StorageUpgradeError::Io {
            operation: "write staged CURRENT v2 pointer",
            source,
        })?;
    file.sync_all().map_err(|source| StorageUpgradeError::Io {
        operation: "sync staged CURRENT v2 pointer",
        source,
    })?;
    crate::manifest::sync_directory(&layout.staging_directory()).map_err(|source| {
        StorageUpgradeError::Io {
            operation: "sync storage-upgrade staging directory",
            source,
        }
    })
}

fn finish_journal(
    path: &Path,
    binding: JournalBinding,
    mut phase: JournalPhase,
) -> Result<(), StorageUpgradeError> {
    while phase < JournalPhase::Complete {
        let next = next_journal_phase(phase).ok_or(StorageUpgradeError::InvalidJournal)?;
        phase = append_journal_phase(path, binding, phase, next)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fuzz_storage_upgrade_journal(bytes: &[u8]) -> bool {
    decode_journal_record(bytes).is_ok()
}

#[cfg(test)]
#[path = "storage_upgrade_tests.rs"]
mod tests;

pub(crate) fn profile_fingerprint(
    current_pointer: CurrentPointerVersion,
    capabilities: FormatCapabilities,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(PROFILE_DOMAIN);
    // This ordered vector is the recognized pre-Alpha component baseline.
    hasher.update(b"format=1.0;domain_wal=1.0;raw_audit_wal=1.0;segment=1.0;manifest=1.0;");
    match current_pointer {
        CurrentPointerVersion::V1 => hasher.update(b"current=1.0;"),
        CurrentPointerVersion::V2 => hasher.update(b"current=2.0;"),
    };
    hasher.update(b"security=1.0;required_audit=1.0;replay_snapshot=1.0;recovery_journal=1.0;index_pointer=1.0;index_generation=1.0;");
    hasher.update(&capabilities.required_flags().to_be_bytes());
    hasher.update(&capabilities.optional_flags().to_be_bytes());
    *hasher.finalize().as_bytes()
}

fn exact_profile_fingerprint(
    compatibility_fingerprint: &[u8; 32],
    inventory: &[ComponentInventoryEntry],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.StorageFormatExactProfile.v1\0");
    hasher.update(compatibility_fingerprint);
    hasher.update(&(inventory.len() as u64).to_be_bytes());
    for entry in inventory {
        hasher.update(&(entry.relative_path.len() as u32).to_be_bytes());
        hasher.update(entry.relative_path.as_bytes());
        hasher.update(&[entry.entry_kind]);
        hasher.update(&entry.length.to_be_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn target_component_inventory(
    target: &mut Vec<ComponentInventoryEntry>,
) -> Result<(), StorageUpgradeError> {
    let current = target
        .iter_mut()
        .find(|entry| entry.relative_path == "CURRENT");
    if let Some(current) = current {
        if current.entry_kind != 1 || current.length != CURRENT_V1_BYTES {
            return Err(StorageUpgradeError::UnsupportedSourceProfile);
        }
        current.length = CURRENT_V2_BYTES;
    } else {
        target.push(ComponentInventoryEntry {
            relative_path: "CURRENT".to_owned(),
            entry_kind: 1,
            length: CURRENT_V2_BYTES,
        });
    }
    target.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(())
}

fn scan_component_inventory(
    root: &Path,
) -> Result<Vec<ComponentInventoryEntry>, StorageUpgradeError> {
    fn walk(
        root: &Path,
        directory: &Path,
        depth: usize,
        entries: &mut Vec<ComponentInventoryEntry>,
        path_bytes: &mut usize,
        visited_entries: &mut usize,
    ) -> Result<(), StorageUpgradeError> {
        if depth > 32 {
            return Err(StorageUpgradeError::BudgetExceeded);
        }
        for entry in fs::read_dir(directory).map_err(|source| StorageUpgradeError::Io {
            operation: "enumerate storage-component profile",
            source,
        })? {
            let entry = entry.map_err(|source| StorageUpgradeError::Io {
                operation: "read storage-component profile entry",
                source,
            })?;
            *visited_entries = visited_entries
                .checked_add(1)
                .ok_or(StorageUpgradeError::BudgetExceeded)?;
            if *visited_entries > 65_536 {
                return Err(StorageUpgradeError::BudgetExceeded);
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| StorageUpgradeError::SourceBindingMismatch)?;
            let relative_path = relative
                .components()
                .map(|component| component.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()
                .ok_or(StorageUpgradeError::SourceBindingMismatch)?
                .join("/");
            if depth == 0 && matches!(relative_path.as_str(), "staging" | "quarantine" | "LOCK") {
                continue;
            }
            let metadata =
                fs::symlink_metadata(&path).map_err(|source| StorageUpgradeError::Io {
                    operation: "inspect storage-component profile entry",
                    source,
                })?;
            if metadata.file_type().is_symlink() {
                return Err(StorageUpgradeError::SourceBindingMismatch);
            }
            let entry_kind = if metadata.is_dir() {
                2
            } else if metadata.is_file() {
                1
            } else {
                return Err(StorageUpgradeError::SourceBindingMismatch);
            };
            // WAL checkpointing can leave a zero-byte next-segment allocation behind.
            // It has no encoded component version or history bytes, so it is not
            // part of the normative component-presence vector.
            if entry_kind == 1 && metadata.len() == 0 && relative_path.starts_with("wal/") {
                continue;
            }
            *path_bytes = path_bytes
                .checked_add(relative_path.len())
                .ok_or(StorageUpgradeError::BudgetExceeded)?;
            if *path_bytes > 8 * 1024 * 1024 || entries.len() >= 65_536 {
                return Err(StorageUpgradeError::BudgetExceeded);
            }
            let estimated_entry_bytes = (entries.len() + 1)
                .checked_mul(std::mem::size_of::<ComponentInventoryEntry>() + 256)
                .ok_or(StorageUpgradeError::BudgetExceeded)?;
            let estimated_memory = estimated_entry_bytes
                .checked_add(*path_bytes)
                .ok_or(StorageUpgradeError::BudgetExceeded)?;
            if u64::try_from(estimated_memory).map_err(|_| StorageUpgradeError::BudgetExceeded)?
                > MAX_PROFILE_SCAN_MEMORY_BYTES
            {
                return Err(StorageUpgradeError::BudgetExceeded);
            }
            entries.push(ComponentInventoryEntry {
                relative_path,
                entry_kind,
                length: if entry_kind == 1 { metadata.len() } else { 0 },
            });
            if entry_kind == 2 {
                walk(root, &path, depth + 1, entries, path_bytes, visited_entries)?;
            }
        }
        Ok(())
    }

    let mut entries = Vec::new();
    let mut path_bytes = 0_usize;
    let mut visited_entries = 0_usize;
    walk(
        root,
        root,
        0,
        &mut entries,
        &mut path_bytes,
        &mut visited_entries,
    )?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}
