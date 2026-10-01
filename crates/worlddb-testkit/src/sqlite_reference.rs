//! In-memory SQLite implementation of the logical revision contract.
//!
//! This adapter is a test/prototype reference only. It is not a WorldDB file
//! format, does not implement the sealed production storage port, and makes
//! no process- or machine-durability claim.

use std::fmt;

use rusqlite::{Connection, Transaction, params};
use worlddb_core::{
    CancellablePublishError, CommitCancellation, Revision, RevisionBackend, RevisionLogError,
};

/// A SQLite setup failure while constructing the logical reference backend.
#[derive(Debug)]
pub struct SqliteReferenceError {
    source: rusqlite::Error,
}

impl fmt::Display for SqliteReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "SQLite reference backend setup failed: {}",
            self.source
        )
    }
}

impl std::error::Error for SqliteReferenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// An in-memory SQLite model for ordered byte-valued revision batches.
///
/// SQLite stores the head and entries in transactional tables. A compact
/// immutable cache supplies the borrowed iterator required by
/// [`RevisionBackend`]; every historical read checks its full ordered SQL
/// result against that cache before returning borrowed values. Revisions use
/// unsigned big-endian 8-byte blobs so SQLite's byte ordering preserves the
/// full valid `u64` revision order.
pub struct SqliteReferenceBackend {
    connection: Connection,
    latest: Revision,
    commits: Vec<ReferenceCommit>,
}

struct ReferenceCommit {
    revision: Revision,
    entries: Vec<u8>,
}

impl SqliteReferenceBackend {
    /// Opens a fresh in-memory SQLite reference database at Genesis.
    pub fn open_in_memory() -> Result<Self, SqliteReferenceError> {
        let connection =
            Connection::open_in_memory().map_err(|source| SqliteReferenceError { source })?;
        connection
            .execute_batch(
                "CREATE TABLE worlddb_head (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    revision BLOB NOT NULL CHECK (
                        typeof(revision) = 'blob'
                        AND length(revision) = 8
                        AND revision <> X'FFFFFFFFFFFFFFFF'
                    )
                );
                INSERT INTO worlddb_head (singleton, revision)
                    VALUES (1, X'0000000000000000');
                CREATE TABLE worlddb_entries (
                    revision BLOB NOT NULL CHECK (
                        typeof(revision) = 'blob'
                        AND length(revision) = 8
                        AND revision <> X'0000000000000000'
                        AND revision <> X'FFFFFFFFFFFFFFFF'
                    ),
                    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
                    value INTEGER NOT NULL CHECK (value BETWEEN 0 AND 255),
                    PRIMARY KEY (revision, ordinal)
                ) WITHOUT ROWID;",
            )
            .map_err(|source| SqliteReferenceError { source })?;
        Ok(Self {
            connection,
            latest: Revision::GENESIS,
            commits: Vec::new(),
        })
    }

    fn publish_batch(&mut self, entries: Vec<u8>) -> Result<Revision, RevisionLogError> {
        let revision = self.latest.next_commit()?;
        self.commits
            .try_reserve(1)
            .map_err(|_| RevisionLogError::BackendFailure)?;
        let transaction = stage_batch(&mut self.connection, self.latest, revision, &entries)?;
        transaction
            .commit()
            .map_err(|_| RevisionLogError::BackendFailure)?;
        self.commits.push(ReferenceCommit { revision, entries });
        self.latest = revision;
        Ok(revision)
    }
}

impl RevisionBackend<u8> for SqliteReferenceBackend {
    type Read<'a>
        = std::vec::IntoIter<(Revision, &'a u8)>
    where
        Self: 'a;

    fn latest_published(&self) -> Revision {
        self.latest
    }

    fn publish(&mut self, entries: Vec<u8>) -> Result<Revision, RevisionLogError> {
        self.publish_batch(entries)
    }

    fn publish_cancellable(
        &mut self,
        entries: Vec<u8>,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        let revision = self
            .latest
            .next_commit()
            .map_err(RevisionLogError::from)
            .map_err(CancellablePublishError::Publish)?;
        self.commits
            .try_reserve(1)
            .map_err(|_| CancellablePublishError::Publish(RevisionLogError::BackendFailure))?;
        let transaction = stage_batch(&mut self.connection, self.latest, revision, &entries)
            .map_err(CancellablePublishError::Publish)?;
        let permit = cancellation
            .begin_commitpoint()
            .map_err(CancellablePublishError::Commitpoint)?;
        match transaction.commit() {
            Ok(()) => {
                self.commits.push(ReferenceCommit { revision, entries });
                self.latest = revision;
                permit.committed();
                Ok(revision)
            }
            Err(_) => {
                permit.not_committed();
                Err(CancellablePublishError::Publish(
                    RevisionLogError::BackendFailure,
                ))
            }
        }
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
        if revision > self.latest {
            return Err(RevisionLogError::RevisionNotPublished {
                requested: revision,
                published: self.latest,
            });
        }
        let expected_head = self.latest.value().to_be_bytes().to_vec();
        let stored_head = self
            .connection
            .query_row(
                "SELECT revision FROM worlddb_head WHERE singleton = 1",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .map_err(|_| RevisionLogError::BackendFailure)?;
        if stored_head != expected_head {
            return Err(RevisionLogError::BackendFailure);
        }
        let revision_bound = revision.value().to_be_bytes().to_vec();
        let expected_count = self
            .commits
            .iter()
            .filter(|commit| commit.revision <= revision)
            .try_fold(0_usize, |count, commit| {
                count.checked_add(commit.entries.len())
            })
            .ok_or(RevisionLogError::BackendFailure)?;
        let mut expected = Vec::new();
        expected
            .try_reserve_exact(expected_count)
            .map_err(|_| RevisionLogError::BackendFailure)?;
        for commit in self
            .commits
            .iter()
            .filter(|commit| commit.revision <= revision)
        {
            let stored_revision = commit.revision.value().to_be_bytes().to_vec();
            for (ordinal, value) in commit.entries.iter().enumerate() {
                let stored_ordinal =
                    i64::try_from(ordinal).map_err(|_| RevisionLogError::BackendFailure)?;
                expected.push((stored_revision.clone(), stored_ordinal, i64::from(*value)));
            }
        }

        let mut statement = self
            .connection
            .prepare(
                "SELECT revision, ordinal, value FROM worlddb_entries \
                 WHERE revision <= ?1 ORDER BY revision, ordinal",
            )
            .map_err(|_| RevisionLogError::BackendFailure)?;
        let rows = statement
            .query_map(params![revision_bound], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|_| RevisionLogError::BackendFailure)?;
        let mut actual = Vec::new();
        actual
            .try_reserve_exact(expected_count)
            .map_err(|_| RevisionLogError::BackendFailure)?;
        for row in rows {
            actual.push(row.map_err(|_| RevisionLogError::BackendFailure)?);
        }
        if actual != expected {
            return Err(RevisionLogError::BackendFailure);
        }

        let mut borrowed = Vec::new();
        borrowed
            .try_reserve_exact(expected_count)
            .map_err(|_| RevisionLogError::BackendFailure)?;
        for commit in self
            .commits
            .iter()
            .filter(|commit| commit.revision <= revision)
        {
            borrowed.extend(commit.entries.iter().map(|entry| (commit.revision, entry)));
        }
        Ok(borrowed.into_iter())
    }
}

fn stage_batch<'connection>(
    connection: &'connection mut Connection,
    current_revision: Revision,
    revision: Revision,
    entries: &[u8],
) -> Result<Transaction<'connection>, RevisionLogError> {
    let stored_revision = revision.value().to_be_bytes().to_vec();
    let expected_head = current_revision.value().to_be_bytes().to_vec();
    let transaction = connection
        .transaction()
        .map_err(|_| RevisionLogError::BackendFailure)?;
    for (ordinal, value) in entries.iter().enumerate() {
        let stored_ordinal =
            i64::try_from(ordinal).map_err(|_| RevisionLogError::BackendFailure)?;
        transaction
            .execute(
                "INSERT INTO worlddb_entries (revision, ordinal, value) VALUES (?1, ?2, ?3)",
                params![stored_revision, stored_ordinal, i64::from(*value)],
            )
            .map_err(|_| RevisionLogError::BackendFailure)?;
    }
    let updated = transaction
        .execute(
            "UPDATE worlddb_head SET revision = ?1 WHERE singleton = 1 AND revision = ?2",
            params![stored_revision, expected_head],
        )
        .map_err(|_| RevisionLogError::BackendFailure)?;
    if updated != 1 {
        return Err(RevisionLogError::BackendFailure);
    }
    Ok(transaction)
}

#[cfg(test)]
mod tests {
    use super::{ReferenceCommit, SqliteReferenceBackend};
    use rusqlite::params;
    use worlddb_core::{
        CancellablePublishError, CommitCancellation, CommitCancellationState, Revision,
        RevisionBackend, RevisionLogError,
    };

    #[test]
    fn sql_constraint_failure_rolls_back_the_entire_batch_and_head() -> Result<(), String> {
        let mut backend =
            SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
        backend
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_marker BEFORE INSERT ON worlddb_entries \
                 WHEN NEW.value = 255 BEGIN SELECT RAISE(ABORT, 'injected reject'); END;",
            )
            .map_err(|error| error.to_string())?;

        assert_eq!(
            backend.publish(vec![7, 255]),
            Err(RevisionLogError::BackendFailure)
        );
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(
            backend
                .read_at(Revision::GENESIS)
                .map_err(|error| error.to_string())?
                .next()
                .is_none()
        );

        backend
            .connection
            .execute_batch("DROP TRIGGER reject_marker;")
            .map_err(|error| error.to_string())?;
        assert_eq!(backend.publish(vec![3]), Ok(Revision::FIRST_COMMIT));
        Ok(())
    }

    #[test]
    fn cancellation_before_commitpoint_publishes_nothing() -> Result<(), String> {
        let mut backend =
            SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
        let cancellation = CommitCancellation::new();
        cancellation.request_cancellation();
        assert!(matches!(
            backend.publish_cancellable(vec![1, 2], &cancellation),
            Err(CancellablePublishError::Commitpoint(_))
        ));
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(
            backend
                .read_at(Revision::GENESIS)
                .map_err(|error| error.to_string())?
                .next()
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn cancellable_publish_enters_and_completes_the_sql_commitpoint() -> Result<(), String> {
        let mut backend =
            SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
        let cancellation = CommitCancellation::new();
        assert_eq!(
            backend
                .publish_cancellable(vec![5, 8], &cancellation)
                .map_err(|error| error.to_string())?,
            Revision::FIRST_COMMIT
        );
        assert_eq!(cancellation.state(), CommitCancellationState::Committed);
        assert_eq!(
            backend
                .read_at(Revision::FIRST_COMMIT)
                .map_err(|error| error.to_string())?
                .map(|(revision, value)| (revision, *value))
                .collect::<Vec<_>>(),
            vec![(Revision::FIRST_COMMIT, 5), (Revision::FIRST_COMMIT, 8)]
        );
        Ok(())
    }

    #[test]
    fn historical_read_fails_when_sqlite_and_cache_disagree() -> Result<(), String> {
        let mut backend =
            SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
        backend
            .publish(vec![11])
            .map_err(|error| error.to_string())?;
        let revision_key = Revision::FIRST_COMMIT.value().to_be_bytes().to_vec();
        backend
            .connection
            .execute(
                "UPDATE worlddb_entries SET value = 12 WHERE revision = ?1 AND ordinal = 0",
                params![revision_key],
            )
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            backend.read_at(Revision::FIRST_COMMIT),
            Err(RevisionLogError::BackendFailure)
        ));
        Ok(())
    }

    #[test]
    fn revision_order_and_publish_cross_the_signed_sqlite_integer_boundary() -> Result<(), String> {
        let mut backend =
            SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
        let previous = Revision::new(i64::MAX as u64).map_err(|error| error.to_string())?;
        let encoded = previous.value().to_be_bytes().to_vec();
        backend
            .connection
            .execute(
                "INSERT INTO worlddb_entries (revision, ordinal, value) VALUES (?1, ?2, ?3)",
                params![encoded.clone(), 0_i64, 41_i64],
            )
            .map_err(|error| error.to_string())?;
        backend
            .connection
            .execute(
                "UPDATE worlddb_head SET revision = ?1 WHERE singleton = 1",
                params![encoded],
            )
            .map_err(|error| error.to_string())?;
        backend.commits.push(ReferenceCommit {
            revision: previous,
            entries: vec![41],
        });
        backend.latest = previous;

        let next = backend
            .publish(vec![42])
            .map_err(|error| error.to_string())?;
        assert_eq!(next.value(), (i64::MAX as u64) + 1);
        assert_eq!(
            backend
                .read_at(next)
                .map_err(|error| error.to_string())?
                .map(|(revision, value)| (revision, *value))
                .collect::<Vec<_>>(),
            vec![(previous, 41), (next, 42)]
        );
        Ok(())
    }
}
