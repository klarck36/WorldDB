# M4-13b verification — Bounded Writer Close

## Delivered behavior

- `DatabaseHandle::submit` reserves admission under a short intake lock, then hands the owned command to the bounded mailbox without holding that lock. This lets shutdown close intake even when an accepted submitter is blocked by mailbox backpressure.
- `WriterCoordinator::close(deadline)` rejects new submissions, closes the coordinator's sender, waits for already admitted handoffs, and lets the single writer drain commands in mailbox order. It polls the writer only until the absolute deadline.
- A timed-out `WriterCloseReport` distinguishes an unfinished writer from in-flight submissions. The coordinator retains the thread handle and can be closed again after blockers are released.
- A completed close reports a payload-free worker panic as `TaskFailure::Panicked`; the existing Writer task mapping classifies this as `NeedsRestart`. `join` remains available for callers that explicitly accept an unbounded wait.
- Dropping `WriterCoordinator` closes intake and detaches the handle without waiting or executing a command on the dropping thread. Commands admitted earlier remain owned by the writer thread.

## Verification

- Focused `writer::tests`: 5 passed, including blocked submit handoff, deadline report and retry, FIFO drain, post-close rejection, writer panic reporting, and nonblocking drop.
- `cargo test --locked --workspace`: 381 Core, 7 Testkit, 2 backend-contract, and 80 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `python WorldDB_1.0_Plancheck.py` and `python WorldDB_1.0_Sourcecheck.py`: passed; 239 tasks, 253 invariants, and 169 follow-up pairs are valid, and all generated source snapshots match.
- `cargo xtask verify`: 31 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. M0-13 evidence run `M0-13-20261001T003552Z-6bc27a5326` passed.

The expected `ci-matrix` skip remains due to the external M0-14 platform evidence requirement; all local verification steps passed.
