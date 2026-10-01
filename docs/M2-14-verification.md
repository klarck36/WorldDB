# M2-14 – Source/Evidence Meta-History Verification

## Implemented behavior

`full_scan_source_evidence` builds a project-wide Source and Evidence view at a
`RecordedAsOf` snapshot. Sources and Evidence records are immutable metadata;
the query has no World-Time input. Source inventory is visible independently
of whether the Source has any Evidence edge.

Evidence entries retain the referenced target metadata and lifecycle status.
An effective `EvidenceRetraction` marks an entry retracted at its transaction
revision; it does not remove the historical reference or reactivate, restore,
or otherwise project the target domain record. The target inventory is used
only to validate identity, creation revision, and scope visibility.

HistorySpace parent cutoffs apply to scoped target visibility at query time.
They do not restrict the project-wide Source inventory or Evidence history.
The projection rejects malformed IDs/inventories, evidence preceding its
source/target, malformed or missing retraction targets, and duplicate active
`(Source, Target, Relation)` keys. A key may be reused after an effective
retraction. Public endpoint authorization remains assigned to M3-04.

## Evidence

- `source_evidence_projection::tests::as_of_filters_project_wide_sources_evidence_and_retractions`
- `source_evidence_projection::tests::child_cutoff_hides_future_scoped_targets_but_keeps_visible_parent_evidence`
- `source_evidence_projection::tests::retracted_domain_target_remains_referenced_without_becoming_an_active_domain_record`
- `source_evidence_projection::tests::malformed_sources_targets_and_retractions_fail_closed`
- `source_evidence_projection::tests::active_duplicate_evidence_tuples_conflict_but_retraction_frees_the_tuple`
- Rustdoc compile-fail: `EvidenceHistoryEntry` cannot be converted to an `Assertion`.
- `cargo test --locked --offline --workspace`: 218 core tests and 70 Rustdoc
  tests passed; two intentionally long fuzz/precision campaigns remain ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`
  passed.
- `cargo fmt --all -- --check` and Plancheck passed. `cargo xtask verify`: 30
  passed, one expected M0-14 CI-matrix skip, zero failed.

GitHub and remote CI were not used.
