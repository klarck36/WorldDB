# M2-12a verification: Event schema, time, and correction

**Status:** PASS for the index-free Event reference projection. Checked 2026-09-30.

## Implemented

- `full_scan_event_candidates` filters Event records by the pinned HistorySpace ancestry and layer selection, `RecordedAsOf`, operational archive state, EventRetraction, EventSpanClosure, and `EventTimeFilter::{Any, At, Overlaps}`.
- Every candidate is revalidated against the caller's pinned EventKind definition. Role membership/cardinality, required attributes and ValueKinds, and EventTime form errors fail closed, including for locally decoded records.
- `RecordedAsOf` independently determines whether a closure or retraction exists. A visible closure supplies the exclusive end of an originally open span. Point and overlap queries respect half-open EventTime intervals and Timeline identity.
- `prepare_event_correction` returns exactly a new Event and a same-revision `Corrects(new, old)` ProvenanceEdge, wrapped in an EventCorrection receipt. Its type has no implicit EventRetraction or EventRelation effect. EventKind successor compatibility is checked through the caller's pinned-schema predicate.
- An Event and an Assertion remain distinct record types. Event participants do not create Knows/Believes/Claims records; an explicit negative Knows Assertion remains distinct from missing Believes.
- Two distinct Events at the same EventTime, including a corrected Event and its original, both remain visible; co-temporal records do not imply ordering or automatic deduplication.

## Verification

- Focused tests pass for closure-before/after RecordedAsOf, open-span overlap, half-open endpoints, event retraction, malformed decoded role/time shape, and Event correction retaining its original at the same EventTime.
- Existing Event tests cover allowed role membership, role cardinality, participant canonicalization/duplicates, required and typed attributes, EventTime form and span shape, and lifecycle closure construction.
- `cargo test --locked --offline --workspace` — PASS: 203 Core tests and 67 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check`, `python -B WorldDB_1.0_Plancheck.py`, and `git diff --check HEAD` — PASS.
- `cargo xtask verify` — PASS: 30 steps, one expected M0-14 `ci-matrix` skip, zero failures.

## Scope

The correction helper prepares the domain pair but does not implement atomic persistence/commitpoint; transaction integration is a later milestone. EventMask visibility is M2-12b. Authorization must happen before the candidate projection under M3-04.
