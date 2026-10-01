# M3-01 verification: concrete error domains

**Result:** PASS — verified 2026-09-30T17:01:54+02:00.

Added and exported the twelve typed error domains required by the plan: Open, Validation, Query, Commit, Storage, Recovery, Migration, Backup, Export, Security, Wire, and Job. Each public error category has an explicit stable `WDB-*` code. Storage failures preserve typed operation and failure-class facts. The existing low-level `WireError` remains the sole wire error type and now exposes stable public codes; only the explicitly named `UnknownExternalCode` variant represents an unknown numeric code supplied by an external protocol.

Commit protocol outcomes are represented independently from failures: `CommitOutcome::Conflict(ConflictReport)` is a normal optimistic-concurrency result, while `CommitError::UnknownCommitOutcome { operation_id }` means the caller must resolve the operation by ID before retrying. Core domain errors use concrete types; error presentation, source-chain exposure, and security-aware mapping remain in M3-02.

## Verification

- `cargo test --locked --workspace`: PASS; 241 core unit tests and 70 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, one expected `ci-matrix` SKIP for deferred M0-14 external platform CI, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.

The focused `errors::tests` cover stable domain-specific codes and prove the protocol distinction between conflict and unknown commit outcome. Existing dependency-policy tests reject erased `Box<dyn Error>` in core domain code and accept concrete domain errors.
