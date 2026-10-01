use std::env;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    DomainId, HistorySpaceDefinition, HistorySpaceId, LayerDefinition, LayerId, Lifecycle,
    OperationId, Record, Revision, SchemaRevision, SecurityEpoch, SecurityPolicySnapshot,
    SecurityPolicyVersion, Symbol, decode_record, encode_record,
};
use worlddb_storage_file::{
    DatabaseLayout, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
    ManifestSnapshot, ManifestStore, RecoveryDisposition, SecurityPolicyHistoryStore,
    StorageDamageClass, StorageVerifier, StorageVerifyAction, StorageVerifyIssue, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("worlddb-m5-16-{}-{sequence}", std::process::id()));
        DatabaseLayout::create(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
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

fn operation_id(tail: u8) -> Result<OperationId, String> {
    OperationId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        tail,
    ])
    .map_err(|error| error.to_string())
}

fn wal_segment_path(layout: &DatabaseLayout, sequence: u64) -> PathBuf {
    layout
        .wal_directory()
        .join(format!("segment-{sequence:020}.wal"))
}

fn domain_id<T: DomainId>(tail: u8) -> Result<T, String> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).map_err(|error| error.to_string())
}

fn install_history_manifest(
    layout: &DatabaseLayout,
    lock: &worlddb_storage_file::WriterLock,
    operation_tail: u8,
    history_tail: u8,
) -> Result<ManifestSegmentReference, String> {
    let record = Record::HistorySpaceDefinition(
        HistorySpaceDefinition::new(
            domain_id::<HistorySpaceId>(history_tail)?,
            None,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?,
    );
    let frame = encode_record(&record).map_err(|error| error.to_string())?;
    let decoded = decode_record(&frame).map_err(|error| error.to_string())?;
    let segment = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(lock, &[decoded])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        segment.id(),
        segment.content_digest(),
        Revision::FIRST_COMMIT,
    );
    let references = vec![reference];
    let wal = WalPrepareLog::new(layout);
    let receipt = wal
        .commit_manifest_snapshot(lock, operation_id(operation_tail)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    let snapshot =
        ManifestSnapshot::new(receipt.revision(), references).map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(lock, &wal, snapshot)
        .map_err(|error| error.to_string())?;
    Ok(reference)
}

fn storage_report(
    layout: &DatabaseLayout,
) -> Result<worlddb_storage_file::StorageVerifyReport, String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())
}

fn assert_actions(
    report: &worlddb_storage_file::StorageVerifyReport,
    class: StorageDamageClass,
) -> Result<(), String> {
    let finding = report
        .findings()
        .iter()
        .find(|finding| finding.class() == class)
        .ok_or_else(|| format!("missing damage class {class:?} in {report}"))?;
    assert!(
        finding
            .safe_next_actions()
            .contains(&StorageVerifyAction::PreserveOriginal)
    );
    assert!(
        finding
            .safe_next_actions()
            .contains(&StorageVerifyAction::KeepReadOnly)
    );
    Ok(())
}

#[test]
fn clean_database_independently_checks_wal_operation_index_and_reports_safe_revision()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;

    let report = storage_report(&layout)?;

    assert!(report.is_clean());
    assert_eq!(report.safe_revision(), Revision::GENESIS);
    assert_eq!(report.disposition(), RecoveryDisposition::Clean);
    assert_eq!(report.inventory().committed_wal_frames(), 0);
    assert_eq!(report.inventory().operation_id_entries(), Some(0));
    assert_eq!(report.inventory().manifest_segments(), None);
    Ok(())
}

#[test]
fn bitflip_reports_prior_safe_revision_and_preserves_original_bytes() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = WalPrepareLog::new(&layout)
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    let path = wal_segment_path(&layout, receipt.reference().segment_sequence());
    let mut bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let last = bytes
        .last_mut()
        .ok_or_else(|| "WAL segment unexpectedly empty".to_owned())?;
    *last ^= 0x80;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let damaged_bytes = fs::read(&path).map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), Revision::GENESIS);
    assert_actions(&report, StorageDamageClass::Bitflip)?;
    assert_eq!(
        fs::read(path).map_err(|error| error.to_string())?,
        damaged_bytes
    );
    Ok(())
}

#[test]
fn manifest_verify_binds_segment_id_separately_from_its_content_digest() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let reference = install_history_manifest(&layout, &lock, 10, 10)?;
    let path = layout.segments_directory().join(format!(
        "segment-{}.wdbseg",
        reference.id().to_canonical_string()
    ));
    let mut bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let identity_tail = bytes
        .get_mut(23)
        .ok_or_else(|| "segment envelope has no complete SegmentId".to_owned())?;
    *identity_tail ^= 0x01;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let damaged_bytes = fs::read(&path).map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), Revision::FIRST_COMMIT);
    assert_actions(&report, StorageDamageClass::Bitflip)?;
    assert!(report.findings().iter().any(|finding| matches!(
        finding.issue(),
        StorageVerifyIssue::Recovery(
            worlddb_storage_file::RecoveryFinding::ReferencedSegmentCorrupt {
                kind: ManifestSegmentKind::History,
                id,
            }
        ) if *id == reference.id()
    )));
    assert_eq!(
        fs::read(path).map_err(|error| error.to_string())?,
        damaged_bytes
    );
    Ok(())
}

#[test]
fn manifest_verify_binds_content_digest_separately_from_segment_id() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let reference = install_history_manifest(&layout, &lock, 11, 11)?;
    let path = layout.segments_directory().join(format!(
        "segment-{}.wdbseg",
        reference.id().to_canonical_string()
    ));
    let mut bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let digest_byte = bytes
        .get_mut(24)
        .ok_or_else(|| "segment envelope has no content digest".to_owned())?;
    *digest_byte ^= 0x01;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let damaged_bytes = fs::read(&path).map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), Revision::FIRST_COMMIT);
    assert_actions(&report, StorageDamageClass::Bitflip)?;
    assert!(report.findings().iter().any(|finding| matches!(
        finding.issue(),
        StorageVerifyIssue::Recovery(
            worlddb_storage_file::RecoveryFinding::ReferencedSegmentCorrupt {
                kind: ManifestSegmentKind::History,
                id,
            }
        ) if *id == reference.id()
    )));
    assert_eq!(
        fs::read(path).map_err(|error| error.to_string())?,
        damaged_bytes
    );
    Ok(())
}

#[test]
fn truncated_wal_tail_reports_only_the_complete_prefix_and_journaled_recovery() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = WalPrepareLog::new(&layout)
        .commit_operation(&lock, operation_id(2)?, b"committed")
        .map_err(|error| error.to_string())?;
    let path = wal_segment_path(&layout, receipt.reference().segment_sequence());
    let file = OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    file.set_len(len.saturating_sub(1))
        .map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), Revision::GENESIS);
    assert_eq!(report.disposition(), RecoveryDisposition::RecoveryRequired);
    assert_actions(&report, StorageDamageClass::Truncation)?;
    let finding = report
        .findings()
        .iter()
        .find(|finding| finding.class() == StorageDamageClass::Truncation)
        .ok_or_else(|| "truncated tail finding missing".to_owned())?;
    assert!(
        finding
            .safe_next_actions()
            .contains(&StorageVerifyAction::RunJournaledTailRecovery)
    );
    Ok(())
}

#[test]
fn reordered_wal_frames_are_reported_as_reorder() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = WalPrepareLog::new(&layout)
        .commit_operation(&lock, operation_id(3)?, b"committed")
        .map_err(|error| error.to_string())?;
    let reference = receipt.reference();
    let path = wal_segment_path(&layout, reference.segment_sequence());
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let prepare_start =
        usize::try_from(reference.byte_offset()).map_err(|error| error.to_string())?;
    let prepare_end = usize::try_from(reference.byte_offset() + reference.frame_length())
        .map_err(|error| error.to_string())?;
    let marker_start =
        usize::try_from(receipt.marker_offset()).map_err(|error| error.to_string())?;
    let marker_end = usize::try_from(receipt.marker_offset() + receipt.marker_length())
        .map_err(|error| error.to_string())?;
    let prepare = bytes
        .get(prepare_start..prepare_end)
        .ok_or_else(|| "prepare frame range outside WAL segment".to_owned())?;
    let marker = bytes
        .get(marker_start..marker_end)
        .ok_or_else(|| "commit marker range outside WAL segment".to_owned())?;
    let mut reordered = Vec::with_capacity(bytes.len());
    reordered.extend_from_slice(marker);
    reordered.extend_from_slice(prepare);
    fs::write(&path, reordered).map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), Revision::GENESIS);
    assert_actions(&report, StorageDamageClass::Reorder)?;
    assert!(report.findings().iter().any(|finding| matches!(
        finding.issue(),
        StorageVerifyIssue::Recovery(worlddb_storage_file::RecoveryFinding::SafeCorruption {
            kind: worlddb_storage_file::RecoveryCorruptionKind::CommitMarkerWithoutPrepare,
            ..
        })
    )));
    Ok(())
}

#[test]
fn duplicate_operation_frame_is_reported_without_advancing_the_safe_prefix() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = WalPrepareLog::new(&layout)
        .commit_operation(&lock, operation_id(4)?, b"committed")
        .map_err(|error| error.to_string())?;
    let reference = receipt.reference();
    let path = wal_segment_path(&layout, reference.segment_sequence());
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let prepare_start =
        usize::try_from(reference.byte_offset()).map_err(|error| error.to_string())?;
    let prepare_end = usize::try_from(reference.byte_offset() + reference.frame_length())
        .map_err(|error| error.to_string())?;
    let prepare = bytes
        .get(prepare_start..prepare_end)
        .ok_or_else(|| "prepare frame range outside WAL segment".to_owned())?
        .to_vec();
    let mut duplicated = bytes;
    duplicated.extend_from_slice(&prepare);
    fs::write(&path, duplicated).map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), receipt.revision());
    assert_actions(&report, StorageDamageClass::DuplicateFrame)?;
    Ok(())
}

#[test]
fn checksum_valid_semantic_invalidity_is_reported_as_such() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;

    // A replay snapshot declaring a segment from a revision newer than its
    // enclosing commit. The WAL frame and commit hashes are valid.
    let mut payload = b"WDBRPL\0\x01".to_vec();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.push(1);
    payload.extend_from_slice(&2_u64.to_le_bytes());
    payload.extend_from_slice(&[
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x4c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        0xcd,
    ]);
    payload.extend_from_slice(&[0x55; 32]);
    let receipt = WalPrepareLog::new(&layout)
        .commit_operation(&lock, operation_id(5)?, &payload)
        .map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), receipt.revision());
    assert_actions(&report, StorageDamageClass::SemanticInvalidity)?;
    assert!(report.findings().iter().any(|finding| matches!(
        finding.issue(),
        StorageVerifyIssue::Recovery(
            worlddb_storage_file::RecoveryFinding::CommittedReplayPayloadCorrupt { .. }
        )
    )));
    Ok(())
}

#[test]
fn valid_manifest_schema_and_capability_inventories_are_checked_independently() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let mut references = Vec::new();

    let layer_id = LayerId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        0x01,
    ])
    .map_err(|error| error.to_string())?;
    let genesis_layer = LayerDefinition::new(
        layer_id,
        Symbol::new("base").map_err(|error| error.to_string())?,
        None,
        0,
        Lifecycle::Active,
        SchemaRevision::from_published_revision(Revision::GENESIS),
    );
    let genesis_frame = encode_record(&Record::LayerDefinition(genesis_layer))
        .map_err(|error| error.to_string())?;
    let genesis_decoded = decode_record(&genesis_frame).map_err(|error| error.to_string())?;
    let layer = LayerDefinition::new(
        layer_id,
        Symbol::new("base").map_err(|error| error.to_string())?,
        None,
        0,
        Lifecycle::Active,
        SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
    );
    let frame =
        encode_record(&Record::LayerDefinition(layer)).map_err(|error| error.to_string())?;
    let decoded = decode_record(&frame).map_err(|error| error.to_string())?;
    let history_receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[genesis_decoded, decoded])
        .map_err(|error| error.to_string())?;
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        history_receipt.id(),
        history_receipt.content_digest(),
        Revision::FIRST_COMMIT,
    ));

    let security_store = SecurityPolicyHistoryStore::new(layout.clone());
    let policy = SecurityPolicySnapshot::new(vec![], vec![], vec![], vec![])
        .map_err(|error| error.to_string())?;
    for revision in [Revision::GENESIS, Revision::FIRST_COMMIT] {
        let receipt = security_store
            .write_version(
                &lock,
                &SecurityPolicyVersion::new(revision, SecurityEpoch::INITIAL, policy.clone()),
                None,
                None,
            )
            .map_err(|error| error.to_string())?;
        references.push(ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            receipt.id(),
            receipt.content_digest(),
            revision,
        ));
    }

    let receipt = WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(6)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    let snapshot =
        ManifestSnapshot::new(receipt.revision(), references).map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(&lock, &WalPrepareLog::new(&layout), snapshot)
        .map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.safe_revision(), Revision::FIRST_COMMIT);
    assert_eq!(report.inventory().manifest_segments(), Some(3));
    assert_eq!(report.inventory().history_segments(), 1);
    assert_eq!(report.inventory().security_policy_segments(), 2);
    assert_eq!(report.inventory().operation_id_entries(), Some(1));
    assert_eq!(report.inventory().schema_definitions(), 2);
    assert_eq!(report.inventory().schema_publication_revisions(), 1);
    assert_eq!(report.inventory().capability_versions(), 2);
    Ok(())
}

#[test]
fn checksum_valid_duplicate_schema_symbol_is_reported_as_semantic_invalidity() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let history = HistorySegmentStore::new(layout.clone());
    let mut decoded = Vec::new();
    for tail in [0x11, 0x12] {
        let layer_id = LayerId::try_from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89,
            0xab, tail,
        ])
        .map_err(|error| error.to_string())?;
        let layer = LayerDefinition::new(
            layer_id,
            Symbol::new("duplicate").map_err(|error| error.to_string())?,
            None,
            0,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
        );
        let frame =
            encode_record(&Record::LayerDefinition(layer)).map_err(|error| error.to_string())?;
        decoded.push(decode_record(&frame).map_err(|error| error.to_string())?);
    }
    let history_receipt = history
        .write_decoded_segment(&lock, &decoded)
        .map_err(|error| error.to_string())?;
    let mut references = vec![ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        history_receipt.id(),
        history_receipt.content_digest(),
        Revision::FIRST_COMMIT,
    )];
    let security_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(
            &lock,
            &SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                SecurityPolicySnapshot::new(vec![], vec![], vec![], vec![])
                    .map_err(|error| error.to_string())?,
            ),
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        security_receipt.id(),
        security_receipt.content_digest(),
        Revision::GENESIS,
    ));
    let receipt = WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(7)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &WalPrepareLog::new(&layout),
            ManifestSnapshot::new(receipt.revision(), references)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(report.safe_revision(), Revision::FIRST_COMMIT);
    assert_actions(&report, StorageDamageClass::SemanticInvalidity)?;
    assert!(report.findings().iter().any(|finding| matches!(
        finding.issue(),
        StorageVerifyIssue::SchemaHistoryInvalid { .. }
    )));
    Ok(())
}

#[test]
fn checksum_valid_capability_history_gap_is_reported_as_semantic_invalidity() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    wal.commit_operation(&lock, operation_id(8)?, b"revision one")
        .map_err(|error| error.to_string())?;

    let policy = SecurityPolicySnapshot::new(vec![], vec![], vec![], vec![])
        .map_err(|error| error.to_string())?;
    let security_store = SecurityPolicyHistoryStore::new(layout.clone());
    let mut references = Vec::new();
    for revision in [
        Revision::GENESIS,
        Revision::try_from(2).map_err(|error| error.to_string())?,
    ] {
        let receipt = security_store
            .write_version(
                &lock,
                &SecurityPolicyVersion::new(revision, SecurityEpoch::INITIAL, policy.clone()),
                None,
                None,
            )
            .map_err(|error| error.to_string())?;
        references.push(ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            receipt.id(),
            receipt.content_digest(),
            revision,
        ));
    }
    let receipt = wal
        .commit_manifest_snapshot(&lock, operation_id(9)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(receipt.revision(), references)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    drop(lock);

    let report = storage_report(&layout)?;

    assert_eq!(
        report.safe_revision(),
        Revision::try_from(2).map_err(|error| error.to_string())?
    );
    assert_actions(&report, StorageDamageClass::SemanticInvalidity)?;
    assert!(report.findings().iter().any(|finding| matches!(
        finding.issue(),
        StorageVerifyIssue::CapabilityHistoryInvalid { .. }
    )));
    Ok(())
}
