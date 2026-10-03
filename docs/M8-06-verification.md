# M8-06 – Windows verification

**Task status: DONE.** The CLI now exposes logical export, rights-filtered sharing export, canonical typed import-plan creation, and import preparation. Linux/macOS execution remains deferred to M9-07 as requested.

## Implemented behavior

- `v1 export logical` requires a closed explicit revision, HistorySpace, and record-class scope. The written canonical artifact carries its complete scope and omission manifests, including always-omitted storage classes.
- `v1 export share` applies current rights at scope, record, field, relationship, and dependency boundaries. Its CLI result and wire artifact do not disclose omitted-record counts. The existing required-audit authorization and completion boundaries are used.
- Both export result formats report the explicitly selected HistorySpace IDs and record classes, as well as their counts.
- Export destinations must be new files outside the source database. Existing files are never overwritten. Success output is path-free.
- `v1 import plan` accepts canonical typed remaps, binds them to the exact logical-export bytes and destination DatabaseId, and writes a new canonical plan file.
- `v1 import prepare` requires current `ProjectRead` and `DataImport`, verifies the destination, builds its durable identity inventory, and validates the source, explicit plan, collisions, and reference closure. The output includes a stable prepared-stream fingerprint and the source scope/omission totals.
- Import preparation does not publish records to the destination. It binds the CLI to the existing M7-13 preparation contract; persistent import publication remains outside M8-06.

## Windows evidence

- `cargo test --locked -p worlddb-cli --all-targets`: 28 passed, 0 failed (9 unit tests, 4 adapter-process tests, and 15 CLI contract tests).
- End-to-end logical export → explicit HistorySpace remap plan → import preparation: PASS.
- Sharing export with no `SourceRead`: the Source record is absent from the artifact and omission counts are withheld: PASS.
- Typed `RecordRef` remaps decode by the stable wire tag; cross-family and cross-variant remaps are rejected: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 39 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. This includes workspace checks, Clippy with warnings denied, CLI/storage contracts, platform feature checks, and source/plan validation.
- Linux/macOS verification: deferred to M9-07.
