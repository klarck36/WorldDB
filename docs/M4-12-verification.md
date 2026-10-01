# M4-12 verification — cancellation and commitpoint boundary

## Delivered behavior

- Each `OpenTransaction` owns a `CommitCancellation` control and exposes a cloneable host handle before the transaction is moved into validation or a worker. Validation preserves that same control; the ordinary `commit()` path always passes it to the backend.
- Cancellation and commitpoint entry race through one atomic state transition. If cancellation wins, the backend returns `CancelledBeforeCommitpoint` and publishes none of the staged batch. If commitpoint entry wins, later cancellation returns `TooLate`; the backend completes publication and returns its full result.
- The state reports `Committed`, `NotCommitted`, or `UnknownOutcome`. A commitpoint permit dropped while publication is in flight records `UnknownOutcome`, so an interrupted operation cannot be reported as definitely cancelled or not committed.
- `RevisionBackend::publish_cancellable` is a required backend contract. Each backend performs reversible work before `begin_commitpoint()`, then completes the whole atomic publication without polling cancellation again. The in-memory backend reserves its next revision invisibly before the race and releases that reservation if cancellation wins.
- Backend publication is explicitly atomic: success exposes the complete batch at one revision; an error exposes no entries. The standard mixed-Record transaction helper uses the transaction-bound cancellation control.

## Limits and follow-up evidence

This is the M4 in-memory reference model. The future durable adapter must place `begin_commitpoint()` at its own irreversible commit boundary; M5 binds that boundary to the durable WAL protocol and restart reconciliation. M5-22 and M8-26d remain follow-up evidence for durable crash recovery and end-to-end cancellation adapters.

## Verification

- `transaction_flow::tests`: 6 passed, including a cancelled two-record mixed batch with no publication, and a post-commitpoint cancel returning `TooLate` while the complete batch commits.
- `commit_cancellation::tests`: 4 passed, including 128 concurrent cancellation/commitpoint races with exactly one winner and an interrupted permit reported as `UnknownOutcome`.
- The existing security-policy backend-failure test also passes through the required cancellable backend contract and leaves policy history at its prior revision.
- `cargo test --locked --workspace`: 377 Core, 7 Testkit, 2 backend-contract, and 80 Rustdoc tests passed; `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, Plancheck and Sourcecheck passed.\n- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. M0-13 run `M0-13-20261001T000603Z-5fcc3ad13b` passed.
