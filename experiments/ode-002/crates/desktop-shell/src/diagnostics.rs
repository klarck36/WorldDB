use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, TryLockError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const MAX_DIAGNOSTIC_RECORDS: usize = 128;
const MAX_DETAIL_BYTES: usize = 4 * 1024;
const MAX_EXPORT_BYTES: usize = 1024 * 1024;
const FORMAT_VERSION: u16 = 1;

struct DiagnosticRecord {
    sequence: u64,
    observed_unix_millis: u128,
    database_id: String,
    public_code: String,
    message_key: String,
    next_action_key: String,
    technical_detail: String,
}

struct DiagnosticState {
    next_sequence: u64,
    records: VecDeque<DiagnosticRecord>,
}

/// Host-only bounded storage for technical causes. It has no renderer serializer.
pub(super) struct DiagnosticStore {
    state: Mutex<DiagnosticState>,
    dropped_records: AtomicU64,
}

/// Opaque proof that the current project policy allowed diagnostic export.
pub(super) struct DiagnosticExportPermit {
    _private: (),
}

impl DiagnosticStore {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(DiagnosticState {
                next_sequence: 1,
                records: VecDeque::with_capacity(MAX_DIAGNOSTIC_RECORDS),
            }),
            dropped_records: AtomicU64::new(0),
        }
    }

    /// Records only host-derived details and never blocks a domain operation.
    pub(super) fn record(&self, database_id: Option<&str>, code: &'static str, detail: &str) {
        let Some(database_id) = database_id else {
            return;
        };
        let detail = bounded_detail(detail);
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(_)) | Err(TryLockError::WouldBlock) => {
                self.dropped_records.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        if state.records.len() == MAX_DIAGNOSTIC_RECORDS {
            state.records.pop_front();
            self.dropped_records.fetch_add(1, Ordering::Relaxed);
        }
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        state.records.push_back(DiagnosticRecord {
            sequence,
            observed_unix_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_millis()),
            database_id: database_id.to_owned(),
            public_code: code.to_owned(),
            message_key: localization_key(code),
            next_action_key: next_action_key(code).to_owned(),
            technical_detail: detail,
        });
    }

    pub(super) fn export_bundle(
        &self,
        database_id: &str,
        _permit: &DiagnosticExportPermit,
    ) -> Result<DiagnosticBundle, DiagnosticStoreError> {
        let state = self
            .state
            .lock()
            .map_err(|_| DiagnosticStoreError::Unavailable)?;
        let records = state
            .records
            .iter()
            .filter(|record| record.database_id == database_id)
            .map(|record| ExportRecord {
                sequence: record.sequence,
                observed_unix_millis: record.observed_unix_millis,
                public_code: record.public_code.clone(),
                message_key: record.message_key.clone(),
                next_action_key: record.next_action_key.clone(),
                technical_detail: record.technical_detail.clone(),
            })
            .collect();
        Ok(DiagnosticBundle {
            format_version: FORMAT_VERSION,
            exported_unix_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_millis()),
            database_id: database_id.to_owned(),
            dropped_record_count: self.dropped_records.load(Ordering::Relaxed),
            records,
        })
    }
}

#[derive(Serialize)]
pub(super) struct DiagnosticBundle {
    format_version: u16,
    exported_unix_millis: u128,
    database_id: String,
    dropped_record_count: u64,
    records: Vec<ExportRecord>,
}

#[derive(Serialize)]
struct ExportRecord {
    sequence: u64,
    observed_unix_millis: u128,
    public_code: String,
    message_key: String,
    next_action_key: String,
    technical_detail: String,
}

#[derive(Debug, Serialize)]
pub(super) struct DiagnosticExportViewV1 {
    pub protocol_version: u16,
    pub file_name: String,
    pub record_count: usize,
    pub bytes: usize,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiagnosticStoreError {
    Unavailable,
}

/// Requires both explicit audit inspection and export rights in the current project.
pub(super) fn authorize_export(capabilities: &[String]) -> Option<DiagnosticExportPermit> {
    (capabilities.iter().any(|value| value == "audit_read")
        && capabilities.iter().any(|value| value == "audit_export"))
    .then_some(DiagnosticExportPermit { _private: () })
}

pub(super) fn localization_key(code: &str) -> String {
    format!("worlddb.error.{code}")
}

pub(super) fn next_action_key(code: &str) -> &'static str {
    match code {
        "unknown_commit_outcome" => "worlddb.error.action.resolve_operation",
        "commit_conflict" => "worlddb.error.action.refresh_and_review",
        "recovery_required" | "recovery_inspection_unavailable" => {
            "worlddb.error.action.open_recovery"
        }
        "unauthorized" | "project_unavailable" => "worlddb.error.action.check_access",
        "invalid_request" | "explicit_confirmation_required" => "worlddb.error.action.review_input",
        "selection_cancelled" => "worlddb.error.action.none",
        _ => "worlddb.error.action.contact_support",
    }
}

pub(super) fn write_export(
    path: &Path,
    bundle: &DiagnosticBundle,
) -> Result<DiagnosticExportViewV1, DiagnosticWriteError> {
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 255
                && !name.chars().any(char::is_control)
                && !name.contains(['/', '\\'])
        })
        .ok_or(DiagnosticWriteError::InvalidTarget)?
        .to_owned();
    if fs::symlink_metadata(path).is_ok() {
        return Err(DiagnosticWriteError::TargetExists);
    }
    let encoded = serde_json::to_vec(bundle).map_err(|_| DiagnosticWriteError::Encode)?;
    if encoded.len() > MAX_EXPORT_BYTES {
        return Err(DiagnosticWriteError::TooLarge);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => DiagnosticWriteError::TargetExists,
            _ => DiagnosticWriteError::Io,
        })?;
    let write_result = file
        .write_all(&encoded)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    if write_result.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(DiagnosticWriteError::Io);
    }
    Ok(DiagnosticExportViewV1 {
        protocol_version: 1,
        file_name,
        record_count: bundle.records.len(),
        bytes: encoded.len(),
        digest: blake3::hash(&encoded).to_hex().to_string(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiagnosticWriteError {
    InvalidTarget,
    TargetExists,
    Encode,
    TooLarge,
    Io,
}

fn bounded_detail(detail: &str) -> String {
    if detail.len() <= MAX_DETAIL_BYTES {
        return detail.to_owned();
    }
    let marker = "…[truncated]";
    let mut end = MAX_DETAIL_BYTES.saturating_sub(marker.len());
    while !detail.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut value = detail[..end].to_owned();
    value.push_str(marker);
    value
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        DiagnosticStore, MAX_DETAIL_BYTES, MAX_DIAGNOSTIC_RECORDS, authorize_export, write_export,
    };

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temporary_directory() -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "worlddb-diagnostic-export-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("unique temporary directory");
        path
    }

    #[test]
    fn diagnostic_export_requires_both_audit_permissions() {
        assert!(authorize_export(&[]).is_none());
        assert!(authorize_export(&["audit_read".to_owned()]).is_none());
        assert!(authorize_export(&["audit_export".to_owned()]).is_none());
        assert!(authorize_export(&["audit_read".to_owned(), "audit_export".to_owned()]).is_some());
    }

    #[test]
    fn diagnostic_store_is_bounded_and_scoped_to_database() {
        let store = DiagnosticStore::new();
        store.record(Some("db-b"), "backup_rejected", "other-project-cause");
        for index in 0..(MAX_DIAGNOSTIC_RECORDS + 2) {
            store.record(
                Some("db-a"),
                "migration_rejected",
                &format!("cause-{index}"),
            );
        }
        let permit = authorize_export(&["audit_read".to_owned(), "audit_export".to_owned()])
            .expect("explicit audit export permissions");
        let bundle = store
            .export_bundle("db-a", &permit)
            .expect("diagnostic bundle");
        assert_eq!(bundle.records.len(), MAX_DIAGNOSTIC_RECORDS);
        assert!(
            bundle
                .records
                .iter()
                .all(|record| !record.technical_detail.contains("other-project"))
        );
        assert!(
            bundle
                .records
                .iter()
                .all(|record| record.technical_detail.len() <= MAX_DETAIL_BYTES)
        );
        assert_eq!(bundle.records[0].technical_detail, "cause-2");
    }

    #[test]
    fn diagnostic_export_creates_new_file_and_refuses_overwrite() {
        let directory = temporary_directory();
        let path = directory.join("WorldDB-Diagnose.json");
        let store = DiagnosticStore::new();
        store.record(
            Some("db-a"),
            "migration_rejected",
            "authorized technical cause",
        );
        let permit = authorize_export(&["audit_read".to_owned(), "audit_export".to_owned()])
            .expect("explicit audit export permissions");
        let bundle = store
            .export_bundle("db-a", &permit)
            .expect("diagnostic bundle");

        let view = write_export(&path, &bundle).expect("new diagnostic export");
        let bytes = fs::read(&path).expect("export bytes");
        assert_eq!(view.file_name, "WorldDB-Diagnose.json");
        assert_eq!(view.bytes, bytes.len());
        assert_eq!(view.record_count, 1);
        assert_eq!(view.digest, blake3::hash(&bytes).to_hex().to_string());
        assert!(write_export(&path, &bundle).is_err());

        fs::remove_file(path).expect("remove test export");
        fs::remove_dir(directory).expect("remove test directory");
    }
}
