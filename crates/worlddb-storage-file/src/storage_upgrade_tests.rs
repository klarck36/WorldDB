use super::{
    CurrentPointerVersion, JOURNAL_RECORD_BYTES, JournalPhase, StorageUpgradeBudget,
    StorageUpgradeError, StorageUpgradeFaultPoint, StorageUpgradeManager, UpgradeExecutionRequest,
    inspect_source_locked, profile_fingerprint, validate_recovery_phase,
};
use crate::{
    ContentDigest, DatabaseLayout, ManifestSnapshot, ManifestStore, StorageVerifier, WalPrepareLog,
    manifest::CurrentPointer, manifest::ManifestError, manifest::encode_current_v2,
};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worlddb_core::{
    AuthorizationMode, Capability, CapabilityGrant, CapabilityRule, DomainId, GrantEffect,
    OperationId, PolicyRuleId, PolicyScope, PolicySubject, Principal, PrincipalId, Revision,
    SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestArea(PathBuf);

impl TestArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("worlddb-m7-10b-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn id<T: DomainId>(tail: u8) -> Result<T, String> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).map_err(|error| error.to_string())
}

fn policy() -> Result<SecurityPolicyHistory, String> {
    policy_for(&[
        Capability::BackupCreate,
        Capability::BackupRestore,
        Capability::StorageFormatUpgrade,
    ])
}

fn policy_for(capabilities: &[Capability]) -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(1)?;
    let rules = capabilities
        .iter()
        .copied()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(u8::try_from(index + 2).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(|error| error.to_string())?;
    SecurityPolicyHistory::new(
        Revision::GENESIS,
        vec![SecurityPolicyVersion::new(
            Revision::GENESIS,
            SecurityEpoch::INITIAL,
            snapshot,
        )],
    )
    .map_err(|error| error.to_string())
}

fn view(history: &SecurityPolicyHistory) -> Result<worlddb_core::SecurityPolicyView<'_>, String> {
    history
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(1)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())
}

fn request<'a>(
    layout: &'a DatabaseLayout,
    plan: &'a super::StorageUpgradePlan,
    proof: &'a super::StorageUpgradeSafeRestorePoint,
    action: &'a super::StorageUpgradeAdminAction,
    permissions: &'a SecurityPolicyHistory,
) -> Result<UpgradeExecutionRequest<'a>, String> {
    Ok(UpgradeExecutionRequest {
        layout,
        plan,
        restore_point: proof,
        action,
        policy: view(permissions)?,
        policy_target: worlddb_core::PolicyTarget::default(),
    })
}

fn current_version(layout: &DatabaseLayout) -> Result<Option<u8>, String> {
    if !layout.current_file().exists() {
        return Ok(None);
    }
    let bytes = fs::read(layout.current_file()).map_err(|error| error.to_string())?;
    if bytes.get(..6) != Some(&b"WDBCUR"[..]) {
        return Err("invalid CURRENT magic".to_owned());
    }
    Ok(bytes.get(7).copied())
}

fn assert_source_or_target_is_clean(layout: &DatabaseLayout) -> Result<(), String> {
    let reopened = DatabaseLayout::open(layout.root()).map_err(|error| error.to_string())?;
    let lock = reopened
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let report = StorageVerifier::new(reopened.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    assert!(report.is_clean(), "{report}");
    let _ = ManifestStore::new(reopened)
        .read_current()
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn install_v1_manifest(layout: &DatabaseLayout) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(layout);
    wal.commit_operation(&lock, id::<OperationId>(20)?, b"upgrade-source")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(Revision::FIRST_COMMIT, vec![])
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn v2_genesis_pointer_is_valid_only_for_the_recognized_profile() -> Result<(), String> {
    let area = TestArea::create()?;
    let layout = DatabaseLayout::create(area.path("genesis")).map_err(|error| error.to_string())?;
    let store = ManifestStore::new(layout.clone());
    let valid = encode_current_v2(CurrentPointer {
        generation: 0,
        manifest_digest: ContentDigest::from_bytes([0; 32]),
        version: CurrentPointerVersion::V2,
        profile_fingerprint: profile_fingerprint(
            CurrentPointerVersion::V2,
            layout.format_capabilities(),
        ),
    })
    .map_err(|error| error.to_string())?;
    fs::write(layout.current_file(), valid).map_err(|error| error.to_string())?;
    assert_eq!(
        store.read_current().map_err(|error| error.to_string())?,
        None
    );

    let invalid = encode_current_v2(CurrentPointer {
        generation: 0,
        manifest_digest: ContentDigest::from_bytes([0; 32]),
        version: CurrentPointerVersion::V2,
        profile_fingerprint: [0x51; 32],
    })
    .map_err(|error| error.to_string())?;
    fs::write(layout.current_file(), invalid).map_err(|error| error.to_string())?;
    let before = fs::read(layout.current_file()).map_err(|error| error.to_string())?;
    assert!(matches!(
        store.read_current(),
        Err(ManifestError::InvalidManifest)
    ));
    assert_eq!(
        fs::read(layout.current_file()).map_err(|error| error.to_string())?,
        before
    );
    Ok(())
}

#[test]
fn current_upgrade_reopens_at_source_or_target_and_resumes_every_publish_boundary()
-> Result<(), String> {
    let area = TestArea::create()?;
    let layout = DatabaseLayout::create(area.path("source")).map_err(|error| error.to_string())?;
    install_v1_manifest(&layout)?;
    let source_id = layout
        .database_id()
        .ok_or("source has no database identity")?;
    let original_manifest = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or("source manifest missing")?;
    let manager = StorageUpgradeManager::new();
    let plan = manager
        .prepare(&layout, StorageUpgradeBudget::current_pointer_v1_to_v2())
        .map_err(|error| error.to_string())?;
    for phase in [JournalPhase::Prepared, JournalPhase::Staged] {
        assert!(validate_recovery_phase(*plan.source_profile(), &plan, phase).is_ok());
    }
    for phase in [JournalPhase::Published, JournalPhase::Complete] {
        assert!(matches!(
            validate_recovery_phase(*plan.source_profile(), &plan, phase),
            Err(StorageUpgradeError::InvalidJournal)
        ));
    }
    assert!(matches!(
        validate_recovery_phase(*plan.target_profile(), &plan, JournalPhase::Prepared),
        Err(StorageUpgradeError::InvalidJournal)
    ));
    for phase in [
        JournalPhase::Staged,
        JournalPhase::Published,
        JournalPhase::Complete,
    ] {
        assert!(validate_recovery_phase(*plan.target_profile(), &plan, phase).is_ok());
    }
    let permissions = policy()?;
    let proof = manager
        .create_safe_restore_point(
            &layout,
            &plan,
            super::StorageUpgradeRestoreTargets::new(&area.path("backup"), &area.path("restored")),
            None,
            view(&permissions)?,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
    let action = manager
        .confirm(
            &plan,
            &proof,
            view(&permissions)?,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;

    let unexpected = layout.root().join("unexpected-component");
    fs::write(&unexpected, b"added after preflight").map_err(|error| error.to_string())?;
    assert!(matches!(
        manager.execute(
            &layout,
            &plan,
            &proof,
            &action,
            view(&permissions)?,
            worlddb_core::PolicyTarget::default(),
        ),
        Err(StorageUpgradeError::SourceBindingMismatch)
    ));
    fs::remove_file(unexpected).map_err(|error| error.to_string())?;

    let no_upgrade_permission = policy_for(&[Capability::BackupCreate, Capability::BackupRestore])?;
    assert!(matches!(
        manager.execute(
            &layout,
            &plan,
            &proof,
            &action,
            view(&no_upgrade_permission)?,
            worlddb_core::PolicyTarget::default(),
        ),
        Err(StorageUpgradeError::AuthorizationDenied {
            capability: Capability::StorageFormatUpgrade
        })
    ));
    assert_eq!(current_version(&layout)?, Some(1));

    let corrupted_action = manager
        .confirm(
            &plan,
            &proof,
            view(&permissions)?,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
    let corrupt_interrupted = manager.execute_inner(
        request(&layout, &plan, &proof, &corrupted_action, &permissions)?,
        Some(StorageUpgradeFaultPoint::AfterPrepared),
        false,
    );
    assert!(matches!(
        corrupt_interrupted,
        Err(StorageUpgradeError::InjectedFailure {
            point: StorageUpgradeFaultPoint::AfterPrepared,
            ..
        })
    ));
    let corrupt_journal = layout.staging_directory().join(format!(
        "storage-upgrade-{}.journal",
        corrupted_action.run_id()
    ));
    let mut corrupt_bytes = fs::read(&corrupt_journal).map_err(|error| error.to_string())?;
    let last = corrupt_bytes
        .last_mut()
        .ok_or("prepared journal is empty")?;
    *last ^= 1;
    fs::write(&corrupt_journal, corrupt_bytes).map_err(|error| error.to_string())?;
    let corrupt_resume = manager.resume(
        &layout,
        &plan,
        &proof,
        &corrupted_action,
        view(&permissions)?,
        worlddb_core::PolicyTarget::default(),
    );
    assert!(matches!(
        corrupt_resume,
        Err(StorageUpgradeError::InvalidJournal)
    ));
    assert_eq!(current_version(&layout)?, Some(1));

    {
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let (source, profile, _, _) =
            inspect_source_locked(&layout, &lock).map_err(|error| error.to_string())?;
        assert_eq!(
            source, plan.source,
            "source binding changed after restore proof"
        );
        assert_eq!(
            profile, plan.source_profile,
            "source profile changed after restore proof"
        );
    }

    let interrupted = manager.execute_inner(
        request(&layout, &plan, &proof, &action, &permissions)?,
        Some(StorageUpgradeFaultPoint::BeforeJournal),
        false,
    );
    assert!(
        matches!(
            &interrupted,
            Err(StorageUpgradeError::InjectedFailure {
                point: StorageUpgradeFaultPoint::BeforeJournal,
                ..
            })
        ),
        "{interrupted:?}"
    );
    assert_eq!(current_version(&layout)?, Some(1));
    assert_source_or_target_is_clean(&layout)?;

    let interrupted = manager.execute_inner(
        request(&layout, &plan, &proof, &action, &permissions)?,
        Some(StorageUpgradeFaultPoint::AfterPrepared),
        false,
    );
    assert!(matches!(
        interrupted,
        Err(StorageUpgradeError::InjectedFailure {
            point: StorageUpgradeFaultPoint::AfterPrepared,
            ..
        })
    ));
    assert_eq!(current_version(&layout)?, Some(1));
    assert_source_or_target_is_clean(&layout)?;

    let journal_path = layout
        .staging_directory()
        .join(format!("storage-upgrade-{}.journal", action.run_id()));
    let mut torn_tail = fs::OpenOptions::new()
        .append(true)
        .open(&journal_path)
        .map_err(|error| error.to_string())?;
    torn_tail
        .write_all(b"torn")
        .map_err(|error| error.to_string())?;
    drop(torn_tail);

    for point in [
        StorageUpgradeFaultPoint::AfterStaged,
        StorageUpgradeFaultPoint::BeforeCurrentPublish,
    ] {
        let interrupted = manager.execute_inner(
            request(&layout, &plan, &proof, &action, &permissions)?,
            Some(point),
            true,
        );
        assert!(matches!(
            interrupted,
            Err(StorageUpgradeError::InjectedFailure { .. })
        ));
        assert_eq!(current_version(&layout)?, Some(1));
        assert_source_or_target_is_clean(&layout)?;
    }

    let interrupted = manager.execute_inner(
        request(&layout, &plan, &proof, &action, &permissions)?,
        Some(StorageUpgradeFaultPoint::AfterCurrentReplace),
        true,
    );
    assert!(matches!(
        interrupted,
        Err(StorageUpgradeError::InjectedFailure {
            point: StorageUpgradeFaultPoint::AfterCurrentReplace,
            ..
        })
    ));
    assert_eq!(current_version(&layout)?, Some(2));
    assert_source_or_target_is_clean(&layout)?;

    let receipt = manager
        .resume(
            &layout,
            &plan,
            &proof,
            &action,
            view(&permissions)?,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
    assert!(receipt.resumed());
    assert_eq!(current_version(&layout)?, Some(2));
    assert_eq!(layout.database_id(), Some(source_id));
    assert_eq!(
        fs::metadata(layout.current_file())
            .map_err(|error| error.to_string())?
            .len(),
        112
    );
    assert_eq!(
        fs::metadata(
            layout
                .staging_directory()
                .join(format!("storage-upgrade-{}.journal", action.run_id()))
        )
        .map_err(|error| error.to_string())?
        .len(),
        u64::try_from(JOURNAL_RECORD_BYTES * 4).map_err(|error| error.to_string())?
    );
    assert_source_or_target_is_clean(&layout)?;

    let upgraded_manifest = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or("upgraded manifest missing")?;
    assert_eq!(
        upgraded_manifest.generation(),
        original_manifest.generation()
    );
    assert_eq!(upgraded_manifest.revision(), original_manifest.revision());
    assert_eq!(
        upgraded_manifest.commit_hash(),
        original_manifest.commit_hash()
    );
    assert_eq!(
        upgraded_manifest.content_digest(),
        original_manifest.content_digest()
    );

    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let next_revision = original_manifest
        .revision()
        .next_commit()
        .map_err(|error| error.to_string())?;
    wal.commit_operation(&lock, id::<OperationId>(21)?, b"post-upgrade-write")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(next_revision, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(current_version(&layout)?, Some(2));
    assert_eq!(
        ManifestStore::new(layout)
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| manifest.revision()),
        Some(next_revision)
    );
    Ok(())
}
