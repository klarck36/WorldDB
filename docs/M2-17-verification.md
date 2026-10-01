# M2-17 – Semantik-Orakel ausreizen

## Generated deterministic oracle cases

The core has no additional property-testing dependency. Instead, the reference
suite generates finite, repeatable tables and complete small-input
permutations, so each case has a fixed name and replay result:

- Assertion full-scan candidates are invariant under all six insertion orders
  of three equal-revision records.
- Equal-precedence single-value resolution returns the same ordered conflict
  for all six permutations of three disagreeing candidates.
- A three-edge EventRelation batch produces the same canonical key order for
  all six input permutations; existing graph tests cover SameTime collapse,
  Before cycles, separately checked Causes cycles, and post-transaction
  retractions.
- Proposition Mask matching uses a five-row truth table: exact equality masks;
  independently changing Subject, Predicate, Value, or Polarity does not.
- Mask visibility is swept across integer WorldTime points 9 through 21 for
  the half-open `[10, 20)` interval; only points inside it mask the candidate.
- A mixed three-relation Provenance cycle has identical diagnostics when its
  candidate edges are reversed. Existing tests also reject cycles within each
  relation family and test post-transaction retraction ordering.
- Existing context-precedence, epistemic-partition, assertion lifecycle,
  boundary, event lifecycle, and schema-aware value tests supply the branch,
  layer, transaction-time, and polarity tables used by the reference model.

These are bounded deterministic property checks, not statistical fuzzing.
Long fuzz campaigns remain separately recorded in M1 evidence. No GitHub or
remote CI was used.

## Evidence

- `candidate_scan::tests::stored_insertion_order_does_not_change_canonical_candidate_set`
- `single_value_resolution::tests::equal_precedence_resolution_is_stable_across_every_three_candidate_permutation`
- `event_relations::tests::relation_batch_is_canonical_for_every_three_edge_input_permutation`
- `mask_projection::tests::proposition_mask_truth_table_requires_each_of_the_four_equality_components`
- `mask_projection::tests::mask_validity_is_half_open_and_future_masks_are_not_visible`
- `provenance_graph::tests::mixed_relation_cycle_is_rejected_stably`
- `cargo test --locked --offline --workspace`: 238 core tests and 70 Rustdoc
  tests passed; all workspace test binaries passed.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and Plancheck passed.
- `cargo xtask verify`: 30 PASS, 1 expected M0-14 SKIP, 0 FAIL.
