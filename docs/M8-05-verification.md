# M8-05 – Windows verification

**Task status: DONE.** The versioned CLI now plans, previews, executes, and resumes guarded schema migrations, and performs the supported current-pointer V1-to-V2 storage upgrade. Linux/macOS execution remains deferred to M9-07 as requested.

## Implemented contract

- `migration plan` and `migration dry-run` check the current source schema and are read-only. Input records are supplied as bounded canonical WorldDB frames, grouped in plan order.
- `migration run` and `migration resume` require current `MigrationExecute`, stable run and operation IDs, and explicit resolutions for unresolved items. Run checks its source precondition again under the writer lock. Resume accepts only a matching Running journal; a missing or completed journal is rejected before backup or restore artifacts are created.
- Breaking migration requires `--confirm-breaking`, an exact backup, and a real verified clone restorepoint. A resumed Breaking run re-verifies the retained exact backup and creates a fresh clone. Without confirmation, no backup or restore directory is created.
- Each migration commit atomically publishes the transformed records, migration identity, Required Audit, and—when a complete policy history exists—the unchanged policy/epoch/audit-retention projection for the new revision. The policy-gated CLI therefore remains able to open the project after migration.
- `storage upgrade` requires current `StorageFormatUpgrade`, `BackupCreate`, and `BackupRestore` permissions and the explicit `--confirm` flag. It upgrades the supported current-pointer V1 profile to V2 through the existing manager and restorepoint gates. The CLI does not currently expose a storage-upgrade resume command; schema-migration `resume` is supported.
- Success output includes migration/upgrade IDs, revisions, result counts, and fingerprints without filesystem paths. Human and JSONL output use the same result contract.

## Windows evidence

- `cargo test --locked -p worlddb-cli --all-targets`: 26 tests passed, 0 failed. The CLI contract suite includes read-only plan/dry-run checks, rejection without Breaking confirmation, a confirmed Breaking migration with real backup and clone creation, policy-history continuity, path-free output, and missing/completed journal guards.
- `cargo test --locked -p worlddb-storage-file --all-targets`: passed; 86 unit tests passed, the long 100,000-point Windows/NTFS campaign was intentionally ignored, and all integration suites passed. Guarded-migration process-crash/reopen/resume coverage passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 39 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. This also runs the workspace check, feature matrix, dependency and source policies, TypeScript transport checks, project Plancheck and Sourcecheck, Windows storage crash contracts, and whitespace validation.
- Linux/macOS execution remains assigned to M9-07.
