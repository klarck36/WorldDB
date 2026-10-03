//! Authorized, WAL-backed publication and historical reads for project schemas.

use std::collections::BTreeMap;
use std::fmt;

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence, AuthorizationDecision,
    Bytes, Capability, EntityTypeConstraint, Lifecycle, OperationId, PolicyTarget,
    PredicateDefinition, PredicateDefinitionSpec, PrincipalId, Record, Revision, SchemaDefinition,
    SchemaHistoryReferenceModel, SchemaMode, SchemaSnapshot, SecurityPolicyVersion, encode_record,
};

use crate::{
    DatabaseLayout, HistorySegmentStore, Manifest, ManifestSegmentKind, ManifestSegmentReference,
    ManifestStore, RecoveryDisposition, RecoveryManager, SecurityPolicyHistorySnapshot,
    SecurityPolicyHistoryStore, StorageVerifier, WalOperationStatus, WalPrepareLog, WriterLock,
};

mod entity;
pub use entity::{EntityCatalogPublicationReceipt, EntityManagementError, FileEntityManager};

const MAX_SCHEMA_BATCH_RECORDS: usize = 1_024;

/// Authorized file-backed schema history and transaction manager.
///
/// The caller owns the exclusive database writer lock for the lifetime of this
/// manager. Every publication rechecks the WAL head and current policy before
/// staging records, then commits history, a policy snapshot, and Required Audit
/// in one replayable WAL operation.
pub struct FileSchemaManager<'a> {
    layout: DatabaseLayout,
    writer_lock: &'a WriterLock,
    principal: PrincipalId,
    manifest: Manifest,
    schema_history: SchemaHistoryReferenceModel,
    policy_history: SecurityPolicyHistorySnapshot,
}

/// Receipt for one atomic schema publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    fingerprint: [u8; 32],
    definition_count: usize,
}

impl SchemaPublicationReceipt {
    /// Stable idempotency identity supplied for this commit.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Shared database revision assigned to the complete publication.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Fingerprint of the effective schema at the publication revision.
    #[must_use]
    pub const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Number of typed schema definitions in the publication batch.
    #[must_use]
    pub const fn definition_count(&self) -> usize {
        self.definition_count
    }
}

impl<'a> FileSchemaManager<'a> {
    /// Recovers, verifies, loads, and authorizes one project schema view.
    pub fn open(
        layout: DatabaseLayout,
        writer_lock: &'a WriterLock,
        principal: PrincipalId,
    ) -> Result<Self, SchemaManagementError> {
        let recovery = RecoveryManager::new(layout.clone())
            .recover(writer_lock)
            .map_err(storage_error)?;
        if !recovery.report().is_clean() {
            return Err(SchemaManagementError::Storage(
                "database requires recovery".to_owned(),
            ));
        }
        let verified = StorageVerifier::new(layout.clone())
            .verify(writer_lock)
            .map_err(storage_error)?;
        if verified.disposition() != RecoveryDisposition::Clean {
            return Err(SchemaManagementError::Storage(
                "database verification is not clean".to_owned(),
            ));
        }

        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(storage_error)?
            .ok_or_else(|| {
                SchemaManagementError::Storage("database manifest is missing".to_owned())
            })?;
        let wal = WalPrepareLog::new(&layout);
        let wal_head = wal
            .commit_head(writer_lock)
            .map_err(storage_error)?
            .revision();
        if manifest.revision() != wal_head {
            return Err(SchemaManagementError::Storage(
                "database manifest and WAL head disagree".to_owned(),
            ));
        }

        let policy_history = load_policy_history(&layout, &manifest)?;
        let current_policy = policy_history
            .policy()
            .latest_version()
            .map_err(|error| SchemaManagementError::Storage(error.to_string()))?;
        authorize(current_policy.snapshot(), principal, Capability::SchemaRead)?;
        let schema_history = load_schema_history(&layout, &manifest)?;

        Ok(Self {
            layout,
            writer_lock,
            principal,
            manifest,
            schema_history,
            policy_history,
        })
    }

    /// Current committed shared revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.manifest.revision()
    }

    /// Selects Historical, Current, or Explicit schema without fallback.
    pub fn schema_at(
        &self,
        mode: SchemaMode,
        recorded_as_of: Revision,
    ) -> Result<SchemaSnapshot, SchemaManagementError> {
        self.schema_history
            .schema_at(mode, recorded_as_of)
            .map_err(SchemaManagementError::History)
    }

    /// Next available shared commit revision.
    pub fn next_revision(&self) -> Result<Revision, SchemaManagementError> {
        self.revision()
            .next_commit()
            .map_err(|error| SchemaManagementError::Storage(error.to_string()))
    }

    /// Publishes a typed schema batch as one authorized, audited WAL transaction.
    ///
    /// `created_revision` values supplied in schema records are rewritten to
    /// the next commit revision. A repeated definition identity is accepted
    /// only for a legal lifecycle transition; structural changes to an
    /// existing identity must go through the migration contract.
    pub fn publish(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        records: Vec<Record>,
    ) -> Result<SchemaPublicationReceipt, SchemaManagementError> {
        if records.is_empty() || records.len() > MAX_SCHEMA_BATCH_RECORDS {
            return Err(SchemaManagementError::InvalidCandidate(
                "a schema batch must contain between 1 and 1024 definitions",
            ));
        }
        if expected_base != self.revision() {
            return Err(SchemaManagementError::Conflict);
        }

        let wal = WalPrepareLog::new(&self.layout);
        let live_head = wal
            .commit_head(self.writer_lock)
            .map_err(storage_error)?
            .revision();
        if live_head != expected_base {
            return Err(SchemaManagementError::Conflict);
        }
        if wal
            .operation_status(self.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(SchemaManagementError::OperationAlreadyUsed);
        }

        let policy_version = self
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| SchemaManagementError::Storage(error.to_string()))?;
        authorize(
            policy_version.snapshot(),
            self.principal,
            Capability::SchemaManage,
        )?;

        let target_revision = self.next_revision()?;
        let mut candidate_records = Vec::new();
        candidate_records
            .try_reserve_exact(records.len())
            .map_err(|_| SchemaManagementError::Storage("allocation failed".to_owned()))?;
        let mut candidate_definitions = Vec::new();
        candidate_definitions
            .try_reserve_exact(records.len())
            .map_err(|_| SchemaManagementError::Storage("allocation failed".to_owned()))?;
        for record in records {
            let definition = normalize_definition_revision(record, target_revision)?;
            validate_symbol_grammar(&definition)?;
            candidate_records.push(definition_record(&definition)?);
            candidate_definitions.push(definition);
        }

        let base_snapshot = self
            .schema_history
            .schema_at(SchemaMode::Current, self.revision())
            .map_err(SchemaManagementError::History)?;
        validate_identity_changes(&base_snapshot, &candidate_definitions, target_revision)?;

        let mut candidate_history = load_schema_history(&self.layout, &self.manifest)?;
        candidate_history
            .publish(target_revision, candidate_definitions.clone())
            .map_err(SchemaManagementError::History)?;
        let published_snapshot = candidate_history
            .schema_at(SchemaMode::Current, target_revision)
            .map_err(SchemaManagementError::History)?;
        validate_references(&published_snapshot)?;

        let sequence = next_audit_sequence(&wal, self.writer_lock)?;
        let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(
            policy_version
                .snapshot()
                .effective_capability_fingerprint(self.principal, PolicyTarget::default())
                .to_vec(),
        ))
        .map_err(|error| SchemaManagementError::Storage(error.to_string()))?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id:
                    worlddb_core::storage_internal::generate_schema_management_audit_record_id()
                        .map_err(|error| SchemaManagementError::Storage(error.to_string()))?,
                sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_schema_management_audit_operation_id()
                        .map_err(|error| SchemaManagementError::Storage(error.to_string()))?,
            },
            AuditRecordDetails {
                actor: self.principal,
                action: AuditAction::SchemaManagement,
                object_class: AuditObjectClass::SchemaDefinition,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: target_revision,
                    operation_id,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint,
            },
        );

        let history_store = HistorySegmentStore::new(self.layout.clone());
        let history_receipt = history_store
            .stage_segment(self.writer_lock, &candidate_records)
            .map_err(storage_error)?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            target_revision,
        );

        let security_store = SecurityPolicyHistoryStore::new(self.layout.clone());
        let retention = self
            .policy_history
            .audit_retention_at(self.revision())
            .map_err(|error| SchemaManagementError::Storage(error.to_string()))?;
        let next_policy_version = SecurityPolicyVersion::new(
            target_revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let security_receipt = match security_store.stage_version(
            self.writer_lock,
            &next_policy_version,
            None,
            retention,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let cleanup = history_store.remove_staged_reference(history_reference);
                return Err(SchemaManagementError::Storage(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; staged schema history cleanup failed: {cleanup_error}")
                    }
                }));
            }
        };
        let security_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            security_receipt.id(),
            security_receipt.content_digest(),
            target_revision,
        );

        let mut next_references = self.manifest.segments().to_vec();
        next_references.push(history_reference);
        next_references.push(security_reference);
        let staged = [history_reference, security_reference];
        let commit_result = wal.commit_audited_manifest_snapshot(
            self.writer_lock,
            operation_id,
            next_references,
            &staged,
            &audit_record,
        );
        match commit_result {
            Ok(receipt) if receipt.revision() == target_revision => {}
            Ok(_) => {
                return Err(SchemaManagementError::UnknownCommit(operation_id));
            }
            Err(commit_error) => {
                let recovered = RecoveryManager::new(self.layout.clone())
                    .recover(self.writer_lock)
                    .map_err(|recovery_error| {
                        SchemaManagementError::UnknownCommitWithDiagnostic(
                            operation_id,
                            format!("{commit_error}; recovery: {recovery_error}"),
                        )
                    })?;
                let status = wal
                    .operation_status(self.writer_lock, operation_id)
                    .map_err(storage_error)?;
                if !recovered.report().is_clean()
                    || !matches!(status, WalOperationStatus::Committed(receipt) if receipt.revision() == target_revision)
                {
                    if status == WalOperationStatus::NotCommitted {
                        cleanup_staged_schema(
                            &history_store,
                            history_reference,
                            &security_store,
                            security_reference,
                        );
                        return Err(SchemaManagementError::Storage(commit_error.to_string()));
                    }
                    return Err(SchemaManagementError::UnknownCommitWithDiagnostic(
                        operation_id,
                        commit_error.to_string(),
                    ));
                }
            }
        }

        let recovered = RecoveryManager::new(self.layout.clone())
            .recover(self.writer_lock)
            .map_err(|error| {
                SchemaManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
            })?;
        if !recovered.report().is_clean() {
            return Err(SchemaManagementError::UnknownCommit(operation_id));
        }

        let next_manager = Self::open(self.layout.clone(), self.writer_lock, self.principal)?;
        if next_manager.revision() != target_revision {
            return Err(SchemaManagementError::UnknownCommit(operation_id));
        }
        let receipt = SchemaPublicationReceipt {
            operation_id,
            revision: target_revision,
            fingerprint: published_snapshot.fingerprint(),
            definition_count: candidate_records.len(),
        };
        *self = next_manager;
        Ok(receipt)
    }
}

fn load_policy_history(
    layout: &DatabaseLayout,
    manifest: &Manifest,
) -> Result<SecurityPolicyHistorySnapshot, SchemaManagementError> {
    let ids = manifest
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|reference| reference.id())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Err(SchemaManagementError::Storage(
            "security-policy history is missing".to_owned(),
        ));
    }
    SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(manifest.revision(), &ids)
        .map_err(storage_error)
}

fn load_schema_history(
    layout: &DatabaseLayout,
    manifest: &Manifest,
) -> Result<SchemaHistoryReferenceModel, SchemaManagementError> {
    let store = HistorySegmentStore::new(layout.clone());
    let mut batches = BTreeMap::<Revision, Vec<SchemaDefinition>>::new();
    for reference in manifest
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::History)
    {
        let segment = store.read_segment(reference.id()).map_err(storage_error)?;
        if segment.content_digest() != reference.content_digest() {
            return Err(SchemaManagementError::Storage(
                "history segment digest does not match the manifest".to_owned(),
            ));
        }
        for decoded in segment.records() {
            if let Some(definition) = schema_definition(decoded.record()) {
                let revision = definition_revision(&definition);
                if revision > reference.through_revision() || revision > manifest.revision() {
                    return Err(SchemaManagementError::Storage(
                        "schema definition lies beyond its committed segment".to_owned(),
                    ));
                }
                batches.entry(revision).or_default().push(definition);
            }
        }
    }

    let genesis = batches.remove(&Revision::GENESIS).unwrap_or_default();
    let mut history = SchemaHistoryReferenceModel::with_genesis(genesis)
        .map_err(SchemaManagementError::History)?;
    for (revision, definitions) in batches {
        history
            .publish(revision, definitions)
            .map_err(SchemaManagementError::History)?;
    }
    if history.latest_published() < manifest.revision() {
        history
            .advance_to(manifest.revision())
            .map_err(SchemaManagementError::History)?;
    }
    Ok(history)
}

fn schema_definition(record: &Record) -> Option<SchemaDefinition> {
    match record {
        Record::LayerDefinition(value) => Some(SchemaDefinition::Layer(value.clone())),
        Record::LayerSchemaSnapshot(value) => Some(SchemaDefinition::LayerSnapshot(value.clone())),
        Record::EntityTypeDefinition(value) => Some(SchemaDefinition::EntityType(value.clone())),
        Record::PredicateDefinition(value) => Some(SchemaDefinition::Predicate(value.clone())),
        Record::EventKindDefinition(value) => Some(SchemaDefinition::EventKind(value.clone())),
        _ => None,
    }
}

fn definition_revision(definition: &SchemaDefinition) -> Revision {
    match definition {
        SchemaDefinition::Layer(value) => value.created_revision().revision(),
        SchemaDefinition::LayerSnapshot(value) => value.revision().revision(),
        SchemaDefinition::EntityType(value) => value.created_revision(),
        SchemaDefinition::Predicate(value) => value.created_revision(),
        SchemaDefinition::EventKind(value) => value.created_revision(),
    }
}

fn normalize_definition_revision(
    record: Record,
    revision: Revision,
) -> Result<SchemaDefinition, SchemaManagementError> {
    let definition = match record {
        Record::EntityTypeDefinition(value) => {
            SchemaDefinition::EntityType(worlddb_core::EntityTypeDefinition::new(
                value.entity_type_id(),
                value.symbol().clone(),
                value.description().map(String::from),
                value.lifecycle(),
                revision,
            ))
        }
        Record::PredicateDefinition(value) => {
            let rebuilt = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id: value.predicate_id(),
                symbol: value.symbol().clone(),
                subject_constraint: value.subject_constraint(),
                value_kind: value.value_kind(),
                object_constraint: value.object_constraint(),
                cardinality: value.cardinality(),
                resolution_policy: value.resolution_policy(),
                constraints: value.constraints().clone(),
                decimal_metadata: value.decimal_metadata(),
                lifecycle: value.lifecycle(),
                created_revision: revision,
            })
            .map_err(|_| SchemaManagementError::InvalidCandidate("predicate schema is invalid"))?;
            SchemaDefinition::Predicate(rebuilt)
        }
        Record::EventKindDefinition(value) => {
            let rebuilt = worlddb_core::EventKindDefinition::new(
                value.event_kind_id(),
                value.symbol().clone(),
                value.roles().to_vec(),
                value.attributes().to_vec(),
                value.event_time_constraint(),
                value.lifecycle(),
                revision,
            )
            .map_err(|_| SchemaManagementError::InvalidCandidate("event schema is invalid"))?;
            SchemaDefinition::EventKind(rebuilt)
        }
        _ => {
            return Err(SchemaManagementError::InvalidCandidate(
                "only EntityType, Predicate, and EventKind definitions can be managed here",
            ));
        }
    };
    Ok(definition)
}

fn definition_record(definition: &SchemaDefinition) -> Result<Record, SchemaManagementError> {
    match definition {
        SchemaDefinition::EntityType(value) => Ok(Record::EntityTypeDefinition(value.clone())),
        SchemaDefinition::Predicate(value) => Ok(Record::PredicateDefinition(value.clone())),
        SchemaDefinition::EventKind(value) => Ok(Record::EventKindDefinition(value.clone())),
        SchemaDefinition::Layer(_) | SchemaDefinition::LayerSnapshot(_) => {
            Err(SchemaManagementError::InvalidCandidate(
                "layer definitions are managed by Branch/Layer management",
            ))
        }
    }
}

fn validate_symbol_grammar(definition: &SchemaDefinition) -> Result<(), SchemaManagementError> {
    let symbols = match definition {
        SchemaDefinition::EntityType(value) => vec![value.symbol()],
        SchemaDefinition::Predicate(value) => vec![value.symbol()],
        SchemaDefinition::EventKind(value) => {
            let mut symbols = vec![value.symbol()];
            symbols.extend(value.roles().iter().map(|role| role.symbol()));
            symbols.extend(
                value
                    .attributes()
                    .iter()
                    .map(|attribute| attribute.symbol()),
            );
            symbols
        }
        SchemaDefinition::Layer(_) | SchemaDefinition::LayerSnapshot(_) => Vec::new(),
    };
    for symbol in symbols {
        let value = symbol.as_str();
        let mut bytes = value.bytes();
        let valid_first = bytes.next().is_some_and(|byte| byte.is_ascii_lowercase());
        if !valid_first
            || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(SchemaManagementError::InvalidCandidate(
                "schema symbols must match [a-z][a-z0-9_]*",
            ));
        }
    }
    Ok(())
}

fn validate_identity_changes(
    base: &SchemaSnapshot,
    candidate: &[SchemaDefinition],
    revision: Revision,
) -> Result<(), SchemaManagementError> {
    for next in candidate {
        let prior = match next {
            SchemaDefinition::EntityType(value) => base.definitions().iter().find(|item| {
                matches!(
                    *item,
                    SchemaDefinition::EntityType(found)
                        if found.entity_type_id() == value.entity_type_id()
                )
            }),
            SchemaDefinition::Predicate(value) => base.definitions().iter().find(|item| {
                matches!(
                    *item,
                    SchemaDefinition::Predicate(found)
                        if found.predicate_id() == value.predicate_id()
                )
            }),
            SchemaDefinition::EventKind(value) => base.definitions().iter().find(|item| {
                matches!(
                    *item,
                    SchemaDefinition::EventKind(found)
                        if found.event_kind_id() == value.event_kind_id()
                )
            }),
            SchemaDefinition::Layer(_) | SchemaDefinition::LayerSnapshot(_) => None,
        };
        if let Some(prior) = prior {
            let expected = revise_lifecycle(prior, next, revision)?;
            if canonical_definition_bytes(&expected)? != canonical_definition_bytes(next)? {
                return Err(SchemaManagementError::InvalidCandidate(
                    "existing schema identity may only receive a legal lifecycle revision; structural changes require migration",
                ));
            }
            if lifecycle(prior) == lifecycle(next) {
                return Err(SchemaManagementError::InvalidCandidate(
                    "schema lifecycle is already at that state",
                ));
            }
        } else if lifecycle(next) != Lifecycle::Active {
            return Err(SchemaManagementError::InvalidCandidate(
                "new schema definitions must begin Active",
            ));
        }
    }
    Ok(())
}

fn revise_lifecycle(
    prior: &SchemaDefinition,
    next: &SchemaDefinition,
    revision: Revision,
) -> Result<SchemaDefinition, SchemaManagementError> {
    let lifecycle = lifecycle(next);
    let result = match (prior, next) {
        (SchemaDefinition::EntityType(old), SchemaDefinition::EntityType(new))
            if old.entity_type_id() == new.entity_type_id() =>
        {
            SchemaDefinition::EntityType(old.revise_lifecycle(lifecycle, revision).map_err(
                |_| SchemaManagementError::InvalidCandidate("lifecycle cannot move backward"),
            )?)
        }
        (SchemaDefinition::Predicate(old), SchemaDefinition::Predicate(new))
            if old.predicate_id() == new.predicate_id() =>
        {
            SchemaDefinition::Predicate(old.revise_lifecycle(lifecycle, revision).map_err(
                |_| SchemaManagementError::InvalidCandidate("lifecycle cannot move backward"),
            )?)
        }
        (SchemaDefinition::EventKind(old), SchemaDefinition::EventKind(new))
            if old.event_kind_id() == new.event_kind_id() =>
        {
            SchemaDefinition::EventKind(old.revise_lifecycle(lifecycle, revision).map_err(
                |_| SchemaManagementError::InvalidCandidate("lifecycle cannot move backward"),
            )?)
        }
        _ => {
            return Err(SchemaManagementError::InvalidCandidate(
                "schema family or stable identity does not match",
            ));
        }
    };
    Ok(result)
}

fn lifecycle(definition: &SchemaDefinition) -> Lifecycle {
    match definition {
        SchemaDefinition::EntityType(value) => value.lifecycle(),
        SchemaDefinition::Predicate(value) => value.lifecycle(),
        SchemaDefinition::EventKind(value) => value.lifecycle(),
        SchemaDefinition::Layer(_) | SchemaDefinition::LayerSnapshot(_) => Lifecycle::Retired,
    }
}

fn canonical_definition_bytes(
    definition: &SchemaDefinition,
) -> Result<Vec<u8>, SchemaManagementError> {
    encode_record(&definition_record(definition)?).map_err(storage_error)
}

fn validate_references(snapshot: &SchemaSnapshot) -> Result<(), SchemaManagementError> {
    let mut entity_types = BTreeMap::new();
    for definition in snapshot.definitions() {
        if let SchemaDefinition::EntityType(entity_type) = definition {
            entity_types.insert(entity_type.entity_type_id(), entity_type.lifecycle());
        }
    }
    for definition in snapshot.definitions() {
        match definition {
            SchemaDefinition::Predicate(predicate) => {
                if predicate.lifecycle() != Lifecycle::Retired {
                    validate_entity_type_reference(predicate.subject_constraint(), &entity_types)?;
                    if let Some(constraint) = predicate.object_constraint() {
                        validate_entity_type_reference(constraint, &entity_types)?;
                    }
                }
            }
            SchemaDefinition::EventKind(event_kind) => {
                if event_kind.lifecycle() != Lifecycle::Retired {
                    for role in event_kind.roles() {
                        validate_entity_type_reference(role.entity_constraint(), &entity_types)?;
                    }
                    for attribute in event_kind.attributes() {
                        if let Some(constraint) = attribute.object_constraint() {
                            validate_entity_type_reference(constraint, &entity_types)?;
                        }
                    }
                }
            }
            SchemaDefinition::EntityType(_) => {}
            // Layer records remain part of the shared SchemaSnapshot, but this
            // manager never writes or validates their domain-specific rules.
            SchemaDefinition::Layer(_) | SchemaDefinition::LayerSnapshot(_) => {}
        }
    }
    Ok(())
}

fn validate_entity_type_reference(
    constraint: EntityTypeConstraint,
    types: &BTreeMap<worlddb_core::EntityTypeId, Lifecycle>,
) -> Result<(), SchemaManagementError> {
    if let EntityTypeConstraint::Exact(id) = constraint {
        match types.get(&id) {
            None => {
                return Err(SchemaManagementError::InvalidCandidate(
                    "schema references an unknown EntityType",
                ));
            }
            Some(Lifecycle::Retired) => {
                return Err(SchemaManagementError::InvalidCandidate(
                    "schema cannot introduce a reference to a Retired EntityType",
                ));
            }
            Some(Lifecycle::Active | Lifecycle::Deprecated) => {}
        }
    }
    Ok(())
}

fn next_audit_sequence(
    wal: &WalPrepareLog,
    writer_lock: &WriterLock,
) -> Result<AuditSequence, SchemaManagementError> {
    let committed = wal
        .committed_required_audit_records(writer_lock)
        .map_err(storage_error)?;
    let mut sequence = AuditSequence::new(0);
    for entry in committed {
        if entry.record().sequence() > sequence {
            sequence = entry.record().sequence();
        }
    }
    sequence
        .next()
        .map_err(|error| SchemaManagementError::Storage(error.to_string()))
}

fn authorize(
    policy: &worlddb_core::SecurityPolicySnapshot,
    principal: PrincipalId,
    capability: Capability,
) -> Result<(), SchemaManagementError> {
    if policy.authorize(principal, capability, PolicyTarget::default())
        == AuthorizationDecision::Allow
    {
        Ok(())
    } else {
        Err(SchemaManagementError::Unauthorized(capability))
    }
}

fn cleanup_staged_schema(
    history: &HistorySegmentStore,
    history_reference: ManifestSegmentReference,
    security: &SecurityPolicyHistoryStore,
    security_reference: ManifestSegmentReference,
) {
    let _ = history.remove_staged_reference(history_reference);
    let _ = security.remove_staged_reference(security_reference);
}

fn storage_error(error: impl fmt::Display) -> SchemaManagementError {
    SchemaManagementError::Storage(error.to_string())
}

/// Schema read, validation, authorization, or publication failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaManagementError {
    /// The current host Principal lacks the requested schema capability.
    Unauthorized(Capability),
    /// The live WAL head differs from the caller's expected base revision.
    Conflict,
    /// The supplied candidate is malformed or violates schema history rules.
    InvalidCandidate(&'static str),
    /// The requested idempotency identity has already been used.
    OperationAlreadyUsed,
    /// The commit point may have been reached but the outcome cannot be proved.
    UnknownCommit(OperationId),
    /// The commit outcome is unknown and recovery emitted a diagnostic.
    UnknownCommitWithDiagnostic(OperationId, String),
    /// A schema history snapshot or publication is invalid.
    History(worlddb_core::SchemaHistoryError),
    /// File storage, recovery, policy, or audit failed.
    Storage(String),
}

impl fmt::Display for SchemaManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(capability) => {
                write!(formatter, "missing required capability {capability:?}")
            }
            Self::Conflict => {
                formatter.write_str("database changed; reload the schema before retrying")
            }
            Self::InvalidCandidate(reason) => formatter.write_str(reason),
            Self::OperationAlreadyUsed => formatter.write_str("OperationId is already used"),
            Self::UnknownCommit(operation_id) => {
                write!(
                    formatter,
                    "schema commit outcome is unknown for {operation_id}"
                )
            }
            Self::UnknownCommitWithDiagnostic(operation_id, reason) => write!(
                formatter,
                "schema commit outcome is unknown for {operation_id}: {reason}"
            ),
            Self::History(error) => write!(formatter, "schema history is invalid: {error}"),
            Self::Storage(reason) => write!(formatter, "schema storage operation failed: {reason}"),
        }
    }
}

impl std::error::Error for SchemaManagementError {}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{
        AuditPolicyFingerprint, AuditRecordId, CapabilityGrant, CapabilityRule, Cardinality,
        ConstraintSet, DomainId, EntityTypeConstraint, EntityTypeDefinition, EntityTypeId,
        GrantEffect, Lifecycle, PolicyRuleId, PolicyScope, PolicySubject, PredicateDefinition,
        PredicateDefinitionSpec, PredicateId, Principal, Record, ResolutionPolicy, SchemaRevision,
        SecurityPolicyChange, SecurityPolicyRecord, SecurityPolicySnapshot, Symbol, ValueKind,
    };

    use crate::{
        DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference, RecoveryManager,
        SecurityPolicyHistoryStore, StorageVerifier, WalPrepareLog,
    };

    use super::{FileSchemaManager, SchemaManagementError};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    pub(super) struct TempArea(PathBuf);

    impl TempArea {
        pub(super) fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-schema-management-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        pub(super) fn database(&self) -> PathBuf {
            self.0.join("database")
        }
    }

    impl Drop for TempArea {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub(super) fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    pub(super) fn create_project(
        root: &PathBuf,
        capabilities: &[worlddb_core::Capability],
    ) -> Result<worlddb_core::PrincipalId, String> {
        let principal = id::<worlddb_core::PrincipalId>(1)?;
        let layout = DatabaseLayout::create(root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let policies = SecurityPolicyHistoryStore::new(layout.clone());
        let genesis = worlddb_core::SecurityPolicyVersion::new(
            worlddb_core::Revision::GENESIS,
            worlddb_core::SecurityEpoch::INITIAL,
            SecurityPolicySnapshot::default(),
        );
        let genesis_receipt = policies
            .stage_version(&lock, &genesis, None, None)
            .map_err(|error| error.to_string())?;
        let rules = capabilities
            .iter()
            .copied()
            .enumerate()
            .map(|(index, capability)| -> Result<_, String> {
                let tail = u8::try_from(index + 1).map_err(|error| error.to_string())?;
                Ok(CapabilityRule::new(
                    id::<PolicyRuleId>(tail)?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(capability, GrantEffect::Allow),
                    PolicyScope::project(),
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let active_snapshot = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![],
            vec![],
            rules.clone(),
        )
        .map_err(|error| error.to_string())?;
        let epoch = worlddb_core::SecurityEpoch::INITIAL
            .next()
            .map_err(|error| error.to_string())?;
        let policy_record = SecurityPolicyRecord::new(
            id::<worlddb_core::SecurityPolicyRecordId>(40)?,
            worlddb_core::Revision::FIRST_COMMIT,
            principal,
            epoch,
            std::iter::once(SecurityPolicyChange::PrincipalRegistered {
                principal_id: principal,
            })
            .chain(
                rules
                    .iter()
                    .map(|rule| SecurityPolicyChange::CapabilityRuleAdded {
                        rule_id: rule.id(),
                        subject: rule.subject(),
                        capability: rule.grant().capability(),
                        effect: rule.grant().effect(),
                        scope: rule.scope(),
                    }),
            )
            .collect(),
        )
        .map_err(|error| error.to_string())?;
        let active = worlddb_core::SecurityPolicyVersion::new(
            worlddb_core::Revision::FIRST_COMMIT,
            epoch,
            active_snapshot,
        );
        let active_receipt = policies
            .stage_version(&lock, &active, Some(&policy_record), None)
            .map_err(|error| error.to_string())?;
        let genesis_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            genesis_receipt.id(),
            genesis_receipt.content_digest(),
            worlddb_core::Revision::GENESIS,
        );
        let active_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            active_receipt.id(),
            active_receipt.content_digest(),
            worlddb_core::Revision::FIRST_COMMIT,
        );
        let operation = id::<worlddb_core::OperationId>(50)?;
        let audit = worlddb_core::AuditRecord::new(
            worlddb_core::AuditRecordIdentity {
                record_id: id::<AuditRecordId>(51)?,
                sequence: worlddb_core::AuditSequence::new(1),
                audit_operation_id: id::<worlddb_core::AuditOperationId>(52)?,
            },
            worlddb_core::AuditRecordDetails {
                actor: principal,
                action: worlddb_core::AuditAction::SecurityPolicyChange,
                object_class: worlddb_core::AuditObjectClass::SecurityPolicy,
                outcome: worlddb_core::AuditOutcome::Succeeded,
                commit_context: worlddb_core::AuditCommitContext::Committed {
                    revision: worlddb_core::Revision::FIRST_COMMIT,
                    operation_id: operation,
                },
                security_epoch: worlddb_core::SecurityEpoch::INITIAL,
                policy_fingerprint: AuditPolicyFingerprint::new(worlddb_core::Bytes::new(
                    SecurityPolicySnapshot::default()
                        .effective_capability_fingerprint(
                            principal,
                            worlddb_core::PolicyTarget::default(),
                        )
                        .to_vec(),
                ))
                .map_err(|error| error.to_string())?,
            },
        );
        WalPrepareLog::new(&layout)
            .commit_audited_manifest_snapshot(
                &lock,
                operation,
                vec![genesis_reference, active_reference],
                &[genesis_reference, active_reference],
                &audit,
            )
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        let report = StorageVerifier::new(layout)
            .verify(&lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("test project did not verify cleanly".to_owned());
        }
        drop(lock);
        Ok(principal)
    }

    #[test]
    fn schema_publication_is_audited_and_retains_historical_views() -> Result<(), String> {
        let area = TempArea::create()?;
        let database = area.database();
        let principal = create_project(
            &database,
            &[
                worlddb_core::Capability::SchemaRead,
                worlddb_core::Capability::SchemaManage,
            ],
        )?;
        let layout = DatabaseLayout::open(&database).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let base = manager.revision();
        let definition = EntityTypeDefinition::new(
            id::<EntityTypeId>(60)?,
            Symbol::new("character").map_err(|error| error.to_string())?,
            Some("A person or creature.".to_owned()),
            worlddb_core::Lifecycle::Active,
            worlddb_core::Revision::GENESIS,
        );
        let receipt = manager
            .publish(
                base,
                id::<worlddb_core::OperationId>(61)?,
                vec![Record::EntityTypeDefinition(definition)],
            )
            .map_err(|error| error.to_string())?;
        if receipt.revision() != base.next_commit().map_err(|error| error.to_string())? {
            return Err("schema publication used an unexpected revision".to_owned());
        }
        let historical = manager
            .schema_at(
                worlddb_core::SchemaMode::Explicit(SchemaRevision::from_published_revision(base)),
                base,
            )
            .map_err(|error| error.to_string())?;
        let current = manager
            .schema_at(worlddb_core::SchemaMode::Current, manager.revision())
            .map_err(|error| error.to_string())?;
        if !historical.definitions().is_empty() || current.definitions().len() != 1 {
            return Err("historical or current schema view is incorrect".to_owned());
        }
        let audit = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.operation_id() == receipt.operation_id())
            .ok_or_else(|| String::from("schema publication Required Audit record is missing"))?;
        if audit.record().action() != worlddb_core::AuditAction::SchemaManagement
            || audit.record().object_class() != worlddb_core::AuditObjectClass::SchemaDefinition
            || audit.revision() != receipt.revision()
        {
            return Err("schema audit record does not match the publication".to_owned());
        }
        let report = StorageVerifier::new(layout)
            .verify(&lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("schema publication failed storage verification".to_owned());
        }
        Ok(())
    }

    #[test]
    fn schema_publication_requires_current_manage_capability() -> Result<(), String> {
        let area = TempArea::create()?;
        let database = area.database();
        let principal = create_project(&database, &[worlddb_core::Capability::SchemaRead])?;
        let layout = DatabaseLayout::open(&database).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let definition = EntityTypeDefinition::new(
            id::<EntityTypeId>(70)?,
            Symbol::new("place").map_err(|error| error.to_string())?,
            None,
            worlddb_core::Lifecycle::Active,
            worlddb_core::Revision::GENESIS,
        );
        let result = manager.publish(
            manager.revision(),
            id::<worlddb_core::OperationId>(71)?,
            vec![Record::EntityTypeDefinition(definition)],
        );
        if !matches!(
            result,
            Err(SchemaManagementError::Unauthorized(
                worlddb_core::Capability::SchemaManage
            ))
        ) {
            return Err("schema publication without SchemaManage was accepted".to_owned());
        }
        if manager.revision() != worlddb_core::Revision::FIRST_COMMIT {
            return Err("unauthorized schema publication changed the database head".to_owned());
        }
        Ok(())
    }

    #[test]
    fn referenced_entity_type_and_dependents_retire_in_one_schema_commit() -> Result<(), String> {
        let area = TempArea::create()?;
        let database = area.database();
        let principal = create_project(
            &database,
            &[
                worlddb_core::Capability::SchemaRead,
                worlddb_core::Capability::SchemaManage,
            ],
        )?;
        let layout = DatabaseLayout::open(&database).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager =
            FileSchemaManager::open(layout, &lock, principal).map_err(|error| error.to_string())?;
        let entity_type_id = id::<EntityTypeId>(80)?;
        let predicate_id = id::<PredicateId>(81)?;
        let entity_type = EntityTypeDefinition::new(
            entity_type_id,
            Symbol::new("person").map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            worlddb_core::Revision::GENESIS,
        );
        let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: Symbol::new("display_name").map_err(|error| error.to_string())?,
            subject_constraint: EntityTypeConstraint::Exact(entity_type_id),
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints: ConstraintSet::default(),
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: worlddb_core::Revision::GENESIS,
        })
        .map_err(|error| error.to_string())?;
        let first = manager
            .publish(
                manager.revision(),
                id::<worlddb_core::OperationId>(82)?,
                vec![
                    Record::EntityTypeDefinition(entity_type.clone()),
                    Record::PredicateDefinition(predicate.clone()),
                ],
            )
            .map_err(|error| error.to_string())?;
        let deprecation_revision = first
            .revision()
            .next_commit()
            .map_err(|error| error.to_string())?;
        let deprecated_entity = entity_type
            .revise_lifecycle(Lifecycle::Deprecated, deprecation_revision)
            .map_err(|error| error.to_string())?;
        let deprecated_predicate = predicate
            .revise_lifecycle(Lifecycle::Deprecated, deprecation_revision)
            .map_err(|error| error.to_string())?;
        let second = manager
            .publish(
                first.revision(),
                id::<worlddb_core::OperationId>(83)?,
                vec![
                    Record::EntityTypeDefinition(deprecated_entity.clone()),
                    Record::PredicateDefinition(deprecated_predicate.clone()),
                ],
            )
            .map_err(|error| error.to_string())?;
        let target = second
            .revision()
            .next_commit()
            .map_err(|error| error.to_string())?;
        let retired_entity = deprecated_entity
            .revise_lifecycle(Lifecycle::Retired, target)
            .map_err(|error| error.to_string())?;
        let retired_predicate = deprecated_predicate
            .revise_lifecycle(Lifecycle::Retired, target)
            .map_err(|error| error.to_string())?;

        let rejected = manager.publish(
            second.revision(),
            id::<worlddb_core::OperationId>(84)?,
            vec![Record::EntityTypeDefinition(retired_entity.clone())],
        );
        if !matches!(rejected, Err(SchemaManagementError::InvalidCandidate(_))) {
            return Err(
                "referenced EntityType was retired without its dependent schema".to_owned(),
            );
        }
        if manager.revision() != second.revision() {
            return Err("rejected retirement changed the live schema revision".to_owned());
        }

        let retired = manager
            .publish(
                second.revision(),
                id::<worlddb_core::OperationId>(85)?,
                vec![
                    Record::EntityTypeDefinition(retired_entity),
                    Record::PredicateDefinition(retired_predicate),
                ],
            )
            .map_err(|error| error.to_string())?;
        let before = manager
            .schema_at(
                worlddb_core::SchemaMode::Explicit(SchemaRevision::from_published_revision(
                    second.revision(),
                )),
                second.revision(),
            )
            .map_err(|error| error.to_string())?;
        let active_history = manager
            .schema_at(
                worlddb_core::SchemaMode::Explicit(SchemaRevision::from_published_revision(
                    first.revision(),
                )),
                first.revision(),
            )
            .map_err(|error| error.to_string())?;
        let after = manager
            .schema_at(worlddb_core::SchemaMode::Current, retired.revision())
            .map_err(|error| error.to_string())?;
        let before_active =
            active_history
                .definitions()
                .iter()
                .all(|definition| match definition {
                    worlddb_core::SchemaDefinition::EntityType(value) => {
                        value.lifecycle() == Lifecycle::Active
                    }
                    worlddb_core::SchemaDefinition::Predicate(value) => {
                        value.lifecycle() == Lifecycle::Active
                    }
                    _ => true,
                });
        let after_retired = after
            .definitions()
            .iter()
            .all(|definition| match definition {
                worlddb_core::SchemaDefinition::EntityType(value) => {
                    value.lifecycle() == Lifecycle::Retired
                }
                worlddb_core::SchemaDefinition::Predicate(value) => {
                    value.lifecycle() == Lifecycle::Retired
                }
                _ => true,
            });
        let before_deprecated = before
            .definitions()
            .iter()
            .all(|definition| match definition {
                worlddb_core::SchemaDefinition::EntityType(value) => {
                    value.lifecycle() == Lifecycle::Deprecated
                }
                worlddb_core::SchemaDefinition::Predicate(value) => {
                    value.lifecycle() == Lifecycle::Deprecated
                }
                _ => true,
            });
        if !before_active || !before_deprecated || !after_retired || retired.revision() != target {
            return Err(
                "atomic lifecycle retirement did not preserve the expected history".to_owned(),
            );
        }
        Ok(())
    }
}
