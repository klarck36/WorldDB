# M2-11 verification: Epistemic partitions

**Status:** PASS for the index-free assertion reference path. Checked 2026-09-30.

## Implemented and verified

- `AssertionCandidateQuery` selects one explicit `(PerspectiveScope, EpistemicMode)` partition. The full scan returns only assertions in that exact partition; WorldState is queried with `PerspectiveScope::World`, while Knows, Believes, and Claims require a concrete Perspective.
- A new four-mode truth-table test stores WorldState, negative Knows, Believes in another Perspective, and Claims. Each query returns only its matching record. A missing Believes query remains empty even when a Claims record for the same Perspective exists.
- Negative polarity in Knows is returned as an explicit negative assertion. Missing Believes remains absent and therefore resolves to Unknown through the M2-08 no-candidate rule; it is not converted into a negative belief.
- Partition identity is not a precedence coordinate and there is no automatic propagation between WorldState, Knows, Believes, or Claims.

## Verification

- `cargo test --locked --offline --workspace` — PASS: 200 Core tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` and `python -B WorldDB_1.0_Plancheck.py` — PASS.
- `cargo xtask verify` — PASS: 30 steps, one expected M0-14 `ci-matrix` skip, zero failures.

## Scope

This task verifies storage/query partition separation and the existing Unknown behavior. It does not equate a Perspective with a security Principal; that mapping remains M3-03, and authorization remains before candidate construction under M3-04.
