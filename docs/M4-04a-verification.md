# M4-04a – Shape-/Schema-Validierung

**Status:** DONE, lokal verifiziert am 30. September 2026.

## Ergebnis

`validate_assertion_batch` prüft einen vollständigen Satz `AssertionDraft` gegen ein bereits berechnetes Post-Transaction-`SchemaSnapshot`. Die Reihenfolge ist fest: Ressourcenlimits, Predicate-Auflösung, exakter `ValueKind`, getypte Constraints, dann Lifecycle einschließlich Deprecated-Opt-in und exakt kontextgebundener `AssertionCreate`-Capability. Fehler liefern kein `ValidatedAssertionBatch`; damit gibt es aus diesem Validator keinen teilweise validierten Batch zur späteren Staging-Publikation.

Alle neun Constraint-Familien werden ohne Typkoercion geprüft. Zeitwerte werden über den Aufrufer gegen den passenden Zeitkatalog aufgelöst. Erfolgreiche Deprecated-Schreibvorgänge tragen eine typisierte `DeprecatedSchemaWriteWarning` mit Predicate, Schema-Revision und Capability. Retired-Predicates werden abgelehnt. `Single`-Cardinality unterdrückt keine widersprüchlichen Assertion-Drafts.

## Nachweise

- `crates/worlddb-core/src/schema_write_validation.rs`
- Sieben Unit-Tests in `schema_write_validation::tests`: Grenzwerte vor Schemaauflösung; Record- und kombinierte Payload-Limits; exakter Typ und Constraints; `Single` ohne Konfliktunterdrückung; Deprecated-Opt-in, Scope-Capability und typisierte Warning; Retired-Ablehnung; positive und negative Prüfungen aller Constraint-Familien.
- `cargo test --locked --workspace`: 314 Core-, 7 Testkit-, 2 Backend-Contract- und 79 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `WorldDB_1.0_Plancheck.py`: PASS.

## Abgrenzung

Der Validator nimmt das Post-Transaction-Schema als fertigen Snapshot entgegen. Die gemeinsame Vorbereitung und atomare Veröffentlichung von Schema- und Assertion-Änderungen folgt M4-05. Referenz-, Ereigniszeit- und HistorySpace-/Layer-/Perspective-Kontextprüfungen liegen in M4-04b. Die Übernahme der typisierten Warning in ein Commit-Receipt folgt M4-09. Vollständige End-to-End- und Non-Interference-Belege zu WDB-SCH-010 bleiben für M4-04c und M8-11a/M8-26d offen.
