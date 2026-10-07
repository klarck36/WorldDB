//! Durable, append-only progress records for restartable tail quarantine.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;

use crate::{DatabaseLayout, WriterLock};

const JOURNAL_NAME: &str = "RECOVERY";
const RECORD_MAGIC: [u8; 4] = *b"WDRJ";
const RECORD_VERSION: u16 = 1;
const RECORD_BYTES: usize = 4 + 2 + 1 + 1 + 8 + 8 + 8 + 8 + 32 + 32;
const RECORD_BODY_BYTES: usize = RECORD_BYTES - 32;
const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JournalPhase {
    Intent = 1,
    Quarantined = 2,
    Truncated = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JournalRecord {
    pub(crate) id: u64,
    pub(crate) phase: JournalPhase,
    pub(crate) sequence: u64,
    pub(crate) offset: u64,
    pub(crate) original_length: u64,
    pub(crate) suffix_digest: [u8; 32],
}

#[derive(Debug)]
pub enum JournalError {
    ForeignWriterLock,
    RecoveryRequired,
    Corrupt,
    InvalidPath,
    ResourceLimit {
        limit: usize,
        actual: usize,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("recovery journal requires this database's writer lock")
            }
            Self::RecoveryRequired => {
                formatter.write_str("recovery journal writes require an active recovery session")
            }
            Self::Corrupt => {
                formatter.write_str("recovery journal is malformed or has an invalid transition")
            }
            Self::InvalidPath => {
                formatter.write_str("recovery journal is not a regular file inside the database")
            }
            Self::ResourceLimit { limit, actual } => write!(
                formatter,
                "recovery journal has {actual} bytes; limit is {limit}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::Corrupt
            | Self::InvalidPath
            | Self::ResourceLimit { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RecoveryJournal {
    root: PathBuf,
    path: PathBuf,
}

impl RecoveryJournal {
    pub(crate) fn new(layout: &DatabaseLayout) -> Self {
        Self {
            root: layout.root().to_path_buf(),
            path: layout.root().join(JOURNAL_NAME),
        }
    }

    pub(crate) fn read_records(
        &self,
        lock: &WriterLock,
    ) -> Result<Vec<JournalRecord>, JournalError> {
        self.require_lock(lock)?;
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(JournalError::Io {
                    operation: "inspect recovery journal",
                    source,
                });
            }
        };
        self.validate_file(&metadata)?;
        let actual = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if actual > MAX_JOURNAL_BYTES {
            return Err(JournalError::ResourceLimit {
                limit: MAX_JOURNAL_BYTES,
                actual,
            });
        }
        let file = File::open(&self.path).map_err(|source| JournalError::Io {
            operation: "open recovery journal",
            source,
        })?;
        let opened = file.metadata().map_err(|source| JournalError::Io {
            operation: "inspect opened recovery journal",
            source,
        })?;
        if !opened.is_file() || opened.len() != metadata.len() {
            return Err(JournalError::InvalidPath);
        }
        let reserve = actual / RECORD_BYTES * RECORD_BYTES;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(actual)
            .map_err(|_| JournalError::ResourceLimit {
                limit: MAX_JOURNAL_BYTES,
                actual,
            })?;
        file.take(u64::try_from(MAX_JOURNAL_BYTES).unwrap_or(u64::MAX) + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| JournalError::Io {
                operation: "read recovery journal",
                source,
            })?;
        if bytes.len() != actual || bytes.len() > MAX_JOURNAL_BYTES {
            return Err(JournalError::Corrupt);
        }

        if reserve != actual {
            let journal = OpenOptions::new()
                .write(true)
                .open(&self.path)
                .map_err(|source| JournalError::Io {
                    operation: "open recovery journal to remove an incomplete final record",
                    source,
                })?;
            journal
                .set_len(u64::try_from(reserve).unwrap_or(u64::MAX))
                .and_then(|()| journal.sync_all())
                .map_err(|source| JournalError::Io {
                    operation: "durably remove an incomplete final recovery-journal record",
                    source,
                })?;
            bytes.truncate(reserve);
        }

        let mut records = Vec::new();
        records
            .try_reserve_exact(reserve / RECORD_BYTES)
            .map_err(|_| JournalError::ResourceLimit {
                limit: MAX_JOURNAL_BYTES,
                actual,
            })?;
        for record_bytes in bytes.chunks_exact(RECORD_BYTES) {
            records.push(decode_record(record_bytes)?);
        }
        validate_transitions(&records)?;
        Ok(records)
    }

    pub(crate) fn append(
        &self,
        lock: &WriterLock,
        record: JournalRecord,
    ) -> Result<(), JournalError> {
        self.require_lock(lock)?;
        if !lock.require_write_access() {
            return Err(JournalError::RecoveryRequired);
        }
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                self.validate_file(&metadata)?;
                Some(metadata)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(JournalError::Io {
                    operation: "inspect recovery journal before append",
                    source,
                });
            }
        };
        let old_length = metadata.as_ref().map_or(0, |metadata| metadata.len());
        if old_length % u64::try_from(RECORD_BYTES).unwrap_or(u64::MAX) != 0 {
            return Err(JournalError::Corrupt);
        }
        let new_length = old_length
            .checked_add(u64::try_from(RECORD_BYTES).unwrap_or(u64::MAX))
            .ok_or(JournalError::ResourceLimit {
                limit: MAX_JOURNAL_BYTES,
                actual: usize::MAX,
            })?;
        if usize::try_from(new_length).unwrap_or(usize::MAX) > MAX_JOURNAL_BYTES {
            return Err(JournalError::ResourceLimit {
                limit: MAX_JOURNAL_BYTES,
                actual: usize::try_from(new_length).unwrap_or(usize::MAX),
            });
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| JournalError::Io {
                operation: "open recovery journal for append",
                source,
            })?;
        if file
            .metadata()
            .map_err(|source| JournalError::Io {
                operation: "inspect recovery journal before append",
                source,
            })?
            .len()
            != old_length
        {
            return Err(JournalError::Corrupt);
        }
        let bytes = encode_record(record);
        file.write_all(&bytes).map_err(|source| JournalError::Io {
            operation: "append recovery journal record",
            source,
        })?;
        file.sync_all().map_err(|source| JournalError::Io {
            operation: "sync recovery journal record",
            source,
        })?;
        if metadata.is_none() {
            crate::manifest::sync_directory(&self.root).map_err(|source| JournalError::Io {
                operation: "sync database directory after creating recovery journal",
                source,
            })?;
        }
        Ok(())
    }

    fn require_lock(&self, lock: &WriterLock) -> Result<(), JournalError> {
        if lock.belongs_to_database_root(&self.root) {
            Ok(())
        } else {
            Err(JournalError::ForeignWriterLock)
        }
    }

    fn validate_file(&self, metadata: &fs::Metadata) -> Result<(), JournalError> {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(JournalError::InvalidPath);
        }
        let canonical = fs::canonicalize(&self.path).map_err(|source| JournalError::Io {
            operation: "resolve recovery journal path",
            source,
        })?;
        if canonical.parent() != Some(self.root.as_path()) {
            return Err(JournalError::InvalidPath);
        }
        Ok(())
    }
}

pub(crate) fn quarantine_path(layout: &DatabaseLayout, record: JournalRecord) -> PathBuf {
    let digest = blake3::Hash::from_bytes(record.suffix_digest)
        .to_hex()
        .to_string();
    layout.quarantine_directory().join(format!(
        "wal-tail-{:020}-{:020}-{:020}-{digest}.bin",
        record.id, record.sequence, record.offset
    ))
}

fn encode_record(record: JournalRecord) -> [u8; RECORD_BYTES] {
    let mut bytes = [0_u8; RECORD_BYTES];
    bytes[..4].copy_from_slice(&RECORD_MAGIC);
    bytes[4..6].copy_from_slice(&RECORD_VERSION.to_le_bytes());
    bytes[6] = record.phase as u8;
    bytes[8..16].copy_from_slice(&record.id.to_le_bytes());
    bytes[16..24].copy_from_slice(&record.sequence.to_le_bytes());
    bytes[24..32].copy_from_slice(&record.offset.to_le_bytes());
    bytes[32..40].copy_from_slice(&record.original_length.to_le_bytes());
    bytes[40..72].copy_from_slice(&record.suffix_digest);
    let checksum = blake3::hash(&bytes[..RECORD_BODY_BYTES]);
    bytes[RECORD_BODY_BYTES..].copy_from_slice(checksum.as_bytes());
    bytes
}

fn decode_record(bytes: &[u8]) -> Result<JournalRecord, JournalError> {
    let body = bytes
        .get(..RECORD_BODY_BYTES)
        .ok_or(JournalError::Corrupt)?;
    let checksum = bytes
        .get(RECORD_BODY_BYTES..)
        .ok_or(JournalError::Corrupt)?;
    if bytes.len() != RECORD_BYTES
        || bytes.get(..4) != Some(&RECORD_MAGIC)
        || bytes.get(4..6) != Some(&RECORD_VERSION.to_le_bytes())
        || bytes.get(7) != Some(&0)
        || checksum != blake3::hash(body).as_bytes()
    {
        return Err(JournalError::Corrupt);
    }
    let phase = match bytes.get(6).copied() {
        Some(1) => JournalPhase::Intent,
        Some(2) => JournalPhase::Quarantined,
        Some(3) => JournalPhase::Truncated,
        _ => return Err(JournalError::Corrupt),
    };
    Ok(JournalRecord {
        id: read_u64(bytes, 8)?,
        phase,
        sequence: read_u64(bytes, 16)?,
        offset: read_u64(bytes, 24)?,
        original_length: read_u64(bytes, 32)?,
        suffix_digest: bytes
            .get(40..72)
            .ok_or(JournalError::Corrupt)?
            .try_into()
            .map_err(|_| JournalError::Corrupt)?,
    })
}

#[cfg(test)]
pub(crate) fn fuzz_recovery_journal(
    layout: &DatabaseLayout,
    lock: &WriterLock,
    bytes: &[u8],
) -> bool {
    let path = layout.root().join(JOURNAL_NAME);
    if fs::write(&path, bytes).is_err() {
        return false;
    }
    let cleanup = RecoveryFuzzJournalFile(path);
    let read = RecoveryJournal::new(layout).read_records(lock).is_ok();
    let decoded = decode_record(bytes).is_ok();
    drop(cleanup);
    read || decoded
}

#[cfg(test)]
struct RecoveryFuzzJournalFile(PathBuf);

#[cfg(test)]
impl Drop for RecoveryFuzzJournalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn validate_transitions(records: &[JournalRecord]) -> Result<(), JournalError> {
    let mut latest_id = 0_u64;
    let mut active: Option<JournalRecord> = None;
    for record in records {
        if record.id == 0 || record.sequence == 0 || record.offset > record.original_length {
            return Err(JournalError::Corrupt);
        }
        match record.phase {
            JournalPhase::Intent => {
                if active.is_some()
                    || record.id != latest_id.checked_add(1).ok_or(JournalError::Corrupt)?
                {
                    return Err(JournalError::Corrupt);
                }
                latest_id = record.id;
                active = Some(*record);
            }
            JournalPhase::Quarantined | JournalPhase::Truncated => {
                let prior = active.as_ref().ok_or(JournalError::Corrupt)?;
                if !same_tail(*prior, *record) {
                    return Err(JournalError::Corrupt);
                }
                match (prior.phase, record.phase) {
                    (JournalPhase::Intent, JournalPhase::Quarantined) => active = Some(*record),
                    (JournalPhase::Quarantined, JournalPhase::Truncated) => active = None,
                    _ => return Err(JournalError::Corrupt),
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn same_tail(left: JournalRecord, right: JournalRecord) -> bool {
    left.id == right.id
        && left.sequence == right.sequence
        && left.offset == right.offset
        && left.original_length == right.original_length
        && left.suffix_digest == right.suffix_digest
}

pub(crate) fn current_record_state(records: &[JournalRecord]) -> (u64, Option<JournalRecord>) {
    let Some(last) = records.last().copied() else {
        return (0, None);
    };
    let pending = if last.phase == JournalPhase::Truncated {
        None
    } else {
        Some(last)
    };
    (last.id, pending)
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, JournalError> {
    let value: [u8; 8] = bytes
        .get(offset..offset.checked_add(8).ok_or(JournalError::Corrupt)?)
        .ok_or(JournalError::Corrupt)?
        .try_into()
        .map_err(|_| JournalError::Corrupt)?;
    Ok(u64::from_le_bytes(value))
}
