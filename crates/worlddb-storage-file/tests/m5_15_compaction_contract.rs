use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    DomainId, HistorySpaceDefinition, HistorySpaceId, OperationId, Record, Revision, SecurityEpoch,
    SecurityPolicySnapshot, SecurityPolicyVersion, encode_decoded_record,
};
use worlddb_storage_file::{
    CompactionManager, DatabaseLayout, HistorySegmentStore, ManifestSegmentKind,
    ManifestSegmentReference, ManifestSnapshot, ManifestStore, RecoveryManager,
    SecurityPolicyHistoryStore, SegmentPinKind, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-15-{}-{sequence}", std::process::id()));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn layout(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::create(self.0.join("database")).map_err(|error| error.to_string())
    }
}

impl Drop for TempArea {
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

fn operation_id(tail: u8) -> Result<OperationId, String> {
    OperationId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        tail,
    ])
    .map_err(|error| error.to_string())
}

fn history_record(tail: u8) -> Result<Record, String> {
    Ok(Record::HistorySpaceDefinition(
        HistorySpaceDefinition::new(id::<HistorySpaceId>(tail)?, None, Revision::GENESIS)
            .map_err(|error| error.to_string())?,
    ))
}

fn install_inventory(
    layout: &DatabaseLayout,
    lock: &worlddb_storage_file::WriterLock,
) -> Result<Vec<ManifestSegmentReference>, String> {
    let history_store = HistorySegmentStore::new(layout.clone());
    let mut references = Vec::new();
    for tail in 1..=4 {
        let record = history_record(tail)?;
        let flags = if tail == 1 { 0x8000_0000_0000_0001 } else { 0 };
        let frame = worlddb_core::encode_record_with_flags(&record, flags)
            .map_err(|error| error.to_string())?;
        let decoded = worlddb_core::decode_record(&frame).map_err(|error| error.to_string())?;
        let receipt = history_store
            .write_decoded_segment(lock, &[decoded])
            .map_err(|error| error.to_string())?;
        references.push(ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            receipt.id(),
            receipt.content_digest(),
            Revision::FIRST_COMMIT,
        ));
    }

    let security_version = SecurityPolicyVersion::new(
        Revision::GENESIS,
        SecurityEpoch::INITIAL,
        SecurityPolicySnapshot::new(vec![], vec![], vec![], vec![])
            .map_err(|error| error.to_string())?,
    );
    let security_receipt = SecurityPolicyHistoryStore::new(layout.clone())
        .write_version(lock, &security_version, None, None)
        .map_err(|error| error.to_string())?;
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        security_receipt.id(),
        security_receipt.content_digest(),
        Revision::GENESIS,
    ));

    ManifestSnapshot::new(Revision::FIRST_COMMIT, references.clone())
        .map_err(|error| error.to_string())?;
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(lock, operation_id(21)?, references.clone(), &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(lock)
        .map_err(|error| error.to_string())?;
    Ok(references)
}

fn segment_path(layout: &DatabaseLayout, reference: ManifestSegmentReference) -> PathBuf {
    layout.segments_directory().join(format!(
        "segment-{}.wdbseg",
        reference.id().to_canonical_string()
    ))
}

fn history_frames(
    layout: &DatabaseLayout,
    references: &[ManifestSegmentReference],
) -> Result<Vec<Vec<u8>>, String> {
    let store = HistorySegmentStore::new(layout.clone());
    let mut frames = Vec::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::History)
    {
        let segment = store
            .read_segment(reference.id())
            .map_err(|error| error.to_string())?;
        if segment.content_digest() != reference.content_digest() {
            return Err("manifest reference digest differs from its history segment".to_owned());
        }
        for record in segment.records() {
            frames.push(encode_decoded_record(record).map_err(|error| error.to_string())?);
        }
    }
    frames.sort_unstable();
    Ok(frames)
}

fn history_references(references: &[ManifestSegmentReference]) -> Vec<ManifestSegmentReference> {
    references
        .iter()
        .copied()
        .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        .collect()
}

fn contains_reference(
    references: &[ManifestSegmentReference],
    expected: ManifestSegmentReference,
) -> bool {
    references.contains(&expected)
}

fn assert_security_reference_preserved(
    layout: &DatabaseLayout,
    expected: ManifestSegmentReference,
) -> Result<(), String> {
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "compaction removed CURRENT".to_owned())?;
    assert!(contains_reference(current.segments(), expected));
    assert_eq!(
        current
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
            .count(),
        1
    );
    Ok(())
}

#[test]
fn each_pin_kind_protects_replaced_segments_until_the_pin_is_dropped() -> Result<(), String> {
    for (case, kind) in [
        SegmentPinKind::Snapshot,
        SegmentPinKind::Backup,
        SegmentPinKind::RecoveryCheckpoint,
        SegmentPinKind::Export,
    ]
    .into_iter()
    .enumerate()
    {
        let area = TempArea::create()?;
        let layout = area.layout()?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let inventory = install_inventory(&layout, &lock)?;
        let inputs = history_references(&inventory);
        let protected = *inputs
            .first()
            .ok_or_else(|| "fixture has no history input".to_owned())?;
        let security = *inventory
            .iter()
            .find(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
            .ok_or_else(|| "fixture has no security-policy input".to_owned())?;
        let before_frames = history_frames(&layout, &inventory)?;
        let pin_manager = CompactionManager::new(layout.clone());
        let manager = CompactionManager::new(layout.clone());
        let pin = pin_manager
            .pin_current_segment(protected, kind)
            .map_err(|error| error.to_string())?;

        let outcome = manager
            .compact_history(&lock)
            .map_err(|error| error.to_string())?;
        assert!(outcome.compacted(), "pin case {case} did not compact");
        assert_eq!(outcome.output_segments().len(), 1);
        assert!(outcome.reclamation().retained_by_pin().contains(&protected));
        assert!(segment_path(&layout, protected).exists());
        assert_eq!(
            history_frames(&layout, outcome.output_segments())?,
            before_frames
        );
        assert_security_reference_preserved(&layout, security)?;

        for reference in inputs
            .iter()
            .copied()
            .filter(|reference| *reference != protected)
        {
            assert!(!segment_path(&layout, reference).exists());
        }
        drop(pin);
        let retried = manager
            .reclaim_retired(&lock, &[protected])
            .map_err(|error| error.to_string())?;
        assert!(retried.reclaimed().contains(&protected));
        assert!(!segment_path(&layout, protected).exists());
    }
    Ok(())
}

#[test]
fn unpinned_history_is_compacted_losslessly_and_reclaimed_after_replay() -> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let inventory = install_inventory(&layout, &lock)?;
    let inputs = history_references(&inventory);
    let security = *inventory
        .iter()
        .find(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
        .ok_or_else(|| "fixture has no security-policy input".to_owned())?;
    let before_frames = history_frames(&layout, &inventory)?;
    let manager = CompactionManager::new(layout.clone());

    let outcome = manager
        .compact_history(&lock)
        .map_err(|error| error.to_string())?;
    assert!(outcome.compacted());
    assert_eq!(outcome.output_segments().len(), 1);
    assert_eq!(outcome.reclamation().reclaimed().len(), inputs.len());
    for reference in &inputs {
        assert!(!segment_path(&layout, *reference).exists());
    }
    assert_eq!(
        history_frames(&layout, outcome.output_segments())?,
        before_frames
    );
    assert_security_reference_preserved(&layout, security)?;
    assert_eq!(
        outcome.maintenance_revision(),
        Some(Revision::new(2).map_err(|e| e.to_string())?)
    );

    let second_pass = manager
        .compact_history(&lock)
        .map_err(|error| error.to_string())?;
    assert!(!second_pass.compacted());

    drop(lock);
    let reopened_layout = DatabaseLayout::open(layout.root()).map_err(|error| error.to_string())?;
    let reopened_lock = reopened_layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let recovered = RecoveryManager::new(reopened_layout.clone())
        .recover(&reopened_lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        recovered.report().disposition(),
        worlddb_storage_file::RecoveryDisposition::Clean
    );
    let current = ManifestStore::new(reopened_layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "reopened database has no current manifest".to_owned())?;
    assert_eq!(
        history_frames(&reopened_layout, current.segments())?,
        before_frames
    );
    Ok(())
}

#[test]
fn current_references_cannot_be_reclaimed_and_stale_references_cannot_be_pinned()
-> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let inventory = install_inventory(&layout, &lock)?;
    let reference = *history_references(&inventory)
        .first()
        .ok_or_else(|| "fixture has no history input".to_owned())?;
    let manager = CompactionManager::new(layout.clone());

    let outcome = manager
        .reclaim_retired(&lock, &[reference])
        .map_err(|error| error.to_string())?;
    assert_eq!(outcome.still_referenced(), &[reference]);
    assert!(segment_path(&layout, reference).exists());

    manager
        .compact_history(&lock)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        manager.pin_current_segment(reference, SegmentPinKind::Snapshot),
        Err(worlddb_storage_file::CompactionError::SegmentNotCurrent)
    ));
    Ok(())
}

#[test]
fn reclamation_never_accepts_security_policy_segments() -> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let inventory = install_inventory(&layout, &lock)?;
    let security = *inventory
        .iter()
        .find(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
        .ok_or_else(|| "fixture has no security-policy input".to_owned())?;
    let manager = CompactionManager::new(layout);

    assert!(matches!(
        manager.reclaim_retired(&lock, &[security]),
        Err(worlddb_storage_file::CompactionError::UnsupportedReclamationKind)
    ));
    Ok(())
}
