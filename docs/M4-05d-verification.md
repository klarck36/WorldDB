# M4-05d verification — domain and lifecycle write matrix

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Added `policy/write-record-matrix.tsv`, mapping every current First-Class type and each wire-persisted `Record` variant to its write surface, validation boundary, and atomic transaction path. The 24 First-Class entries include the later accepted `TransferLineage` and `SecurityPolicyRecord` additions.
- Added `tools/check_write_record_matrix.py` and registered it in `cargo xtask verify`. It reads the canonical First-Class register and Rust `Record` enum, then rejects missing, duplicate, unknown, or unreferenced entries and paths to missing source files.
- Added `commit_assertion_correction` and `PreparedAssertionCorrection::into_records`. The helper requires the current backend head to equal its base, requires the prepared next revision, stages exactly one new Assertion, one explicit target Retraction, and one `Corrects` Provenance edge, and invokes the complete caller validator before the single mixed-batch publish.
- Existing Event correction semantics remain separate: a new Event and `Corrects` edge do not imply EventRetraction, EventMask, or EventSpanClosure. A standalone `Corrects` edge remains a normal independently validated relationship write.

## Verification results

- Focused `assertion_correction` tests: 5 passed, including publication of the exact three-record correction batch at one revision.
- `tools/check_write_record_matrix.py`: 24 First-Class types and 36 wire Record variants, each mapped exactly once.
- `cargo test --locked --workspace`: 336 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `WorldDB_1.0_Plancheck.py`: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs structurally valid.
- Contract-source verification passed.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL; the M0-13 evidence run passed.
- `git diff --check HEAD`: passed; Git emitted only line-ending conversion advisories.

## Integration boundary

The matrix makes callback boundaries explicit; it does not replace each domain's validator with one universal function. `commit_mixed_record_batch` still requires the caller to run the listed schema, reference, security, lifecycle, and cross-record checks before publish. This task verifies the in-memory reference transaction path, not durable storage or crash recovery. Runtime-only types such as Transaction, Snapshot, and Job do not have `Record` enum variants and remain on their dedicated handle/state paths.
