//! Slow, explicit ContextPrecedence calculation for M2 reference models.

use std::cmp::Ordering;
use std::fmt;

use crate::catalog::HistorySpaceCatalog;
use crate::context::ContextKey;
use crate::ids::{HistorySpaceId, LayerId};
use crate::layers::LayerSchemaSnapshot;

/// Priority coordinates for one candidate in a pinned HistorySpace/layer view.
///
/// Ordering follows resolution priority: `Greater` means the left-hand
/// precedence is preferred. HistorySpace distance is compared first, so a
/// local record outranks every inherited record regardless of LayerRank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextPrecedence {
    history_space_distance: usize,
    layer_rank: i32,
}

impl ContextPrecedence {
    /// Resolves one context against the selected HistorySpace and schema snapshots.
    pub fn for_context(
        query_history_space_id: HistorySpaceId,
        record_history_space_id: HistorySpaceId,
        layer_id: LayerId,
        history_spaces: &HistorySpaceCatalog,
        layers: &LayerSchemaSnapshot,
    ) -> Result<Self, ContextPrecedenceError> {
        let distance = ancestry_distance(
            query_history_space_id,
            record_history_space_id,
            history_spaces,
        )?;
        let definition = layers
            .definition(layer_id)
            .ok_or(ContextPrecedenceError::UnknownLayer { layer_id })?;
        Ok(Self {
            history_space_distance: distance,
            layer_rank: definition.precedence_rank(),
        })
    }

    /// Compares assertion contexts only when their Perspective/Epistemic partitions match.
    pub fn compare_assertion_contexts(
        query_history_space_id: HistorySpaceId,
        left: ContextKey,
        right: ContextKey,
        history_spaces: &HistorySpaceCatalog,
        layers: &LayerSchemaSnapshot,
    ) -> Result<Ordering, ContextPrecedenceError> {
        if left.perspective_scope() != right.perspective_scope()
            || left.epistemic_mode() != right.epistemic_mode()
        {
            return Err(ContextPrecedenceError::DifferentPartition);
        }
        let left = Self::for_context(
            query_history_space_id,
            left.history_space_id(),
            left.layer_id(),
            history_spaces,
            layers,
        )?;
        let right = Self::for_context(
            query_history_space_id,
            right.history_space_id(),
            right.layer_id(),
            history_spaces,
            layers,
        )?;
        Ok(left.cmp(&right))
    }

    /// Compares event contexts, which have no Perspective/Epistemic partition.
    pub fn compare_event_contexts(
        query_history_space_id: HistorySpaceId,
        left_history_space_id: HistorySpaceId,
        left_layer_id: LayerId,
        right_history_space_id: HistorySpaceId,
        right_layer_id: LayerId,
        history_spaces: &HistorySpaceCatalog,
        layers: &LayerSchemaSnapshot,
    ) -> Result<Ordering, ContextPrecedenceError> {
        let left = Self::for_context(
            query_history_space_id,
            left_history_space_id,
            left_layer_id,
            history_spaces,
            layers,
        )?;
        let right = Self::for_context(
            query_history_space_id,
            right_history_space_id,
            right_layer_id,
            history_spaces,
            layers,
        )?;
        Ok(left.cmp(&right))
    }

    /// Returns the number of parent edges from query space to record origin.
    #[must_use]
    pub const fn history_space_distance(self) -> usize {
        self.history_space_distance
    }

    /// Returns the historical rank from the pinned schema snapshot.
    #[must_use]
    pub const fn layer_rank(self) -> i32 {
        self.layer_rank
    }
}

impl Ord for ContextPrecedence {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .history_space_distance
            .cmp(&self.history_space_distance)
            .then_with(|| self.layer_rank.cmp(&other.layer_rank))
    }
}

impl PartialOrd for ContextPrecedence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn ancestry_distance(
    query_history_space_id: HistorySpaceId,
    record_history_space_id: HistorySpaceId,
    catalog: &HistorySpaceCatalog,
) -> Result<usize, ContextPrecedenceError> {
    let mut current = query_history_space_id;
    let mut distance = 0_usize;
    loop {
        let definition =
            catalog
                .definition(current)
                .ok_or(ContextPrecedenceError::UnknownHistorySpace {
                    history_space_id: current,
                })?;
        if current == record_history_space_id {
            return Ok(distance);
        }
        match definition.parent_history_space_id() {
            Some(parent) => {
                distance = distance.saturating_add(1);
                current = parent;
            }
            None => {
                return Err(ContextPrecedenceError::RecordOutsideQueryAncestry {
                    query_history_space_id,
                    record_history_space_id,
                });
            }
        }
    }
}

/// Invalid context partition, layer, or HistorySpace ancestry for precedence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPrecedenceError {
    /// A context references a HistorySpace not in the selected catalog.
    UnknownHistorySpace { history_space_id: HistorySpaceId },
    /// The record origin is neither the query space nor one of its ancestors.
    RecordOutsideQueryAncestry {
        query_history_space_id: HistorySpaceId,
        record_history_space_id: HistorySpaceId,
    },
    /// A concrete record/query LayerId is absent from the pinned schema snapshot.
    UnknownLayer { layer_id: LayerId },
    /// Assertions from different Perspective/Epistemic partitions have no shared rank.
    DifferentPartition,
    /// The process index-memory admission budget was exhausted while building a precedence cache.
    ResourceBudgetExceeded,
}

impl fmt::Display for ContextPrecedenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHistorySpace { history_space_id } => {
                write!(formatter, "unknown HistorySpace in precedence calculation: {history_space_id}")
            }
            Self::RecordOutsideQueryAncestry { query_history_space_id, record_history_space_id } => write!(
                formatter,
                "record HistorySpace {record_history_space_id} is not visible through query HistorySpace {query_history_space_id}"
            ),
            Self::UnknownLayer { layer_id } => {
                write!(formatter, "unknown LayerId in precedence calculation: {layer_id}")
            }
            Self::DifferentPartition => formatter.write_str(
                "Perspective/Epistemic partitions are separate and have no ContextPrecedence comparison",
            ),
            Self::ResourceBudgetExceeded => {
                formatter.write_str("context precedence index exceeded the process index-memory budget")
            }
        }
    }
}

impl std::error::Error for ContextPrecedenceError {}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{ContextPrecedence, ContextPrecedenceError};
    use crate::Symbol;
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition};
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{DomainId, HistorySpaceId, LayerId, PerspectiveId, Revision, SchemaRevision};
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
    use crate::schema::Lifecycle;

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn fixture() -> Result<
        (
            HistorySpaceCatalog,
            LayerSchemaSnapshot,
            [HistorySpaceId; 3],
            [LayerId; 2],
        ),
        crate::IdValidationError,
    > {
        let root_id = id::<HistorySpaceId>(1)?;
        let child_id = id::<HistorySpaceId>(2)?;
        let grandchild_id = id::<HistorySpaceId>(3)?;
        let root = HistorySpaceDefinition::new(root_id, None, Revision::GENESIS)
            .map_err(|_| crate::IdValidationError::InvalidText)?;
        let child = HistorySpaceDefinition::new(child_id, Some(root_id), Revision::GENESIS)
            .map_err(|_| crate::IdValidationError::InvalidText)?;
        let grandchild =
            HistorySpaceDefinition::new(grandchild_id, Some(child_id), Revision::GENESIS)
                .map_err(|_| crate::IdValidationError::InvalidText)?;
        let base_id = id::<LayerId>(4)?;
        let override_id = id::<LayerId>(5)?;
        let base = LayerDefinition::new(
            base_id,
            Symbol::new("base").map_err(|_| crate::IdValidationError::InvalidText)?,
            None,
            -10,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(Revision::GENESIS),
        );
        let overlay = LayerDefinition::new(
            override_id,
            Symbol::new("overlay").map_err(|_| crate::IdValidationError::InvalidText)?,
            None,
            20,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(Revision::GENESIS),
        );
        let catalog = HistorySpaceCatalog::new(vec![root, child, grandchild])
            .map_err(|_| crate::IdValidationError::InvalidText)?;
        let layers = LayerSchemaSnapshot::new(
            SchemaRevision::from_published_revision(Revision::GENESIS),
            vec![base, overlay],
            base_id,
        )
        .map_err(|_| crate::IdValidationError::InvalidText)?;
        Ok((
            catalog,
            layers,
            [root_id, child_id, grandchild_id],
            [base_id, override_id],
        ))
    }

    #[test]
    fn history_space_distance_precedes_any_layer_rank() -> Result<(), crate::IdValidationError> {
        let (catalog, layers, spaces, layer_ids) = fixture()?;
        let [root, _, child] = spaces;
        let [base, overlay] = layer_ids;
        let local = ContextPrecedence::for_context(child, child, base, &catalog, &layers)
            .map_err(|_| crate::IdValidationError::InvalidText)?;
        let inherited = ContextPrecedence::for_context(child, root, overlay, &catalog, &layers)
            .map_err(|_| crate::IdValidationError::InvalidText)?;
        assert_eq!(local.history_space_distance(), 0);
        assert_eq!(inherited.history_space_distance(), 2);
        assert_eq!(local.cmp(&inherited), Ordering::Greater);
        Ok(())
    }

    #[test]
    fn higher_layer_rank_wins_within_one_origin_and_equal_rank_stays_equal()
    -> Result<(), crate::IdValidationError> {
        let (catalog, layers, spaces, layer_ids) = fixture()?;
        let [root, _, _] = spaces;
        let [base, overlay] = layer_ids;
        let comparison = ContextPrecedence::compare_event_contexts(
            root, root, overlay, root, base, &catalog, &layers,
        );
        assert_eq!(comparison, Ok(Ordering::Greater));
        let equal = ContextPrecedence::compare_event_contexts(
            root, root, base, root, base, &catalog, &layers,
        );
        assert_eq!(equal, Ok(Ordering::Equal));
        Ok(())
    }

    #[test]
    fn assertion_partitions_are_never_compared_and_descendants_are_rejected()
    -> Result<(), crate::IdValidationError> {
        let (catalog, layers, spaces, layer_ids) = fixture()?;
        let [root, _, child] = spaces;
        let [base, overlay] = layer_ids;
        let world = ContextKey::new(
            root,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )
        .map_err(|_| crate::IdValidationError::InvalidText)?;
        let perspective = ContextKey::new(
            root,
            base,
            PerspectiveScope::Perspective(id::<PerspectiveId>(6)?),
            EpistemicMode::Knows,
        )
        .map_err(|_| crate::IdValidationError::InvalidText)?;
        assert_eq!(
            ContextPrecedence::compare_assertion_contexts(
                root,
                world,
                perspective,
                &catalog,
                &layers
            ),
            Err(ContextPrecedenceError::DifferentPartition),
        );
        assert!(matches!(
            ContextPrecedence::for_context(root, child, overlay, &catalog, &layers),
            Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. })
        ));
        Ok(())
    }
}
