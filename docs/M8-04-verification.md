# M8-04 – Interim Windows verification

**Task status: PLANNED pending M8-09 and M8-10.** This is an interim checkpoint, not M8-04 acceptance. Independent Exact and AuditComplete backup verification are wired into the versioned CLI. All backup creation requires the current `BackupCreate` capability; AuditComplete also needs `AuditRead` and `AuditExport`. Clone restore requires `BackupRestore` (and the audit capabilities for AuditComplete). The standalone CLI lacks a trusted host-authenticated policy context, so all creation and restore requests fail closed. GitHub authentication does not establish a WorldDB Principal. M8-09 will provide authenticated host sessions and M8-10 will bind them to a project Principal; M8-04 now depends on M8-10. Same-identity disaster recovery is not exposed by the storage contract.

## Implemented surface

- `v1 backup create` requires an explicit profile and matching audit scope, then returns `Unauthorized` before path access in the standalone CLI because no trusted `BackupCreate` policy context is available.
- `v1 backup verify` reports the verified profile, audit scope, revision, item count, authenticity label, and audit watermark where present.
- The human and JSONL success result repeats the requested profile and audit scope and does not include local paths.
- Exact and AuditComplete creation return `Unauthorized` before source or destination access because current `BackupCreate` context is unavailable; AuditComplete additionally requires `AuditRead` and `AuditExport`.
- `v1 restore clone` validates the explicit profile/scope and returns `Unauthorized` before backup inspection or destination creation because current `BackupRestore` context is unavailable.
- Scope/profile mismatch is rejected before any path is opened. Same-identity disaster recovery is not implemented.

## Verification so far

- `cargo check --locked -p worlddb-cli --all-targets`: PASS.
- `cargo test --locked -p worlddb-cli --all-targets`: 23 passed, 0 failed.
- `cargo clippy --locked -p worlddb-cli --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS.

The Exact and AuditComplete CLI backup-verification JSONL results were both parsed successfully. The final `cargo xtask verify` passed on Windows with 39 PASS, 1 expected M0-14 `ci-matrix` SKIP, and 0 FAIL after both profile cases were present. The task remains planned until trusted host and per-project Principal binding are available; do not treat the above as full CLI Backup/Restore acceptance. Linux/macOS runs remain deferred to M9-07.
