# ADR-032 – Closed constraints and registered time semantics

**Status:** Accepted for the WorldDB 1.0 working contract

**Task:** M0-04b

**Decision owner:** Luna (contract author)

**Normative detail:** [Constraints and time registration supplement](constraint_time_contract.md)

## Context

Master §31.3 uses `ConstraintSet` without defining its variants. Master §12 stores `Time` as a TimelineId, signed ticks, and Unit, but does not define registration, scale conversion, or lifecycle. ADR-025 keeps `CalendarPeriod` outside `Value` while allowing schema, query, and migration use; those allowed operations and calendar rules were unspecified.

## Decision

1. `ConstraintSet` is a closed conjunction of typed, deterministic value rules with inclusive checked ranges and no user-code or generic escape hatch. Parsing, canonical ordering, type matching, and evaluation are specified. `EntityTypeConstraint` remains separate. Decimal presentation metadata remains separate from numeric identity.
2. Timeline definitions and positive integer nanosecond TimeUnit scales are project-schema records on the shared Revision axis. A TimeUnit's Symbol and scale and a TimelineId's calendar mapping are immutable. Times normalize exactly within one Timeline; different TimelineIds remain incomparable.
3. `CalendarPeriod` is a non-negative canonical `(years, months, days)` schema/query/migration parameter, never an Assertion Value. Only a Timeline with an explicit proleptic Gregorian UTC projection supports it. Month-end clamping, operation order, half-open query windows, overflow, and migration faults are explicit.
4. Schema parsing, decoding, validation, query planning, and migration use the same closed types and checked arithmetic. Schema evolution uses the existing Historical/Current/Explicit and migration classification rules.

## Consequences

- `EventKindDefinition` may carry an optional `max_calendar_span` for span-capable EventTime. An open span is checked when its end is recorded.
- Query callers supply the anchor and Timeline; no current-time or timezone default enters a historical query.
- Unit-scale/calendar changes require new stable symbols/TimelineIds and explicit migrations. Unknown units, cross-Timeline comparisons, rounding, and overflow are errors.
- M1-02 implements time types and intervals, M1-04 enforces the CalendarPeriod/Value boundary, M1-05 builds schema constraints, and M2 query/migration work uses the same semantics.

## Verification obligations

The supplement is the acceptance checklist. Parser, validator, migration, and query tests must share boundary vectors for all constraints, exact unit normalization, Timeline incompatibility, leap-year/month-end behavior, half-open windows, open-span closure, historical schema snapshots, canonical fingerprints, and overflow/error atomicity.
