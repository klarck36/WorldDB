use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition, ArchiveTransitionId,
    DomainId, Entity, EntityId, EntityRetirement, EntityRetirementId, EntityTypeId,
    HistorySpaceDefinition, HistorySpaceId, LayerDefinition, LayerId, Lifecycle,
    PerspectiveDefinitionRevision, PerspectiveId, Record, Revision, SchemaRevision, Symbol,
};
use worlddb_storage_file::{DatabaseLayout, HistorySegmentStore, SegmentError, SegmentId};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-06-{}-{sequence}", std::process::id()));
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

fn fixture_records() -> Result<Vec<Record>, String> {
    let revision = Revision::GENESIS;
    let history_space = HistorySpaceDefinition::new(id::<HistorySpaceId>(1)?, None, revision)
        .map_err(|error| error.to_string())?;
    let entity = Entity::new(id::<EntityId>(2)?, id::<EntityTypeId>(3)?, revision);
    let retirement =
        EntityRetirement::new(id::<EntityRetirementId>(6)?, entity.entity_id(), revision);
    let perspective = PerspectiveDefinitionRevision::new(
        id::<PerspectiveId>(4)?,
        Some(String::from("Research")),
        Some(String::from("Local segment roundtrip")),
        revision,
    )
    .map_err(|error| error.to_string())?;
    let layer = LayerDefinition::new(
        id::<LayerId>(5)?,
        Symbol::new("base").map_err(|error| error.to_string())?,
        Some(String::from("Base layer")),
        0,
        Lifecycle::Active,
        SchemaRevision::from_published_revision(revision),
    );
    let archive = ArchiveTransition::new(
        id::<ArchiveTransitionId>(7)?,
        ArchiveTargetRef::EntityRetirement(retirement.entity_retirement_id()),
        ArchiveAction::Archive,
        ArchiveState::Unarchived,
        revision,
    )
    .map_err(|error| error.to_string())?;

    Ok(vec![
        Record::HistorySpaceDefinition(history_space),
        Record::Entity(entity),
        Record::EntityRetirement(retirement),
        Record::PerspectiveDefinitionRevision(perspective),
        Record::LayerDefinition(layer),
        Record::ArchiveTransition(archive),
    ])
}

fn expected_frames(records: &[Record]) -> Result<Vec<Vec<u8>>, String> {
    let mut frames = records
        .iter()
        .map(|record| worlddb_core::encode_record(record).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    frames.sort_unstable();
    Ok(frames)
}

#[test]
fn random_segment_identity_and_content_digest_are_independent() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    let records = fixture_records()?;

    let first = store
        .write_segment(&lock, &records)
        .map_err(|error| error.to_string())?;
    let first_path = layout
        .segments_directory()
        .join(format!("segment-{}.wdbseg", first.id()));
    let first_bytes = fs::read(&first_path).map_err(|error| error.to_string())?;
    let mut reversed = records.clone();
    reversed.reverse();
    let second = store
        .write_segment(&lock, &reversed)
        .map_err(|error| error.to_string())?;

    assert_ne!(first.id(), second.id());
    assert_eq!(first.content_digest(), second.content_digest());
    assert_eq!(
        fs::read(&first_path).map_err(|error| error.to_string())?,
        first_bytes
    );
    assert_eq!(first.record_count(), records.len());
    assert_eq!(
        first.id().to_bytes()[6] >> 4,
        4,
        "random file IDs use UUIDv4"
    );
    assert_eq!(first.id().to_bytes()[8] & 0xc0, 0x80);

    let first_segment = store
        .read_segment(first.id())
        .map_err(|error| error.to_string())?;
    assert_eq!(first_segment.id(), first.id());
    assert_eq!(first_segment.content_digest(), first.content_digest());
    let mut stored_frames = first_segment
        .records()
        .iter()
        .map(|record| {
            worlddb_core::encode_decoded_record(record).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    stored_frames.sort_unstable();
    assert_eq!(stored_frames, expected_frames(&records)?);

    let second_path = layout
        .segments_directory()
        .join(format!("segment-{}.wdbseg", second.id()));
    assert_eq!(
        fs::metadata(first_path)
            .map_err(|error| error.to_string())?
            .len(),
        first.file_bytes()
    );
    assert_eq!(
        fs::metadata(second_path)
            .map_err(|error| error.to_string())?
            .len(),
        second.file_bytes()
    );
    Ok(())
}

#[test]
fn decoded_segment_preserves_optional_record_flags() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout);
    let records = fixture_records()?;
    let flags = 0x8000_0000_0000_0001;
    let first_record = records
        .first()
        .ok_or_else(|| String::from("fixture has no records"))?;
    let frame = worlddb_core::encode_record_with_flags(first_record, flags)
        .map_err(|error| error.to_string())?;
    let decoded = worlddb_core::decode_record(&frame).map_err(|error| error.to_string())?;
    let receipt = store
        .write_decoded_segment(&lock, &[decoded])
        .map_err(|error| error.to_string())?;
    let readback = store
        .read_segment(receipt.id())
        .map_err(|error| error.to_string())?;
    assert_eq!(readback.records().len(), 1);
    let decoded = readback
        .records()
        .first()
        .ok_or_else(|| String::from("readback has no records"))?;
    assert_eq!(decoded.optional_flags(), flags);
    Ok(())
}

#[test]
fn readback_rejects_changed_identity_and_content_digest() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    let receipt = store
        .write_segment(&lock, &fixture_records()?)
        .map_err(|error| error.to_string())?;
    let path = layout
        .segments_directory()
        .join(format!("segment-{}.wdbseg", receipt.id()));
    let original = fs::read(&path).map_err(|error| error.to_string())?;

    let mut changed_digest = original.clone();
    let digest_byte = changed_digest
        .get_mut(24)
        .ok_or_else(|| String::from("written segment has no digest envelope"))?;
    *digest_byte ^= 1;
    fs::write(&path, changed_digest).map_err(|error| error.to_string())?;
    assert!(matches!(
        store.read_segment(receipt.id()),
        Err(SegmentError::ContentDigestMismatch)
    ));

    let mut changed_identity = original;
    let other_id = id::<SegmentId>(9)?;
    changed_identity
        .get_mut(8..24)
        .ok_or_else(|| String::from("written segment has no identity envelope"))?
        .copy_from_slice(&other_id.to_bytes());
    fs::write(&path, changed_identity).map_err(|error| error.to_string())?;
    assert!(matches!(
        store.read_segment(receipt.id()),
        Err(SegmentError::SegmentIdMismatch)
    ));
    Ok(())
}

#[test]
fn segment_writes_require_a_matching_lock_and_nonempty_records() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let other_database = TempDatabase::create()?;
    let layout = database.layout()?;
    let other_layout = other_database.layout()?;
    let other_lock = other_layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let store = HistorySegmentStore::new(layout.clone());
    assert!(matches!(
        store.write_segment(&other_lock, &fixture_records()?),
        Err(SegmentError::ForeignWriterLock)
    ));

    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        store.write_segment(&lock, &[]),
        Err(SegmentError::EmptySegment)
    ));
    Ok(())
}

#[test]
fn segment_id_validates_the_registered_uuid_shape() -> Result<(), String> {
    let valid = id::<SegmentId>(10)?;
    assert_eq!(
        SegmentId::try_from_bytes(valid.to_bytes()).map_err(|error| error.to_string())?,
        valid
    );
    assert!(matches!(
        SegmentId::try_from_bytes([0; 16]),
        Err(worlddb_core::IdValidationError::ReservedZero)
    ));
    Ok(())
}
