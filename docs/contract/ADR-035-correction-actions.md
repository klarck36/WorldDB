# ADR-035 – Atomic assertion correction and distinct event correction

**Status:** Accepted for the WorldDB 1.0 working contract

**Task:** M0-04e

**Decision owner:** Luna (contract author)

**Normative detail:** [Correction actions supplement](correction_contract.md)

## Context

The Consolidation Trace preserves a legacy correction command that creates a replacement Assertion, an explicit Retraction of the original, and a `Corrects` Provenance edge. The Master also states that `Corrects` itself never retracts. Event correction is intentionally different: it creates a new Event and `Corrects` edge while leaving the original Event active. The model, commit boundary, permissions, retry behavior, and UI preview needed to make these differences unambiguous.

## Decision

1. `CorrectAssertion` is one ordinary transaction command that writes exactly a new Assertion, an explicit `AssertionRetraction` targeting the original, and `Corrects(new, old)` at one shared Revision. The replacement remains in the same HistorySpace, Layer, Perspective/Epistemic partition, Subject, and Predicate slot; its Value, Polarity, and Validity are explicit and may change.
2. `CorrectEvent` is a separate command that writes exactly a new compatible Event and `Corrects(new, old)` at one shared Revision. It never creates EventRetraction, EventSpanClosure, EventMask, or EventRelation. The original Event stays active unless a separately named EventRetraction is explicitly requested.
3. `Corrects` is explanatory Provenance only. Its fixed endpoint direction, family/slot compatibility, later Transaction Time, duplicate validation, and cycle rules remain owned by Master §31.2 and ADR-026.
4. Both commands require their specialized operation Capability plus every produced record's create/retract Capability, target-read, field, Layer, endpoint, and relationship rights as applicable. Event correction does not require EventRetract. Current authorization is rechecked at commit. One failed component aborts the whole write set; OperationId and CommitOutcome follow the normal transaction contract.
5. The UI previews the exact records and commit effect, shows that Event correction retains the original, and reports success only after the durable CommitReceipt is known. It does not simulate authorization or mutate history in place.

## Consequences

- The Retraction in Assertion correction is an explicit generated record requiring both `AssertionCorrect` and `AssertionRetract`; `Corrects` alone and general Provenance creation never trigger lifecycle effects.
- Event correction does not mean temporal succession or causal ordering. Separate EventRelation and EventRetraction commands retain their own permissions and semantics.
- M2/M3/M4/M5/M8 must model the same outputs, OCC conflicts, authorization composition, idempotent retry, commit uncertainty, UI preview, and injected storage/audit faults.
- No persisted type or WDB invariant ID is added; ADR-039 later closes the 52 HARD source gaps; the two GUARDED gaps remain open.

## Verification obligations

The assertion reference model must show exactly three outputs or none, with pre-/post-correction RecordedAsOf behavior. The event reference model must show exactly two outputs while retaining the original. Negative cases cover incompatible slots/kinds, already-retracted targets, missing rights for each output/field/endpoint, denied policies, duplicate/cycle conflicts, concurrent corrections, cancellation/fault before commit, crash after commitpoint, and OperationId mismatch/replay.
