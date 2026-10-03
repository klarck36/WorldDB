# M7-16d – Guarded migration on the file store

**Checked:** 2026-10-03, Windows
**Status:** complete for the local Windows phase; Linux/macOS remain deferred to M9-07.

## Integrated execution path

`FileStoreGuardedMigrationRun` opens the persistent migration commit backend and migration-run sidecar against one recovered `DatabaseLayout`, while retaining the exclusive `WriterLock`. The core guarded executor therefore reads the current file-store history and publishes Restrictive/Breaking migration steps through the actual WAL backend. The wrapper exposes the recovered head revision and durable journal status.

The wrapper constructs and verifies a journal specification from the plan, run identity, transformer version, ordered per-step inputs, and their canonical input fingerprints. At each step it binds the commit marker to the plan, run, step, operation, input fingerprint, target revision, and terminal position of the marker. The transformed records are fingerprinted using the core v2 encoding.

The run sidecar is created only when an authorized step reaches publication. The wrapper syncs `Prepared` before the WAL commit and records `Committed` only after the migration record, canonical migration action, Required Audit record, and history replay state have committed under the same WAL marker. It records `Completed` after every step. If the WAL commit succeeds but the journal cannot be updated, the API returns the operation identity as an unknown/reconciliation case; it does not claim that the run failed without publication. A prepared or committed prefix currently returns `ResumeRequired`; resumption is M7-16e.

## Windows verification

Targeted command: `cargo test --locked -p worlddb-storage-file --lib guarded_migration::tests:: -- --nocapture` — **4 passed, 0 failed**.

- `restrictive_run_persists_each_step_journal_and_required_audit_on_file_store`: executes two Restrictive steps over non-empty canonical history records; checks per-step journal state, audit action, Required Audit, and committed file-store history.
- `denied_current_rights_create_no_journal_or_normative_commit`: denies stale/current unauthorized execution without creating a journal or committing migration history or Required Audit.
- `stale_plan_source_revision_fails_occ_before_journal_or_publication`: checks current-head OCC and confirms no journal or migration audit is created on a stale plan.
- `breaking_run_requires_real_restore_proof_and_explicit_admin_action`: performs exact backup and real clone restore; missing admin action fails closed, while explicit confirmation plus the restore proof commits to the real file store with the required audit.

Workspace Clippy passed with warnings denied. `cargo xtask verify` passed with **38 PASS, 1 expected M0-14 ci-matrix SKIP, and 0 FAIL**. Plancheck passed with 252 tasks, 253 invariants, and 209 follow-up pairs; Sourcecheck passed all source and ZIP hash checks. The verify run also passed format-check and `git diff --check HEAD`.

## Boundaries

The adapter does not resume a prepared prefix in this task; M7-16e adds crash/reopen reconciliation and idempotent resume. Cross-platform execution remains deferred to M9-07 per project instruction.
