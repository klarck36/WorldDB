# M4-13 verification — Shutdown and Ownership Gate

## Review result

The M4-13a, M4-13b, and M4-13c acceptance evidence closes the local shutdown and ownership contract:

- Job and writer intake stop before either subsystem consumes the shared absolute deadline. Queued jobs are cancelled before work starts; in-flight writer handoffs remain accounted for and accepted mailbox messages drain in FIFO order.
- A deadline timeout reports unfinished jobs, workers, or writer handoffs. The job supervisor, writer coordinator, and database owner retain their handles for a later close attempt.
- `DatabaseCloseReport` separates resource completion from telemetry flush outcome. A failed or panicking exporter does not change the resource-close result.
- Drop signals both subsystem shutdowns, detaches outstanding handles, and does not wait or execute a command on the dropping thread. Owned components, shared handles, and the borrowed telemetry boundary are explicit in the API.
- Lock boundaries are short: writer mailbox sends happen outside the intake lock, callbacks run after resource-close calls return, and shutdown does not hold the job registry lock while draining workers.

## Invariant decisions

- `WDB-CON-002` is covered by the blocked-mailbox close test and the combined owner test, which prove shutdown can acquire the intake gate while an accepted submitter is blocked and callbacks run outside component locks.
- `WDB-OWN-002` is covered by the non-cloneable owner compile-fail example and the supervisor, writer, and combined-owner Drop tests.
- `WDB-OWN-003` is covered by the owner API taking ownership of the supervisor and writer while borrowing `TelemetryFlusher` explicitly at `close`; the integration tests exercise that borrowed adapter.
- `WDB-TX-002` keeps its existing M4-03 model evidence and M5-22 persistence follow-up. M4-13 adds shutdown behavior without claiming the later durable recovery proof.

## Verification

- `cargo test --locked --workspace`: 385 Core, 7 Testkit, 2 backend-contract, and 81 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`, Plancheck, Sourcecheck, and `git diff --check HEAD`: passed.
- `cargo xtask verify`: 31 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. M0-13 evidence run `M0-13-20261001T005730Z-d1be894be6` passed.

The telemetry adapter contract requires the adapter to honor the absolute deadline; arbitrary external callback code cannot be forcibly interrupted by the core. The expected `ci-matrix` skip is the deferred M0-14 external platform evidence requirement.
