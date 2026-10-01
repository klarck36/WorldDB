//! Slow, index-free reference model for published database revisions.

use std::fmt;
use std::slice;

use crate::ids::{Revision, RevisionError};

/// One immutable published commit in revision order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedCommit<T> {
    revision: Revision,
    entries: Vec<T>,
}

impl<T> PublishedCommit<T> {
    /// Returns the contiguous revision assigned to this published commit.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Returns the commit's immutable insertion-ordered entries.
    #[must_use]
    pub fn entries(&self) -> &[T] {
        &self.entries
    }
}

/// An in-memory append-only sequence used as an index-free history oracle.
///
/// Reservations are private to the store and do not advance its published head.
/// The only way to publish is to commit the exact next reserved revision. Reads
/// walk the commit vector in insertion order and never consult an index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryRevisionLog<T> {
    latest_published: Revision,
    commits: Vec<PublishedCommit<T>>,
    reservation: Option<Revision>,
}

impl<T> Default for InMemoryRevisionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> InMemoryRevisionLog<T> {
    /// Creates an empty history at the Genesis revision.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            latest_published: Revision::GENESIS,
            commits: Vec::new(),
            reservation: None,
        }
    }

    /// Returns the latest published revision; unpublished reservations are hidden.
    #[must_use]
    pub const fn latest_published(&self) -> Revision {
        self.latest_published
    }

    /// Returns commits in their immutable, contiguous publication order.
    #[must_use]
    pub fn commits(&self) -> &[PublishedCommit<T>] {
        &self.commits
    }

    /// Reserves exactly the next revision without making it readable.
    pub fn reserve_next(&mut self) -> Result<Revision, RevisionLogError> {
        if let Some(revision) = self.reservation {
            return Err(RevisionLogError::ReservationAlreadyOpen { revision });
        }
        let revision = self.latest_published.next_commit()?;
        self.reservation = Some(revision);
        Ok(revision)
    }

    /// Publishes entries at the current reservation, preserving insertion order.
    pub fn publish(&mut self, revision: Revision, entries: Vec<T>) -> Result<(), RevisionLogError> {
        let expected = self.reservation.ok_or(RevisionLogError::NoReservation)?;
        if revision != expected {
            return Err(RevisionLogError::RevisionGap {
                expected,
                actual: revision,
            });
        }
        let next = self.latest_published.next_commit()?;
        if revision != next {
            return Err(RevisionLogError::RevisionGap {
                expected: next,
                actual: revision,
            });
        }

        self.commits.push(PublishedCommit { revision, entries });
        self.latest_published = revision;
        self.reservation = None;
        Ok(())
    }

    /// Cancels the unpublished reservation so the same revision can be retried.
    pub fn cancel_reservation(&mut self) -> Result<Revision, RevisionLogError> {
        self.reservation
            .take()
            .ok_or(RevisionLogError::NoReservation)
    }

    /// Reads all entries published through `as_of` in deterministic commit order.
    pub fn read_at(&self, as_of: Revision) -> Result<HistoricalRead<'_, T>, RevisionLogError> {
        if as_of > self.latest_published {
            return Err(RevisionLogError::RevisionNotPublished {
                requested: as_of,
                published: self.latest_published,
            });
        }
        Ok(HistoricalRead {
            as_of,
            commits: self.commits.iter(),
            current_revision: Revision::GENESIS,
            current_entries: None,
        })
    }
}

/// A borrowed, deterministic scan of commits up to one published revision.
pub struct HistoricalRead<'a, T> {
    as_of: Revision,
    commits: slice::Iter<'a, PublishedCommit<T>>,
    current_revision: Revision,
    current_entries: Option<slice::Iter<'a, T>>,
}

impl<'a, T> Iterator for HistoricalRead<'a, T> {
    type Item = (Revision, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(entries) = &mut self.current_entries {
                if let Some(entry) = entries.next() {
                    return Some((self.current_revision, entry));
                }
            }

            let commit = self.commits.next()?;
            if commit.revision > self.as_of {
                return None;
            }
            self.current_revision = commit.revision;
            self.current_entries = Some(commit.entries.iter());
        }
    }
}

/// Invalid reservation use, revision gaps, unpublished reads, or exhaustion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionLogError {
    /// A backend could not complete or verify its logical revision operation.
    BackendFailure,
    /// A reservation is already open for this revision.
    ReservationAlreadyOpen { revision: Revision },
    /// Publication was attempted without a reservation.
    NoReservation,
    /// A caller attempted to publish something other than the next reservation.
    RevisionGap {
        expected: Revision,
        actual: Revision,
    },
    /// The requested read revision has not been published.
    RevisionNotPublished {
        requested: Revision,
        published: Revision,
    },
    /// No later revision can be reserved without reaching the reserved maximum.
    RevisionExhausted(RevisionError),
}

impl From<RevisionError> for RevisionLogError {
    fn from(error: RevisionError) -> Self {
        Self::RevisionExhausted(error)
    }
}

impl fmt::Display for RevisionLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendFailure => formatter.write_str("revision backend operation failed"),
            Self::ReservationAlreadyOpen { revision } => {
                write!(formatter, "revision {revision} is already reserved")
            }
            Self::NoReservation => formatter.write_str("there is no reserved revision"),
            Self::RevisionGap { expected, actual } => write!(
                formatter,
                "cannot publish revision {actual}; the next revision is {expected}"
            ),
            Self::RevisionNotPublished {
                requested,
                published,
            } => write!(
                formatter,
                "revision {requested} is not published; latest is {published}"
            ),
            Self::RevisionExhausted(error) => {
                write!(formatter, "revision space exhausted: {error}")
            }
        }
    }
}

impl std::error::Error for RevisionLogError {}

#[cfg(test)]
mod tests {
    use super::{InMemoryRevisionLog, RevisionLogError};
    use crate::ids::{Revision, RevisionError};

    #[test]
    fn unpublished_reservations_are_invisible_and_cancel_without_gaps()
    -> Result<(), RevisionLogError> {
        let mut log = InMemoryRevisionLog::<u8>::new();
        assert_eq!(log.latest_published(), Revision::GENESIS);
        assert!(log.commits().is_empty());

        let first = log.reserve_next()?;
        assert_eq!(first, Revision::FIRST_COMMIT);
        assert_eq!(log.latest_published(), Revision::GENESIS);
        assert_eq!(
            log.read_at(first).err(),
            Some(RevisionLogError::RevisionNotPublished {
                requested: first,
                published: Revision::GENESIS,
            })
        );
        assert!(log.read_at(Revision::GENESIS)?.next().is_none());

        log.publish(first, vec![10, 11])?;
        let second = log.reserve_next()?;
        assert_eq!(second.value(), first.value() + 1);
        assert_eq!(log.latest_published(), first);
        assert_eq!(log.cancel_reservation()?, second);

        let retried = log.reserve_next()?;
        assert_eq!(retried, second);
        log.publish(retried, vec![20])?;
        assert_eq!(log.latest_published(), second);
        assert_eq!(
            log.commits()
                .iter()
                .map(|commit| commit.revision())
                .collect::<Vec<_>>(),
            vec![Revision::FIRST_COMMIT, second]
        );

        let at_first = log.read_at(first)?.collect::<Vec<_>>();
        let at_first_again = log.read_at(first)?.collect::<Vec<_>>();
        assert_eq!(at_first, at_first_again);
        assert_eq!(at_first, vec![(first, &10_u8), (first, &11_u8)]);
        assert_eq!(
            log.read_at(second)?.collect::<Vec<_>>(),
            vec![(first, &10_u8), (first, &11_u8), (second, &20_u8)]
        );
        assert!(log.read_at(Revision::GENESIS)?.next().is_none());
        Ok(())
    }

    #[test]
    fn publication_rejects_missing_reservation_and_revision_gaps() -> Result<(), RevisionLogError> {
        let mut log = InMemoryRevisionLog::<u8>::new();
        assert_eq!(
            log.publish(Revision::FIRST_COMMIT, vec![1]),
            Err(RevisionLogError::NoReservation)
        );

        let first = log.reserve_next()?;
        assert_eq!(
            log.reserve_next(),
            Err(RevisionLogError::ReservationAlreadyOpen { revision: first })
        );
        let gap = Revision::new(2)?;
        assert_eq!(
            log.publish(gap, vec![2]),
            Err(RevisionLogError::RevisionGap {
                expected: first,
                actual: gap,
            })
        );
        assert_eq!(log.latest_published(), Revision::GENESIS);
        assert_eq!(log.cancel_reservation()?, first);
        assert_eq!(log.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[test]
    fn transaction_revision_order_does_not_follow_domain_value_order()
    -> Result<(), RevisionLogError> {
        let mut log = InMemoryRevisionLog::new();
        let first = log.reserve_next()?;
        log.publish(first, vec![9_u8])?;
        let second = log.reserve_next()?;
        log.publish(second, vec![1_u8])?;

        assert!(first < second);
        assert_eq!(
            log.read_at(second)?.collect::<Vec<_>>(),
            vec![(first, &9_u8), (second, &1_u8)]
        );
        Ok(())
    }

    #[test]
    fn revision_exhaustion_never_publishes_reserved_maximum() -> Result<(), RevisionLogError> {
        let last_before_reserved = Revision::new(u64::MAX - 1)?;
        let mut log = InMemoryRevisionLog::<()> {
            latest_published: last_before_reserved,
            commits: Vec::new(),
            reservation: None,
        };
        assert_eq!(
            log.reserve_next(),
            Err(RevisionLogError::RevisionExhausted(
                RevisionError::ReservedMaximum
            ))
        );
        assert_eq!(log.latest_published(), last_before_reserved);
        assert!(log.commits().is_empty());
        Ok(())
    }
}
