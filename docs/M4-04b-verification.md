# M4-04b – Referenz-/Zeit-/Context-Validierung

**Status:** DONE, lokal verifiziert am 30. September 2026.

## Ergebnis

`validate_write_references` gibt Assertion- und Event-Drafts nur gemeinsam zurück, wenn alle Referenzen und Kontexte valide sind. Es prüft HistorySpace, Layer, Perspective, Entity, EntityType, Predicate, EventKind, Event-Rolle, Timeline und TimeUnit. Retired Entities, Layer, Perspective, Schema-Typen, EventKinds, Timelines und TimeUnits werden fail-closed behandelt; neue Time-Werte unter Deprecated/Retired TimeUnit oder Timeline werden abgewiesen.

Layer-, Entity- und Perspective-Katalog müssen dieselbe Revision wie das gewählte Post-Transaction-Schema tragen. Assertions müssen auf existierende, nicht retired Subjects und entity-valued Objects zeigen und die jeweiligen Exact-EntityType-Constraints erfüllen. Assertion-Validity und EventTime müssen registrierte Timelines verwenden. Time-Values verlangen zudem eine aktive registrierte TimeUnit. Event-Drafts müssen dieselbe EventKind-Revision wie das ausgewählte Schema verwenden; Teilnehmer und entity-valued Attribute werden gegen die Rolle beziehungsweise das Attributschema geprüft.

## Nachweise

- `crates/worlddb-core/src/write_reference_validation.rs`
- Sieben Unit-Tests: vollständige Assertion-Referenzen; gültiger Perspektivenkontext; EventKind-/Rollen-/Entity-/Zeitbezüge; ein Event mit einer Rolle, die nur in einem abweichenden EventKind-Snapshot vorkommt, wird gegen das ausgewählte Post-Transaction-Schema abgelehnt; abweichende Katalogrevision; retired Layer und unbekannte Timeline; unbekannte und Deprecated TimeUnit.
- `crates/worlddb-core/src/events.rs`: read-only EventDraft-Zugriffe auf die referenzrelevanten Felder.
- `cargo test --locked --workspace`: 320 Core-, 7 Testkit-, 2 Backend-Contract- und 79 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `WorldDB_1.0_Plancheck.py`: PASS.

## Abgrenzung

Der Validator bekommt Lifecycle-Maps für Timeline und TimeUnit als Teil des vom Aufrufer gepinnten WriteReferenceSnapshot. Die aktuellen `SchemaDefinition`- und Wire-Record-Familien enthalten dafür noch keine erstklassigen Definitionstypen. Das ist hier ein klarer Input-Port, keine Persistenz- oder Decoderbehauptung. Die Normalisierung von Time-Werten und Predicate-Constraints bleibt bei M4-04a; zusammengesetzte Security-/Cross-Record-Prüfungen folgen M4-04c, gemeinsames Schema-/Daten-Publish folgt M4-05. Deprecated Timeline/TimeUnit wird bis zu einem expliziten autorisierten Warning-Pfad fail-closed abgewiesen.
