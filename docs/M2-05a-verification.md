# M2-05a verification: archive projection

**Status:** PASS for the index-free operational archive reference model. Checked 2026-09-30T13:19:17+02:00.

## Implemented

- `ArchiveHistoryReferenceModel` receives a closed inventory of archivable target identities with their creation revisions and immutable `ArchiveTransition` records. It validates unique target/transition identities, known targets, target-before-transition ordering, strictly increasing transition revisions per target, and the effective Archive/Unarchive state sequence.
- `state_at(RecordedAsOf)` projects reversible operational archive state from only transitions committed by that revision. `ordinary_targets_at` hides currently archived targets; `raw_targets_at` still returns every target that existed by the same revision, regardless of archive state. Historical queries before an Archive or after an Unarchive see the correct earlier state.
- The model contains no mutation, Retraction, Closure, or Purge operation. Its immutable target inventory retains archived records; ArchiveTransition cannot target itself because its target type is the closed `ArchiveTargetRef` subset.

## Verification

- Four focused `archive_projection::tests` pass: archived targets remain in raw history, ordinary reads filter only operationally archived targets, Unarchive restores visibility, malformed state transitions/unknown targets fail closed, and targets are absent before their creation revision.
- `cargo test --locked --offline --workspace` — PASS: 170 core unit tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python -B WorldDB_1.0_Plancheck.py` — PASS; 236 tasks, 11 milestones, 253 invariants and 161 follow-up pairs validate.

## Scope

This completes operational archive visibility for the M2 reference model without changing domain history. Storage durability, transaction-integrated archive writes, recovery, and physical Purge remain separate later work. The raw-history query continues to expose archived targets; no purge is simulated by this projection.
