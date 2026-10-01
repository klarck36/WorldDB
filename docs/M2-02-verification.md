# M2-02 verification: HistorySpace inheritance

**Status:** DONE; checked 2026-09-30.

`HistorySpaceReferenceModel<T>` is an index-free reference model over the immutable HistorySpace forest and shared revision log. A child sees its parent only through its fixed `base_revision`; nested ancestry applies each cutoff; later parent commits do not flow into existing children or siblings. Creating a later child at a newer published cutoff is the explicit way to adopt that later parent snapshot. Unknown spaces, future cutoffs, reads before a selected space's base, and child cutoffs before the parent's base fail closed.

The model stores only catalog definitions and scoped history entries; it has no schema-history or schema-identity write API. A `compile_fail` rustdoc example guards that boundary.

Four unit tests pass:

- `history_model::tests::child_and_sibling_reads_use_fixed_parent_cutoffs`
- `history_model::tests::future_cutoffs_unknown_spaces_and_pre_base_reads_fail_closed`
- `history_model::tests::nested_children_apply_each_ancestor_cutoff`
- `history_model::tests::child_cutoff_cannot_predate_its_parent_base`

These tests provide positive and negative evidence for `WDB-BRA-001` through `WDB-BRA-004`. Explicit content transfer remains the separate M2-02a task.

**Verification:** `cargo test --locked --offline -p worlddb-core history_model::tests` — 4 passed, 0 failed. `cargo test --locked --offline --workspace --doc` — 66 passed, including the schema-write compile-fail example.
