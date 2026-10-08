# M7-10a – Restrictive and Breaking migration verification

## Scope and decision

The guarded core executor accepts only `Restrictive` and `Breaking` plans. It rechecks current `MigrationExecute` permission, the exact source revision and schema fingerprint, transformer version, plan fingerprint, validated-decision context, current policy fingerprint, ordered input fingerprint, and finite work and memory budgets before publishing a step.

The confirmed audit policy is Required Audit for both categories. Every step binds its plan, source input, decision fingerprint, target schema, OCC base and commit revisions, operation identity, and Required Audit Record. Publication is exposed through `MigrationCommitBackend`, whose contract requires the migration batch and audit record to share one atomic commitpoint. A later rejected step preserves the already committed and audited prefix.

Breaking plans also require an explicit administrator action bound to current rights and a restore proof. `ExactBackupManager::create_migration_safe_restore_point` creates and verifies an exact backup, performs a real clone restore in a separate destination, reopens the clone, and checks the original source identity and revision against the clone's new identity and advanced revision. The proof is bound to the plan, source commit, backup inventory and manifest digests, clone destination, and independent verification fingerprint.

## Evidence

- `migration_execution::tests::restrictive_migration_binds_plan_decisions_and_each_required_audit_atomically` checks decision/plan fingerprints, per-step audit IDs, and guarded step markers.
- `migration_execution::tests::guarded_later_step_rejection_keeps_the_audited_committed_prefix` rejects the second intermediate schema and verifies that only the first data step and its audit record remain published.
- `migration_execution::tests::breaking_migration_fails_closed_without_proof_or_explicit_admin_action` verifies that a missing restore proof or administrator confirmation prevents any commit; the positive path commits both audited steps.
- `m7_10a_migration_guard_contract::breaking_migration_restore_point_requires_exact_backup_and_real_verified_clone` checks the proof against an exact backup and an actual restored clone with a distinct database identity.
- The fixed wire golden and independent frame oracle include the new guarded migration marker with plan and decision fingerprints; all 52 registered record goldens round-trip.
- `cargo test --locked --workspace`, `cargo check --locked --workspace --all-targets`, strict Clippy, formatting, Plancheck, Sourcecheck, and `git diff --check` passed on Windows. The project Verify result is recorded in the task register.

## Current integration boundary

At the initial M7-10a review, the production persistent adapter was still open. M7-16c subsequently delivered `FileMigrationCommitBackend`, which writes the migration batch, canonical action, Required Audit Record, and replay snapshot under one WAL commit marker. M7-16d connected guarded execution to a real file-store handle and verified Restrictive and Breaking runs, current authorization, the exact backup/real-restore proof, OCC, Run Journal, and audit binding. Current evidence is in `docs/M7-16c-persistent-migration-commit.md` and `docs/M7-16d-guarded-file-store-integration.md`.

Opening or recovering a database does not start a schema or storage-format upgrade. Schema changes require an explicit migration plan and run; a Breaking migration additionally requires administrator action and a verified restore proof. Published migration steps remain append-only; reversal uses an explicit compensating migration or restores a verified backup as a new clone.

Transformer version 1 currently performs canonical pass-through or its declared calendar shift and does not generate unresolved cases. The guarded path still validates and binds the decision set, while M7-04 tests the individual unresolved-item decisions and their failure cases.

## Platform boundary

Verified on Windows. Linux/macOS validation remains deferred to M9-07 as directed.
