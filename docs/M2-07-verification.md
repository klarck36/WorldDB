# M2-07 verification: assertion masking

**Status:** PASS for assertion Mask filtering in the index-free reference path. Checked 2026-09-30T13:51:59+02:00.

## Implemented

- `apply_assertion_masks` consumes only an already security-filtered candidate set. It returns a subset of those immutable Assertion candidates; it cannot create a positive/negative fact or turn a Mask into assertion polarity.
- ExactAssertion, Proposition, and Slot selectors are evaluated separately. Proposition equality is Subject + Predicate + Value + Polarity; context, identity, validity, and creation revision are excluded. Scalar values use exact canonical equality. Temporal values require the caller's comparator bound to the selected schema; unavailable comparison fails closed.
- Only a Mask in the same Perspective/Epistemic partition, selected layer set, and query HistorySpace ancestry can act. Its computed HistorySpace-before-Layer precedence must be strictly greater than the target candidate's precedence. Equal or lower precedence has no effect.
- Mask creation, optional world-time validity, historical validity closure, Transaction-Time retraction, and caller-provided archive visibility are applied independently. Invalid lifecycle references, duplicate lifecycle identities, invalid revision order, incomparable timelines, and out-of-range closures fail closed.

## Verification

- Six focused `mask_projection::tests` pass: exact-target precedence, the distinct Proposition and Slot scopes (including value/polarity mismatch), equal precedence and partition isolation, temporal comparator fail-closed behavior, independent closure/retraction time axes, half-open validity, and future-record visibility.
- `cargo test --locked --offline --workspace` — PASS: 180 Core tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS through the canonical verifier.
- `cargo xtask verify` — PASS: 30 steps, one expected M0-14 `ci-matrix` skip, zero failures.
- `python -B WorldDB_1.0_Plancheck.py` — PASS; 236 tasks, 11 milestones, 253 invariants and 161 follow-up pairs validate.

## Scope

The caller provides archive-visible Mask IDs and a comparator for temporal Values from its pinned schema; Mask lifecycle projection itself validates its own history. Security filtering must precede candidate construction, as required by WDB-RES-003; its end-to-end non-interference proof remains M3-15. An empty filtered candidate set remains an empty candidate set; mapping it to the public `Unknown` resolution outcome is covered by M2-08 and later policy tests.
