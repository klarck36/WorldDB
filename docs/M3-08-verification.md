# M3-08 verification: candidate stream terminal semantics

`CandidateStream` is the engine-internal iterator wrapper for candidate production. Construction returns `Result<CandidateStream<_>, QueryError>`; after construction, `Iterator::next` returns `Some(Result<_, QueryItemError<_>>)` for items and failures, and returns `None` for end-of-stream. Cancellation and budget exhaustion are distinct item-error variants. All non-skippable item errors are emitted once and then make the stream terminal. An explicitly skippable diagnostic may be passed through without terminating; it is not a domain result. The dedicated core-history constructor maps every source error to a non-skippable item error.

## Verification

- `query_stream::tests::construction_failure_is_separate_from_stream_items` distinguishes construction failure (`QueryError::InvalidQuery`).
- `query_stream::tests::item_failure_is_returned_once_then_stream_ends_with_none` rejects continuing after a core item error.
- `query_stream::tests::core_history_constructor_cannot_mark_item_errors_skippable` checks the core-history safety path.
- `query_stream::tests::cancellation_budget_and_skippable_diagnostics_have_distinct_semantics` checks distinct terminal outcomes and explicit diagnostic continuation.
- `query_stream::tests::natural_end_is_none_and_not_an_error_item` checks the exact EOF representation.

This contract is the stream primitive only. Paging, public ports, serialization/backpressure, and incomplete aggregate handling remain their later milestone scopes.
