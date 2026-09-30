//! Project-wide HistorySpace, Entity, and Perspective catalog values.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{
    EntityId, EntityRetirementId, EntityTypeId, HistorySpaceId, PerspectiveId,
    PerspectiveRetirementId, Revision,
};

/// Immutable ancestry metadata for one HistorySpace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HistorySpaceDefinition {
    history_space_id: HistorySpaceId,
    parent_history_space_id: Option<HistorySpaceId>,
    base_revision: Revision,
}

impl HistorySpaceDefinition {
    /// Creates a root or single-parent HistorySpace definition.
    pub fn new(
        history_space_id: HistorySpaceId,
        parent_history_space_id: Option<HistorySpaceId>,
        base_revision: Revision,
    ) -> Result<Self, HistorySpaceError> {
        if parent_history_space_id == Some(history_space_id) {
            return Err(HistorySpaceError::SelfParent);
        }
        if parent_history_space_id.is_none() && base_revision != Revision::GENESIS {
            return Err(HistorySpaceError::RootBaseRevisionNotGenesis);
        }
        Ok(Self {
            history_space_id,
            parent_history_space_id,
            base_revision,
        })
    }

    /// Returns the stable HistorySpace identity.
    #[must_use]
    pub const fn history_space_id(self) -> HistorySpaceId {
        self.history_space_id
    }

    /// Returns its single parent, or `None` for a root.
    #[must_use]
    pub const fn parent_history_space_id(self) -> Option<HistorySpaceId> {
        self.parent_history_space_id
    }

    /// Returns the immutable cutoff revision in the parent; roots use Genesis.
    #[must_use]
    pub const fn base_revision(self) -> Revision {
        self.base_revision
    }
}

/// A validated forest of HistorySpaces with no missing parents or ancestry cycles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySpaceCatalog {
    definitions: Vec<HistorySpaceDefinition>,
}

impl HistorySpaceCatalog {
    /// Validates unique identities, parent references, and acyclic ancestry.
    pub fn new(mut definitions: Vec<HistorySpaceDefinition>) -> Result<Self, HistorySpaceError> {
        if definitions.is_empty() {
            return Err(HistorySpaceError::EmptyCatalog);
        }
        definitions.sort_by_key(|definition| definition.history_space_id());

        for pair in definitions.windows(2) {
            if let [left, right] = pair {
                if left.history_space_id() == right.history_space_id() {
                    return Err(HistorySpaceError::DuplicateHistorySpaceId);
                }
            }
        }
        for definition in &definitions {
            if let Some(parent_id) = definition.parent_history_space_id() {
                if definitions
                    .binary_search_by_key(&parent_id, |candidate| candidate.history_space_id())
                    .is_err()
                {
                    return Err(HistorySpaceError::UnknownParent);
                }
            }
        }
        validate_acyclic_ancestry(&definitions)?;

        Ok(Self { definitions })
    }

    /// Returns the canonical ID-sorted HistorySpace definitions.
    #[must_use]
    pub fn definitions(&self) -> &[HistorySpaceDefinition] {
        &self.definitions
    }

    /// Looks up a HistorySpace by its typed identity.
    #[must_use]
    pub fn definition(&self, id: HistorySpaceId) -> Option<HistorySpaceDefinition> {
        self.definitions
            .binary_search_by_key(&id, |definition| definition.history_space_id())
            .ok()
            .and_then(|index| self.definitions.get(index).copied())
    }
}

fn validate_acyclic_ancestry(
    definitions: &[HistorySpaceDefinition],
) -> Result<(), HistorySpaceError> {
    let mut complete = BTreeSet::new();
    for start in definitions {
        let mut path = Vec::new();
        let mut visiting = BTreeSet::new();
        let mut cursor = Some(start.history_space_id());
        while let Some(id) = cursor {
            if complete.contains(&id) {
                break;
            }
            if !visiting.insert(id) {
                return Err(HistorySpaceError::AncestryCycle);
            }
            path.push(id);
            cursor = definitions
                .binary_search_by_key(&id, |definition| definition.history_space_id())
                .ok()
                .and_then(|index| definitions.get(index))
                .and_then(|definition| definition.parent_history_space_id());
        }
        complete.extend(path);
    }
    Ok(())
}

/// Immutable Entity identity and its permanent schema type assignment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Entity {
    entity_id: EntityId,
    entity_type_id: EntityTypeId,
    created_revision: Revision,
}

impl Entity {
    /// Creates the immutable identity row; entity names and facts are Assertions.
    #[must_use]
    pub const fn new(
        entity_id: EntityId,
        entity_type_id: EntityTypeId,
        created_revision: Revision,
    ) -> Self {
        Self {
            entity_id,
            entity_type_id,
            created_revision,
        }
    }

    /// Returns the stable entity identity.
    #[must_use]
    pub const fn entity_id(self) -> EntityId {
        self.entity_id
    }

    /// Returns the permanent EntityType assignment.
    #[must_use]
    pub const fn entity_type_id(self) -> EntityTypeId {
        self.entity_type_id
    }

    /// Returns the revision that created this catalog identity.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One immutable project-wide Entity retirement record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityRetirement {
    entity_retirement_id: EntityRetirementId,
    entity_id: EntityId,
    created_revision: Revision,
}

impl EntityRetirement {
    /// Creates a typed retirement event for an Entity identity.
    #[must_use]
    pub const fn new(
        entity_retirement_id: EntityRetirementId,
        entity_id: EntityId,
        created_revision: Revision,
    ) -> Self {
        Self {
            entity_retirement_id,
            entity_id,
            created_revision,
        }
    }

    /// Returns this retirement record's concrete identity.
    #[must_use]
    pub const fn entity_retirement_id(self) -> EntityRetirementId {
        self.entity_retirement_id
    }

    /// Returns the direct typed target EntityId.
    #[must_use]
    pub const fn entity_id(self) -> EntityId {
        self.entity_id
    }

    /// Returns the revision that introduced the retirement.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// A historical view of immutable Entity rows and retirement records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityCatalogSnapshot {
    revision: Revision,
    entities: Vec<Entity>,
    retirements: Vec<EntityRetirement>,
}

impl EntityCatalogSnapshot {
    /// Validates the complete Entity catalog as of one shared database revision.
    pub fn new(
        revision: Revision,
        mut entities: Vec<Entity>,
        mut retirements: Vec<EntityRetirement>,
    ) -> Result<Self, EntityCatalogError> {
        entities.sort_by_key(|entity| entity.entity_id());
        retirements.sort_by_key(|retirement| retirement.entity_retirement_id());
        validate_entity_catalog(revision, &entities, &retirements)?;
        Ok(Self {
            revision,
            entities,
            retirements,
        })
    }

    /// Creates a later historical view without removing, retyping, or rewriting identities.
    pub fn revise(
        &self,
        revision: Revision,
        entities: Vec<Entity>,
        retirements: Vec<EntityRetirement>,
    ) -> Result<Self, EntityCatalogError> {
        if revision <= self.revision {
            return Err(EntityCatalogError::RevisionNotIncreasing);
        }
        let next = Self::new(revision, entities, retirements)?;
        for entity in &self.entities {
            match next.entity(entity.entity_id()) {
                None => return Err(EntityCatalogError::EntityRemoved),
                Some(current) if current != *entity => {
                    return Err(EntityCatalogError::EntityIdentityChanged);
                }
                Some(_) => {}
            }
        }
        for retirement in &self.retirements {
            if !next.retirements.contains(retirement) {
                return Err(EntityCatalogError::RetirementHistoryRemoved);
            }
        }
        if next.entities.iter().any(|entity| {
            self.entity(entity.entity_id()).is_none() && entity.created_revision() != revision
        }) {
            return Err(EntityCatalogError::NewEntityRevisionMismatch);
        }
        if next.retirements.iter().any(|retirement| {
            !self
                .retirements
                .iter()
                .any(|prior| prior.entity_retirement_id() == retirement.entity_retirement_id())
                && retirement.created_revision() != revision
        }) {
            return Err(EntityCatalogError::NewRetirementRevisionMismatch);
        }
        Ok(next)
    }

    /// Returns the catalog's pinned revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Returns all retained Entity identities in ID order.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Returns the immutable retirement history in record-ID order.
    #[must_use]
    pub fn retirements(&self) -> &[EntityRetirement] {
        &self.retirements
    }

    /// Looks up an Entity even after retirement, preserving historical identity.
    #[must_use]
    pub fn entity(&self, entity_id: EntityId) -> Option<Entity> {
        self.entities
            .binary_search_by_key(&entity_id, |entity| entity.entity_id())
            .ok()
            .and_then(|index| self.entities.get(index).copied())
    }

    /// Returns whether this snapshot contains a retirement for the Entity.
    #[must_use]
    pub fn is_retired(&self, entity_id: EntityId) -> bool {
        self.retirements
            .iter()
            .any(|retirement| retirement.entity_id() == entity_id)
    }
}

fn validate_entity_catalog(
    revision: Revision,
    entities: &[Entity],
    retirements: &[EntityRetirement],
) -> Result<(), EntityCatalogError> {
    for pair in entities.windows(2) {
        if let [left, right] = pair {
            if left.entity_id() == right.entity_id() {
                return Err(EntityCatalogError::DuplicateEntityId);
            }
        }
    }
    for entity in entities {
        if entity.created_revision() > revision {
            return Err(EntityCatalogError::EntityAfterSnapshot);
        }
    }

    let entity_ids: BTreeSet<_> = entities.iter().map(|entity| entity.entity_id()).collect();
    let mut retirement_ids = BTreeSet::new();
    let mut retired_entities = BTreeSet::new();
    for retirement in retirements {
        if retirement.created_revision() > revision {
            return Err(EntityCatalogError::RetirementAfterSnapshot);
        }
        if !retirement_ids.insert(retirement.entity_retirement_id()) {
            return Err(EntityCatalogError::DuplicateRetirementId);
        }
        if !retired_entities.insert(retirement.entity_id()) {
            return Err(EntityCatalogError::DuplicateRetirementTarget);
        }
        if !entity_ids.contains(&retirement.entity_id()) {
            return Err(EntityCatalogError::UnknownEntityRetirementTarget);
        }
        let Some(entity) = entities
            .iter()
            .find(|entity| entity.entity_id() == retirement.entity_id())
        else {
            return Err(EntityCatalogError::UnknownEntityRetirementTarget);
        };
        if retirement.created_revision() <= entity.created_revision() {
            return Err(EntityCatalogError::RetirementNotAfterCreation);
        }
    }
    Ok(())
}

/// One revision of a Perspective's optional display metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerspectiveDefinitionRevision {
    perspective_id: PerspectiveId,
    display_name: Option<String>,
    description: Option<String>,
    recorded_revision: Revision,
}

impl PerspectiveDefinitionRevision {
    /// Creates an immutable metadata revision with non-empty present fields.
    pub fn new(
        perspective_id: PerspectiveId,
        display_name: Option<String>,
        description: Option<String>,
        recorded_revision: Revision,
    ) -> Result<Self, PerspectiveCatalogError> {
        if display_name.as_ref().is_some_and(String::is_empty) {
            return Err(PerspectiveCatalogError::EmptyDisplayName);
        }
        if description.as_ref().is_some_and(String::is_empty) {
            return Err(PerspectiveCatalogError::EmptyDescription);
        }
        Ok(Self {
            perspective_id,
            display_name,
            description,
            recorded_revision,
        })
    }

    /// Returns the stable Perspective identity.
    #[must_use]
    pub const fn perspective_id(&self) -> PerspectiveId {
        self.perspective_id
    }

    /// Returns the optional display label at this revision.
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    /// Returns the optional description at this revision.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the revision that introduced this metadata value.
    #[must_use]
    pub const fn recorded_revision(&self) -> Revision {
        self.recorded_revision
    }
}

/// One immutable project-wide Perspective retirement record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PerspectiveRetirement {
    perspective_retirement_id: PerspectiveRetirementId,
    perspective_id: PerspectiveId,
    created_revision: Revision,
}

impl PerspectiveRetirement {
    /// Creates a typed retirement event for a Perspective identity.
    #[must_use]
    pub const fn new(
        perspective_retirement_id: PerspectiveRetirementId,
        perspective_id: PerspectiveId,
        created_revision: Revision,
    ) -> Self {
        Self {
            perspective_retirement_id,
            perspective_id,
            created_revision,
        }
    }

    /// Returns this retirement record's concrete identity.
    #[must_use]
    pub const fn perspective_retirement_id(self) -> PerspectiveRetirementId {
        self.perspective_retirement_id
    }

    /// Returns the direct typed target PerspectiveId.
    #[must_use]
    pub const fn perspective_id(self) -> PerspectiveId {
        self.perspective_id
    }

    /// Returns the revision that introduced the retirement.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// A historical view of Perspective definition revisions and retirements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerspectiveCatalogSnapshot {
    revision: Revision,
    definitions: Vec<PerspectiveDefinitionRevision>,
    retirements: Vec<PerspectiveRetirement>,
}

impl PerspectiveCatalogSnapshot {
    /// Validates Perspective metadata history and terminal retirement state.
    pub fn new(
        revision: Revision,
        mut definitions: Vec<PerspectiveDefinitionRevision>,
        mut retirements: Vec<PerspectiveRetirement>,
    ) -> Result<Self, PerspectiveCatalogError> {
        definitions.sort_by_key(|definition| {
            (definition.perspective_id(), definition.recorded_revision())
        });
        retirements.sort_by_key(|retirement| retirement.perspective_retirement_id());
        validate_perspective_catalog(revision, &definitions, &retirements)?;
        Ok(Self {
            revision,
            definitions,
            retirements,
        })
    }

    /// Creates a later view by appending metadata and retirement records only.
    pub fn revise(
        &self,
        revision: Revision,
        definitions: Vec<PerspectiveDefinitionRevision>,
        retirements: Vec<PerspectiveRetirement>,
    ) -> Result<Self, PerspectiveCatalogError> {
        if revision <= self.revision {
            return Err(PerspectiveCatalogError::RevisionNotIncreasing);
        }
        let next = Self::new(revision, definitions, retirements)?;
        for definition in &self.definitions {
            if !next.definitions.contains(definition) {
                return Err(PerspectiveCatalogError::DefinitionHistoryRemoved);
            }
        }
        for retirement in &self.retirements {
            if !next.retirements.contains(retirement) {
                return Err(PerspectiveCatalogError::RetirementHistoryRemoved);
            }
        }
        if next.definitions.iter().any(|definition| {
            !self.definitions.contains(definition) && definition.recorded_revision() != revision
        }) {
            return Err(PerspectiveCatalogError::NewDefinitionRevisionMismatch);
        }
        if next.retirements.iter().any(|retirement| {
            !self.retirements.iter().any(|prior| {
                prior.perspective_retirement_id() == retirement.perspective_retirement_id()
            }) && retirement.created_revision() != revision
        }) {
            return Err(PerspectiveCatalogError::NewRetirementRevisionMismatch);
        }
        Ok(next)
    }

    /// Returns the catalog's pinned revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Returns all retained definition revisions in canonical identity/revision order.
    #[must_use]
    pub fn definitions(&self) -> &[PerspectiveDefinitionRevision] {
        &self.definitions
    }

    /// Returns the immutable retirement history in record-ID order.
    #[must_use]
    pub fn retirements(&self) -> &[PerspectiveRetirement] {
        &self.retirements
    }

    /// Returns the latest metadata revision for a Perspective, if it exists.
    #[must_use]
    pub fn latest_definition(
        &self,
        perspective_id: PerspectiveId,
    ) -> Option<&PerspectiveDefinitionRevision> {
        self.definitions
            .iter()
            .rev()
            .find(|definition| definition.perspective_id() == perspective_id)
    }

    /// Returns whether this snapshot contains a retirement for the Perspective.
    #[must_use]
    pub fn is_retired(&self, perspective_id: PerspectiveId) -> bool {
        self.retirements
            .iter()
            .any(|retirement| retirement.perspective_id() == perspective_id)
    }
}

fn validate_perspective_catalog(
    revision: Revision,
    definitions: &[PerspectiveDefinitionRevision],
    retirements: &[PerspectiveRetirement],
) -> Result<(), PerspectiveCatalogError> {
    let mut latest = BTreeMap::new();
    for definition in definitions {
        if definition.recorded_revision() > revision {
            return Err(PerspectiveCatalogError::DefinitionAfterSnapshot);
        }
        if latest
            .insert(definition.perspective_id(), definition.recorded_revision())
            .is_some_and(|prior| prior >= definition.recorded_revision())
        {
            return Err(PerspectiveCatalogError::DefinitionRevisionNotIncreasing);
        }
    }

    let mut retirement_ids = BTreeSet::new();
    let mut retired_perspectives = BTreeSet::new();
    for retirement in retirements {
        if retirement.created_revision() > revision {
            return Err(PerspectiveCatalogError::RetirementAfterSnapshot);
        }
        if !retirement_ids.insert(retirement.perspective_retirement_id()) {
            return Err(PerspectiveCatalogError::DuplicateRetirementId);
        }
        if !retired_perspectives.insert(retirement.perspective_id()) {
            return Err(PerspectiveCatalogError::DuplicateRetirementTarget);
        }
        let Some(last_definition_revision) = latest.get(&retirement.perspective_id()) else {
            return Err(PerspectiveCatalogError::UnknownRetirementTarget);
        };
        if retirement.created_revision() <= *last_definition_revision {
            return Err(PerspectiveCatalogError::RetirementNotAfterDefinition);
        }
    }
    Ok(())
}

/// Invalid parent/base relationships in a HistorySpace forest.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HistorySpaceError {
    /// A root HistorySpace must use the Genesis base revision.
    RootBaseRevisionNotGenesis,
    /// A HistorySpace cannot be its own parent.
    SelfParent,
    /// A catalog must contain at least one HistorySpace.
    EmptyCatalog,
    /// A HistorySpace identity appears more than once.
    DuplicateHistorySpaceId,
    /// A parent ID does not exist in the catalog.
    UnknownParent,
    /// Parent links contain a cycle.
    AncestryCycle,
}

impl fmt::Display for HistorySpaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootBaseRevisionNotGenesis => {
                formatter.write_str("root HistorySpace base revision must be Genesis")
            }
            Self::SelfParent => formatter.write_str("HistorySpace cannot parent itself"),
            Self::EmptyCatalog => formatter.write_str("HistorySpace catalog must not be empty"),
            Self::DuplicateHistorySpaceId => formatter.write_str("duplicate HistorySpaceId"),
            Self::UnknownParent => formatter.write_str("HistorySpace parent is unknown"),
            Self::AncestryCycle => formatter.write_str("HistorySpace ancestry contains a cycle"),
        }
    }
}

impl std::error::Error for HistorySpaceError {}

/// Invalid Entity identity, history, or retirement data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EntityCatalogError {
    /// An EntityId appears more than once in one snapshot.
    DuplicateEntityId,
    /// An Entity was created after the requested catalog revision.
    EntityAfterSnapshot,
    /// An Entity retirement was created after the requested catalog revision.
    RetirementAfterSnapshot,
    /// Two retirement IDs collide.
    DuplicateRetirementId,
    /// One Entity has more than one retirement record.
    DuplicateRetirementTarget,
    /// A retirement refers to no retained Entity.
    UnknownEntityRetirementTarget,
    /// Retirement must follow creation of its target Entity.
    RetirementNotAfterCreation,
    /// A later catalog state removed an immutable Entity row.
    EntityRemoved,
    /// A later catalog state changed an Entity ID or permanent EntityType assignment.
    EntityIdentityChanged,
    /// A later catalog state removed a retirement record.
    RetirementHistoryRemoved,
    /// A new Entity must be created in the revision that adds it.
    NewEntityRevisionMismatch,
    /// A new retirement record must use the revision that adds it.
    NewRetirementRevisionMismatch,
    /// A catalog revision must be greater than its prior view.
    RevisionNotIncreasing,
}

impl fmt::Display for EntityCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DuplicateEntityId => "duplicate EntityId",
            Self::EntityAfterSnapshot => "Entity is newer than its catalog snapshot",
            Self::RetirementAfterSnapshot => "Entity retirement is newer than its catalog snapshot",
            Self::DuplicateRetirementId => "duplicate EntityRetirementId",
            Self::DuplicateRetirementTarget => "Entity has more than one retirement record",
            Self::UnknownEntityRetirementTarget => "Entity retirement target is unknown",
            Self::RetirementNotAfterCreation => "Entity retirement must follow Entity creation",
            Self::EntityRemoved => "Entity catalog revisions cannot remove identity rows",
            Self::EntityIdentityChanged => {
                "Entity identity and EntityType assignment are immutable"
            }
            Self::RetirementHistoryRemoved => {
                "Entity catalog revisions cannot remove retirement history"
            }
            Self::NewEntityRevisionMismatch => {
                "new Entity must use the catalog revision that adds it"
            }
            Self::NewRetirementRevisionMismatch => {
                "new Entity retirement must use the catalog revision that adds it"
            }
            Self::RevisionNotIncreasing => "Entity catalog revision must increase",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for EntityCatalogError {}

/// Invalid Perspective metadata history or retirement data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PerspectiveCatalogError {
    /// A present display name must not be empty.
    EmptyDisplayName,
    /// A present description must not be empty.
    EmptyDescription,
    /// Metadata was recorded after the requested catalog revision.
    DefinitionAfterSnapshot,
    /// Metadata revisions for one Perspective must be strictly increasing.
    DefinitionRevisionNotIncreasing,
    /// Retirement was created after the requested catalog revision.
    RetirementAfterSnapshot,
    /// Two retirement IDs collide.
    DuplicateRetirementId,
    /// One Perspective has more than one retirement record.
    DuplicateRetirementTarget,
    /// A retirement refers to no known Perspective definition.
    UnknownRetirementTarget,
    /// Perspective retirement must follow its latest metadata revision.
    RetirementNotAfterDefinition,
    /// A later catalog view removed an immutable metadata revision.
    DefinitionHistoryRemoved,
    /// A later catalog view removed a retirement record.
    RetirementHistoryRemoved,
    /// A new metadata revision must use the revision that adds it.
    NewDefinitionRevisionMismatch,
    /// A new retirement record must use the revision that adds it.
    NewRetirementRevisionMismatch,
    /// A catalog revision must be greater than its prior view.
    RevisionNotIncreasing,
}

impl fmt::Display for PerspectiveCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyDisplayName => "Perspective display name must not be empty",
            Self::EmptyDescription => "Perspective description must not be empty",
            Self::DefinitionAfterSnapshot => {
                "Perspective definition is newer than its catalog snapshot"
            }
            Self::DefinitionRevisionNotIncreasing => "Perspective metadata revisions must increase",
            Self::RetirementAfterSnapshot => {
                "Perspective retirement is newer than its catalog snapshot"
            }
            Self::DuplicateRetirementId => "duplicate PerspectiveRetirementId",
            Self::DuplicateRetirementTarget => "Perspective has more than one retirement record",
            Self::UnknownRetirementTarget => "Perspective retirement target is unknown",
            Self::RetirementNotAfterDefinition => {
                "Perspective retirement must follow its latest definition"
            }
            Self::DefinitionHistoryRemoved => {
                "Perspective catalog revisions cannot remove metadata history"
            }
            Self::RetirementHistoryRemoved => {
                "Perspective catalog revisions cannot remove retirement history"
            }
            Self::NewDefinitionRevisionMismatch => {
                "new Perspective metadata must use the catalog revision that adds it"
            }
            Self::NewRetirementRevisionMismatch => {
                "new Perspective retirement must use the catalog revision that adds it"
            }
            Self::RevisionNotIncreasing => "Perspective catalog revision must increase",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PerspectiveCatalogError {}

#[cfg(test)]
mod tests {
    use super::{
        Entity, EntityCatalogError, EntityCatalogSnapshot, EntityRetirement, HistorySpaceCatalog,
        HistorySpaceDefinition, HistorySpaceError, PerspectiveCatalogError,
        PerspectiveCatalogSnapshot, PerspectiveDefinitionRevision, PerspectiveRetirement,
    };
    use crate::ids::{
        DomainId, EntityId, EntityRetirementId, EntityTypeId, HistorySpaceId, IdValidationError,
        PerspectiveId, PerspectiveRetirementId, Revision, RevisionError,
    };
    use std::error::Error;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        HistorySpace(HistorySpaceError),
        EntityCatalog(EntityCatalogError),
        PerspectiveCatalog(PerspectiveCatalogError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::HistorySpace(error) => write!(formatter, "{error}"),
                Self::EntityCatalog(error) => write!(formatter, "{error}"),
                Self::PerspectiveCatalog(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl Error for TestError {}

    impl From<IdValidationError> for TestError {
        fn from(error: IdValidationError) -> Self {
            Self::Id(error)
        }
    }

    impl From<RevisionError> for TestError {
        fn from(error: RevisionError) -> Self {
            Self::Revision(error)
        }
    }

    impl From<HistorySpaceError> for TestError {
        fn from(error: HistorySpaceError) -> Self {
            Self::HistorySpace(error)
        }
    }

    impl From<EntityCatalogError> for TestError {
        fn from(error: EntityCatalogError) -> Self {
            Self::EntityCatalog(error)
        }
    }

    impl From<PerspectiveCatalogError> for TestError {
        fn from(error: PerspectiveCatalogError) -> Self {
            Self::PerspectiveCatalog(error)
        }
    }

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn uuid<T: DomainId>(byte: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    #[test]
    fn history_space_root_and_parent_shape_are_validated() -> TestResult {
        let root_id = value!(uuid::<HistorySpaceId>(1));
        let child_id = value!(uuid::<HistorySpaceId>(2));
        let root = HistorySpaceDefinition::new(root_id, None, Revision::GENESIS)?;
        let child = HistorySpaceDefinition::new(child_id, Some(root_id), value!(revision(7)))?;

        assert_eq!(root.parent_history_space_id(), None);
        assert_eq!(root.base_revision(), Revision::GENESIS);
        assert_eq!(child.parent_history_space_id(), Some(root_id));
        assert_eq!(child.base_revision(), value!(revision(7)));
        assert_eq!(
            HistorySpaceDefinition::new(root_id, None, value!(revision(1))).err(),
            Some(HistorySpaceError::RootBaseRevisionNotGenesis)
        );
        assert_eq!(
            HistorySpaceDefinition::new(child_id, Some(child_id), Revision::GENESIS).err(),
            Some(HistorySpaceError::SelfParent)
        );
        Ok(())
    }

    #[test]
    fn history_space_catalog_rejects_missing_parents_duplicates_and_cycles() -> TestResult {
        let first = value!(uuid::<HistorySpaceId>(1));
        let second = value!(uuid::<HistorySpaceId>(2));
        let third = value!(uuid::<HistorySpaceId>(3));
        let root = HistorySpaceDefinition::new(first, None, Revision::GENESIS)?;
        let child = HistorySpaceDefinition::new(second, Some(first), Revision::GENESIS)?;
        let grandchild = HistorySpaceDefinition::new(third, Some(second), value!(revision(4)))?;
        let catalog = HistorySpaceCatalog::new(vec![grandchild, root, child])?;
        assert_eq!(catalog.definitions().len(), 3);
        assert_eq!(
            catalog
                .definition(third)
                .map(HistorySpaceDefinition::parent_history_space_id),
            Some(Some(second))
        );

        assert_eq!(
            HistorySpaceCatalog::new(vec![
                root,
                HistorySpaceDefinition::new(second, Some(third), Revision::GENESIS)?,
                grandchild,
            ])
            .err(),
            Some(HistorySpaceError::AncestryCycle)
        );
        assert_eq!(
            HistorySpaceCatalog::new(vec![root, root]).err(),
            Some(HistorySpaceError::DuplicateHistorySpaceId)
        );
        assert_eq!(
            HistorySpaceCatalog::new(vec![child]).err(),
            Some(HistorySpaceError::UnknownParent)
        );
        Ok(())
    }

    #[test]
    fn entity_catalog_keeps_identity_and_validates_retirement_history() -> TestResult {
        let entity_id = value!(uuid::<EntityId>(1));
        let entity_type_id = value!(uuid::<EntityTypeId>(2));
        let entity = Entity::new(entity_id, entity_type_id, value!(revision(1)));
        let initial = EntityCatalogSnapshot::new(value!(revision(1)), vec![entity], vec![])?;

        assert_eq!(
            EntityCatalogSnapshot::new(value!(revision(1)), vec![entity, entity], vec![]).err(),
            Some(EntityCatalogError::DuplicateEntityId)
        );

        assert_eq!(
            EntityCatalogSnapshot::new(
                value!(revision(1)),
                vec![entity],
                vec![EntityRetirement::new(
                    value!(uuid::<EntityRetirementId>(3)),
                    entity_id,
                    value!(revision(1))
                )]
            )
            .err(),
            Some(EntityCatalogError::RetirementNotAfterCreation)
        );
        assert_eq!(
            EntityCatalogSnapshot::new(
                value!(revision(1)),
                vec![entity],
                vec![EntityRetirement::new(
                    value!(uuid::<EntityRetirementId>(3)),
                    value!(uuid::<EntityId>(4)),
                    value!(revision(2))
                )]
            )
            .err(),
            Some(EntityCatalogError::RetirementAfterSnapshot)
        );
        assert_eq!(
            EntityCatalogSnapshot::new(
                value!(revision(1)),
                vec![entity],
                vec![EntityRetirement::new(
                    value!(uuid::<EntityRetirementId>(3)),
                    value!(uuid::<EntityId>(4)),
                    value!(revision(1))
                )]
            )
            .err(),
            Some(EntityCatalogError::UnknownEntityRetirementTarget)
        );

        let retirement = EntityRetirement::new(
            value!(uuid::<EntityRetirementId>(5)),
            entity_id,
            value!(revision(2)),
        );
        let retired = initial.revise(value!(revision(2)), vec![entity], vec![retirement])?;
        assert!(retired.is_retired(entity_id));
        assert_eq!(retired.entity(entity_id), Some(entity));

        let retyped = Entity::new(
            entity_id,
            value!(uuid::<EntityTypeId>(6)),
            value!(revision(1)),
        );
        assert_eq!(
            initial
                .revise(value!(revision(2)), vec![retyped], vec![])
                .err(),
            Some(EntityCatalogError::EntityIdentityChanged)
        );
        assert_eq!(
            initial.revise(value!(revision(2)), vec![], vec![]).err(),
            Some(EntityCatalogError::EntityRemoved)
        );
        assert_eq!(
            EntityCatalogSnapshot::new(
                value!(revision(2)),
                vec![entity],
                vec![retirement, retirement]
            )
            .err(),
            Some(EntityCatalogError::DuplicateRetirementId)
        );
        assert_eq!(
            EntityCatalogSnapshot::new(
                value!(revision(2)),
                vec![entity],
                vec![
                    retirement,
                    EntityRetirement::new(
                        value!(uuid::<EntityRetirementId>(6)),
                        entity_id,
                        value!(revision(2))
                    )
                ]
            )
            .err(),
            Some(EntityCatalogError::DuplicateRetirementTarget)
        );
        Ok(())
    }

    #[test]
    fn perspective_metadata_revisions_are_nonempty_and_append_only() -> TestResult {
        let perspective_id = value!(uuid::<PerspectiveId>(1));
        let initial_definition = PerspectiveDefinitionRevision::new(
            perspective_id,
            Some("Archivist".to_owned()),
            None,
            value!(revision(1)),
        )?;
        let initial = PerspectiveCatalogSnapshot::new(
            value!(revision(1)),
            vec![initial_definition.clone()],
            vec![],
        )?;
        let updated_definition = PerspectiveDefinitionRevision::new(
            perspective_id,
            Some("Historian".to_owned()),
            Some("Long-term memory".to_owned()),
            value!(revision(2)),
        )?;
        let updated = initial.revise(
            value!(revision(2)),
            vec![initial_definition, updated_definition],
            vec![],
        )?;
        assert_eq!(
            updated
                .latest_definition(perspective_id)
                .and_then(PerspectiveDefinitionRevision::display_name),
            Some("Historian")
        );
        assert_eq!(updated.definitions().len(), 2);
        assert_eq!(
            PerspectiveDefinitionRevision::new(
                perspective_id,
                Some(String::new()),
                None,
                value!(revision(3))
            )
            .err(),
            Some(PerspectiveCatalogError::EmptyDisplayName)
        );
        assert_eq!(
            PerspectiveDefinitionRevision::new(
                perspective_id,
                None,
                Some(String::new()),
                value!(revision(3))
            )
            .err(),
            Some(PerspectiveCatalogError::EmptyDescription)
        );
        assert_eq!(
            PerspectiveCatalogSnapshot::new(
                value!(revision(2)),
                vec![
                    PerspectiveDefinitionRevision::new(
                        perspective_id,
                        None,
                        None,
                        value!(revision(1))
                    )?,
                    PerspectiveDefinitionRevision::new(
                        perspective_id,
                        None,
                        None,
                        value!(revision(1))
                    )?,
                ],
                vec![]
            )
            .err(),
            Some(PerspectiveCatalogError::DefinitionRevisionNotIncreasing)
        );
        assert!(updated.latest_definition(perspective_id).is_some());
        if let Some(latest_definition) = updated.latest_definition(perspective_id) {
            assert_eq!(
                updated
                    .revise(value!(revision(3)), vec![latest_definition.clone()], vec![])
                    .err(),
                Some(PerspectiveCatalogError::DefinitionHistoryRemoved)
            );
        }
        Ok(())
    }

    #[test]
    fn perspective_retirement_is_terminal_and_keeps_metadata_history() -> TestResult {
        let perspective_id = value!(uuid::<PerspectiveId>(1));
        let retirement_id = value!(uuid::<PerspectiveRetirementId>(2));
        let definition = PerspectiveDefinitionRevision::new(
            perspective_id,
            None,
            Some("A project perspective".to_owned()),
            value!(revision(1)),
        )?;
        let initial =
            PerspectiveCatalogSnapshot::new(value!(revision(1)), vec![definition.clone()], vec![])?;
        let retirement =
            PerspectiveRetirement::new(retirement_id, perspective_id, value!(revision(2)));
        let retired = initial.revise(
            value!(revision(2)),
            vec![definition.clone()],
            vec![retirement],
        )?;
        assert!(retired.is_retired(perspective_id));
        assert_eq!(retired.latest_definition(perspective_id), Some(&definition));

        let after_retirement = PerspectiveDefinitionRevision::new(
            perspective_id,
            Some("Reactivated".to_owned()),
            None,
            value!(revision(3)),
        )?;
        assert_eq!(
            retired
                .revise(
                    value!(revision(3)),
                    vec![definition, after_retirement],
                    vec![retirement]
                )
                .err(),
            Some(PerspectiveCatalogError::RetirementNotAfterDefinition)
        );
        assert_eq!(
            PerspectiveCatalogSnapshot::new(value!(revision(2)), vec![], vec![retirement]).err(),
            Some(PerspectiveCatalogError::UnknownRetirementTarget)
        );
        assert_eq!(
            PerspectiveCatalogSnapshot::new(
                value!(revision(3)),
                vec![PerspectiveDefinitionRevision::new(
                    perspective_id,
                    None,
                    None,
                    value!(revision(1))
                )?],
                vec![
                    retirement,
                    PerspectiveRetirement::new(
                        value!(uuid::<PerspectiveRetirementId>(3)),
                        perspective_id,
                        value!(revision(2))
                    ),
                ],
            )
            .err(),
            Some(PerspectiveCatalogError::DuplicateRetirementTarget)
        );
        Ok(())
    }
}
