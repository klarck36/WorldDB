# M3-02 verification: leak-safe error mapping

**Result:** PASS for the core error boundary contract — checked 2026-09-30T17:26:16+02:00.

All 12 error domains now have pure, stable-code `Display` and `Debug` output. `WireError` uses the same safe presentation. `InternalError<E>` keeps a concrete typed cause in `source()` while its own `Display`, `Debug`, and `PublicErrorDto` view omit the cause. The public DTO whitelists only the stable code, message key, retry hint, and optional OperationId/JobId; it has no field for causes, paths, queries, or raw values.

ErrorFacts contains only severity, retryability, and integrity impact. Security lookup concealment, recovery actions, retry facts, and public-code projections use exhaustive matches without semantic catch-all arms. Commit `Conflict` remains a normal outcome, while `UnknownCommitOutcome` exposes only its OperationId and instructs clients to resolve status before retrying.

The repository has not implemented the desktop IPC serializer or public diagnostic/log sinks yet. The code tested here is the safe core DTO and formatting boundary; end-to-end serialization and renderer canaries remain explicitly registered for M8-09 and M8-24 in the follow-up evidence matrix.

## Verification

- `cargo test --locked --workspace`: PASS; 243 core unit tests and 70 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, one expected `ci-matrix` SKIP for deferred M0-14 external platform CI, 0 FAIL.
- `errors::tests::public_error_views_hide_secret_causes_and_preserve_typed_sources`: PASS; the canary remains available through the typed internal `source()` chain and is absent from public `Display`/`Debug`.
- `errors::tests::security_and_recovery_boundary_mappings_are_explicit`: PASS; lookup existence is concealed when requested, recovery is denied without authorization, and typed ErrorFacts remain separate from action policy.
