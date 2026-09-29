# WorldDB 1.0 supplement – Constraints and time registration

**Status:** accepted working contract for M0-04b

**Decision record:** [ADR-032](ADR-032-constraint-time.md)

**Scope:** completes Master §§2.1, 2.3, 12 and 31.3 for `ConstraintSet`, project `Timeline`/`TimeUnit` registration, and the schema/query/migration uses of `CalendarPeriod`. It adds no new WDB invariant IDs and leaves the M0-02a source gaps open.

## 1. Closed `ConstraintSet`

`ConstraintSet` is a typed schema value, not executable code or arbitrary JSON:

```text
ConstraintSet { rules: List<ValueConstraint> }

ValueConstraint =
    BoolSet(NonEmptySet<Bool>)
  | IntRange(InclusiveRange<Option<Int>>)
  | UIntRange(InclusiveRange<Option<UInt>>)
  | DecimalRange(InclusiveRange<Option<Decimal>>)
  | StringByteLength(InclusiveRange<Option<UInt>>)
  | SymbolSet(NonEmptySet<Symbol>)
  | TimeRange(InclusiveRange<Option<Time>>)
  | DurationRange(InclusiveRange<Option<Duration>>)
  | BytesLength(InclusiveRange<Option<UInt>>)

InclusiveRange<T> { min: Option<T>, max: Option<T> }
```

An empty `rules` list means unconstrained. A range must have at least one bound; when both are present, `min <= max`. Bounds are inclusive. `TimeRange` endpoints must use the same `TimelineId`; unit conversion is exact. Sets are non-empty, sorted canonically, and contain no duplicates. Only one rule of each variant may occur in a set. Different rules compose by conjunction.

Parsing recognizes only the listed variant tags and typed payloads. It rejects an unknown tag, missing payload, wrong scalar type, duplicate variant, empty set, malformed Symbol, or range with no bounds. Canonical output sorts rules by variant tag, set members by their canonical scalar encoding, and preserves range endpoints; it does not coerce values or repair invalid input. For a bounded range, equality of `min` and `max` denotes a valid singleton and `min > max` is invalid. Validation checks each value against its exact `ValueKind`, then requires every rule to pass. These same parse, canonicalization, and evaluation rules apply to persisted schema, API schema input, and migration input.

Each rule must match the `ValueKind` it constrains. `Entity` is constrained by `EntityTypeConstraint`, not by `ConstraintSet`. `DecimalRange` compares exact numeric values and never constrains or changes Decimal scale. String length counts UTF-8 bytes without Unicode normalization; byte length counts bytes. Symbols must pass the existing Symbol grammar. A rejected value produces a structural `ConstraintViolation` before commit; it is distinct from a resolution `Conflict`.

1.0 has no regex, callback, plugin, user code, locale-dependent validator, implicit coercion, or generic escape variant in `ConstraintSet`. Decimal display precision, measurement precision, and currency scale are schema/field metadata, not value constraints or part of Decimal equality. An unknown constraint variant is rejected.

## 2. Project-wide Timeline and TimeUnit registries

Timeline and TimeUnit definitions are immutable project-schema records on the shared `Revision` axis. The operation's selected `SchemaSnapshot` supplies their definitions: writes use the Post-Transaction snapshot, while reads, queries, and migrations use the Historical/Current/Explicit source or target snapshot required by their contract. Registration rejects a duplicate TimelineId, Timeline Symbol, or TimeUnit Symbol; retired symbols and IDs are never reused. A lifecycle change appends a new revisioned definition state and does not edit an earlier record; the stable identity, symbol, and numeric/calendar mapping remain fixed.

```text
TimelineDefinition {
  timeline_id: TimelineId,
  symbol: Symbol,
  calendar: None | ProlepticGregorianUtc { epoch_unix_nanoseconds: i128 },
  lifecycle: Active | Deprecated | Retired,
  created_revision: Revision
}

TimeUnitDefinition {
  symbol: Symbol,
  nanoseconds_per_tick: UInt64NonZero,
  lifecycle: Active | Deprecated | Retired,
  created_revision: Revision
}

Time { timeline_id: TimelineId, ticks: i128, unit: Symbol }
```

The TimeUnit registry key is a validated `Symbol`; 1.0 introduces no `TimeUnitId`. Its scale is a positive integer nanosecond count. Calendar units such as month and year are not TimeUnits because their lengths vary. A Time value's checked `ticks * nanoseconds_per_tick` must fit signed i128 nanoseconds. Exact normalized nanoseconds are its comparison/equality coordinate within one Timeline; the stored unit preserves the requested tick representation. Explicit conversion to another TimeUnit fails if it would require rounding. A registry snapshot resolves every Symbol before a value can be accepted; an unregistered unit or Timeline is invalid.

TimeUnits are project-wide and may be used by any Timeline. A unit's symbol and numeric scale never change. A different scale requires a new symbol. TimelineId and its symbol are stable; a Timeline's calendar profile and epoch never change. Changing a calendar mapping requires a new TimelineId and an explicit migration. Different TimelineIds remain incomparable even if their units, symbols, or calendar profiles match. No timezone or unit is inferred.

New-value creation and writes require registered definitions at the operation's post-transaction schema. `Active` permits new values. `Deprecated` values require the existing explicit opt-in, capability, and typed warning contract. `Retired` definitions reject new values. Historical decoding and reads continue to use the definition valid at the record's revision. Retirement does not delete definitions or rewrite stored times. A TimeRange and both compared Time values must resolve to the same Timeline; cross-Timeline ordering or conversion fails as `IncomparableTimeline`.

## 3. `CalendarPeriod`

`CalendarPeriod` is a closed, structured schema/query/migration parameter, never an Assertion `Value`:

```text
CalendarPeriod { years: UInt32, months: UInt8, days: UInt32 }
```

`months` is canonical in `0..=11`; parser and decoder reject larger values rather than silently normalizing them. `years` and `days` are non-negative, and zero is valid. No field is a sentinel. The triplet is a calendar magnitude; it is not a fixed nanosecond `Duration` and is not comparable by `Value::Ord`.

Calendar arithmetic is available only on a Timeline registered with `ProlepticGregorianUtc`. Its UTC coordinate is checked `epoch_unix_nanoseconds + ticks * nanoseconds_per_tick`, where epoch nanoseconds are measured from `1970-01-01T00:00:00Z`; TimeUnit scales and all arithmetic use checked signed i128 integer nanoseconds. The profile uses the proleptic Gregorian calendar (a year is leap when divisible by 4, except centuries not divisible by 400), astronomical year numbering (year zero exists), UTC, fixed 86,400-second civil days, and no leap-second or daylight-saving adjustments. No host locale, local timezone, or current clock participates. Calendar conversion uses floor division for negative coordinates so pre-epoch instants retain a non-negative time-of-day remainder. An intermediate or result that cannot be represented by the i128 coordinate or the calendar algorithm is `CalendarPeriodOverflow`.

To shift an anchor by a period, preserve its time-of-day and apply years first, months second, and days last. After applying years or months, clamp a day that does not exist in the target month to that month's final day (for example, January 31 plus one month becomes the final day of February; February 29 plus one year becomes February 28 in a non-leap year). Days then advance or retreat by exact civil days. `Past` applies the negative components in the same order; `Future` applies the positive components. Clamping is intentionally not invertible: shifting into a shorter month and then back may not recover the original date. Every conversion, multiplication, addition, and date result is checked; overflow returns `CalendarPeriodOverflow` with no saturation or wraparound.

### Allowed schema use

For this contract, the existing Event-Time enum is represented as `EventTimeConstraint { form: InstantOnly | SpanOnly | InstantOrSpan | OpenSpanAllowed, max_calendar_span: Option<CalendarPeriod> }`; the four existing forms retain their meaning. An absent `max_calendar_span` means no calendar-span limit. This optional field is valid only for `SpanOnly`, `InstantOrSpan`, or `OpenSpanAllowed`; on `InstantOrSpan` it applies only when the record is a span. This is an explicit schema-contract extension governed by WDB-SCH-011–015.

`CalendarPeriod` may appear only as `EventKindDefinition.time_constraint.max_calendar_span`, a maximum calendar span for `EventTime::Span`. It is invalid with `InstantOnly`; it may accompany span-capable constraints. Start and end must use the same Timeline, which must have a calendar profile. The existing EventTime rule `start < end` for a closed span is always checked; the maximum adds `end <= shift(start, max_calendar_span, Future)`. An open span may be stored while pending; the same checks run when `EventSpanClosure` supplies its end. This limit is a structural schema constraint and never changes EventTime or resolution semantics.

Other `ConstraintSet` variants contain no `CalendarPeriod`. It cannot be used as a Predicate value, EntityType, general field constraint, `Duration`, validity endpoint, Revision, or implicit Timeline conversion.

### Allowed query use

Time filters may use absolute typed `Time` bounds or `CalendarRelativeWindow { anchor: Time, period: CalendarPeriod, direction: Past | Future }`. A relative window resolves to `[shift(anchor, period, Past), anchor)` for `Past` and `[anchor, shift(anchor, period, Future))` for `Future`. Anchor and candidates must use the same calendar-capable Timeline. Queries without an explicit anchor do not use `CalendarPeriod`; the engine never supplies the current time. CalendarPeriod filters apply to Time values, EventTime, and AssertionValidity on that Timeline, not to `RecordedAsOf`, `Revision`, `Duration`, or another Timeline.

The resolved absolute interval is part of the query fingerprint and is evaluated against the pinned SchemaSnapshot. Existing `[start,end)` rules apply after resolution. `CalendarPeriod` itself is not stored in an Assertion and does not become a query result Value. A timestamp text input must include an explicit offset and Timeline choice; an adapter cannot assume a local timezone.

### Allowed migration use

A versioned `MigrationPlan` may carry `CalendarPeriod` as a typed parameter for an explicit, deterministic time shift on a calendar-capable Timeline. The plan fingerprint includes the period, TimelineId, direction, and relevant schema preconditions. Dry Run and execution call the same checked arithmetic. Missing calendar profiles, incompatible Timelines, overflow, or an ambiguous mapping produce an `UnresolvedMigrationItem`/error; migration never substitutes a fixed Duration or rounds. Cross-Timeline migration requires an explicit versioned mapping and cannot be inferred from matching units or calendar profiles.

## 4. Validation, query, and migration agreement

1. Parser/decoder construction, schema validation, commit validation, query planning, and migration validation use the same closed variants, canonicalization, range rules, registry snapshot, exact Time normalization, and calendar-shift function. Constraint evaluation is conjunction: `Accepts(set, value, snapshot)` is true exactly when each rule accepts the typed value under that snapshot.
2. Predicate and EventKind references validate against the Post-Transaction SchemaAt snapshot. Schema/data changes commit atomically; historical reads use `SchemaAt(RecordedAsOf)` and never coerce to the current Timeline or unit definition.
3. Constraint edits are classified by the accepted migration contract: a change that narrows writes or changes historical interpretation is `Restrictive` or `Breaking`; a widening is compatible only when existing meanings remain stable. TimeUnit scale and Timeline calendar mapping cannot be edited in place.
4. Query plans and migration plans include every relevant Timeline/TimeUnit definition and typed CalendarPeriod parameter in their canonical fingerprints. Migration dry-run and execution call the same validator and transformer; only the sink differs. Unregistered symbols, stale schema preconditions, invalid bounds, wrong ValueKind, cross-Timeline comparisons, and arithmetic overflow fail explicitly and atomically.

The supplement preserves WDB-VAL-001–006, WDB-TIM-001–003, WDB-SCH-001–017, WDB-MIG-001–023, WDB-WIR-001–005, and WDB-PHIL-001. Implementation tests must cover every `ConstraintSet` variant, range endpoints, unit registry lifecycle and immutability, exact cross-unit comparisons, incomparable Timelines, Gregorian leap/month-end arithmetic, relative query half-open boundaries, span-limit validation and closure, invalid CalendarPeriod encodings, schema evolution, migration dry-run/execution parity, and overflow/fault atomicity.
