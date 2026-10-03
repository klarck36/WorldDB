//! Persistent guarded-migration commits over the file store's WAL transaction path.

use std::fmt;

use worlddb_core::{
    AuditCommitContext, AuditOutcome, AuditRecord, AuditRetentionPolicy, CancellablePublishError,
    CommitCancellation, DomainId, MigrationAuditCommit, MigrationCommitBackend, OperationId,
    Record, Revision, RevisionBackend, RevisionLogError, SecurityPolicyVersion,
};

use crate::{
    CommittedRequiredAuditRecord, DatabaseLayout, HistorySegmentStore, Manifest, ManifestError,
    ManifestSegmentKind, ManifestSegmentReference, ManifestStore, RecoveryError, RecoveryManager,
    RequiredAuditError, SecurityPolicyHistoryStore, SecurityPolicyStorageError, SegmentError,
    WalError, WalPrepareLog, WriterLock,
};

/// A validated view of persistent migration history could not be opened.
#[derive(Debug)]
pub enum FileMigrationBackendError {
    /// Recovery could not produce a clean, replayed storage view.
    Recovery(RecoveryError),
    /// The complete WAL commit chain could not be verified.
    Wal(WalError),
    /// The current manifest could not be read or verified.
    Manifest(ManifestError),
    /// A referenced history segment could not be read or verified.
    HistorySegment(SegmentError),
    /// The required-audit stream could not be reconstructed.
    RequiredAudit(RequiredAuditError),
    /// The committed policy projection could not be reconstructed or carried forward.
    SecurityPolicy(SecurityPolicyStorageError),
    /// A non-genesis WAL head has no corresponding complete manifest snapshot.
    ManifestWalMismatch {
        /// Latest verified revision named by the WAL.
        wal_revision: Revision,
        /// Latest revision materialized by `CURRENT`, if present.
        manifest_revision: Option<Revision>,
    },
    /// The segment's envelope digest does not match the current manifest reference.
    HistoryDigestMismatch,
    /// A stored guarded migration marker has no exact matching required-audit action.
    MigrationAuditMismatch(OperationId),
    /// Persistent history uses a form this adapter cannot read without guessing.
    UnsupportedHistory(&'static str),
}

impl fmt::Display for FileMigrationBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => {
                write!(formatter, "migration backend recovery failed: {error}")
            }
            Self::Wal(error) => write!(formatter, "migration backend WAL is invalid: {error}"),
            Self::Manifest(error) => {
                write!(formatter, "migration backend manifest is invalid: {error}")
            }
            Self::HistorySegment(error) => {
                write!(
                    formatter,
                    "migration backend history segment is invalid: {error}"
                )
            }
            Self::RequiredAudit(error) => {
                write!(
                    formatter,
                    "migration backend required audit is invalid: {error}"
                )
            }
            Self::SecurityPolicy(error) => {
                write!(
                    formatter,
                    "migration backend policy history is invalid: {error}"
                )
            }
            Self::ManifestWalMismatch {
                wal_revision,
                manifest_revision,
            } => write!(
                formatter,
                "migration backend requires a complete manifest at WAL revision {wal_revision}; CURRENT is {manifest_revision:?}"
            ),
            Self::HistoryDigestMismatch => {
                formatter.write_str("history segment digest differs from its manifest reference")
            }
            Self::MigrationAuditMismatch(operation_id) => write!(
                formatter,
                "migration marker for OperationId {operation_id} has no matching required-audit action"
            ),
            Self::UnsupportedHistory(reason) => {
                write!(
                    formatter,
                    "unsupported persistent migration history: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for FileMigrationBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::HistorySegment(error) => Some(error),
            Self::RequiredAudit(error) => Some(error),
            Self::SecurityPolicy(error) => Some(error),
            Self::ManifestWalMismatch { .. }
            | Self::HistoryDigestMismatch
            | Self::MigrationAuditMismatch(_)
            | Self::UnsupportedHistory(_) => None,
        }
    }
}

/// Durable `MigrationCommitBackend` bound to one exclusively locked file store.
///
/// Construction recovers the WAL before loading records. Guarded migration
/// records and their exact canonical Required Audit action payloads are written
/// in one WAL commit marker. Historical reads older than the current head fail
/// closed because a compacted manifest may not retain per-record revisions.
pub struct FileMigrationCommitBackend<'a> {
    layout: DatabaseLayout,
    writer_lock: &'a WriterLock,
    wal: WalPrepareLog,
    latest: Revision,
    references: Vec<ManifestSegmentReference>,
    records: Vec<(Revision, Record)>,
    security_policy_version: Option<SecurityPolicyVersion>,
    security_audit_retention: Option<AuditRetentionPolicy>,
    writes_blocked: bool,
    last_failure: Option<String>,
}

struct MigrationCommitRecoveryExpectation<'a> {
    operation_id: OperationId,
    expected_base: Revision,
    target_revision: Revision,
    audit_record: &'a AuditRecord,
    canonical_action_payload: &'a [u8],
    history_reference: ManifestSegmentReference,
    security_reference: Option<ManifestSegmentReference>,
}

impl<'a> FileMigrationCommitBackend<'a> {
    /// Recovers and opens a migration backend while retaining the caller's exclusive lock.
    pub fn open(
        layout: DatabaseLayout,
        writer_lock: &'a WriterLock,
    ) -> Result<Self, FileMigrationBackendError> {
        RecoveryManager::new(layout.clone())
            .recover(writer_lock)
            .map_err(FileMigrationBackendError::Recovery)?;

        let wal = WalPrepareLog::new(&layout);
        let wal_revision = wal
            .commit_head(writer_lock)
            .map_err(FileMigrationBackendError::Wal)?
            .revision();
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(FileMigrationBackendError::Manifest)?;
        if manifest.as_ref().map(Manifest::revision) != Some(wal_revision)
            && !(manifest.is_none() && wal_revision == Revision::GENESIS)
        {
            return Err(FileMigrationBackendError::ManifestWalMismatch {
                wal_revision,
                manifest_revision: manifest.as_ref().map(Manifest::revision),
            });
        }

        let references = manifest
            .as_ref()
            .map_or_else(Vec::new, |manifest| manifest.segments().to_vec());
        let records = load_current_records(&layout, writer_lock, wal_revision, &references, &wal)?;
        let security_reference_ids = references
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
            .map(|reference| reference.id())
            .collect::<Vec<_>>();
        let (security_policy_version, security_audit_retention) =
            if security_reference_ids.is_empty() {
                (None, None)
            } else {
                match SecurityPolicyHistoryStore::new(layout.clone())
                    .load_history(wal_revision, &security_reference_ids)
                {
                    Ok(history) => {
                        let version = history.policy().versions().last().cloned().ok_or(
                            FileMigrationBackendError::UnsupportedHistory(
                                "security-policy history has no current version",
                            ),
                        )?;
                        let retention = history.audit_retention_at(wal_revision).map_err(|_| {
                            FileMigrationBackendError::UnsupportedHistory(
                                "security-policy history has no current audit-retention version",
                            )
                        })?;
                        (Some(version), retention)
                    }
                    Err(SecurityPolicyStorageError::History(
                        worlddb_core::SecurityPolicyHistoryError::IncompleteThroughCommittedRevision
                        | worlddb_core::SecurityPolicyHistoryError::IncompleteRevisionCoverage,
                    )) => {
                        // Legacy non-policy migrations can carry older snapshots without complete
                        // coverage; the policy-gated CLI rejects those projects before this layer.
                        (None, None)
                    }
                    Err(error) => return Err(FileMigrationBackendError::SecurityPolicy(error)),
                }
            };

        Ok(Self {
            layout,
            writer_lock,
            wal,
            latest: wal_revision,
            references,
            records,
            security_policy_version,
            security_audit_retention,
            writes_blocked: false,
            last_failure: None,
        })
    }

    /// Returns the last diagnostic hidden by the core's compact backend error type.
    #[must_use]
    pub fn last_failure(&self) -> Option<&str> {
        self.last_failure.as_deref()
    }

    /// Verifies that a prior guarded step and its exact Required Audit record are committed.
    pub(crate) fn verify_committed_migration_step(
        &self,
        expected_base_revision: Revision,
        entries: &[Record],
        audit_commit: MigrationAuditCommit,
        audit_record: &AuditRecord,
    ) -> Result<Revision, FileMigrationBackendError> {
        let operation_id = audit_commit.operation_id();
        let target_revision = expected_base_revision
            .next_commit()
            .map_err(|_| FileMigrationBackendError::MigrationAuditMismatch(operation_id))?;
        if audit_commit.commit_revision() != target_revision || target_revision > self.latest {
            return Err(FileMigrationBackendError::MigrationAuditMismatch(
                operation_id,
            ));
        }

        let identity = audit_commit.identity();
        let mut batch_identity = None;
        for (index, record) in entries.iter().enumerate() {
            if let Record::MigrationStepCommitIdentity(found) = record {
                if batch_identity.replace((index, *found)).is_some() {
                    return Err(FileMigrationBackendError::MigrationAuditMismatch(
                        operation_id,
                    ));
                }
            }
        }
        if batch_identity != entries.len().checked_sub(1).zip(Some(identity)) {
            return Err(FileMigrationBackendError::MigrationAuditMismatch(
                operation_id,
            ));
        }

        let mut stored_marker = None;
        for (revision, record) in &self.records {
            if let Record::MigrationStepCommitIdentity(found) = record {
                if found.operation_id() == operation_id
                    && stored_marker.replace((*revision, *found)).is_some()
                {
                    return Err(FileMigrationBackendError::MigrationAuditMismatch(
                        operation_id,
                    ));
                }
            }
        }
        if stored_marker != Some((target_revision, identity)) {
            return Err(FileMigrationBackendError::MigrationAuditMismatch(
                operation_id,
            ));
        }

        let audits = self
            .wal
            .committed_required_audit_records(self.writer_lock)
            .map_err(FileMigrationBackendError::RequiredAudit)?;
        let mut matching_audit = audits
            .iter()
            .filter(|audit| audit.operation_id() == operation_id);
        let Some(committed_audit) = matching_audit.next() else {
            return Err(FileMigrationBackendError::MigrationAuditMismatch(
                operation_id,
            ));
        };
        let action_payload = audit_commit.canonical_action_payload();
        if matching_audit.next().is_some()
            || committed_audit.revision() != target_revision
            || committed_audit.record() != audit_record
            || committed_audit.migration_action_payload() != Some(action_payload.as_slice())
        {
            return Err(FileMigrationBackendError::MigrationAuditMismatch(
                operation_id,
            ));
        }
        Ok(target_revision)
    }

    fn publish_failure(&mut self, message: impl fmt::Display) -> CancellablePublishError {
        self.last_failure = Some(message.to_string());
        CancellablePublishError::Publish(RevisionLogError::BackendFailure)
    }

    fn reconcile_unknown_commit(
        &mut self,
        expectation: MigrationCommitRecoveryExpectation<'_>,
    ) -> Result<bool, String> {
        let MigrationCommitRecoveryExpectation {
            operation_id,
            expected_base,
            target_revision,
            audit_record,
            canonical_action_payload,
            history_reference,
            security_reference,
        } = expectation;
        RecoveryManager::new(self.layout.clone())
            .recover(self.writer_lock)
            .map_err(|error| error.to_string())?;

        let operation_status = self
            .wal
            .reconcile_operation_after_recovery(self.writer_lock, operation_id)
            .map_err(|error| error.to_string())?;

        let head = self
            .wal
            .commit_head(self.writer_lock)
            .map_err(|error| error.to_string())?
            .revision();
        let audits = self
            .wal
            .committed_required_audit_records(self.writer_lock)
            .map_err(|error| error.to_string())?;
        let matching = audits.iter().find(|entry| {
            entry.operation_id() == operation_id
                && entry.revision() == target_revision
                && entry.record() == audit_record
                && entry.migration_action_payload() == Some(canonical_action_payload)
        });
        if matches!(
            operation_status,
            crate::WalOperationStatus::Committed(receipt)
                if receipt.revision() == target_revision
        ) && head == target_revision
            && matching.is_some()
        {
            let manifest = ManifestStore::new(self.layout.clone())
                .read_current()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| String::from("recovered migration manifest is missing"))?;
            if manifest.revision() != target_revision
                || !manifest.segments().contains(&history_reference)
                || security_reference
                    .is_some_and(|reference| !manifest.segments().contains(&reference))
            {
                return Err(String::from(
                    "committed audit and recovered migration manifest do not match",
                ));
            }
            Ok(true)
        } else if operation_status == crate::WalOperationStatus::NotCommitted
            && head == expected_base
            && matching.is_none()
        {
            Ok(false)
        } else {
            Err(format!(
                "could not prove whether OperationId {operation_id} committed at revision {target_revision}"
            ))
        }
    }

    fn append_committed_entries(
        &mut self,
        target_revision: Revision,
        entries: Vec<Record>,
        history_reference: ManifestSegmentReference,
        security_reference: Option<ManifestSegmentReference>,
        security_policy_version: Option<&SecurityPolicyVersion>,
    ) {
        self.records
            .extend(entries.into_iter().map(|record| (target_revision, record)));
        self.references.push(history_reference);
        if let Some(reference) = security_reference {
            self.references.push(reference);
        }
        if let Some(version) = security_policy_version {
            self.security_policy_version = Some(version.clone());
        }
        self.latest = target_revision;
    }
}

impl RevisionBackend<Record> for FileMigrationCommitBackend<'_> {
    type Read<'b>
        = FileMigrationHistoryRead<'b>
    where
        Self: 'b,
        Record: 'b;

    fn latest_published(&self) -> Revision {
        self.latest
    }

    fn publish(&mut self, _entries: Vec<Record>) -> Result<Revision, RevisionLogError> {
        Err(RevisionLogError::BackendFailure)
    }

    fn publish_cancellable(
        &mut self,
        _entries: Vec<Record>,
        _cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        Err(CancellablePublishError::Publish(
            RevisionLogError::BackendFailure,
        ))
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
        let live_head = self
            .wal
            .commit_head(self.writer_lock)
            .map_err(|_| RevisionLogError::BackendFailure)?
            .revision();
        if live_head != self.latest {
            return Err(RevisionLogError::BackendFailure);
        }
        if revision > self.latest {
            return Err(RevisionLogError::RevisionNotPublished {
                requested: revision,
                published: self.latest,
            });
        }
        if revision != Revision::GENESIS && revision != self.latest {
            return Err(RevisionLogError::BackendFailure);
        }
        Ok(FileMigrationHistoryRead {
            records: self.records.iter(),
            as_of: revision,
        })
    }
}

impl MigrationCommitBackend for FileMigrationCommitBackend<'_> {
    fn publish_migration_step_with_required_audit(
        &mut self,
        expected_base_revision: Revision,
        entries: Vec<Record>,
        audit_commit: MigrationAuditCommit,
        audit_record: worlddb_core::AuditRecord,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        self.last_failure = None;
        if self.writes_blocked {
            return Err(self.publish_failure("writes are blocked pending storage recovery"));
        }
        let live_head = match self.wal.commit_head(self.writer_lock) {
            Ok(head) => head.revision(),
            Err(error) => return Err(self.publish_failure(error)),
        };
        if live_head != self.latest {
            return Err(self.publish_failure(format!(
                "live WAL head {live_head} changed after the migration backend was opened"
            )));
        }
        if self.latest != expected_base_revision {
            let message = format!(
                "OCC base {expected_base_revision} differs from WAL head {}",
                self.latest
            );
            return Err(self.publish_failure(message));
        }
        let target_revision = expected_base_revision
            .next_commit()
            .map_err(|error| self.publish_failure(error))?;
        if audit_commit.commit_revision() != target_revision
            || audit_commit.operation_id()
                != match audit_record.commit_context() {
                    AuditCommitContext::Committed {
                        revision,
                        operation_id,
                    } if revision == target_revision
                        && audit_record.outcome() == AuditOutcome::Succeeded =>
                    {
                        operation_id
                    }
                    _ => {
                        return Err(self.publish_failure(
                            "required audit record does not bind this migration commit",
                        ));
                    }
                }
        {
            return Err(self.publish_failure(
                "migration audit commit revision or operation identity is inconsistent",
            ));
        }
        let marker_count = entries
            .iter()
            .filter(|entry| {
                matches!(entry, Record::MigrationStepCommitIdentity(identity) if *identity == audit_commit.identity())
            })
            .count();
        if marker_count != 1 {
            return Err(self.publish_failure(
                "migration batch must contain exactly its matching persistent step marker",
            ));
        }
        if self.records.try_reserve(entries.len()).is_err()
            || self
                .references
                .try_reserve(1 + usize::from(self.security_policy_version.is_some()))
                .is_err()
        {
            return Err(self.publish_failure("could not reserve migration history memory"));
        }

        let history = HistorySegmentStore::new(self.layout.clone());
        let receipt = history
            .stage_segment(self.writer_lock, &entries)
            .map_err(|error| self.publish_failure(error))?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            receipt.id(),
            receipt.content_digest(),
            target_revision,
        );
        let security_store = SecurityPolicyHistoryStore::new(self.layout.clone());
        let (security_reference, next_security_version) =
            if let Some(current_version) = self.security_policy_version.as_ref() {
                let version = SecurityPolicyVersion::new(
                    target_revision,
                    current_version.epoch(),
                    current_version.snapshot().clone(),
                );
                let receipt = match security_store.stage_version(
                    self.writer_lock,
                    &version,
                    None,
                    self.security_audit_retention,
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        let cleanup = history.remove_staged_reference(history_reference);
                        let message = match cleanup {
                            Ok(()) => error.to_string(),
                            Err(cleanup_error) => {
                                format!("{error}; staged migration cleanup failed: {cleanup_error}")
                            }
                        };
                        return Err(self.publish_failure(message));
                    }
                };
                (
                    Some(ManifestSegmentReference::new(
                        ManifestSegmentKind::SecurityPolicy,
                        receipt.id(),
                        receipt.content_digest(),
                        target_revision,
                    )),
                    Some(version),
                )
            } else {
                (None, None)
            };
        let mut next_references = Vec::new();
        if next_references
            .try_reserve_exact(
                self.references
                    .len()
                    .saturating_add(1 + usize::from(security_reference.is_some())),
            )
            .is_err()
        {
            let _ = history.remove_staged_reference(history_reference);
            if let Some(reference) = security_reference {
                let _ = security_store.remove_staged_reference(reference);
            }
            return Err(self.publish_failure("could not reserve migration manifest references"));
        }
        next_references.extend_from_slice(&self.references);
        next_references.push(history_reference);
        if let Some(reference) = security_reference {
            next_references.push(reference);
        }

        let mut staged_references = Vec::new();
        if staged_references
            .try_reserve_exact(1 + usize::from(security_reference.is_some()))
            .is_err()
        {
            let _ = history.remove_staged_reference(history_reference);
            if let Some(reference) = security_reference {
                let _ = security_store.remove_staged_reference(reference);
            }
            return Err(self.publish_failure("could not reserve migration staging references"));
        }
        staged_references.push(history_reference);
        if let Some(reference) = security_reference {
            staged_references.push(reference);
        }

        let canonical_action_payload = audit_commit.canonical_action_payload();
        let prepare = match self.wal.prepare_audited_manifest_snapshot_with_action(
            self.writer_lock,
            audit_commit.operation_id(),
            next_references,
            &staged_references,
            &canonical_action_payload,
            &audit_record,
        ) {
            Ok(prepare) => prepare,
            Err(error) => {
                let message = cleanup_staged_migration_segments(
                    &history,
                    history_reference,
                    &security_store,
                    security_reference,
                )
                .map_or_else(
                    || error.to_string(),
                    |cleanup_error| {
                        format!("{error}; staged migration cleanup failed: {cleanup_error}")
                    },
                );
                return Err(self.publish_failure(message));
            }
        };

        let prepared_commit = match self.wal.prepare_commit_marker(self.writer_lock, prepare) {
            Ok(prepared) => prepared,
            Err(error) => {
                let recovery = RecoveryManager::new(self.layout.clone()).recover(self.writer_lock);
                let cleanup = cleanup_staged_migration_segments(
                    &history,
                    history_reference,
                    &security_store,
                    security_reference,
                );
                if recovery.is_err() || cleanup.is_some() {
                    self.writes_blocked = true;
                }
                let mut message = error.to_string();
                if let Err(recovery_error) = recovery {
                    message.push_str(&format!("; WAL prepare recovery failed: {recovery_error}"));
                }
                if let Some(cleanup_error) = cleanup {
                    message.push_str(&format!(
                        "; staged migration cleanup failed: {cleanup_error}"
                    ));
                }
                return Err(self.publish_failure(message));
            }
        };

        let permit = match cancellation.begin_commitpoint() {
            Ok(permit) => permit,
            Err(error) => {
                let recovery = RecoveryManager::new(self.layout.clone()).recover(self.writer_lock);
                let cleanup = cleanup_staged_migration_segments(
                    &history,
                    history_reference,
                    &security_store,
                    security_reference,
                );
                if recovery.is_err() || cleanup.is_some() {
                    self.writes_blocked = true;
                }
                self.last_failure = recovery
                    .err()
                    .map(|failure| {
                        format!("cancelled before commitpoint; recovery failed: {failure}")
                    })
                    .or_else(|| {
                        cleanup.map(|failure| {
                            format!(
                                "cancelled before commitpoint; staged cleanup failed: {failure}"
                            )
                        })
                    });
                return Err(CancellablePublishError::Commitpoint(error));
            }
        };

        match self.wal.publish_prepared_commit(prepared_commit) {
            Ok(receipt) if receipt.revision() == target_revision => {
                permit.committed();
                self.append_committed_entries(
                    target_revision,
                    entries,
                    history_reference,
                    security_reference,
                    next_security_version.as_ref(),
                );
                if let Err(error) =
                    RecoveryManager::new(self.layout.clone()).recover(self.writer_lock)
                {
                    self.writes_blocked = true;
                    self.last_failure = Some(format!(
                        "migration commit is durable; manifest materialization needs recovery: {error}"
                    ));
                }
                Ok(target_revision)
            }
            Ok(_) => {
                self.writes_blocked = true;
                self.last_failure = Some(String::from(
                    "WAL committed a migration at an unexpected revision",
                ));
                drop(permit);
                Err(CancellablePublishError::OutcomeUnknown(
                    audit_commit.operation_id(),
                ))
            }
            Err(error @ WalError::UnknownCommitOutcome { .. }) => {
                match self.reconcile_unknown_commit(MigrationCommitRecoveryExpectation {
                    operation_id: audit_commit.operation_id(),
                    expected_base: expected_base_revision,
                    target_revision,
                    audit_record: &audit_record,
                    canonical_action_payload: &canonical_action_payload,
                    history_reference,
                    security_reference,
                }) {
                    Ok(true) => {
                        permit.committed();
                        self.append_committed_entries(
                            target_revision,
                            entries,
                            history_reference,
                            security_reference,
                            next_security_version.as_ref(),
                        );
                        Ok(target_revision)
                    }
                    Ok(false) => {
                        permit.not_committed();
                        let _ = cleanup_staged_migration_segments(
                            &history,
                            history_reference,
                            &security_store,
                            security_reference,
                        );
                        Err(self.publish_failure(error))
                    }
                    Err(reconcile_error) => {
                        self.writes_blocked = true;
                        self.last_failure = Some(format!(
                            "{error}; commit reconciliation failed: {reconcile_error}"
                        ));
                        drop(permit);
                        Err(CancellablePublishError::OutcomeUnknown(
                            audit_commit.operation_id(),
                        ))
                    }
                }
            }
            Err(error) => {
                permit.not_committed();
                let _ = RecoveryManager::new(self.layout.clone()).recover(self.writer_lock);
                let _ = cleanup_staged_migration_segments(
                    &history,
                    history_reference,
                    &security_store,
                    security_reference,
                );
                Err(self.publish_failure(error))
            }
        }
    }
}

fn cleanup_staged_migration_segments(
    history: &HistorySegmentStore,
    history_reference: ManifestSegmentReference,
    security: &SecurityPolicyHistoryStore,
    security_reference: Option<ManifestSegmentReference>,
) -> Option<String> {
    let mut failures = Vec::new();
    if let Err(error) = history.remove_staged_reference(history_reference) {
        failures.push(error.to_string());
    }
    if let Some(reference) = security_reference {
        if let Err(error) = security.remove_staged_reference(reference) {
            failures.push(error.to_string());
        }
    }
    (!failures.is_empty()).then(|| failures.join("; "))
}

/// Borrowed read of migration history through one supported snapshot.
pub struct FileMigrationHistoryRead<'a> {
    records: std::slice::Iter<'a, (Revision, Record)>,
    as_of: Revision,
}

impl<'a> Iterator for FileMigrationHistoryRead<'a> {
    type Item = (Revision, &'a Record);

    fn next(&mut self) -> Option<Self::Item> {
        self.records
            .by_ref()
            .find(|(revision, _)| *revision <= self.as_of)
            .map(|(revision, record)| (*revision, record))
    }
}

fn load_current_records(
    layout: &DatabaseLayout,
    writer_lock: &WriterLock,
    latest: Revision,
    references: &[ManifestSegmentReference],
    wal: &WalPrepareLog,
) -> Result<Vec<(Revision, Record)>, FileMigrationBackendError> {
    let audits = wal
        .committed_required_audit_records(writer_lock)
        .map_err(FileMigrationBackendError::RequiredAudit)?;
    let mut by_operation = std::collections::BTreeMap::new();
    for audit in audits {
        if by_operation.insert(audit.operation_id(), audit).is_some() {
            return Err(FileMigrationBackendError::UnsupportedHistory(
                "a required audit OperationId appears more than once",
            ));
        }
    }

    let history = HistorySegmentStore::new(layout.clone());
    let mut records = Vec::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::History)
    {
        if reference.through_revision() > latest {
            return Err(FileMigrationBackendError::UnsupportedHistory(
                "history segment extends beyond the verified WAL head",
            ));
        }
        let segment = history
            .read_segment(reference.id())
            .map_err(FileMigrationBackendError::HistorySegment)?;
        if segment.content_digest() != reference.content_digest() {
            return Err(FileMigrationBackendError::HistoryDigestMismatch);
        }
        if !segment.records().is_empty() && reference.through_revision() == Revision::GENESIS {
            return Err(FileMigrationBackendError::UnsupportedHistory(
                "non-empty history is assigned to the genesis revision",
            ));
        }
        records.try_reserve(segment.records().len()).map_err(|_| {
            FileMigrationBackendError::UnsupportedHistory(
                "history inventory exceeds available memory",
            )
        })?;
        for decoded in segment.records() {
            let record = decoded.clone().into_record();
            let revision = match &record {
                Record::MigrationStepCommitIdentity(identity) => {
                    let operation_id = identity.operation_id();
                    let audit = by_operation.get(&operation_id).ok_or(
                        FileMigrationBackendError::MigrationAuditMismatch(operation_id),
                    )?;
                    if !migration_action_matches(audit, *identity, audit.revision()) {
                        return Err(FileMigrationBackendError::MigrationAuditMismatch(
                            operation_id,
                        ));
                    }
                    audit.revision()
                }
                _ => reference.through_revision(),
            };
            if revision > latest {
                return Err(FileMigrationBackendError::UnsupportedHistory(
                    "migration audit revision extends beyond the verified WAL head",
                ));
            }
            records.push((revision, record));
        }
    }
    records.sort_by_key(|(revision, _)| *revision);
    Ok(records)
}

fn migration_action_matches(
    audit: &CommittedRequiredAuditRecord,
    identity: worlddb_core::MigrationStepCommitIdentity,
    revision: Revision,
) -> bool {
    if audit.revision() != revision
        || audit.operation_id() != identity.operation_id()
        || audit.record().outcome() != AuditOutcome::Succeeded
        || audit.record().commit_context()
            != (AuditCommitContext::Committed {
                revision,
                operation_id: identity.operation_id(),
            })
    {
        return false;
    }

    let Some(payload) = audit.migration_action_payload() else {
        return false;
    };
    let prefix = b"WorldDB.RequiredAudit.Migration.v1\0";
    if !payload.starts_with(prefix) {
        return false;
    }
    let mut reader = ActionReader {
        bytes: payload,
        offset: prefix.len(),
    };
    let Some(plan_fingerprint) = reader.take(32) else {
        return false;
    };
    let Some(migration_id) = reader.take(16) else {
        return false;
    };
    let Some(run_id) = reader.take(16) else {
        return false;
    };
    let Some(step_id) = reader.take(16) else {
        return false;
    };
    let Some(operation_id) = reader.take(16) else {
        return false;
    };
    let Some(base_revision) = reader.u64() else {
        return false;
    };
    let Some(commit_revision) = reader.u64() else {
        return false;
    };
    let Some(schema_revision) = reader.u64() else {
        return false;
    };
    if reader.take(32).is_none() {
        return false;
    }
    let Some(input_fingerprint) = reader.take(32) else {
        return false;
    };
    let Some(decision_fingerprint) = reader.take(32) else {
        return false;
    };
    let Some(expected_base) = Revision::try_from(base_revision).ok() else {
        return false;
    };
    let Some(expected_commit) = Revision::try_from(commit_revision).ok() else {
        return false;
    };
    let Some(_schema_revision) = Revision::try_from(schema_revision).ok() else {
        return false;
    };
    let Some(expected_plan_fingerprint) = identity.plan_fingerprint() else {
        return false;
    };
    let Some(expected_input_fingerprint) = identity.input_fingerprint() else {
        return false;
    };
    let Some(expected_decision_fingerprint) = identity.decision_fingerprint() else {
        return false;
    };
    let expected_migration_id = identity.migration_id().to_bytes();
    let expected_run_id = identity.run_id().to_bytes();
    let expected_step_id = identity.step_id().to_bytes();
    let expected_operation_id = identity.operation_id().to_bytes();

    reader.is_finished()
        && expected_base.next_commit().ok() == Some(expected_commit)
        && expected_commit == revision
        && plan_fingerprint == expected_plan_fingerprint
        && migration_id == expected_migration_id
        && run_id == expected_run_id
        && step_id == expected_step_id
        && operation_id == expected_operation_id
        && input_fingerprint == expected_input_fingerprint
        && decision_fingerprint == expected_decision_fingerprint
}

struct ActionReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ActionReader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(length)?;
        let bytes = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    const fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{
        AuditAction, AuditCommitContext, AuditObjectClass, AuditOperationId, AuditOutcome,
        AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordId,
        AuditRecordIdentity, AuditSequence, Bytes, Capability, CapabilityGrant, CapabilityRule,
        DatabaseId, DomainId, GrantEffect, JobBudget, MigrationCategory, MigrationDryRun,
        MigrationPlan, MigrationPlanSpec, MigrationStepInput, MigrationStepTargetSchema,
        MigrationTargetSchema, MigrationTransformer, MigrationTransformerVersion, PolicyScope,
        PolicySubject, PolicyTarget, PredicateId, Principal, PrincipalId, Revision,
        RevisionBackend, SchemaDefinitionId, SchemaIdentityTransition, SchemaRevision,
        SecurityEpoch, SecurityPolicySnapshot, SourceSchemaPrecondition,
        ValidatedMigrationDecisions, execute_guarded_migration,
    };

    use super::FileMigrationCommitBackend;
    use crate::{DatabaseLayout, RecoveryManager, WalPrepareLog};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(std::path::PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("worlddb-m7-16c-{}-{sequence}", std::process::id()));
            DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
            Ok(Self(root))
        }

        fn layout(&self) -> Result<DatabaseLayout, String> {
            DatabaseLayout::open(&self.0).map_err(|error| error.to_string())
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    #[test]
    fn guarded_steps_and_canonical_audit_actions_reopen_from_file_store_commits()
    -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = database.layout()?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let migration_id = id(1)?;
        let first_step_id = id(2)?;
        let first_operation_id = id(3)?;
        let second_step_id = id(11)?;
        let second_operation_id = id(12)?;
        let run_id = id(4)?;
        let actor = id::<PrincipalId>(5)?;
        let database_id = id::<DatabaseId>(6)?;
        let predicate_id = id::<PredicateId>(7)?;
        let source_fingerprint = [0x11; 32];
        let target_fingerprint = [0x22; 32];
        let final_fingerprint = [0x33; 32];
        let transformer_version =
            MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?;
        let first_target_schema = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            target_fingerprint,
        );
        let second_revision = Revision::try_from(2).map_err(|error| error.to_string())?;
        let final_target_schema = MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(second_revision),
            final_fingerprint,
        );
        let transition = SchemaIdentityTransition::new(
            Some(SchemaDefinitionId::Predicate(predicate_id)),
            Some(SchemaDefinitionId::Predicate(predicate_id)),
            MigrationCategory::Restrictive,
        )
        .map_err(|error| error.to_string())?;
        let plan = MigrationPlan::new(MigrationPlanSpec {
            migration_id,
            category: MigrationCategory::Restrictive,
            source_schema: SourceSchemaPrecondition::new(
                SchemaRevision::from_published_revision(Revision::GENESIS),
                source_fingerprint,
            ),
            target_schema: final_target_schema,
            steps: vec![first_step_id, second_step_id],
            step_targets: Some(vec![
                MigrationStepTargetSchema::new(first_step_id, first_target_schema),
                MigrationStepTargetSchema::new(second_step_id, final_target_schema),
            ]),
            schema_changes: vec![transition],
            transformer_version,
            calendar_shift: None,
            budget: JobBudget::new(10, 1024 * 1024).map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())?;
        let inputs = vec![
            MigrationStepInput::new(first_step_id, first_operation_id, Vec::new()),
            MigrationStepInput::new(second_step_id, second_operation_id, Vec::new()),
        ];
        let rule = CapabilityRule::new(
            id(8)?,
            PolicySubject::Principal(actor),
            CapabilityGrant::new(Capability::MigrationExecute, GrantEffect::Allow),
            PolicyScope::project(),
        );
        let policy =
            SecurityPolicySnapshot::new(vec![Principal::new(actor)], vec![], vec![], vec![rule])
                .map_err(|error| error.to_string())?;
        let source = plan.source_schema_precondition();
        let dry_run = MigrationDryRun::run(&plan, source.revision(), *source.fingerprint(), &[]);
        let decisions = ValidatedMigrationDecisions::validate(
            &plan,
            &dry_run,
            Vec::new(),
            &policy,
            actor,
            PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
        let policy_fingerprint =
            policy.effective_capability_fingerprint(actor, PolicyTarget::default());
        let audit_record = |record_tail, sequence, audit_tail, revision, operation_id| {
            Ok::<_, String>(AuditRecord::new(
                AuditRecordIdentity {
                    record_id: id::<AuditRecordId>(record_tail)?,
                    sequence: AuditSequence::new(sequence),
                    audit_operation_id: id::<AuditOperationId>(audit_tail)?,
                },
                AuditRecordDetails {
                    actor,
                    action: AuditAction::Migration,
                    object_class: AuditObjectClass::Migration,
                    outcome: AuditOutcome::Succeeded,
                    commit_context: AuditCommitContext::Committed {
                        revision,
                        operation_id,
                    },
                    security_epoch: SecurityEpoch::INITIAL,
                    policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                        policy_fingerprint.to_vec(),
                    ))
                    .map_err(|error| error.to_string())?,
                },
            ))
        };
        let audit_records = vec![
            audit_record(9, 1, 10, Revision::FIRST_COMMIT, first_operation_id)?,
            audit_record(13, 2, 14, second_revision, second_operation_id)?,
        ];
        let expected_audits = audit_records.clone();
        let mut backend = FileMigrationCommitBackend::open(layout.clone(), &lock)
            .map_err(|error| error.to_string())?;
        let result = execute_guarded_migration(
            &mut backend,
            &plan,
            run_id,
            database_id,
            source_fingerprint,
            MigrationTransformer::for_version(transformer_version)
                .map_err(|error| error.to_string())?,
            inputs,
            decisions,
            actor,
            &policy,
            PolicyTarget::default(),
            SecurityEpoch::INITIAL,
            None,
            None,
            audit_records,
            |_, _, _, _| Ok::<(), String>(()),
        )
        .map_err(|failure| format!("guarded migration failed: {:?}", failure.error()))?;

        assert_eq!(result.final_revision(), second_revision);
        assert_eq!(backend.latest_published(), second_revision);
        {
            let history = backend
                .read_at(second_revision)
                .map_err(|error| error.to_string())?;
            let markers = history
                .filter_map(|(revision, record)| match record {
                    worlddb_core::Record::MigrationStepCommitIdentity(identity) => {
                        Some((revision, identity))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(markers.len(), 2);
            let mut marker_iter = markers.iter();
            let first_marker = marker_iter
                .next()
                .ok_or_else(|| String::from("first migration marker is missing"))?;
            let second_marker = marker_iter
                .next()
                .ok_or_else(|| String::from("second migration marker is missing"))?;
            assert!(marker_iter.next().is_none());
            assert_eq!(first_marker.0, Revision::FIRST_COMMIT);
            assert_eq!(first_marker.1.operation_id(), first_operation_id);
            assert_eq!(first_marker.1.migration_id(), migration_id);
            assert_eq!(second_marker.0, second_revision);
            assert_eq!(second_marker.1.operation_id(), second_operation_id);
            assert_eq!(second_marker.1.migration_id(), migration_id);
        }
        assert!(backend.read_at(Revision::FIRST_COMMIT).is_err());

        let committed_audits = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(committed_audits.len(), 2);
        let expected_revisions = [Revision::FIRST_COMMIT, second_revision];
        for ((audit, expected_record), expected_revision) in committed_audits
            .iter()
            .zip(&expected_audits)
            .zip(expected_revisions)
        {
            assert_eq!(audit.record(), expected_record);
            assert_eq!(audit.revision(), expected_revision);
            assert!(audit.migration_action_payload().is_some_and(|payload| {
                payload.starts_with(b"WorldDB.RequiredAudit.Migration.v1\0")
            }));
        }
        drop(backend);
        drop(lock);

        let reopened_layout = database.layout()?;
        let reopened_lock = reopened_layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(reopened_layout.clone())
            .recover(&reopened_lock)
            .map_err(|error| error.to_string())?;
        let reopened = FileMigrationCommitBackend::open(reopened_layout, &reopened_lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(reopened.latest_published(), second_revision);
        {
            let history = reopened
                .read_at(second_revision)
                .map_err(|error| error.to_string())?;
            let markers = history
                .filter_map(|(revision, record)| match record {
                    worlddb_core::Record::MigrationStepCommitIdentity(identity) => {
                        Some((revision, identity))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(markers.len(), 2);
            let mut marker_iter = markers.iter();
            let first_marker = marker_iter
                .next()
                .ok_or_else(|| String::from("reopened first migration marker is missing"))?;
            let second_marker = marker_iter
                .next()
                .ok_or_else(|| String::from("reopened second migration marker is missing"))?;
            assert!(marker_iter.next().is_none());
            assert_eq!(first_marker.0, Revision::FIRST_COMMIT);
            assert_eq!(first_marker.1.operation_id(), first_operation_id);
            assert_eq!(second_marker.0, second_revision);
            assert_eq!(second_marker.1.operation_id(), second_operation_id);
        }
        Ok(())
    }
}
