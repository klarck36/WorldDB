# M7-16 – Fault and compatibility inventory

**Checked:** 2026-10-03, Windows
**Scope task:** M7-16 (inventory and split only; no crash hook implementation is claimed here)

## Release and fixture baseline

The public GitHub repository has no published releases and `git ls-remote --tags --refs origin` returns no tags. No Alpha has been published, so N-1 compatibility is **not applicable**, not passed. Recheck this condition when M7-16h and M9-02 run.

The existing pre-release storage-profile baseline is `crates/worlddb-storage-file/tests/fixtures/m7-07/`: a 72-byte v1.0 `FORMAT` probe, an unsupported v2.0 probe, and a v1.0 probe with an unsupported required capability. Their fixed hashes and limitations are recorded in that directory's README and `docs/M7-07-storage-format-upgrade-plan.md`. These probes do not constitute a complete database, backup, migration, or export compatibility corpus. M7-16h owns the broader versioned fixture baseline.

## Current phase coverage and gaps

| Area | Existing boundaries and evidence | Missing evidence for M7-16 |
|---|---|---|
| Backup | `backup.rs` pins a snapshot, builds the destination with an incomplete marker, copies inventory items, writes/syncs the manifest, removes the marker, and verifies the target. Existing checks include pinned-snapshot behavior and target-verify failure retaining the incomplete marker. | No child-process exit/reopen at each durable copy, manifest, marker-removal, or verify boundary. A returned injected error is not a process crash. |
| Restore | `restore.rs` has `BeforePublish` and `AfterPublish` error checkpoints. `publication_faults_leave_either_no_target_or_a_verified_audited_target` checks the two synchronous error outcomes. | No process kill/reopen at preparation, atomic publish, or post-publish outcome. |
| Schema migration | Core has journal/reconciliation coverage, including `resume_reconciles_commit_when_journal_update_fails_after_publication`. M7-16c implements the persistent WAL adapter; M7-16d now runs Restrictive and Breaking execution through a recovered file-store handle and durable journal, including live authorization, OCC, restore proof, and Required Audit. | M7-16e must still cover process crashes and idempotent resume. M7-16d intentionally returns `ResumeRequired` for prepared/committed prefixes; the integration evidence does not substitute for the crash/resume matrix. |
| Logical and sharing export | Logical export has canonical artifact, pin, digest, and scope checks. Sharing export already has `process_crash_after_authorization_commit_recovers_only_that_audit_boundary`. | No process-exit matrix for logical export pin release / artifact visibility or the remaining sharing completion boundary. |
| Offline purge | `purge_rewrite.rs` injects faults before audit commit, before publish, and after publish; contract tests verify source/target/audit outcomes for returned errors. | No process kill/reopen at those durable boundaries. |

All new platform execution in this round is Windows-local. Linux and macOS runs remain deferred to M9-07, as directed by the user; this inventory does not treat them as locally runnable or passed.

## Ordered implementation tasks

The former cross-cutting M7-16 is now limited to this phase map and pre-implementation split. The following tasks are ordered so only one is READY at a time:

| Task | Deliverable and acceptance |
|---|---|
| M7-16a | Child-process backup crash matrix. At each durable phase, reopening proves the source is valid and the target is unmistakably incomplete or fully verified; retry cannot mistake partial bytes for success. |
| M7-16b | Child-process restore crash matrix. Before publication, no target is visible; after publication, the target is complete, independently verified, and carries its required audit. Existing destinations remain untouched. |
| M7-16c | Persistent file-store migration commit adapter. Implement the guarded `MigrationCommitBackend` on the real storage transaction path so a migration batch and its Required Audit record share the same atomic commitpoint; fail closed on unsupported states. |
| M7-16d | Production guarded-migration integration. **Complete on Windows:** exercise the adapter through a real file-store database handle, including authorization, restore-point proof for Breaking, OCC, journal status, and audit binding. Evidence: `docs/M7-16d-guarded-file-store-integration.md`. |
| M7-16e | Migration process-crash/reopen matrix on that adapter. Interrupt each journal/commit boundary; resume reconciles the committed prefix by operation identity, preserves valid intermediate schemas, and never duplicates a step. |
| M7-16f | Logical/sharing export process lifecycle matrix. No incomplete artifact is returned or published; required audit boundaries remain exact; durable pins are released or recovered safely after process exit. |
| M7-16g | Offline-purge child-process crash matrix. The source remains valid; the destination is absent or fully verified with its Required Audit and report; secure erase is never claimed. |
| M7-16h | Versioned pre-Alpha compatibility fixtures covering the supported storage profile and representative complete backup/restore, migration, and logical/sharing export artifacts. Record hashes and expected open/read/upgrade outcomes. Do not label any fixture N-1 before an Alpha is actually published. |
| M7-16i | Aggregate phase-by-phase crash/compatibility matrix and evidence review. Confirm each crash result is source intact plus target absent/incomplete or valid, verify hashes and exact test manifests, and record N-1 applicability. |

The sequence intentionally places M7-16c before migration crash testing: current M7-10a tests a contract backend and restore-point builder, but it does not expose a production file-store migration commit route. M7-17 must not claim that route passed unless M7-16c through M7-16e produce real file-store evidence.

## Evidence references

- `artifact:docs/M7-16-fault-compatibility-inventory.md`
- `artifact:docs/M7-07-storage-format-upgrade-plan.md`
- `artifact:docs/M7-10a-verification.md`
- `test:m7_08_exact_backup_contract::target_verify_failure_keeps_the_backup_marked_incomplete`
- `test:backup::restore::tests::publication_faults_leave_either_no_target_or_a_verified_audited_target`
- `test:migration_execution::tests::resume_reconciles_commit_when_journal_update_fails_after_publication`
- `test:sharing_export::tests::process_crash_after_authorization_commit_recovers_only_that_audit_boundary`
- `test:purge_rewrite::tests::publication_faults_never_expose_an_unaudited_database`
- `run:github-public-release-and-tag-check-2026-10-03 (0 published releases and zero refs from git ls-remote --tags --refs origin)`

## M7-16 parent-task verification

Checked on Windows at 2026-10-03T04:20:38+02:00. `python -X utf8 WorldDB_1.0_Plancheck.py` passed with 252 tasks, 253 invariants, and 209 follow-up pairs. `python -X utf8 WorldDB_1.0_Sourcecheck.py` passed all source and ZIP hash checks. `cargo xtask verify` passed with 38 PASS, 1 expected M0-14 ci-matrix SKIP, and 0 FAIL. `git diff --check HEAD` passed. These checks validate the inventory and plan structure; the new crash rounds remain unimplemented under their separate task IDs.

The M7-16 plan/inventory commit `3df80f7` was pushed to `origin/codex/worlddb-project-integration`; `git ls-remote` confirmed the remote ref at that commit.

Follow-up: M7-16a completed on Windows; see `docs/M7-16a-backup-crash-matrix.md`. M7-16b completed on Windows (`docs/M7-16b-restore-crash-matrix.md`). M7-16c completed on Windows (`docs/M7-16c-persistent-migration-commit.md`). M7-16d completed on Windows (`docs/M7-16d-guarded-file-store-integration.md`); M7-16e is next. Linux/macOS execution remains deferred to M9-07.
