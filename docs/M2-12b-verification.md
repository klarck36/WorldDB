# M2-12b – EventMask and Retraction Verification

## Implemented behavior

`EventHistory::with_masks` adds EventMask records and their independent
EventMaskRetraction records to the index-free Event projection. A mask applies
only when it exists at `RecordedAsOf`, is in a selected layer and visible
archive state, targets an existing Event, has no visible retraction, and has
strictly greater `ContextPrecedence` than the target Event. Equal-precedence
masks have no effect. Context precedence follows the pinned HistorySpace and
Layer snapshots.

EventMask changes only whether an Event is returned by the Event projection. It
does not create an EventRetraction, does not retract Assertions, and does not
cascade to any other record family. EventRetraction remains independently
evaluated. Duplicate IDs, missing targets, invalid lifecycle revision order,
missing archive inventory, and archive revision mismatches fail closed.

## Evidence

- `events::tests::event_mask_requires_strict_precedence_and_retracts_independently`
  covers a mask that is not yet visible, a strictly higher-precedence local mask,
  its explicit retraction restoring Event visibility, independent EventRetraction
  behavior, and a same-precedence mask that cannot hide the Event.
- The Rustdoc compile-fail case in `lib.rs` verifies that `EventMask` cannot be
  used as an Assertion `Mask`; the existing Event/EventRetraction type separation
  remains in force.
- `cargo test --locked --offline --workspace`: 204 core tests and 68 Rustdoc
  tests passed.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and `python -B WorldDB_1.0_Plancheck.py` passed.
- `cargo xtask verify`: 30 passed, one expected M0-14 CI-matrix skip, zero failed.

GitHub and remote CI were not used.
