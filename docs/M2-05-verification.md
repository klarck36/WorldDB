# M2-05 verification: assertion validity and lifecycle projection

**Status:** PASS for the index-free assertion history projection. Checked 2026-09-30T13:15:23+02:00.

## Implemented

- `AssertionLifecycleProjection` retains immutable Assertions, `AssertionValidityClosure` records, and `AssertionRetraction` records. Construction sorts by typed ID and validates duplicate IDs, lifecycle targets, strictly later Transaction-Time revisions, closure timeline identity, and closure times within the target's original validity bounds.
- `candidates(RecordedAsOf, WorldTime)` applies two independent time axes. An Assertion created after `RecordedAsOf` is absent. A closure affects results only if its own creation revision is visible and its exclusive close time has been reached in World Time. A retraction takes effect at and after its visible creation revision, independent of World Time.
- Assertion validity remains `[start,end)`: start is included, end excluded. A bad timeline or out-of-range closure fails closed. Decoded lifecycle fields are revalidated against the target instead of trusting constructor-only checks.
- Raw immutable assertions remain available through `assertions()`; this projection does not edit or erase history, perform archive filtering, or implement atomic Correct transactions.

## Verification

- Six focused `assertion_projection::tests` pass, covering creation revision, half-open validity bounds, closure effective World Time and RecordedAsOf, retraction effective revision, malformed decoded lifecycle fields, missing targets, closure range and timeline errors.
- `cargo test --locked --offline --workspace` — PASS: 166 core unit tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python -B WorldDB_1.0_Plancheck.py` — PASS; 236 tasks, 11 milestones, 253 invariants and 161 follow-up pairs validate.
- `cargo xtask verify` — PASS: 30 steps passed, 1 expected M0-14 `ci-matrix` skip, 0 failures.

## Scope

This completes assertion World-Time and Transaction-Time projection for the M2 reference model. Archive state is deliberately not interpreted here; that is M2-05a. Candidate collection, resolver semantics, storage, publication checks for caller-supplied `RecordedAsOf`, and atomic correction remain in their planned tasks.
