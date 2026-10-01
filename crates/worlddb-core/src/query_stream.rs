//! Terminal candidate-stream semantics for query execution.

use crate::errors::QueryError;

/// Failure emitted by an already-constructed candidate stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QueryItemError<E> {
    /// A source/core item failed. This always terminates the stream.
    Item(E),
    /// The query was cancelled. This is distinct from construction and item failures.
    Cancelled,
    /// The query exhausted its semantic budget. This is distinct from cancellation.
    BudgetExceeded,
    /// An explicitly skippable diagnostic item; not a domain result and does not terminate.
    SkippableDiagnostic,
}

/// Iterator wrapper that enforces terminal errors and `None` end-of-stream.
#[derive(Clone, Debug)]
pub(crate) struct CandidateStream<I> {
    inner: I,
    terminal: bool,
}

impl<I> CandidateStream<I> {
    /// Constructs a stream; failure here is a query-construction `QueryError`.
    pub(crate) fn try_construct(
        factory: impl FnOnce() -> Result<I, QueryError>,
    ) -> Result<Self, QueryError> {
        Ok(Self {
            inner: factory()?,
            terminal: false,
        })
    }

    /// Builds a stream of diagnostics or adapter items after successful construction.
    pub(crate) fn from_items(inner: I) -> Self {
        Self {
            inner,
            terminal: false,
        }
    }

    /// Builds a core-history stream. Its item failures are always non-skippable.
    pub(crate) fn try_from_core_history<T, E>(
        factory: impl FnOnce() -> Result<I, QueryError>,
    ) -> Result<CandidateStream<impl Iterator<Item = Result<T, QueryItemError<E>>>>, QueryError>
    where
        I: Iterator<Item = Result<T, E>>,
    {
        let inner = factory()?.map(map_core_history_item::<T, E>);
        Ok(CandidateStream::from_items(inner))
    }
}

fn map_core_history_item<T, E>(item: Result<T, E>) -> Result<T, QueryItemError<E>> {
    item.map_err(QueryItemError::Item)
}

impl<I, T, E> Iterator for CandidateStream<I>
where
    I: Iterator<Item = Result<T, QueryItemError<E>>>,
{
    type Item = Result<T, QueryItemError<E>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terminal {
            return None;
        }
        match self.inner.next() {
            Some(item @ Err(QueryItemError::SkippableDiagnostic)) => Some(item),
            Some(item @ Err(_)) => {
                self.terminal = true;
                Some(item)
            }
            Some(item @ Ok(_)) => Some(item),
            None => {
                self.terminal = true;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CandidateStream, QueryItemError};
    use crate::errors::QueryError;

    #[test]
    fn construction_failure_is_separate_from_stream_items() {
        let result =
            CandidateStream::<std::iter::Empty<Result<u8, QueryItemError<()>>>>::try_construct(
                || Err(QueryError::InvalidQuery),
            );
        assert!(matches!(result, Err(QueryError::InvalidQuery)));
    }

    #[test]
    fn item_failure_is_returned_once_then_stream_ends_with_none() {
        let items = [
            Ok(1_u8),
            Err(QueryItemError::Item("corrupt history")),
            Ok(2_u8),
        ];
        let mut stream = CandidateStream::from_items(items.into_iter());
        assert_eq!(stream.next(), Some(Ok(1)));
        assert_eq!(
            stream.next(),
            Some(Err(QueryItemError::Item("corrupt history")))
        );
        assert_eq!(stream.next(), None);
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn core_history_constructor_cannot_mark_item_errors_skippable() {
        let mut stream = CandidateStream::try_from_core_history(|| {
            Ok([Ok(1_u8), Err("bad history"), Ok(2)].into_iter())
        });
        assert!(stream.is_ok());
        if let Ok(ref mut stream) = stream {
            assert_eq!(stream.next(), Some(Ok(1)));
            assert_eq!(
                stream.next(),
                Some(Err(QueryItemError::Item("bad history")))
            );
            assert_eq!(stream.next(), None);
        }
    }

    #[test]
    fn cancellation_budget_and_skippable_diagnostics_have_distinct_semantics() {
        for terminal in [QueryItemError::Cancelled, QueryItemError::BudgetExceeded] {
            let mut stream = CandidateStream::from_items(
                [Err(terminal.clone()), Ok::<u8, QueryItemError<()>>(1)].into_iter(),
            );
            assert_eq!(stream.next(), Some(Err(terminal)));
            assert_eq!(stream.next(), None);
        }
        let mut diagnostic = CandidateStream::from_items(
            [
                Err(QueryItemError::SkippableDiagnostic),
                Ok::<u8, QueryItemError<()>>(1),
            ]
            .into_iter(),
        );
        assert_eq!(
            diagnostic.next(),
            Some(Err(QueryItemError::SkippableDiagnostic))
        );
        assert_eq!(diagnostic.next(), Some(Ok(1)));
        assert_eq!(diagnostic.next(), None);
    }

    #[test]
    fn natural_end_is_none_and_not_an_error_item() {
        let mut stream =
            CandidateStream::from_items(std::iter::empty::<Result<u8, QueryItemError<()>>>());
        assert_eq!(stream.next(), None);
    }
}
