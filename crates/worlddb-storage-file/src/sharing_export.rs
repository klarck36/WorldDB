//! Security-filtered sharing exports with durable authorization and completion audit boundaries.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use worlddb_core::{
    ArchiveTargetRef, AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome,
    AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence,
    AuthorizationDecision, Bytes, Capability, DecodedRecord, DomainId, EventRelationKind,
    EvidenceTargetRef, FieldSelector, MaskSelector, PolicyTarget, ProvenanceEndpointRef,
    ProvenanceRelation, ProvenanceRelationship, Record, RecordKind, RecordRef,
    RelationshipSelector, Revision, SecurityPolicyView, decode_record, encode_decoded_record,
};

use crate::{
    DatabaseLayout, LogicalExport, LogicalExportError, LogicalExportManager, LogicalExportScope,
    Manifest, ManifestSegmentReference, ManifestStore, RecoveryManager, RequiredAuditError,
    SnapshotCommitError, StorageFileError, StorageVerifier, StorageVerifyError, WalError,
    WalPrepareLog, WriterLockError,
};

const SHARING_EXPORT_MAGIC: &[u8; 8] = b"WDBSE\0\0\x01";
const SHARING_EXPORT_CONTEXT: &[u8] = b"WorldDB.SharingExport.v1\0";
const SHARING_EXPORT_MAX_BYTES: usize = 512 * 1024 * 1024;
const SHARING_EXPORT_MAX_RECORDS: usize = 1_000_000;
const SHARING_EXPORT_MAX_SPACES: usize = 65_536;
const HEADER_BYTES: usize = 8 + 8;
const DIGEST_BYTES: usize = 32;

/// The caller's explicit HistorySpace, Transaction-Time and record-class selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharingExportScope {
    logical: LogicalExportScope,
}

impl SharingExportScope {
    /// Creates a canonical, non-empty sharing scope.
    pub fn new(
        from_revision: Revision,
        through_revision: Revision,
        history_spaces: Vec<worlddb_core::HistorySpaceId>,
        record_kinds: Vec<RecordKind>,
    ) -> Result<Self, SharingExportError> {
        Ok(Self {
            logical: LogicalExportScope::new(
                from_revision,
                through_revision,
                history_spaces,
                record_kinds,
            )
            .map_err(SharingExportError::LogicalExport)?,
        })
    }

    /// Inclusive lower Transaction-Time revision.
    #[must_use]
    pub const fn from_revision(&self) -> Revision {
        self.logical.from_revision()
    }

    /// Inclusive upper Transaction-Time revision.
    #[must_use]
    pub const fn through_revision(&self) -> Revision {
        self.logical.through_revision()
    }

    /// Explicitly selected HistorySpaces in canonical order.
    #[must_use]
    pub fn history_spaces(&self) -> &[worlddb_core::HistorySpaceId] {
        self.logical.history_spaces()
    }

    /// Explicitly selected record classes in canonical order.
    #[must_use]
    pub fn record_kinds(&self) -> &[RecordKind] {
        self.logical.record_kinds()
    }
}

/// A standalone sharing artifact containing only records authorized for the caller.
///
/// The wire form carries the caller's explicit scope and included records. It has no source
/// DatabaseId, snapshot revision, omitted-class table, source totals, or exact-backup metadata.
#[derive(Clone, Debug)]
pub struct SharingExport {
    from_revision: Revision,
    through_revision: Revision,
    history_spaces: Vec<worlddb_core::HistorySpaceId>,
    records: Vec<DecodedRecord>,
}

impl SharingExport {
    /// Inclusive lower Transaction-Time revision requested by the caller.
    #[must_use]
    pub const fn from_revision(&self) -> Revision {
        self.from_revision
    }

    /// Inclusive upper Transaction-Time revision requested by the caller.
    #[must_use]
    pub const fn through_revision(&self) -> Revision {
        self.through_revision
    }

    /// Explicit HistorySpaces requested by the caller.
    #[must_use]
    pub fn history_spaces(&self) -> &[worlddb_core::HistorySpaceId] {
        &self.history_spaces
    }

    /// Records that passed the current record, field, relationship, and dependency checks.
    #[must_use]
    pub fn records(&self) -> &[DecodedRecord] {
        &self.records
    }

    /// Encodes a canonical sharing artifact with an integrity digest.
    pub fn encode(&self) -> Result<Vec<u8>, SharingExportError> {
        validate_sharing_export(self)?;
        let mut payload = Vec::new();
        put_u64(&mut payload, self.from_revision.value());
        put_u64(&mut payload, self.through_revision.value());
        put_u32(
            &mut payload,
            u32::try_from(self.history_spaces.len())
                .map_err(|_| SharingExportError::ResourceLimit)?,
        );
        for space in &self.history_spaces {
            payload.extend_from_slice(&space.to_bytes());
        }
        put_u32(
            &mut payload,
            u32::try_from(self.records.len()).map_err(|_| SharingExportError::ResourceLimit)?,
        );
        for record in &self.records {
            let frame = encode_decoded_record(record).map_err(SharingExportError::Record)?;
            put_bytes_u32(&mut payload, &frame)?;
            if payload.len() > SHARING_EXPORT_MAX_BYTES {
                return Err(SharingExportError::ResourceLimit);
            }
        }
        let total_len = HEADER_BYTES
            .checked_add(payload.len())
            .and_then(|length| length.checked_add(DIGEST_BYTES))
            .ok_or(SharingExportError::ResourceLimit)?;
        if total_len > SHARING_EXPORT_MAX_BYTES {
            return Err(SharingExportError::ResourceLimit);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total_len)
            .map_err(|_| SharingExportError::AllocationFailed)?;
        bytes.extend_from_slice(SHARING_EXPORT_MAGIC);
        put_u64(
            &mut bytes,
            u64::try_from(payload.len()).map_err(|_| SharingExportError::ResourceLimit)?,
        );
        bytes.extend_from_slice(&payload);
        let mut digest = blake3::Hasher::new();
        digest.update(SHARING_EXPORT_CONTEXT);
        digest.update(&bytes);
        bytes.extend_from_slice(digest.finalize().as_bytes());
        Ok(bytes)
    }

    /// Decodes a sharing artifact and checks its digest, bounds, and canonical byte form.
    pub fn decode(bytes: &[u8]) -> Result<Self, SharingExportError> {
        if bytes.len() < HEADER_BYTES + DIGEST_BYTES
            || bytes.len() > SHARING_EXPORT_MAX_BYTES
            || bytes.get(..8) != Some(SHARING_EXPORT_MAGIC.as_slice())
        {
            return Err(SharingExportError::InvalidEncoding);
        }
        let digest_offset = bytes
            .len()
            .checked_sub(DIGEST_BYTES)
            .ok_or(SharingExportError::InvalidEncoding)?;
        let claimed_digest: [u8; 32] = bytes
            .get(digest_offset..)
            .ok_or(SharingExportError::InvalidEncoding)?
            .try_into()
            .map_err(|_| SharingExportError::InvalidEncoding)?;
        let mut digest = blake3::Hasher::new();
        digest.update(SHARING_EXPORT_CONTEXT);
        digest.update(
            bytes
                .get(..digest_offset)
                .ok_or(SharingExportError::InvalidEncoding)?,
        );
        if *digest.finalize().as_bytes() != claimed_digest {
            return Err(SharingExportError::DigestMismatch);
        }
        let payload_len =
            usize::try_from(read_u64(bytes, 8)?).map_err(|_| SharingExportError::ResourceLimit)?;
        let payload_end = HEADER_BYTES
            .checked_add(payload_len)
            .ok_or(SharingExportError::InvalidEncoding)?;
        if payload_end != digest_offset {
            return Err(SharingExportError::InvalidEncoding);
        }
        let mut cursor = Cursor::new(
            bytes
                .get(HEADER_BYTES..payload_end)
                .ok_or(SharingExportError::InvalidEncoding)?,
        );
        let from_revision =
            Revision::new(cursor.u64()?).map_err(|_| SharingExportError::InvalidEncoding)?;
        let through_revision =
            Revision::new(cursor.u64()?).map_err(|_| SharingExportError::InvalidEncoding)?;
        if from_revision > through_revision {
            return Err(SharingExportError::InvalidEncoding);
        }
        let space_count = cursor.count(SHARING_EXPORT_MAX_SPACES)?;
        if space_count == 0 {
            return Err(SharingExportError::InvalidEncoding);
        }
        let mut history_spaces = Vec::new();
        history_spaces
            .try_reserve_exact(space_count)
            .map_err(|_| SharingExportError::AllocationFailed)?;
        for _ in 0..space_count {
            history_spaces.push(cursor.id()?);
        }
        if history_spaces
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left >= right))
        {
            return Err(SharingExportError::NonCanonicalEncoding);
        }
        let record_count = cursor.count(SHARING_EXPORT_MAX_RECORDS)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(record_count)
            .map_err(|_| SharingExportError::AllocationFailed)?;
        for _ in 0..record_count {
            records.push(
                decode_record(cursor.bytes_u32(SHARING_EXPORT_MAX_BYTES)?)
                    .map_err(SharingExportError::Record)?,
            );
        }
        if !cursor.is_empty() {
            return Err(SharingExportError::InvalidEncoding);
        }
        let export = Self {
            from_revision,
            through_revision,
            history_spaces,
            records,
        };
        validate_sharing_export(&export)?;
        if export.encode()?.as_slice() != bytes {
            return Err(SharingExportError::NonCanonicalEncoding);
        }
        Ok(export)
    }
}

/// Creates sharing exports and durably audits authorization and successful completion.
#[derive(Clone, Debug)]
pub struct SharingExportManager {
    layout: DatabaseLayout,
}

impl SharingExportManager {
    /// Binds the manager to an opened database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Builds a filtered export. Authorization is checked before data reads, then its audit
    /// record is committed before the logical snapshot is read. No artifact is returned until
    /// the matching completion audit record has also recovered and verified.
    pub fn export(
        &self,
        scope: SharingExportScope,
        policy: SecurityPolicyView<'_>,
    ) -> Result<SharingExport, SharingExportError> {
        authorize_scope(&scope.logical, policy)?;
        let policy_fingerprint = policy_fingerprint(&scope.logical, policy)?;
        let audit_operation_id =
            worlddb_core::storage_internal::generate_sharing_export_audit_operation_id()
                .map_err(SharingExportError::Identity)?;
        commit_audit_boundary(
            &self.layout,
            policy,
            policy_fingerprint.clone(),
            audit_operation_id,
            AuditAction::ExportAuthorization,
        )?;

        let logical = LogicalExportManager::new(self.layout.clone())
            .export(scope.logical.clone(), policy)
            .map_err(SharingExportError::LogicalExport)?;
        let export = filtered_export(&scope.logical, &logical, policy)?;
        let bytes = export.encode()?;
        if bytes.is_empty() {
            return Err(SharingExportError::InvalidEncoding);
        }

        commit_audit_boundary(
            &self.layout,
            policy,
            policy_fingerprint,
            audit_operation_id,
            AuditAction::ExportCompletion,
        )?;
        Ok(export)
    }
}

fn authorize_scope(
    scope: &LogicalExportScope,
    policy: SecurityPolicyView<'_>,
) -> Result<(), SharingExportError> {
    let snapshot = policy.current_snapshot();
    let principal = policy.principal_id();
    let needs_project = scope.record_kinds().iter().copied().any(is_global_kind);
    for space in scope.history_spaces() {
        let target = PolicyTarget::new(Some(*space), None, None, None, None);
        for capability in [Capability::DataExport, Capability::HistorySpaceRead] {
            if snapshot.authorize(principal, capability, target) != AuthorizationDecision::Allow {
                return Err(SharingExportError::AuthorizationDenied);
            }
        }
    }
    if needs_project {
        let target = PolicyTarget::default();
        for capability in [Capability::DataExport, Capability::ProjectRead] {
            if snapshot.authorize(principal, capability, target) != AuthorizationDecision::Allow {
                return Err(SharingExportError::AuthorizationDenied);
            }
        }
    }
    Ok(())
}

fn is_global_kind(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::Entity
            | RecordKind::EntityRetirement
            | RecordKind::PerspectiveDefinitionRevision
            | RecordKind::PerspectiveRetirement
            | RecordKind::LayerDefinition
            | RecordKind::LayerSchemaSnapshot
            | RecordKind::EntityTypeDefinition
            | RecordKind::PredicateDefinition
            | RecordKind::EventKindDefinition
            | RecordKind::MigrationPlan
            | RecordKind::MigrationRun
            | RecordKind::MigrationStepCommitIdentity
            | RecordKind::Source
            | RecordKind::Evidence
            | RecordKind::Provenance
            | RecordKind::EvidenceRetraction
            | RecordKind::ProvenanceRetraction
    )
}

fn required_kind_capabilities(kind: RecordKind) -> Vec<Capability> {
    let mut capabilities = match kind {
        RecordKind::HistorySpaceDefinition => vec![Capability::HistorySpaceRead],
        RecordKind::Entity => vec![Capability::EntityRead, Capability::SchemaRead],
        RecordKind::EntityRetirement => vec![Capability::EntityRead],
        RecordKind::PerspectiveDefinitionRevision | RecordKind::PerspectiveRetirement => {
            vec![Capability::PerspectiveRead]
        }
        RecordKind::LayerDefinition
        | RecordKind::LayerSchemaSnapshot
        | RecordKind::EntityTypeDefinition
        | RecordKind::PredicateDefinition
        | RecordKind::EventKindDefinition => vec![Capability::SchemaRead],
        RecordKind::MigrationPlan
        | RecordKind::MigrationRun
        | RecordKind::MigrationStepCommitIdentity => vec![Capability::MigrationPlan],
        RecordKind::Assertion => vec![
            Capability::AssertionRead,
            Capability::EntityRead,
            Capability::SchemaRead,
            Capability::PerspectiveRead,
        ],
        RecordKind::AssertionValidityClosure | RecordKind::AssertionRetraction => {
            vec![Capability::AssertionRead]
        }
        RecordKind::Mask | RecordKind::MaskValidityClosure | RecordKind::MaskRetraction => {
            vec![
                Capability::MaskRead,
                Capability::AssertionRead,
                Capability::EntityRead,
                Capability::SchemaRead,
                Capability::PerspectiveRead,
            ]
        }
        RecordKind::ReplacementBoundary => vec![
            Capability::ReplacementBoundaryRead,
            Capability::EntityRead,
            Capability::SchemaRead,
            Capability::PerspectiveRead,
        ],
        RecordKind::ReplacementBoundaryValidityClosure
        | RecordKind::ReplacementBoundaryRetraction => {
            vec![Capability::ReplacementBoundaryRead]
        }
        RecordKind::ArchiveTransition => vec![Capability::LifecycleRead],
        RecordKind::Event => vec![
            Capability::EventRead,
            Capability::EntityRead,
            Capability::SchemaRead,
        ],
        RecordKind::EventSpanClosure | RecordKind::EventRetraction => {
            vec![Capability::EventRead]
        }
        RecordKind::EventMask => vec![Capability::EventMaskRead, Capability::EventRead],
        RecordKind::EventMaskRetraction => vec![Capability::EventMaskRead],
        RecordKind::EventRelation | RecordKind::EventRelationRetraction => {
            vec![Capability::EventRead]
        }
        RecordKind::Source => vec![Capability::SourceRead],
        RecordKind::Evidence | RecordKind::EvidenceRetraction => {
            vec![Capability::EvidenceRead]
        }
        RecordKind::Provenance | RecordKind::ProvenanceRetraction => {
            vec![Capability::ProvenanceRead]
        }
        RecordKind::TransferLineage => {
            vec![
                Capability::HistorySpaceRead,
                Capability::HistorySpaceTransfer,
            ]
        }
    };
    if kind != RecordKind::Source
        && matches!(
            kind,
            RecordKind::Assertion
                | RecordKind::Mask
                | RecordKind::ReplacementBoundary
                | RecordKind::Event
                | RecordKind::EventMask
        )
    {
        capabilities.push(Capability::LayerRead);
    }
    if matches!(
        kind,
        RecordKind::AssertionValidityClosure
            | RecordKind::AssertionRetraction
            | RecordKind::MaskValidityClosure
            | RecordKind::MaskRetraction
            | RecordKind::ReplacementBoundaryValidityClosure
            | RecordKind::ReplacementBoundaryRetraction
            | RecordKind::EventSpanClosure
            | RecordKind::EventRetraction
            | RecordKind::EventMaskRetraction
            | RecordKind::EventRelationRetraction
            | RecordKind::EvidenceRetraction
            | RecordKind::ProvenanceRetraction
            | RecordKind::ArchiveTransition
    ) && !capabilities.contains(&Capability::LifecycleRead)
    {
        capabilities.push(Capability::LifecycleRead);
    }
    capabilities
}

fn policy_fingerprint(
    scope: &LogicalExportScope,
    policy: SecurityPolicyView<'_>,
) -> Result<AuditPolicyFingerprint, SharingExportError> {
    let mut digest = blake3::Hasher::new();
    digest.update(b"WorldDB.SharingExport.PolicyScope.v1\0");
    digest.update(&policy.current_epoch().value().to_le_bytes());
    digest.update(
        &policy
            .current_snapshot()
            .effective_capability_fingerprint(policy.principal_id(), PolicyTarget::default()),
    );
    digest.update(&scope.from_revision().value().to_le_bytes());
    digest.update(&scope.through_revision().value().to_le_bytes());
    for kind in scope.record_kinds() {
        digest.update(&kind.number().to_le_bytes());
    }
    for space in scope.history_spaces() {
        digest.update(&space.to_bytes());
        digest.update(&policy.current_snapshot().effective_capability_fingerprint(
            policy.principal_id(),
            PolicyTarget::new(Some(*space), None, None, None, None),
        ));
    }
    AuditPolicyFingerprint::new(Bytes::new(digest.finalize().as_bytes().to_vec()))
        .map_err(SharingExportError::AuditFingerprint)
}

fn commit_audit_boundary(
    original_layout: &DatabaseLayout,
    policy: SecurityPolicyView<'_>,
    policy_fingerprint: AuditPolicyFingerprint,
    audit_operation_id: worlddb_core::AuditOperationId,
    action: AuditAction,
) -> Result<(), SharingExportError> {
    let layout =
        DatabaseLayout::open(original_layout.root()).map_err(SharingExportError::Layout)?;
    let lock = layout
        .try_writer_lock()
        .map_err(SharingExportError::WriterLock)?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(SharingExportError::Recovery)?;
    let before = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(SharingExportError::StorageVerify)?;
    if !before.is_clean() {
        return Err(SharingExportError::SourceNotClean);
    }
    let wal = WalPrepareLog::new(&layout);
    let head = wal.commit_head(&lock).map_err(SharingExportError::Wal)?;
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(SharingExportError::Manifest)?;
    let segments: Vec<ManifestSegmentReference> = current
        .as_ref()
        .map_or_else(Vec::new, |manifest: &Manifest| manifest.segments().to_vec());
    match current.as_ref() {
        Some(manifest)
            if manifest.revision() == head.revision()
                && manifest.commit_hash() == head.commit_hash() => {}
        None if head.revision() == Revision::GENESIS => {}
        _ => return Err(SharingExportError::SnapshotMismatch),
    }
    if before.safe_revision() != head.revision() {
        return Err(SharingExportError::SnapshotMismatch);
    }
    let operation_id = worlddb_core::storage_internal::generate_sharing_export_operation_id()
        .map_err(SharingExportError::Identity)?;
    let next_revision = head
        .revision()
        .next_commit()
        .map_err(|error| SharingExportError::Wal(WalError::Revision(error)))?;
    let committed = wal
        .committed_required_audit_records(&lock)
        .map_err(SharingExportError::RequiredAudit)?;
    let sequence = committed
        .last()
        .map_or(AuditSequence::new(0), |entry| entry.record().sequence())
        .next()
        .map_err(SharingExportError::AuditSequence)?;
    let record = AuditRecord::new(
        AuditRecordIdentity {
            record_id: worlddb_core::storage_internal::generate_sharing_export_audit_record_id()
                .map_err(SharingExportError::Identity)?,
            sequence,
            audit_operation_id,
        },
        AuditRecordDetails {
            actor: policy.principal_id(),
            action,
            object_class: AuditObjectClass::Export,
            outcome: AuditOutcome::Succeeded,
            commit_context: AuditCommitContext::Committed {
                revision: next_revision,
                operation_id,
            },
            security_epoch: policy.current_epoch(),
            policy_fingerprint,
        },
    );
    let receipt = wal
        .commit_audited_manifest_snapshot(&lock, operation_id, segments, &[], &record)
        .map_err(SharingExportError::SnapshotCommit)?;
    if receipt.revision() != next_revision {
        return Err(SharingExportError::SnapshotMismatch);
    }
    #[cfg(test)]
    if action == AuditAction::ExportAuthorization
        && std::env::var_os("WORLDDB_M7_12_CRASH_AFTER_AUTH_COMMIT").is_some()
    {
        std::process::exit(86);
    }
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(SharingExportError::Recovery)?;
    let after = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(SharingExportError::StorageVerify)?;
    if !after.is_clean() || after.safe_revision() != next_revision {
        return Err(SharingExportError::SnapshotMismatch);
    }
    let verified = wal
        .committed_required_audit_records(&lock)
        .map_err(SharingExportError::RequiredAudit)?;
    if !verified.iter().any(|entry| {
        entry.revision() == next_revision
            && entry.operation_id() == operation_id
            && entry.record() == &record
    }) {
        return Err(SharingExportError::AuditReceiptMissing);
    }
    Ok(())
}

fn filtered_export(
    scope: &LogicalExportScope,
    logical: &LogicalExport,
    policy: SecurityPolicyView<'_>,
) -> Result<SharingExport, SharingExportError> {
    if logical
        .manifest()
        .visible_history_spaces()
        .iter()
        .any(|space| !space.selected())
    {
        return Err(SharingExportError::ImplicitHistorySpaceDependency);
    }
    let visible: Vec<&Record> = logical
        .records()
        .iter()
        .filter(|entry| !matches!(entry.record().record(), Record::HistorySpaceDefinition(_)))
        .map(|entry| entry.record().record())
        .collect();
    let by_ref = visible
        .iter()
        .filter_map(|record| record_ref(record).map(|reference| (reference, *record)))
        .collect::<BTreeMap<_, _>>();
    if by_ref.len()
        != visible
            .iter()
            .filter(|record| record_ref(record).is_some())
            .count()
    {
        return Err(SharingExportError::DuplicateRecordIdentity);
    }
    let mut eligible = BTreeMap::<RecordRef, bool>::new();
    for record in &visible {
        if !owned_by_explicit_spaces(record, &by_ref, scope.history_spaces()) {
            continue;
        }
        if record_authorized(record, &by_ref, policy) {
            if let Some(reference) = record_ref(record) {
                eligible.insert(reference, true);
            }
        }
    }
    let allowed_refs = eligible.keys().copied().collect::<BTreeSet<_>>();
    let mut included = logical
        .records()
        .iter()
        .filter(|entry| !matches!(entry.record().record(), Record::HistorySpaceDefinition(_)))
        .filter(|entry| {
            let record = entry.record().record();
            if let Some(reference) = record_ref(record) {
                eligible.contains_key(&reference) && dependencies_visible(record, &allowed_refs)
            } else {
                owned_by_explicit_spaces(record, &by_ref, scope.history_spaces())
                    && record_authorized(record, &by_ref, policy)
            }
        })
        .map(|entry| entry.record().clone())
        .collect::<Vec<_>>();
    included.sort_by(|left, right| {
        let left_frame = encode_decoded_record(left).unwrap_or_default();
        let right_frame = encode_decoded_record(right).unwrap_or_default();
        let left_key = (
            record_revision(left.record()).unwrap_or(Revision::GENESIS),
            RecordKind::of(left.record()),
            left_frame,
        );
        let right_key = (
            record_revision(right.record()).unwrap_or(Revision::GENESIS),
            RecordKind::of(right.record()),
            right_frame,
        );
        left_key.cmp(&right_key)
    });
    if included.len() > SHARING_EXPORT_MAX_RECORDS {
        return Err(SharingExportError::ResourceLimit);
    }
    let result = SharingExport {
        from_revision: scope.from_revision(),
        through_revision: scope.through_revision(),
        history_spaces: scope.history_spaces().to_vec(),
        records: included,
    };
    validate_sharing_export(&result)?;
    Ok(result)
}

fn record_authorized(
    record: &Record,
    by_ref: &BTreeMap<RecordRef, &Record>,
    policy: SecurityPolicyView<'_>,
) -> bool {
    let principal = policy.principal_id();
    let Some(targets) = record_targets(record, by_ref) else {
        return false;
    };
    let capabilities = record_capabilities(record);
    for mut target in targets {
        for capability in [Capability::HistorySpaceRead, Capability::LayerRead] {
            if ((target.history_space().is_some() && capability == Capability::HistorySpaceRead)
                || (target.layer().is_some() && capability == Capability::LayerRead))
                && policy
                    .current_snapshot()
                    .authorize(principal, capability, target)
                    != AuthorizationDecision::Allow
            {
                return false;
            }
        }
        for capability in &capabilities {
            if policy
                .current_snapshot()
                .authorize(principal, *capability, target)
                != AuthorizationDecision::Allow
            {
                return false;
            }
        }
        if let Some(relationship) = relationship_selector(record) {
            target = PolicyTarget::new(
                target.history_space(),
                target.layer(),
                target.record(),
                None,
                Some(relationship),
            );
            let relationship_capability = if matches!(record, Record::ArchiveTransition(_)) {
                Capability::LifecycleRead
            } else {
                Capability::RelationshipRead
            };
            if policy
                .current_snapshot()
                .authorize(principal, relationship_capability, target)
                != AuthorizationDecision::Allow
            {
                return false;
            }
        }
        for field in field_selectors(record) {
            let field_target = PolicyTarget::new(
                target.history_space(),
                target.layer(),
                target.record(),
                Some(field),
                None,
            );
            if policy
                .current_snapshot()
                .authorize(principal, Capability::FieldRead, field_target)
                != AuthorizationDecision::Allow
            {
                return false;
            }
        }
    }
    true
}

fn record_capabilities(record: &Record) -> Vec<Capability> {
    let kind = RecordKind::of(record);
    required_kind_capabilities(kind)
}

fn record_targets(
    record: &Record,
    by_ref: &BTreeMap<RecordRef, &Record>,
) -> Option<Vec<PolicyTarget>> {
    let self_ref = record_ref(record);
    let make = |space, layer| PolicyTarget::new(space, layer, self_ref, None, None);
    let targets = match record {
        Record::Assertion(value) => vec![make(
            Some(value.context().history_space_id()),
            Some(value.context().layer_id()),
        )],
        Record::Mask(value) => vec![make(
            Some(value.context().history_space_id()),
            Some(value.context().layer_id()),
        )],
        Record::ReplacementBoundary(value) => vec![make(
            Some(value.context().history_space_id()),
            Some(value.context().layer_id()),
        )],
        Record::Event(value) => vec![make(Some(value.history_space_id()), Some(value.layer_id()))],
        Record::EventMask(value) => {
            vec![make(Some(value.history_space_id()), Some(value.layer_id()))]
        }
        Record::TransferLineage(value) => vec![
            make(Some(value.source_history_space_id()), None),
            make(Some(value.target_history_space_id()), None),
        ],
        Record::EventRelation(value) => {
            let from = by_ref.get(&RecordRef::Event(value.from_event()))?;
            let to = by_ref.get(&RecordRef::Event(value.to_event()))?;
            let (Record::Event(from), Record::Event(to)) = (from, to) else {
                return None;
            };
            vec![
                make(Some(from.history_space_id()), Some(from.layer_id())),
                make(Some(to.history_space_id()), Some(to.layer_id())),
            ]
        }
        Record::HistorySpaceDefinition(_) => return None,
        _ => {
            if let Some((_, target_ref)) = lifecycle_identity_and_target(record) {
                let target_record = by_ref.get(&target_ref)?;
                return record_targets(target_record, by_ref).map(|targets| {
                    targets
                        .into_iter()
                        .map(|target| {
                            PolicyTarget::new(
                                target.history_space(),
                                target.layer(),
                                self_ref,
                                None,
                                None,
                            )
                        })
                        .collect()
                });
            }
            if let Record::ArchiveTransition(transition) = record {
                let target_ref = archive_target_record_ref(transition.target());
                let target_record = by_ref.get(&target_ref)?;
                return record_targets(target_record, by_ref).map(|targets| {
                    targets
                        .into_iter()
                        .map(|target| {
                            PolicyTarget::new(
                                target.history_space(),
                                target.layer(),
                                self_ref,
                                None,
                                None,
                            )
                        })
                        .collect()
                });
            }
            vec![make(None, None)]
        }
    };
    Some(targets)
}

fn field_selectors(record: &Record) -> Vec<FieldSelector> {
    match record {
        Record::Assertion(value) => vec![
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(value.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ],
        Record::Mask(_) => vec![FieldSelector::MaskSelector, FieldSelector::MaskValidity],
        Record::ReplacementBoundary(_) => vec![
            FieldSelector::ReplacementBoundarySubject,
            FieldSelector::ReplacementBoundaryPredicate,
            FieldSelector::ReplacementBoundaryValidity,
        ],
        Record::Event(value) => {
            let kind = value.event_kind_id();
            let mut fields = vec![FieldSelector::EventKind, FieldSelector::EventTime(kind)];
            fields.extend(
                value.participants().as_slice().iter().map(|participant| {
                    FieldSelector::EventParticipant(kind, participant.role_id())
                }),
            );
            fields.extend(
                value
                    .attributes()
                    .as_slice()
                    .iter()
                    .map(|attribute| FieldSelector::EventAttribute(kind, attribute.attribute_id())),
            );
            fields
        }
        Record::EventMask(_) => vec![FieldSelector::EventMaskTarget],
        Record::Source(_) => vec![
            FieldSelector::SourceKind,
            FieldSelector::SourceLocator,
            FieldSelector::SourceContentDigest,
            FieldSelector::SourceMetadata,
        ],
        _ => Vec::new(),
    }
}

fn relationship_selector(record: &Record) -> Option<RelationshipSelector> {
    Some(match record {
        Record::EventRelation(value) => {
            let kind = match value.kind() {
                EventRelationKind::Before => worlddb_core::PolicyEventRelationKind::Before,
                EventRelationKind::SameTime => worlddb_core::PolicyEventRelationKind::SameTime,
                EventRelationKind::Causes => worlddb_core::PolicyEventRelationKind::Causes,
            };
            RelationshipSelector::EventRelation(kind)
        }
        Record::Evidence(value) => {
            let relation = match value.relation() {
                worlddb_core::EvidenceRelation::Supports => {
                    worlddb_core::EvidenceRelationship::Supports
                }
                worlddb_core::EvidenceRelation::Contradicts => {
                    worlddb_core::EvidenceRelationship::Contradicts
                }
                worlddb_core::EvidenceRelation::Documents => {
                    worlddb_core::EvidenceRelationship::Documents
                }
            };
            RelationshipSelector::Evidence(relation)
        }
        Record::Provenance(value) => {
            let relation = match value.relation() {
                ProvenanceRelation::Corrects => ProvenanceRelationship::Corrects,
                ProvenanceRelation::DerivedFrom => ProvenanceRelationship::DerivedFrom,
                ProvenanceRelation::ResultedFrom => ProvenanceRelationship::ResultedFrom,
            };
            RelationshipSelector::Provenance(relation)
        }
        Record::ArchiveTransition(_) => RelationshipSelector::LifecycleTarget,
        _ => return None,
    })
}

fn owned_by_explicit_spaces(
    record: &Record,
    by_ref: &BTreeMap<RecordRef, &Record>,
    selected: &[worlddb_core::HistorySpaceId],
) -> bool {
    let Some(targets) = record_targets(record, by_ref) else {
        return false;
    };
    targets
        .iter()
        .filter_map(|target| target.history_space())
        .all(|space| selected.binary_search(&space).is_ok())
}

fn dependencies_visible(record: &Record, visible: &BTreeSet<RecordRef>) -> bool {
    let required: Vec<RecordRef> = match record {
        Record::Mask(value) => match value.selector() {
            MaskSelector::ExactAssertion(id) => vec![RecordRef::Assertion(*id)],
            MaskSelector::Proposition(_) | MaskSelector::Slot(_) => Vec::new(),
        },
        Record::AssertionValidityClosure(value) => vec![RecordRef::Assertion(value.assertion_id())],
        Record::AssertionRetraction(value) => vec![RecordRef::Assertion(value.assertion_id())],
        Record::MaskValidityClosure(value) => vec![RecordRef::Mask(value.mask_id())],
        Record::MaskRetraction(value) => vec![RecordRef::Mask(value.mask_id())],
        Record::ReplacementBoundaryValidityClosure(value) => {
            vec![RecordRef::ReplacementBoundary(
                value.replacement_boundary_id(),
            )]
        }
        Record::ReplacementBoundaryRetraction(value) => {
            vec![RecordRef::ReplacementBoundary(
                value.replacement_boundary_id(),
            )]
        }
        Record::ArchiveTransition(value) => vec![archive_target_record_ref(value.target())],
        Record::EventMask(value) => vec![RecordRef::Event(value.target_event())],
        Record::EventSpanClosure(value) => vec![RecordRef::Event(value.event_id())],
        Record::EventRetraction(value) => vec![RecordRef::Event(value.event_id())],
        Record::EventMaskRetraction(value) => vec![RecordRef::EventMask(value.event_mask_id())],
        Record::EventRelation(value) => vec![
            RecordRef::Event(value.from_event()),
            RecordRef::Event(value.to_event()),
        ],
        Record::EventRelationRetraction(value) => {
            vec![RecordRef::EventRelation(value.event_relation_id())]
        }
        Record::Evidence(value) => vec![
            RecordRef::Source(value.source_id()),
            evidence_target_record_ref(value.target()),
        ],
        Record::EvidenceRetraction(value) => vec![RecordRef::Evidence(value.evidence_id())],
        Record::Provenance(value) => vec![
            provenance_endpoint_record_ref(value.from()),
            provenance_endpoint_record_ref(value.to()),
        ],
        Record::ProvenanceRetraction(value) => vec![RecordRef::Provenance(value.provenance_id())],
        Record::TransferLineage(value) => {
            vec![value.source().record_ref(), value.target().record_ref()]
        }
        _ => Vec::new(),
    };
    required.iter().all(|reference| visible.contains(reference))
}

fn evidence_target_record_ref(target: EvidenceTargetRef) -> RecordRef {
    match target {
        EvidenceTargetRef::Assertion(id) => RecordRef::Assertion(id),
        EvidenceTargetRef::Mask(id) => RecordRef::Mask(id),
        EvidenceTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        EvidenceTargetRef::Event(id) => RecordRef::Event(id),
        EvidenceTargetRef::EventMask(id) => RecordRef::EventMask(id),
        EvidenceTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        EvidenceTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        EvidenceTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        EvidenceTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        EvidenceTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        EvidenceTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        EvidenceTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        EvidenceTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        EvidenceTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        EvidenceTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        EvidenceTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        EvidenceTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        EvidenceTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        EvidenceTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        EvidenceTargetRef::Provenance(id) => RecordRef::Provenance(id),
        EvidenceTargetRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn provenance_endpoint_record_ref(endpoint: ProvenanceEndpointRef) -> RecordRef {
    match endpoint {
        ProvenanceEndpointRef::Assertion(id) => RecordRef::Assertion(id),
        ProvenanceEndpointRef::Mask(id) => RecordRef::Mask(id),
        ProvenanceEndpointRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ProvenanceEndpointRef::Event(id) => RecordRef::Event(id),
        ProvenanceEndpointRef::EventMask(id) => RecordRef::EventMask(id),
        ProvenanceEndpointRef::Source(id) => RecordRef::Source(id),
        ProvenanceEndpointRef::Evidence(id) => RecordRef::Evidence(id),
        ProvenanceEndpointRef::Provenance(id) => RecordRef::Provenance(id),
        ProvenanceEndpointRef::AssertionValidityClosure(id) => {
            RecordRef::AssertionValidityClosure(id)
        }
        ProvenanceEndpointRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ProvenanceEndpointRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ProvenanceEndpointRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ProvenanceEndpointRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ProvenanceEndpointRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ProvenanceEndpointRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ProvenanceEndpointRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ProvenanceEndpointRef::EventRelationRetraction(id) => {
            RecordRef::EventRelationRetraction(id)
        }
        ProvenanceEndpointRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ProvenanceEndpointRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ProvenanceEndpointRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ProvenanceEndpointRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ProvenanceEndpointRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn lifecycle_identity_and_target(record: &Record) -> Option<(RecordRef, RecordRef)> {
    Some(match record {
        Record::AssertionValidityClosure(value) => (
            RecordRef::AssertionValidityClosure(value.id()),
            RecordRef::Assertion(value.assertion_id()),
        ),
        Record::AssertionRetraction(value) => (
            RecordRef::AssertionRetraction(value.id()),
            RecordRef::Assertion(value.assertion_id()),
        ),
        Record::MaskValidityClosure(value) => (
            RecordRef::MaskValidityClosure(value.id()),
            RecordRef::Mask(value.mask_id()),
        ),
        Record::MaskRetraction(value) => (
            RecordRef::MaskRetraction(value.id()),
            RecordRef::Mask(value.mask_id()),
        ),
        Record::ReplacementBoundaryValidityClosure(value) => (
            RecordRef::ReplacementBoundaryValidityClosure(value.id()),
            RecordRef::ReplacementBoundary(value.replacement_boundary_id()),
        ),
        Record::ReplacementBoundaryRetraction(value) => (
            RecordRef::ReplacementBoundaryRetraction(value.id()),
            RecordRef::ReplacementBoundary(value.replacement_boundary_id()),
        ),
        Record::EventSpanClosure(value) => (
            RecordRef::EventSpanClosure(value.id()),
            RecordRef::Event(value.event_id()),
        ),
        Record::EventRetraction(value) => (
            RecordRef::EventRetraction(value.id()),
            RecordRef::Event(value.event_id()),
        ),
        Record::EventMaskRetraction(value) => (
            RecordRef::EventMaskRetraction(value.id()),
            RecordRef::EventMask(value.event_mask_id()),
        ),
        Record::EventRelationRetraction(value) => (
            RecordRef::EventRelationRetraction(value.id()),
            RecordRef::EventRelation(value.event_relation_id()),
        ),
        Record::EvidenceRetraction(value) => (
            RecordRef::EvidenceRetraction(value.id()),
            RecordRef::Evidence(value.evidence_id()),
        ),
        Record::ProvenanceRetraction(value) => (
            RecordRef::ProvenanceRetraction(value.id()),
            RecordRef::Provenance(value.provenance_id()),
        ),
        _ => return None,
    })
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> RecordRef {
    match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        ArchiveTargetRef::EventMask(id) => RecordRef::EventMask(id),
        ArchiveTargetRef::EventRelation(id) => RecordRef::EventRelation(id),
        ArchiveTargetRef::Source(id) => RecordRef::Source(id),
        ArchiveTargetRef::Evidence(id) => RecordRef::Evidence(id),
        ArchiveTargetRef::Provenance(id) => RecordRef::Provenance(id),
        ArchiveTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        ArchiveTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ArchiveTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ArchiveTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ArchiveTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ArchiveTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ArchiveTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ArchiveTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ArchiveTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ArchiveTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        ArchiveTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ArchiveTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ArchiveTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ArchiveTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ArchiveTargetRef::TransferLineage(id) => RecordRef::TransferLineage(id),
    }
}

fn record_ref(record: &Record) -> Option<RecordRef> {
    Some(match record {
        Record::Assertion(value) => RecordRef::Assertion(value.id()),
        Record::AssertionValidityClosure(value) => RecordRef::AssertionValidityClosure(value.id()),
        Record::AssertionRetraction(value) => RecordRef::AssertionRetraction(value.id()),
        Record::Mask(value) => RecordRef::Mask(value.id()),
        Record::MaskValidityClosure(value) => RecordRef::MaskValidityClosure(value.id()),
        Record::MaskRetraction(value) => RecordRef::MaskRetraction(value.id()),
        Record::ReplacementBoundary(value) => RecordRef::ReplacementBoundary(value.id()),
        Record::ReplacementBoundaryValidityClosure(value) => {
            RecordRef::ReplacementBoundaryValidityClosure(value.id())
        }
        Record::ReplacementBoundaryRetraction(value) => {
            RecordRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::ArchiveTransition(value) => RecordRef::ArchiveTransition(value.id()),
        Record::Event(value) => RecordRef::Event(value.id()),
        Record::EventMask(value) => RecordRef::EventMask(value.id()),
        Record::EventSpanClosure(value) => RecordRef::EventSpanClosure(value.id()),
        Record::EventRetraction(value) => RecordRef::EventRetraction(value.id()),
        Record::EventMaskRetraction(value) => RecordRef::EventMaskRetraction(value.id()),
        Record::EventRelation(value) => RecordRef::EventRelation(value.id()),
        Record::EventRelationRetraction(value) => RecordRef::EventRelationRetraction(value.id()),
        Record::Source(value) => RecordRef::Source(value.id()),
        Record::Evidence(value) => RecordRef::Evidence(value.id()),
        Record::EvidenceRetraction(value) => RecordRef::EvidenceRetraction(value.id()),
        Record::Provenance(value) => RecordRef::Provenance(value.id()),
        Record::ProvenanceRetraction(value) => RecordRef::ProvenanceRetraction(value.id()),
        Record::EntityRetirement(value) => {
            RecordRef::EntityRetirement(value.entity_retirement_id())
        }
        Record::PerspectiveRetirement(value) => {
            RecordRef::PerspectiveRetirement(value.perspective_retirement_id())
        }
        Record::TransferLineage(value) => RecordRef::TransferLineage(value.id()),
        _ => return None,
    })
}

fn record_revision(record: &Record) -> Option<Revision> {
    Some(match record {
        Record::HistorySpaceDefinition(_) => return None,
        Record::Entity(value) => value.created_revision(),
        Record::EntityRetirement(value) => value.created_revision(),
        Record::PerspectiveDefinitionRevision(value) => value.recorded_revision(),
        Record::PerspectiveRetirement(value) => value.created_revision(),
        Record::LayerDefinition(value) => value.created_revision().revision(),
        Record::LayerSchemaSnapshot(value) => value.revision().revision(),
        Record::EntityTypeDefinition(value) => value.created_revision(),
        Record::PredicateDefinition(value) => value.created_revision(),
        Record::EventKindDefinition(value) => value.created_revision(),
        Record::MigrationPlan(_)
        | Record::MigrationRun(_)
        | Record::MigrationStepCommitIdentity(_) => return None,
        Record::Assertion(value) => value.created_revision(),
        Record::AssertionValidityClosure(value) => value.created_revision(),
        Record::AssertionRetraction(value) => value.created_revision(),
        Record::Mask(value) => value.created_revision(),
        Record::MaskValidityClosure(value) => value.created_revision(),
        Record::MaskRetraction(value) => value.created_revision(),
        Record::ReplacementBoundary(value) => value.created_revision(),
        Record::ReplacementBoundaryValidityClosure(value) => value.created_revision(),
        Record::ReplacementBoundaryRetraction(value) => value.created_revision(),
        Record::ArchiveTransition(value) => value.created_revision(),
        Record::Event(value) => value.created_revision(),
        Record::EventMask(value) => value.created_revision(),
        Record::EventSpanClosure(value) => value.created_revision(),
        Record::EventRetraction(value) => value.created_revision(),
        Record::EventMaskRetraction(value) => value.created_revision(),
        Record::EventRelation(value) => value.created_revision(),
        Record::EventRelationRetraction(value) => value.created_revision(),
        Record::Source(value) => value.created_revision(),
        Record::Evidence(value) => value.created_revision(),
        Record::EvidenceRetraction(value) => value.created_revision(),
        Record::Provenance(value) => value.created_revision(),
        Record::ProvenanceRetraction(value) => value.created_revision(),
        Record::TransferLineage(value) => value.created_revision(),
    })
}

fn validate_sharing_export(export: &SharingExport) -> Result<(), SharingExportError> {
    if export.from_revision > export.through_revision
        || export.history_spaces.is_empty()
        || export.history_spaces.len() > SHARING_EXPORT_MAX_SPACES
        || export
            .history_spaces
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left >= right))
        || export.records.len() > SHARING_EXPORT_MAX_RECORDS
    {
        return Err(SharingExportError::InvalidEncoding);
    }
    let mut previous: Option<(Revision, RecordKind, Vec<u8>)> = None;
    let mut seen_refs = BTreeSet::new();
    let mut total = 0usize;
    for decoded in &export.records {
        if matches!(decoded.record(), Record::HistorySpaceDefinition(_))
            || record_revision(decoded.record()).is_none()
        {
            return Err(SharingExportError::InvalidEncoding);
        }
        let frame = encode_decoded_record(decoded).map_err(SharingExportError::Record)?;
        total = total
            .checked_add(frame.len())
            .ok_or(SharingExportError::ResourceLimit)?;
        if total > SHARING_EXPORT_MAX_BYTES {
            return Err(SharingExportError::ResourceLimit);
        }
        let key = (
            record_revision(decoded.record()).ok_or(SharingExportError::InvalidEncoding)?,
            RecordKind::of(decoded.record()),
            frame,
        );
        if previous.as_ref().is_some_and(|previous| previous >= &key) {
            return Err(SharingExportError::NonCanonicalEncoding);
        }
        previous = Some(key);
        if let Some(reference) = record_ref(decoded.record()) {
            if !seen_refs.insert(reference) {
                return Err(SharingExportError::DuplicateRecordIdentity);
            }
        }
    }
    Ok(())
}

/// Why a sharing export could not be authorized, audited, or encoded.
#[derive(Debug)]
pub enum SharingExportError {
    /// The security policy denied at least one requested class or record.
    AuthorizationDenied,
    /// Selected records depend on an ancestor HistorySpace that was not explicit in the scope.
    ImplicitHistorySpaceDependency,
    /// The visible records contain a repeated typed identity.
    DuplicateRecordIdentity,
    /// A successful audit boundary could not be found after recovery.
    AuditReceiptMissing,
    /// The source database was not a clean verified snapshot.
    SourceNotClean,
    /// The source WAL and manifest do not identify the same committed snapshot.
    SnapshotMismatch,
    /// The encoded artifact is malformed or has an unsupported record kind.
    InvalidEncoding,
    /// The artifact digest does not match its bytes.
    DigestMismatch,
    /// The artifact is valid but is not in canonical order/encoding.
    NonCanonicalEncoding,
    /// The artifact exceeds a configured resource bound.
    ResourceLimit,
    /// A bounded allocation could not be completed.
    AllocationFailed,
    /// The requested or decoded scope is invalid.
    LogicalExport(LogicalExportError),
    /// Database layout open failed.
    Layout(StorageFileError),
    /// Writer lock acquisition failed.
    WriterLock(WriterLockError),
    /// The source verifier failed.
    StorageVerify(StorageVerifyError),
    /// Recovery or replay failed.
    Recovery(crate::RecoveryError),
    /// WAL verification or operation commit failed.
    Wal(WalError),
    /// Manifest read or validation failed.
    Manifest(crate::ManifestError),
    /// Required audit validation or sequencing failed.
    RequiredAudit(RequiredAuditError),
    /// A manifest snapshot and its audit record could not be committed atomically.
    SnapshotCommit(SnapshotCommitError),
    /// The next audit sequence could not be allocated.
    AuditSequence(worlddb_core::AuditSequenceError),
    /// Persistent identity generation failed.
    Identity(worlddb_core::IdGenerationError),
    /// The safe audit fingerprint could not be built.
    AuditFingerprint(worlddb_core::AuditFingerprintError),
    /// A typed record frame could not be encoded or decoded.
    Record(worlddb_core::RecordCodecError),
}

impl fmt::Display for SharingExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorizationDenied => {
                formatter.write_str("sharing export authorization was denied")
            }
            Self::ImplicitHistorySpaceDependency => {
                formatter.write_str("sharing export depends on an unselected ancestor HistorySpace")
            }
            Self::DuplicateRecordIdentity => {
                formatter.write_str("sharing export has a duplicate record identity")
            }
            Self::AuditReceiptMissing => {
                formatter.write_str("sharing export audit commit was not verified after recovery")
            }
            Self::SourceNotClean => {
                formatter.write_str("sharing export requires a clean source snapshot")
            }
            Self::SnapshotMismatch => {
                formatter.write_str("sharing export source snapshot changed or is inconsistent")
            }
            Self::InvalidEncoding => formatter.write_str("sharing export artifact is malformed"),
            Self::DigestMismatch => {
                formatter.write_str("sharing export digest does not match its content")
            }
            Self::NonCanonicalEncoding => {
                formatter.write_str("sharing export artifact is not canonical")
            }
            Self::ResourceLimit => {
                formatter.write_str("sharing export exceeds a configured resource limit")
            }
            Self::AllocationFailed => formatter.write_str("sharing export allocation failed"),
            Self::LogicalExport(error) => write!(formatter, "logical source snapshot: {error}"),
            Self::Layout(error) => write!(formatter, "database layout: {error}"),
            Self::WriterLock(error) => write!(formatter, "database writer lock: {error}"),
            Self::StorageVerify(error) => write!(formatter, "storage verification: {error}"),
            Self::Recovery(error) => write!(formatter, "storage recovery: {error}"),
            Self::Wal(error) => write!(formatter, "audit WAL: {error}"),
            Self::Manifest(error) => write!(formatter, "manifest: {error}"),
            Self::RequiredAudit(error) => write!(formatter, "required audit: {error}"),
            Self::SnapshotCommit(error) => write!(formatter, "audited manifest commit: {error}"),
            Self::AuditSequence(error) => write!(formatter, "audit sequence: {error}"),
            Self::Identity(error) => write!(formatter, "identity generation: {error}"),
            Self::AuditFingerprint(error) => write!(formatter, "audit fingerprint: {error}"),
            Self::Record(error) => write!(formatter, "record frame: {error}"),
        }
    }
}

impl std::error::Error for SharingExportError {}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SharingExportError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(SharingExportError::InvalidEncoding)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(SharingExportError::InvalidEncoding)?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, SharingExportError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| SharingExportError::InvalidEncoding)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, SharingExportError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| SharingExportError::InvalidEncoding)?,
        ))
    }

    fn id(&mut self) -> Result<worlddb_core::HistorySpaceId, SharingExportError> {
        worlddb_core::HistorySpaceId::try_from_bytes(
            self.take(16)?
                .try_into()
                .map_err(|_| SharingExportError::InvalidEncoding)?,
        )
        .map_err(|_| SharingExportError::InvalidEncoding)
    }

    fn count(&mut self, maximum: usize) -> Result<usize, SharingExportError> {
        let value = usize::try_from(self.u32()?).map_err(|_| SharingExportError::ResourceLimit)?;
        if value > maximum {
            return Err(SharingExportError::ResourceLimit);
        }
        Ok(value)
    }

    fn bytes_u32(&mut self, maximum: usize) -> Result<&'a [u8], SharingExportError> {
        let length = self.count(maximum)?;
        self.take(length)
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, SharingExportError> {
    let end = offset
        .checked_add(8)
        .ok_or(SharingExportError::InvalidEncoding)?;
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(SharingExportError::InvalidEncoding)?
            .try_into()
            .map_err(|_| SharingExportError::InvalidEncoding)?,
    ))
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_bytes_u32(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), SharingExportError> {
    put_u32(
        output,
        u32::try_from(bytes.len()).map_err(|_| SharingExportError::ResourceLimit)?,
    );
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        SHARING_EXPORT_MAGIC, SharingExportError, SharingExportManager, SharingExportScope,
    };
    use crate::{
        DatabaseLayout, HistorySegmentStore, LogicalExportError, ManifestSegmentKind,
        ManifestSegmentReference, ManifestSnapshot, RecoveryManager, SecurityPolicyHistoryStore,
        WalPrepareLog,
    };
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use worlddb_core::{
        AuditAction, AuthorizationMode, Capability, CapabilityGrant, CapabilityRule, DomainId,
        FieldSelector, GrantEffect, HistorySpaceDefinition, HistorySpaceId, PolicyRuleId,
        PolicyScope, PolicySubject, Principal, PrincipalId, Record, RecordKind, RecordRef,
        Revision, SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot,
        SecurityPolicyVersion, Source, SourceLocator, SourceMetadata, Symbol,
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestArea(PathBuf);

    impl TestArea {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("worlddb-m7-12-{}-{sequence}", std::process::id()));
            fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn layout(&self) -> Result<DatabaseLayout, String> {
            DatabaseLayout::create(self.0.join("database")).map_err(|error| error.to_string())
        }
    }

    impl Drop for TestArea {
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

    fn fixture() -> Result<
        (
            TestArea,
            DatabaseLayout,
            HistorySpaceId,
            worlddb_core::SourceId,
        ),
        String,
    > {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let space = id::<HistorySpaceId>(1)?;
        let source_id = id::<worlddb_core::SourceId>(2)?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let records = [
            Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(space, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            ),
            Record::Source(Source::new(
                source_id,
                Symbol::new("fixture").map_err(|error| error.to_string())?,
                Some(
                    SourceLocator::new("hidden-locator-value")
                        .map_err(|error| error.to_string())?,
                ),
                None,
                SourceMetadata::default(),
                Revision::FIRST_COMMIT,
            )),
        ];
        let history = HistorySegmentStore::new(layout.clone())
            .write_segment(&lock, &records)
            .map_err(|error| error.to_string())?;
        let history_ref = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history.id(),
            history.content_digest(),
            Revision::FIRST_COMMIT,
        );
        let policy_version = SecurityPolicyVersion::new(
            Revision::GENESIS,
            SecurityEpoch::INITIAL,
            SecurityPolicySnapshot::default(),
        );
        let policy = SecurityPolicyHistoryStore::new(layout.clone())
            .write_version(&lock, &policy_version, None, None)
            .map_err(|error| error.to_string())?;
        let policy_ref = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            policy.id(),
            policy.content_digest(),
            Revision::GENESIS,
        );
        let references = vec![history_ref, policy_ref];
        ManifestSnapshot::new(Revision::FIRST_COMMIT, references.clone())
            .map_err(|error| error.to_string())?;
        WalPrepareLog::new(&layout)
            .commit_manifest_snapshot(&lock, id::<worlddb_core::OperationId>(3)?, references, &[])
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        drop(lock);
        Ok((area, layout, space, source_id))
    }

    fn policy(
        space: HistorySpaceId,
        source_id: worlddb_core::SourceId,
        denied_field: Option<FieldSelector>,
    ) -> Result<SecurityPolicyHistory, String> {
        let principal = id::<PrincipalId>(4)?;
        let mut rules = Vec::new();
        for (tail, capability) in [
            Capability::DataExport,
            Capability::ProjectRead,
            Capability::SourceRead,
            Capability::FieldRead,
        ]
        .into_iter()
        .enumerate()
        {
            rules.push(CapabilityRule::new(
                id::<PolicyRuleId>(10 + u8::try_from(tail).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        rules.push(CapabilityRule::new(
            id::<PolicyRuleId>(15)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
            PolicyScope::new(Some(space), None, None, None, None),
        ));
        if let Some(field) = denied_field {
            rules.push(CapabilityRule::new(
                id::<PolicyRuleId>(20)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
                PolicyScope::new(
                    None,
                    None,
                    Some(RecordRef::Source(source_id)),
                    Some(field),
                    None,
                ),
            ));
        }
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

    fn per_field_policy(
        space: HistorySpaceId,
        source_id: worlddb_core::SourceId,
    ) -> Result<SecurityPolicyHistory, String> {
        let principal = id::<PrincipalId>(4)?;
        let mut rules = Vec::new();
        for (tail, capability) in [
            Capability::DataExport,
            Capability::ProjectRead,
            Capability::SourceRead,
        ]
        .into_iter()
        .enumerate()
        {
            rules.push(CapabilityRule::new(
                id::<PolicyRuleId>(30 + u8::try_from(tail).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ));
        }
        rules.push(CapabilityRule::new(
            id::<PolicyRuleId>(34)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
            PolicyScope::new(Some(space), None, None, None, None),
        ));
        for (offset, field) in [
            FieldSelector::SourceKind,
            FieldSelector::SourceLocator,
            FieldSelector::SourceContentDigest,
            FieldSelector::SourceMetadata,
        ]
        .into_iter()
        .enumerate()
        {
            rules.push(CapabilityRule::new(
                id::<PolicyRuleId>(35 + u8::try_from(offset).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::FieldRead, GrantEffect::Allow),
                PolicyScope::new(
                    None,
                    None,
                    Some(RecordRef::Source(source_id)),
                    Some(field),
                    None,
                ),
            ));
        }
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

    fn view(
        history: &SecurityPolicyHistory,
    ) -> Result<worlddb_core::SecurityPolicyView<'_>, String> {
        history
            .select(
                AuthorizationMode::Now,
                id::<PrincipalId>(4)?,
                Revision::GENESIS,
            )
            .map_err(|error| error.to_string())
    }

    fn scope(space: HistorySpaceId, through: Revision) -> Result<SharingExportScope, String> {
        SharingExportScope::new(
            Revision::GENESIS,
            through,
            vec![space],
            vec![RecordKind::HistorySpaceDefinition, RecordKind::Source],
        )
        .map_err(|error| error.to_string())
    }

    fn export_audits(layout: &DatabaseLayout) -> Result<Vec<worlddb_core::AuditRecord>, String> {
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        Ok(WalPrepareLog::new(layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| entry.record().clone())
            .filter(|record| {
                matches!(
                    record.action(),
                    AuditAction::ExportAuthorization | AuditAction::ExportCompletion
                )
            })
            .collect())
    }

    #[test]
    fn sharing_export_roundtrips_and_commits_both_audit_boundaries() -> Result<(), String> {
        let (_area, layout, space, source_id) = fixture()?;
        let history = policy(space, source_id, None)?;
        let export = SharingExportManager::new(layout.clone())
            .export(scope(space, Revision::FIRST_COMMIT)?, view(&history)?)
            .map_err(|error| error.to_string())?;
        assert_eq!(export.records().len(), 1);
        let only_record = export
            .records()
            .first()
            .ok_or_else(|| String::from("sharing export record is missing"))?;
        assert!(matches!(only_record.record(), Record::Source(_)));
        let bytes = export.encode().map_err(|error| error.to_string())?;
        assert_eq!(bytes.get(..8), Some(SHARING_EXPORT_MAGIC.as_slice()));
        assert!(
            bytes
                .windows(b"hidden-locator-value".len())
                .any(|window| window == b"hidden-locator-value")
        );
        let database_id = layout
            .database_id()
            .ok_or_else(|| String::from("fixture database id missing"))?;
        assert!(
            !bytes
                .windows(16)
                .any(|window| window == database_id.to_bytes())
        );
        let decoded = super::SharingExport::decode(&bytes).map_err(|error| error.to_string())?;
        assert_eq!(decoded.records().len(), 1);
        assert_eq!(decoded.encode().map_err(|error| error.to_string())?, bytes);
        let audits = export_audits(&layout)?;
        assert_eq!(audits.len(), 2);
        let [authorization, completion] = audits.as_slice() else {
            return Err(String::from(
                "expected authorization and completion audit records",
            ));
        };
        assert_eq!(authorization.action(), AuditAction::ExportAuthorization);
        assert_eq!(completion.action(), AuditAction::ExportCompletion);
        assert_eq!(
            authorization.outcome(),
            worlddb_core::AuditOutcome::Succeeded
        );
        assert_eq!(completion.outcome(), worlddb_core::AuditOutcome::Succeeded);
        assert_eq!(
            authorization.audit_operation_id(),
            completion.audit_operation_id()
        );
        Ok(())
    }

    #[test]
    fn denied_source_field_is_excluded_without_an_omission_count() -> Result<(), String> {
        let (_area, layout, space, source_id) = fixture()?;
        let history = policy(space, source_id, Some(FieldSelector::SourceLocator))?;
        let export = SharingExportManager::new(layout.clone())
            .export(scope(space, Revision::FIRST_COMMIT)?, view(&history)?)
            .map_err(|error| error.to_string())?;
        assert!(export.records().is_empty());
        let bytes = export.encode().map_err(|error| error.to_string())?;
        assert!(
            !bytes
                .windows(b"hidden-locator-value".len())
                .any(|window| window == b"hidden-locator-value")
        );
        assert_eq!(export_audits(&layout)?.len(), 2);
        Ok(())
    }

    #[test]
    fn exact_field_grants_work_without_project_wide_field_read() -> Result<(), String> {
        let (_area, layout, space, source_id) = fixture()?;
        let history = per_field_policy(space, source_id)?;
        let export = SharingExportManager::new(layout)
            .export(scope(space, Revision::FIRST_COMMIT)?, view(&history)?)
            .map_err(|error| error.to_string())?;
        assert_eq!(export.records().len(), 1);
        assert!(matches!(
            export
                .records()
                .first()
                .ok_or_else(|| String::from("authorized source is missing"))?
                .record(),
            Record::Source(_)
        ));
        Ok(())
    }

    #[test]
    fn failure_after_authorization_never_commits_completion_or_returns_artifact()
    -> Result<(), String> {
        let (_area, layout, space, source_id) = fixture()?;
        let history = policy(space, source_id, None)?;
        let result = SharingExportManager::new(layout.clone()).export(
            scope(space, Revision::new(99).map_err(|error| error.to_string())?)?,
            view(&history)?,
        );
        assert!(matches!(
            result,
            Err(SharingExportError::LogicalExport(
                LogicalExportError::SnapshotMismatch
            ))
        ));
        let audits = export_audits(&layout)?;
        assert_eq!(audits.len(), 1);
        assert_eq!(
            audits
                .first()
                .ok_or_else(|| String::from("authorization audit record is missing"))?
                .action(),
            AuditAction::ExportAuthorization
        );
        Ok(())
    }

    #[test]
    fn process_crash_after_authorization_commit_recovers_only_that_audit_boundary()
    -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M7_12_CRASH_ROOT";
        const TEST_NAME: &str = "sharing_export::tests::process_crash_after_authorization_commit_recovers_only_that_audit_boundary";
        if let Ok(root) = env::var(ROOT_ENV) {
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let space = id::<HistorySpaceId>(1)?;
            let source_id = id::<worlddb_core::SourceId>(2)?;
            let history = policy(space, source_id, None)?;
            let _ = SharingExportManager::new(layout)
                .export(scope(space, Revision::FIRST_COMMIT)?, view(&history)?);
            return Err(String::from(
                "child returned instead of crashing after the authorization commit",
            ));
        }

        let (_area, layout, _, _) = fixture()?;
        let status = Command::new(env::current_exe().map_err(|error| error.to_string())?)
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(ROOT_ENV, layout.root())
            .env("WORLDDB_M7_12_CRASH_AFTER_AUTH_COMMIT", "1")
            .status()
            .map_err(|error| error.to_string())?;
        if status.code() != Some(86) {
            return Err(format!(
                "sharing-export child exited with {:?}, expected process exit code 86",
                status.code()
            ));
        }

        let reopened = DatabaseLayout::open(layout.root()).map_err(|error| error.to_string())?;
        let lock = reopened
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let recovered = RecoveryManager::new(reopened.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        assert!(recovered.report().is_clean());
        assert_eq!(
            recovered.report().safe_revision(),
            Revision::FIRST_COMMIT
                .next_commit()
                .map_err(|error| error.to_string())?
        );
        let records = WalPrepareLog::new(&reopened)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(records.len(), 1);
        let record = records
            .first()
            .ok_or_else(|| String::from("recovered authorization record is missing"))?;
        assert_eq!(record.record().action(), AuditAction::ExportAuthorization);
        assert_eq!(
            record.record().outcome(),
            worlddb_core::AuditOutcome::Succeeded
        );
        Ok(())
    }
}
