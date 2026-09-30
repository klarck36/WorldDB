//! Revisioned project-wide layer definitions and snapshot-bound selection.

use std::collections::BTreeSet;
use std::fmt;

use crate::ids::{LayerId, SchemaRevision};
use crate::schema::{Lifecycle, NonEmptySet};
use crate::values::Symbol;

/// One immutable revision of a project-wide layer definition.
///
/// The layer ID and symbol identify the same layer across later definition
/// revisions. The symbol grammar is enforced by [`Symbol`]. Rank, description,
/// and lifecycle changes create a new value and leave earlier snapshots intact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerDefinition {
    layer_id: LayerId,
    symbol: Symbol,
    description: Option<String>,
    precedence_rank: i32,
    lifecycle: Lifecycle,
    created_revision: SchemaRevision,
}

impl LayerDefinition {
    /// Creates a layer definition at a published schema revision.
    #[must_use]
    pub const fn new(
        layer_id: LayerId,
        symbol: Symbol,
        description: Option<String>,
        precedence_rank: i32,
        lifecycle: Lifecycle,
        created_revision: SchemaRevision,
    ) -> Self {
        Self {
            layer_id,
            symbol,
            description,
            precedence_rank,
            lifecycle,
            created_revision,
        }
    }

    /// Appends one atomic state revision, preserving the stable ID and symbol.
    pub fn revise_state(
        &self,
        description: Option<String>,
        precedence_rank: i32,
        lifecycle: Lifecycle,
        created_revision: SchemaRevision,
    ) -> Result<Self, LayerSchemaError> {
        self.validate_next_revision(created_revision)?;
        if !self.lifecycle.may_follow(lifecycle) {
            return Err(LayerSchemaError::LifecycleRegression);
        }
        Ok(Self {
            layer_id: self.layer_id,
            symbol: self.symbol.clone(),
            description,
            precedence_rank,
            lifecycle,
            created_revision,
        })
    }

    /// Appends a revision with a new precedence rank, preserving identity and symbol.
    pub fn revise_precedence_rank(
        &self,
        precedence_rank: i32,
        created_revision: SchemaRevision,
    ) -> Result<Self, LayerSchemaError> {
        self.revise_state(
            self.description.clone(),
            precedence_rank,
            self.lifecycle,
            created_revision,
        )
    }

    /// Appends a description revision while preserving rank, identity, symbol, and lifecycle.
    pub fn revise_description(
        &self,
        description: Option<String>,
        created_revision: SchemaRevision,
    ) -> Result<Self, LayerSchemaError> {
        self.revise_state(
            description,
            self.precedence_rank,
            self.lifecycle,
            created_revision,
        )
    }

    /// Appends a lifecycle revision while retaining ID, symbol, rank, and description.
    pub fn revise_lifecycle(
        &self,
        lifecycle: Lifecycle,
        created_revision: SchemaRevision,
    ) -> Result<Self, LayerSchemaError> {
        self.revise_state(
            self.description.clone(),
            self.precedence_rank,
            lifecycle,
            created_revision,
        )
    }

    fn validate_next_revision(
        &self,
        created_revision: SchemaRevision,
    ) -> Result<(), LayerSchemaError> {
        if created_revision <= self.created_revision {
            return Err(LayerSchemaError::SchemaRevisionNotIncreasing);
        }
        Ok(())
    }

    /// Returns the stable project-wide layer identity.
    #[must_use]
    pub const fn layer_id(&self) -> LayerId {
        self.layer_id
    }

    /// Returns the stable schema symbol.
    #[must_use]
    pub const fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the optional human-readable description.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the rank used for layer precedence; lower values rank below higher values.
    #[must_use]
    pub const fn precedence_rank(&self) -> i32 {
        self.precedence_rank
    }

    /// Returns this definition's lifecycle at its revision.
    #[must_use]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Returns the schema revision that introduced this definition state.
    #[must_use]
    pub const fn created_revision(&self) -> SchemaRevision {
        self.created_revision
    }
}

/// The layer-definition portion of one immutable, historical schema snapshot.
///
/// Constructing a new value validates the complete post-change state. A base
/// switch is therefore represented by a new schema revision with a new explicit
/// `base_layer_id`; earlier snapshots retain their prior designation. This type
/// does not migrate or rewrite records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerSchemaSnapshot {
    revision: SchemaRevision,
    definitions: Vec<LayerDefinition>,
    base_layer_id: LayerId,
}

impl LayerSchemaSnapshot {
    /// Validates a complete layer schema state at one published schema revision.
    pub fn new(
        revision: SchemaRevision,
        mut definitions: Vec<LayerDefinition>,
        base_layer_id: LayerId,
    ) -> Result<Self, LayerSchemaError> {
        if definitions.is_empty() {
            return Err(LayerSchemaError::NoLayerDefinitions);
        }
        definitions.sort_by_key(LayerDefinition::layer_id);

        let mut ids = BTreeSet::new();
        let mut symbols = BTreeSet::new();
        let mut base = None;
        let mut lowest_active_rank = None;
        let mut lowest_active_count = 0;

        for definition in &definitions {
            if !ids.insert(definition.layer_id()) {
                return Err(LayerSchemaError::DuplicateLayerId);
            }
            if !symbols.insert(definition.symbol().clone()) {
                return Err(LayerSchemaError::DuplicateLayerSymbol);
            }
            if definition.created_revision() > revision {
                return Err(LayerSchemaError::DefinitionRevisionAfterSnapshot);
            }

            if definition.layer_id() == base_layer_id {
                base = Some(definition);
            }
            if definition.lifecycle() == Lifecycle::Active {
                match lowest_active_rank {
                    None => {
                        lowest_active_rank = Some(definition.precedence_rank());
                        lowest_active_count = 1;
                    }
                    Some(rank) if definition.precedence_rank() < rank => {
                        lowest_active_rank = Some(definition.precedence_rank());
                        lowest_active_count = 1;
                    }
                    Some(rank) if definition.precedence_rank() == rank => {
                        lowest_active_count += 1;
                    }
                    Some(_) => {}
                }
            }
        }

        let Some(base) = base else {
            return Err(LayerSchemaError::UnknownBaseLayer);
        };
        if base.lifecycle() != Lifecycle::Active {
            return Err(LayerSchemaError::BaseLayerNotActive);
        }
        if lowest_active_rank != Some(base.precedence_rank()) || lowest_active_count != 1 {
            return Err(LayerSchemaError::BaseLayerNotUniqueLowest);
        }

        Ok(Self {
            revision,
            definitions,
            base_layer_id,
        })
    }

    /// Creates a new complete schema state from this snapshot at a later revision.
    ///
    /// The definitions and base designation are validated together, so a base
    /// switch, rank changes, and retirement of the former base can be one atomic
    /// schema update.
    pub fn revise(
        &self,
        revision: SchemaRevision,
        definitions: Vec<LayerDefinition>,
        base_layer_id: LayerId,
    ) -> Result<Self, LayerSchemaError> {
        if revision <= self.revision {
            return Err(LayerSchemaError::SchemaRevisionNotIncreasing);
        }
        let next = Self::new(revision, definitions, base_layer_id)?;

        for previous in &self.definitions {
            let Some(current) = next.definition(previous.layer_id()) else {
                return Err(LayerSchemaError::LayerDefinitionRemoved);
            };
            if current.symbol() != previous.symbol() {
                return Err(LayerSchemaError::StableLayerSymbolChanged);
            }
            if !previous.lifecycle().may_follow(current.lifecycle()) {
                return Err(LayerSchemaError::LifecycleRegression);
            }
            if current.created_revision() < previous.created_revision() {
                return Err(LayerSchemaError::SchemaRevisionNotIncreasing);
            }
            let state_changed = current.description != previous.description
                || current.precedence_rank() != previous.precedence_rank()
                || current.lifecycle() != previous.lifecycle();
            if state_changed && current.created_revision() <= previous.created_revision() {
                return Err(LayerSchemaError::SchemaRevisionNotIncreasing);
            }
        }

        if next.definitions.iter().any(|definition| {
            self.definition(definition.layer_id()).is_none()
                && definition.created_revision() != revision
        }) {
            return Err(LayerSchemaError::NewLayerRevisionMismatch);
        }

        Ok(next)
    }

    /// Returns this layer view's pinned schema revision.
    #[must_use]
    pub const fn revision(&self) -> SchemaRevision {
        self.revision
    }

    /// Returns the explicit base layer stored in this historical schema state.
    #[must_use]
    pub const fn base_layer_id(&self) -> LayerId {
        self.base_layer_id
    }

    /// Returns the canonical ID-sorted definitions in this snapshot.
    #[must_use]
    pub fn definitions(&self) -> &[LayerDefinition] {
        &self.definitions
    }

    /// Returns a layer definition by its stable ID, whether active or retired.
    #[must_use]
    pub fn definition(&self, layer_id: LayerId) -> Option<&LayerDefinition> {
        self.definitions
            .binary_search_by_key(&layer_id, LayerDefinition::layer_id)
            .ok()
            .and_then(|index| self.definitions.get(index))
    }

    /// Resolves a query selection against this exact historical layer snapshot.
    pub fn resolve(
        &self,
        selection: &LayerSelection,
    ) -> Result<NonEmptySet<LayerId>, LayerSchemaError> {
        let ids = match selection {
            LayerSelection::BaseOnly => vec![self.base_layer_id],
            LayerSelection::AllActive => self
                .definitions
                .iter()
                .filter(|definition| definition.lifecycle() == Lifecycle::Active)
                .map(LayerDefinition::layer_id)
                .collect(),
            LayerSelection::Explicit(layers) => {
                for layer_id in layers.as_slice() {
                    if self.definition(*layer_id).is_none() {
                        return Err(LayerSchemaError::UnknownSelectedLayer);
                    }
                }
                layers.as_slice().to_vec()
            }
        };
        NonEmptySet::new(ids).map_err(|_| LayerSchemaError::EmptyResolvedSelection)
    }
}

/// Closed query-time layer selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayerSelection {
    /// Select the `base_layer_id` from the pinned schema snapshot.
    BaseOnly,
    /// Select every active layer in the pinned schema snapshot.
    AllActive,
    /// Select an explicit non-empty set known to the pinned schema snapshot.
    Explicit(NonEmptySet<LayerId>),
}

/// Failure while constructing a layer definition, schema snapshot, or selection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LayerSchemaError {
    /// A schema snapshot must contain at least the designated base layer.
    NoLayerDefinitions,
    /// The same stable LayerId appears more than once in a schema snapshot.
    DuplicateLayerId,
    /// Layer symbols are unique across active and retained definitions.
    DuplicateLayerSymbol,
    /// A definition cannot come from a revision later than its containing snapshot.
    DefinitionRevisionAfterSnapshot,
    /// The designated base LayerId is absent from this schema snapshot.
    UnknownBaseLayer,
    /// The designated base layer must be active in this schema snapshot.
    BaseLayerNotActive,
    /// The designated base must be the uniquely lowest-ranked active layer.
    BaseLayerNotUniqueLowest,
    /// An explicit query selection names a layer absent from the pinned snapshot.
    UnknownSelectedLayer,
    /// A revision attempted to remove a retained layer definition.
    LayerDefinitionRemoved,
    /// A stable layer symbol changed for an existing LayerId.
    StableLayerSymbolChanged,
    /// A new layer definition must start at the schema revision that adds it.
    NewLayerRevisionMismatch,
    /// Resolution unexpectedly yielded no layers.
    EmptyResolvedSelection,
    /// A layer definition revision must be greater than its prior revision.
    SchemaRevisionNotIncreasing,
    /// A lifecycle revision would move backward or skip the allowed progression.
    LifecycleRegression,
}

impl fmt::Display for LayerSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLayerDefinitions => formatter.write_str("layer schema must not be empty"),
            Self::DuplicateLayerId => formatter.write_str("duplicate LayerId in layer schema"),
            Self::DuplicateLayerSymbol => {
                formatter.write_str("duplicate layer symbol in layer schema")
            }
            Self::DefinitionRevisionAfterSnapshot => {
                formatter.write_str("layer definition is newer than its schema snapshot")
            }
            Self::UnknownBaseLayer => {
                formatter.write_str("base LayerId is absent from the layer schema")
            }
            Self::BaseLayerNotActive => formatter.write_str("base layer must be active"),
            Self::BaseLayerNotUniqueLowest => {
                formatter.write_str("base layer must have the unique lowest active rank")
            }
            Self::UnknownSelectedLayer => {
                formatter.write_str("explicit layer selection is unknown to the snapshot")
            }
            Self::LayerDefinitionRemoved => {
                formatter.write_str("layer schema revisions must retain prior definitions")
            }
            Self::StableLayerSymbolChanged => {
                formatter.write_str("a layer symbol is stable for its LayerId")
            }
            Self::NewLayerRevisionMismatch => {
                formatter.write_str("new layer must use the revision that adds it")
            }
            Self::EmptyResolvedSelection => {
                formatter.write_str("layer selection resolved to no layers")
            }
            Self::SchemaRevisionNotIncreasing => {
                formatter.write_str("layer definition schema revision must increase")
            }
            Self::LifecycleRegression => {
                formatter.write_str("layer lifecycle cannot move backward")
            }
        }
    }
}

impl std::error::Error for LayerSchemaError {}

#[cfg(test)]
mod tests {
    use super::{LayerDefinition, LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
    use crate::Symbol;
    use crate::ids::{
        DomainId, IdValidationError, LayerId, Revision, RevisionError, SchemaRevision,
    };
    use crate::schema::{Lifecycle, NonEmptySet, SchemaDefinitionError};
    use crate::values::SymbolError;
    use std::error::Error;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        SchemaDefinition(SchemaDefinitionError),
        LayerSchema(LayerSchemaError),
        Symbol(SymbolError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::SchemaDefinition(error) => write!(formatter, "{error}"),
                Self::LayerSchema(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
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

    impl From<SchemaDefinitionError> for TestError {
        fn from(error: SchemaDefinitionError) -> Self {
            Self::SchemaDefinition(error)
        }
    }

    impl From<LayerSchemaError> for TestError {
        fn from(error: LayerSchemaError) -> Self {
            Self::LayerSchema(error)
        }
    }

    impl From<SymbolError> for TestError {
        fn from(error: SymbolError) -> Self {
            Self::Symbol(error)
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

    fn id(byte: u8) -> Result<LayerId, TestError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        Ok(LayerId::try_from_bytes(bytes)?)
    }

    fn schema_revision(value: u64) -> Result<SchemaRevision, TestError> {
        Ok(SchemaRevision::from_published_revision(Revision::new(
            value,
        )?))
    }

    fn definition(
        layer_id: LayerId,
        symbol: &str,
        rank: i32,
    ) -> Result<LayerDefinition, TestError> {
        Ok(LayerDefinition::new(
            layer_id,
            Symbol::new(symbol)?,
            None,
            rank,
            Lifecycle::Active,
            schema_revision(0)?,
        ))
    }

    #[test]
    fn snapshot_requires_one_explicit_unique_lowest_active_base() -> TestResult {
        let base = value!(id(1));
        let overlay = value!(id(2));
        let valid = LayerSchemaSnapshot::new(
            value!(schema_revision(0)),
            vec![
                value!(definition(overlay, "scenario", 5)),
                value!(definition(base, "world", -10)),
            ],
            base,
        );
        assert!(valid.is_ok());

        let deprecated_base = LayerDefinition::new(
            base,
            value!(Symbol::new("world")),
            None,
            -10,
            Lifecycle::Deprecated,
            value!(schema_revision(0)),
        );
        assert_eq!(
            LayerSchemaSnapshot::new(value!(schema_revision(0)), vec![deprecated_base], base).err(),
            Some(LayerSchemaError::BaseLayerNotActive)
        );

        assert_eq!(
            LayerSchemaSnapshot::new(
                value!(schema_revision(0)),
                vec![
                    value!(definition(base, "world", 0)),
                    value!(definition(overlay, "scenario", 0))
                ],
                base,
            )
            .err(),
            Some(LayerSchemaError::BaseLayerNotUniqueLowest)
        );
        assert_eq!(
            LayerSchemaSnapshot::new(
                value!(schema_revision(0)),
                vec![
                    value!(definition(base, "world", 1)),
                    value!(definition(overlay, "scenario", 0))
                ],
                base,
            )
            .err(),
            Some(LayerSchemaError::BaseLayerNotUniqueLowest)
        );
        Ok(())
    }

    #[test]
    fn snapshot_rejects_duplicate_ids_symbols_and_future_definitions() -> TestResult {
        let first = value!(id(1));
        let second = value!(id(2));
        assert_eq!(
            LayerSchemaSnapshot::new(
                value!(schema_revision(0)),
                vec![
                    value!(definition(first, "world", 0)),
                    value!(definition(first, "other", 1))
                ],
                first,
            )
            .err(),
            Some(LayerSchemaError::DuplicateLayerId)
        );
        assert_eq!(
            LayerSchemaSnapshot::new(
                value!(schema_revision(0)),
                vec![
                    value!(definition(first, "world", 0)),
                    value!(definition(second, "world", 1))
                ],
                first,
            )
            .err(),
            Some(LayerSchemaError::DuplicateLayerSymbol)
        );
        let future = LayerDefinition::new(
            second,
            value!(Symbol::new("scenario")),
            None,
            5,
            Lifecycle::Active,
            value!(schema_revision(1)),
        );
        assert_eq!(
            LayerSchemaSnapshot::new(
                value!(schema_revision(0)),
                vec![value!(definition(first, "world", 0)), future],
                first
            )
            .err(),
            Some(LayerSchemaError::DefinitionRevisionAfterSnapshot)
        );
        assert_eq!(
            LayerSchemaSnapshot::new(
                value!(schema_revision(0)),
                vec![value!(definition(first, "world", 0))],
                second,
            )
            .err(),
            Some(LayerSchemaError::UnknownBaseLayer)
        );
        Ok(())
    }

    #[test]
    fn lifecycle_revisions_are_forward_only_and_keep_stable_identity() -> TestResult {
        let first = value!(definition(value!(id(1)), "world", 0));
        let deprecated =
            first.revise_lifecycle(Lifecycle::Deprecated, value!(schema_revision(1)))?;
        assert_eq!(deprecated.layer_id(), first.layer_id());
        assert_eq!(deprecated.symbol(), first.symbol());
        assert_eq!(
            deprecated
                .revise_lifecycle(Lifecycle::Active, value!(schema_revision(2)))
                .err(),
            Some(LayerSchemaError::LifecycleRegression)
        );
        assert_eq!(
            first
                .revise_lifecycle(Lifecycle::Deprecated, value!(schema_revision(0)))
                .err(),
            Some(LayerSchemaError::SchemaRevisionNotIncreasing)
        );
        let retired =
            deprecated.revise_lifecycle(Lifecycle::Retired, value!(schema_revision(2)))?;
        let snapshot =
            LayerSchemaSnapshot::new(value!(schema_revision(2)), vec![retired], value!(id(1)));
        assert_eq!(snapshot.err(), Some(LayerSchemaError::BaseLayerNotActive));
        Ok(())
    }

    #[test]
    fn schema_revision_preserves_layer_identity_symbols_and_history() -> TestResult {
        let base = value!(definition(value!(id(1)), "world", 0));
        let overlay = value!(definition(value!(id(2)), "scenario", 10));
        let current = LayerSchemaSnapshot::new(
            value!(schema_revision(0)),
            vec![base.clone(), overlay.clone()],
            base.layer_id(),
        )?;

        assert_eq!(
            current
                .revise(
                    value!(schema_revision(1)),
                    vec![base.clone()],
                    base.layer_id()
                )
                .err(),
            Some(LayerSchemaError::LayerDefinitionRemoved)
        );

        let renamed = LayerDefinition::new(
            base.layer_id(),
            value!(Symbol::new("earth")),
            None,
            base.precedence_rank(),
            Lifecycle::Active,
            value!(schema_revision(1)),
        );
        assert_eq!(
            current
                .revise(
                    value!(schema_revision(1)),
                    vec![renamed, overlay.clone()],
                    base.layer_id(),
                )
                .err(),
            Some(LayerSchemaError::StableLayerSymbolChanged)
        );

        let deprecated =
            overlay.revise_lifecycle(Lifecycle::Deprecated, value!(schema_revision(1)))?;
        let deprecated_schema = current.revise(
            value!(schema_revision(1)),
            vec![base.clone(), deprecated],
            base.layer_id(),
        )?;
        let reactivated = LayerDefinition::new(
            overlay.layer_id(),
            overlay.symbol().clone(),
            None,
            overlay.precedence_rank(),
            Lifecycle::Active,
            value!(schema_revision(2)),
        );
        assert_eq!(
            deprecated_schema
                .revise(
                    value!(schema_revision(2)),
                    vec![base.clone(), reactivated],
                    base.layer_id(),
                )
                .err(),
            Some(LayerSchemaError::LifecycleRegression)
        );

        let late_added = LayerDefinition::new(
            value!(id(3)),
            value!(Symbol::new("temporary")),
            None,
            20,
            Lifecycle::Active,
            value!(schema_revision(0)),
        );
        assert_eq!(
            current
                .revise(
                    value!(schema_revision(1)),
                    vec![base, overlay, late_added],
                    value!(id(1)),
                )
                .err(),
            Some(LayerSchemaError::NewLayerRevisionMismatch)
        );
        Ok(())
    }

    #[test]
    fn base_switch_is_a_new_atomic_historical_state_and_rank_revisions_keep_ids() -> TestResult {
        let old_base = value!(definition(value!(id(1)), "world", 0));
        let next_base = value!(definition(value!(id(2)), "scenario", 10));
        let before = LayerSchemaSnapshot::new(
            value!(schema_revision(0)),
            vec![old_base.clone(), next_base.clone()],
            old_base.layer_id(),
        )?;
        let first_commit = value!(schema_revision(1));
        let old_base_after_switch =
            old_base.revise_state(None, 20, Lifecycle::Deprecated, first_commit)?;
        let next_base_after_switch = next_base.revise_precedence_rank(-5, first_commit)?;
        let after = before.revise(
            first_commit,
            vec![
                old_base_after_switch.clone(),
                next_base_after_switch.clone(),
            ],
            next_base.layer_id(),
        )?;
        let old_base_retired = old_base_after_switch
            .revise_lifecycle(Lifecycle::Retired, value!(schema_revision(2)))?;
        let after_retirement = after.revise(
            value!(schema_revision(2)),
            vec![old_base_retired, next_base_after_switch.clone()],
            next_base.layer_id(),
        )?;

        assert_eq!(before.revision(), value!(schema_revision(0)));
        assert_eq!(before.base_layer_id(), old_base.layer_id());
        assert_eq!(after.base_layer_id(), next_base.layer_id());
        assert_eq!(old_base_after_switch.layer_id(), old_base.layer_id());
        assert_eq!(old_base_after_switch.lifecycle(), Lifecycle::Deprecated);
        assert_eq!(next_base_after_switch.symbol(), next_base.symbol());
        assert_eq!(
            before
                .definition(old_base.layer_id())
                .map(LayerDefinition::precedence_rank),
            Some(0)
        );
        assert_eq!(
            after
                .definition(old_base.layer_id())
                .map(LayerDefinition::precedence_rank),
            Some(20)
        );
        assert_eq!(
            after_retirement
                .definition(old_base.layer_id())
                .map(LayerDefinition::lifecycle),
            Some(Lifecycle::Retired)
        );
        assert_eq!(
            before
                .revise(before.revision(), vec![], next_base.layer_id())
                .err(),
            Some(LayerSchemaError::SchemaRevisionNotIncreasing)
        );
        Ok(())
    }

    #[test]
    fn selections_resolve_only_against_the_pinned_snapshot() -> TestResult {
        let base = value!(id(1));
        let active = value!(id(2));
        let retired = LayerDefinition::new(
            value!(id(3)),
            value!(Symbol::new("legacy")),
            None,
            20,
            Lifecycle::Retired,
            value!(schema_revision(0)),
        );
        let snapshot = LayerSchemaSnapshot::new(
            value!(schema_revision(0)),
            vec![
                value!(definition(base, "world", 0)),
                value!(definition(active, "scenario", 10)),
                retired.clone(),
            ],
            base,
        )?;

        assert_eq!(
            snapshot.resolve(&LayerSelection::BaseOnly)?.as_slice(),
            &[base]
        );
        assert_eq!(
            snapshot.resolve(&LayerSelection::AllActive)?.as_slice(),
            &[base, active]
        );
        let explicit = NonEmptySet::new(vec![retired.layer_id()])?;
        assert_eq!(
            snapshot
                .resolve(&LayerSelection::Explicit(explicit))?
                .as_slice(),
            &[retired.layer_id()]
        );
        let unknown = NonEmptySet::new(vec![value!(id(4))])?;
        assert_eq!(
            snapshot.resolve(&LayerSelection::Explicit(unknown)).err(),
            Some(LayerSchemaError::UnknownSelectedLayer)
        );
        Ok(())
    }
}
