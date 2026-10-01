# M4-13a verification — JobSupervisor Shutdown/Drain

## Delivered behavior

- `JobSupervisor::close(deadline)` stops intake, requests cancellation for every nonterminal job, closes both bounded worker queues, and drains until the absolute `Instant` deadline.
- Queued jobs whose cancellation was requested before worker start become terminal `Cancelled` without invoking their closure. Running jobs receive the cooperative cancellation flag.
- A deadline report lists unfinished job IDs and worker handles that remain active. Their handles and records stay owned by the supervisor, so callers can release blockers and call `close` again. The second close reports completion after the workers exit.
- Submission after shutdown begins returns `JobSubmitError::ShuttingDown`. Worker panics observed during joining are counted in `JobShutdownReport`.
- Dropping the supervisor only closes admission and signals cancellation; it does not wait for blocked work and does not perform a commit.

## Verification

- Focused `job_supervisor::tests`: 8 passed, including queued-job cancellation, deadline reporting and retry, post-close admission rejection, and nonblocking drop.
- `cargo test --locked --workspace`: 379 Core, 7 Testkit, 2 backend-contract, and 80 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `python WorldDB_1.0_Plancheck.py` and `python WorldDB_1.0_Sourcecheck.py`: passed; 239 tasks, 253 invariants, and 169 follow-up pairs are valid, and all generated source snapshots match.
- `cargo xtask verify`: 31 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. M0-13 evidence run `M0-13-20261001T002109Z-6279fa4313` passed.

The supervisor cannot forcibly interrupt arbitrary job code. An uncooperative running job is reported as unfinished when the deadline expires and can be drained later if the job eventually returns.
