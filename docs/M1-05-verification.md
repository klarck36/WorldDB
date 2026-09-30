# M1-05 – Schema-Definitionstypen

Status: **DONE**\
Prüfdatum: 2026-09-29 16:20 CEST

## Ergebnis

`schema.rs` enthält die revisionierten Definitionstypen `EntityTypeDefinition`, `PredicateDefinition` und `EventKindDefinition` sowie die zugehörigen geschlossenen Typen. Predicate-Eingaben werden über `PredicateDefinitionSpec` aufgebaut und im Konstruktor strukturell geprüft. `ValueKind` enthält genau die zehn Varianten des M1-04-Value-Katalogs. `EntityTypeConstraint` unterscheidet `AnyEntity` und `Exact(EntityTypeId)`.

`PredicateDefinition` prüft die Objektbedingung exakt: `ValueKind::Entity` verlangt ein `object_constraint`; jeder andere ValueKind weist eine gesetzte Objektbedingung ab. `ConstraintSet` ist geschlossen, sortiert Regeln kanonisch und weist doppelte Regeltypen ab. Jeder Constraint-Typ ist an genau einen ValueKind gebunden. Ranges brauchen mindestens eine Grenze und eine geordnete Grenze; Mengen sind nicht leer, sortiert und duplikatfrei.

Die Kardinalitäts-Policy-Matrix lautet:

| Cardinality | Aufnehmbare ResolutionPolicy |
|---|---|
| `Single` | `SingleValueReplace` |
| `Multi` | `MultiValueOverlay`, `MultiValueReplace` |

Inkompatible Kombinationen liefern `SchemaDefinitionError::CardinalityPolicyMismatch`. `Single` begrenzt die Anzahl gespeicherter Assertions nicht; widersprüchliche Assertions bleiben fachlich darstellbar und werden später von Resolution als Conflict behandelt. Die getrennten Laufzeitprüfungen und Konfliktresultate bleiben M2-08/M4-04a-Folgebelegen zugeordnet.

`DecimalFieldMetadata` liegt neben der Predicate- oder EventAttributdefinition und enthält optionale Zähler für Darstellungspräzision, Messpräzision und Currency Scale. In dieser API zählen die Werte jeweils fractional decimal places (`u32`). Metadaten sind ausschließlich für Decimal-Felder zulässig. Die Tests belegen zwei Revisionen derselben PredicateId mit unterschiedlicher Präzisionsanzeige, während `Decimal("1") == Decimal("1.00")` bestehen bleibt.

Lifecycle-Revisionen erhöhen `created_revision`, behalten die Schema-ID und erlauben keinen Rückschritt (`Active → Deprecated → Retired`). EventKind-Definitionen führen erlaubte Rollen mit EntityType-Bedingung und `min..=max`-RoleCardinality sowie erlaubte/erforderliche, getypte Attribute samt Constraints. Die EventTime-Regel speichert Instant-/Span-Form und optionale maximale CalendarPeriod; letztere ist bei `InstantOnly` unzulässig. `CalendarPeriod` ist ein separater Schema-/Query-/Migrationstyp, kein `Value`; sein kanonischer Monatsteil liegt in `0..12`.

## Grenzen dieser Aufgabe

Diese Definitionstypen historisieren Daten nicht selbst und prüfen keine registrierten EntityType-, Timeline- oder TimeUnit-Referenzen. `TimeRange` kann Timeline-Gleichheit prüfen; bei verschiedenen Unit-Symbolen muss der ausgewählte Schema-Snapshot die Endpunktordnung noch exakt normalisieren. Diese Snapshot-/Registry- und Laufzeitvalidierung gehört zu den späteren Schema-/Validation-Aufgaben. Auch die Deprecated-Write-Opt-in-, Capability- und Warnungsprüfung folgt später WDB-SCH-010. Das kanonische TLV-Encoding und Decoderbudgets folgen M1-16/M1-18.

## Belege

- `crates/worlddb-core/src/schema.rs`: Definitionen, validierende Konstruktoren, Constraints, Lifecycle und EventKind-Schema.
- `crates/worlddb-core/src/values.rs`: Symbol-, Time-, Bytes- und geschlossene Value-Grundtypen.
- `crates/worlddb-core/src/lib.rs`: öffentliche Reexports und Compile-Fail-Grenze für `CalendarPeriod` als Nicht-Value.
- `cargo test --locked --offline --workspace --all-targets`: 47 Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 22 Dokumentationstests bestanden (21 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: Struktur und DAG gültig.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.
