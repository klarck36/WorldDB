# M2-15a – Provenance Endpoints and Meaning Verification

## Implemented behavior

`ProvenanceEdge` keeps the three relation kinds distinct and uses closed typed
endpoint variants. `Corrects` requires the same concrete endpoint family;
`DerivedFrom` excludes operational directives as sources; `ResultedFrom` only
accepts Assertion/Event endpoints. Self-loops are rejected.

`ProvenanceEdge::corrects_assertion` binds `Corrects(replacement, target)` to
the same Proposition-Slot tuple `(Subject, PredicateId, PerspectiveScope,
EpistemicMode)` and requires the replacement record to be later on
Transaction Time. Value, polarity, HistorySpace, Layer, and validity may
differ at this relation-level check. The higher-level `CorrectAssertion`
command still has its stricter same-context rule and atomic explicit
retraction in M2-15c. Event correction uses the existing pinned-schema
compatibility callback and now has a negative incompatibility test.

`project_active_provenance_edges` filters edges and their separate
ProvenanceRetractions at `RecordedAsOf`, validates IDs and lifecycle target
order, rejects active duplicate `(From, To, Relation)` tuples, and permits
tuple reuse after an effective retraction. It accepts no World-Time,
ContextPrecedence, or Resolution input. A Provenance edge remains explanatory
metadata; it does not trigger a retraction or cascade.

## Evidence

- `source_provenance::tests::derived_from_rejects_exactly_the_disallowed_from_families`
- `source_provenance::tests::resulted_from_accepts_only_assertion_or_event_on_each_side`
- `source_provenance::tests::corrects_requires_same_concrete_family_and_provenance_rejects_self_loops`
- `source_provenance::tests::assertion_corrects_requires_same_proposition_slot_and_later_record`
- `source_provenance::tests::provenance_projection_filters_as_of_retractions_and_active_duplicates`
- `events::tests::event_correction_adds_new_event_and_corrects_edge_but_keeps_original_active`
- `wire_records::tests::source_meta_decoder_enforces_the_closed_evidence_and_provenance_matrices`
- `cargo test --locked --offline --workspace`: 220 core tests and 70 Rustdoc
  tests passed; two intentionally long fuzz/precision campaigns remain ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and Plancheck passed.
- `cargo xtask verify`: 30 passed, one expected M0-14 CI-matrix skip, zero
  failed.

GitHub and remote CI were not used.
