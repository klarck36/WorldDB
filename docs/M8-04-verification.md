# M8-04 – Windows verification

**Task status: DONE.** Windows CLI backup creation and clone restore use the current account SID as the host-bound WorldDB Principal and check the current project policy. Linux/macOS execution remains deferred to M9-07 as requested.

## Implemented contract

- `backup create` requires the current `ProjectRead` and `BackupCreate` grants. `AuditComplete` also requires `AuditRead` and `AuditExport`; its audit prefix is captured under the source snapshot lock.
- `backup verify` verifies either profile and requires the matching explicit audit scope.
- `restore clone` requires `--authorize-with <current-project>`, current `ProjectRead` and `BackupRestore`, and, for `AuditComplete`, current `AuditRead` and `AuditExport`. The authorized project must match the backup DatabaseId.
- Restore publishes a new DatabaseId, verifies the new target, refuses a destination overlapping the authorization project, and never replaces an existing destination. Same-identity disaster recovery remains unavailable.
- Success JSONL reports profile, scope, IDs/revisions, audit watermark where applicable, authenticity, and target verification without filesystem paths.

## Windows evidence

- `cargo test --locked -p worlddb-cli --all-targets`: 24 passed, 0 failed (including authorized Exact and AuditComplete create/restore end-to-end coverage).
- `cargo clippy --locked -p worlddb-cli --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 39 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. This includes workspace checks, workspace Clippy, storage crash/restore contracts, policy checks, feature matrix, source and plan validation, and whitespace checks.
- `WorldDB_1.0_Plancheck.py`: PASS after task-register update.
- `WorldDB_1.0_Sourcecheck.py`: PASS.
- `git diff --check HEAD`: PASS.
