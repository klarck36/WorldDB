# M7-16e – Migration process-crash/reopen matrix

**Checked:** 2026-10-03, Windows

## Scope

The matrix exercises guarded Restrictive schema migration through a real file-store handle. A child process performs a two-step run and exits with code 86 at each durable journal/WAL boundary. The parent then reopens the database and run journal, checks the recovered state, and either resumes with the original inputs or confirms that a completed run cannot be replayed.

The test is `guarded_migration::tests::process_crash_at_each_guarded_migration_boundary_resumes_without_duplicate_steps` in `crates/worlddb-storage-file/src/guarded_migration.rs`.

## Crash points and recovery

| Child exit checkpoint | Durable state at exit | Reopen and resume result |
|---|---|---|
| `running` | Run journal is Running; both steps Pending; source head unchanged; no migration marker or Required Audit record. | Same OperationId run resumes and commits both steps. |
| `prepared_step_1` | Step 1 is Prepared before WAL publication; source head unchanged; no marker or Required Audit record. | Same run retries the prepared OperationId and commits both steps once. |
| `wal_commit_step_1` | Step 1 WAL commit is durable, while its sidecar step remains Prepared; head is at step 1; exactly one marker and Required Audit record exist. | Reopen verifies the exact marker and audit bytes, reconciles the sidecar, and commits step 2 without duplicating step 1. |
| `committed_step_1` | Step 1 is Committed in the sidecar and normative history; step 2 is Pending. | Same run keeps the committed prefix and commits step 2 once. |
| `prepared_step_2` | Step 1 is Committed; step 2 is Prepared before its WAL publication; head and audit count still reflect only step 1. | Same run retries step 2 and finishes without changing step 1. |
| `wal_commit_step_2` | Step 2 WAL commit is durable, while its sidecar state remains Prepared; both markers and Required Audit records exist. | Reopen verifies and reconciles step 2; the run completes without duplicate publication. |
| `committed_step_2` | Both steps are Committed; the final head, two markers, and two Required Audit records are durable. | Resume reconciles the full committed prefix and records Completed without republishing either step. |
| `completed` | Both steps and the Completed sidecar state are durable. | Re-execution is rejected as `RunAlreadyCompleted`; history and audit counts stay unchanged. |

At each reopening, the test verifies the journal state, current head, OperationId marker count, and Required Audit count before continuing. Every nonterminal checkpoint ends with exactly two committed markers and two Required Audit records, with no duplicate commit. The intermediate schema at step 1 remains the durable head at all checkpoints before step 2 publishes.

## Changed input and verification

After `wal_commit_step_1`, the test changes step 2's input while retaining the same run identity. Resume returns `JournalIdentityMismatch`; the head, marker count, and Required Audit count do not change. Retrying with the original immutable inputs then completes the same run.

Windows verification:

- `cargo test --locked -p worlddb-storage-file --lib guarded_migration::tests:: -- --nocapture` — 5 passed, 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- `cargo xtask verify` — 38 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed.
- Plancheck and Sourcecheck — passed.

Linux/macOS execution is deferred to M9-07 per project instruction.
