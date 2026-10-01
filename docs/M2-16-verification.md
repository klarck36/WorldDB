# M2-16 – Referenz-Query/Explain

## Implemented reference behavior

`HistoricalQueryBinding` resolves one `SchemaMode` at the caller's published
`RecordedAsOf` and captures the data revision, requested mode, effective schema
revision, and canonical schema fingerprint. Rebinding the same data revision
and mode against unchanged schema history yields an equal binding. Historical,
Current, and Explicit modes remain distinguishable by their effective schema
identity; no mode falls back to another one.

`full_scan_raw_history` reads the selected HistorySpace at the pinned revision,
returns owned rows, and orders them by the stable `(RecordRef wire tag, ID
bytes)` key. Duplicate typed identities fail closed. This layer does no
resolution; an authorization boundary must filter rows before exposure.

`ResolvedView` is built from the concrete single- or multi-value resolution
outcome. It derives its contributors from that outcome, sorts them by typed
Assertion ID, and rejects duplicate contributor identities.

`ReferenceExplain` binds the same historical query key and validates one
contiguous CandidateScan → optional MaskProjection → optional
ReplacementBoundary → Resolution trace. Every filtering stage can only remove
candidate assertions, and its applied record references must match the stage's
Mask or ReplacementBoundary family. IDs in stages and contributors are
canonicalized. The Explain model is a core reference result structure; public
security-aware ports and stream error semantics remain assigned to M3.

## Evidence

- `reference_query::tests::historical_binding_repeats_exactly_for_the_same_data_and_schema_snapshots`
- `reference_query::tests::raw_rows_sort_by_fixed_typed_tag_then_id_and_reject_duplicate_identity`
- `reference_query::tests::raw_full_scan_pins_recorded_revision_and_returns_owned_canonical_rows`
- `reference_query::tests::resolved_view_orders_contributors_and_rejects_duplicates`
- `reference_query::tests::explain_trace_binds_sorted_mask_and_boundary_stages_to_the_same_snapshot`
- The existing full-scan assertion, mask, replacement-boundary, single-value,
  and multi-value tests remain the underlying reference oracles.
- `cargo test --locked --offline --workspace`: 235 core tests and 70 Rustdoc
  tests passed; all workspace test binaries passed.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and Plancheck passed.
- `cargo xtask verify`: 30 PASS, 1 expected M0-14 SKIP, 0 FAIL.

GitHub and remote CI were not used.
