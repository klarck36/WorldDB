# M7-11 – Verification

**Checked:** 2026-10-02T23:28:43+02:00, Windows

## Focused evidence

All nine `logical_export` tests pass:

- `logical_export_pins_snapshot_and_roundtrips_scope_and_omissions` checks the selected space dependency, the full 36-class manifest, all five omitted storage classes, selected-class counts, and byte-stable encode/decode roundtrip.
- `logical_export_rejects_missing_history_space_dependency` rejects an artifact whose visible HistorySpace ancestry no longer has its complete definition dependency set.
- `logical_export_includes_selected_schema_metadata_and_lifecycle_classes` checks that selected Entity, EntityRetirement, LayerDefinition, PerspectiveDefinitionRevision, and HistorySpaceDefinition records survive scope filtering and roundtrip.
- `durable_export_pin_keeps_replaced_history_until_export_releases_it` replaces the pinned source generation and proves reclamation waits for the export lease to be released.
- `logical_export_uses_current_data_export_permissions_for_each_scope` rejects missing HistorySpace permission and a HistorySpace-only grant for project-scoped records.
- `historical_data_export_allow_does_not_override_current_denial` rejects an earlier allow after the current policy removes that permission.
- `scope_rejects_record_classes_without_durable_revision_coordinates` rejects revisionless migration metadata.
- `logical_export_digest_detects_changed_content` detects modified artifact bytes.
- `logical_export_rejects_missing_omission_class_manifest_row` rejects an incomplete omission manifest.

## Windows workspace checks

- `cargo test --workspace --locked --quiet`: all executed workspace unit, integration, contract, and Rustdoc tests passed. This includes 501 Core unit tests, 59 storage-file unit tests, and 84 Rustdoc tests. Designated long-running campaigns remained ignored.
- `cargo clippy --workspace --locked --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo check --workspace --locked --all-targets`: PASS.
- `WorldDB_1.0_Plancheck.py`: PASS; 243 tasks, 11 milestones, 253 invariants, 177 follow-up pairs.
- `WorldDB_1.0_Sourcecheck.py`: PASS; all source mirrors and the lossless consolidation archive match.
- `cargo xtask verify`: 38 PASS, 1 expected `M0-14 ci-matrix` SKIP, 0 FAIL.
- `git diff --check`: PASS.

Linux and macOS execution remain deferred to M9-07 as requested. This M7-11 result is a Windows-local verification, not the later cross-platform release gate.
