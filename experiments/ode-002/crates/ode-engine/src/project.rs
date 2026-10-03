use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence, Bytes, Capability,
    HistorySpaceDefinition, HistorySpaceId, LayerDefinition, LayerId, LayerSchemaSnapshot,
    Lifecycle, PolicyBundle, PolicyScope, PolicyTarget, Principal, PrincipalId, Record, Revision,
    RoleAssignment, RoleDefinition, SchemaRevision, SecurityEpoch, SecurityPolicyChange,
    SecurityPolicyRecord, SecurityPolicySnapshot, SecurityPolicyVersion, Symbol,
};
use worlddb_storage_file::{
    DatabaseLayout, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
    ManifestStore, RecoveryDisposition, RecoveryManager, SecurityPolicyHistoryStore,
    StorageVerifier, WalOperationStatus, WalPrepareLog,
};

/// A safe host-facing failure class for project create/open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectError {
    AlreadyExists,
    AlreadyOpen,
    AccessDenied,
    InvalidProject,
    RecoveryRequired,
    UnsupportedIdentity,
    HostUnavailable,
    UnknownCommitOutcome,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AlreadyExists => "project destination already exists",
            Self::AlreadyOpen => "project is open in another WorldDB host",
            Self::AccessDenied => "the current account has no project access",
            Self::InvalidProject => "selected folder is not a valid WorldDB project",
            Self::RecoveryRequired => "project needs recovery and was not opened",
            Self::UnsupportedIdentity => "host account identity is unavailable",
            Self::HostUnavailable => "project storage is unavailable",
            Self::UnknownCommitOutcome => "project creation outcome needs recovery",
        })
    }
}

impl std::error::Error for ProjectError {}

/// Safe project facts resolved by the host from the selected WorldDB directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAccess {
    canonical_root: PathBuf,
    database_id: worlddb_core::DatabaseId,
    revision: Revision,
    role_symbol: String,
    principal_id: PrincipalId,
}

impl ProjectAccess {
    #[must_use]
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    #[must_use]
    pub const fn database_id(&self) -> worlddb_core::DatabaseId {
        self.database_id
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn role_symbol(&self) -> &str {
        &self.role_symbol
    }

    /// Host-internal identity for the engine connection; never serialize this value to a renderer.
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }
}

/// Creates a new project with the complete initial security state and required audit commit.
/// The caller must obtain `root` from a native host dialog, not from renderer input.
pub fn create_project(
    root: impl AsRef<Path>,
    creator: PrincipalId,
) -> Result<ProjectAccess, ProjectError> {
    let requested_root = root.as_ref();
    if requested_root.exists() {
        return Err(ProjectError::AlreadyExists);
    }
    let parent = requested_root
        .parent()
        .ok_or(ProjectError::HostUnavailable)?;
    let parent_metadata =
        fs::symlink_metadata(parent).map_err(|_| ProjectError::HostUnavailable)?;
    if !parent_metadata.is_dir() || is_reparse_point(&parent_metadata) {
        return Err(ProjectError::HostUnavailable);
    }
    let root_name = requested_root
        .file_name()
        .ok_or(ProjectError::HostUnavailable)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| ProjectError::HostUnavailable)?;
    let root = canonical_parent.join(root_name);
    if root.exists() {
        return Err(ProjectError::AlreadyExists);
    }

    let layout = DatabaseLayout::create(&root).map_err(|_| ProjectError::HostUnavailable)?;
    let lock = layout
        .try_writer_lock()
        .map_err(|_| ProjectError::HostUnavailable)?;
    let recovery = RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|_| ProjectError::HostUnavailable)?;
    if !recovery.report().is_clean() {
        return Err(ProjectError::RecoveryRequired);
    }

    let bootstrap = build_bootstrap(creator)?;
    let policy_store = SecurityPolicyHistoryStore::new(layout.clone());
    let genesis = SecurityPolicyVersion::new(
        Revision::GENESIS,
        SecurityEpoch::INITIAL,
        SecurityPolicySnapshot::default(),
    );
    let genesis_receipt = policy_store
        .stage_version(&lock, &genesis, None, None)
        .map_err(|_| ProjectError::HostUnavailable)?;
    let policy_receipt = policy_store
        .stage_version(
            &lock,
            &bootstrap.version,
            Some(&bootstrap.policy_record),
            None,
        )
        .map_err(|_| ProjectError::HostUnavailable)?;

    let genesis_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        genesis_receipt.id(),
        genesis_receipt.content_digest(),
        Revision::GENESIS,
    );
    let policy_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        policy_receipt.id(),
        policy_receipt.content_digest(),
        bootstrap.version.revision(),
    );
    let history_store = HistorySegmentStore::new(layout.clone());
    let history_receipt = history_store
        .stage_segment(&lock, &bootstrap.metadata_records)
        .map_err(|_| ProjectError::HostUnavailable)?;
    let history_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        history_receipt.id(),
        history_receipt.content_digest(),
        bootstrap.version.revision(),
    );
    let references = vec![genesis_reference, policy_reference, history_reference];
    let wal = WalPrepareLog::new(&layout);
    let committed = wal.commit_audited_manifest_snapshot(
        &lock,
        bootstrap.operation_id,
        references,
        &[genesis_reference, policy_reference, history_reference],
        &bootstrap.audit_record,
    );
    if committed.is_err() {
        match wal.operation_status(&lock, bootstrap.operation_id) {
            Ok(WalOperationStatus::NotCommitted) => {
                drop(lock);
                let _ = fs::remove_dir_all(layout.root());
                return Err(ProjectError::HostUnavailable);
            }
            Ok(WalOperationStatus::Committed(_)) => {}
            Ok(WalOperationStatus::Indeterminate) | Err(_) => {
                return Err(ProjectError::UnknownCommitOutcome);
            }
        }
    }

    let recovered = RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|_| ProjectError::UnknownCommitOutcome)?;
    if !recovered.report().is_clean() {
        return Err(ProjectError::RecoveryRequired);
    }
    drop(lock);
    open_project(layout.root(), creator)
}

/// Opens a validated project and resolves the current host-authenticated Principal.
/// Recovery, migration, and format changes are never performed by this operation.
pub fn open_project(
    root: impl AsRef<Path>,
    principal_id: PrincipalId,
) -> Result<ProjectAccess, ProjectError> {
    let selected_root = root.as_ref();
    let selected_metadata =
        fs::symlink_metadata(selected_root).map_err(|_| ProjectError::InvalidProject)?;
    if !selected_metadata.is_dir() || is_reparse_point(&selected_metadata) {
        return Err(ProjectError::InvalidProject);
    }
    let canonical_root =
        fs::canonicalize(selected_root).map_err(|_| ProjectError::InvalidProject)?;
    let layout = DatabaseLayout::open(&canonical_root).map_err(|_| ProjectError::InvalidProject)?;
    let lock = layout.try_writer_lock().map_err(map_writer_lock_error)?;
    resolve_open_access(&layout, &lock, principal_id)
}

pub(crate) fn resolve_open_access(
    layout: &DatabaseLayout,
    lock: &worlddb_storage_file::WriterLock,
    principal_id: PrincipalId,
) -> Result<ProjectAccess, ProjectError> {
    let verification = StorageVerifier::new(layout.clone())
        .verify(lock)
        .map_err(|_| ProjectError::InvalidProject)?;
    if verification.disposition() != RecoveryDisposition::Clean {
        return Err(ProjectError::RecoveryRequired);
    }
    let manifest = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|_| ProjectError::InvalidProject)?
        .ok_or(ProjectError::InvalidProject)?;
    let security_segments = manifest
        .segments()
        .iter()
        .filter(|segment| segment.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|segment| segment.id())
        .collect::<Vec<_>>();
    if security_segments.is_empty() {
        return Err(ProjectError::InvalidProject);
    }
    let policy_history = SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(manifest.revision(), &security_segments)
        .map_err(|_| ProjectError::InvalidProject)?;
    let policy_version = policy_history
        .policy()
        .latest_version()
        .map_err(|_| ProjectError::InvalidProject)?;
    let policy = policy_version.snapshot();
    if policy.authorize(
        principal_id,
        Capability::ProjectRead,
        PolicyTarget::default(),
    ) != worlddb_core::AuthorizationDecision::Allow
    {
        return Err(ProjectError::AccessDenied);
    }
    let role_symbol = resolve_role_symbol(policy, principal_id);
    let database_id = layout.database_id().ok_or(ProjectError::InvalidProject)?;
    Ok(ProjectAccess {
        canonical_root: layout.root().to_owned(),
        database_id,
        revision: manifest.revision(),
        role_symbol,
        principal_id,
    })
}

struct ProjectBootstrap {
    version: SecurityPolicyVersion,
    policy_record: SecurityPolicyRecord,
    audit_record: AuditRecord,
    operation_id: worlddb_core::OperationId,
    metadata_records: Vec<Record>,
}

fn build_bootstrap(creator: PrincipalId) -> Result<ProjectBootstrap, ProjectError> {
    use worlddb_core::storage_internal::generate_project_bootstrap_id as generate;

    let gm_id = generate::<worlddb_core::RoleId>().map_err(|_| ProjectError::HostUnavailable)?;
    let player_id =
        generate::<worlddb_core::RoleId>().map_err(|_| ProjectError::HostUnavailable)?;
    let assignment_id =
        generate::<worlddb_core::RoleAssignmentId>().map_err(|_| ProjectError::HostUnavailable)?;
    let gm_symbol = Symbol::new("gm").map_err(|_| ProjectError::HostUnavailable)?;
    let player_symbol = Symbol::new("player").map_err(|_| ProjectError::HostUnavailable)?;
    let gm = RoleDefinition::new(gm_id, gm_symbol.as_str(), PolicyBundle::standard_gm())
        .map_err(|_| ProjectError::HostUnavailable)?;
    let player = RoleDefinition::new(
        player_id,
        player_symbol.as_str(),
        PolicyBundle::standard_player(),
    )
    .map_err(|_| ProjectError::HostUnavailable)?;
    let assignment = RoleAssignment::new(assignment_id, creator, gm_id, PolicyScope::project());
    let candidate = SecurityPolicySnapshot::new(
        vec![Principal::new(creator)],
        vec![gm, player],
        vec![assignment],
        vec![],
    )
    .map_err(|_| ProjectError::HostUnavailable)?;
    let revision = Revision::FIRST_COMMIT;
    let history_space_id =
        generate::<HistorySpaceId>().map_err(|_| ProjectError::HostUnavailable)?;
    let layer_id = generate::<LayerId>().map_err(|_| ProjectError::HostUnavailable)?;
    let history_space = HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)
        .map_err(|_| ProjectError::HostUnavailable)?;
    let base_layer = LayerDefinition::new(
        layer_id,
        Symbol::new("base").map_err(|_| ProjectError::HostUnavailable)?,
        Some("Grundlage für neue Inhalte".to_owned()),
        0,
        Lifecycle::Active,
        SchemaRevision::from_published_revision(revision),
    );
    let layer_snapshot = LayerSchemaSnapshot::new(
        SchemaRevision::from_published_revision(revision),
        vec![base_layer.clone()],
        layer_id,
    )
    .map_err(|_| ProjectError::HostUnavailable)?;
    let metadata_records = vec![
        Record::HistorySpaceDefinition(history_space),
        Record::LayerDefinition(base_layer),
        Record::LayerSchemaSnapshot(layer_snapshot),
    ];
    let epoch = SecurityEpoch::INITIAL
        .next()
        .map_err(|_| ProjectError::HostUnavailable)?;
    let policy_record = SecurityPolicyRecord::new(
        generate::<worlddb_core::SecurityPolicyRecordId>()
            .map_err(|_| ProjectError::HostUnavailable)?,
        revision,
        creator,
        epoch,
        vec![
            SecurityPolicyChange::PrincipalRegistered {
                principal_id: creator,
            },
            SecurityPolicyChange::RoleRegistered {
                role_id: gm_id,
                symbol: gm_symbol,
            },
            SecurityPolicyChange::RoleRegistered {
                role_id: player_id,
                symbol: player_symbol,
            },
            SecurityPolicyChange::RoleAssigned {
                assignment_id,
                principal_id: creator,
                role_id: gm_id,
                scope: PolicyScope::project(),
            },
        ],
    )
    .map_err(|_| ProjectError::HostUnavailable)?;
    let version = SecurityPolicyVersion::new(revision, epoch, candidate);
    let operation_id =
        generate::<worlddb_core::OperationId>().map_err(|_| ProjectError::HostUnavailable)?;
    let audit_record = AuditRecord::new(
        AuditRecordIdentity {
            record_id: generate::<worlddb_core::AuditRecordId>()
                .map_err(|_| ProjectError::HostUnavailable)?,
            sequence: AuditSequence::new(1),
            audit_operation_id: generate::<worlddb_core::AuditOperationId>()
                .map_err(|_| ProjectError::HostUnavailable)?,
        },
        AuditRecordDetails {
            actor: creator,
            action: AuditAction::SecurityPolicyChange,
            object_class: AuditObjectClass::SecurityPolicy,
            outcome: AuditOutcome::Succeeded,
            commit_context: AuditCommitContext::Committed {
                revision,
                operation_id,
            },
            security_epoch: SecurityEpoch::INITIAL,
            policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                SecurityPolicySnapshot::default()
                    .effective_capability_fingerprint(creator, PolicyTarget::default())
                    .to_vec(),
            ))
            .map_err(|_| ProjectError::HostUnavailable)?,
        },
    );
    Ok(ProjectBootstrap {
        version,
        policy_record,
        audit_record,
        operation_id,
        metadata_records,
    })
}

fn resolve_role_symbol(policy: &SecurityPolicySnapshot, principal: PrincipalId) -> String {
    let target = PolicyTarget::default();
    policy
        .assignments()
        .iter()
        .filter(|assignment| {
            assignment.principal() == principal && assignment.scope().matches(target)
        })
        .find_map(|assignment| {
            policy
                .roles()
                .iter()
                .find(|role| role.id() == assignment.role())
                .map(|role| role.symbol().to_owned())
        })
        .unwrap_or_else(|| "custom".to_owned())
}

fn map_writer_lock_error(error: worlddb_storage_file::WriterLockError) -> ProjectError {
    match error {
        worlddb_storage_file::WriterLockError::AlreadyHeld => ProjectError::AlreadyOpen,
        worlddb_storage_file::WriterLockError::LockFileMissing
        | worlddb_storage_file::WriterLockError::Io(_) => ProjectError::HostUnavailable,
    }
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{
        Capability, PolicyTarget, PrincipalId, Revision, SchemaMode, SchemaRevision,
    };

    use super::{ProjectError, create_project, open_project};
    use crate::EngineHost;

    static NEXT_PROJECT: AtomicU64 = AtomicU64::new(0);

    fn principal(value: u8) -> Result<PrincipalId, String> {
        let text = format!("00000000-0000-7000-8000-{value:012x}");
        PrincipalId::from_str(&text).map_err(|error| error.to_string())
    }

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "worlddb-ode-project-{}-{}",
            std::process::id(),
            NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn bootstrap_persists_creator_roles_initial_grants_and_required_audit() -> Result<(), String> {
        let root = root();
        let creator = principal(1)?;
        let other = principal(2)?;
        let access = create_project(&root, creator).map_err(|error| error.to_string())?;
        assert_eq!(access.revision().value(), 1);
        assert_eq!(access.role_symbol(), "gm");
        assert!(access.database_id().to_string().len() == 36);

        let (engine, reopened) =
            EngineHost::open_authorized(&root, creator).map_err(|error| error.to_string())?;
        assert_eq!(engine.principal_id(), Some(creator));
        assert_eq!(reopened.database_id(), access.database_id());
        assert_eq!(reopened.revision(), access.revision());
        assert_eq!(reopened.role_symbol(), "gm");
        assert!(matches!(
            EngineHost::open_authorized(&root, creator),
            Err(ProjectError::AlreadyOpen)
        ));
        drop(engine);

        assert!(matches!(
            open_project(&root, other),
            Err(ProjectError::AccessDenied)
        ));
        let reopened = open_project(&root, creator).map_err(|error| error.to_string())?;
        assert_eq!(reopened.role_symbol(), "gm");

        let (verified, _) =
            EngineHost::open_authorized(&root, creator).map_err(|error| error.to_string())?;
        assert!(matches!(
            verified.health().map_err(|error| error.to_string())?,
            crate::Response::Health {
                writer_owned: true,
                ..
            }
        ));
        drop(verified);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn bootstrap_branch_and_layer_metadata_keep_historical_cutoffs() -> Result<(), String> {
        let root = root();
        let creator = principal(17)?;
        create_project(&root, creator).map_err(|error| error.to_string())?;
        let layout =
            worlddb_storage_file::DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager =
            worlddb_storage_file::FileProjectMetadataManager::open(layout, &lock, creator)
                .map_err(|error| error.to_string())?;

        let initial = manager
            .snapshot_at(SchemaMode::Current, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        assert_eq!(initial.history_spaces().definitions().len(), 1);
        assert_eq!(initial.layers().definitions().len(), 1);
        let root_space = initial.history_spaces().definitions()[0].history_space_id();
        let initial_base = initial.layers().base_layer_id();
        assert_eq!(
            initial
                .layers()
                .definition(initial_base)
                .map(|value| value.symbol().as_str()),
            Some("base")
        );

        let child_id = worlddb_core::storage_internal::generate_project_bootstrap_id::<
            worlddb_core::HistorySpaceId,
        >()
        .map_err(|error| error.to_string())?;
        let child = manager
            .create_child(
                manager.revision(),
                worlddb_core::storage_internal::generate_schema_management_operation_id()
                    .map_err(|error| error.to_string())?,
                child_id,
                root_space,
                Revision::FIRST_COMMIT,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(child.revision().value(), 2);

        let overlay_id = worlddb_core::storage_internal::generate_project_bootstrap_id::<
            worlddb_core::LayerId,
        >()
        .map_err(|error| error.to_string())?;
        let overlay = manager
            .create_layer(
                manager.revision(),
                worlddb_core::storage_internal::generate_schema_management_operation_id()
                    .map_err(|error| error.to_string())?,
                overlay_id,
                worlddb_core::Symbol::new("notes").map_err(|error| error.to_string())?,
                Some("Notizen und ergänzende Fakten".to_owned()),
                1,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(overlay.revision().value(), 3);

        let switched = manager
            .revise_layer(
                manager.revision(),
                worlddb_core::storage_internal::generate_schema_management_operation_id()
                    .map_err(|error| error.to_string())?,
                overlay_id,
                Some("Notizen und ergänzende Fakten".to_owned()),
                1,
                worlddb_core::Lifecycle::Active,
                overlay_id,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(switched.revision().value(), 4);

        let historical = manager
            .snapshot_at(
                SchemaMode::Explicit(SchemaRevision::from_published_revision(
                    Revision::FIRST_COMMIT,
                )),
                Revision::GENESIS,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(historical.layers().base_layer_id(), initial_base);
        let current = manager
            .snapshot_at(SchemaMode::Current, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        assert_eq!(current.layers().base_layer_id(), overlay_id);
        assert_eq!(
            current
                .history_spaces()
                .definition(child_id)
                .and_then(|value| value.parent_history_space_id()),
            Some(root_space)
        );
        assert_eq!(
            current
                .history_spaces()
                .definition(child_id)
                .map(|value| value.base_revision()),
            Some(Revision::FIRST_COMMIT)
        );

        let stale = manager.create_child(
            Revision::new(3).map_err(|error| error.to_string())?,
            worlddb_core::storage_internal::generate_schema_management_operation_id()
                .map_err(|error| error.to_string())?,
            worlddb_core::storage_internal::generate_project_bootstrap_id::<
                worlddb_core::HistorySpaceId,
            >()
            .map_err(|error| error.to_string())?,
            root_space,
            Revision::FIRST_COMMIT,
        );
        assert!(matches!(
            stale,
            Err(worlddb_storage_file::LayerManagementError::Conflict)
        ));
        assert_eq!(manager.revision().value(), 4);
        drop(lock);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn existing_destination_is_never_reinitialized() -> Result<(), String> {
        let root = root();
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let result = create_project(&root, principal(3)?);
        assert!(matches!(result, Err(ProjectError::AlreadyExists)));
        assert!(
            std::fs::read_dir(&root)
                .map_err(|error| error.to_string())?
                .next()
                .is_none()
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn opening_requires_the_current_principal_and_rejects_malformed_storage() -> Result<(), String>
    {
        let root = root();
        let creator = principal(4)?;
        let other = principal(5)?;
        create_project(&root, creator).map_err(|error| error.to_string())?;
        let access = open_project(&root, creator).map_err(|error| error.to_string())?;
        assert!(access.role_symbol().eq_ignore_ascii_case("gm"));
        assert!(matches!(
            open_project(&root, other),
            Err(ProjectError::AccessDenied)
        ));

        let _ = std::fs::remove_dir_all(root);

        let malformed_root = std::env::temp_dir().join(format!(
            "worlddb-ode-malformed-{}-{}",
            std::process::id(),
            NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&malformed_root).map_err(|error| error.to_string())?;
        let before = std::fs::read_dir(&malformed_root)
            .map_err(|error| error.to_string())?
            .count();
        assert!(matches!(
            open_project(&malformed_root, creator),
            Err(ProjectError::InvalidProject)
        ));
        let after = std::fs::read_dir(&malformed_root)
            .map_err(|error| error.to_string())?
            .count();
        assert_eq!(
            before, after,
            "opening a malformed folder must not mutate it"
        );
        let _ = std::fs::remove_dir_all(malformed_root);
        Ok(())
    }

    #[test]
    fn creator_policy_is_role_based_and_does_not_grant_raw_admin() -> Result<(), String> {
        let root = root();
        let creator = principal(6)?;
        create_project(&root, creator).map_err(|error| error.to_string())?;
        let layout =
            worlddb_storage_file::DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let manifest = worlddb_storage_file::ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "manifest missing".to_owned())?;
        let segment_ids = manifest
            .segments()
            .iter()
            .filter(|segment| {
                segment.kind() == worlddb_storage_file::ManifestSegmentKind::SecurityPolicy
            })
            .map(|segment| segment.id())
            .collect::<Vec<_>>();
        let history = worlddb_storage_file::SecurityPolicyHistoryStore::new(layout)
            .load_history(manifest.revision(), &segment_ids)
            .map_err(|error| error.to_string())?;
        let policy = history
            .policy()
            .latest_version()
            .map_err(|error| error.to_string())?
            .snapshot();
        assert_eq!(policy.roles().len(), 2);
        assert!(policy.roles().iter().any(|role| role.symbol() == "gm"));
        assert!(policy.roles().iter().any(|role| role.symbol() == "player"));
        assert_eq!(
            policy.authorize(creator, Capability::ProjectRead, PolicyTarget::default()),
            worlddb_core::AuthorizationDecision::Allow
        );
        assert_eq!(
            policy.authorize(creator, Capability::AdminRawRead, PolicyTarget::default()),
            worlddb_core::AuthorizationDecision::Deny
        );
        drop(lock);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn opening_never_repairs_a_project_with_an_incomplete_wal_tail() -> Result<(), String> {
        use std::fs::OpenOptions;

        let root = root();
        let creator = principal(8)?;
        create_project(&root, creator).map_err(|error| error.to_string())?;
        let layout =
            worlddb_storage_file::DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let wal_path = std::fs::read_dir(layout.wal_directory())
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|extension| extension == "wal"))
            .ok_or_else(|| String::from("project WAL segment missing"))?;
        let file = OpenOptions::new()
            .write(true)
            .open(&wal_path)
            .map_err(|error| error.to_string())?;
        let length = file.metadata().map_err(|error| error.to_string())?.len();
        file.set_len(length.saturating_sub(1))
            .map_err(|error| error.to_string())?;
        drop(file);
        let damaged_bytes = std::fs::read(&wal_path).map_err(|error| error.to_string())?;

        assert!(matches!(
            open_project(&root, creator),
            Err(ProjectError::RecoveryRequired)
        ));
        assert_eq!(
            std::fs::read(&wal_path).map_err(|error| error.to_string())?,
            damaged_bytes,
            "open must not repair or rewrite the incomplete WAL tail"
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn opening_a_directory_junction_is_rejected() -> Result<(), String> {
        use std::process::Command;

        let target = root();
        let creator = principal(7)?;
        create_project(&target, creator).map_err(|error| error.to_string())?;
        let junction = target.with_file_name(format!(
            "worlddb-ode-junction-{}-{}",
            std::process::id(),
            NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
        ));
        let junction_result = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .map_err(|error| error.to_string())?;
        if !junction_result.status.success() {
            let _ = std::fs::remove_dir_all(&target);
            return Err(String::from_utf8_lossy(&junction_result.stderr).into_owned());
        }

        let result = open_project(&junction, creator);
        let _ = std::fs::remove_dir(&junction);
        let _ = std::fs::remove_dir_all(&target);
        assert!(matches!(result, Err(ProjectError::InvalidProject)));
        Ok(())
    }
}
