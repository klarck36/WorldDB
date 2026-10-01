# M4-14 verification — Concurrency-Modelltests Gate

## Review of the three schedule suites

- **M4-14a, OCC and phantoms:** Five fixed seeds drive 64 batches each, with 2–5 transactions per batch. Point reads (including absence), ranges, Predicate/Schema/HistorySpace-head scopes, conflicts, and successful commit revisions are checked against an independent serial reference. Each seed is replayed from a fresh store and must produce the identical ordered outcome trace.
- **M4-14b, SnapshotPin and lock ordering:** Explicit fork/release and expiry schedules prove that every lease keeps its immutable binding and a generation remains pinned through the final release. Barrier-started duplicate pin and concurrent fork/read schedules finish within bounded waits and accept only the specified linearized outcomes. The `SnapshotLease` non-Clone compile-fail check passes.
- **M4-14c, queues and shutdown:** Direct JobSupervisor and writer admission schedules prove stop/cancel/drain order and FIFO execution. Existing gated schedules cover queued job cancellation, mailbox handoff timeout/retry, 128 commitpoint races, no partial publication before commitpoint, complete publication after commitpoint, combined owner close, nonblocking Drop, and non-cloneable handles/owners.

## Gate result

All M4-14 acceptance points are covered by named tests and the checks below. No omitted schedule, unbounded wait, or unresolved handle compile-fail case was found in these in-memory M4 contracts. Durable restart, WAL recovery, and physical reclamation remain assigned to later storage tasks.

## Verification

- `cargo test --locked --workspace`: 391 Core, 7 Testkit, 2 backend-contract, and 81 Rustdoc tests passed; the 2 long-running fuzz/precision campaigns remain explicitly ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`: PASS.
- Plancheck and Sourcecheck: PASS; 242 tasks, 11 milestones, 253 invariants, and 169 follow-up pairs are structurally valid; source copies and the audit ZIP match.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. Fresh M0-13 evidence run `M0-13-20261001T012558Z-e8a433d682`: PASS.
