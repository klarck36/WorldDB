# M4-04c – Security-/Cross-Record-Validierung

**Status:** DONE, lokal verifiziert am 30. September 2026.

## Ergebnis

`authorize_validated_write_batch` prüft Assertion- und Event-Schreibrechte erneut gegen die aktuelle Policy: Domainoperation, Layer, jedes konkrete Feld, Entity-Referenzen und Perspective-Nutzung sind konjunktiv. Für Assertions wird `AssertionCreate` auf das konkrete Predicate-Wertfeld geprüft. Schema-validierte Assertions müssen im selben Batch und in derselben Reihenfolge mit den referenzvalidierten Assertions übereinstimmen. Jedes Deprecated-Warning wird am Commit-Grenzpfad gegen die aktuelle Capability-Policy und den konkreten HistorySpace-, Layer- und Predicate-Wertfeldkontext erneut geprüft; ein entzogener Grant verwirft den gesamten Versuch.

`validate_write_cross_record_state` nimmt einen typisierten vollständigen Kandidatenstand an. Er prüft Provenance-Endpunkte gegen das vollständige Post-Transaction-Inventar, EventRelation-Zyklen gegen alle Event-IDs und Provenance-Zyklen gegen den gemeinsamen Post-Transaction-Graph. Retractions werden durch die bestehenden Graphvalidatoren vor allen Additions angewendet. Source/Evidence wird aus vollständiger vorhandener plus gestufter History bei der Commit-Revision projiziert; ungültige Ziele und doppelte aktive Source/Target/Relation-Tupel schlagen fehl. Das Ergebnis wird nur vollständig oder gar nicht zurückgegeben.

Die Layer-Referenzen werden gegen denselben gepinnten `LayerSchemaSnapshot` validiert, dessen Konstruktion genau einen aktiven, eindeutig niedrigsten Base-Layer voraussetzt. Abweichende Revisionsstände zwischen Layer-, Entity-, Perspective- und Schema-Snapshot werden in M4-04b zurückgewiesen.

## Nachweise

- `crates/worlddb-core/src/write_authorization.rs`
- `crates/worlddb-core/src/write_cross_record_validation.rs`
- `crates/worlddb-core/src/source_evidence_projection.rs`
- Elf Tests im Modul `write_reference_validation`: Berechtigungsverweigerung, fehlendes FieldWrite, Deprecated-Warning-Recheck nach Grant-Entzug, vollständige Batchgrenzen und Referenzfehler. Die M4-Gate-Ergänzungen prüfen außerdem eine Event-Rolle außerhalb des gewählten Post-Transaction-Schemas sowie, dass ein gemischter Schema-/Assertion-/Event-Batch mit unbekannter Predicate-Referenz vor Publication vollständig verworfen wird.
- Evidence-Duplikate werden sowohl in der normalen Projektion als auch im vollständigen Post-Transaction-Prüfpfad abgewiesen.
- `cargo test --locked --workspace`: 323 Core-, 7 Testkit-, 2 Backend-Contract- und 79 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `WorldDB_1.0_Plancheck.py`: vollständiger Lauf nach der Registeraktualisierung.

## Abgrenzung

Die Funktion ist die vollständige Validierungsgrenze, die der Schreibpfad vor OCC und Veröffentlichung aufrufen muss. Die Einbindung in die atomare gemischte Transaktion und den tatsächlichen Publish-Pfad folgt M4-05. Schemaänderungen einschließlich atomarem Base-Layer-Wechsel und dem Nachweis, dass dabei keine Records verschoben werden, bleiben M4-05a. Der Deprecated-Warning-Receipt sowie vollständige Non-Interference-/End-to-End-Belege für WDB-SCH-010 bleiben in M4-09 und M8-11a/M8-26d offen. Die vorhandenen Timeline-/TimeUnit-Lifecycle-Maps und vollständigen Event-/Provenance-/Evidence-Inventare sind verpflichtende Eingaben des Aufrufers; der Validator behauptet keine noch nicht implementierte Storage-Rekonstruktion.
