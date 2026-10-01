# M2-13a – EventRelation Canonicalization and Historical Projection

## Implemented behavior

`EventRelation` stores only the canonical relation kinds `Before`, `SameTime`,
and `Causes`. An `After(A, B)` input is normalized to `Before(B, A)` before key
comparison. `SameTime` endpoints are sorted by EventId, while self-relations are
rejected. `EventRelationBatch` rejects duplicate normalized keys independent of
input spelling and ordering.

The new `EventRelationHistory` / `project_active_event_relations` reference
projection validates unique relation/retraction identities, requires every
retraction to name an existing relation at a later Transaction-Time revision,
filters by `RecordedAsOf`, rejects overlapping active duplicate keys, and
returns active relations in canonical key order. A normalized key may be reused
after the previous record has been retracted. EventRelation has no World-Time
validity field. M2-13b completes the transitive SameTime equivalence projection
without materializing extra relation records; its follow-up for WDB-EVT-020 is
now verified.

## Evidence

- `event_relations::tests::after_is_stored_as_the_inverse_before_edge`
- `event_relations::tests::same_time_is_symmetric_and_canonically_ordered`
- `event_relations::tests::self_relations_are_rejected_for_every_input_kind`
- `event_relations::tests::candidate_batch_rejects_normalized_before_after_and_sametime_duplicates`
- `event_relations::tests::relation_projection_filters_transaction_time_and_returns_key_order`
- `event_relations::tests::relation_projection_allows_key_reuse_only_after_prior_retraction`
- `event_relations::tests::relation_projection_rejects_duplicate_identity_and_missing_retraction_target`
- Rustdoc compile-fail cases verify that `After` is not a stored enum variant,
  an EventRelation cannot be mutated, and EventRelation has no World-Time
  validity member.
- `cargo test --locked --offline --workspace`: 207 core tests and 69 Rustdoc
  tests passed.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and `python -B WorldDB_1.0_Plancheck.py` passed.
- `cargo xtask verify`: 30 passed, one expected M0-14 CI-matrix skip, zero failed.

GitHub and remote CI were not used.
