//! Point-dependency optimistic concurrency for immutable write batches.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Bound, RangeBounds};

use crate::errors::{CommitOutcome, ConflictFact, ConflictReport};
use crate::ids::{OperationId, Revision, RevisionError};

#[derive(Clone, Debug)]
struct VersionedPoint<V> {
    revision: Revision,
    value: V,
}

/// In-memory point-version history used to exercise OCC read/write sets.
///
/// This model retains each value version so transactions can read their exact
/// base revision. It detects point conflicts; range, predicate, schema and
/// HistorySpace-head dependencies are handled by later OCC layers.
#[derive(Clone, Debug)]
pub struct OccPointStore<K, V> {
    head: Revision,
    points: BTreeMap<K, Vec<VersionedPoint<V>>>,
    change_log: Vec<(Revision, K)>,
    scope_versions: BTreeMap<OccScopeKey, Vec<Revision>>,
}

impl<K, V> Default for OccPointStore<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> OccPointStore<K, V> {
    /// Creates an empty point store at Genesis.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head: Revision::GENESIS,
            points: BTreeMap::new(),
            change_log: Vec::new(),
            scope_versions: BTreeMap::new(),
        }
    }

    /// Latest published revision in this model.
    #[must_use]
    pub const fn head(&self) -> Revision {
        self.head
    }
}

impl<K: Ord, V> OccPointStore<K, V> {
    /// Begins a transaction at a published revision.
    pub fn begin(
        &self,
        base_revision: Revision,
        operation_id: OperationId,
    ) -> Result<OccPointTransaction<K, V>, OccPointError> {
        if base_revision > self.head {
            return Err(OccPointError::BaseNotPublished {
                requested: base_revision,
                published: self.head,
            });
        }
        Ok(OccPointTransaction {
            base_revision,
            operation_id,
            reads: BTreeMap::new(),
            writes: BTreeMap::new(),
            write_versions: BTreeMap::new(),
            ranges: Vec::new(),
            scope_reads: BTreeMap::new(),
            scope_writes: std::collections::BTreeSet::new(),
        })
    }

    fn version_at(&self, key: &K, revision: Revision) -> Option<&VersionedPoint<V>> {
        self.points
            .get(key)?
            .iter()
            .rev()
            .find(|point| point.revision <= revision)
    }

    fn current_version(&self, key: &K) -> Option<Revision> {
        self.points
            .get(key)
            .and_then(|versions| versions.last())
            .map(|point| point.revision)
    }

    fn scope_version_at(&self, key: &OccScopeKey, revision: Revision) -> Option<Revision> {
        self.scope_versions
            .get(key)?
            .iter()
            .rev()
            .copied()
            .find(|version| *version <= revision)
    }

    fn current_scope_version(&self, key: &OccScopeKey) -> Option<Revision> {
        self.scope_versions
            .get(key)
            .and_then(|versions| versions.last().copied())
    }
}

/// One consuming point-read/write transaction.
#[derive(Clone, Debug)]
pub struct OccPointTransaction<K, V> {
    base_revision: Revision,
    operation_id: OperationId,
    reads: BTreeMap<K, Option<Revision>>,
    writes: BTreeMap<K, V>,
    write_versions: BTreeMap<K, Option<Revision>>,
    ranges: Vec<OccRangeDependency<K>>,
    scope_reads: BTreeMap<OccScopeKey, Option<Revision>>,
    scope_writes: std::collections::BTreeSet<OccScopeKey>,
}

#[derive(Clone, Debug)]
struct OccRangeDependency<K> {
    start: Bound<K>,
    end: Bound<K>,
}

/// Normalized predicate, schema-object, or HistorySpace-head dependency token.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OccScopeKey {
    /// Fingerprint of a normalized subject/predicate/context scope.
    Predicate(Vec<u8>),
    /// Stable key of one schema object whose selected definition may change.
    Schema(Vec<u8>),
    /// Stable key of one HistorySpace head and its parent/base metadata.
    HistorySpaceHead(Vec<u8>),
}

impl OccScopeKey {
    /// Creates a predicate-generation token.
    #[must_use]
    pub const fn predicate(fingerprint: Vec<u8>) -> Self {
        Self::Predicate(fingerprint)
    }
    /// Creates a schema-object generation token.
    #[must_use]
    pub const fn schema(object_key: Vec<u8>) -> Self {
        Self::Schema(object_key)
    }
    /// Creates a HistorySpace-head generation token.
    #[must_use]
    pub const fn history_space_head(space_key: Vec<u8>) -> Self {
        Self::HistorySpaceHead(space_key)
    }
}

impl<K: Ord> OccRangeDependency<K> {
    fn contains(&self, key: &K) -> bool {
        let after_start = match &self.start {
            Bound::Included(start) => key >= start,
            Bound::Excluded(start) => key > start,
            Bound::Unbounded => true,
        };
        let before_end = match &self.end {
            Bound::Included(end) => key <= end,
            Bound::Excluded(end) => key < end,
            Bound::Unbounded => true,
        };
        after_start && before_end
    }
}

fn clone_bound<K: Clone>(bound: Bound<&K>) -> Bound<K> {
    match bound {
        Bound::Included(value) => Bound::Included(value.clone()),
        Bound::Excluded(value) => Bound::Excluded(value.clone()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

impl<K, V> OccPointTransaction<K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    /// Pinned transaction base.
    #[must_use]
    pub const fn base_revision(&self) -> Revision {
        self.base_revision
    }

    /// Reads one point at the base revision and records its version or absence.
    pub fn read(&mut self, store: &OccPointStore<K, V>, key: &K) -> Option<V> {
        let observed = store
            .version_at(key, self.base_revision)
            .map(|point| point.revision);
        self.reads.entry(key.clone()).or_insert(observed);
        self.writes.get(key).cloned().or_else(|| {
            store
                .version_at(key, self.base_revision)
                .map(|point| point.value.clone())
        })
    }

    /// Stages a point write and captures the target's version or absence.
    pub fn write(
        &mut self,
        store: &OccPointStore<K, V>,
        key: K,
        value: V,
    ) -> Result<(), OccPointError> {
        self.write_with_scopes(store, key, value, [])
    }

    /// Stages a point write and records every predicate/schema/head generation it changes.
    pub fn write_with_scopes(
        &mut self,
        store: &OccPointStore<K, V>,
        key: K,
        value: V,
        changed_scopes: impl IntoIterator<Item = OccScopeKey>,
    ) -> Result<(), OccPointError> {
        if self.writes.contains_key(&key) {
            return Err(OccPointError::DuplicateWrite);
        }
        let observed = store
            .version_at(&key, self.base_revision)
            .map(|point| point.revision);
        self.write_versions.insert(key.clone(), observed);
        self.writes.insert(key, value);
        self.scope_writes.extend(changed_scopes);
        Ok(())
    }

    /// Reads all points in a bounded range and registers it for phantom checks.
    pub fn read_range<R>(&mut self, store: &OccPointStore<K, V>, range: R) -> Vec<(K, V)>
    where
        R: RangeBounds<K>,
    {
        let dependency = OccRangeDependency {
            start: clone_bound(range.start_bound()),
            end: clone_bound(range.end_bound()),
        };
        let mut rows = Vec::new();
        for key in store.points.keys().filter(|key| range.contains(key)) {
            let observed = store
                .version_at(key, self.base_revision)
                .map(|point| point.revision);
            self.reads.entry(key.clone()).or_insert(observed);
            if let Some(point) = store.version_at(key, self.base_revision) {
                rows.push((key.clone(), point.value.clone()));
            }
        }
        self.ranges.push(dependency);
        rows
    }

    /// Reads a versioned predicate, schema, or HistorySpace-head generation.
    pub fn read_scope(&mut self, store: &OccPointStore<K, V>, key: OccScopeKey) {
        let observed = store.scope_version_at(&key, self.base_revision);
        self.scope_reads.entry(key).or_insert(observed);
    }

    /// Revalidates all point dependencies and publishes every staged write once.
    pub fn commit(self, store: &mut OccPointStore<K, V>) -> Result<CommitOutcome, OccPointError> {
        if store.head < self.base_revision {
            return Err(OccPointError::BaseNotPublished {
                requested: self.base_revision,
                published: store.head,
            });
        }
        if self.writes.is_empty() {
            return Err(OccPointError::EmptyWriteSet);
        }

        let mut facts = Vec::new();
        for (key, observed) in &self.reads {
            if store.current_version(key) != *observed
                && !facts.contains(&ConflictFact::ReadDependencyChanged)
            {
                facts.push(ConflictFact::ReadDependencyChanged);
            }
        }
        for (key, observed) in &self.write_versions {
            if store.current_version(key) != *observed
                && !facts.contains(&ConflictFact::WriteTargetChanged)
            {
                facts.push(ConflictFact::WriteTargetChanged);
            }
        }
        let range_changed = self.ranges.iter().any(|range| {
            store
                .change_log
                .iter()
                .any(|(revision, key)| *revision > self.base_revision && range.contains(key))
        });
        if range_changed && !facts.contains(&ConflictFact::ReadDependencyChanged) {
            facts.push(ConflictFact::ReadDependencyChanged);
        }
        for (key, observed) in &self.scope_reads {
            if store.current_scope_version(key) != *observed
                && !facts.contains(&ConflictFact::ReadDependencyChanged)
            {
                facts.push(ConflictFact::ReadDependencyChanged);
            }
        }
        for key in &self.scope_writes {
            let observed = store.scope_version_at(key, self.base_revision);
            if store.current_scope_version(key) != observed
                && !facts.contains(&ConflictFact::WriteTargetChanged)
            {
                facts.push(ConflictFact::WriteTargetChanged);
            }
        }
        if !facts.is_empty() {
            return Ok(CommitOutcome::Conflict(ConflictReport::new(facts)));
        }

        let revision = store
            .head
            .next_commit()
            .map_err(OccPointError::RevisionExhausted)?;
        let mut candidate_points = store.points.clone();
        let mut candidate_change_log = store.change_log.clone();
        let mut candidate_scope_versions = store.scope_versions.clone();
        for (key, value) in self.writes {
            candidate_change_log.push((revision, key.clone()));
            candidate_points
                .entry(key)
                .or_default()
                .push(VersionedPoint { revision, value });
        }
        for key in self.scope_writes {
            candidate_scope_versions
                .entry(key)
                .or_default()
                .push(revision);
        }
        store.points = candidate_points;
        store.change_log = candidate_change_log;
        store.scope_versions = candidate_scope_versions;
        store.head = revision;
        Ok(CommitOutcome::Committed(crate::errors::CommitReceipt::new(
            self.operation_id,
            revision,
        )))
    }
}

/// Invalid base, duplicate/empty write set, or exhausted revision in the point model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OccPointError {
    /// The requested transaction base has not been published.
    BaseNotPublished {
        requested: Revision,
        published: Revision,
    },
    /// A transaction attempted to stage the same point twice.
    DuplicateWrite,
    /// A point transaction must publish at least one write.
    EmptyWriteSet,
    /// The shared revision could not advance.
    RevisionExhausted(RevisionError),
}

impl fmt::Display for OccPointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BaseNotPublished {
                requested,
                published,
            } => write!(
                formatter,
                "point transaction base {requested} is newer than published revision {published}"
            ),
            Self::DuplicateWrite => {
                formatter.write_str("point written more than once in one transaction")
            }
            Self::EmptyWriteSet => formatter.write_str("point transaction has no writes"),
            Self::RevisionExhausted(error) => {
                write!(formatter, "point revision exhausted: {error}")
            }
        }
    }
}

impl std::error::Error for OccPointError {}

#[cfg(test)]
mod tests {
    use super::{OccPointError, OccPointStore, OccScopeKey};
    use crate::errors::{CommitOutcome, ConflictFact};
    use crate::ids::{DomainId, OperationId, Revision};
    use std::collections::{BTreeMap, BTreeSet};

    fn operation_id() -> Result<OperationId, crate::ids::IdValidationError> {
        operation_id_for(7)
    }

    fn operation_id_for(sequence: u16) -> Result<OperationId, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[14] = (sequence >> 8) as u8;
        bytes[15] = sequence as u8;
        OperationId::try_from_bytes(bytes)
    }

    fn second_commit() -> Result<Revision, crate::ids::RevisionError> {
        Revision::FIRST_COMMIT.next_commit()
    }

    fn commit_is_conflict(outcome: CommitOutcome, fact: ConflictFact) -> bool {
        matches!(outcome, CommitOutcome::Conflict(report) if report.facts().contains(&fact))
    }

    #[test]
    fn concurrent_write_to_same_point_is_write_write_conflict() -> Result<(), String> {
        let mut store = OccPointStore::<&str, u8>::new();
        let mut first = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        let mut second = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        first.write(&store, "key", 1).map_err(|e| e.to_string())?;
        second.write(&store, "key", 2).map_err(|e| e.to_string())?;
        assert!(matches!(
            first.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));
        let conflict = second.commit(&mut store).map_err(|e| e.to_string())?;
        assert!(commit_is_conflict(
            conflict,
            ConflictFact::WriteTargetChanged
        ));
        assert_eq!(store.head(), Revision::FIRST_COMMIT);
        Ok(())
    }

    #[test]
    fn read_dependency_detects_changed_value_and_absence() -> Result<(), String> {
        let mut store = OccPointStore::<&str, u8>::new();
        let mut seed = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        seed.write(&store, "present", 1)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            seed.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));

        let mut value_reader = store
            .begin(
                Revision::FIRST_COMMIT,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        assert_eq!(value_reader.read(&store, &"present"), Some(1));
        let mut absence_reader = store
            .begin(
                Revision::FIRST_COMMIT,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        assert_eq!(absence_reader.read(&store, &"absent"), None);

        let mut concurrent = store
            .begin(
                Revision::FIRST_COMMIT,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        concurrent
            .write(&store, "present", 2)
            .map_err(|e| e.to_string())?;
        concurrent
            .write(&store, "absent", 3)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            concurrent.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));

        value_reader
            .write(&store, "other-a", 4)
            .map_err(|e| e.to_string())?;
        absence_reader
            .write(&store, "other-b", 5)
            .map_err(|e| e.to_string())?;
        assert!(commit_is_conflict(
            value_reader.commit(&mut store).map_err(|e| e.to_string())?,
            ConflictFact::ReadDependencyChanged,
        ));
        assert!(commit_is_conflict(
            absence_reader
                .commit(&mut store)
                .map_err(|e| e.to_string())?,
            ConflictFact::ReadDependencyChanged,
        ));
        assert_eq!(store.head(), second_commit().map_err(|e| e.to_string())?);
        Ok(())
    }

    #[test]
    fn disjoint_point_writes_can_commit_from_the_same_base() -> Result<(), String> {
        let mut store = OccPointStore::<&str, u8>::new();
        let mut first = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        let mut second = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        first.write(&store, "left", 1).map_err(|e| e.to_string())?;
        second
            .write(&store, "right", 2)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            first.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));
        assert!(matches!(
            second.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));
        assert_eq!(store.head(), second_commit().map_err(|e| e.to_string())?);
        Ok(())
    }

    #[test]
    fn duplicate_point_write_is_rejected_before_publication() -> Result<(), String> {
        let store = OccPointStore::<&str, u8>::new();
        let mut transaction = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        transaction
            .write(&store, "key", 1)
            .map_err(|e| e.to_string())?;
        assert_eq!(
            transaction.write(&store, "key", 2),
            Err(OccPointError::DuplicateWrite)
        );
        Ok(())
    }

    #[test]
    fn range_dependency_detects_new_phantom_key() -> Result<(), String> {
        let mut store = OccPointStore::<&str, u8>::new();
        let mut seed = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        seed.write(&store, "m", 1).map_err(|e| e.to_string())?;
        assert!(matches!(
            seed.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));

        let mut range_reader = store
            .begin(
                Revision::FIRST_COMMIT,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        assert_eq!(range_reader.read_range(&store, "a"..="z"), vec![("m", 1)]);
        let mut phantom_writer = store
            .begin(
                Revision::FIRST_COMMIT,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        phantom_writer
            .write(&store, "n", 2)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            phantom_writer.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));

        range_reader
            .write(&store, "outside", 3)
            .map_err(|e| e.to_string())?;
        assert!(commit_is_conflict(
            range_reader.commit(&mut store).map_err(|e| e.to_string())?,
            ConflictFact::ReadDependencyChanged,
        ));
        Ok(())
    }

    #[test]
    fn range_dependency_ignores_writes_outside_the_registered_range() -> Result<(), String> {
        let mut store = OccPointStore::<&str, u8>::new();
        let mut range_reader = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        assert!(range_reader.read_range(&store, "a"..="z").is_empty());
        let mut outside_writer = store
            .begin(
                Revision::GENESIS,
                operation_id().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        outside_writer
            .write(&store, "zzzz", 1)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            outside_writer.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));
        range_reader
            .write(&store, "local", 2)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            range_reader.commit(&mut store),
            Ok(CommitOutcome::Committed(_))
        ));
        Ok(())
    }

    #[test]
    fn predicate_schema_and_history_space_head_tokens_detect_phantoms() -> Result<(), String> {
        let scopes = [
            OccScopeKey::predicate(vec![1, 2]),
            OccScopeKey::schema(vec![3, 4]),
            OccScopeKey::history_space_head(vec![5, 6]),
        ];
        for scope in scopes {
            let mut store = OccPointStore::<&str, u8>::new();
            let mut reader = store
                .begin(
                    Revision::GENESIS,
                    operation_id().map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            reader.read_scope(&store, scope.clone());
            let mut writer = store
                .begin(
                    Revision::GENESIS,
                    operation_id().map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            writer
                .write_with_scopes(&store, "candidate", 1, [scope])
                .map_err(|e| e.to_string())?;
            assert!(matches!(
                writer.commit(&mut store),
                Ok(CommitOutcome::Committed(_))
            ));
            reader
                .write(&store, "unrelated", 2)
                .map_err(|e| e.to_string())?;
            assert!(commit_is_conflict(
                reader.commit(&mut store).map_err(|e| e.to_string())?,
                ConflictFact::ReadDependencyChanged,
            ));
        }
        Ok(())
    }

    #[derive(Clone, Debug)]
    struct ModelWrite {
        value: u8,
        changed_scopes: Vec<OccScopeKey>,
    }

    #[derive(Clone, Debug)]
    struct ModelTransaction {
        base_index: usize,
        reads: Vec<u8>,
        range: Option<(u8, u8)>,
        scope_reads: Vec<OccScopeKey>,
        writes: BTreeMap<u8, ModelWrite>,
    }

    #[derive(Clone, Debug)]
    struct ModelCommit {
        index: usize,
        keys: Vec<u8>,
    }

    #[derive(Default)]
    struct SerialReference {
        point_versions: BTreeMap<u8, Vec<(usize, u8)>>,
        scope_versions: BTreeMap<OccScopeKey, Vec<usize>>,
        commits: Vec<ModelCommit>,
    }

    impl SerialReference {
        fn point_version_at(&self, key: u8, index: usize) -> Option<usize> {
            self.point_versions
                .get(&key)?
                .iter()
                .rev()
                .find(|(version, _)| *version <= index)
                .map(|(version, _)| *version)
        }

        fn current_point_version(&self, key: u8) -> Option<usize> {
            self.point_versions
                .get(&key)
                .and_then(|versions| versions.last())
                .map(|(version, _)| *version)
        }

        fn value_at(&self, key: u8, index: usize) -> Option<u8> {
            self.point_versions
                .get(&key)?
                .iter()
                .rev()
                .find(|(version, _)| *version <= index)
                .map(|(_, value)| *value)
        }

        fn scope_version_at(&self, key: &OccScopeKey, index: usize) -> Option<usize> {
            self.scope_versions
                .get(key)?
                .iter()
                .rev()
                .copied()
                .find(|version| *version <= index)
        }

        fn current_scope_version(&self, key: &OccScopeKey) -> Option<usize> {
            self.scope_versions
                .get(key)
                .and_then(|versions| versions.last().copied())
        }

        fn range_at(&self, lower: u8, upper: u8, index: usize) -> Vec<(u8, u8)> {
            self.point_versions
                .keys()
                .filter_map(|key| {
                    (*key >= lower && *key <= upper)
                        .then(|| self.value_at(*key, index).map(|value| (*key, value)))
                        .flatten()
                })
                .collect()
        }

        fn expected_conflicts(&self, transaction: &ModelTransaction) -> Vec<ConflictFact> {
            let mut facts = Vec::new();
            let mut add = |fact| {
                if !facts.contains(&fact) {
                    facts.push(fact);
                }
            };

            if transaction.reads.iter().any(|key| {
                self.point_version_at(*key, transaction.base_index)
                    != self.current_point_version(*key)
            }) || transaction.range.is_some_and(|(lower, upper)| {
                self.commits.iter().any(|commit| {
                    commit.index > transaction.base_index
                        && commit.keys.iter().any(|key| *key >= lower && *key <= upper)
                })
            }) || transaction.scope_reads.iter().any(|key| {
                self.scope_version_at(key, transaction.base_index)
                    != self.current_scope_version(key)
            }) {
                add(ConflictFact::ReadDependencyChanged);
            }

            let changed_scopes: BTreeSet<_> = transaction
                .writes
                .values()
                .flat_map(|write| write.changed_scopes.iter().cloned())
                .collect();
            if transaction.writes.keys().any(|key| {
                self.point_version_at(*key, transaction.base_index)
                    != self.current_point_version(*key)
            }) || changed_scopes.iter().any(|key| {
                self.scope_version_at(key, transaction.base_index)
                    != self.current_scope_version(key)
            }) {
                add(ConflictFact::WriteTargetChanged);
            }
            facts
        }

        fn publish(&mut self, transaction: &ModelTransaction) -> usize {
            let index = self.commits.len() + 1;
            let mut changed_scopes = BTreeSet::new();
            for (key, write) in &transaction.writes {
                self.point_versions
                    .entry(*key)
                    .or_default()
                    .push((index, write.value));
                changed_scopes.extend(write.changed_scopes.iter().cloned());
            }
            for key in changed_scopes {
                self.scope_versions.entry(key).or_default().push(index);
            }
            self.commits.push(ModelCommit {
                index,
                keys: transaction.writes.keys().copied().collect(),
            });
            index
        }
    }

    struct SeededRandom(u64);

    impl SeededRandom {
        fn next(&mut self) -> u64 {
            // xorshift64*: small, deterministic, and dependency-free for replayable schedules.
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn below(&mut self, bound: usize) -> usize {
            (self.next() % bound as u64) as usize
        }

        fn scope(&mut self) -> OccScopeKey {
            let token = self.below(5) as u8;
            match self.below(3) {
                0 => OccScopeKey::predicate(vec![token]),
                1 => OccScopeKey::schema(vec![token]),
                _ => OccScopeKey::history_space_head(vec![token]),
            }
        }
    }

    fn random_transaction(random: &mut SeededRandom, head_index: usize) -> ModelTransaction {
        let base_index = random.below(head_index + 1);
        let reads: BTreeSet<_> = (0..random.below(4))
            .map(|_| random.below(8) as u8)
            .collect();
        let range = if random.below(3) != 0 {
            let lower = random.below(7) as u8;
            let upper = lower + random.below(8 - usize::from(lower)) as u8;
            Some((lower, upper))
        } else {
            None
        };
        let scope_reads: BTreeSet<_> = (0..random.below(3)).map(|_| random.scope()).collect();
        let write_count = 1 + random.below(3);
        let mut writes = BTreeMap::new();
        while writes.len() < write_count {
            let key = random.below(8) as u8;
            let changed_scopes: BTreeSet<_> =
                (0..random.below(3)).map(|_| random.scope()).collect();
            writes.entry(key).or_insert_with(|| ModelWrite {
                value: random.below(16) as u8,
                changed_scopes: changed_scopes.into_iter().collect(),
            });
        }
        ModelTransaction {
            base_index,
            reads: reads.into_iter().collect(),
            range,
            scope_reads: scope_reads.into_iter().collect(),
            writes,
        }
    }

    #[test]
    fn seeded_occ_histories_match_serial_reference_and_replay_exactly() -> Result<(), String> {
        const SEEDS: [u64; 5] = [1, 7, 0x5eed, 0xdecafbad, 0x5eed_cafe_f00d];

        for seed in SEEDS {
            let mut traces = Vec::new();
            for _replay in 0..2 {
                let mut operation_sequence = 1_u16;
                let mut trace = Vec::new();
                let mut random = SeededRandom(seed);
                let mut store = OccPointStore::<u8, u8>::new();
                let mut reference = SerialReference::default();
                let mut revisions = vec![Revision::GENESIS];

                for batch in 0..64 {
                    let transaction_count = 2 + random.below(4);
                    let transactions: Vec<_> = (0..transaction_count)
                        .map(|_| random_transaction(&mut random, reference.commits.len()))
                        .collect();
                    let mut commit_order: Vec<_> = (0..transaction_count).collect();
                    for index in (1..commit_order.len()).rev() {
                        let other = random.below(index + 1);
                        commit_order.swap(index, other);
                    }

                    for transaction_index in commit_order {
                        let spec = transactions.get(transaction_index).ok_or_else(|| {
                            format!(
                                "seed={seed:#x}, batch={batch}: transaction index {transaction_index} is missing"
                            )
                        })?;
                        let operation_number = operation_sequence;
                        let mut transaction = store
                            .begin(
                                revisions.get(spec.base_index).copied().ok_or_else(|| {
                                    format!(
                                        "seed={seed:#x}, batch={batch}, transaction={transaction_index}: base index {} is missing",
                                        spec.base_index
                                    )
                                })?,
                                operation_id_for(operation_sequence)
                                    .map_err(|error| error.to_string())?,
                            )
                            .map_err(|error| error.to_string())?;
                        operation_sequence = operation_sequence.wrapping_add(1);

                        for key in &spec.reads {
                            assert_eq!(
                                transaction.read(&store, key),
                                reference.value_at(*key, spec.base_index),
                                "seed={seed:#x}, batch={batch}, transaction={transaction_index}, point={key}"
                            );
                        }
                        if let Some((lower, upper)) = spec.range {
                            assert_eq!(
                                transaction.read_range(&store, lower..=upper),
                                reference.range_at(lower, upper, spec.base_index),
                                "seed={seed:#x}, batch={batch}, transaction={transaction_index}, range={lower}..={upper}"
                            );
                        }
                        for scope in &spec.scope_reads {
                            transaction.read_scope(&store, scope.clone());
                        }
                        for (key, write) in &spec.writes {
                            transaction
                                .write_with_scopes(
                                    &store,
                                    *key,
                                    write.value,
                                    write.changed_scopes.clone(),
                                )
                                .map_err(|error| error.to_string())?;
                        }

                        let expected_facts = reference.expected_conflicts(spec);
                        let expected_operation =
                            operation_id_for(operation_sequence.wrapping_sub(1))
                                .map_err(|error| error.to_string())?;
                        let expected_revision = store
                            .head()
                            .next_commit()
                            .map_err(|error| error.to_string())?;
                        let actual = transaction
                            .commit(&mut store)
                            .map_err(|error| error.to_string())?;
                        if expected_facts.is_empty() {
                            match actual {
                                CommitOutcome::Committed(receipt) => {
                                    assert_eq!(receipt.operation_id(), expected_operation);
                                    assert_eq!(receipt.revision(), expected_revision);
                                    trace.push(format!(
                                        "{operation_number}:commit:{:?}",
                                        receipt.revision()
                                    ));
                                }
                                CommitOutcome::Conflict(report) => {
                                    return Err(format!(
                                        "seed={seed:#x}, batch={batch}, transaction={transaction_index}: unexpected conflict {:?}",
                                        report.facts()
                                    ));
                                }
                            }
                            let published_index = reference.publish(spec);
                            assert_eq!(published_index, revisions.len());
                            revisions.push(expected_revision);
                        } else {
                            match actual {
                                CommitOutcome::Conflict(report) => {
                                    assert_eq!(
                                        report.facts().len(),
                                        expected_facts.len(),
                                        "seed={seed:#x}, batch={batch}, transaction={transaction_index}"
                                    );
                                    assert!(
                                        expected_facts
                                            .iter()
                                            .all(|fact| report.facts().contains(fact)),
                                        "seed={seed:#x}, batch={batch}, transaction={transaction_index}: actual {:?}, expected {expected_facts:?}",
                                        report.facts()
                                    );
                                    trace.push(format!(
                                        "{operation_number}:conflict:{:?}",
                                        report.facts()
                                    ));
                                }
                                CommitOutcome::Committed(receipt) => {
                                    return Err(format!(
                                        "seed={seed:#x}, batch={batch}, transaction={transaction_index}: reference expected {expected_facts:?}, committed at {}",
                                        receipt.revision()
                                    ));
                                }
                            }
                        }
                        assert_eq!(
                            store.head(),
                            revisions.last().copied().ok_or_else(|| {
                                "genesis revision unexpectedly missing".to_owned()
                            })?
                        );
                    }
                }
                traces.push(trace);
            }
            let (first, second) = traces
                .first()
                .zip(traces.get(1))
                .ok_or_else(|| format!("seed={seed:#x}: replay trace is missing"))?;
            assert_eq!(first, second, "seed={seed:#x} did not replay exactly");
        }
        Ok(())
    }
}
