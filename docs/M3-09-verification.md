# M3-09 verification: owned query ports

`OwnedQueryResult<T>` pins each returned value to the exact `HistoricalQueryBinding`, current `SecurityEpoch`, and evaluated `SecurityEpoch`. Its `T: 'static` bound prevents references into request-scoped locks, memory maps, or storage buffers from escaping. The owned Raw History port runs the security-filtered full scan and returns only owned `RawHistoryRow` values. Resolved and Explain binders validate the supplied visible contributor/record sets before binding the result.

Visibility failures use ID-free `QueryPortError` variants, so a rejected hidden contributor or Explain record does not disclose which identity failed. Explain must also match the exact QueryContext data/schema binding. Canonical ordering remains provided by the existing raw-row, ResolvedView contributor, and ExplainStage constructors.

## Verification

- `query_context::tests::owned_resolved_port_pins_schema_and_security_epochs`: creates repeated complete policy projections, runs the authorized owned raw port, checks owned rows, and binds a resolved result to the same schema binding and both security epochs.
- `query_ports::tests::resolved_view_cannot_expose_a_hidden_contributor`: accepts visible contributors and rejects one outside the visible candidate set with an ID-free error.
- `query_ports::tests::explain_boundary_rejects_hidden_candidates_and_applied_records`: accepts visible candidates/records and rejects hidden candidate or mask references with ID-free errors.
- Rustdoc compile-fail on `OwnedQueryResult`: a non-static borrow cannot be used as the result payload.
- `reference_query::tests::raw_rows_sort_by_fixed_typed_tag_then_id_and_reject_duplicate_identity`, `reference_query::tests::resolved_view_orders_contributors_and_rejects_duplicates`, and `reference_query::tests::explain_trace_binds_sorted_mask_and_boundary_stages_to_the_same_snapshot`: retain the positive and representative violating ordering checks.

This milestone establishes owned Core query results and visibility checks. Public IPC serialization, paging/cursors, and optimized storage/index paths remain in their later milestone scopes.
