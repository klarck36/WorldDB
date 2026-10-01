# M2-15c – Assertion Correction Action Verification

## Implemented behavior

`prepare_assertion_correction` requires the immutable target creation revision
expected by the caller and a target active at the pinned `RecordedAsOf`
snapshot. A Retraction effective at that snapshot rejects the command; a
Retraction after the snapshot but before the proposed commit rejects it as a
stale target. A commit revision must follow the base snapshot.

The complete replacement draft must preserve the exact HistorySpace, Layer,
Perspective/Epistemic context, Subject, and Predicate of the target. Its Value,
Polarity, and validity remain explicit replacement fields. Success returns
one `PreparedAssertionCorrection` containing exactly three immutable records:
the new Assertion, an explicit AssertionRetraction for the target, and
`Corrects(new, old)`. All three use the same commit revision. Validation errors
return no record bundle, so no subset is exposed for publication. The Corrects
edge itself has no retraction side effect.

Event correction remains separate: `prepare_event_correction` returns a new
Event plus Corrects edge and leaves the old Event active without an implicit
EventRetraction.

Schema validation, endpoint authorization, and durable atomic publication
remain at their later operation/transaction boundaries; this task supplies the
typed reference action and its all-or-nothing record set.

## Evidence

- `assertion_correction::tests::correction_prepares_exactly_three_records_at_one_commit_revision`
- `assertion_correction::tests::correction_rejects_wrong_slot_or_context_without_returning_any_record`
- `assertion_correction::tests::correction_rejects_stale_future_or_retracted_targets`
- `assertion_correction::tests::correction_commit_must_follow_the_base_snapshot`
- `source_provenance::tests::assertion_corrects_requires_same_proposition_slot_and_later_record`
- `events::tests::event_correction_adds_new_event_and_corrects_edge_but_keeps_original_active`
- `cargo test --locked --offline --workspace`: 230 core tests and 70 Rustdoc
  tests passed; two intentionally long fuzz/precision campaigns remain ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and Plancheck passed.
- `cargo xtask verify`: 30 PASS, 1 expected M0-14 SKIP, 0 FAIL.

GitHub and remote CI were not used.
