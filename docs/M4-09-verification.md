# M4-09 verification — OperationId deduplication and status

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Added an in-memory reference journal binding each `OperationId` to a BLAKE3 fingerprint of the canonical request payload. Reuse with different bytes returns `IdempotencyMismatch`; replay after commit returns the original `CommitReceipt`.
- Exposed `Committed(receipt)`, `NotCommitted`, and `Indeterminate` status. A repeated unresolved request stays `Indeterminate` and cannot execute again. A retry becomes executable only after the journal records `NotCommitted`, and it must use the same payload.
- Added typed deprecated-schema warnings to `CommitReceipt`. The receipt keeps the `DeprecatedSchemaWriteWarning` values from validation and exposes them as typed data.
- Preserved the strict archive timing decision: transfer copies begin unarchived; any archive transition must be committed in a later transaction.

## Verification results

- Operation status tests: 5 passed, covering same-payload receipt replay, payload mismatch, unresolved duplicate prevention, proven-not-committed retry and absent-ID status.
- Deprecated warning receipt path: the validation test confirms warning values survive in the successful receipt.
- `cargo test --locked --workspace`: 353 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Plancheck: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs valid.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. M0-13 run `M0-13-20260930T230231Z-7e4c1f5fcc` passed.

## Durability boundary

`OperationStatusJournal` is the M4 in-memory reference model; it is not atomically coupled to a durable commitpoint and does not claim crash recovery. A production storage adapter must persist the payload binding, status transition and receipt together with the WAL/manifest commit marker, and recover ambiguous entries as `Indeterminate`. That durable replay and restart evidence remains M5 work. An absent ID means `NotCommitted` only when the queried durable journal view is complete and authoritative.
