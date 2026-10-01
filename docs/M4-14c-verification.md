# M4-14c verification — Queue-/Shutdown-Schedules

## Schedules

- `begin_shutdown_closes_admission_before_cooperative_cancel_drains` starts a worker, closes JobSupervisor admission, confirms a later submission is refused, observes cooperative cancellation, then drains the worker.
- `begin_shutdown_rejects_new_commands_and_drains_accepted_fifo` blocks the first writer command, accepts a second, closes admission, confirms a third is refused, then observes the two accepted receipts and execution order `[1, 2]`.
- `shutdown_cancels_queued_jobs_and_can_retry_after_deadline` keeps one worker occupied while another job queues. Shutdown cancels the queued job without running its closure, rejects later admission, and can be retried after the running job exits.
- `bounded_close_reports_pending_handoff_then_drains_in_order` combines a blocked writer, queued FIFO messages, a submitter in mailbox handoff, admission closure, a retryable deadline report and the final ordered drain.
- `cancellation_and_commitpoint_race_has_one_linearized_winner` repeats the cancellation/commitpoint race 128 times and accepts exactly the two valid linearized outcomes. Transaction-flow tests separately prove that cancellation before commitpoint publishes no part of the batch and cancellation after commitpoint lets the complete batch finish.
- `owner_stops_both_intakes_then_retries_resources_separately_from_telemetry` exercises the combined JobSupervisor/Writer close under a shared deadline. Drop tests prove blocked jobs/writes do not make owner Drop wait or execute extra work. Public ownership/handle compile-fail examples run as Rustdoc tests.

## Verification results

- New direct shutdown admission schedules: 2 passed.
- Full workspace: 391 Core, 7 Testkit, 2 backend-contract, and 81 Rustdoc tests passed; 2 explicitly long-running campaigns remained ignored by their test annotations.
- Clippy with warnings denied, Rust formatting, Plancheck, and Sourcecheck: PASS.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. Fresh M0-13 evidence run `M0-13-20261001T012558Z-e8a433d682`: PASS.

## Scope

These are deterministic channel/barrier schedules for the in-memory M4 coordination model. Durable commitpoint recovery and writer/job recovery after process restart remain M5 work; M4-14 still requires its gate after these schedules and the SnapshotLease/OCC schedules are all reviewed.
