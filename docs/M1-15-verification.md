# M1-15 verification: transaction, migration, job, and audit values

**Status:** DONE\
**Verified:** 2026-09-29 19:23:27 Europe/Berlin\
**Scope:** Typed identity families, immutable value DTOs, and closed runtime-state enums. This task adds no I/O, worker, cancellation, or storage behavior.

## Implemented

- `TransactionState` models the six contract states: `Open`, `Validated`, `Committing`, `Committed`, `Aborted`, and client-side `UnknownOutcome`. `TransactionIdentity` keeps `TransactionId` separate from the retry-stable `OperationId`.
- `MigrationCategory` contains the exact five categories. `MigrationPlan` has a non-empty, ordered, duplicate-free set of `MigrationStepId`s. `MigrationRun` uses its own `MigrationRunId`; each step-commit identity also keeps `MigrationId`, `MigrationStepId`, and `OperationId` separate. Source-schema preconditions, target schema payload, transformer version, fingerprint, budgets, and execution remain with M7.
- Job DTOs keep `JobId`, optional `PrincipalId` owner, `JobKind`, `JobStatus`, finite work/memory budgets, and progress distinct. Determinate progress rejects a zero denominator and `completed > total`. Worker handles and `CancellationToken` remain runtime-owned.
- Audit DTOs separate `AuditRecordId`, `AuditSequence`, and `AuditOperationId`. `RawReadAttempt` also binds `ClientRequestId`, principal, scope fingerprint, snapshot, `SecurityEpoch`, and page ordinal. Retry evidence confirms that `ClientRequestId` stays stable while each attempt receives a new audit-operation ID and sequence. Fingerprints remain opaque non-empty bytes because the contract does not choose their wire length or algorithm.
- `ClientRequestId` is registered as a persistent audit-subsystem UUID identity. `SecurityEpoch` is a separate `u64` newtype initialized to zero; increment uses checked arithmetic and reports exhaustion without wrapping.

## Invariant evidence

- `WDB-SEN-002` is DONE. Positive tests exercise the state/value types; Rustdoc compile-fail examples reject `None` or `Err` as `JobStatus`, cross-family state assignments, `AuditSequence` as `Revision`, and open `Other` variants.
- The registered UUID identity taxonomy now contains 51 distinct types (`WDB-ID-001/004`). `ClientRequestId` has an explicit audit namespace, persistence boundary, and wire boundary.
- M1-15 supplies the value-type groundwork for `WDB-TX`, `WDB-MIG-007/017`, and job/audit contracts. Transaction typestate and commit semantics remain M4; complete migration planning and resume remain M7; durable audit sequencing and raw-read fail-closed I/O remain M5. Those invariants are not marked complete here.

## Verification results

- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline --workspace --all-targets`: 105 passed (94 core, 7 testkit, 4 xtask; CLI and file-storage crates contain no unit tests), 0 failed.
- `cargo test --locked --offline --doc --package worlddb-core`: 65 passed, 0 failed.
- `cargo xtask verify`: 27 PASS, 1 visible `ci-matrix` SKIP required by deferred M0-14 GitHub/macOS CI, 0 FAIL.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: 236 tasks, 11 milestones, 253 invariants, and 159 follow-up pairs; structure valid.
- `git diff --check HEAD`: PASS. Git printed only the existing LF-to-CRLF working-copy notices.

**Next task:** M1-16, TLV frame and scalar encoding.
