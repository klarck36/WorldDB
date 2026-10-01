# M2-01 verification: in-memory revisions

**Status:** DONE; checked 2026-09-30.

`InMemoryRevisionLog<T>` is an append-only reference sequence of immutable commits. Genesis is revision 0; the first published commit is revision 1; only the exact next reservation can publish. A reservation stays invisible until publication, cancellation reuses the same next revision, reads scan insertion order without an index, and the reserved maximum is never published.

Four unit tests pass:

- `revision_history::tests::unpublished_reservations_are_invisible_and_cancel_without_gaps`
- `revision_history::tests::publication_rejects_missing_reservation_and_revision_gaps`
- `revision_history::tests::transaction_revision_order_does_not_follow_domain_value_order`
- `revision_history::tests::revision_exhaustion_never_publishes_reserved_maximum`

The order-counterexample commits payload values 9 then 1 and reads them in their transaction revision order. That demonstrates that revision records commit order; it does not sort or infer domain precedence. `WDB-HIS-001` and `WDB-HIS-002` have positive and negative evidence in the invariant matrix. Durable publication and crash behavior remain a separate M5-22 follow-up.

**Verification:** `cargo test --locked --offline -p worlddb-core revision_history::tests` — 4 passed, 0 failed.
