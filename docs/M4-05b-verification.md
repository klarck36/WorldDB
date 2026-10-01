# M4-05b verification — Archive and HistorySpace transfer transactions

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- `archive_transaction.rs` validates an append-only archive post-state against the pinned base, checks the exact staged `ArchiveTransition` delta, confirms newly introduced targets are staged exactly once at the commit revision, re-evaluates the per-record `Archive` or `Unarchive` capability, and rebuilds the validated archive projection.
- `transfer_transaction.rs` checks current `HistorySpaceTransfer` rights for source and destination, clones the transfer reference model, builds the transfer receipt on that candidate, invokes complete caller-supplied Record post-state validation, then publishes the mixed Record batch once. The original reference model advances only after publication. Stale database or target heads return a Conflict outcome.
- Transfer archive preservation follows the strict rule that an ArchiveTransition is later than target creation. `PreserveArchiveState` now fails closed if selected sources are archived; `StartUnarchived` may transfer atomically, followed by a separate archive transaction if desired. The accepted supplement, ADR-031 and generated Master copy reflect this behavior.
- No archive cascade is created. Raw target history remains append-only; source, parent and sibling HistorySpaces are not rewritten.

## Verification results

- Focused archive transaction tests: 2 passed, including deny without backend publish and no cascade.
- Focused transfer transaction test: 1 passed, including one shared revision for the target Assertion and its `DerivedFrom` Record.
- Transfer reference tests: 10 passed, including strict archive-preservation rejection with unchanged heads.
- `cargo test --locked --workspace`: 332 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long-running campaigns remained intentionally ignored.
- Clippy with `-D warnings`, formatting, workspace exception policy and contract-source regeneration/verification passed.
- `cargo xtask verify`: 30 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL.
- `WorldDB_1.0_Plancheck.py`: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs structurally valid.
- `git diff --check HEAD`: passed; Git emitted only line-ending conversion advisories.

## Boundary

The transaction adapter accepts a full post-state validator callback because the generic transfer reference model does not construct every domain Record family. The production integration must provide that callback with the authoritative schema, graph, cross-record and staged-batch checks before publish. The adapter itself binds the receipt, current transfer rights, candidate model and one backend publish.
