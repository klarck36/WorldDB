# M4-13c verification — Database Close, Telemetry and Ownership

## Delivered behavior

- `DatabaseCloseOwner` exclusively owns the `JobSupervisor` and `WriterCoordinator`. Shared writer handles can outlive the owner, but become unusable as soon as its shutdown begins. Telemetry stays an explicitly borrowed adapter.
- `close(deadline, telemetry)` closes job and writer admission before waiting on either subsystem, then drains both against the same absolute deadline. A later call can finish a timed-out drain.
- `DatabaseCloseReport` exposes job and writer outcomes separately from the telemetry flush outcome. Resource completion does not depend on a telemetry exporter succeeding; worker panics remain visible and can require restart/recovery.
- Telemetry is passed the absolute close deadline and must honor it. Exporter failures and panics are reported without vetoing resource shutdown or exposing panic payloads. `NoTelemetryFlusher` reports `NotConfigured` explicitly.
- Dropping the owner closes both intakes and detaches remaining thread handles without waiting or executing work on the dropping thread. The owner is not cloneable; component ownership and the borrowed telemetry boundary are visible in the API.

## Verification

- Focused `database_close::tests`: 4 passed, covering coordinated timeout and retry, both intake gates, independent telemetry outcomes, no configured exporter, and nonblocking owner drop.
- `cargo test --locked --workspace`: 385 Core, 7 Testkit, 2 backend-contract, and 81 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.

`python WorldDB_1.0_Plancheck.py` and `python WorldDB_1.0_Sourcecheck.py`: passed; 239 tasks, 253 invariants, and 169 follow-up pairs are valid, and all generated source snapshots match.

`cargo xtask verify`: 31 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. M0-13 evidence run `M0-13-20261001T005157Z-04d69d8154` passed.

The expected `ci-matrix` skip remains due to the external M0-14 platform evidence requirement; all local verification steps passed.
