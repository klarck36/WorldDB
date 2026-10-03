# M8-07 – Windows verification

**Task status: DONE for the Windows development profile.** Linux/macOS checks remain deferred to M9-07 as requested.

## Implemented behavior

- `v1 purge plan` and `v1 purge run` require explicit typed targets, `reject-if-referenced` or `cascade`, a new destination, and the caller's explicit external-copy inventory completeness declaration.
- Plan inventories the complete logical snapshot, all local index generations and declared external copies. Its path-free report names every target record, dependant, affected record, and the exact plan fingerprint.
- Run recomputes and revalidates the plan under the source writer lock. It accepts only the exact confirmed fingerprint and never replaces the source or an existing destination.
- The storage rewrite publishes a new `DatabaseId`, verifies the retained snapshot and persisted `PURGE_REPORT`, and commits the Required `PurgePublication` audit record with the publication revision.
- Removing a selected HistorySpace adjusts the destination verification scope to retained HistorySpaces; all retained source records are still compared against the rewritten export.
- The report explicitly records known external copies and whether the operator declared their inventory complete. Secure erasure is never claimed.

## Windows evidence

- `cargo test --locked -p worlddb-cli --all-targets`: 30 passed, 0 failed (9 unit tests, 4 adapter-process tests, 17 CLI contract tests).
- Cascade plan reports the exact dependant; reject-if-referenced refuses that same target: PASS.
- Confirmed cascade rewrite publishes a new DatabaseId, verifies cleanly, writes a digest-valid `PURGE_REPORT`, and leaves the source byte-for-byte unchanged: PASS.
- Missing current `Purge` permission and in-place destination are rejected without changing the source: PASS.
- The initial end-to-end run exposed a verifier selecting a HistorySpace removed by the approved plan. Verification now selects only retained HistorySpaces and the regression test passes.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 39 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed. Workspace check, CLI contracts, TypeScript transport, strict Clippy, source check, plan check, Windows crash/reopen suites, and whitespace verification passed.
- Linux/macOS verification: deferred to M9-07.
