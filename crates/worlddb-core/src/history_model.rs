//! Slow, index-free reference model for HistorySpace parent cutoffs.

use std::fmt;

use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition, HistorySpaceError};
use crate::ids::{HistorySpaceId, Revision};
use crate::revision_history::{InMemoryRevisionLog, RevisionLogError};

#[derive(Clone)]
struct ScopedEntry<T> {
    history_space_id: HistorySpaceId,
    value: T,
}

/// Index-free HistorySpace reference model over the shared revision sequence.
///
/// Every child pins a parent snapshot at its declared `base_revision`. A read
/// walks the global commit sequence in order and retains only entries owned by
/// the selected space or one of its ancestors at that ancestor's fixed cutoff.
/// The model has no write API for schema identities or schema history.
///
/// ```compile_fail
/// use worlddb_core::HistorySpaceReferenceModel;
/// fn cannot_write_schema<T>(model: &mut HistorySpaceReferenceModel<T>) {
///     model.publish_schema_revision();
/// }
/// ```
#[derive(Clone)]
pub struct HistorySpaceReferenceModel<T> {
    catalog: HistorySpaceCatalog,
    history: InMemoryRevisionLog<ScopedEntry<T>>,
}

impl<T> HistorySpaceReferenceModel<T> {
    /// Creates a model from a valid initial forest at Genesis.
    pub fn new(definitions: Vec<HistorySpaceDefinition>) -> Result<Self, HistorySpaceModelError> {
        let catalog = HistorySpaceCatalog::new(definitions)?;
        for definition in catalog.definitions() {
            if definition.base_revision() > Revision::GENESIS {
                return Err(HistorySpaceModelError::BaseRevisionNotPublished {
                    requested: definition.base_revision(),
                    published: Revision::GENESIS,
                });
            }
        }
        Ok(Self {
            catalog,
            history: InMemoryRevisionLog::new(),
        })
    }

    /// Reconstructs a persisted global history while retaining commits that do
    /// not contain HistorySpace-owned records. Each vector element is one
    /// shared database revision and may contain records owned by several spaces.
    pub fn from_published_history(
        definitions: Vec<HistorySpaceDefinition>,
        commits: Vec<Vec<(HistorySpaceId, T)>>,
    ) -> Result<Self, HistorySpaceModelError> {
        let catalog = HistorySpaceCatalog::new(definitions)?;
        let mut history = InMemoryRevisionLog::new();
        for commit in commits {
            let revision = history.reserve_next()?;
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(commit.len())
                .map_err(|_| HistorySpaceModelError::AllocationFailed)?;
            for (history_space_id, value) in commit {
                if catalog.definition(history_space_id).is_none() {
                    return Err(HistorySpaceModelError::UnknownHistorySpace);
                }
                entries.push(ScopedEntry {
                    history_space_id,
                    value,
                });
            }
            history.publish(revision, entries)?;
        }

        let published = history.latest_published();
        for definition in catalog.definitions() {
            if definition.base_revision() > published {
                return Err(HistorySpaceModelError::BaseRevisionNotPublished {
                    requested: definition.base_revision(),
                    published,
                });
            }
            if let Some(parent_id) = definition.parent_history_space_id() {
                let parent =
                    catalog
                        .definition(parent_id)
                        .ok_or(HistorySpaceModelError::Catalog(
                            HistorySpaceError::UnknownParent,
                        ))?;
                if definition.base_revision() < parent.base_revision() {
                    return Err(HistorySpaceModelError::CutoffBeforeParentBase {
                        requested: definition.base_revision(),
                        parent_base: parent.base_revision(),
                    });
                }
            }
        }

        Ok(Self { catalog, history })
    }

    /// Reconstructs a persisted snapshot from only revisions containing local
    /// HistorySpace records. Commit revisions may have gaps because other
    /// project transactions can advance the shared revision without adding
    /// HistorySpace-owned data.
    pub fn from_published_snapshot(
        definitions: Vec<HistorySpaceDefinition>,
        latest_published: Revision,
        commits: Vec<(Revision, Vec<(HistorySpaceId, T)>)>,
    ) -> Result<Self, HistorySpaceModelError> {
        let catalog = HistorySpaceCatalog::new(definitions)?;
        for definition in catalog.definitions() {
            if definition.base_revision() > latest_published {
                return Err(HistorySpaceModelError::BaseRevisionNotPublished {
                    requested: definition.base_revision(),
                    published: latest_published,
                });
            }
            if let Some(parent_id) = definition.parent_history_space_id() {
                let parent =
                    catalog
                        .definition(parent_id)
                        .ok_or(HistorySpaceModelError::Catalog(
                            HistorySpaceError::UnknownParent,
                        ))?;
                if definition.base_revision() < parent.base_revision() {
                    return Err(HistorySpaceModelError::CutoffBeforeParentBase {
                        requested: definition.base_revision(),
                        parent_base: parent.base_revision(),
                    });
                }
            }
        }

        let mut scoped_commits = Vec::new();
        scoped_commits
            .try_reserve_exact(commits.len())
            .map_err(|_| HistorySpaceModelError::AllocationFailed)?;
        for (revision, entries) in commits {
            let mut scoped = Vec::new();
            scoped
                .try_reserve_exact(entries.len())
                .map_err(|_| HistorySpaceModelError::AllocationFailed)?;
            for (history_space_id, value) in entries {
                if catalog.definition(history_space_id).is_none() {
                    return Err(HistorySpaceModelError::UnknownHistorySpace);
                }
                scoped.push(ScopedEntry {
                    history_space_id,
                    value,
                });
            }
            scoped_commits.push((revision, scoped));
        }
        let history =
            InMemoryRevisionLog::from_published_commits(latest_published, scoped_commits)?;
        Ok(Self { catalog, history })
    }

    /// Returns the global latest published revision.
    #[must_use]
    pub const fn latest_published(&self) -> Revision {
        self.history.latest_published()
    }

    /// Returns the immutable, validated HistorySpace forest.
    #[must_use]
    pub fn catalog(&self) -> &HistorySpaceCatalog {
        &self.catalog
    }

    /// Adds a space with an explicit, permanently pinned parent cutoff.
    pub fn add_history_space(
        &mut self,
        definition: HistorySpaceDefinition,
    ) -> Result<(), HistorySpaceModelError> {
        if definition.base_revision() > self.history.latest_published() {
            return Err(HistorySpaceModelError::BaseRevisionNotPublished {
                requested: definition.base_revision(),
                published: self.history.latest_published(),
            });
        }
        if let Some(parent_id) = definition.parent_history_space_id() {
            let parent =
                self.catalog
                    .definition(parent_id)
                    .ok_or(HistorySpaceModelError::Catalog(
                        HistorySpaceError::UnknownParent,
                    ))?;
            if definition.base_revision() < parent.base_revision() {
                return Err(HistorySpaceModelError::CutoffBeforeParentBase {
                    requested: definition.base_revision(),
                    parent_base: parent.base_revision(),
                });
            }
        }

        let mut definitions = self.catalog.definitions().to_vec();
        definitions.push(definition);
        self.catalog = HistorySpaceCatalog::new(definitions)?;
        Ok(())
    }

    /// Publishes one space-local batch at the next global revision.
    pub fn publish(
        &mut self,
        history_space_id: HistorySpaceId,
        values: Vec<T>,
    ) -> Result<Revision, HistorySpaceModelError> {
        if self.catalog.definition(history_space_id).is_none() {
            return Err(HistorySpaceModelError::UnknownHistorySpace);
        }
        let revision = self.history.reserve_next()?;
        let entries = values
            .into_iter()
            .map(|value| ScopedEntry {
                history_space_id,
                value,
            })
            .collect();
        self.history.publish(revision, entries)?;
        Ok(revision)
    }

    /// Reads one HistorySpace through `as_of`, including only its pinned ancestry.
    pub fn read_at(
        &self,
        history_space_id: HistorySpaceId,
        as_of: Revision,
    ) -> Result<Vec<(Revision, HistorySpaceId, &T)>, HistorySpaceModelError> {
        Ok(self.iter_at(history_space_id, as_of)?.collect())
    }

    /// Streams one HistorySpace through `as_of` without first allocating all visible rows.
    pub fn iter_at(
        &self,
        history_space_id: HistorySpaceId,
        as_of: Revision,
    ) -> Result<impl Iterator<Item = (Revision, HistorySpaceId, &T)> + '_, HistorySpaceModelError>
    {
        let definition = self
            .catalog
            .definition(history_space_id)
            .ok_or(HistorySpaceModelError::UnknownHistorySpace)?;
        if as_of < definition.base_revision() {
            return Err(HistorySpaceModelError::ReadBeforeBase {
                requested: as_of,
                base_revision: definition.base_revision(),
            });
        }

        let history = self.history.read_at(as_of)?;
        Ok(history.filter_map(move |(revision, entry)| {
            self.visible_cutoff(history_space_id, entry.history_space_id, as_of)
                .filter(|cutoff| revision <= *cutoff)
                .map(|_| (revision, entry.history_space_id, &entry.value))
        }))
    }

    fn visible_cutoff(
        &self,
        selected_space: HistorySpaceId,
        entry_space: HistorySpaceId,
        as_of: Revision,
    ) -> Option<Revision> {
        let mut current_id = selected_space;
        let mut cutoff = as_of;
        loop {
            let current = self.catalog.definition(current_id)?;
            if current_id == entry_space {
                return Some(cutoff);
            }
            let parent_id = current.parent_history_space_id()?;
            cutoff = cutoff.min(current.base_revision());
            current_id = parent_id;
        }
    }
}

/// Invalid HistorySpace lookup, cutoff, ancestry, or revision use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistorySpaceModelError {
    /// The selected HistorySpace does not exist in this model.
    UnknownHistorySpace,
    /// A declared base cutoff is later than the latest published revision.
    BaseRevisionNotPublished {
        requested: Revision,
        published: Revision,
    },
    /// A child cutoff predates the point at which its parent itself became available.
    CutoffBeforeParentBase {
        requested: Revision,
        parent_base: Revision,
    },
    /// A historical read predates the selected space's pinned base.
    ReadBeforeBase {
        requested: Revision,
        base_revision: Revision,
    },
    /// An invalid HistorySpace catalog was supplied.
    Catalog(HistorySpaceError),
    /// A required in-memory history reservation could not be allocated.
    AllocationFailed,
    /// The global revision sequence rejected a publication or read.
    Revision(RevisionLogError),
}

impl From<HistorySpaceError> for HistorySpaceModelError {
    fn from(error: HistorySpaceError) -> Self {
        Self::Catalog(error)
    }
}

impl From<RevisionLogError> for HistorySpaceModelError {
    fn from(error: RevisionLogError) -> Self {
        Self::Revision(error)
    }
}

impl fmt::Display for HistorySpaceModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHistorySpace => formatter.write_str("unknown HistorySpaceId"),
            Self::BaseRevisionNotPublished {
                requested,
                published,
            } => write!(
                formatter,
                "base revision {requested} is not published; latest is {published}"
            ),
            Self::CutoffBeforeParentBase {
                requested,
                parent_base,
            } => write!(
                formatter,
                "child cutoff {requested} predates the parent base {parent_base}"
            ),
            Self::ReadBeforeBase {
                requested,
                base_revision,
            } => write!(
                formatter,
                "read revision {requested} predates HistorySpace base {base_revision}"
            ),
            Self::Catalog(error) => write!(formatter, "invalid HistorySpace catalog: {error}"),
            Self::AllocationFailed => formatter.write_str("HistorySpace history allocation failed"),
            Self::Revision(error) => write!(formatter, "invalid revision history: {error}"),
        }
    }
}

impl std::error::Error for HistorySpaceModelError {}

#[cfg(test)]
mod tests {
    use super::{HistorySpaceModelError, HistorySpaceReferenceModel};
    use crate::catalog::{HistorySpaceDefinition, HistorySpaceError};
    use crate::ids::{DomainId, HistorySpaceId, IdValidationError, Revision, RevisionError};

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        Catalog(HistorySpaceError),
        Model(HistorySpaceModelError),
    }

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
            Self::Catalog(error)
        }
    }

    impl From<HistorySpaceModelError> for TestError {
        fn from(error: HistorySpaceModelError) -> Self {
            Self::Model(error)
        }
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "invalid test ID: {error}"),
                Self::Revision(error) => write!(formatter, "invalid test revision: {error}"),
                Self::Catalog(error) => write!(formatter, "invalid test catalog: {error}"),
                Self::Model(error) => write!(formatter, "invalid test model: {error}"),
            }
        }
    }

    impl std::error::Error for TestError {}

    fn id<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, crate::ids::RevisionError> {
        Revision::new(value)
    }

    #[test]
    fn child_and_sibling_reads_use_fixed_parent_cutoffs() -> Result<(), TestError> {
        let root = id::<HistorySpaceId>(1)?;
        let early_child = id::<HistorySpaceId>(2)?;
        let sibling = id::<HistorySpaceId>(3)?;
        let later_child = id::<HistorySpaceId>(4)?;
        let root_definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut model = HistorySpaceReferenceModel::new(vec![root_definition])?;

        let root_one = model.publish(root, vec![10_u8])?;
        let root_two = model.publish(root, vec![20_u8])?;
        let early_definition = HistorySpaceDefinition::new(early_child, Some(root), root_one)?;
        model.add_history_space(early_definition)?;
        let sibling_definition = HistorySpaceDefinition::new(sibling, Some(root), root_two)?;
        model.add_history_space(sibling_definition)?;

        let root_three = model.publish(root, vec![30_u8])?;
        let early_four = model.publish(early_child, vec![41_u8])?;
        let sibling_five = model.publish(sibling, vec![51_u8])?;
        let later_definition = HistorySpaceDefinition::new(later_child, Some(root), root_three)?;
        model.add_history_space(later_definition)?;

        assert_eq!(
            model.read_at(early_child, sibling_five)?,
            vec![(root_one, root, &10), (early_four, early_child, &41)]
        );
        assert_eq!(
            model.read_at(sibling, sibling_five)?,
            vec![
                (root_one, root, &10),
                (root_two, root, &20),
                (sibling_five, sibling, &51)
            ]
        );
        assert_eq!(
            model.read_at(root, sibling_five)?,
            vec![
                (root_one, root, &10),
                (root_two, root, &20),
                (root_three, root, &30)
            ]
        );

        let later_six = model.publish(later_child, vec![61_u8])?;
        assert_eq!(
            model.read_at(later_child, later_six)?,
            vec![
                (root_one, root, &10),
                (root_two, root, &20),
                (root_three, root, &30),
                (later_six, later_child, &61)
            ]
        );
        assert_eq!(
            model.read_at(early_child, later_six)?,
            vec![(root_one, root, &10), (early_four, early_child, &41)]
        );
        Ok(())
    }

    #[test]
    fn future_cutoffs_unknown_spaces_and_pre_base_reads_fail_closed() -> Result<(), TestError> {
        let root = id::<HistorySpaceId>(11)?;
        let child = id::<HistorySpaceId>(12)?;
        let missing = id::<HistorySpaceId>(13)?;
        let root_definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut model = HistorySpaceReferenceModel::new(vec![root_definition])?;
        let root_one = model.publish(root, vec![10_u8])?;

        let future = revision(2)?;
        let future_definition = HistorySpaceDefinition::new(child, Some(root), future)?;
        assert_eq!(
            model.add_history_space(future_definition),
            Err(HistorySpaceModelError::BaseRevisionNotPublished {
                requested: future,
                published: root_one,
            })
        );
        let root_only_read = model.read_at(root, root_one)?;
        assert_eq!(root_only_read, vec![(root_one, root, &10)]);

        assert_eq!(
            model.read_at(missing, root_one),
            Err(HistorySpaceModelError::UnknownHistorySpace)
        );
        let child_definition = HistorySpaceDefinition::new(child, Some(root), root_one)?;
        model.add_history_space(child_definition)?;
        assert_eq!(
            model.read_at(child, Revision::GENESIS),
            Err(HistorySpaceModelError::ReadBeforeBase {
                requested: Revision::GENESIS,
                base_revision: root_one,
            })
        );
        Ok(())
    }

    #[test]
    fn nested_children_apply_each_ancestor_cutoff() -> Result<(), TestError> {
        let root = id::<HistorySpaceId>(31)?;
        let child = id::<HistorySpaceId>(32)?;
        let grandchild = id::<HistorySpaceId>(33)?;
        let root_definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut model = HistorySpaceReferenceModel::new(vec![root_definition])?;

        let root_one = model.publish(root, vec![10_u8])?;
        let root_two = model.publish(root, vec![20_u8])?;
        let child_definition = HistorySpaceDefinition::new(child, Some(root), root_two)?;
        model.add_history_space(child_definition)?;
        let child_three = model.publish(child, vec![30_u8])?;

        let grandchild_definition =
            HistorySpaceDefinition::new(grandchild, Some(child), child_three)?;
        model.add_history_space(grandchild_definition)?;
        let child_four = model.publish(child, vec![40_u8])?;
        let root_five = model.publish(root, vec![50_u8])?;
        let grandchild_six = model.publish(grandchild, vec![60_u8])?;

        assert_eq!(
            model.read_at(grandchild, grandchild_six)?,
            vec![
                (root_one, root, &10),
                (root_two, root, &20),
                (child_three, child, &30),
                (grandchild_six, grandchild, &60),
            ]
        );
        assert_eq!(
            model
                .iter_at(grandchild, grandchild_six)?
                .collect::<Vec<_>>(),
            model.read_at(grandchild, grandchild_six)?
        );
        assert_eq!(model.latest_published(), grandchild_six);
        assert!(child_four < root_five && root_five < grandchild_six);
        Ok(())
    }

    #[test]
    fn child_cutoff_cannot_predate_its_parent_base() -> Result<(), TestError> {
        let root = id::<HistorySpaceId>(21)?;
        let child = id::<HistorySpaceId>(22)?;
        let grandchild = id::<HistorySpaceId>(23)?;
        let root_definition = HistorySpaceDefinition::new(root, None, Revision::GENESIS)?;
        let mut model = HistorySpaceReferenceModel::new(vec![root_definition])?;
        let root_one = model.publish(root, vec![1_u8])?;
        let child_definition = HistorySpaceDefinition::new(child, Some(root), root_one)?;
        model.add_history_space(child_definition)?;
        let too_early = Revision::GENESIS;
        let grandchild_definition =
            HistorySpaceDefinition::new(grandchild, Some(child), too_early)?;
        assert_eq!(
            model.add_history_space(grandchild_definition),
            Err(HistorySpaceModelError::CutoffBeforeParentBase {
                requested: too_early,
                parent_base: root_one,
            })
        );
        Ok(())
    }
}
