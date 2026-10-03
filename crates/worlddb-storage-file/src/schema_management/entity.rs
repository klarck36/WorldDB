//! Authorized Entity catalog reads and WAL-backed Entity creation/retirement.

use std::fmt;

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuthorizationDecision, Bytes, Capability,
    Entity, EntityCatalogError, EntityCatalogSnapshot, EntityId, EntityRetirement,
    EntityRetirementId, EntityTypeDefinition, EntityTypeId, Lifecycle, OperationId, PolicyTarget,
    Record, Revision, SchemaDefinition, SchemaHistoryError, SchemaMode, SchemaSnapshot,
    SecurityPolicyVersion,
};

use crate::{
    HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference, RecoveryManager,
    SecurityPolicyHistoryStore, WalOperationStatus, WalPrepareLog,
};

use super::{FileSchemaManager, SchemaManagementError, cleanup_staged_schema};

/// Receipt for one committed Entity creation or retirement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityCatalogPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    entity: Option<Entity>,
    retirement: Option<EntityRetirement>,
    used_deprecated_type: bool,
}

impl EntityCatalogPublicationReceipt {
    /// Stable idempotency identity for this publication.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared database revision assigned to the operation.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Created Entity row, when this receipt describes creation.
    #[must_use]
    pub const fn entity(self) -> Option<Entity> {
        self.entity
    }

    /// Created retirement row, when this receipt describes retirement.
    #[must_use]
    pub const fn retirement(self) -> Option<EntityRetirement> {
        self.retirement
    }

    /// Whether creation explicitly opted into a Deprecated EntityType.
    #[must_use]
    pub const fn used_deprecated_type(self) -> bool {
        self.used_deprecated_type
    }
}

/// File-backed project Entity catalog manager.
///
/// The manager uses the shared History revision axis and commits its record,
/// policy-history advance, manifest, WAL receipt, and required AuditRecord as
/// one recoverable transaction. `FileSchemaManager` supplies the validated
/// current schema and the already-authenticated SchemaRead boundary.
pub struct FileEntityManager<'a> {
    schema: FileSchemaManager<'a>,
}

impl<'a> FileEntityManager<'a> {
    /// Opens and verifies the database and loads its authorized schema view.
    pub fn open(
        layout: crate::DatabaseLayout,
        writer_lock: &'a crate::WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<Self, EntityManagementError> {
        let schema =
            FileSchemaManager::open(layout, writer_lock, principal).map_err(map_schema_error)?;
        Ok(Self { schema })
    }

    /// Current committed shared revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.schema.revision()
    }

    /// Returns a schema view for a type-labelled Entity snapshot.
    pub fn schema_at(
        &self,
        mode: SchemaMode,
        recorded_as_of: Revision,
    ) -> Result<SchemaSnapshot, EntityManagementError> {
        self.schema
            .schema_at(mode, recorded_as_of)
            .map_err(map_schema_error)
    }

    /// Returns a catalog and matching schema at the requested shared revision.
    pub fn snapshot_at(
        &self,
        mode: SchemaMode,
        recorded_as_of: Revision,
    ) -> Result<(EntityCatalogSnapshot, SchemaSnapshot), EntityManagementError> {
        self.authorize(Capability::EntityRead)?;
        let revision = match mode {
            SchemaMode::Current => self.revision(),
            SchemaMode::Historical => recorded_as_of,
            SchemaMode::Explicit(schema_revision) => schema_revision.revision(),
        };
        if revision > self.revision() {
            return Err(EntityManagementError::RevisionNotPublished);
        }

        let catalog = self.catalog_at_revision(revision)?;
        let schema = self
            .schema
            .schema_history
            .schema_at(mode, recorded_as_of)
            .map_err(EntityManagementError::SchemaHistory)?;
        validate_entity_types_at_snapshot(&catalog, &schema)?;
        Ok((catalog, schema))
    }

    /// Creates one immutable Entity with a host-generated stable identity.
    pub fn create(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        entity_id: EntityId,
        entity_type_id: EntityTypeId,
        accept_deprecated_type: bool,
    ) -> Result<EntityCatalogPublicationReceipt, EntityManagementError> {
        if expected_base != self.revision() {
            return Err(EntityManagementError::Conflict);
        }
        self.authorize(Capability::EntityCreate)?;

        let schema = self
            .schema
            .schema_history
            .schema_at(SchemaMode::Current, self.revision())
            .map_err(EntityManagementError::SchemaHistory)?;
        let entity_type =
            entity_type(&schema, entity_type_id).ok_or(EntityManagementError::UnknownEntityType)?;
        let used_deprecated_type = match entity_type.lifecycle() {
            Lifecycle::Active if !accept_deprecated_type => false,
            Lifecycle::Active => return Err(EntityManagementError::UnexpectedDeprecatedOptIn),
            Lifecycle::Deprecated if !accept_deprecated_type => {
                return Err(EntityManagementError::DeprecatedTypeRequiresOptIn);
            }
            Lifecycle::Deprecated => {
                self.authorize(Capability::SchemaManage)?;
                true
            }
            Lifecycle::Retired => return Err(EntityManagementError::RetiredEntityType),
        };

        let base = self.catalog_at_revision(self.revision())?;
        if base.entity(entity_id).is_some() {
            return Err(EntityManagementError::DuplicateEntityIdentity);
        }
        let target_revision = self.schema.next_revision().map_err(map_schema_error)?;
        let entity = Entity::new(entity_id, entity_type_id, target_revision);
        let mut entities = base.entities().to_vec();
        entities.push(entity);
        base.revise(target_revision, entities, base.retirements().to_vec())
            .map_err(EntityManagementError::Catalog)?;

        self.publish_record(
            expected_base,
            operation_id,
            Record::Entity(entity),
            AuditAction::EntityCreation,
        )?;
        Ok(EntityCatalogPublicationReceipt {
            operation_id,
            revision: target_revision,
            entity: Some(entity),
            retirement: None,
            used_deprecated_type,
        })
    }

    /// Appends an irreversible retirement without rewriting existing records.
    pub fn retire(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retirement_id: EntityRetirementId,
        entity_id: EntityId,
    ) -> Result<EntityCatalogPublicationReceipt, EntityManagementError> {
        if expected_base != self.revision() {
            return Err(EntityManagementError::Conflict);
        }
        self.authorize(Capability::EntityRead)?;
        self.authorize(Capability::EntityRetire)?;

        let base = self.catalog_at_revision(self.revision())?;
        if base.entity(entity_id).is_none() {
            return Err(EntityManagementError::UnknownEntity);
        }
        if base.is_retired(entity_id) {
            return Err(EntityManagementError::AlreadyRetired);
        }
        let target_revision = self.schema.next_revision().map_err(map_schema_error)?;
        let retirement = EntityRetirement::new(retirement_id, entity_id, target_revision);
        let mut retirements = base.retirements().to_vec();
        retirements.push(retirement);
        base.revise(target_revision, base.entities().to_vec(), retirements)
            .map_err(EntityManagementError::Catalog)?;

        self.publish_record(
            expected_base,
            operation_id,
            Record::EntityRetirement(retirement),
            AuditAction::EntityRetirement,
        )?;
        Ok(EntityCatalogPublicationReceipt {
            operation_id,
            revision: target_revision,
            entity: None,
            retirement: Some(retirement),
            used_deprecated_type: false,
        })
    }

    fn catalog_at_revision(
        &self,
        revision: Revision,
    ) -> Result<EntityCatalogSnapshot, EntityManagementError> {
        let manifest = &self.schema.manifest;
        if revision > manifest.revision() {
            return Err(EntityManagementError::RevisionNotPublished);
        }
        let store = HistorySegmentStore::new(self.schema.layout.clone());
        let mut entities = Vec::new();
        let mut retirements = Vec::new();
        for reference in manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        {
            let segment = store.read_segment(reference.id()).map_err(storage_error)?;
            if segment.content_digest() != reference.content_digest() {
                return Err(EntityManagementError::Storage(
                    "history segment digest differs from its manifest".to_owned(),
                ));
            }
            for decoded in segment.records() {
                match decoded.record() {
                    Record::Entity(entity) => {
                        let created = entity.created_revision();
                        validate_record_revision(created, *reference, manifest.revision())?;
                        if created <= revision {
                            entities.push(*entity);
                        }
                    }
                    Record::EntityRetirement(retirement) => {
                        let created = retirement.created_revision();
                        validate_record_revision(created, *reference, manifest.revision())?;
                        if created <= revision {
                            retirements.push(*retirement);
                        }
                    }
                    _ => {}
                }
            }
        }
        let catalog = EntityCatalogSnapshot::new(revision, entities, retirements)
            .map_err(EntityManagementError::Catalog)?;
        validate_entity_type_creation_history(&catalog, &self.schema.schema_history)?;
        Ok(catalog)
    }

    fn authorize(&self, capability: Capability) -> Result<(), EntityManagementError> {
        let policy = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| EntityManagementError::Storage(error.to_string()))?;
        if policy
            .snapshot()
            .authorize(self.schema.principal, capability, PolicyTarget::default())
            == AuthorizationDecision::Allow
        {
            Ok(())
        } else {
            Err(EntityManagementError::Unauthorized(capability))
        }
    }

    fn publish_record(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        record: Record,
        action: AuditAction,
    ) -> Result<(), EntityManagementError> {
        if expected_base != self.revision() {
            return Err(EntityManagementError::Conflict);
        }
        let wal = WalPrepareLog::new(&self.schema.layout);
        let live_head = wal
            .commit_head(self.schema.writer_lock)
            .map_err(storage_error)?
            .revision();
        if live_head != expected_base {
            return Err(EntityManagementError::Conflict);
        }
        if wal
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(EntityManagementError::OperationAlreadyUsed);
        }

        let policy_version = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| EntityManagementError::Storage(error.to_string()))?;
        let target_revision = expected_base
            .next_commit()
            .map_err(|error| EntityManagementError::Storage(error.to_string()))?;
        let record_revision = match &record {
            Record::Entity(entity) => entity.created_revision(),
            Record::EntityRetirement(retirement) => retirement.created_revision(),
            _ => return Err(EntityManagementError::InvalidRecordFamily),
        };
        if record_revision != target_revision {
            return Err(EntityManagementError::InvalidRecordRevision);
        }

        let sequence =
            super::next_audit_sequence(&wal, self.schema.writer_lock).map_err(map_schema_error)?;
        let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(
            policy_version
                .snapshot()
                .effective_capability_fingerprint(self.schema.principal, PolicyTarget::default())
                .to_vec(),
        ))
        .map_err(|error| EntityManagementError::Storage(error.to_string()))?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id:
                    worlddb_core::storage_internal::generate_entity_management_audit_record_id()
                        .map_err(storage_error)?,
                sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_entity_management_audit_operation_id()
                        .map_err(storage_error)?,
            },
            AuditRecordDetails {
                actor: self.schema.principal,
                action,
                object_class: AuditObjectClass::EntityCatalog,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: target_revision,
                    operation_id,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint,
            },
        );

        let history_store = HistorySegmentStore::new(self.schema.layout.clone());
        let history_receipt = history_store
            .stage_segment(self.schema.writer_lock, &[record])
            .map_err(storage_error)?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            target_revision,
        );

        let security_store = SecurityPolicyHistoryStore::new(self.schema.layout.clone());
        let retention = self
            .schema
            .policy_history
            .audit_retention_at(expected_base)
            .map_err(|error| EntityManagementError::Storage(error.to_string()))?;
        let next_policy_version = SecurityPolicyVersion::new(
            target_revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let security_receipt = match security_store.stage_version(
            self.schema.writer_lock,
            &next_policy_version,
            None,
            retention,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let cleanup = history_store.remove_staged_reference(history_reference);
                return Err(EntityManagementError::Storage(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; staged entity history cleanup failed: {cleanup_error}")
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

        let mut next_references = self.schema.manifest.segments().to_vec();
        next_references.push(history_reference);
        next_references.push(security_reference);
        let staged = [history_reference, security_reference];
        let commit_result = wal.commit_audited_manifest_snapshot(
            self.schema.writer_lock,
            operation_id,
            next_references,
            &staged,
            &audit_record,
        );
        match commit_result {
            Ok(receipt) if receipt.revision() == target_revision => {}
            Ok(_) => return Err(EntityManagementError::UnknownCommit(operation_id)),
            Err(commit_error) => {
                let recovered = RecoveryManager::new(self.schema.layout.clone())
                    .recover(self.schema.writer_lock)
                    .map_err(|recovery_error| {
                        EntityManagementError::UnknownCommitWithDiagnostic(
                            operation_id,
                            format!("{commit_error}; recovery: {recovery_error}"),
                        )
                    })?;
                let status = wal
                    .operation_status(self.schema.writer_lock, operation_id)
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
                        return Err(EntityManagementError::Storage(commit_error.to_string()));
                    }
                    return Err(EntityManagementError::UnknownCommitWithDiagnostic(
                        operation_id,
                        commit_error.to_string(),
                    ));
                }
            }
        }

        let recovered = RecoveryManager::new(self.schema.layout.clone())
            .recover(self.schema.writer_lock)
            .map_err(|error| {
                EntityManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
            })?;
        if !recovered.report().is_clean() {
            return Err(EntityManagementError::UnknownCommit(operation_id));
        }
        let next_schema = FileSchemaManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| {
            EntityManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
        })?;
        if next_schema.revision() != target_revision {
            return Err(EntityManagementError::UnknownCommit(operation_id));
        }
        self.schema = next_schema;
        Ok(())
    }
}

fn validate_record_revision(
    created: Revision,
    reference: ManifestSegmentReference,
    manifest_revision: Revision,
) -> Result<(), EntityManagementError> {
    if created > reference.through_revision() || created > manifest_revision {
        return Err(EntityManagementError::Storage(
            "Entity catalog record exceeds its committed history segment".to_owned(),
        ));
    }
    Ok(())
}

fn validate_entity_types_at_snapshot(
    catalog: &EntityCatalogSnapshot,
    schema: &SchemaSnapshot,
) -> Result<(), EntityManagementError> {
    for entity in catalog.entities() {
        if entity_type(schema, entity.entity_type_id()).is_none() {
            return Err(EntityManagementError::Storage(
                "Entity refers to an EntityType missing at the selected revision".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_entity_type_creation_history(
    catalog: &EntityCatalogSnapshot,
    history: &worlddb_core::SchemaHistoryReferenceModel,
) -> Result<(), EntityManagementError> {
    for entity in catalog.entities() {
        let schema = history
            .schema_at(SchemaMode::Historical, entity.created_revision())
            .map_err(EntityManagementError::SchemaHistory)?;
        match entity_type(&schema, entity.entity_type_id()).map(EntityTypeDefinition::lifecycle) {
            None | Some(Lifecycle::Retired) => {
                return Err(EntityManagementError::Storage(
                    "Entity was created with an unknown or retired EntityType".to_owned(),
                ));
            }
            Some(Lifecycle::Active | Lifecycle::Deprecated) => {}
        }
    }
    Ok(())
}

fn entity_type(schema: &SchemaSnapshot, id: EntityTypeId) -> Option<&EntityTypeDefinition> {
    schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::EntityType(value) if value.entity_type_id() == id => Some(value),
            _ => None,
        })
}

fn map_schema_error(error: SchemaManagementError) -> EntityManagementError {
    match error {
        SchemaManagementError::Unauthorized(capability) => {
            EntityManagementError::Unauthorized(capability)
        }
        SchemaManagementError::Conflict => EntityManagementError::Conflict,
        SchemaManagementError::OperationAlreadyUsed => EntityManagementError::OperationAlreadyUsed,
        SchemaManagementError::UnknownCommit(operation_id) => {
            EntityManagementError::UnknownCommit(operation_id)
        }
        SchemaManagementError::UnknownCommitWithDiagnostic(operation_id, diagnostic) => {
            EntityManagementError::UnknownCommitWithDiagnostic(operation_id, diagnostic)
        }
        other => EntityManagementError::Storage(other.to_string()),
    }
}

fn storage_error(error: impl fmt::Display) -> EntityManagementError {
    EntityManagementError::Storage(error.to_string())
}

/// Entity read, validation, authorization, or publication failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityManagementError {
    /// The host-authenticated Principal lacks the named permission.
    Unauthorized(Capability),
    /// The database changed before the requested revision could be published.
    Conflict,
    /// A selected catalog or schema revision is newer than the database head.
    RevisionNotPublished,
    /// The selected type identity is not visible in the requested schema view.
    UnknownEntityType,
    /// A new Entity cannot be assigned to a Retired EntityType.
    RetiredEntityType,
    /// Deprecated EntityType creation requires an explicit renderer opt-in.
    DeprecatedTypeRequiresOptIn,
    /// An explicit Deprecated opt-in was supplied for an Active type.
    UnexpectedDeprecatedOptIn,
    /// An EntityId collided with a catalog identity that already exists.
    DuplicateEntityIdentity,
    /// The requested entity is absent from the authorized catalog view.
    UnknownEntity,
    /// The entity already has its terminal retirement record.
    AlreadyRetired,
    /// The supplied operation identity has already been used.
    OperationAlreadyUsed,
    /// A record outside the closed Entity catalog family was supplied.
    InvalidRecordFamily,
    /// A catalog record revision differs from the next shared commit revision.
    InvalidRecordRevision,
    /// A catalog snapshot failed its append-only history checks.
    Catalog(EntityCatalogError),
    /// Shared schema history is malformed or does not cover the selected revision.
    SchemaHistory(SchemaHistoryError),
    /// The commit outcome cannot be established after recovery.
    UnknownCommit(OperationId),
    /// The commit outcome is unknown and recovery produced a diagnostic.
    UnknownCommitWithDiagnostic(OperationId, String),
    /// File storage, recovery, policy, or audit failed.
    Storage(String),
}

impl fmt::Display for EntityManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(capability) => {
                write!(formatter, "missing required capability {capability:?}")
            }
            Self::Conflict => {
                formatter.write_str("database changed; reload Entities before retrying")
            }
            Self::RevisionNotPublished => formatter.write_str("selected revision is not published"),
            Self::UnknownEntityType => formatter.write_str("EntityType is unknown or not visible"),
            Self::RetiredEntityType => {
                formatter.write_str("Retired EntityTypes cannot receive new Entities")
            }
            Self::DeprecatedTypeRequiresOptIn => {
                formatter.write_str("Deprecated EntityType requires explicit opt-in")
            }
            Self::UnexpectedDeprecatedOptIn => {
                formatter.write_str("Deprecated opt-in does not apply to an Active EntityType")
            }
            Self::DuplicateEntityIdentity => {
                formatter.write_str("generated EntityId already exists")
            }
            Self::UnknownEntity => formatter.write_str("Entity is unknown or not visible"),
            Self::AlreadyRetired => formatter.write_str("Entity is already retired"),
            Self::OperationAlreadyUsed => formatter.write_str("OperationId is already used"),
            Self::InvalidRecordFamily => {
                formatter.write_str("record is not an Entity catalog record")
            }
            Self::InvalidRecordRevision => {
                formatter.write_str("Entity catalog record does not use the next shared revision")
            }
            Self::Catalog(error) => write!(formatter, "invalid Entity catalog: {error}"),
            Self::SchemaHistory(error) => write!(formatter, "invalid schema history: {error}"),
            Self::UnknownCommit(operation_id) => write!(
                formatter,
                "Entity commit outcome is unknown for {operation_id}"
            ),
            Self::UnknownCommitWithDiagnostic(operation_id, reason) => write!(
                formatter,
                "Entity commit outcome is unknown for {operation_id}: {reason}"
            ),
            Self::Storage(reason) => write!(formatter, "Entity storage operation failed: {reason}"),
        }
    }
}

impl std::error::Error for EntityManagementError {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use worlddb_core::{
        AuditAction, AuditCommitContext, AuditObjectClass, Capability, EntityTypeDefinition,
        EntityTypeId, Lifecycle, Record, SchemaDefinition, SchemaMode, Symbol,
    };

    use crate::{DatabaseLayout, FileSchemaManager, WalPrepareLog};

    use super::super::tests::{TempArea, create_project, id};
    use super::{EntityManagementError, FileEntityManager};

    const TEST_CAPABILITIES: &[Capability] = &[
        Capability::SchemaRead,
        Capability::SchemaManage,
        Capability::EntityCreate,
        Capability::EntityRead,
        Capability::EntityRetire,
    ];

    fn add_entity_type(
        root: &PathBuf,
        principal: worlddb_core::PrincipalId,
        type_id: EntityTypeId,
        symbol: &str,
        operation_tail: u8,
    ) -> Result<worlddb_core::Revision, String> {
        let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut schema =
            FileSchemaManager::open(layout, &lock, principal).map_err(|error| error.to_string())?;
        let revision = schema.next_revision().map_err(|error| error.to_string())?;
        let definition = EntityTypeDefinition::new(
            type_id,
            Symbol::new(symbol).map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            revision,
        );
        let receipt = schema
            .publish(
                schema.revision(),
                id::<worlddb_core::OperationId>(operation_tail)?,
                vec![Record::EntityTypeDefinition(definition)],
            )
            .map_err(|error| error.to_string())?;
        Ok(receipt.revision())
    }

    #[test]
    fn entity_create_retire_share_history_and_required_audit() -> Result<(), String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(&root, TEST_CAPABILITIES)?;
        let type_id = id::<EntityTypeId>(61)?;
        let type_revision = add_entity_type(&root, principal, type_id, "person", 62)?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut entities = FileEntityManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;

        let before = entities
            .snapshot_at(
                SchemaMode::Explicit(worlddb_core::SchemaRevision::from_published_revision(
                    type_revision,
                )),
                type_revision,
            )
            .map_err(|error| error.to_string())?;
        if !before.0.entities().is_empty() {
            return Err("Entity catalog was not empty before creation".to_owned());
        }

        let entity_id = id::<worlddb_core::EntityId>(63)?;
        let created = entities
            .create(
                type_revision,
                id::<worlddb_core::OperationId>(64)?,
                entity_id,
                type_id,
                false,
            )
            .map_err(|error| error.to_string())?;
        let created_revision = created.revision();
        let historical_before = entities
            .snapshot_at(SchemaMode::Historical, type_revision)
            .map_err(|error| error.to_string())?;
        let historical_created = entities
            .snapshot_at(SchemaMode::Historical, created_revision)
            .map_err(|error| error.to_string())?;
        if !historical_before.0.entities().is_empty()
            || historical_created.0.entities().len() != 1
            || historical_created
                .0
                .entities()
                .first()
                .map(|entity| entity.entity_type_id())
                != Some(type_id)
        {
            return Err(
                "Entity creation was not visible on the correct historical revision".to_owned(),
            );
        }

        let retired = entities
            .retire(
                created_revision,
                id::<worlddb_core::OperationId>(65)?,
                id::<worlddb_core::EntityRetirementId>(66)?,
                entity_id,
            )
            .map_err(|error| error.to_string())?;
        let after_retirement = entities
            .snapshot_at(SchemaMode::Current, worlddb_core::Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        let still_active_before = entities
            .snapshot_at(SchemaMode::Historical, created_revision)
            .map_err(|error| error.to_string())?;
        if !after_retirement.0.is_retired(entity_id)
            || still_active_before.0.is_retired(entity_id)
            || retired.revision() <= created_revision
        {
            return Err("Entity retirement rewrote history or failed to publish".to_owned());
        }

        let committed = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        let entity_audits = committed
            .iter()
            .filter_map(|entry| {
                let audit = entry.record();
                matches!(
                    audit.action(),
                    AuditAction::EntityCreation | AuditAction::EntityRetirement
                )
                .then_some((
                    audit.action(),
                    audit.object_class(),
                    audit.commit_context(),
                ))
            })
            .collect::<Vec<_>>();
        if entity_audits.len() != 2
            || entity_audits
                .iter()
                .any(|(_, object, _)| *object != AuditObjectClass::EntityCatalog)
            || !entity_audits.iter().any(|(action, _, context)| {
                *action == AuditAction::EntityCreation
                    && *context
                        == AuditCommitContext::Committed {
                            revision: created_revision,
                            operation_id: created.operation_id(),
                        }
            })
            || !entity_audits.iter().any(|(action, _, context)| {
                *action == AuditAction::EntityRetirement
                    && *context
                        == AuditCommitContext::Committed {
                            revision: retired.revision(),
                            operation_id: retired.operation_id(),
                        }
            })
        {
            return Err("Entity changes were not audited with their shared commit".to_owned());
        }
        drop(lock);
        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn deprecated_type_requires_opt_in_and_retired_types_reject_creation() -> Result<(), String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(&root, TEST_CAPABILITIES)?;
        let type_id = id::<EntityTypeId>(71)?;
        let active_revision = add_entity_type(&root, principal, type_id, "npc", 72)?;

        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut schema = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let deprecated_revision = schema.next_revision().map_err(|error| error.to_string())?;
        let active = schema
            .schema_at(SchemaMode::Current, active_revision)
            .map_err(|error| error.to_string())?
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::EntityType(value) if value.entity_type_id() == type_id => {
                    Some(value.clone())
                }
                _ => None,
            })
            .ok_or_else(|| "new EntityType missing".to_owned())?;
        let deprecated = active
            .revise_lifecycle(Lifecycle::Deprecated, deprecated_revision)
            .map_err(|error| error.to_string())?;
        let deprecation = schema
            .publish(
                active_revision,
                id::<worlddb_core::OperationId>(73)?,
                vec![Record::EntityTypeDefinition(deprecated)],
            )
            .map_err(|error| error.to_string())?;
        let mut entities = FileEntityManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;

        let missing_opt_in = entities.create(
            deprecation.revision(),
            id::<worlddb_core::OperationId>(74)?,
            id::<worlddb_core::EntityId>(75)?,
            type_id,
            false,
        );
        if !matches!(
            missing_opt_in,
            Err(EntityManagementError::DeprecatedTypeRequiresOptIn)
        ) || entities.revision() != deprecation.revision()
        {
            return Err("Deprecated EntityType was used without explicit consent".to_owned());
        }
        let opted_in = entities
            .create(
                deprecation.revision(),
                id::<worlddb_core::OperationId>(76)?,
                id::<worlddb_core::EntityId>(77)?,
                type_id,
                true,
            )
            .map_err(|error| error.to_string())?;
        if !opted_in.used_deprecated_type() {
            return Err(
                "Deprecated EntityType opt-in did not produce its typed warning state".to_owned(),
            );
        }

        let mut schema = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let retired_revision = schema.next_revision().map_err(|error| error.to_string())?;
        let deprecated = schema
            .schema_at(SchemaMode::Current, retired_revision)
            .map_err(|error| error.to_string())?
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::EntityType(value) if value.entity_type_id() == type_id => {
                    Some(value.clone())
                }
                _ => None,
            })
            .ok_or_else(|| "Deprecated EntityType missing".to_owned())?;
        let retired_type = deprecated
            .revise_lifecycle(Lifecycle::Retired, retired_revision)
            .map_err(|error| error.to_string())?;
        let retired_schema = schema
            .publish(
                opted_in.revision(),
                id::<worlddb_core::OperationId>(78)?,
                vec![Record::EntityTypeDefinition(retired_type)],
            )
            .map_err(|error| error.to_string())?;
        let mut entities =
            FileEntityManager::open(layout, &lock, principal).map_err(|error| error.to_string())?;
        let rejected = entities.create(
            retired_schema.revision(),
            id::<worlddb_core::OperationId>(79)?,
            id::<worlddb_core::EntityId>(80)?,
            type_id,
            true,
        );
        if !matches!(rejected, Err(EntityManagementError::RetiredEntityType))
            || entities.revision() != retired_schema.revision()
        {
            return Err("Retired EntityType accepted a new Entity".to_owned());
        }
        drop(lock);
        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn read_and_create_permissions_are_checked_without_advancing_history() -> Result<(), String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(&root, &[Capability::SchemaRead, Capability::SchemaManage])?;
        let type_id = id::<EntityTypeId>(91)?;
        let revision = add_entity_type(&root, principal, type_id, "place", 92)?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut entities =
            FileEntityManager::open(layout, &lock, principal).map_err(|error| error.to_string())?;
        let read = entities.snapshot_at(SchemaMode::Current, worlddb_core::Revision::GENESIS);
        let create = entities.create(
            revision,
            id::<worlddb_core::OperationId>(93)?,
            id::<worlddb_core::EntityId>(94)?,
            type_id,
            false,
        );
        if !matches!(
            read,
            Err(EntityManagementError::Unauthorized(Capability::EntityRead))
        ) || !matches!(
            create,
            Err(EntityManagementError::Unauthorized(
                Capability::EntityCreate
            ))
        ) || entities.revision() != revision
        {
            return Err("Entity permission denial leaked or published catalog state".to_owned());
        }
        drop(lock);
        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn stale_live_head_rejects_entity_creation_without_publication() -> Result<(), String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(&root, TEST_CAPABILITIES)?;
        let type_id = id::<EntityTypeId>(101)?;
        let base_revision = add_entity_type(&root, principal, type_id, "stale_test", 102)?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut entities = FileEntityManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;

        let mut schema = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let next_revision = schema.next_revision().map_err(|error| error.to_string())?;
        let second_type = EntityTypeDefinition::new(
            id::<EntityTypeId>(103)?,
            Symbol::new("head_advanced").map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            next_revision,
        );
        let advanced = schema
            .publish(
                base_revision,
                id::<worlddb_core::OperationId>(104)?,
                vec![Record::EntityTypeDefinition(second_type)],
            )
            .map_err(|error| error.to_string())?;

        let rejected = entities.create(
            base_revision,
            id::<worlddb_core::OperationId>(105)?,
            id::<worlddb_core::EntityId>(106)?,
            type_id,
            false,
        );
        if !matches!(rejected, Err(EntityManagementError::Conflict))
            || entities.revision() != base_revision
        {
            return Err("Entity creation published against a stale live head".to_owned());
        }
        let current =
            FileEntityManager::open(layout, &lock, principal).map_err(|error| error.to_string())?;
        if current.revision() != advanced.revision()
            || !current
                .snapshot_at(SchemaMode::Current, worlddb_core::Revision::GENESIS)
                .map_err(|error| error.to_string())?
                .0
                .entities()
                .is_empty()
        {
            return Err("A rejected stale Entity operation left catalog history behind".to_owned());
        }
        drop(lock);
        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn retirement_permission_is_checked_without_advancing_history() -> Result<(), String> {
        let area = TempArea::create()?;
        let root = area.database();
        let capabilities = &[
            Capability::SchemaRead,
            Capability::SchemaManage,
            Capability::EntityCreate,
            Capability::EntityRead,
        ];
        let principal = create_project(&root, capabilities)?;
        let type_id = id::<EntityTypeId>(111)?;
        let type_revision = add_entity_type(&root, principal, type_id, "retire_denied", 112)?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut entities =
            FileEntityManager::open(layout, &lock, principal).map_err(|error| error.to_string())?;
        let entity_id = id::<worlddb_core::EntityId>(113)?;
        let created = entities
            .create(
                type_revision,
                id::<worlddb_core::OperationId>(114)?,
                entity_id,
                type_id,
                false,
            )
            .map_err(|error| error.to_string())?;
        let rejected = entities.retire(
            created.revision(),
            id::<worlddb_core::OperationId>(115)?,
            id::<worlddb_core::EntityRetirementId>(116)?,
            entity_id,
        );
        let current = entities
            .snapshot_at(SchemaMode::Current, worlddb_core::Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        if !matches!(
            rejected,
            Err(EntityManagementError::Unauthorized(
                Capability::EntityRetire
            ))
        ) || entities.revision() != created.revision()
            || current.0.is_retired(entity_id)
        {
            return Err("Unauthorized Entity retirement changed catalog state".to_owned());
        }
        drop(lock);
        let _ = fs::remove_dir_all(root);
        Ok(())
    }
}
