# M4-11 verification — JobSupervisor

## Delivered behavior

- `JobSupervisor` runs fixed-size CPU and blocking-I/O pools with independent bounded `sync_channel` queues. Submission never waits for queue capacity: a full pool returns `QueueFull`, and admission is also limited by the hard maximum number of tracked jobs.
- Every job has a typed `JobId`, kind, optional owner, pool, `JobBudget`, queued/running/terminal status, and determinate or indeterminate progress. Determinate work cannot exceed the declared work-unit budget.
- A task can cooperatively observe cancellation, reserve and release memory against its declared budget, and publish versioned opaque resume metadata within a configured byte limit. Reservation is accounting supplied by the task adapter; it does not measure physical allocations.
- Worker panics are caught and exposed as payload-free `TaskFailure::Panicked`; the panic payload is not retained in the job snapshot. A panic in the actual writer thread is observed by the writer join path and maps to `NeedsRestart`.
- Terminal records remain visible until `forget_terminal` is called, so registry usage stays bounded and capacity reuse is explicit. Memory reservations are cleared when a job becomes terminal.

## Limits

This is the M4 in-memory/reference supervisor. The resume metadata is held only in memory; durable checkpoint storage and restart recovery belong to the later storage integration. M4-13a replaces the baseline blocking `join_workers` path with `close(deadline)`, which reports unfinished jobs and retains worker handles for a later drain attempt. The supervisor does not itself make cancellation interrupt arbitrary user code.

## Verification

- `cargo test --locked --workspace`: 370 Core, 7 Testkit, 2 backend-contract, and 80 Rustdoc tests passed.
- Focused `job_supervisor::tests`: 6 passed; actual writer-panic recovery test: 1 passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Queue saturation, hard registry capacity, independent CPU/I/O progress, cooperative cancellation, bounded progress/checkpoints/memory accounting, task-panic capture, and writer-panic `NeedsRestart` have named automated tests.

The workspace test and lint runs cover the implementation-level acceptance for M4-11. Plancheck and Sourcecheck passed. `cargo xtask verify` passed with 31 PASS, one expected M0-14 `ci-matrix` SKIP and 0 FAIL; M0-13 run `M0-13-20260930T233923Z-5465f552de` passed.
