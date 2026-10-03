//! Authorized project metadata catalog views backed by ordinary WAL commits.

use std::fmt;

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuthorizationDecision, Bytes, Capability,
    EntityCatalogSnapshot, HistorySpaceCatalog, HistorySpaceDefinition, HistorySpaceId,
    LayerDefinition, LayerId, LayerSchemaSnapshot, Lifecycle, OperationId,
    PerspectiveCatalogSnapshot, PerspectiveDefinitionRevision, PerspectiveId,
    PerspectiveRetirement, PerspectiveRetirementId, PolicyTarget, ProjectMetadataCandidate,
    ProjectMetadataSnapshot, ProjectMetadataValidationError, Record, RecordRef, Revision,
    SchemaDefinition, SchemaMode, SchemaRevision, SchemaSnapshot, SecurityPolicyVersion, Symbol,
};

use crate::{
    HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference, RecoveryManager,
    SecurityPolicyHistoryStore, WalOperationStatus, WalPrepareLog,
};

use super::{
    FileSchemaManager, SchemaManagementError, cleanup_staged_schema, load_schema_history,
    next_audit_sequence,
};

/// Receipt for a committed HistorySpace or Layer catalog operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    history_space_id: Option<HistorySpaceId>,
    layer_id: Option<LayerId>,
    perspective_id: Option<PerspectiveId>,
    perspective_retirement_id: Option<PerspectiveRetirementId>,
}

impl MetadataPublicationReceipt {
    /// Stable idempotency identity supplied for the commit.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared revision assigned to the operation.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Created HistorySpace, for child-creation operations.
    #[must_use]
    pub const fn history_space_id(self) -> Option<HistorySpaceId> {
        self.history_space_id
    }

    /// Created or revised Layer, for Layer operations.
    #[must_use]
    pub const fn layer_id(self) -> Option<LayerId> {
        self.layer_id
    }

    /// Created or revised Perspective, for Perspective catalog operations.
    #[must_use]
    pub const fn perspective_id(self) -> Option<PerspectiveId> {
        self.perspective_id
    }

    /// Created retirement record, for Perspective retirement operations.
    #[must_use]
    pub const fn perspective_retirement_id(self) -> Option<PerspectiveRetirementId> {
        self.perspective_retirement_id
    }
}

/// Durable file-backed manager for project-wide catalog metadata.
///
/// Catalog projections are rebuilt from immutable history records at the
/// requested shared revision. Mutations use the project metadata validator,
/// carry the current security-policy view, and publish through one WAL commit.
pub struct FileProjectMetadataManager<'a> {
    schema: FileSchemaManager<'a>,
}

impl<'a> FileProjectMetadataManager<'a> {
    /// Opens a clean database and binds the manager to its authenticated principal.
    pub fn open(
        layout: crate::DatabaseLayout,
        writer_lock: &'a crate::WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<Self, LayerManagementError> {
        let schema =
            FileSchemaManager::open(layout, writer_lock, principal).map_err(schema_error)?;
        Ok(Self { schema })
    }

    /// Current committed shared revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.schema.revision()
    }

    /// Returns the complete validated catalog projection at Current, Historical, or Explicit.
    pub fn snapshot_at(
        &self,
        mode: SchemaMode,
        recorded_as_of: Revision,
    ) -> Result<ProjectMetadataSnapshot, LayerManagementError> {
        self.authorize(Capability::HistorySpaceRead, PolicyTarget::default())?;
        self.authorize(Capability::LayerRead, PolicyTarget::default())?;
        let requested = requested_revision(mode, recorded_as_of, self.revision())?;
        let schema = self
            .schema
            .schema_at(mode, recorded_as_of)
            .map_err(schema_error)?;
        self.snapshot_for_revision(requested, schema)
    }

    /// Returns only the authorized Perspective catalog at a shared revision.
    pub fn perspectives_at(
        &self,
        mode: SchemaMode,
        recorded_as_of: Revision,
    ) -> Result<PerspectiveCatalogSnapshot, LayerManagementError> {
        self.authorize(Capability::PerspectiveRead, PolicyTarget::default())?;
        let requested = requested_revision(mode, recorded_as_of, self.revision())?;
        let schema = self
            .schema
            .schema_at(mode, recorded_as_of)
            .map_err(schema_error)?;
        Ok(self
            .snapshot_for_revision(requested, schema)?
            .perspectives()
            .clone())
    }

    /// Validates an explicit active Perspective selection under current use/read policy.
    pub fn validate_perspective_use(
        &self,
        perspective_id: PerspectiveId,
    ) -> Result<PerspectiveDefinitionRevision, LayerManagementError> {
        self.authorize(Capability::PerspectiveUse, PolicyTarget::default())?;
        self.authorize(Capability::PerspectiveRead, PolicyTarget::default())?;
        let current = self.current_snapshot()?;
        if current.perspectives().is_retired(perspective_id) {
            return Err(LayerManagementError::InvalidCandidate(
                "the selected Perspective is unavailable",
            ));
        }
        current
            .perspectives()
            .latest_definition(perspective_id)
            .cloned()
            .ok_or(LayerManagementError::InvalidCandidate(
                "the selected Perspective is unavailable",
            ))
    }

    /// Creates the first immutable definition revision for a host-generated Perspective ID.
    pub fn create_perspective(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        perspective_id: PerspectiveId,
        display_name: Option<String>,
        description: Option<String>,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        self.authorize(Capability::PerspectiveCreate, PolicyTarget::default())?;
        validate_perspective_text(&display_name, &description)?;
        if expected_base != self.revision() {
            return Err(LayerManagementError::Conflict);
        }
        let base = self.current_snapshot()?;
        if base
            .perspectives()
            .latest_definition(perspective_id)
            .is_some()
        {
            return Err(LayerManagementError::InvalidCandidate(
                "the generated Perspective identity is unavailable",
            ));
        }
        let target_revision = self.next_revision()?;
        let definition = PerspectiveDefinitionRevision::new(
            perspective_id,
            display_name,
            description,
            target_revision,
        )
        .map_err(|_| LayerManagementError::InvalidCandidate("Perspective metadata is invalid"))?;
        let mut definitions = base.perspectives().definitions().to_vec();
        definitions.push(definition.clone());
        let perspectives = base
            .perspectives()
            .revise(
                target_revision,
                definitions,
                base.perspectives().retirements().to_vec(),
            )
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let schema = self.schema_at_target(target_revision)?;
        let candidate = candidate_from_parts(
            &base,
            base.history_spaces().clone(),
            schema,
            target_revision,
            Some(perspectives),
        )?;
        self.publish(
            expected_base,
            operation_id,
            &base,
            candidate,
            vec![Record::PerspectiveDefinitionRevision(definition)],
            MetadataPublicationIdentity {
                perspective_id: Some(perspective_id),
                ..MetadataPublicationIdentity::default()
            },
        )
    }

    /// Appends optional display metadata under the same immutable Perspective ID.
    pub fn update_perspective(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        perspective_id: PerspectiveId,
        display_name: Option<String>,
        description: Option<String>,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        self.authorize(Capability::PerspectiveUpdate, PolicyTarget::default())?;
        validate_perspective_text(&display_name, &description)?;
        if expected_base != self.revision() {
            return Err(LayerManagementError::Conflict);
        }
        let base = self.current_snapshot()?;
        if base.perspectives().is_retired(perspective_id)
            || base
                .perspectives()
                .latest_definition(perspective_id)
                .is_none()
        {
            return Err(LayerManagementError::InvalidCandidate(
                "the selected Perspective is unavailable",
            ));
        }
        let target_revision = self.next_revision()?;
        let definition = PerspectiveDefinitionRevision::new(
            perspective_id,
            display_name,
            description,
            target_revision,
        )
        .map_err(|_| LayerManagementError::InvalidCandidate("Perspective metadata is invalid"))?;
        let mut definitions = base.perspectives().definitions().to_vec();
        definitions.push(definition.clone());
        let perspectives = base
            .perspectives()
            .revise(
                target_revision,
                definitions,
                base.perspectives().retirements().to_vec(),
            )
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let schema = self.schema_at_target(target_revision)?;
        let candidate = candidate_from_parts(
            &base,
            base.history_spaces().clone(),
            schema,
            target_revision,
            Some(perspectives),
        )?;
        self.publish(
            expected_base,
            operation_id,
            &base,
            candidate,
            vec![Record::PerspectiveDefinitionRevision(definition)],
            MetadataPublicationIdentity {
                perspective_id: Some(perspective_id),
                ..MetadataPublicationIdentity::default()
            },
        )
    }

    /// Appends one irreversible terminal retirement for an active Perspective.
    pub fn retire_perspective(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retirement_id: PerspectiveRetirementId,
        perspective_id: PerspectiveId,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        self.authorize(Capability::PerspectiveRead, PolicyTarget::default())?;
        self.authorize(
            Capability::PerspectiveRetire,
            PolicyTarget::new(
                None,
                None,
                Some(RecordRef::PerspectiveRetirement(retirement_id)),
                None,
                None,
            ),
        )?;
        if expected_base != self.revision() {
            return Err(LayerManagementError::Conflict);
        }
        let base = self.current_snapshot()?;
        if base.perspectives().is_retired(perspective_id)
            || base
                .perspectives()
                .latest_definition(perspective_id)
                .is_none()
        {
            return Err(LayerManagementError::InvalidCandidate(
                "the selected Perspective is unavailable",
            ));
        }
        let target_revision = self.next_revision()?;
        let retirement = PerspectiveRetirement::new(retirement_id, perspective_id, target_revision);
        let mut retirements = base.perspectives().retirements().to_vec();
        retirements.push(retirement);
        let perspectives = base
            .perspectives()
            .revise(
                target_revision,
                base.perspectives().definitions().to_vec(),
                retirements,
            )
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let schema = self.schema_at_target(target_revision)?;
        let candidate = candidate_from_parts(
            &base,
            base.history_spaces().clone(),
            schema,
            target_revision,
            Some(perspectives),
        )?;
        self.publish(
            expected_base,
            operation_id,
            &base,
            candidate,
            vec![Record::PerspectiveRetirement(retirement)],
            MetadataPublicationIdentity {
                perspective_id: Some(perspective_id),
                perspective_retirement_id: Some(retirement_id),
                ..MetadataPublicationIdentity::default()
            },
        )
    }

    /// Creates one immutable child with the caller-selected parent cutoff.
    pub fn create_child(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        history_space_id: HistorySpaceId,
        parent_history_space_id: HistorySpaceId,
        base_revision: Revision,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        let parent_target =
            PolicyTarget::new(Some(parent_history_space_id), None, None, None, None);
        self.authorize(Capability::HistorySpaceRead, parent_target)?;
        self.authorize(Capability::HistorySpaceCreate, parent_target)?;
        if expected_base != self.revision() {
            return Err(LayerManagementError::Conflict);
        }
        let base = self.current_snapshot()?;
        if base.history_spaces().definition(history_space_id).is_some() {
            return Err(LayerManagementError::InvalidCandidate(
                "the new branch identity already exists",
            ));
        }
        if base
            .history_spaces()
            .definition(parent_history_space_id)
            .is_none()
        {
            return Err(LayerManagementError::InvalidCandidate(
                "the selected parent branch is unavailable",
            ));
        }
        let definition = HistorySpaceDefinition::new(
            history_space_id,
            Some(parent_history_space_id),
            base_revision,
        )
        .map_err(|_| {
            LayerManagementError::InvalidCandidate(
                "the branch cutoff must be at or after its parent's cutoff",
            )
        })?;

        let target_revision = self.next_revision()?;
        let mut definitions = base.history_spaces().definitions().to_vec();
        definitions.push(definition);
        let spaces = HistorySpaceCatalog::new(definitions)
            .map_err(|_| LayerManagementError::InvalidCandidate("branch ancestry is invalid"))?;
        let mut schema_history = self.schema_history()?;
        schema_history
            .advance_to(target_revision)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let schema = schema_history
            .schema_at(SchemaMode::Current, target_revision)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let candidate = candidate_from_parts(&base, spaces, schema, target_revision, None)?;
        let records = vec![Record::HistorySpaceDefinition(definition)];
        self.publish(
            expected_base,
            operation_id,
            &base,
            candidate,
            records,
            MetadataPublicationIdentity {
                history_space_id: Some(history_space_id),
                ..MetadataPublicationIdentity::default()
            },
        )
    }

    /// Creates one active overlay Layer at a caller-selected precedence rank.
    pub fn create_layer(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        layer_id: LayerId,
        symbol: Symbol,
        description: Option<String>,
        precedence_rank: i32,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        self.authorize(Capability::LayerRead, PolicyTarget::default())?;
        if expected_base != self.revision() {
            return Err(LayerManagementError::Conflict);
        }
        let base = self.current_snapshot()?;
        let target_revision = self.next_revision()?;
        let schema_revision = SchemaRevision::from_published_revision(target_revision);
        let layer = LayerDefinition::new(
            layer_id,
            symbol,
            description,
            precedence_rank,
            Lifecycle::Active,
            schema_revision,
        );
        let mut definitions = base.layers().definitions().to_vec();
        definitions.push(layer.clone());
        let layers = base
            .layers()
            .revise(schema_revision, definitions, base.layers().base_layer_id())
            .map_err(|_| {
                LayerManagementError::InvalidCandidate(
                    "the overlay must remain above the unique active base Layer",
                )
            })?;
        let layer_snapshot = layers.clone();
        let mut schema_history = self.schema_history()?;
        schema_history
            .publish(
                target_revision,
                vec![
                    SchemaDefinition::Layer(layer.clone()),
                    SchemaDefinition::LayerSnapshot(layer_snapshot),
                ],
            )
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let schema = schema_history
            .schema_at(SchemaMode::Current, target_revision)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let candidate = candidate_from_parts(
            &base,
            base.history_spaces().clone(),
            schema,
            target_revision,
            None,
        )?;
        let records = vec![
            Record::LayerDefinition(layer),
            Record::LayerSchemaSnapshot(layers),
        ];
        self.publish(
            expected_base,
            operation_id,
            &base,
            candidate,
            records,
            MetadataPublicationIdentity {
                layer_id: Some(layer_id),
                ..MetadataPublicationIdentity::default()
            },
        )
    }

    /// Revises one Layer and, optionally, changes the base designation atomically.
    ///
    /// A base switch exchanges the old and new base ranks so that the selected
    /// active Layer remains the unique lowest active rank. Other Layer ranks do
    /// not move. A base Layer cannot be deprecated or retired until another
    /// active base has been selected in the same operation.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0007")]
    pub fn revise_layer(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        layer_id: LayerId,
        description: Option<String>,
        precedence_rank: i32,
        lifecycle: Lifecycle,
        requested_base_layer_id: LayerId,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        self.authorize(Capability::LayerRead, PolicyTarget::default())?;
        if expected_base != self.revision() {
            return Err(LayerManagementError::Conflict);
        }
        let base = self.current_snapshot()?;
        let old_base_id = base.layers().base_layer_id();
        let prior_selected = base
            .layers()
            .definition(layer_id)
            .ok_or(LayerManagementError::UnknownLayer)?;
        let prior_new_base = base
            .layers()
            .definition(requested_base_layer_id)
            .ok_or(LayerManagementError::UnknownLayer)?;
        let requested_rank =
            if requested_base_layer_id != old_base_id && layer_id == requested_base_layer_id {
                prior_new_base.precedence_rank()
            } else {
                precedence_rank
            };
        if requested_base_layer_id == old_base_id
            && prior_selected_state_is_unchanged(
                prior_selected,
                &description,
                requested_rank,
                lifecycle,
            )
        {
            return Err(LayerManagementError::InvalidCandidate(
                "there are no Layer changes to publish",
            ));
        }
        let target_revision = self.next_revision()?;
        let schema_revision = SchemaRevision::from_published_revision(target_revision);
        let mut proposed = base.layers().definitions().to_vec();

        let selected = proposed
            .iter_mut()
            .find(|definition| definition.layer_id() == layer_id)
            .ok_or(LayerManagementError::UnknownLayer)?;
        if prior_selected.description() != description.as_deref()
            || prior_selected.precedence_rank() != requested_rank
            || prior_selected.lifecycle() != lifecycle
        {
            *selected = selected
                .revise_state(description, requested_rank, lifecycle, schema_revision)
                .map_err(|_| LayerManagementError::InvalidCandidate("Layer lifecycle regressed"))?;
        }

        if requested_base_layer_id != old_base_id {
            let old_base_rank = base
                .layers()
                .definition(old_base_id)
                .ok_or(LayerManagementError::UnknownLayer)?
                .precedence_rank();
            let new_base_rank = prior_new_base.precedence_rank();
            for definition in &mut proposed {
                let desired_rank = if definition.layer_id() == requested_base_layer_id {
                    Some(old_base_rank)
                } else if definition.layer_id() == old_base_id {
                    Some(new_base_rank)
                } else {
                    None
                };
                if let Some(desired_rank) = desired_rank {
                    if definition.precedence_rank() != desired_rank {
                        let current = base
                            .layers()
                            .definition(definition.layer_id())
                            .ok_or(LayerManagementError::UnknownLayer)?;
                        *definition = current
                            .revise_state(
                                definition.description().map(String::from),
                                desired_rank,
                                definition.lifecycle(),
                                schema_revision,
                            )
                            .map_err(|_| {
                                LayerManagementError::InvalidCandidate("Layer state is invalid")
                            })?;
                    }
                }
            }
        }

        if proposed
            .iter()
            .all(|definition| base.layers().definition(definition.layer_id()) == Some(definition))
            && requested_base_layer_id == old_base_id
        {
            return Err(LayerManagementError::InvalidCandidate(
                "there are no Layer changes to publish",
            ));
        }

        let layers = base
            .layers()
            .revise(schema_revision, proposed, requested_base_layer_id)
            .map_err(|_| {
                LayerManagementError::InvalidCandidate(
                    "the selected base must be active and have a unique lowest precedence rank",
                )
            })?;
        let changed_definitions = layers
            .definitions()
            .iter()
            .filter(|definition| {
                base.layers().definition(definition.layer_id()) != Some(*definition)
            })
            .cloned()
            .map(SchemaDefinition::Layer)
            .chain(std::iter::once(SchemaDefinition::LayerSnapshot(
                layers.clone(),
            )))
            .collect::<Vec<_>>();
        let records = changed_definitions
            .iter()
            .map(|definition| match definition {
                SchemaDefinition::Layer(layer) => Record::LayerDefinition(layer.clone()),
                SchemaDefinition::LayerSnapshot(snapshot) => {
                    Record::LayerSchemaSnapshot(snapshot.clone())
                }
                _ => unreachable!("only Layer records were staged"),
            })
            .collect::<Vec<_>>();
        let mut schema_history = self.schema_history()?;
        schema_history
            .publish(target_revision, changed_definitions)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let schema = schema_history
            .schema_at(SchemaMode::Current, target_revision)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let candidate = candidate_from_parts(
            &base,
            base.history_spaces().clone(),
            schema,
            target_revision,
            None,
        )?;
        self.publish(
            expected_base,
            operation_id,
            &base,
            candidate,
            records,
            MetadataPublicationIdentity {
                layer_id: Some(layer_id),
                ..MetadataPublicationIdentity::default()
            },
        )
    }

    fn schema_at_target(
        &self,
        target_revision: Revision,
    ) -> Result<SchemaSnapshot, LayerManagementError> {
        let mut schema_history = self.schema_history()?;
        schema_history
            .advance_to(target_revision)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        schema_history
            .schema_at(SchemaMode::Current, target_revision)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))
    }

    fn current_snapshot(&self) -> Result<ProjectMetadataSnapshot, LayerManagementError> {
        let revision = self.revision();
        let schema = self
            .schema
            .schema_at(SchemaMode::Current, revision)
            .map_err(schema_error)?;
        self.snapshot_for_revision(revision, schema)
    }

    fn snapshot_for_revision(
        &self,
        revision: Revision,
        schema: SchemaSnapshot,
    ) -> Result<ProjectMetadataSnapshot, LayerManagementError> {
        if revision > self.revision() {
            return Err(LayerManagementError::RevisionNotPublished);
        }
        let mut spaces = Vec::new();
        let mut entities = Vec::new();
        let mut entity_retirements = Vec::new();
        let mut perspectives = Vec::<PerspectiveDefinitionRevision>::new();
        let mut perspective_retirements = Vec::<PerspectiveRetirement>::new();
        let store = HistorySegmentStore::new(self.schema.layout.clone());
        for reference in self
            .schema
            .manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        {
            if reference.through_revision() > self.schema.manifest.revision() {
                return Err(LayerManagementError::Storage(
                    "history reference exceeds the manifest revision".to_owned(),
                ));
            }
            let segment = store.read_segment(reference.id()).map_err(storage_error)?;
            if segment.content_digest() != reference.content_digest() {
                return Err(LayerManagementError::Storage(
                    "history segment digest differs from its manifest".to_owned(),
                ));
            }
            for decoded in segment.records() {
                match decoded.record() {
                    Record::HistorySpaceDefinition(definition)
                        if reference.through_revision() <= revision =>
                    {
                        spaces.push(*definition);
                    }
                    Record::Entity(entity) if entity.created_revision() <= revision => {
                        validate_record_revision(
                            entity.created_revision(),
                            *reference,
                            self.revision(),
                        )?;
                        entities.push(*entity);
                    }
                    Record::EntityRetirement(retirement)
                        if retirement.created_revision() <= revision =>
                    {
                        validate_record_revision(
                            retirement.created_revision(),
                            *reference,
                            self.revision(),
                        )?;
                        entity_retirements.push(*retirement);
                    }
                    Record::PerspectiveDefinitionRevision(definition)
                        if definition.recorded_revision() <= revision =>
                    {
                        validate_record_revision(
                            definition.recorded_revision(),
                            *reference,
                            self.revision(),
                        )?;
                        perspectives.push(definition.clone());
                    }
                    Record::PerspectiveRetirement(retirement)
                        if retirement.created_revision() <= revision =>
                    {
                        validate_record_revision(
                            retirement.created_revision(),
                            *reference,
                            self.revision(),
                        )?;
                        perspective_retirements.push(*retirement);
                    }
                    _ => {}
                }
            }
        }

        let history_spaces = HistorySpaceCatalog::new(spaces)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let layer_snapshot = schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::LayerSnapshot(snapshot) => Some(snapshot),
                _ => None,
            })
            .ok_or_else(|| {
                LayerManagementError::Storage(
                    "project has no published base Layer snapshot".to_owned(),
                )
            })?;
        let layer_definitions = schema
            .definitions()
            .iter()
            .filter_map(|definition| match definition {
                SchemaDefinition::Layer(layer) => Some(layer.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let layers = LayerSchemaSnapshot::new(
            SchemaRevision::from_published_revision(revision),
            layer_definitions,
            layer_snapshot.base_layer_id(),
        )
        .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let entities = EntityCatalogSnapshot::new(revision, entities, entity_retirements)
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let perspectives =
            PerspectiveCatalogSnapshot::new(revision, perspectives, perspective_retirements)
                .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        ProjectMetadataSnapshot::new(
            revision,
            history_spaces,
            layers,
            entities,
            perspectives,
            schema,
        )
        .map_err(metadata_error)
    }

    fn schema_history(
        &self,
    ) -> Result<worlddb_core::SchemaHistoryReferenceModel, LayerManagementError> {
        load_schema_history(&self.schema.layout, &self.schema.manifest).map_err(schema_error)
    }

    fn next_revision(&self) -> Result<Revision, LayerManagementError> {
        self.schema.next_revision().map_err(schema_error)
    }

    fn authorize(
        &self,
        capability: Capability,
        target: PolicyTarget,
    ) -> Result<(), LayerManagementError> {
        let policy = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        if policy
            .snapshot()
            .authorize(self.schema.principal, capability, target)
            == AuthorizationDecision::Allow
        {
            Ok(())
        } else {
            Err(LayerManagementError::Unauthorized(capability))
        }
    }

    fn publish(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        base: &ProjectMetadataSnapshot,
        candidate: ProjectMetadataCandidate,
        records: Vec<Record>,
        publication: MetadataPublicationIdentity,
    ) -> Result<MetadataPublicationReceipt, LayerManagementError> {
        if records.is_empty()
            || expected_base != self.revision()
            || base.revision() != expected_base
        {
            return Err(LayerManagementError::Conflict);
        }
        let wal = WalPrepareLog::new(&self.schema.layout);
        if wal
            .commit_head(self.schema.writer_lock)
            .map_err(storage_error)?
            .revision()
            != expected_base
        {
            return Err(LayerManagementError::Conflict);
        }
        if wal
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(LayerManagementError::OperationAlreadyUsed);
        }
        let policy_version = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let staged_records = records.as_slice();
        let _ = worlddb_core::validate_project_metadata_transaction(
            base,
            candidate,
            staged_records,
            policy_version.snapshot(),
            self.schema.principal,
        )
        .map_err(metadata_error)?;
        let target_revision = expected_base
            .next_commit()
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;

        let sequence = next_audit_sequence(&wal, self.schema.writer_lock).map_err(schema_error)?;
        let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(
            policy_version
                .snapshot()
                .effective_capability_fingerprint(self.schema.principal, PolicyTarget::default())
                .to_vec(),
        ))
        .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
        let action = AuditAction::SchemaManagement;
        let object_class = if publication.layer_id.is_some() {
            AuditObjectClass::SchemaDefinition
        } else {
            AuditObjectClass::Database
        };
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id:
                    worlddb_core::storage_internal::generate_schema_management_audit_record_id()
                        .map_err(storage_error)?,
                sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_schema_management_audit_operation_id()
                        .map_err(storage_error)?,
            },
            AuditRecordDetails {
                actor: self.schema.principal,
                action,
                object_class,
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
            .stage_segment(self.schema.writer_lock, &records)
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
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
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
                return Err(LayerManagementError::Storage(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; history cleanup failed: {cleanup_error}")
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
        match wal.commit_audited_manifest_snapshot(
            self.schema.writer_lock,
            operation_id,
            next_references,
            &staged,
            &audit_record,
        ) {
            Ok(receipt) if receipt.revision() == target_revision => {}
            Ok(_) => return Err(LayerManagementError::UnknownCommit(operation_id)),
            Err(commit_error) => {
                let recovery = RecoveryManager::new(self.schema.layout.clone())
                    .recover(self.schema.writer_lock)
                    .map_err(|error| {
                        LayerManagementError::UnknownCommitWithDiagnostic(
                            operation_id,
                            format!("{commit_error}; recovery: {error}"),
                        )
                    })?;
                let status = wal
                    .operation_status(self.schema.writer_lock, operation_id)
                    .map_err(storage_error)?;
                if !recovery.report().is_clean()
                    || !matches!(status, WalOperationStatus::Committed(receipt) if receipt.revision() == target_revision)
                {
                    if status == WalOperationStatus::NotCommitted {
                        cleanup_staged_schema(
                            &history_store,
                            history_reference,
                            &security_store,
                            security_reference,
                        );
                        return Err(LayerManagementError::Storage(commit_error.to_string()));
                    }
                    return Err(LayerManagementError::UnknownCommitWithDiagnostic(
                        operation_id,
                        commit_error.to_string(),
                    ));
                }
            }
        }
        let recovery = RecoveryManager::new(self.schema.layout.clone())
            .recover(self.schema.writer_lock)
            .map_err(|error| {
                LayerManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
            })?;
        if !recovery.report().is_clean() {
            return Err(LayerManagementError::UnknownCommit(operation_id));
        }
        let reopened = FileSchemaManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| {
            LayerManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
        })?;
        if reopened.revision() != target_revision {
            return Err(LayerManagementError::UnknownCommit(operation_id));
        }
        self.schema = reopened;
        Ok(MetadataPublicationReceipt {
            operation_id,
            revision: target_revision,
            history_space_id: publication.history_space_id,
            layer_id: publication.layer_id,
            perspective_id: publication.perspective_id,
            perspective_retirement_id: publication.perspective_retirement_id,
        })
    }
}

fn candidate_from_parts(
    base: &ProjectMetadataSnapshot,
    history_spaces: HistorySpaceCatalog,
    schema: SchemaSnapshot,
    revision: Revision,
    perspectives_override: Option<PerspectiveCatalogSnapshot>,
) -> Result<ProjectMetadataCandidate, LayerManagementError> {
    let embedded_layers = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::LayerSnapshot(snapshot) => Some(snapshot),
            _ => None,
        })
        .ok_or_else(|| {
            LayerManagementError::Storage("base Layer snapshot is missing".to_owned())
        })?;
    let layer_definitions = schema
        .definitions()
        .iter()
        .filter_map(|definition| match definition {
            SchemaDefinition::Layer(layer) => Some(layer.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let layers = LayerSchemaSnapshot::new(
        SchemaRevision::from_published_revision(revision),
        layer_definitions,
        embedded_layers.base_layer_id(),
    )
    .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
    let entities = base
        .entities()
        .revise(
            revision,
            base.entities().entities().to_vec(),
            base.entities().retirements().to_vec(),
        )
        .map_err(|error| LayerManagementError::Storage(error.to_string()))?;
    let perspectives = match perspectives_override {
        Some(perspectives) => perspectives,
        None => base
            .perspectives()
            .revise(
                revision,
                base.perspectives().definitions().to_vec(),
                base.perspectives().retirements().to_vec(),
            )
            .map_err(|error| LayerManagementError::Storage(error.to_string()))?,
    };
    ProjectMetadataCandidate::new(
        revision,
        history_spaces,
        layers,
        entities,
        perspectives,
        schema,
    )
    .map_err(metadata_error)
}

#[derive(Clone, Copy, Debug, Default)]
struct MetadataPublicationIdentity {
    history_space_id: Option<HistorySpaceId>,
    layer_id: Option<LayerId>,
    perspective_id: Option<PerspectiveId>,
    perspective_retirement_id: Option<PerspectiveRetirementId>,
}

fn validate_perspective_text(
    display_name: &Option<String>,
    description: &Option<String>,
) -> Result<(), LayerManagementError> {
    let maximum = worlddb_core::DecoderLimits::DEFAULT.max_string_or_bytes;
    if display_name
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > maximum)
        || description
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > maximum)
    {
        return Err(LayerManagementError::InvalidCandidate(
            "Perspective metadata is empty or exceeds the shared field limit",
        ));
    }
    Ok(())
}

fn requested_revision(
    mode: SchemaMode,
    recorded_as_of: Revision,
    latest: Revision,
) -> Result<Revision, LayerManagementError> {
    let requested = match mode {
        SchemaMode::Current => latest,
        SchemaMode::Historical => recorded_as_of,
        SchemaMode::Explicit(revision) => revision.revision(),
    };
    if requested > latest {
        return Err(LayerManagementError::RevisionNotPublished);
    }
    Ok(requested)
}

fn validate_record_revision(
    created: Revision,
    reference: ManifestSegmentReference,
    latest: Revision,
) -> Result<(), LayerManagementError> {
    if created > reference.through_revision() || created > latest {
        return Err(LayerManagementError::Storage(
            "catalog record exceeds its committed history segment".to_owned(),
        ));
    }
    Ok(())
}

fn prior_selected_state_is_unchanged(
    prior: &LayerDefinition,
    description: &Option<String>,
    precedence_rank: i32,
    lifecycle: Lifecycle,
) -> bool {
    prior.description() == description.as_deref()
        && prior.precedence_rank() == precedence_rank
        && prior.lifecycle() == lifecycle
}

fn schema_error(error: SchemaManagementError) -> LayerManagementError {
    LayerManagementError::Storage(error.to_string())
}

fn storage_error(error: impl fmt::Display) -> LayerManagementError {
    LayerManagementError::Storage(error.to_string())
}

fn metadata_error(error: ProjectMetadataValidationError) -> LayerManagementError {
    LayerManagementError::InvalidMetadata(error)
}

/// Failure while reading or publishing HistorySpace and Layer metadata.
#[derive(Debug)]
pub enum LayerManagementError {
    /// Underlying validated schema, file, or WAL boundary failed.
    Storage(String),
    /// Metadata invariants or exact staged-record matching failed.
    InvalidMetadata(ProjectMetadataValidationError),
    /// A typed user candidate violates a simple catalog rule.
    InvalidCandidate(&'static str),
    /// Current policy denied a capability required by the operation.
    Unauthorized(Capability),
    /// Expected or live WAL head changed while editing.
    Conflict,
    /// The requested shared revision has not been published.
    RevisionNotPublished,
    /// A Layer identity is absent from the selected current snapshot.
    UnknownLayer,
    /// An OperationId is already committed or bound to another payload.
    OperationAlreadyUsed,
    /// WAL publication was committed or may have committed, but its receipt is unknown.
    UnknownCommit(OperationId),
    /// WAL publication outcome could not be reconciled after recovery.
    UnknownCommitWithDiagnostic(OperationId, String),
}

impl fmt::Display for LayerManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(message) => formatter.write_str(message),
            Self::InvalidMetadata(error) => {
                write!(formatter, "metadata validation failed: {error}")
            }
            Self::InvalidCandidate(message) => formatter.write_str(message),
            Self::Unauthorized(capability) => {
                write!(formatter, "permission denied: {capability:?}")
            }
            Self::Conflict => formatter.write_str("project changed; refresh and try again"),
            Self::RevisionNotPublished => {
                formatter.write_str("the selected revision is not published")
            }
            Self::UnknownLayer => formatter.write_str("the selected Layer is unavailable"),
            Self::OperationAlreadyUsed => {
                formatter.write_str("operation identity was already used")
            }
            Self::UnknownCommit(operation_id) => {
                write!(formatter, "commit outcome is unknown for {operation_id}")
            }
            Self::UnknownCommitWithDiagnostic(operation_id, diagnostic) => write!(
                formatter,
                "commit outcome is unknown for {operation_id}: {diagnostic}"
            ),
        }
    }
}

impl std::error::Error for LayerManagementError {}
