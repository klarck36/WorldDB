# M7-16 – Fault and compatibility inventory

**Checked:** 2026-10-03, Windows
**Scope task:** M7-16 (inventory and split only; no crash hook implementation is claimed here)

## Release and fixture baseline

The public GitHub repository has no published releases and `git ls-remote --tags --refs origin` returns no tags. No Alpha has been published, so N-1 compatibility is **not applicable**, not passed. M7-16h rechecked this condition; M9-02 must check again after the first Alpha is published.

The M7-07 storage-profile baseline is `crates/worlddb-storage-file/tests/fixtures/m7-07/`: a 72-byte v1.0 `FORMAT` probe, an unsupported v2.0 probe, and a v1.0 probe with an unsupported required capability. Their fixed hashes and limitations are recorded in that directory's README and `docs/M7-07-storage-format-upgrade-plan.md`. Those probes do not constitute a complete database, backup, migration, or export compatibility corpus. M7-16h now adds that separate synthetic corpus; its manifest digest and verified outcomes are in `docs/M7-16h-versioned-fixture-baseline.md` and the aggregate matrix `docs/M7-16i-fault-compatibility-matrix.md`.

## Current phase coverage and gaps

The table below is the initial inventory captured before M7-16a through M7-16i ran. Its missing-evidence column records the original gaps; closure evidence is consolidated in `docs/M7-16i-fault-compatibility-matrix.md`.

| Area | Existing boundaries and evidence | Missing evidence for M7-16 |
|---|---|---|
| Backup | `backup.rs` pins a snapshot, builds the destination with an incomplete marker, copies inventory items, writes/syncs the manifest, removes the marker, and verifies the target. Existing checks include pinned-snapshot behavior and target-verify failure retaining the incomplete marker. | No child-process exit/reopen at each durable copy, manifest, marker-removal, or verify boundary. A returned injected error is not a process crash. |
| Restore | `restore.rs` has `BeforePublish` and `AfterPublish` error checkpoints. `publication_faults_leave_either_no_target_or_a_verified_audited_target` checks the two synchronous error outcomes. | No process kill/reopen at preparation, atomic publish, or post-publish outcome. |
| Schema migration | Core has journal/reconciliation coverage, including `resume_reconciles_commit_when_journal_update_fails_after_publication`. M7-16c implements the persistent WAL adapter; M7-16d runs Restrictive and Breaking execution through a recovered file-store handle and durable journal, including live authorization, OCC, restore proof, and Required Audit. M7-16e adds child-process exits at all eight durable journal/WAL boundaries, exact marker/audit reconciliation by OperationId, and idempotent resume. | No further process-crash gap remains for the guarded schema-migration route on Windows. Linux/macOS execution remains deferred to M9-07. |
| Logical and sharing export | Logical export has canonical artifact, pin, digest, and scope checks. M7-16f adds process exits while a durable pin is live, before logical return, after the sharing logical export returns, and after each sharing audit boundary. Reopen verifies artifact absence, exact audit state, stale-pin cleanup, and history reclamation. | No further process-crash gap remains for these Windows export routes. Linux/macOS execution remains deferred to M9-07. |
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
| M7-16e | **Complete on Windows:** migration process-crash/reopen matrix on that adapter. Child-process exits interrupt all eight durable journal/WAL boundaries; resume reconciles the committed prefix by operation identity, preserves valid intermediate schemas, rejects changed inputs, and never duplicates a step. Evidence: `docs/M7-16e-migration-process-crash-matrix.md`. |
| M7-16f | **Complete on Windows:** Logical/sharing export process lifecycle matrix. No incomplete artifact is returned or published; required audit boundaries remain exact; durable pins are released or recovered safely after process exit. Evidence: `docs/M7-16f-export-process-crash-matrix.md`. |
| M7-16g | **Complete on Windows:** Offline-purge child-process crash matrix. The source remains valid; the destination is absent or fully verified with its Required Audit and report; secure erase is never claimed. See `docs/M7-16g-purge-process-crash-matrix.md`. |
| M7-16h | **Complete on Windows:** Versioned pre-Alpha fixtures for current storage, backup/restore, schema migration, and Logical/Sharing exports, with fixed hashes and replayed outcomes. N-1 remains not applicable before a published Alpha. See `docs/M7-16h-versioned-fixture-baseline.md`. |
| M7-16i | **Complete on Windows:** Phase-by-phase crash/compatibility matrix, fixture hashes, exact test manifests, N-1 determination, and M7-17 artifact handoff. See `docs/M7-16i-fault-compatibility-matrix.md`. |

The sequence intentionally places M7-16c before migration crash testing: current M7-10a tests a contract backend and restore-point builder, but it does not expose a production file-store migration commit route. M7-16c through M7-16e now provide real file-store evidence for that route; Linux/macOS execution remains deferred to M9-07.

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

Follow-up: M7-16a through M7-16i are complete for the Windows phase. Their process boundaries, reopen invariants, fixed fixture identities, command/test manifests, and release applicability are consolidated in `docs/M7-16i-fault-compatibility-matrix.md`; per-task details remain in `docs/M7-16a-backup-crash-matrix.md`, `docs/M7-16b-restore-crash-matrix.md`, `docs/M7-16c-persistent-migration-commit.md`, `docs/M7-16d-guarded-file-store-integration.md`, `docs/M7-16e-migration-process-crash-matrix.md`, `docs/M7-16f-export-process-crash-matrix.md`, `docs/M7-16g-purge-process-crash-matrix.md`, and `docs/M7-16h-versioned-fixture-baseline.md`. M7-17 takes these paths as explicit inputs. Linux/macOS execution remains deferred to M9-07.
