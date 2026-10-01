# M2-13b – Event Graph Verification

## Implemented behavior

`validate_event_graph_transaction` starts from relations active at a
`RecordedAsOf` snapshot, applies the whole candidate set of relation
retractions, adds the proposed `EventRelationBatch`, and validates that
post-transaction graph atomically. Candidate additions and retractions must
carry the transaction revision; retractions must target an active relation.
The supplied Event inventory is checked for duplicate IDs and missing
endpoints.

The graph projection computes SameTime connected components as sorted EventId
sets. It returns the original explicit relations and never creates transitive
SameTime records. It collapses those components before checking the directed
Before graph. A Before edge inside one component and a cycle after collapse
return distinct deterministic errors. Causes is checked in a separate directed
graph, orthogonal to both SameTime and Before. Cycle errors carry sorted blocked
node and relation-ID sets. EventTime is not an input to graph validation, so
temporal proximity cannot create relations.

## Evidence

- `event_relations::tests::graph_projection_builds_same_time_components_without_transitive_records`
- `event_relations::tests::before_inside_a_same_time_component_is_a_conflict`
- `event_relations::tests::before_cycle_after_same_time_collapse_is_stable_under_input_order`
- `event_relations::tests::causes_cycles_are_checked_separately_and_do_not_create_before_edges`
- `event_relations::tests::post_transaction_graph_applies_retractions_before_cycle_validation`
- `event_relations::tests::event_times_are_not_graph_inputs_and_cannot_infer_relations`
- `cargo test --locked --offline --workspace`: 213 core tests and 69 Rustdoc
  tests passed.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and `python -B WorldDB_1.0_Plancheck.py` passed.
- `cargo xtask verify`: 30 passed, one expected M0-14 CI-matrix skip, zero failed.

GitHub and remote CI were not used.
