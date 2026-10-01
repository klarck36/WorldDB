use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AdminRawError, AuditScopeFingerprint, AuthorizationMode, Bytes, CancellationToken, Capability,
    CapabilityGrant, CapabilityRule, ClientRequestId, DomainId, EpistemicMode, GrantEffect,
    HistoricalQueryBinding, HistorySpaceId, LayerDefinition, LayerId, LayerSchemaSnapshot,
    LayerSelection, Lifecycle, PageOrdinal, PerspectiveScope, PolicyRuleId, PolicyScope,
    PolicySubject, PolicyTarget, Principal, PrincipalId, QueryBudget, QueryBudgetLimits,
    QueryContext, QueryContextInput, RecordedAsOf, Revision, SchemaDefinition,
    SchemaHistoryReferenceModel, SchemaMode, SchemaRevision, SecurityContext, SecurityEpoch,
    SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion, SnapshotId, SnapshotRef,
    ValidatedLayerSelection, WorldTimeSelector, release_admin_raw_page,
};
use worlddb_storage_file::{
    DatabaseLayout, RawReadAuditError, RawReadAuditRecoveryDisposition, RawReadAuditWal,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-18-{}-{sequence}", std::process::id()));
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

fn policies() -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(4)?;
    let capabilities = [
        Capability::HistorySpaceRead,
        Capability::RawHistoryRead,
        Capability::AdminRawRead,
        Capability::AuditRead,
        Capability::AuditExport,
    ];
    let rules = capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(u8::try_from(index + 1).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(*capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(|error| error.to_string())?;
    let revision_zero = Revision::new(0).map_err(|error| error.to_string())?;
    let revision_one = Revision::new(1).map_err(|error| error.to_string())?;
    SecurityPolicyHistory::new(
        revision_one,
        vec![
            SecurityPolicyVersion::new(revision_zero, SecurityEpoch::INITIAL, snapshot.clone()),
            SecurityPolicyVersion::new(revision_one, SecurityEpoch::INITIAL, snapshot),
        ],
    )
    .map_err(|error| error.to_string())
}

fn scope_fingerprint() -> Result<AuditScopeFingerprint, String> {
    AuditScopeFingerprint::new(Bytes::new(vec![0x32, 0x18, 0x7a]))
        .map_err(|error| error.to_string())
}

#[test]
fn durable_raw_pages_keep_retry_identity_and_use_an_independent_audit_sequence()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let _data_writer = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    assert!(matches!(
        audit.try_writer(),
        Err(RawReadAuditError::WriterAlreadyHeld)
    ));
    let context = context()?;
    let policies = policies()?;
    let client_request_id = id::<ClientRequestId>(8)?;

    let first = release_admin_raw_page::<u8>(
        &context,
        &policies,
        scope_fingerprint()?,
        PageOrdinal::new(0),
        client_request_id,
        Vec::new(),
        &writer,
    )
    .map_err(|error| error.to_string())?;
    let second = release_admin_raw_page::<u8>(
        &context,
        &policies,
        scope_fingerprint()?,
        PageOrdinal::new(1),
        client_request_id,
        Vec::new(),
        &writer,
    )
    .map_err(|error| error.to_string())?;

    assert_eq!(
        first.query_context_binding().snapshot().id(),
        id::<SnapshotId>(2)?
    );
    assert_eq!(
        second.query_context_binding().snapshot().id(),
        id::<SnapshotId>(2)?
    );
    assert!(!layout.current_file().exists());
    assert!(
        fs::read_dir(layout.wal_directory())
            .map_err(|error| error.to_string())?
            .next()
            .is_none()
    );
    drop(writer);
    let policy = policies
        .resolve(&context)
        .map_err(|error| error.to_string())?;
    let attempts = audit
        .read_authorized(policy, PolicyTarget::default())
        .map_err(|error| error.to_string())?
        .1;
    assert_eq!(attempts.len(), 2);
    let first_attempt = attempts
        .first()
        .ok_or_else(|| String::from("first raw-read audit attempt is missing"))?;
    let second_attempt = attempts
        .get(1)
        .ok_or_else(|| String::from("retry raw-read audit attempt is missing"))?;
    assert_eq!(first_attempt.attempt().sequence().value(), 1);
    assert_eq!(second_attempt.attempt().sequence().value(), 2);
    assert_ne!(
        first_attempt.attempt().audit_operation_id(),
        second_attempt.attempt().audit_operation_id()
    );
    for attempt in [first_attempt.attempt(), second_attempt.attempt()] {
        assert_eq!(attempt.client_request_id(), client_request_id);
        assert_eq!(attempt.principal_id(), id::<PrincipalId>(4)?);
        assert_eq!(attempt.scope_fingerprint(), &scope_fingerprint()?);
        assert_eq!(attempt.snapshot_id(), id::<SnapshotId>(2)?);
        assert_eq!(attempt.security_epoch(), SecurityEpoch::INITIAL);
    }
    assert_eq!(first_attempt.attempt().page_ordinal(), PageOrdinal::new(0));
    assert_eq!(second_attempt.attempt().page_ordinal(), PageOrdinal::new(1));

    // A committed attempt records that a page was authorized; it has no field that claims
    // bytes were actually delivered. A process crash immediately after this sync is representable.
    let policy = policies
        .resolve(&context)
        .map_err(|error| error.to_string())?;
    let (head, recovered) = audit
        .export_authorized(policy, PolicyTarget::default())
        .map_err(|error| error.to_string())?;
    assert_eq!(head.sequence().value(), 2);
    assert_eq!(recovered, attempts);
    assert!(!layout.current_file().exists());
    Ok(())
}

#[test]
fn raw_read_audit_child_stops_after_durable_attempt() -> Result<(), String> {
    let Some(root) = env::var_os("WORLDDB_M5_18_CHILD_ROOT") else {
        return Ok(());
    };
    let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
    let writer = RawReadAuditWal::new(layout)
        .try_writer()
        .map_err(|error| error.to_string())?;
    writer
        .append_attempt(
            worlddb_core::RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: scope_fingerprint()?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(5),
            },
            id::<ClientRequestId>(11)?,
        )
        .map_err(|error| error.to_string())?;
    // Model process loss in the interval after the durable attempt and before any page handoff.
    std::process::exit(73);
}

#[test]
fn process_exit_after_attempt_sync_recovers_an_attempt_without_claimed_output() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let child = Command::new(executable)
        .arg("--exact")
        .arg("raw_read_audit_child_stops_after_durable_attempt")
        .arg("--nocapture")
        .env("WORLDDB_M5_18_CHILD_ROOT", &database.0)
        .status()
        .map_err(|error| error.to_string())?;
    assert_eq!(child.code(), Some(73));

    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let query = context()?;
    let policies = policies()?;
    let policy = policies
        .resolve(&query)
        .map_err(|error| error.to_string())?;
    let (head, attempts) = audit
        .read_authorized(policy, PolicyTarget::default())
        .map_err(|error| error.to_string())?;
    assert_eq!(head.sequence().value(), 1);
    assert_eq!(attempts.len(), 1);
    let attempt = attempts
        .first()
        .ok_or_else(|| String::from("durable child attempt is missing"))?
        .attempt();
    assert_eq!(attempt.client_request_id(), id::<ClientRequestId>(11)?);
    assert_eq!(attempt.page_ordinal(), PageOrdinal::new(5));
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
fn raw_audit_failure_blocks_the_admin_page_before_release() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    fs::create_dir(layout.audit_wal_directory().join("raw-read.wal"))
        .map_err(|error| error.to_string())?;

    let result = release_admin_raw_page::<u8>(
        &context()?,
        &policies()?,
        scope_fingerprint()?,
        PageOrdinal::new(0),
        id::<ClientRequestId>(9)?,
        Vec::new(),
        &writer,
    );
    assert!(matches!(result, Err(AdminRawError::AuditAuthorization(_))));
    assert!(matches!(
        writer.head(),
        Err(RawReadAuditError::WriterPoisoned)
    ));
    assert!(!layout.current_file().exists());
    Ok(())
}

#[test]
fn partial_audit_tail_is_never_silently_treated_as_a_committed_attempt() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let audit = RawReadAuditWal::new(layout.clone());
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    let client_request_id = id::<ClientRequestId>(10)?;
    let scope = worlddb_core::RawReadAttemptScope {
        principal_id: id::<PrincipalId>(4)?,
        scope_fingerprint: scope_fingerprint()?,
        snapshot_id: id::<SnapshotId>(2)?,
        security_epoch: SecurityEpoch::INITIAL,
        page_ordinal: PageOrdinal::new(0),
    };
    writer
        .append_attempt(scope, client_request_id)
        .map_err(|error| error.to_string())?;
    drop(writer);

    let mut wal_file = fs::OpenOptions::new()
        .append(true)
        .open(layout.audit_wal_directory().join("raw-read.wal"))
        .map_err(|error| error.to_string())?;
    wal_file
        .write_all(&[0x57, 0x44, 0x42])
        .map_err(|error| error.to_string())?;
    wal_file.sync_all().map_err(|error| error.to_string())?;
    drop(wal_file);

    let recovery = audit.recover().map_err(|error| error.to_string())?;
    assert_eq!(
        recovery.disposition(),
        RawReadAuditRecoveryDisposition::RecoveredIncompleteTail
    );
    assert_eq!(recovery.head().sequence().value(), 1);
    assert_eq!(recovery.discarded_tail_bytes(), 3);
    let writer = audit.try_writer().map_err(|error| error.to_string())?;
    assert_eq!(
        writer
            .head()
            .map_err(|error| error.to_string())?
            .sequence()
            .value(),
        1
    );
    drop(writer);
    let query = context()?;
    let policies = policies()?;
    let policy = policies
        .resolve(&query)
        .map_err(|error| error.to_string())?;
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
