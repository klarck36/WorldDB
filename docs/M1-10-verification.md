# M1-10 – Mask und ReplacementBoundary

Status: **DONE**\
Prüfdatum: 2026-09-29

## Ergebnis

`crates/worlddb-core/src/masks.rs` definiert die geschlossene `MaskSelector`-Familie mit `ExactAssertion`, `Proposition` und `Slot`. `PropositionKey` enthält genau Subject, Predicate, Value und Polarity. `MaskSlotSelector` akzeptiert nur gültige Perspective-/Epistemic-Paare; beim Bau der Mask muss ein Slot außerdem dieselbe Partition wie der Mask-Context verwenden.

`Mask` und `ReplacementBoundary` sind eigene unveränderliche Records mit konkreten IDs, Context, optionaler World-Time-Validity und `created_revision`. Mask-Closure, Mask-Retraction, Boundary-Closure und Boundary-Retraction sind ebenfalls separate Records mit ihren konkreten Lifecycle-IDs. Closure prüft die Timeline, wenn das Ziel eine explizite Validity besitzt; alle Lifecycle-Records müssen eine spätere Transaction-Time-Revision als ihr Ziel verwenden.

`ReplacementBoundary::new` übernimmt die konkrete `PredicateDefinition` und akzeptiert nur `MultiValueReplace`. Eine Boundary enthält keine Assertion-Liste: ihre Existenz markiert die vollständige Ersetzungsmenge. Fehlen passende Assertions, repräsentiert sie damit eine explizite leere Menge.

## Grenzen und Folgebelege

Referenzexistenz, Predicate-/Subject-Constraints gegen den vollständigen Post-Transaction-Schemazustand, Maskwirkung auf ContextPrecedence und Security, Closure-Projektion sowie sichtbares `Known`/`Unknown`/`Conflict`-Verhalten folgen M2-05/M2-07/M2-08/M2-10. `PropositionKey` bietet absichtlich keine strukturelle Value-Gleichheit; insbesondere Time-Werte benötigen die schema-aware Vergleichslogik. Codec- und Decoderfälle folgen M1-17b.

## Belege

- `crates/worlddb-core/src/masks.rs`: Selector-, Mask-, Boundary- und Lifecycletypen samt Konstruktorvalidierung.
- `crates/worlddb-core/src/lib.rs`: öffentliche Exporte und Compile-Fail-Fälle für Selectorfamilie und Boundary/Mask-Trennung.
- `masks::tests::mask_selectors_are_closed_and_slot_masks_stay_in_one_partition`: positive Selectorformen und negative Partitionkombinationen.
- `masks::tests::replacement_boundary_requires_multivalue_replace_and_can_be_empty`: Single/Overlay-Abweisung, gültiges MultiValueReplace und explizite leere Boundary.
- `masks::tests::mask_and_boundary_lifecycle_records_keep_distinct_typed_targets`: getrennte Lifecycle-IDs und Revisionsprüfung.
- `cargo test --locked --offline --workspace --all-targets`: 56 Core-, 7 Testkit- und 4 xtask-Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 35 Dokumentationstests bestanden (34 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.
