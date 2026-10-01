# M2-15b – Shared Provenance Graph Verification

## Implemented behavior

`validate_provenance_graph_transaction` starts from the active historical
Provenance view at `RecordedAsOf`, applies all candidate ProvenanceRetractions,
adds the complete candidate edge batch at one commit revision, and validates
one post-transaction graph. Candidate IDs and revisions are checked before a
projection is returned.

Every relation maps to the Master dependency direction:

- `Corrects(new, old)`: `new -> old`
- `DerivedFrom(source, derived)`: `derived -> source`
- `ResultedFrom(cause, effect)`: `effect -> cause`

A deterministic Kahn traversal rejects cycles within each relation and mixed
cycles across relation kinds. Cycle errors contain sorted blocked endpoints
and edge IDs. The caller supplies a hard `GraphValidationBudget`; retained
record processing and graph node/edge visits consume it. Exhaustion returns
`GraphValidationBudgetExceeded` with no partial graph result. Active duplicate
logical tuples are rejected after candidate retractions and additions have
been applied together.

## Evidence

- `provenance_graph::tests::all_three_relations_project_to_the_contract_dependency_direction`
- `provenance_graph::tests::mixed_relation_cycle_is_rejected_stably`
- `provenance_graph::tests::each_relation_rejects_its_own_cycle`
- `provenance_graph::tests::retractions_apply_before_atomic_candidate_cycle_validation`
- `provenance_graph::tests::exhausted_budget_fails_closed_without_partial_projection`
- `provenance_graph::tests::post_transaction_batch_rejects_duplicate_active_logical_tuples`
- `cargo test --locked --offline --workspace`: 226 core tests and 70 Rustdoc
  tests passed; two intentionally long fuzz/precision campaigns remain ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and Plancheck passed.
- `cargo xtask verify`: 30 passed, one expected M0-14 CI-matrix skip, zero
  failed.

GitHub and remote CI were not used.
