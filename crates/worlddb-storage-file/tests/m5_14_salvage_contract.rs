use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    DomainId, HistorySpaceDefinition, HistorySpaceId, OperationId, Record, Revision,
};
use worlddb_storage_file::{
    DatabaseLayout, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
    ManifestSnapshot, ManifestStore, RecoveryDisposition, RecoveryManager, SalvageInventorySource,
    SalvageManager, SalvageSegmentOutcome, SalvageSegmentSource, SalvageUnit, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-14-{}-{sequence}", std::process::id()));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn source(&self) -> Result<DatabaseLayout, String> {
        let path = self.0.join("source");
        DatabaseLayout::create(path).map_err(|error| error.to_string())
    }

    fn target(&self, name: &str) -> PathBuf {
        self.0.join(name)
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

fn reference_for(
    store: &HistorySegmentStore,
    lock: &worlddb_storage_file::WriterLock,
    tail: u8,
) -> Result<ManifestSegmentReference, String> {
    let receipt = store
        .stage_segment(lock, &[history_record(tail)?])
        .map_err(|error| error.to_string())?;
    Ok(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    ))
}

fn snapshot_tree(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf();
                files.insert(relative, fs::read(path).map_err(|error| error.to_string())?);
            } else {
                return Err("source fixture contains an unexpected non-file entry".to_owned());
            }
        }
    }
    Ok(files)
}

fn segment_path(directory: &Path, reference: ManifestSegmentReference) -> PathBuf {
    directory.join(format!(
        "segment-{}.wdbseg",
        reference.id().to_canonical_string()
    ))
}

#[test]
fn salvage_copies_verified_inventory_verbatim_and_leaves_source_tree_unchanged()
-> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.source()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    let reference = reference_for(&store, &lock, 1)?;
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(1)?, vec![reference], &[reference])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    drop(lock);
    let source_before = snapshot_tree(layout.root())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let source_segment = segment_path(&layout.segments_directory(), reference);
    let exact_segment_bytes = fs::read(&source_segment).map_err(|error| error.to_string())?;
    let target = area.target("clean-fork");
    let report = SalvageManager::new(layout.clone())
        .salvage(&lock, &target)
        .map_err(|error| error.to_string())?;

    assert_eq!(report.disposition(), RecoveryDisposition::Clean);
    assert_eq!(
        report.inventory_source(),
        SalvageInventorySource::CommittedWalSnapshot {
            revision: Revision::FIRST_COMMIT
        }
    );
    assert_eq!(report.segments().len(), 1);
    let salvaged_segment = report
        .segments()
        .first()
        .ok_or_else(|| "salvage report has no segment row".to_owned())?;
    assert_eq!(
        salvaged_segment.outcome(),
        &SalvageSegmentOutcome::Copied {
            source: SalvageSegmentSource::Final,
            units: 1,
        }
    );
    assert_eq!(salvaged_segment.unit(), SalvageUnit::HistoryRecords);
    assert_eq!(
        fs::read(segment_path(
            &target.join("history").join("segments"),
            reference
        ))
        .map_err(|error| error.to_string())?,
        exact_segment_bytes
    );
    let report_bytes =
        fs::read(target.join("SALVAGE_REPORT.tsv")).map_err(|error| error.to_string())?;
    assert_eq!(
        report.report_digest().to_string(),
        blake3::hash(&report_bytes).to_hex().to_string()
    );
    let completion =
        fs::read_to_string(target.join("SALVAGE")).map_err(|error| error.to_string())?;
    assert!(completion.contains(&report.database_id().to_string()));
    assert!(!target.join("SALVAGE_BUILDING").exists());
    assert!(
        String::from_utf8(report_bytes)
            .map_err(|error| error.to_string())?
            .contains("segment\thistory\t")
    );
    drop(lock);
    assert_eq!(snapshot_tree(layout.root())?, source_before);
    Ok(())
}

#[test]
fn salvage_reports_corrupt_candidate_as_omitted_and_copies_good_segment() -> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.source()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    let good = reference_for(&store, &lock, 2)?;
    let corrupt = reference_for(&store, &lock, 3)?;
    let mut references = vec![good, corrupt];
    references.sort_by_key(|reference| (reference.kind(), reference.id().to_bytes()));
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(2)?, references.clone(), &references)
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    let corrupt_path = segment_path(&layout.segments_directory(), corrupt);
    let mut damaged_bytes = fs::read(&corrupt_path).map_err(|error| error.to_string())?;
    let first_byte = damaged_bytes
        .first_mut()
        .ok_or_else(|| "segment file is unexpectedly empty".to_owned())?;
    *first_byte ^= 0x40;
    fs::write(&corrupt_path, damaged_bytes).map_err(|error| error.to_string())?;
    drop(lock);
    let source_before = snapshot_tree(layout.root())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let target = area.target("partial-fork");
    let report = SalvageManager::new(layout.clone())
        .salvage(&lock, &target)
        .map_err(|error| error.to_string())?;

    assert_eq!(
        report.disposition(),
        RecoveryDisposition::QuarantinedReadOnly
    );
    assert_eq!(report.segments().len(), 2);
    let good_result = report
        .segments()
        .iter()
        .find(|entry| entry.reference() == good)
        .ok_or_else(|| "good reference is missing from salvage report".to_owned())?;
    assert!(matches!(
        good_result.outcome(),
        SalvageSegmentOutcome::Copied {
            source: SalvageSegmentSource::Final,
            units: 1
        }
    ));
    let corrupt_result = report
        .segments()
        .iter()
        .find(|entry| entry.reference() == corrupt)
        .ok_or_else(|| "corrupt reference is missing from salvage report".to_owned())?;
    assert!(matches!(
        corrupt_result.outcome(),
        SalvageSegmentOutcome::Omitted { reason } if reason.contains("digest") || reason.contains("envelope")
    ));
    assert!(segment_path(&target.join("history").join("segments"), good).is_file());
    assert!(!segment_path(&target.join("history").join("segments"), corrupt).exists());
    let report_text =
        fs::read_to_string(target.join("SALVAGE_REPORT.tsv")).map_err(|error| error.to_string())?;
    assert!(report_text.contains(&corrupt.id().to_canonical_string()));
    assert!(report_text.contains("omitted"));
    assert!(report_text.contains("M5-18/M5-19"));
    drop(lock);
    assert_eq!(snapshot_tree(layout.root())?, source_before);
    Ok(())
}

#[test]
fn salvage_uses_wal_named_staged_bytes_without_recovering_source() -> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.source()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    let reference = reference_for(&store, &lock, 4)?;
    WalPrepareLog::new(&layout)
        .commit_manifest_snapshot(&lock, operation_id(3)?, vec![reference], &[reference])
        .map_err(|error| error.to_string())?;

    drop(lock);
    let source_before = snapshot_tree(layout.root())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let target = area.target("staged-fork");
    let report = SalvageManager::new(layout.clone())
        .salvage(&lock, &target)
        .map_err(|error| error.to_string())?;

    assert_eq!(report.disposition(), RecoveryDisposition::RecoveryRequired);
    assert_eq!(
        report.inventory_source(),
        SalvageInventorySource::CommittedWalSnapshot {
            revision: Revision::FIRST_COMMIT
        }
    );
    let salvaged_segment = report
        .segments()
        .first()
        .ok_or_else(|| "salvage report has no segment row".to_owned())?;
    assert!(matches!(
        salvaged_segment.outcome(),
        SalvageSegmentOutcome::Copied {
            source: SalvageSegmentSource::Staged,
            units: 1
        }
    ));
    assert!(segment_path(&target.join("history").join("segments"), reference).is_file());
    drop(lock);
    assert_eq!(snapshot_tree(layout.root())?, source_before);
    assert!(!layout.current_file().exists());
    Ok(())
}

#[test]
fn salvage_prefers_committed_wal_inventory_over_same_revision_manifest_mismatch()
-> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.source()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    let committed_reference = reference_for(&store, &lock, 5)?;
    let wal = WalPrepareLog::new(&layout);
    let receipt = wal
        .commit_manifest_snapshot(
            &lock,
            operation_id(5)?,
            vec![committed_reference],
            &[committed_reference],
        )
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    let other_segment = store
        .write_segment(&lock, &[history_record(6)?])
        .map_err(|error| error.to_string())?;
    let uncommitted_inventory_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        other_segment.id(),
        other_segment.content_digest(),
        receipt.revision(),
    );
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(receipt.revision(), vec![uncommitted_inventory_reference])
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;

    drop(lock);
    let source_before = snapshot_tree(layout.root())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let target = area.target("mismatched-fork");
    let report = SalvageManager::new(layout.clone())
        .salvage(&lock, &target)
        .map_err(|error| error.to_string())?;

    assert_eq!(
        report.disposition(),
        RecoveryDisposition::QuarantinedReadOnly
    );
    assert_eq!(
        report.inventory_source(),
        SalvageInventorySource::CommittedWalSnapshot {
            revision: receipt.revision()
        }
    );
    assert_eq!(report.segments().len(), 1);
    let salvaged_segment = report
        .segments()
        .first()
        .ok_or_else(|| "salvage report has no segment row".to_owned())?;
    assert_eq!(salvaged_segment.reference(), committed_reference);
    assert!(
        segment_path(
            &target.join("history").join("segments"),
            committed_reference
        )
        .is_file()
    );
    assert!(
        !segment_path(
            &target.join("history").join("segments"),
            uncommitted_inventory_reference
        )
        .exists()
    );
    let report_text =
        fs::read_to_string(target.join("SALVAGE_REPORT.tsv")).map_err(|error| error.to_string())?;
    assert!(report_text.contains("ManifestSnapshotMismatch"));
    assert!(report_text.contains("unreferenced source entry segments/"));
    drop(lock);
    assert_eq!(snapshot_tree(layout.root())?, source_before);
    Ok(())
}

#[test]
fn salvage_rejects_source_child_and_existing_targets_without_writing_them() -> Result<(), String> {
    let area = TempArea::create()?;
    let layout = area.source()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let manager = SalvageManager::new(layout.clone());
    drop(lock);
    let before = snapshot_tree(layout.root())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        manager.salvage(&lock, layout.root().join("inside")),
        Err(worlddb_storage_file::SalvageError::InvalidTarget)
    ));
    let existing = area.target("already-there");
    fs::create_dir(&existing).map_err(|error| error.to_string())?;
    assert!(matches!(
        manager.salvage(&lock, &existing),
        Err(worlddb_storage_file::SalvageError::TargetExists)
    ));
    drop(lock);
    assert_eq!(snapshot_tree(layout.root())?, before);
    assert_eq!(
        fs::read_dir(existing)
            .map_err(|error| error.to_string())?
            .count(),
        0
    );
    Ok(())
}
