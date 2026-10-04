//! Atomic project-catalog and schema metadata validation.

use std::fmt;

use crate::catalog::{
    EntityCatalogError, EntityCatalogSnapshot, HistorySpaceCatalog, PerspectiveCatalogError,
    PerspectiveCatalogSnapshot,
};
use crate::ids::{EntityTypeId, HistorySpaceId, LayerId, PredicateId, Revision};
use crate::layers::{LayerSchemaError, LayerSchemaSnapshot};
use crate::record_refs::RecordRef;
use crate::schema::Lifecycle;
use crate::schema_history::{SchemaDefinition, SchemaHistoryError, SchemaSnapshot};
use crate::security::{AuthorizationDecision, Capability, PolicyTarget, SecurityPolicySnapshot};
use crate::wire_records::{Record, RecordCodecError, encode_record};

/// One mutually consistent project metadata view at a shared data revision.
#[derive(Clone, Debug)]
pub struct ProjectMetadataSnapshot {
    revision: Revision,
    history_spaces: HistorySpaceCatalog,
    layers: LayerSchemaSnapshot,
    entities: EntityCatalogSnapshot,
    perspectives: PerspectiveCatalogSnapshot,
    schema: SchemaSnapshot,
}

impl ProjectMetadataSnapshot {
    /// Validates the shared revision and explicit layer state embedded in schema.
    pub fn new(
        revision: Revision,
        history_spaces: HistorySpaceCatalog,
        layers: LayerSchemaSnapshot,
        entities: EntityCatalogSnapshot,
        perspectives: PerspectiveCatalogSnapshot,
        schema: SchemaSnapshot,
    ) -> Result<Self, ProjectMetadataValidationError> {
        validate_shared_revision(revision, &layers, &entities, &perspectives, &schema)?;
        validate_history_space_cutoffs(&history_spaces, revision)?;
        validate_schema_catalog_consistency(&layers, &entities, &schema)?;
        Ok(Self {
            revision,
            history_spaces,
            layers,
            entities,
            perspectives,
            schema,
        })
    }

    /// Shared data revision represented by every metadata projection.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Full immutable HistorySpace forest.
    #[must_use]
    pub const fn history_spaces(&self) -> &HistorySpaceCatalog {
        &self.history_spaces
    }

    /// Full immutable layer schema state and base designation.
    #[must_use]
    pub const fn layers(&self) -> &LayerSchemaSnapshot {
        &self.layers
    }

    /// Full immutable Entity identities and retirement history.
    #[must_use]
    pub const fn entities(&self) -> &EntityCatalogSnapshot {
        &self.entities
    }

    /// Full immutable Perspective metadata and retirement history.
    #[must_use]
    pub const fn perspectives(&self) -> &PerspectiveCatalogSnapshot {
        &self.perspectives
    }

    /// Effective schema snapshot at this same shared revision.
    #[must_use]
    pub const fn schema(&self) -> &SchemaSnapshot {
        &self.schema
    }
}

/// One proposed complete project metadata post-state.
#[derive(Clone, Debug)]
pub struct ProjectMetadataCandidate {
    snapshot: ProjectMetadataSnapshot,
}

impl ProjectMetadataCandidate {
    /// Validates internal consistency before the candidate reaches authorization.
    pub fn new(
        revision: Revision,
        history_spaces: HistorySpaceCatalog,
        layers: LayerSchemaSnapshot,
        entities: EntityCatalogSnapshot,
        perspectives: PerspectiveCatalogSnapshot,
        schema: SchemaSnapshot,
    ) -> Result<Self, ProjectMetadataValidationError> {
        Ok(Self {
            snapshot: ProjectMetadataSnapshot::new(
                revision,
                history_spaces,
                layers,
                entities,
                perspectives,
                schema,
            )?,
        })
    }

    /// Candidate shared revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.snapshot.revision
    }
}

/// Rechecks permissions and the complete post-state before metadata Records publish.
///
/// `staged_records` is the metadata subset from the complete transaction batch;
/// other record families may be validated by their own composed boundary in the
/// same transaction callback. Every metadata delta in the candidate must have
/// exactly one matching typed Record in this slice.
pub fn validate_project_metadata_transaction(
    base: &ProjectMetadataSnapshot,
    candidate: ProjectMetadataCandidate,
    staged_records: &[Record],
    policy: &SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
) -> Result<ProjectMetadataSnapshot, ProjectMetadataValidationError> {
    authorize_metadata_records(base, staged_records, policy, principal)?;
    validate_post_state(base, &candidate.snapshot)?;
    validate_exact_metadata_batch(base, &candidate.snapshot, staged_records)?;
    Ok(candidate.snapshot)
}

fn validate_shared_revision(
    revision: Revision,
    layers: &LayerSchemaSnapshot,
    entities: &EntityCatalogSnapshot,
    perspectives: &PerspectiveCatalogSnapshot,
    schema: &SchemaSnapshot,
) -> Result<(), ProjectMetadataValidationError> {
    if layers.revision().revision() != revision
        || entities.revision() != revision
        || perspectives.revision() != revision
        || schema.schema_revision().revision() != revision
    {
        return Err(ProjectMetadataValidationError::SharedRevisionMismatch);
    }
    Ok(())
}

fn validate_history_space_cutoffs(
    catalog: &HistorySpaceCatalog,
    maximum_base: Revision,
) -> Result<(), ProjectMetadataValidationError> {
    for definition in catalog.definitions() {
        if definition.base_revision() > maximum_base {
            return Err(
                ProjectMetadataValidationError::HistorySpaceBaseNotPublished {
                    history_space_id: definition.history_space_id(),
                    base_revision: definition.base_revision(),
                    published: maximum_base,
                },
            );
        }
        if let Some(parent_id) = definition.parent_history_space_id() {
            let parent = catalog
                .definition(parent_id)
                .ok_or(ProjectMetadataValidationError::HistorySpaceHistoryChanged)?;
            if definition.base_revision() < parent.base_revision() {
                return Err(
                    ProjectMetadataValidationError::HistorySpaceCutoffBeforeParent {
                        history_space_id: definition.history_space_id(),
                        base_revision: definition.base_revision(),
                        parent_base_revision: parent.base_revision(),
                    },
                );
            }
        }
    }
    Ok(())
}

fn validate_post_state(
    base: &ProjectMetadataSnapshot,
    candidate: &ProjectMetadataSnapshot,
) -> Result<(), ProjectMetadataValidationError> {
    if candidate.revision <= base.revision {
        return Err(ProjectMetadataValidationError::RevisionNotIncreasing);
    }
    validate_shared_revision(
        candidate.revision,
        &candidate.layers,
        &candidate.entities,
        &candidate.perspectives,
        &candidate.schema,
    )?;

    for previous in base.history_spaces.definitions() {
        if candidate
            .history_spaces
            .definition(previous.history_space_id())
            != Some(*previous)
        {
            return Err(ProjectMetadataValidationError::HistorySpaceHistoryChanged);
        }
    }
    for definition in candidate.history_spaces.definitions() {
        if base
            .history_spaces
            .definition(definition.history_space_id())
            .is_none()
            && definition.base_revision() > base.revision
        {
            return Err(
                ProjectMetadataValidationError::HistorySpaceBaseNotPublished {
                    history_space_id: definition.history_space_id(),
                    base_revision: definition.base_revision(),
                    published: base.revision,
                },
            );
        }
    }
    validate_history_space_cutoffs(&candidate.history_spaces, base.revision)?;

    let revised_entities = base.entities.revise(
        candidate.revision,
        candidate.entities.entities().to_vec(),
        candidate.entities.retirements().to_vec(),
    )?;
    if revised_entities != candidate.entities {
        return Err(ProjectMetadataValidationError::EntityHistoryChanged);
    }
    let revised_perspectives = base.perspectives.revise(
        candidate.revision,
        candidate.perspectives.definitions().to_vec(),
        candidate.perspectives.retirements().to_vec(),
    )?;
    if revised_perspectives != candidate.perspectives {
        return Err(ProjectMetadataValidationError::PerspectiveHistoryChanged);
    }
    let revised_layers = base.layers.revise(
        candidate.layers.revision(),
        candidate.layers.definitions().to_vec(),
        candidate.layers.base_layer_id(),
    )?;
    if revised_layers != candidate.layers {
        return Err(ProjectMetadataValidationError::LayerSchemaMismatch);
    }

    let base_schema = base
        .schema
        .definitions()
        .iter()
        .map(|definition| {
            Ok((
                schema_key(definition),
                encode_schema_definition(definition)?,
            ))
        })
        .collect::<Result<Vec<_>, ProjectMetadataValidationError>>()?;
    let candidate_schema = candidate
        .schema
        .definitions()
        .iter()
        .map(|definition| {
            Ok((
                schema_key(definition),
                encode_schema_definition(definition)?,
            ))
        })
        .collect::<Result<Vec<_>, ProjectMetadataValidationError>>()?;
    for (key, prior_encoding) in &base_schema {
        let Some((_, current_encoding)) = candidate_schema
            .iter()
            .find(|(current_key, _)| current_key == key)
        else {
            return Err(ProjectMetadataValidationError::SchemaDefinitionRemoved);
        };
        if current_encoding != prior_encoding {
            let definition = candidate
                .schema
                .definitions()
                .iter()
                .find(|definition| schema_key(definition) == *key)
                .ok_or(ProjectMetadataValidationError::SchemaDefinitionRemoved)?;
            if schema_definition_revision(definition) != candidate.revision {
                return Err(ProjectMetadataValidationError::SchemaDefinitionRevisionMismatch);
            }
        }
    }
    for definition in candidate.schema.definitions() {
        if !base_schema
            .iter()
            .any(|(key, _)| *key == schema_key(definition))
            && schema_definition_revision(definition) != candidate.revision
        {
            return Err(ProjectMetadataValidationError::SchemaDefinitionRevisionMismatch);
        }
    }
    validate_schema_catalog_consistency(&candidate.layers, &candidate.entities, &candidate.schema)?;
    Ok(())
}

fn validate_schema_catalog_consistency(
    layers: &LayerSchemaSnapshot,
    entities: &EntityCatalogSnapshot,
    schema: &SchemaSnapshot,
) -> Result<(), ProjectMetadataValidationError> {
    let Some(schema_layer_snapshot) =
        schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::LayerSnapshot(snapshot) => Some(snapshot),
                _ => None,
            })
    else {
        return Err(ProjectMetadataValidationError::LayerSchemaMismatch);
    };
    if schema_layer_snapshot.base_layer_id() != layers.base_layer_id()
        || schema_layer_snapshot.definitions() != layers.definitions()
    {
        return Err(ProjectMetadataValidationError::LayerSchemaMismatch);
    }

    let mut schema_layers = schema
        .definitions()
        .iter()
        .filter_map(|definition| match definition {
            SchemaDefinition::Layer(layer) => Some(layer),
            _ => None,
        })
        .collect::<Vec<_>>();
    schema_layers.sort_by_key(|layer| layer.layer_id());
    if schema_layers.len() != layers.definitions().len()
        || schema_layers
            .iter()
            .zip(layers.definitions())
            .any(|(left, right)| *left != right)
    {
        return Err(ProjectMetadataValidationError::LayerSchemaMismatch);
    }

    let entity_types = schema
        .definitions()
        .iter()
        .filter_map(|definition| match definition {
            SchemaDefinition::EntityType(entity_type) => {
                Some((entity_type.entity_type_id(), entity_type.lifecycle()))
            }
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for entity in entities.entities() {
        match entity_types.get(&entity.entity_type_id()) {
            None => {
                return Err(ProjectMetadataValidationError::UnknownEntityType(
                    entity.entity_type_id(),
                ));
            }
            Some(Lifecycle::Retired) if entity.created_revision() == entities.revision() => {
                return Err(
                    ProjectMetadataValidationError::EntityTypeNotActiveAtCreation(
                        entity.entity_type_id(),
                    ),
                );
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn authorize_metadata_records(
    base: &ProjectMetadataSnapshot,
    staged_records: &[Record],
    policy: &SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
) -> Result<(), ProjectMetadataValidationError> {
    for record in staged_records {
        let (capability, target) = match record {
            Record::HistorySpaceDefinition(definition) => (
                Capability::HistorySpaceCreate,
                definition
                    .parent_history_space_id()
                    .map_or_else(PolicyTarget::default, |parent| {
                        PolicyTarget::new(Some(parent), None, None, None, None)
                    }),
            ),
            Record::Entity(_) => (Capability::EntityCreate, PolicyTarget::default()),
            Record::EntityRetirement(retirement) => (
                Capability::EntityRetire,
                PolicyTarget::new(
                    None,
                    None,
                    Some(RecordRef::EntityRetirement(
                        retirement.entity_retirement_id(),
                    )),
                    None,
                    None,
                ),
            ),
            Record::PerspectiveDefinitionRevision(definition) => {
                let capability = if base
                    .perspectives
                    .latest_definition(definition.perspective_id())
                    .is_some()
                {
                    Capability::PerspectiveUpdate
                } else {
                    Capability::PerspectiveCreate
                };
                (capability, PolicyTarget::default())
            }
            Record::PerspectiveRetirement(retirement) => (
                Capability::PerspectiveRetire,
                PolicyTarget::new(
                    None,
                    None,
                    Some(RecordRef::PerspectiveRetirement(
                        retirement.perspective_retirement_id(),
                    )),
                    None,
                    None,
                ),
            ),
            Record::LayerDefinition(definition) => (
                Capability::LayerManage,
                PolicyTarget::new(None, Some(definition.layer_id()), None, None, None),
            ),
            Record::LayerSchemaSnapshot(snapshot) => {
                let old_base = base.layers.base_layer_id();
                let new_base = snapshot.base_layer_id();
                if old_base != new_base {
                    require(
                        policy,
                        principal,
                        Capability::LayerManage,
                        PolicyTarget::new(None, Some(old_base), None, None, None),
                    )?;
                }
                (
                    Capability::LayerManage,
                    PolicyTarget::new(None, Some(new_base), None, None, None),
                )
            }
            Record::EntityTypeDefinition(_)
            | Record::PredicateDefinition(_)
            | Record::EventKindDefinition(_)
            | Record::TimelineDefinition(_)
            | Record::TimeUnitDefinition(_) => (Capability::SchemaManage, PolicyTarget::default()),
            _ => continue,
        };
        require(policy, principal, capability, target)?;
    }
    Ok(())
}

fn validate_exact_metadata_batch(
    base: &ProjectMetadataSnapshot,
    candidate: &ProjectMetadataSnapshot,
    staged_records: &[Record],
) -> Result<(), ProjectMetadataValidationError> {
    let mut expected = Vec::<Vec<u8>>::new();
    for entity in candidate.entities.entities() {
        if base.entities.entity(entity.entity_id()).is_none() {
            expected.push(encode_record(&Record::Entity(*entity))?);
        }
    }
    for retirement in candidate.entities.retirements() {
        if !base.entities.retirements().contains(retirement) {
            expected.push(encode_record(&Record::EntityRetirement(*retirement))?);
        }
    }
    for definition in candidate.perspectives.definitions() {
        if !base.perspectives.definitions().contains(definition) {
            expected.push(encode_record(&Record::PerspectiveDefinitionRevision(
                definition.clone(),
            ))?);
        }
    }
    for retirement in candidate.perspectives.retirements() {
        if !base.perspectives.retirements().contains(retirement) {
            expected.push(encode_record(&Record::PerspectiveRetirement(*retirement))?);
        }
    }
    for definition in candidate.history_spaces.definitions() {
        if base
            .history_spaces
            .definition(definition.history_space_id())
            .is_none()
        {
            expected.push(encode_record(&Record::HistorySpaceDefinition(*definition))?);
        }
    }
    for definition in candidate.layers.definitions() {
        if base.layers.definition(definition.layer_id()) != Some(definition) {
            expected.push(encode_record(&Record::LayerDefinition(definition.clone()))?);
        }
    }

    let base_schema = base
        .schema
        .definitions()
        .iter()
        .map(|definition| {
            Ok((
                schema_key(definition),
                encode_schema_definition(definition)?,
            ))
        })
        .collect::<Result<Vec<_>, ProjectMetadataValidationError>>()?;
    for definition in candidate.schema.definitions() {
        // Layer definitions are already emitted from the authoritative layer
        // catalog above. The schema projection contains the same records.
        if matches!(definition, SchemaDefinition::Layer(_)) {
            continue;
        }
        let encoding = encode_schema_definition(definition)?;
        let changed = base_schema
            .iter()
            .find(|(key, _)| *key == schema_key(definition))
            .is_none_or(|(_, prior)| prior != &encoding);
        if changed {
            expected.push(encoding);
        }
    }

    let mut actual = Vec::new();
    for record in staged_records {
        if is_project_metadata_record(record) {
            actual.push(encode_record(record)?);
        }
    }
    expected.sort();
    actual.sort();
    if expected != actual {
        return Err(ProjectMetadataValidationError::MetadataBatchMismatch);
    }
    Ok(())
}

fn is_project_metadata_record(record: &Record) -> bool {
    matches!(
        record,
        Record::HistorySpaceDefinition(_)
            | Record::Entity(_)
            | Record::EntityRetirement(_)
            | Record::PerspectiveDefinitionRevision(_)
            | Record::PerspectiveRetirement(_)
            | Record::LayerDefinition(_)
            | Record::LayerSchemaSnapshot(_)
            | Record::EntityTypeDefinition(_)
            | Record::PredicateDefinition(_)
            | Record::EventKindDefinition(_)
            | Record::TimelineDefinition(_)
            | Record::TimeUnitDefinition(_)
    )
}

fn schema_key(definition: &SchemaDefinition) -> SchemaKey {
    match definition {
        SchemaDefinition::Layer(value) => SchemaKey::Layer(value.layer_id()),
        SchemaDefinition::LayerSnapshot(_) => SchemaKey::LayerSnapshot,
        SchemaDefinition::EntityType(value) => SchemaKey::EntityType(value.entity_type_id()),
        SchemaDefinition::Predicate(value) => SchemaKey::Predicate(value.predicate_id()),
        SchemaDefinition::EventKind(value) => SchemaKey::EventKind(value.event_kind_id()),
        SchemaDefinition::Timeline(value) => SchemaKey::Timeline(value.timeline_id()),
        SchemaDefinition::TimeUnit(value) => {
            SchemaKey::TimeUnit(value.symbol().as_str().to_owned())
        }
    }
}

fn schema_definition_revision(definition: &SchemaDefinition) -> Revision {
    match definition {
        SchemaDefinition::Layer(value) => value.created_revision().revision(),
        SchemaDefinition::LayerSnapshot(value) => value.revision().revision(),
        SchemaDefinition::EntityType(value) => value.created_revision(),
        SchemaDefinition::Predicate(value) => value.created_revision(),
        SchemaDefinition::EventKind(value) => value.created_revision(),
        SchemaDefinition::Timeline(value) => value.created_revision(),
        SchemaDefinition::TimeUnit(value) => value.created_revision(),
    }
}

fn encode_schema_definition(
    definition: &SchemaDefinition,
) -> Result<Vec<u8>, ProjectMetadataValidationError> {
    let record = match definition {
        SchemaDefinition::Layer(value) => Record::LayerDefinition(value.clone()),
        SchemaDefinition::LayerSnapshot(value) => Record::LayerSchemaSnapshot(value.clone()),
        SchemaDefinition::EntityType(value) => Record::EntityTypeDefinition(value.clone()),
        SchemaDefinition::Predicate(value) => Record::PredicateDefinition(value.clone()),
        SchemaDefinition::EventKind(value) => Record::EventKindDefinition(value.clone()),
        SchemaDefinition::Timeline(value) => Record::TimelineDefinition(value.clone()),
        SchemaDefinition::TimeUnit(value) => Record::TimeUnitDefinition(value.clone()),
    };
    Ok(encode_record(&record)?)
}

fn require(
    policy: &SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
    capability: Capability,
    target: PolicyTarget,
) -> Result<(), ProjectMetadataValidationError> {
    if policy.authorize(principal, capability, target) == AuthorizationDecision::Allow {
        Ok(())
    } else {
        Err(ProjectMetadataValidationError::Denied { capability, target })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SchemaKey {
    Layer(LayerId),
    LayerSnapshot,
    EntityType(EntityTypeId),
    Predicate(PredicateId),
    EventKind(crate::ids::EventKindId),
    Timeline(crate::ids::TimelineId),
    TimeUnit(String),
}

/// Invalid metadata authorization, history preservation, or shared post-state.
#[derive(Debug)]
pub enum ProjectMetadataValidationError {
    /// All project catalogs and schema must share exactly one commit revision.
    SharedRevisionMismatch,
    /// Every existing HistorySpace definition is immutable and retained.
    HistorySpaceHistoryChanged,
    /// A new child cutoff cannot exceed the already published base.
    HistorySpaceBaseNotPublished {
        history_space_id: HistorySpaceId,
        base_revision: Revision,
        published: Revision,
    },
    /// A child cutoff cannot predate its parent's own pinned base.
    HistorySpaceCutoffBeforeParent {
        history_space_id: HistorySpaceId,
        base_revision: Revision,
        parent_base_revision: Revision,
    },
    /// Candidate shared revision must be later than its base.
    RevisionNotIncreasing,
    /// Existing Entity identities/retirements were removed or changed.
    EntityHistoryChanged,
    /// Existing Perspective definitions/retirements were removed or changed.
    PerspectiveHistoryChanged,
    /// Existing schema definitions cannot be removed.
    SchemaDefinitionRemoved,
    /// A changed or new schema definition must use the candidate commit revision.
    SchemaDefinitionRevisionMismatch,
    /// Schema's layer snapshot differs from the project layer catalog.
    LayerSchemaMismatch,
    /// An Entity refers to an EntityType absent from the candidate schema.
    UnknownEntityType(EntityTypeId),
    /// A newly created Entity cannot use a retired EntityType.
    EntityTypeNotActiveAtCreation(EntityTypeId),
    /// Staged metadata Records do not exactly match the candidate metadata delta.
    MetadataBatchMismatch,
    /// One exact metadata operation right did not match the current policy.
    Denied {
        /// Required operation capability.
        capability: Capability,
        /// Exact target coordinates evaluated by policy.
        target: PolicyTarget,
    },
    /// A catalog rejected an Entity identity or retirement transition.
    EntityCatalog(EntityCatalogError),
    /// A catalog rejected a Perspective metadata or retirement transition.
    PerspectiveCatalog(PerspectiveCatalogError),
    /// A schema rejected layer identity, lifecycle, or base-layer transition.
    LayerSchema(LayerSchemaError),
    /// Canonical schema history could not validate this schema snapshot.
    SchemaHistory(SchemaHistoryError),
    /// A metadata record could not be canonically encoded for batch equality.
    RecordCodec(RecordCodecError),
}

impl From<EntityCatalogError> for ProjectMetadataValidationError {
    fn from(error: EntityCatalogError) -> Self {
        Self::EntityCatalog(error)
    }
}
impl From<PerspectiveCatalogError> for ProjectMetadataValidationError {
    fn from(error: PerspectiveCatalogError) -> Self {
        Self::PerspectiveCatalog(error)
    }
}
impl From<LayerSchemaError> for ProjectMetadataValidationError {
    fn from(error: LayerSchemaError) -> Self {
        Self::LayerSchema(error)
    }
}
impl From<SchemaHistoryError> for ProjectMetadataValidationError {
    fn from(error: SchemaHistoryError) -> Self {
        Self::SchemaHistory(error)
    }
}
impl From<RecordCodecError> for ProjectMetadataValidationError {
    fn from(error: RecordCodecError) -> Self {
        Self::RecordCodec(error)
    }
}

impl fmt::Display for ProjectMetadataValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SharedRevisionMismatch => {
                formatter.write_str("project metadata revisions differ")
            }
            Self::HistorySpaceHistoryChanged => {
                formatter.write_str("HistorySpace ancestry history is immutable")
            }
            Self::HistorySpaceBaseNotPublished {
                history_space_id,
                base_revision,
                published,
            } => write!(
                formatter,
                "HistorySpace {history_space_id} pins unpublished base {base_revision}; latest is {published}"
            ),
            Self::HistorySpaceCutoffBeforeParent {
                history_space_id,
                base_revision,
                parent_base_revision,
            } => write!(
                formatter,
                "HistorySpace {history_space_id} cutoff {base_revision} predates parent base {parent_base_revision}"
            ),
            Self::RevisionNotIncreasing => {
                formatter.write_str("project metadata revision must advance")
            }
            Self::EntityHistoryChanged => {
                formatter.write_str("Entity identity and retirement history must be append-only")
            }
            Self::PerspectiveHistoryChanged => {
                formatter.write_str("Perspective definitions and retirements must be append-only")
            }
            Self::SchemaDefinitionRemoved => {
                formatter.write_str("schema definitions cannot be removed")
            }
            Self::SchemaDefinitionRevisionMismatch => {
                formatter.write_str("schema definition delta must use the commit revision")
            }
            Self::LayerSchemaMismatch => {
                formatter.write_str("schema and layer catalog post-states differ")
            }
            Self::UnknownEntityType(entity_type_id) => write!(
                formatter,
                "Entity references unknown EntityType {entity_type_id}"
            ),
            Self::EntityTypeNotActiveAtCreation(entity_type_id) => write!(
                formatter,
                "Entity cannot be created with retired EntityType {entity_type_id}"
            ),
            Self::MetadataBatchMismatch => formatter
                .write_str("staged metadata Records do not match the complete candidate delta"),
            Self::Denied { capability, .. } => {
                write!(formatter, "project metadata write requires {capability:?}")
            }
            Self::EntityCatalog(error) => write!(formatter, "invalid Entity catalog: {error}"),
            Self::PerspectiveCatalog(error) => {
                write!(formatter, "invalid Perspective catalog: {error}")
            }
            Self::LayerSchema(error) => write!(formatter, "invalid Layer schema: {error}"),
            Self::SchemaHistory(error) => write!(formatter, "invalid schema history: {error}"),
            Self::RecordCodec(error) => {
                write!(formatter, "metadata record encoding failed: {error}")
            }
        }
    }
}

impl std::error::Error for ProjectMetadataValidationError {}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::{
        ProjectMetadataCandidate, ProjectMetadataSnapshot, ProjectMetadataValidationError,
        validate_project_metadata_transaction,
    };
    use crate::catalog::{
        Entity, EntityCatalogSnapshot, HistorySpaceCatalog, HistorySpaceDefinition,
        PerspectiveCatalogSnapshot, PerspectiveDefinitionRevision,
    };
    use crate::ids::{
        DomainId, EntityId, EntityTypeId, HistorySpaceId, LayerId, PerspectiveId, PolicyRuleId,
        PrincipalId, Revision, SchemaRevision,
    };
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
    use crate::revision_backend::{InMemoryRevisionBackend, RevisionBackend};
    use crate::schema::{EntityTypeDefinition, Lifecycle};
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, PolicyScope, PolicySubject,
        SecurityPolicySnapshot,
    };
    use crate::values::Symbol;
    use crate::wire_records::Record;

    #[derive(Debug)]
    struct TestError(String);

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    impl std::error::Error for TestError {}

    macro_rules! test_error_conversions {
        ($($error:ty),+ $(,)?) => {
            $(
                impl From<$error> for TestError {
                    fn from(error: $error) -> Self {
                        Self(error.to_string())
                    }
                }
            )+
        };
    }

    test_error_conversions!(
        crate::IdValidationError,
        crate::RevisionError,
        crate::catalog::HistorySpaceError,
        crate::catalog::EntityCatalogError,
        crate::catalog::PerspectiveCatalogError,
        crate::layers::LayerSchemaError,
        crate::values::SymbolError,
        crate::schema::SchemaDefinitionError,
        crate::schema_history::SchemaHistoryError,
        super::ProjectMetadataValidationError,
        crate::SecurityPolicyError,
        crate::RevisionLogError,
        crate::TransactionBeginError,
        std::num::TryFromIntError,
    );

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    struct Scenario {
        base: ProjectMetadataSnapshot,
        candidate: ProjectMetadataCandidate,
        records: Vec<Record>,
        principal: PrincipalId,
    }

    fn scenario(child_base: Revision) -> Result<Scenario, TestError> {
        let base_revision = Revision::FIRST_COMMIT;
        let commit_revision = base_revision.next_commit()?;
        let base_schema_revision = SchemaRevision::from_published_revision(base_revision);
        let commit_schema_revision = SchemaRevision::from_published_revision(commit_revision);

        let history_space_id = id::<HistorySpaceId>(1)?;
        let child_history_space_id = id::<HistorySpaceId>(2)?;
        let base_layer_id = id::<LayerId>(3)?;
        let next_base_layer_id = id::<LayerId>(4)?;
        let entity_type_id = id::<EntityTypeId>(5)?;
        let entity_id = id::<EntityId>(6)?;
        let perspective_id = id::<PerspectiveId>(7)?;

        let base_history_spaces = HistorySpaceCatalog::new(vec![HistorySpaceDefinition::new(
            history_space_id,
            None,
            Revision::GENESIS,
        )?])?;
        let base_layer = LayerDefinition::new(
            base_layer_id,
            Symbol::new("alpha")?,
            None,
            0,
            Lifecycle::Active,
            base_schema_revision,
        );
        let base_layers = LayerSchemaSnapshot::new(
            base_schema_revision,
            vec![base_layer.clone()],
            base_layer_id,
        )?;
        let entity_type = EntityTypeDefinition::new(
            entity_type_id,
            Symbol::new("person")?,
            None,
            Lifecycle::Active,
            base_revision,
        );

        let mut schema_history = SchemaHistoryReferenceModel::new();
        schema_history.publish(
            base_revision,
            vec![
                SchemaDefinition::Layer(base_layer.clone()),
                SchemaDefinition::LayerSnapshot(base_layers.clone()),
                SchemaDefinition::EntityType(entity_type),
            ],
        )?;
        let base_schema = schema_history.schema_at(SchemaMode::Current, base_revision)?;

        let base = ProjectMetadataSnapshot::new(
            base_revision,
            base_history_spaces.clone(),
            base_layers.clone(),
            EntityCatalogSnapshot::new(base_revision, vec![], vec![])?,
            PerspectiveCatalogSnapshot::new(base_revision, vec![], vec![])?,
            base_schema,
        )?;

        let revised_base_layer =
            base_layer.revise_state(None, 1, Lifecycle::Deprecated, commit_schema_revision)?;
        let new_base_layer = LayerDefinition::new(
            next_base_layer_id,
            Symbol::new("beta")?,
            Some("new base".to_owned()),
            0,
            Lifecycle::Active,
            commit_schema_revision,
        );
        let next_layers = LayerSchemaSnapshot::new(
            commit_schema_revision,
            vec![revised_base_layer.clone(), new_base_layer.clone()],
            next_base_layer_id,
        )?;
        schema_history.publish(
            commit_revision,
            vec![
                SchemaDefinition::Layer(revised_base_layer.clone()),
                SchemaDefinition::Layer(new_base_layer.clone()),
                SchemaDefinition::LayerSnapshot(next_layers.clone()),
            ],
        )?;
        let next_schema = schema_history.schema_at(SchemaMode::Current, commit_revision)?;

        let next_history_spaces = HistorySpaceCatalog::new(vec![
            HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)?,
            HistorySpaceDefinition::new(
                child_history_space_id,
                Some(history_space_id),
                child_base,
            )?,
        ])?;
        let next_entities = EntityCatalogSnapshot::new(
            commit_revision,
            vec![Entity::new(entity_id, entity_type_id, commit_revision)],
            vec![],
        )?;
        let next_perspectives = PerspectiveCatalogSnapshot::new(
            commit_revision,
            vec![PerspectiveDefinitionRevision::new(
                perspective_id,
                Some("observer".to_owned()),
                None,
                commit_revision,
            )?],
            vec![],
        )?;
        let candidate = ProjectMetadataCandidate::new(
            commit_revision,
            next_history_spaces,
            next_layers,
            next_entities,
            next_perspectives,
            next_schema,
        )?;
        let records = vec![
            Record::HistorySpaceDefinition(HistorySpaceDefinition::new(
                child_history_space_id,
                Some(history_space_id),
                child_base,
            )?),
            Record::Entity(Entity::new(entity_id, entity_type_id, commit_revision)),
            Record::PerspectiveDefinitionRevision(PerspectiveDefinitionRevision::new(
                perspective_id,
                Some("observer".to_owned()),
                None,
                commit_revision,
            )?),
            Record::LayerDefinition(revised_base_layer),
            Record::LayerDefinition(new_base_layer),
            Record::LayerSchemaSnapshot(LayerSchemaSnapshot::new(
                commit_schema_revision,
                vec![
                    LayerDefinition::new(
                        base_layer_id,
                        Symbol::new("alpha")?,
                        None,
                        1,
                        Lifecycle::Deprecated,
                        commit_schema_revision,
                    ),
                    LayerDefinition::new(
                        next_base_layer_id,
                        Symbol::new("beta")?,
                        Some("new base".to_owned()),
                        0,
                        Lifecycle::Active,
                        commit_schema_revision,
                    ),
                ],
                next_base_layer_id,
            )?),
        ];
        Ok(Scenario {
            base,
            candidate,
            records,
            principal: id::<PrincipalId>(8)?,
        })
    }

    fn policy(principal: PrincipalId, allow: bool) -> Result<SecurityPolicySnapshot, TestError> {
        let capabilities = [
            Capability::HistorySpaceCreate,
            Capability::EntityCreate,
            Capability::PerspectiveCreate,
            Capability::LayerManage,
        ];
        let rules = capabilities
            .into_iter()
            .enumerate()
            .map(|(index, capability)| {
                Ok(CapabilityRule::new(
                    id::<PolicyRuleId>(20 + u8::try_from(index)?)?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(
                        capability,
                        if allow {
                            crate::security::GrantEffect::Allow
                        } else {
                            crate::security::GrantEffect::Deny
                        },
                    ),
                    PolicyScope::project(),
                ))
            })
            .collect::<Result<Vec<_>, TestError>>()?;
        Ok(SecurityPolicySnapshot::new(
            vec![crate::security::Principal::new(principal)],
            vec![],
            vec![],
            rules,
        )?)
    }

    fn bootstrap_records(snapshot: &ProjectMetadataSnapshot) -> Vec<Record> {
        let mut records = snapshot
            .history_spaces()
            .definitions()
            .iter()
            .copied()
            .map(Record::HistorySpaceDefinition)
            .collect::<Vec<_>>();
        records.extend(
            snapshot
                .schema()
                .definitions()
                .iter()
                .map(|definition| match definition {
                    SchemaDefinition::Layer(value) => Record::LayerDefinition(value.clone()),
                    SchemaDefinition::LayerSnapshot(value) => {
                        Record::LayerSchemaSnapshot(value.clone())
                    }
                    SchemaDefinition::EntityType(value) => {
                        Record::EntityTypeDefinition(value.clone())
                    }
                    SchemaDefinition::Predicate(value) => {
                        Record::PredicateDefinition(value.clone())
                    }
                    SchemaDefinition::EventKind(value) => {
                        Record::EventKindDefinition(value.clone())
                    }
                    SchemaDefinition::Timeline(value) => Record::TimelineDefinition(value.clone()),
                    SchemaDefinition::TimeUnit(value) => Record::TimeUnitDefinition(value.clone()),
                }),
        );
        records
    }

    #[test]
    fn one_authorized_transaction_publishes_metadata_and_atomic_base_switch()
    -> Result<(), TestError> {
        let scenario = scenario(Revision::FIRST_COMMIT)?;
        let policy = policy(scenario.principal, true)?;
        let post_state = validate_project_metadata_transaction(
            &scenario.base,
            scenario.candidate,
            &scenario.records,
            &policy,
            scenario.principal,
        )?;
        assert_eq!(post_state.revision(), Revision::FIRST_COMMIT.next_commit()?);
        assert_eq!(post_state.layers().base_layer_id(), id::<LayerId>(4)?);
        assert_eq!(post_state.entities().entities().len(), 1);
        assert_eq!(post_state.perspectives().definitions().len(), 1);
        assert_eq!(post_state.history_spaces().definitions().len(), 2);
        Ok(())
    }

    #[test]
    fn metadata_validator_runs_inside_one_publish_and_denial_leaves_head_unchanged()
    -> Result<(), TestError> {
        let successful = scenario(Revision::FIRST_COMMIT)?;
        let allowed = policy(successful.principal, true)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        assert_eq!(
            backend.publish(bootstrap_records(&successful.base))?,
            Revision::FIRST_COMMIT
        );
        let mut transaction = crate::OpenTransaction::begin(&mut backend, Revision::FIRST_COMMIT)?;
        for record in successful.records.iter().cloned() {
            transaction.stage(record);
        }
        let committed = crate::commit_mixed_record_batch(transaction, |base_revision, entries| {
            if base_revision != Revision::FIRST_COMMIT {
                return Err(ProjectMetadataValidationError::RevisionNotIncreasing);
            }
            validate_project_metadata_transaction(
                &successful.base,
                successful.candidate,
                entries,
                &allowed,
                successful.principal,
            )
            .map(|_| ())
        })
        .map_err(|error| TestError(error.to_string()))?;
        assert_eq!(committed, Revision::FIRST_COMMIT.next_commit()?);
        assert_eq!(
            backend
                .read_at(committed)?
                .filter(|(record_revision, _)| *record_revision == committed)
                .count(),
            successful.records.len()
        );

        let denied_case = scenario(Revision::FIRST_COMMIT)?;
        let denied = policy(denied_case.principal, false)?;
        let mut denied_backend = InMemoryRevisionBackend::<Record>::new();
        denied_backend.publish(bootstrap_records(&denied_case.base))?;
        let mut transaction =
            crate::OpenTransaction::begin(&mut denied_backend, Revision::FIRST_COMMIT)?;
        for record in denied_case.records.iter().cloned() {
            transaction.stage(record);
        }
        assert!(
            crate::commit_mixed_record_batch(transaction, |_, entries| {
                validate_project_metadata_transaction(
                    &denied_case.base,
                    denied_case.candidate,
                    entries,
                    &denied,
                    denied_case.principal,
                )
                .map(|_| ())
            })
            .is_err()
        );
        assert_eq!(denied_backend.latest_published(), Revision::FIRST_COMMIT);
        assert_eq!(denied_backend.commits().len(), 1);
        Ok(())
    }

    #[test]
    fn unpublished_child_cutoff_and_denied_metadata_rights_reject_candidate()
    -> Result<(), TestError> {
        let unpublished = scenario(Revision::FIRST_COMMIT.next_commit()?)?;
        let allowed = policy(unpublished.principal, true)?;
        assert!(matches!(
            validate_project_metadata_transaction(
                &unpublished.base,
                unpublished.candidate.clone(),
                &unpublished.records,
                &allowed,
                unpublished.principal,
            ),
            Err(ProjectMetadataValidationError::HistorySpaceBaseNotPublished { .. })
        ));

        let authorized_case = scenario(Revision::FIRST_COMMIT)?;
        let denied = policy(authorized_case.principal, false)?;
        assert!(matches!(
            validate_project_metadata_transaction(
                &authorized_case.base,
                authorized_case.candidate,
                &authorized_case.records,
                &denied,
                authorized_case.principal,
            ),
            Err(ProjectMetadataValidationError::Denied { .. })
        ));
        Ok(())
    }

    #[test]
    fn data_only_revision_keeps_catalog_views_aligned_without_metadata_records()
    -> Result<(), TestError> {
        let scenario = scenario(Revision::FIRST_COMMIT)?;
        let revision = Revision::FIRST_COMMIT.next_commit()?;
        let schema_revision = SchemaRevision::from_published_revision(revision);
        let mut history = SchemaHistoryReferenceModel::new();
        history.publish(
            Revision::FIRST_COMMIT,
            scenario.base.schema().definitions().to_vec(),
        )?;
        history.publish(revision, vec![])?;
        let schema = history.schema_at(SchemaMode::Current, revision)?;
        let layers = LayerSchemaSnapshot::new(
            schema_revision,
            scenario.base.layers().definitions().to_vec(),
            scenario.base.layers().base_layer_id(),
        )?;
        let entities = scenario.base.entities().revise(
            revision,
            scenario.base.entities().entities().to_vec(),
            scenario.base.entities().retirements().to_vec(),
        )?;
        let perspectives = scenario.base.perspectives().revise(
            revision,
            scenario.base.perspectives().definitions().to_vec(),
            scenario.base.perspectives().retirements().to_vec(),
        )?;
        let candidate = ProjectMetadataCandidate::new(
            revision,
            scenario.base.history_spaces().clone(),
            layers,
            entities,
            perspectives,
            schema,
        )?;
        let policy = SecurityPolicySnapshot::new(
            vec![crate::security::Principal::new(scenario.principal)],
            vec![],
            vec![],
            vec![],
        )?;
        let post_state = validate_project_metadata_transaction(
            &scenario.base,
            candidate,
            &[],
            &policy,
            scenario.principal,
        )?;
        assert_eq!(post_state.revision(), revision);
        assert!(post_state.entities().entities().is_empty());
        Ok(())
    }

    #[test]
    fn history_space_children_publish_without_mutating_schema_or_siblings() -> Result<(), TestError>
    {
        let scenario = scenario(Revision::FIRST_COMMIT)?;
        let revision = Revision::FIRST_COMMIT.next_commit()?;
        let schema_revision = SchemaRevision::from_published_revision(revision);
        let history_space_id = id::<HistorySpaceId>(1)?;
        let left_child_id = id::<HistorySpaceId>(30)?;
        let right_child_id = id::<HistorySpaceId>(31)?;
        let root = HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)?;
        let left_child = HistorySpaceDefinition::new(
            left_child_id,
            Some(history_space_id),
            Revision::FIRST_COMMIT,
        )?;
        let right_child = HistorySpaceDefinition::new(
            right_child_id,
            Some(history_space_id),
            Revision::FIRST_COMMIT,
        )?;

        let mut history = SchemaHistoryReferenceModel::new();
        history.publish(
            Revision::FIRST_COMMIT,
            scenario.base.schema().definitions().to_vec(),
        )?;
        let prior_fingerprint = history
            .schema_at(SchemaMode::Current, Revision::FIRST_COMMIT)?
            .fingerprint();
        history.publish(revision, vec![])?;
        let schema = history.schema_at(SchemaMode::Current, revision)?;
        let candidate = ProjectMetadataCandidate::new(
            revision,
            HistorySpaceCatalog::new(vec![root, left_child, right_child])?,
            LayerSchemaSnapshot::new(
                schema_revision,
                scenario.base.layers().definitions().to_vec(),
                scenario.base.layers().base_layer_id(),
            )?,
            scenario.base.entities().revise(
                revision,
                scenario.base.entities().entities().to_vec(),
                scenario.base.entities().retirements().to_vec(),
            )?,
            scenario.base.perspectives().revise(
                revision,
                scenario.base.perspectives().definitions().to_vec(),
                scenario.base.perspectives().retirements().to_vec(),
            )?,
            schema,
        )?;
        let child_records = vec![
            Record::HistorySpaceDefinition(left_child),
            Record::HistorySpaceDefinition(right_child),
        ];
        let policy = SecurityPolicySnapshot::new(
            vec![crate::security::Principal::new(scenario.principal)],
            vec![],
            vec![],
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(40)?,
                PolicySubject::Principal(scenario.principal),
                CapabilityGrant::new(
                    Capability::HistorySpaceCreate,
                    crate::security::GrantEffect::Allow,
                ),
                PolicyScope::new(Some(history_space_id), None, None, None, None),
            )],
        )?;
        let post_state = validate_project_metadata_transaction(
            &scenario.base,
            candidate.clone(),
            &child_records,
            &policy,
            scenario.principal,
        )?;
        assert_eq!(post_state.history_spaces().definitions().len(), 3);
        assert_eq!(post_state.schema().fingerprint(), prior_fingerprint);

        let wrong_target_policy = SecurityPolicySnapshot::new(
            vec![crate::security::Principal::new(scenario.principal)],
            vec![],
            vec![],
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(41)?,
                PolicySubject::Principal(scenario.principal),
                CapabilityGrant::new(
                    Capability::HistorySpaceCreate,
                    crate::security::GrantEffect::Allow,
                ),
                PolicyScope::new(Some(left_child_id), None, None, None, None),
            )],
        )?;
        assert!(matches!(
            validate_project_metadata_transaction(
                &scenario.base,
                candidate,
                &child_records,
                &wrong_target_policy,
                scenario.principal,
            ),
            Err(ProjectMetadataValidationError::Denied {
                capability: Capability::HistorySpaceCreate,
                ..
            })
        ));
        Ok(())
    }
}
