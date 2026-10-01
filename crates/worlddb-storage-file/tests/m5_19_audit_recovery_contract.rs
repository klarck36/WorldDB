use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditRetentionPolicy, AuditScopeFingerprint, AuthorizationMode, Bytes, CancellationToken,
    Capability, CapabilityGrant, CapabilityRule, ClientRequestId, DomainId, EpistemicMode,
    FrameHeader, GrantEffect, HistoricalQueryBinding, HistorySpaceId, LayerDefinition, LayerId,
    LayerSchemaSnapshot, LayerSelection, Lifecycle, PageOrdinal, PerspectiveScope, PolicyRuleId,
    PolicyScope, PolicySubject, PolicyTarget, Principal, PrincipalId, QueryBudget,
    QueryBudgetLimits, QueryContext, QueryContextInput, RawReadAttempt, RawReadAttemptIdentity,
    RawReadAttemptScope, RecordedAsOf, Revision, SchemaDefinition, SchemaHistoryReferenceModel,
    SchemaMode, SchemaRevision, SecurityContext, SecurityEpoch, SecurityPolicyHistory,
    SecurityPolicySnapshot, SecurityPolicyVersion, SnapshotId, SnapshotRef, TlvEncoder,
    ValidatedLayerSelection, WorldTimeSelector, encode_frame,
};
use worlddb_storage_file::{
    DatabaseLayout, RawReadAuditAccessError, RawReadAuditError, RawReadAuditPolicyError,
    RawReadAuditWal,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-19-{}-{sequence}", std::process::id()));
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

fn context() -> Result<QueryContext, String> {
    let revision = Revision::new(1).map_err(|error| error.to_string())?;
    let schema_revision = SchemaRevision::from_published_revision(revision);
    let layer_id = id::<LayerId>(1)?;
    let layer = LayerDefinition::new(
        layer_id,
        worlddb_core::Symbol::new("world").map_err(|error| error.to_string())?,
        None,
        0,
        Lifecycle::Active,
        schema_revision,
    );
    let schema = LayerSchemaSnapshot::new(schema_revision, vec![layer], layer_id)
        .map_err(|error| error.to_string())?;
    let layers = ValidatedLayerSelection::resolve(&schema, LayerSelection::BaseOnly)
        .map_err(|error| error.to_string())?;
    let as_of = RecordedAsOf::from_published_revision(revision);
    let mut schema_history = SchemaHistoryReferenceModel::new();
    schema_history
        .publish(revision, vec![SchemaDefinition::LayerSnapshot(schema)])
        .map_err(|error| error.to_string())?;
    let schema_binding =
        HistoricalQueryBinding::bind(&schema_history, as_of, SchemaMode::Historical)
            .map_err(|error| error.to_string())?;
    let budget = QueryBudget::new(
        10,
        100,
        100,
        QueryBudgetLimits::new(100, 1_000, 1_000).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    QueryContext::new(QueryContextInput {
        snapshot: SnapshotRef::new(id::<SnapshotId>(2)?),
        snapshot_revision: revision,
        recorded_as_of: as_of,
        history_space: id::<HistorySpaceId>(3)?,
        layers,
        world_time: WorldTimeSelector::AllTimes,
        perspective: PerspectiveScope::World,
        epistemic_mode: EpistemicMode::WorldState,
        schema_binding,
        security: SecurityContext::new(id::<PrincipalId>(4)?, AuthorizationMode::Now),
        budget,
        cancellation: CancellationToken::new(),
    })
    .map_err(|error| error.to_string())
}

fn policy_history(capabilities: &[Capability]) -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(4)?;
    let rules = capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            let rule_id =
                id::<PolicyRuleId>(u8::try_from(index + 1).map_err(|error| error.to_string())?)?;
            Ok(CapabilityRule::new(
                rule_id,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(*capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(|error| error.to_string())?;
    let genesis = Revision::new(0).map_err(|error| error.to_string())?;
    let committed = Revision::new(1).map_err(|error| error.to_string())?;
    SecurityPolicyHistory::new(
        committed,
        vec![
            SecurityPolicyVersion::new(genesis, SecurityEpoch::INITIAL, snapshot.clone()),
            SecurityPolicyVersion::new(committed, SecurityEpoch::INITIAL, snapshot),
        ],
    )
    .map_err(|error| error.to_string())
}

fn scope_fingerprint() -> Result<AuditScopeFingerprint, String> {
    AuditScopeFingerprint::new(Bytes::new(vec![0x19, 0x55, 0xab]))
        .map_err(|error| error.to_string())
}

fn uncommitted_prepare(sequence: u64) -> Result<Vec<u8>, String> {
    let audit_operation_id = id::<worlddb_core::AuditOperationId>(52)?;
    let attempt = RawReadAttempt::new(
        RawReadAttemptIdentity {
            record_id: id::<worlddb_core::AuditRecordId>(51)?,
            sequence: worlddb_core::AuditSequence::new(sequence),
            audit_operation_id,
            client_request_id: id::<ClientRequestId>(53)?,
        },
        RawReadAttemptScope {
            principal_id: id::<PrincipalId>(4)?,
            scope_fingerprint: scope_fingerprint()?,
            snapshot_id: id::<SnapshotId>(2)?,
            security_epoch: SecurityEpoch::INITIAL,
            page_ordinal: PageOrdinal::new(9),
        },
    );
    let encoded_attempt =
        worlddb_core::encode_raw_read_attempt(&attempt).map_err(|error| error.to_string())?;
    let mut fields = TlvEncoder::new();
    fields
        .push(1, &sequence.to_le_bytes())
        .map_err(|error| error.to_string())?;
    fields
        .push(2, audit_operation_id.as_bytes())
        .map_err(|error| error.to_string())?;
    fields
        .push(3, &encoded_attempt)
        .map_err(|error| error.to_string())?;
    encode_frame(FrameHeader::new(0x5744_4150), &fields.finish()).map_err(|error| error.to_string())
}

#[test]
fn startup_quarantines_a_complete_uncommitted_prepare_before_opening_the_writer()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(0),
            },
            id::<ClientRequestId>(50)?,
        )
        .map_err(|error| error.to_string())?;
    drop(writer);

    let tail = uncommitted_prepare(2)?;
    let wal_path = layout.audit_wal_directory().join("raw-read.wal");
    let mut wal = fs::OpenOptions::new()
        .append(true)
        .open(&wal_path)
        .map_err(|error| error.to_string())?;
    wal.write_all(&tail).map_err(|error| error.to_string())?;
    wal.sync_all().map_err(|error| error.to_string())?;
    drop(wal);

    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    assert_eq!(
        writer
            .head()
            .map_err(|error| error.to_string())?
            .sequence()
            .value(),
        1
    );
    let quarantine = layout.audit_wal_directory().join("quarantine");
    let mut preserved_tail_found = false;
    for entry in fs::read_dir(&quarantine).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("bin")
        {
            let bytes = fs::read(entry.path()).map_err(|error| error.to_string())?;
            preserved_tail_found |= bytes == tail;
        }
    }
    assert!(preserved_tail_found);
    drop(writer);
    let query = context()?;
    let history = policy_history(&[Capability::AuditRead])?;
    let policy = history.resolve(&query).map_err(|error| error.to_string())?;
    assert_eq!(
        audit
            .read_authorized(policy, PolicyTarget::default())
            .map_err(|error| error.to_string())?
            .1
            .len(),
        1
    );

    let reopened = audit.try_writer().map_err(|error| error.to_string())?;
    let next = reopened
        .append_attempt(
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(1),
            },
            id::<ClientRequestId>(54)?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(next.sequence().value(), 2);
    assert!(!layout.current_file().exists());
    assert!(
        fs::read_dir(layout.wal_directory())
            .map_err(|error| error.to_string())?
            .next()
            .is_none()
    );
    Ok(())
}

#[test]
fn committed_prefix_corruption_is_read_only_and_byte_preserving() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(0),
            },
            id::<ClientRequestId>(55)?,
        )
        .map_err(|error| error.to_string())?;
    drop(writer);

    let wal_path = layout.audit_wal_directory().join("raw-read.wal");
    let mut bytes = fs::read(&wal_path).map_err(|error| error.to_string())?;
    let last = bytes
        .last_mut()
        .ok_or_else(|| String::from("committed audit WAL is unexpectedly empty"))?;
    *last ^= 0x80;
    fs::write(&wal_path, &bytes).map_err(|error| error.to_string())?;
    let before = fs::read(&wal_path).map_err(|error| error.to_string())?;

    assert!(matches!(
        audit.try_writer(),
        Err(RawReadAuditError::Frame(_))
    ));
    let query = context()?;
    let history = policy_history(&[Capability::AuditRead])?;
    let policy = history.resolve(&query).map_err(|error| error.to_string())?;
    assert!(matches!(
        audit.read_authorized(policy, PolicyTarget::default()),
        Err(RawReadAuditAccessError::Audit(RawReadAuditError::Frame(_)))
    ));
    assert!(matches!(audit.recover(), Err(RawReadAuditError::Frame(_))));
    assert_eq!(
        fs::read(&wal_path).map_err(|error| error.to_string())?,
        before
    );
    assert!(!layout.audit_wal_directory().join("quarantine").exists());
    Ok(())
}

#[test]
fn startup_finishes_an_interrupted_journaled_tail_recovery() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(0),
            },
            id::<ClientRequestId>(58)?,
        )
        .map_err(|error| error.to_string())?;
    drop(writer);

    let tail = uncommitted_prepare(2)?;
    let wal_path = layout.audit_wal_directory().join("raw-read.wal");
    let mut wal = fs::OpenOptions::new()
        .append(true)
        .open(&wal_path)
        .map_err(|error| error.to_string())?;
    wal.write_all(&tail).map_err(|error| error.to_string())?;
    wal.sync_all().map_err(|error| error.to_string())?;
    drop(wal);

    // Create a valid recovery intent and quarantine copy, then reconstruct the durable state
    // after intent publication but before truncation/Done publication.
    audit.recover().map_err(|error| error.to_string())?;
    let quarantine_directory = layout.audit_wal_directory().join("quarantine");
    let intent = fs::read_dir(&quarantine_directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("intent"))
        .ok_or_else(|| String::from("recovery intent was not persisted"))?;
    let done = intent.with_extension("done");
    if !done.is_file() {
        return Err(String::from("completed recovery marker was not persisted"));
    }
    let mut wal = fs::OpenOptions::new()
        .append(true)
        .open(&wal_path)
        .map_err(|error| error.to_string())?;
    wal.write_all(&tail).map_err(|error| error.to_string())?;
    wal.sync_all().map_err(|error| error.to_string())?;
    drop(wal);
    fs::remove_file(&done).map_err(|error| error.to_string())?;

    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    assert_eq!(
        writer
            .head()
            .map_err(|error| error.to_string())?
            .sequence()
            .value(),
        1
    );
    assert!(done.is_file());
    drop(writer);
    let query = context()?;
    let history = policy_history(&[Capability::AuditRead])?;
    let policy = history.resolve(&query).map_err(|error| error.to_string())?;
    assert_eq!(
        audit
            .read_authorized(policy, PolicyTarget::default())
            .map_err(|error| error.to_string())?
            .1
            .len(),
        1
    );
    Ok(())
}

#[test]
fn audit_read_export_and_retention_configuration_require_distinct_current_capabilities()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(0),
            },
            id::<ClientRequestId>(56)?,
        )
        .map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(1),
            },
            id::<ClientRequestId>(57)?,
        )
        .map_err(|error| error.to_string())?;
    drop(writer);

    let query = context()?;
    let read_only = policy_history(&[Capability::AuditRead])?;
    let read_view = read_only
        .resolve(&query)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        audit
            .read_authorized(read_view, PolicyTarget::default())
            .map_err(|error| error.to_string())?
            .1
            .len(),
        2
    );
    assert!(matches!(
        audit.export_authorized(read_view, PolicyTarget::default()),
        Err(RawReadAuditAccessError::ExportUnauthorized)
    ));

    let export_only = policy_history(&[Capability::AuditExport])?;
    let export_view = export_only
        .resolve(&query)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        audit.export_authorized(export_view, PolicyTarget::default()),
        Err(RawReadAuditAccessError::ReadUnauthorized)
    ));

    let read_and_export = policy_history(&[Capability::AuditRead, Capability::AuditExport])?;
    let read_and_export_view = read_and_export
        .resolve(&query)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        audit
            .export_authorized(read_and_export_view, PolicyTarget::default())
            .map_err(|error| error.to_string())?
            .1
            .len(),
        2
    );

    let config_only = policy_history(&[Capability::AuditConfigure])?;
    let config_view = config_only
        .resolve(&query)
        .map_err(|error| error.to_string())?;
    let long_retention =
        AuditRetentionPolicy::new(86_400_000, 10).map_err(|error| error.to_string())?;
    assert!(
        audit
            .validate_retention_change(config_view, None, long_retention)
            .is_ok()
    );
    let shorter_retention =
        AuditRetentionPolicy::new(43_200_000, 10).map_err(|error| error.to_string())?;
    assert!(matches!(
        audit.validate_retention_change(config_view, Some(long_retention), shorter_retention),
        Err(RawReadAuditPolicyError::MinimumRetentionReduced { .. })
    ));
    let maximum_below_current =
        AuditRetentionPolicy::new(86_400_000, 1).map_err(|error| error.to_string())?;
    assert!(matches!(
        audit.validate_retention_change(config_view, Some(long_retention), maximum_below_current),
        Err(RawReadAuditPolicyError::MaximumRecordsBelowCurrentCount {
            retained_records: 2,
            proposed_maximum: 1,
        })
    ));
    let no_config = policy_history(&[Capability::AuditRead])?;
    let no_config_view = no_config
        .resolve(&query)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        audit.validate_retention_change(no_config_view, None, long_retention),
        Err(RawReadAuditPolicyError::ConfigureUnauthorized)
    ));
    Ok(())
}
