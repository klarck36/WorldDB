//! Verified offline rewrite of an approved purge into a new database identity.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence, AuthorizationDecision,
    Capability, DatabaseId, DomainId, PolicyTarget, Revision, SecurityPolicyVersion,
    SecurityPolicyView, encode_decoded_record,
};

use crate::logical_export::{
    LogicalExport, LogicalExportError, LogicalExportManager, LogicalExportScope, record_revision,
};
use crate::logical_import::record_defined_identities;
use crate::purge::{
    PurgeApproval, PurgeCascadePlan, PurgeError, PurgeExternalArtifact, PurgePlan,
    PurgePlanManager, PurgeRecordId, PurgeSidecarInventory,
};
use crate::{
    DatabaseLayout, HistorySegmentStore, IndexGenerationStore, LogicalImportIdentity,
    ManifestSegmentKind, ManifestSegmentReference, ManifestStore, RecoveryError, RecoveryManager,
    RequiredAuditError, SecurityPolicyHistoryStore, SecurityPolicyStorageError, SegmentError,
    SnapshotCommitError, StorageFileError, StorageVerifier, StorageVerifyError, WalError,
    WalPrepareLog, WriterLockError,
};

const REPORT_MAGIC: &[u8; 8] = b"WDBPRG\0\x01";
const REPORT_CONTEXT: &[u8] = b"WorldDB.PurgeReport.v1\0";
const REPORT_FILE: &str = "PURGE_REPORT";
const REPORT_LIMIT: usize = 128 * 1024 * 1024;
const MAX_REWRITE_REVISIONS: u64 = 65_536;
const MAX_STAGE_ATTEMPTS: u8 = 32;

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PurgeRewriteCheckpoint {
    BeforeAuditCommit,
    BeforePublish,
    AfterPublish,
}

/// One retained or removed identity assignment in a purge report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PurgeIdMapping {
    source: LogicalImportIdentity,
    destination: Option<LogicalImportIdentity>,
}

impl PurgeIdMapping {
    /// Identity in the source database.
    #[must_use]
    pub const fn source(self) -> LogicalImportIdentity {
        self.source
    }

    /// Identity in the rewritten database, or `None` when the approved purge removed it.
    #[must_use]
    pub const fn destination(self) -> Option<LogicalImportIdentity> {
        self.destination
    }
}

/// Durable report describing an offline purge rewrite and its retained external copies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeReport {
    source_database_id: DatabaseId,
    destination_database_id: DatabaseId,
    source_revision: Revision,
    destination_revision: Revision,
    source_artifact_digest: [u8; 32],
    plan_fingerprint: [u8; 32],
    operation_id: worlddb_core::OperationId,
    audit_record_id: worlddb_core::AuditRecordId,
    removed_records: Vec<PurgeRecordId>,
    id_mappings: Vec<PurgeIdMapping>,
    retained_external_artifacts: Vec<PurgeExternalArtifact>,
    external_inventory_complete: bool,
    index_families_to_rebuild: Vec<worlddb_core::IndexFamily>,
}

impl PurgeReport {
    /// Source DatabaseId bound by the approved plan.
    #[must_use]
    pub const fn source_database_id(&self) -> DatabaseId {
        self.source_database_id
    }

    /// New DatabaseId of the rewritten destination.
    #[must_use]
    pub const fn destination_database_id(&self) -> DatabaseId {
        self.destination_database_id
    }

    /// Last source revision retained as logical history.
    #[must_use]
    pub const fn source_revision(&self) -> Revision {
        self.source_revision
    }

    /// Destination revision containing the atomic PurgePublication and Required Audit Record.
    #[must_use]
    pub const fn destination_revision(&self) -> Revision {
        self.destination_revision
    }

    /// Exact source artifact digest reviewed by the operator.
    #[must_use]
    pub const fn source_artifact_digest(&self) -> [u8; 32] {
        self.source_artifact_digest
    }

    /// Fingerprint of the approved target, dependant, and sidecar inventory.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> [u8; 32] {
        self.plan_fingerprint
    }

    /// WAL operation that committed the publication action and audit record.
    #[must_use]
    pub const fn operation_id(&self) -> worlddb_core::OperationId {
        self.operation_id
    }

    /// Required Audit Record committed with the publication action.
    #[must_use]
    pub const fn audit_record_id(&self) -> worlddb_core::AuditRecordId {
        self.audit_record_id
    }

    /// Exact target and dependant records removed by the approved rewrite.
    #[must_use]
    pub fn removed_records(&self) -> &[PurgeRecordId] {
        &self.removed_records
    }

    /// Source identities and their unchanged or absent destination identities.
    #[must_use]
    pub fn id_mappings(&self) -> &[PurgeIdMapping] {
        &self.id_mappings
    }

    /// Known backups and exports that remain outside the destination.
    #[must_use]
    pub fn retained_external_artifacts(&self) -> &[PurgeExternalArtifact] {
        &self.retained_external_artifacts
    }

    /// Whether the caller declared the external-copy search complete.
    #[must_use]
    pub const fn external_inventory_complete(&self) -> bool {
        self.external_inventory_complete
    }

    /// Purge never claims secure erasure for external copies or underlying storage media.
    #[must_use]
    pub const fn secure_erase_claimed(&self) -> bool {
        false
    }

    /// Index families that must be rebuilt for the destination.
    #[must_use]
    pub fn index_families_to_rebuild(&self) -> &[worlddb_core::IndexFamily] {
        &self.index_families_to_rebuild
    }

    /// Encodes a bounded canonical report with a domain-separated integrity digest.
    pub fn encode(&self) -> Result<Vec<u8>, PurgeRewriteError> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.source_database_id.to_bytes());
        payload.extend_from_slice(&self.destination_database_id.to_bytes());
        push_u64(&mut payload, self.source_revision.value());
        push_u64(&mut payload, self.destination_revision.value());
        payload.extend_from_slice(&self.source_artifact_digest);
        payload.extend_from_slice(&self.plan_fingerprint);
        payload.extend_from_slice(&self.operation_id.to_bytes());
        payload.extend_from_slice(&self.audit_record_id.to_bytes());
        push_count(&mut payload, self.removed_records.len())?;
        for record in &self.removed_records {
            record.identity().encode(&mut payload)?;
            payload.extend_from_slice(&record.content_digest());
        }
        push_count(&mut payload, self.id_mappings.len())?;
        for mapping in &self.id_mappings {
            mapping.source.encode(&mut payload)?;
            match mapping.destination {
                Some(destination) => {
                    payload.push(1);
                    destination.encode(&mut payload)?;
                }
                None => payload.push(0),
            }
        }
        push_count(&mut payload, self.retained_external_artifacts.len())?;
        for artifact in &self.retained_external_artifacts {
            payload.push(artifact.kind() as u8);
            payload.extend_from_slice(&artifact.digest());
        }
        payload.push(u8::from(self.external_inventory_complete));
        push_count(&mut payload, self.index_families_to_rebuild.len())?;
        for family in &self.index_families_to_rebuild {
            payload.extend_from_slice(&family.wire_tag().to_le_bytes());
        }
        // A zero bit is an explicit declaration that this report makes no secure-erasure claim.
        payload.push(0);
        if payload.len() > REPORT_LIMIT {
            return Err(PurgeRewriteError::ResourceLimit);
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(REPORT_MAGIC);
        push_u64(
            &mut bytes,
            u64::try_from(payload.len()).map_err(|_| PurgeRewriteError::ResourceLimit)?,
        );
        bytes.extend_from_slice(&payload);
        let mut hasher = blake3::Hasher::new();
        hasher.update(REPORT_CONTEXT);
        hasher.update(&bytes);
        bytes.extend_from_slice(hasher.finalize().as_bytes());
        if bytes.len() > REPORT_LIMIT {
            return Err(PurgeRewriteError::ResourceLimit);
        }
        Ok(bytes)
    }
}

/// Outcome returned only after the new database and its report are published and verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeRewriteReceipt {
    destination: PathBuf,
    report: PurgeReport,
}

impl PurgeRewriteReceipt {
    /// Published and verified destination root.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Complete persisted purge report.
    #[must_use]
    pub const fn report(&self) -> &PurgeReport {
        &self.report
    }
}

/// Executes an approved purge as a new-identity, offline database rewrite.
#[derive(Clone, Copy, Debug, Default)]
pub struct PurgeRewriteManager;

/// Inputs bound to one approved offline rewrite.
pub struct PurgeRewriteRequest<'a> {
    /// Source database root.
    pub source_root: &'a Path,
    /// Exact complete logical export used by the approved plan.
    pub source_artifact: &'a [u8],
    /// Explicitly approved target and dependant set.
    pub plan: &'a PurgePlan,
    /// Known external backup/export inventory supplied again for revalidation.
    pub known_external_artifacts: &'a [PurgeExternalArtifact],
    /// Whether the caller declares its external-copy search complete.
    pub external_inventory_complete: bool,
    /// New destination root. It must not exist and must be outside the source database.
    pub destination: &'a Path,
    /// Current selected policy used for Purge and DataExport authorization.
    pub policy: SecurityPolicyView<'a>,
    /// Policy target at which Purge permission is checked.
    pub policy_target: PolicyTarget,
    /// Safe current-policy fingerprint copied into the Required Audit Record.
    pub policy_fingerprint: AuditPolicyFingerprint,
}

impl PurgeRewriteManager {
    /// Creates a stateless offline purge manager.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Revalidates the live source and approved sidecars, writes a new database, and publishes it
    /// only after Storage Verify and the Required PurgePublication audit record both pass.
    pub fn rewrite(
        &self,
        request: PurgeRewriteRequest<'_>,
    ) -> Result<PurgeRewriteReceipt, PurgeRewriteError> {
        self.rewrite_with_checkpoint(request, |_| false)
    }

    fn rewrite_with_checkpoint(
        &self,
        request: PurgeRewriteRequest<'_>,
        mut checkpoint: impl FnMut(PurgeRewriteCheckpoint) -> bool,
    ) -> Result<PurgeRewriteReceipt, PurgeRewriteError> {
        let PurgeRewriteRequest {
            source_root,
            source_artifact,
            plan,
            known_external_artifacts,
            external_inventory_complete,
            destination,
            policy,
            policy_target,
            policy_fingerprint,
        } = request;
        authorize_purge(policy, policy_target)?;
        if plan.approval().is_none() {
            return Err(PurgeRewriteError::ApprovalMissing);
        }
        if plan.source_artifact_digest() != *blake3::hash(source_artifact).as_bytes() {
            return Err(PurgeRewriteError::SourceArtifactMismatch);
        }

        let source_layout =
            DatabaseLayout::open(source_root).map_err(PurgeRewriteError::StorageFile)?;
        let source_database_id = source_layout
            .database_id()
            .ok_or(PurgeRewriteError::DatabaseIdentityMissing)?;
        if source_database_id != plan.source_database_id() {
            return Err(PurgeRewriteError::SourceIdentityMismatch);
        }
        let target = normalize_target(destination, source_layout.root())?;
        let source_lock = source_layout
            .try_writer_lock()
            .map_err(PurgeRewriteError::WriterLock)?;
        let source_verify = StorageVerifier::new(source_layout.clone())
            .verify(&source_lock)
            .map_err(PurgeRewriteError::StorageVerify)?;
        if !source_verify.is_clean()
            || source_verify.safe_revision() != plan.source_snapshot_revision()
        {
            return Err(PurgeRewriteError::SourceNotClean);
        }
        let source_head = WalPrepareLog::new(&source_layout)
            .commit_head(&source_lock)
            .map_err(PurgeRewriteError::Wal)?;
        if source_head.revision() != plan.source_snapshot_revision() {
            return Err(PurgeRewriteError::SourceSnapshotChanged);
        }
        if source_head.revision().value() > MAX_REWRITE_REVISIONS {
            return Err(PurgeRewriteError::ResourceLimit);
        }
        let source_manifest = ManifestStore::new(source_layout.clone())
            .read_current()
            .map_err(PurgeRewriteError::Manifest)?
            .ok_or(PurgeRewriteError::SourceSnapshotChanged)?;
        if source_manifest.revision() != source_head.revision()
            || source_manifest.commit_hash() != source_head.commit_hash()
        {
            return Err(PurgeRewriteError::SourceSnapshotChanged);
        }

        let source_export =
            LogicalExport::decode(source_artifact).map_err(PurgeRewriteError::Export)?;
        let scope = LogicalExportScope::new(
            source_export.manifest().from_revision(),
            source_export.manifest().through_revision(),
            source_export.manifest().selected_history_spaces().to_vec(),
            source_export.manifest().selected_record_kinds().to_vec(),
        )
        .map_err(PurgeRewriteError::Export)?;
        let live_export = LogicalExportManager::new(source_layout.clone())
            .export_locked(scope.clone(), policy, &source_lock)
            .map_err(PurgeRewriteError::Export)?
            .encode()
            .map_err(PurgeRewriteError::Export)?;
        if live_export != source_artifact {
            return Err(PurgeRewriteError::SourceSnapshotChanged);
        }

        let index_inventory =
            IndexGenerationStore::inventory_all_locked(&source_layout, &source_lock)
                .map_err(PurgeRewriteError::IndexInventory)?;
        let sidecars = PurgeSidecarInventory::new(
            index_inventory,
            known_external_artifacts.to_vec(),
            external_inventory_complete,
        )
        .map_err(PurgeRewriteError::PurgePlan)?;
        if !plan.matches_sidecars(&sidecars) {
            return Err(PurgeRewriteError::SidecarInventoryChanged);
        }
        let preview = PurgePlanManager::preview(&live_export, plan.targets().to_vec(), sidecars)
            .map_err(PurgeRewriteError::PurgePlan)?;
        let revalidated_plan = match plan.approval() {
            Some(PurgeApproval::RejectIfReferenced) => preview
                .approve_reject_if_referenced()
                .map_err(PurgeRewriteError::PurgePlan)?,
            Some(PurgeApproval::CascadePlan) => {
                let cascade = PurgeCascadePlan::new(preview.dependants().to_vec())
                    .map_err(PurgeRewriteError::PurgePlan)?;
                preview
                    .approve_cascade(&cascade)
                    .map_err(PurgeRewriteError::PurgePlan)?
            }
            None => return Err(PurgeRewriteError::ApprovalMissing),
        };
        if &revalidated_plan != plan {
            return Err(PurgeRewriteError::PlanFingerprintMismatch);
        }

        let source_security_segments =
            read_source_security_segments(&source_layout, source_manifest.segments())?;
        if source_security_segments
            .first()
            .is_none_or(|segment| segment.version().revision() != Revision::GENESIS)
        {
            return Err(PurgeRewriteError::SecurityHistoryMissing);
        }
        let latest_source_policy = source_security_segments
            .last()
            .ok_or(PurgeRewriteError::SecurityHistoryMissing)?;
        if latest_source_policy.version().epoch() != policy.current_epoch() {
            return Err(PurgeRewriteError::PolicySnapshotMismatch);
        }

        let mut histories = BTreeMap::<u64, Vec<worlddb_core::DecodedRecord>>::new();
        let mut genesis_records = Vec::new();
        for entry in source_export.records() {
            let decoded = entry.record();
            if revalidated_plan
                .is_affected(decoded)
                .map_err(PurgeRewriteError::PurgePlan)?
            {
                continue;
            }
            match record_revision(decoded.record()) {
                Some(revision) if revision == Revision::GENESIS => {
                    genesis_records.push(decoded.clone());
                }
                Some(revision) => histories
                    .entry(revision.value())
                    .or_default()
                    .push(decoded.clone()),
                None => genesis_records.push(decoded.clone()),
            }
        }
        if histories
            .keys()
            .any(|revision| *revision > source_head.revision().value())
        {
            return Err(PurgeRewriteError::SourceSnapshotChanged);
        }
        let id_mappings = build_id_mappings(&source_export, &revalidated_plan)?;

        let stage = create_stage_path(&target)?;
        let mut stage_guard = PurgeStageGuard::new(stage.clone());
        let stage_layout =
            DatabaseLayout::create(&stage).map_err(PurgeRewriteError::StorageFile)?;
        let destination_database_id = stage_layout
            .database_id()
            .ok_or(PurgeRewriteError::DatabaseIdentityMissing)?;
        if destination_database_id == source_database_id {
            return Err(PurgeRewriteError::IdentityCollision);
        }
        let destination_lock = stage_layout
            .try_writer_lock()
            .map_err(PurgeRewriteError::WriterLock)?;
        RecoveryManager::new(stage_layout.clone())
            .recover(&destination_lock)
            .map_err(PurgeRewriteError::Recovery)?;

        let history_store = HistorySegmentStore::new(stage_layout.clone());
        let security_store = SecurityPolicyHistoryStore::new(stage_layout.clone());
        let wal = WalPrepareLog::new(&stage_layout);
        let mut manifest_segments = Vec::<ManifestSegmentReference>::new();
        let mut staged_segments = Vec::<ManifestSegmentReference>::new();

        if !genesis_records.is_empty() {
            let receipt = history_store
                .stage_decoded_segment(&destination_lock, &genesis_records)
                .map_err(PurgeRewriteError::Segment)?;
            let reference = ManifestSegmentReference::new(
                ManifestSegmentKind::History,
                receipt.id(),
                receipt.content_digest(),
                Revision::GENESIS,
            );
            manifest_segments.push(reference);
            staged_segments.push(reference);
        }

        let mut latest_security = source_security_segments
            .first()
            .cloned()
            .ok_or(PurgeRewriteError::SecurityHistoryMissing)?;
        let initial_security = security_store
            .stage_version(
                &destination_lock,
                latest_security.version(),
                latest_security.policy_record(),
                latest_security.audit_retention(),
            )
            .map_err(PurgeRewriteError::SecuritySegment)?;
        let initial_security_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            initial_security.id(),
            initial_security.content_digest(),
            Revision::GENESIS,
        );
        manifest_segments.push(initial_security_reference);
        staged_segments.push(initial_security_reference);

        for revision_value in 1..=source_head.revision().value() {
            let revision = Revision::new(revision_value).map_err(PurgeRewriteError::Revision)?;
            if revision_value > 1 {
                staged_segments.clear();
            }
            if let Some(records) = histories.get(&revision_value) {
                if !records.is_empty() {
                    let receipt = history_store
                        .stage_decoded_segment(&destination_lock, records)
                        .map_err(PurgeRewriteError::Segment)?;
                    let reference = ManifestSegmentReference::new(
                        ManifestSegmentKind::History,
                        receipt.id(),
                        receipt.content_digest(),
                        revision,
                    );
                    manifest_segments.push(reference);
                    staged_segments.push(reference);
                }
            }
            if let Some(segment) = source_security_segments
                .iter()
                .find(|segment| segment.version().revision() == revision)
            {
                latest_security = segment.clone();
            }
            let policy_version = SecurityPolicyVersion::new(
                revision,
                latest_security.version().epoch(),
                latest_security.version().snapshot().clone(),
            );
            let policy_record = source_security_segments
                .iter()
                .find(|segment| segment.version().revision() == revision)
                .and_then(|segment| segment.policy_record());
            let policy_segment = security_store
                .stage_version(
                    &destination_lock,
                    &policy_version,
                    policy_record,
                    latest_security.audit_retention(),
                )
                .map_err(PurgeRewriteError::SecuritySegment)?;
            let policy_reference = ManifestSegmentReference::new(
                ManifestSegmentKind::SecurityPolicy,
                policy_segment.id(),
                policy_segment.content_digest(),
                revision,
            );
            manifest_segments.push(policy_reference);
            staged_segments.push(policy_reference);

            let operation_id =
                worlddb_core::storage_internal::generate_storage_maintenance_operation_id()
                    .map_err(PurgeRewriteError::Identity)?;
            wal.commit_manifest_snapshot(
                &destination_lock,
                operation_id,
                manifest_segments.clone(),
                &staged_segments,
            )
            .map_err(PurgeRewriteError::SnapshotCommit)?;
            RecoveryManager::new(stage_layout.clone())
                .recover(&destination_lock)
                .map_err(PurgeRewriteError::Recovery)?;
        }

        let publication_revision = source_head
            .revision()
            .next_commit()
            .map_err(PurgeRewriteError::Revision)?;
        let carried_policy = SecurityPolicyVersion::new(
            publication_revision,
            latest_security.version().epoch(),
            latest_security.version().snapshot().clone(),
        );
        let final_policy_segment = security_store
            .stage_version(
                &destination_lock,
                &carried_policy,
                None,
                latest_security.audit_retention(),
            )
            .map_err(PurgeRewriteError::SecuritySegment)?;
        let final_policy_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            final_policy_segment.id(),
            final_policy_segment.content_digest(),
            publication_revision,
        );
        manifest_segments.push(final_policy_reference);
        staged_segments.clear();
        staged_segments.push(final_policy_reference);

        let operation_id =
            worlddb_core::storage_internal::generate_purge_publication_operation_id()
                .map_err(PurgeRewriteError::Identity)?;
        let audit_record_id =
            worlddb_core::storage_internal::generate_purge_publication_audit_record_id()
                .map_err(PurgeRewriteError::Identity)?;
        let audit_operation_id =
            worlddb_core::storage_internal::generate_purge_publication_audit_operation_id()
                .map_err(PurgeRewriteError::Identity)?;
        let previous_audit_sequence = wal
            .committed_required_audit_records(&destination_lock)
            .map_err(PurgeRewriteError::RequiredAudit)?
            .last()
            .map_or(AuditSequence::new(0), |entry| entry.record().sequence());
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id: audit_record_id,
                sequence: previous_audit_sequence
                    .next()
                    .map_err(PurgeRewriteError::AuditSequence)?,
                audit_operation_id,
            },
            AuditRecordDetails {
                actor: policy.principal_id(),
                action: AuditAction::PurgePublication,
                object_class: AuditObjectClass::Database,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: publication_revision,
                    operation_id,
                },
                security_epoch: policy.current_epoch(),
                policy_fingerprint,
            },
        );
        let report = PurgeReport {
            source_database_id,
            destination_database_id,
            source_revision: source_head.revision(),
            destination_revision: publication_revision,
            source_artifact_digest: plan.source_artifact_digest(),
            plan_fingerprint: plan.fingerprint(),
            operation_id,
            audit_record_id,
            removed_records: revalidated_plan.affected_records(),
            id_mappings,
            retained_external_artifacts: revalidated_plan.retained_external_artifacts().to_vec(),
            external_inventory_complete: revalidated_plan.external_inventory_complete(),
            index_families_to_rebuild: PurgePlan::index_families_to_rebuild().to_vec(),
        };
        let report_bytes = report.encode()?;
        write_report(&stage_layout, &report_bytes)?;

        if checkpoint(PurgeRewriteCheckpoint::BeforeAuditCommit) {
            return Err(PurgeRewriteError::InjectedBeforeAuditCommit);
        }

        let receipt = wal
            .commit_audited_manifest_snapshot(
                &destination_lock,
                operation_id,
                manifest_segments,
                &staged_segments,
                &audit_record,
            )
            .map_err(PurgeRewriteError::SnapshotCommit)?;
        if receipt.revision() != publication_revision {
            return Err(PurgeRewriteError::DestinationVerification);
        }
        RecoveryManager::new(stage_layout.clone())
            .recover(&destination_lock)
            .map_err(PurgeRewriteError::Recovery)?;
        verify_published_state(
            &stage_layout,
            &destination_lock,
            publication_revision,
            operation_id,
            &audit_record,
        )?;
        verify_rewritten_records(
            &stage_layout,
            &destination_lock,
            &scope,
            policy,
            &source_export,
            &revalidated_plan,
        )?;
        if fs::read(stage_layout.root().join(REPORT_FILE)).map_err(|source| {
            PurgeRewriteError::Io {
                operation: "read staged PurgeReport for verification",
                source,
            }
        })? != report_bytes
        {
            return Err(PurgeRewriteError::ReportVerification);
        }
        drop(destination_lock);

        if checkpoint(PurgeRewriteCheckpoint::BeforePublish) {
            return Err(PurgeRewriteError::InjectedBeforePublish);
        }

        if let Err(source) = crate::backup::publish_restore_directory(&stage, &target) {
            if source.kind() == io::ErrorKind::AlreadyExists {
                return Err(PurgeRewriteError::TargetAlreadyExists);
            }
            return Err(PurgeRewriteError::Io {
                operation: "atomically publish purged database directory",
                source,
            });
        }
        stage_guard.mark_published();
        if let Err(source) =
            crate::manifest::sync_directory(target.parent().unwrap_or_else(|| Path::new(".")))
        {
            return Err(PurgeRewriteError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: Some(source),
            });
        }
        if checkpoint(PurgeRewriteCheckpoint::AfterPublish) {
            return Err(PurgeRewriteError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            });
        }
        let published_layout = DatabaseLayout::open(&target).map_err(|error| {
            PurgeRewriteError::PublishedOutcomeUnknown {
                target: target.clone(),
                operation_id,
                source: Some(io::Error::other(error.to_string())),
            }
        })?;
        if published_layout.database_id() != Some(destination_database_id) {
            return Err(PurgeRewriteError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            });
        }
        let published_lock = published_layout.try_writer_lock().map_err(|error| {
            PurgeRewriteError::PublishedOutcomeUnknown {
                target: target.clone(),
                operation_id,
                source: Some(io::Error::other(error.to_string())),
            }
        })?;
        verify_published_state(
            &published_layout,
            &published_lock,
            publication_revision,
            operation_id,
            &audit_record,
        )
        .map_err(|error| PurgeRewriteError::PublishedOutcomeUnknown {
            target: target.clone(),
            operation_id,
            source: Some(io::Error::other(error.to_string())),
        })?;
        if fs::read(published_layout.root().join(REPORT_FILE)).map_err(|source| {
            PurgeRewriteError::PublishedOutcomeUnknown {
                target: target.clone(),
                operation_id,
                source: Some(source),
            }
        })? != report_bytes
        {
            return Err(PurgeRewriteError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            });
        }
        drop(published_lock);
        drop(source_lock);

        Ok(PurgeRewriteReceipt {
            destination: target,
            report,
        })
    }
}

fn authorize_purge(
    policy: SecurityPolicyView<'_>,
    target: PolicyTarget,
) -> Result<(), PurgeRewriteError> {
    if policy
        .current_snapshot()
        .authorize(policy.principal_id(), Capability::Purge, target)
        == AuthorizationDecision::Allow
    {
        Ok(())
    } else {
        Err(PurgeRewriteError::AuthorizationDenied)
    }
}

fn read_source_security_segments(
    source: &DatabaseLayout,
    references: &[ManifestSegmentReference],
) -> Result<Vec<crate::SecurityPolicySegment>, PurgeRewriteError> {
    let store = SecurityPolicyHistoryStore::new(source.clone());
    let mut segments = Vec::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
    {
        let segment = store
            .read_version(reference.id())
            .map_err(PurgeRewriteError::SecuritySegment)?;
        if segment.content_digest() != reference.content_digest()
            || segment.version().revision() != reference.through_revision()
        {
            return Err(PurgeRewriteError::SourceSnapshotChanged);
        }
        segments.push(segment);
    }
    segments.sort_by_key(|segment| segment.version().revision());
    if segments.windows(2).any(|pair| {
        matches!(pair, [left, right] if left.version().revision() == right.version().revision())
    }) {
        return Err(PurgeRewriteError::SecurityHistoryInvalid);
    }
    Ok(segments)
}

fn build_id_mappings(
    source: &LogicalExport,
    plan: &PurgePlan,
) -> Result<Vec<PurgeIdMapping>, PurgeRewriteError> {
    let mut destinations = BTreeMap::<LogicalImportIdentity, bool>::new();
    for space in source.manifest().visible_history_spaces() {
        destinations.insert(LogicalImportIdentity::HistorySpace(space.id()), false);
    }
    for entry in source.records() {
        let affected = plan
            .is_affected(entry.record())
            .map_err(PurgeRewriteError::PurgePlan)?;
        for identity in record_defined_identities(entry.record().record()) {
            destinations
                .entry(identity)
                .and_modify(|retained| *retained |= !affected)
                .or_insert(!affected);
        }
    }
    Ok(destinations
        .into_iter()
        .map(|(source, retained)| PurgeIdMapping {
            source,
            destination: retained.then_some(source),
        })
        .collect())
}

fn verify_published_state(
    layout: &DatabaseLayout,
    lock: &crate::WriterLock,
    expected_revision: Revision,
    operation_id: worlddb_core::OperationId,
    expected_record: &AuditRecord,
) -> Result<(), PurgeRewriteError> {
    let verify = StorageVerifier::new(layout.clone())
        .verify(lock)
        .map_err(PurgeRewriteError::StorageVerify)?;
    if !verify.is_clean() || verify.safe_revision() != expected_revision {
        return Err(PurgeRewriteError::DestinationVerification);
    }
    let audited = WalPrepareLog::new(layout)
        .committed_required_audit_records(lock)
        .map_err(PurgeRewriteError::RequiredAudit)?
        .into_iter()
        .any(|entry| {
            entry.revision() == expected_revision
                && entry.operation_id() == operation_id
                && entry.record() == expected_record
        });
    if !audited {
        return Err(PurgeRewriteError::RequiredAuditMissing);
    }
    Ok(())
}

fn verify_rewritten_records(
    layout: &DatabaseLayout,
    lock: &crate::WriterLock,
    scope: &LogicalExportScope,
    policy: SecurityPolicyView<'_>,
    source: &LogicalExport,
    plan: &PurgePlan,
) -> Result<(), PurgeRewriteError> {
    let rewritten = LogicalExportManager::new(layout.clone())
        .export_locked(scope.clone(), policy, lock)
        .map_err(PurgeRewriteError::Export)?;
    let mut expected = Vec::new();
    for entry in source.records() {
        if !plan
            .is_affected(entry.record())
            .map_err(PurgeRewriteError::PurgePlan)?
        {
            expected
                .push(encode_decoded_record(entry.record()).map_err(PurgeRewriteError::Record)?);
        }
    }
    let mut actual = rewritten
        .records()
        .iter()
        .map(|entry| encode_decoded_record(entry.record()).map_err(PurgeRewriteError::Record))
        .collect::<Result<Vec<_>, _>>()?;
    expected.sort_unstable();
    actual.sort_unstable();
    if actual != expected {
        return Err(PurgeRewriteError::DestinationVerification);
    }
    Ok(())
}

fn normalize_target(target: &Path, source_root: &Path) -> Result<PathBuf, PurgeRewriteError> {
    let file_name = target.file_name().ok_or(PurgeRewriteError::InvalidTarget)?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(PurgeRewriteError::TargetParentMissing);
    }
    let canonical_parent = fs::canonicalize(parent).map_err(|source| PurgeRewriteError::Io {
        operation: "resolve purge destination parent",
        source,
    })?;
    let candidate = canonical_parent.join(file_name);
    match fs::symlink_metadata(&candidate) {
        Ok(_) => return Err(PurgeRewriteError::TargetAlreadyExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(PurgeRewriteError::Io {
                operation: "inspect purge destination",
                source,
            });
        }
    }
    if candidate.starts_with(source_root) || source_root.starts_with(&candidate) {
        return Err(PurgeRewriteError::InvalidTarget);
    }
    Ok(candidate)
}

fn create_stage_path(target: &Path) -> Result<PathBuf, PurgeRewriteError> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..MAX_STAGE_ATTEMPTS {
        let sequence = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".worlddb-purge-stage-{}-{sequence}",
            std::process::id()
        ));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(source) => {
                return Err(PurgeRewriteError::Io {
                    operation: "inspect purge staging path",
                    source,
                });
            }
        }
    }
    Err(PurgeRewriteError::StagingNameExhausted)
}

fn write_report(layout: &DatabaseLayout, bytes: &[u8]) -> Result<(), PurgeRewriteError> {
    let path = layout.root().join(REPORT_FILE);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|source| PurgeRewriteError::Io {
            operation: "create staged PurgeReport",
            source,
        })?;
    file.write_all(bytes)
        .map_err(|source| PurgeRewriteError::Io {
            operation: "write staged PurgeReport",
            source,
        })?;
    file.sync_all().map_err(|source| PurgeRewriteError::Io {
        operation: "sync staged PurgeReport",
        source,
    })?;
    crate::manifest::sync_directory(layout.root()).map_err(|source| PurgeRewriteError::Io {
        operation: "sync staged database after PurgeReport",
        source,
    })
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_count(bytes: &mut Vec<u8>, value: usize) -> Result<(), PurgeRewriteError> {
    let count = u32::try_from(value).map_err(|_| PurgeRewriteError::ResourceLimit)?;
    bytes.extend_from_slice(&count.to_le_bytes());
    Ok(())
}

struct PurgeStageGuard {
    path: PathBuf,
    published: bool,
}

impl PurgeStageGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            published: false,
        }
    }

    fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for PurgeStageGuard {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Why an approved offline purge could not be rewritten or published.
#[derive(Debug)]
pub enum PurgeRewriteError {
    /// The policy does not grant current Purge capability.
    AuthorizationDenied,
    /// The plan has not received an explicit RejectIfReferenced or exact cascade approval.
    ApprovalMissing,
    /// The supplied source bytes differ from the plan's bound artifact.
    SourceArtifactMismatch,
    /// The live source DatabaseId differs from the plan.
    SourceIdentityMismatch,
    /// The source is not a clean verified snapshot.
    SourceNotClean,
    /// The source WAL and current manifest no longer name the same snapshot.
    SourceSnapshotChanged,
    /// The current sidecar inventory differs from the inventory explicitly reviewed.
    SidecarInventoryChanged,
    /// Recomputed plan fingerprint differs from the approved plan.
    PlanFingerprintMismatch,
    /// The source snapshot has no initial policy history segment.
    SecurityHistoryMissing,
    /// The source policy history contains invalid or duplicate revision rows.
    SecurityHistoryInvalid,
    /// Caller policy epoch differs from the persisted source policy epoch.
    PolicySnapshotMismatch,
    /// The new database identity collided with the source identity.
    IdentityCollision,
    /// The requested destination is unsafe or overlaps the source database.
    InvalidTarget,
    /// The destination parent does not exist or is not a directory.
    TargetParentMissing,
    /// The destination already exists and is never overwritten.
    TargetAlreadyExists,
    /// A unique sibling staging directory could not be allocated.
    StagingNameExhausted,
    /// A configured resource bound was exceeded.
    ResourceLimit,
    /// Target verify did not establish the exact expected records, audit, and revision.
    DestinationVerification,
    /// Required PurgePublication audit record is absent from the committed WAL.
    RequiredAuditMissing,
    /// The staged or published report does not match its canonical bytes.
    ReportVerification,
    /// A failure was injected before the PurgePublication and audit commit.
    InjectedBeforeAuditCommit,
    /// A failure was injected after verification but before destination publication.
    InjectedBeforePublish,
    /// Destination publication succeeded but final durability or verification is uncertain.
    PublishedOutcomeUnknown {
        target: PathBuf,
        operation_id: worlddb_core::OperationId,
        source: Option<io::Error>,
    },
    /// Filesystem operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// Purge planning failed while revalidating its closure.
    PurgePlan(PurgeError),
    /// Current local index inventory could not be verified.
    IndexInventory(crate::IndexRebuildError),
    /// Source or destination export failed.
    Export(LogicalExportError),
    /// Source or destination layout failed validation.
    StorageFile(StorageFileError),
    /// Source or destination has no durable DatabaseId.
    DatabaseIdentityMissing,
    /// Writer lock could not be acquired.
    WriterLock(WriterLockError),
    /// Storage verification failed.
    StorageVerify(StorageVerifyError),
    /// Manifest validation or publication failed.
    Manifest(crate::ManifestError),
    /// History segment operation failed.
    Segment(SegmentError),
    /// Security history operation failed.
    SecuritySegment(SecurityPolicyStorageError),
    /// Recovery did not establish a writable committed prefix.
    Recovery(RecoveryError),
    /// WAL commit failed.
    Wal(WalError),
    /// Required Audit validation or read failed.
    RequiredAudit(RequiredAuditError),
    /// Audited manifest transaction failed.
    SnapshotCommit(SnapshotCommitError),
    /// Identity generation failed.
    Identity(worlddb_core::IdGenerationError),
    /// Revision or audit sequence could not advance.
    Revision(worlddb_core::RevisionError),
    /// Audit sequence allocation failed.
    AuditSequence(worlddb_core::AuditSequenceError),
    /// Re-encoding one logical record failed.
    Record(worlddb_core::RecordCodecError),
}

impl fmt::Display for PurgeRewriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorizationDenied => formatter.write_str("current Purge permission is denied"),
            Self::ApprovalMissing => formatter.write_str("purge plan has no explicit approval"),
            Self::SourceArtifactMismatch => {
                formatter.write_str("source artifact differs from the approved plan")
            }
            Self::SourceIdentityMismatch => {
                formatter.write_str("source DatabaseId differs from the approved plan")
            }
            Self::SourceNotClean => formatter.write_str("source is not a clean verified snapshot"),
            Self::SourceSnapshotChanged => {
                formatter.write_str("source snapshot changed after purge planning")
            }
            Self::SidecarInventoryChanged => {
                formatter.write_str("sidecar inventory changed after purge planning")
            }
            Self::PlanFingerprintMismatch => {
                formatter.write_str("revalidated purge plan differs from the approved plan")
            }
            Self::SecurityHistoryMissing => {
                formatter.write_str("source security history has no initial version")
            }
            Self::SecurityHistoryInvalid => {
                formatter.write_str("source security history has duplicate revisions")
            }
            Self::PolicySnapshotMismatch => formatter
                .write_str("current authorization epoch differs from persisted source policy"),
            Self::IdentityCollision => {
                formatter.write_str("new DatabaseId collided with source DatabaseId")
            }
            Self::InvalidTarget => {
                formatter.write_str("purge destination overlaps the source or is invalid")
            }
            Self::TargetParentMissing => {
                formatter.write_str("purge destination parent does not exist")
            }
            Self::TargetAlreadyExists => {
                formatter.write_str("purge destination already exists and was not overwritten")
            }
            Self::StagingNameExhausted => {
                formatter.write_str("purge could not allocate a unique staging directory")
            }
            Self::ResourceLimit => {
                formatter.write_str("purge rewrite exceeded a registered resource bound")
            }
            Self::DestinationVerification => {
                formatter.write_str("rewritten database failed exact verification")
            }
            Self::RequiredAuditMissing => {
                formatter.write_str("Required PurgePublication audit record is absent")
            }
            Self::ReportVerification => {
                formatter.write_str("PurgeReport bytes differ from the verified report")
            }
            Self::InjectedBeforeAuditCommit => {
                formatter.write_str("purge fault injected before audited publication commit")
            }
            Self::InjectedBeforePublish => {
                formatter.write_str("purge fault injected before destination publication")
            }
            Self::PublishedOutcomeUnknown {
                target,
                operation_id,
                source,
            } => {
                write!(
                    formatter,
                    "purge publication operation {operation_id} is visible at {} but its final outcome is uncertain",
                    target.display()
                )?;
                if let Some(source) = source {
                    write!(formatter, ": {source}")?;
                }
                Ok(())
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::PurgePlan(error) => write!(formatter, "purge plan: {error}"),
            Self::IndexInventory(error) => write!(formatter, "index inventory: {error}"),
            Self::Export(error) => write!(formatter, "logical export: {error}"),
            Self::StorageFile(error) => write!(formatter, "database layout: {error}"),
            Self::DatabaseIdentityMissing => formatter.write_str("durable DatabaseId is missing"),
            Self::WriterLock(error) => write!(formatter, "database writer lock: {error}"),
            Self::StorageVerify(error) => write!(formatter, "storage verification: {error}"),
            Self::Manifest(error) => write!(formatter, "manifest: {error}"),
            Self::Segment(error) => write!(formatter, "history segment: {error}"),
            Self::SecuritySegment(error) => write!(formatter, "security history: {error}"),
            Self::Recovery(error) => write!(formatter, "recovery: {error}"),
            Self::Wal(error) => write!(formatter, "WAL: {error}"),
            Self::RequiredAudit(error) => write!(formatter, "required audit: {error}"),
            Self::SnapshotCommit(error) => write!(formatter, "audited manifest commit: {error}"),
            Self::Identity(error) => write!(formatter, "identity generation: {error}"),
            Self::Revision(error) => write!(formatter, "revision allocation: {error}"),
            Self::AuditSequence(error) => write!(formatter, "audit sequence allocation: {error}"),
            Self::Record(error) => write!(formatter, "record frame: {error}"),
        }
    }
}

impl std::error::Error for PurgeRewriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::PublishedOutcomeUnknown { source, .. } => source
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::PurgePlan(error) => Some(error),
            Self::IndexInventory(error) => Some(error),
            Self::Export(error) => Some(error),
            Self::StorageFile(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            Self::StorageVerify(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::Segment(error) => Some(error),
            Self::SecuritySegment(error) => Some(error),
            Self::Recovery(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::RequiredAudit(error) => Some(error),
            Self::SnapshotCommit(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::Revision(error) => Some(error),
            Self::AuditSequence(error) => Some(error),
            Self::Record(error) => Some(error),
            _ => None,
        }
    }
}

impl From<crate::logical_import::LogicalImportError> for PurgeRewriteError {
    fn from(error: crate::logical_import::LogicalImportError) -> Self {
        Self::PurgePlan(PurgeError::Identity(error))
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{
        AuditAction, AuditPolicyFingerprint, AuthorizationMode, Bytes, Capability, CapabilityGrant,
        CapabilityRule, DomainId, Entity, EntityId, EntityTypeDefinition, EntityTypeId,
        GrantEffect, HistorySpaceDefinition, HistorySpaceId, Lifecycle, OperationId, PolicyRuleId,
        PolicyScope, PolicySubject, PolicyTarget, Principal, PrincipalId, Record, RecordKind,
        Revision, SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot,
        SecurityPolicyVersion, Symbol,
    };

    use crate::{
        DatabaseLayout, HistorySegmentStore, IndexGenerationStore, LogicalExportManager,
        LogicalExportScope, LogicalImportIdentity, ManifestSegmentKind, ManifestSegmentReference,
        PurgePlan, PurgePlanManager, PurgeRewriteRequest, PurgeSidecarInventory, RecoveryManager,
        SecurityPolicyHistoryStore, StorageVerifier, WalPrepareLog,
    };

    use super::{PurgeRewriteCheckpoint, PurgeRewriteError, PurgeRewriteManager};

    static NEXT_AREA: AtomicU64 = AtomicU64::new(0);

    struct TestArea(PathBuf);

    impl TestArea {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_AREA.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-purge-rewrite-fault-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }
    }

    impl Drop for TestArea {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        _area: TestArea,
        source: DatabaseLayout,
        source_artifact: Vec<u8>,
        plan: PurgePlan,
        permissions: SecurityPolicyHistory,
        destination: PathBuf,
    }

    impl Fixture {
        fn request(&self) -> Result<PurgeRewriteRequest<'_>, String> {
            let policy = self
                .permissions
                .select(
                    AuthorizationMode::Now,
                    id::<PrincipalId>(1)?,
                    Revision::GENESIS,
                )
                .map_err(|error| error.to_string())?;
            Ok(PurgeRewriteRequest {
                source_root: self.source.root(),
                source_artifact: &self.source_artifact,
                plan: &self.plan,
                known_external_artifacts: &[],
                external_inventory_complete: true,
                destination: &self.destination,
                policy,
                policy_target: PolicyTarget::default(),
                policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(vec![0x4d, 0x37, 0x15]))
                    .map_err(|error| error.to_string())?,
            })
        }
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn permissions() -> Result<SecurityPolicyHistory, String> {
        let principal = id::<PrincipalId>(1)?;
        let rules = [Capability::Purge, Capability::DataExport]
            .into_iter()
            .enumerate()
            .map(|(index, capability)| {
                Ok(CapabilityRule::new(
                    id::<PolicyRuleId>(
                        2 + u8::try_from(index).map_err(|error| error.to_string())?,
                    )?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(capability, GrantEffect::Allow),
                    PolicyScope::project(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let snapshot =
            SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
                .map_err(|error| error.to_string())?;
        SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                snapshot,
            )],
        )
        .map_err(|error| error.to_string())
    }

    fn fixture() -> Result<Fixture, String> {
        let area = TestArea::create()?;
        let source =
            DatabaseLayout::create(area.0.join("source")).map_err(|error| error.to_string())?;
        let space = id::<HistorySpaceId>(4)?;
        let entity_type = id::<EntityTypeId>(5)?;
        let entity = id::<EntityId>(6)?;
        let lock = source
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let records = [
            Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(space, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            ),
            Record::EntityTypeDefinition(EntityTypeDefinition::new(
                entity_type,
                Symbol::new("fault_fixture").map_err(|error| error.to_string())?,
                None,
                Lifecycle::Active,
                Revision::FIRST_COMMIT,
            )),
            Record::Entity(Entity::new(entity, entity_type, Revision::FIRST_COMMIT)),
        ];
        let history_receipt = HistorySegmentStore::new(source.clone())
            .write_segment(&lock, &records)
            .map_err(|error| error.to_string())?;
        let permissions = permissions()?;
        let policy_version = permissions
            .versions()
            .first()
            .ok_or_else(|| String::from("initial policy missing"))?;
        let security_receipt = SecurityPolicyHistoryStore::new(source.clone())
            .write_version(&lock, policy_version, None, None)
            .map_err(|error| error.to_string())?;
        let references = vec![
            ManifestSegmentReference::new(
                ManifestSegmentKind::History,
                history_receipt.id(),
                history_receipt.content_digest(),
                Revision::FIRST_COMMIT,
            ),
            ManifestSegmentReference::new(
                ManifestSegmentKind::SecurityPolicy,
                security_receipt.id(),
                security_receipt.content_digest(),
                Revision::GENESIS,
            ),
        ];
        WalPrepareLog::new(&source)
            .commit_manifest_snapshot(&lock, id::<OperationId>(7)?, references, &[])
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(source.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        drop(lock);

        let principal = id::<PrincipalId>(1)?;
        let policy_view = permissions
            .select(AuthorizationMode::Now, principal, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        let record_kinds = RecordKind::ALL
            .into_iter()
            .filter(|kind| {
                !matches!(
                    kind,
                    RecordKind::MigrationPlan
                        | RecordKind::MigrationRun
                        | RecordKind::MigrationStepCommitIdentity
                )
            })
            .collect();
        let scope = LogicalExportScope::new(
            Revision::GENESIS,
            Revision::FIRST_COMMIT,
            vec![space],
            record_kinds,
        )
        .map_err(|error| error.to_string())?;
        let artifact = LogicalExportManager::new(source.clone())
            .export(scope, policy_view)
            .and_then(|export| export.encode())
            .map_err(|error| error.to_string())?;
        let sidecars = PurgeSidecarInventory::new(
            IndexGenerationStore::inventory_all(&source).map_err(|error| error.to_string())?,
            Vec::new(),
            true,
        )
        .map_err(|error| error.to_string())?;
        let preview = PurgePlanManager::preview(
            &artifact,
            vec![LogicalImportIdentity::Entity(entity)],
            sidecars,
        )
        .map_err(|error| error.to_string())?;
        let plan = preview
            .approve_reject_if_referenced()
            .map_err(|error| error.to_string())?;

        Ok(Fixture {
            destination: area.0.join("purged"),
            _area: area,
            source,
            source_artifact: artifact,
            plan,
            permissions,
        })
    }

    #[test]
    fn publication_faults_never_expose_an_unaudited_database() -> Result<(), String> {
        for checkpoint in [
            PurgeRewriteCheckpoint::BeforeAuditCommit,
            PurgeRewriteCheckpoint::BeforePublish,
        ] {
            let fixture = fixture()?;
            let request = fixture.request()?;
            let result = PurgeRewriteManager::new()
                .rewrite_with_checkpoint(request, |actual| actual == checkpoint);
            let expected_error = match checkpoint {
                PurgeRewriteCheckpoint::BeforeAuditCommit => {
                    matches!(&result, Err(PurgeRewriteError::InjectedBeforeAuditCommit))
                }
                PurgeRewriteCheckpoint::BeforePublish => {
                    matches!(&result, Err(PurgeRewriteError::InjectedBeforePublish))
                }
                PurgeRewriteCheckpoint::AfterPublish => unreachable!(),
            };
            assert!(
                expected_error,
                "unexpected injected fault result: {result:?}"
            );
            assert!(!fixture.destination.exists());
        }

        let fixture = fixture()?;
        let request = fixture.request()?;
        let error = match PurgeRewriteManager::new().rewrite_with_checkpoint(request, |actual| {
            actual == PurgeRewriteCheckpoint::AfterPublish
        }) {
            Err(error) => error,
            Ok(_) => return Err(String::from("injected post-publication fault was ignored")),
        };
        let target = match error {
            PurgeRewriteError::PublishedOutcomeUnknown {
                target,
                operation_id: _,
                source: None,
            } => target,
            other => return Err(format!("unexpected injected fault result: {other}")),
        };
        assert_eq!(target.file_name(), fixture.destination.file_name());
        assert!(fixture.destination.exists());
        let published = DatabaseLayout::open(&target).map_err(|error| error.to_string())?;
        let lock = published
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let verification = StorageVerifier::new(published.clone())
            .verify(&lock)
            .map_err(|error| error.to_string())?;
        assert!(verification.is_clean());
        let audits = WalPrepareLog::new(&published)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        assert!(
            audits
                .iter()
                .any(|entry| { entry.record().action() == AuditAction::PurgePublication })
        );
        drop(lock);
        Ok(())
    }
}
