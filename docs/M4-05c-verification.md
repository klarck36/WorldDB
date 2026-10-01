# M4-05c verification — atomic security policy changes

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Added typed, append-only `SecurityPolicyRecord` and all eight contract change variants: principal registration/state changes, role registration/retirement/assignment/revocation, and capability-rule addition/revocation. Empty change sets fail closed.
- `commit_security_policy_change` rechecks current `SecurityPolicyManage` against the history base, computes exactly the next shared revision and `SecurityEpoch`, passes the complete old/new policy and typed record to the caller's authoritative post-state validator, and binds a required successful `SecurityPolicyChange` audit record to that revision and `OperationId`.
- The backend publishes one `SecurityPolicyCommitBatch` containing the policy record, policy version, epoch and required audit record. `SecurityPolicyHistory` advances only after successful publication. Stale heads return `Conflict`; authorization, validation, audit, empty-record and backend failures have no partial history or backend effect.
- The user-confirmed strict archive timing rule is unaffected: archive transitions must be later than target creation.

## Verification results

- Focused `security_transaction` tests: 3 passed, covering atomic publication, denied/invalid/empty/rejected requests and backend failure without partial state.
- `cargo test --locked --workspace`: 335 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Contract-source verification and Plancheck passed: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs.
- `cargo xtask verify`: 30 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. The M0-13 evidence run passed.
- `git diff --check HEAD`: passed; Git emitted only line-ending conversion advisories.

## Integration boundary

This core adapter publishes the typed policy transaction batch through the generic revision backend; it does not itself implement a durable security-policy table or merge arbitrary domain records into this specialized batch. The caller supplies the authoritative policy post-state validator, including typed-change consistency, affected-ID and scope checks. Production storage must map the single backend publication to one atomic durability unit, and the normal mixed-domain write integration must preserve the same shared revision and audit commitpoint. Existing cursor/page paths bind `SecurityEpoch`; a policy commit increments that epoch so the next page validation can reject an old cursor.
