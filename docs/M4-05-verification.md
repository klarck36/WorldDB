# M4-05 – Atomare Mischtransaktionen

**Status:** DONE, lokal gegen den In-Memory-Modellbackend verifiziert am 30. September 2026.

## Ergebnis

`commit_mixed_record_batch` nimmt eine offene Transaktion aus heterogenen `Record`-Varianten plus verpflichtendem Engine-Validierungscallback entgegen. Der Callback sieht die Basisrevision und den gesamten gestagten Batch. Erst wenn er erfolgreich zurückkehrt, wechselt die Transaktion in den validierten Zustand und publiziert den vollständigen Batch über genau einen Backend-Aufruf. Validierungsfehler konsumieren den offenen Zustand ohne Publication.

Der Integrationstest kombiniert in einem Batch drei Schema-Definitionen, Assertion, Event, Source, Evidence und Provenance. Er liest die Schema-Definitionen direkt aus den gestagten `Record`-Werten, baut daraus den Post-Transaction-Snapshot, prüft dessen Fingerprint, validiert Assertion und Event gegen diesen Snapshot und ruft danach den zusammengesetzten M4-04c-Validator für Security, Eventgraph, Provenancegraph und Source/Evidence auf. Der erfolgreiche Read enthält alle acht Records unter derselben Revision. Eine zweite Ausführung mit entzogener Schreibberechtigung lässt den Backend-Head auf Genesis und die Commitliste leer. Ein dritter Batch enthält eine Assertion mit einer im gestagten Post-Transaction-Schema unbekannten Predicate-ID; die Referenzvalidierung verwirft den gesamten Batch und lässt Head und Commitliste ebenfalls unverändert.

## Nachweise

- `crates/worlddb-core/src/transaction_flow.rs`: `commit_mixed_record_batch` und typisierter Validierungs-/Publish-Fehler.
- `crates/worlddb-core/src/write_cross_record_validation.rs`: kompletter zusammengesetzter M4-04c-Validator, aufgerufen im Batch-Callback vor Publication.
- `crates/worlddb-core/src/write_reference_validation.rs`: `mixed_schema_domain_event_evidence_and_provenance_publish_once`.
- `cargo test --locked --workspace`: 324 Core-, 7 Testkit-, 2 Backend-Contract- und 79 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `WorldDB_1.0_Plancheck.py`: 236 Tasks, 253 Invarianten und 169 Folgebelegpaare valide.

## Abgrenzung

Der Beleg verwendet den M4-01-In-Memory-Modellbackend und zeigt atomare Sichtbarkeit innerhalb eines Prozesses, keine Crash- oder Machine-Durability. Die generische Backend-Schnittstelle muss für dauerhafte Publication von einem konkreten Backend implementiert werden. Der Aufrufer muss den M4-04c-Grenzvalidator im Callback tatsächlich aufrufen; der Integrationstest belegt diesen vorgesehenen Pfad. Projektmetadaten einschließlich atomarem Base-Layer-Wechsel folgen M4-05a. OCC-Revalidierung gegen einen inzwischen veränderten Head folgt M4-06/M4-07.
