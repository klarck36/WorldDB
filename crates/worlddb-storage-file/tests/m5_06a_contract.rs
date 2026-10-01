use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AssertionId, AuditRetentionPolicy, Capability, CapabilityGrant, CapabilityRule, DomainId,
    EventAttributeId, EventKindId, EventRoleId, EvidenceRelationship, FieldSelector, GrantEffect,
    HistorySpaceId, LayerId, PolicyBundle, PolicyScope, PolicySubject, Principal, PrincipalId,
    PrincipalState, ProvenanceRelationship, RecordRef, Revision, RoleAssignment, RoleAssignmentId,
    RoleDefinition, RoleId, SecurityEpoch, SecurityPolicyChange, SecurityPolicyHistoryError,
    SecurityPolicyRecord, SecurityPolicyRecordId, SecurityPolicySnapshot, SecurityPolicyVersion,
    Symbol,
};
use worlddb_storage_file::{
    DatabaseLayout, SecurityPolicyHistoryStore, SecurityPolicyStorageError,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root =
            env::temp_dir().join(format!("worlddb-m5-06a-{}-{sequence}", std::process::id()));
        DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn layout(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::open(&self.0).map_err(|error| error.to_string())
    }
}

impl Drop for TempDatabase {
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

fn policy(disabled_state: PrincipalState) -> Result<SecurityPolicySnapshot, String> {
    let owner = id::<PrincipalId>(1)?;
    let disabled = id::<PrincipalId>(2)?;
    let role_id = id::<RoleId>(3)?;
    let role = RoleDefinition::new(
        role_id,
        "reviewer_1",
        PolicyBundle::from_grants([
            CapabilityGrant::new(Capability::ProjectRead, GrantEffect::Allow),
            CapabilityGrant::new(Capability::AuditExport, GrantEffect::Deny),
        ])
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let scope = PolicyScope::new(
        Some(id::<HistorySpaceId>(4)?),
        Some(id::<LayerId>(5)?),
        Some(RecordRef::Assertion(id::<AssertionId>(6)?)),
        Some(FieldSelector::EventParticipant(
            id::<EventKindId>(7)?,
            id::<EventRoleId>(8)?,
        )),
        Some(worlddb_core::RelationshipSelector::Evidence(
            EvidenceRelationship::Documents,
        )),
    );
    let assignment = RoleAssignment::new(id::<RoleAssignmentId>(9)?, owner, role_id, scope);
    let direct_rule = CapabilityRule::new(
        id::<worlddb_core::PolicyRuleId>(10)?,
        PolicySubject::Principal(owner),
        CapabilityGrant::new(Capability::SchemaRead, GrantEffect::Allow),
        PolicyScope::new(
            None,
            None,
            None,
            Some(FieldSelector::EventAttribute(
                id::<EventKindId>(11)?,
                id::<EventAttributeId>(12)?,
            )),
            Some(worlddb_core::RelationshipSelector::Provenance(
                ProvenanceRelationship::DerivedFrom,
            )),
        ),
    );
    SecurityPolicySnapshot::new(
        vec![
            Principal::new(owner),
            Principal::new(disabled).with_state(disabled_state),
        ],
        vec![role],
        vec![assignment],
        vec![direct_rule],
    )
    .map_err(|error| error.to_string())
}

fn version(revision: u64, epoch: u64) -> Result<SecurityPolicyVersion, String> {
    version_with_state(
        revision,
        epoch,
        if epoch == 0 {
            PrincipalState::Disabled
        } else {
            PrincipalState::Retired
        },
    )
}

fn version_with_state(
    revision: u64,
    epoch: u64,
    principal_state: PrincipalState,
) -> Result<SecurityPolicyVersion, String> {
    Ok(SecurityPolicyVersion::new(
        Revision::try_from(revision).map_err(|error| error.to_string())?,
        SecurityEpoch::new(epoch),
        policy(principal_state)?,
    ))
}

fn principal_state_change_record() -> Result<SecurityPolicyRecord, String> {
    SecurityPolicyRecord::new(
        id::<SecurityPolicyRecordId>(13)?,
        Revision::try_from(2).map_err(|error| error.to_string())?,
        id::<PrincipalId>(1)?,
        SecurityEpoch::new(1),
        vec![SecurityPolicyChange::PrincipalStateChanged {
            principal_id: id::<PrincipalId>(2)?,
            state: PrincipalState::Retired,
        }],
    )
    .map_err(|error| error.to_string())
}

#[test]
fn complete_security_snapshot_epoch_and_audit_configuration_roundtrip() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = SecurityPolicyHistoryStore::new(layout);
    let version = version(0, 0)?;
    let retention =
        AuditRetentionPolicy::new(86_400_000, 2_048).map_err(|error| error.to_string())?;

    let receipt = store
        .write_version(&lock, &version, None, Some(retention))
        .map_err(|error| error.to_string())?;
    let restored = store
        .read_version(receipt.id())
        .map_err(|error| error.to_string())?;

    assert_eq!(restored.version(), &version);
    assert_eq!(restored.audit_retention(), Some(retention));
    assert_eq!(restored.content_digest(), receipt.content_digest());
    assert_eq!(restored.version().epoch(), SecurityEpoch::INITIAL);
    assert_eq!(restored.version().snapshot().principals().len(), 2);
    assert_eq!(restored.version().snapshot().roles().len(), 1);
    let restored_role = restored
        .version()
        .snapshot()
        .roles()
        .first()
        .ok_or_else(|| String::from("readback has no security role"))?;
    assert_eq!(restored_role.bundle().len(), 2);
    Ok(())
}

#[test]
fn complete_closed_capability_catalog_roundtrips_with_stable_tags() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = SecurityPolicyHistoryStore::new(layout);
    let principal_id = id::<PrincipalId>(31)?;
    let role_id = id::<RoleId>(32)?;
    let bundle = PolicyBundle::from_grants(
        Capability::ALL
            .into_iter()
            .map(|capability| CapabilityGrant::new(capability, GrantEffect::Allow)),
    )
    .map_err(|error| error.to_string())?;
    let role =
        RoleDefinition::new(role_id, "catalog", bundle).map_err(|error| error.to_string())?;
    let snapshot = SecurityPolicySnapshot::new(
        vec![Principal::new(principal_id)],
        vec![role],
        vec![],
        vec![],
    )
    .map_err(|error| error.to_string())?;
    let version = SecurityPolicyVersion::new(Revision::GENESIS, SecurityEpoch::INITIAL, snapshot);
    let receipt = store
        .write_version(&lock, &version, None, None)
        .map_err(|error| error.to_string())?;
    let restored = store
        .read_version(receipt.id())
        .map_err(|error| error.to_string())?;
    assert_eq!(restored.version(), &version);
    let restored_role = restored
        .version()
        .snapshot()
        .roles()
        .first()
        .ok_or_else(|| String::from("readback has no catalog role"))?;
    assert_eq!(restored_role.bundle().len(), Capability::ALL.len());
    Ok(())
}

#[test]
fn all_typed_policy_change_variants_roundtrip_without_domain_record_encoding() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = SecurityPolicyHistoryStore::new(layout);
    let owner = id::<PrincipalId>(41)?;
    let role = id::<RoleId>(42)?;
    let assignment = id::<RoleAssignmentId>(43)?;
    let rule = id::<worlddb_core::PolicyRuleId>(44)?;
    let record = SecurityPolicyRecord::new(
        id::<SecurityPolicyRecordId>(45)?,
        Revision::GENESIS,
        owner,
        SecurityEpoch::INITIAL,
        vec![
            SecurityPolicyChange::PrincipalRegistered {
                principal_id: owner,
            },
            SecurityPolicyChange::PrincipalStateChanged {
                principal_id: owner,
                state: PrincipalState::Disabled,
            },
            SecurityPolicyChange::RoleRegistered {
                role_id: role,
                symbol: Symbol::new("editor").map_err(|error| error.to_string())?,
            },
            SecurityPolicyChange::RoleRetired { role_id: role },
            SecurityPolicyChange::RoleAssigned {
                assignment_id: assignment,
                principal_id: owner,
                role_id: role,
                scope: PolicyScope::project(),
            },
            SecurityPolicyChange::RoleAssignmentRevoked {
                assignment_id: assignment,
            },
            SecurityPolicyChange::CapabilityRuleAdded {
                rule_id: rule,
                subject: PolicySubject::Role(role),
                capability: Capability::ProjectRead,
                effect: GrantEffect::Allow,
                scope: PolicyScope::project(),
            },
            SecurityPolicyChange::CapabilityRuleRevoked { rule_id: rule },
        ],
    )
    .map_err(|error| error.to_string())?;
    let version = version(0, 0)?;

    let receipt = store
        .write_version(&lock, &version, Some(&record), None)
        .map_err(|error| error.to_string())?;
    let restored = store
        .read_version(receipt.id())
        .map_err(|error| error.to_string())?;
    assert_eq!(restored.policy_record(), Some(&record));
    Ok(())
}

#[test]
fn history_reconstruction_fails_closed_when_any_revision_snapshot_is_missing() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = SecurityPolicyHistoryStore::new(layout);
    let retention0 = AuditRetentionPolicy::new(60_000, 512).map_err(|error| error.to_string())?;
    let retention1 = AuditRetentionPolicy::new(120_000, 512).map_err(|error| error.to_string())?;

    let genesis = store
        .write_version(&lock, &version(0, 0)?, None, Some(retention0))
        .map_err(|error| error.to_string())?;
    let first = store
        .write_version(&lock, &version(1, 0)?, None, Some(retention1))
        .map_err(|error| error.to_string())?;
    let policy_record = principal_state_change_record()?;
    let second = store
        .write_version(&lock, &version(2, 1)?, Some(&policy_record), None)
        .map_err(|error| error.to_string())?;
    let committed = Revision::try_from(2).map_err(|error| error.to_string())?;

    let complete = store
        .load_history(committed, &[second.id(), genesis.id(), first.id()])
        .map_err(|error| error.to_string())?;
    assert_eq!(complete.policy().committed_revision(), committed);
    assert_eq!(
        complete
            .policy_record_at(committed)
            .map_err(|error| error.to_string())?,
        Some(&policy_record)
    );
    assert_eq!(
        complete
            .audit_retention_at(Revision::GENESIS)
            .map_err(|error| error.to_string())?,
        Some(retention0)
    );
    assert_eq!(
        complete
            .audit_retention_at(Revision::try_from(1).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?,
        Some(retention1)
    );
    assert_eq!(
        complete
            .audit_retention_at(committed)
            .map_err(|error| error.to_string())?,
        None
    );

    assert!(matches!(
        store.load_history(committed, &[genesis.id(), second.id()]),
        Err(SecurityPolicyStorageError::History(
            SecurityPolicyHistoryError::IncompleteRevisionCoverage
        ))
    ));
    Ok(())
}

#[test]
fn an_epoch_advance_requires_a_policy_record_bound_to_the_same_revision() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = SecurityPolicyHistoryStore::new(layout);
    let genesis = store
        .write_version(&lock, &version(0, 0)?, None, None)
        .map_err(|error| error.to_string())?;
    let changed = version(1, 1)?;
    let missing_record = store
        .write_version(&lock, &changed, None, None)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        store.load_history(
            Revision::try_from(1).map_err(|error| error.to_string())?,
            &[genesis.id(), missing_record.id()]
        ),
        Err(SecurityPolicyStorageError::MissingPolicyRecord)
    ));

    let wrong_revision_record = principal_state_change_record()?;
    assert!(matches!(
        store.write_version(&lock, &changed, Some(&wrong_revision_record), None),
        Err(SecurityPolicyStorageError::PolicyRecordVersionMismatch)
    ));
    Ok(())
}

#[test]
fn a_persistent_policy_change_without_epoch_advance_fails_closed() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = SecurityPolicyHistoryStore::new(layout);
    let genesis = store
        .write_version(&lock, &version(0, 0)?, None, None)
        .map_err(|error| error.to_string())?;
    let changed_without_epoch = store
        .write_version(
            &lock,
            &version_with_state(1, 0, PrincipalState::Retired)?,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        store.load_history(
            Revision::try_from(1).map_err(|error| error.to_string())?,
            &[genesis.id(), changed_without_epoch.id()]
        ),
        Err(SecurityPolicyStorageError::SnapshotChangedWithoutEpochAdvance)
    ));
    Ok(())
}

#[test]
fn an_empty_or_missing_genesis_snapshot_does_not_open_policy_history() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let store = SecurityPolicyHistoryStore::new(layout);
    assert!(matches!(
        store.load_history(Revision::GENESIS, &[]),
        Err(SecurityPolicyStorageError::History(
            SecurityPolicyHistoryError::MissingInitialSnapshot
        ))
    ));
    Ok(())
}
