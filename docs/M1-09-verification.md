# M1-09 – Assertion und Lifecycle-Records

Status: **DONE**\
Prüfdatum: 2026-09-29

## Ergebnis

`crates/worlddb-core/src/assertions.rs` stellt unveränderliche Werte für `Assertion`, `AssertionValidityClosure` und `AssertionRetraction` bereit. Alle Felder sind privat und besitzen nur lesende Accessors. Gültigkeitsende und Retraction sind eigene Records mit konkreten IDs; beide referenzieren das unveränderte Ziel.

`AssertionDraft` bündelt die vollständige Nutzlast; `Assertion::new` ergänzt nur Assertion-ID und `created_revision`. Die Assertion bindet den bereits validierten `ContextKey`, den Entity-Subject, das Predicate, den getypten Value, die Polarity und `AssertionValidity`. `Subject` ist ein eigener Wrapper um `EntityId`; die zulässige EntityType-Constraint sowie Referenzexistenz und Security bleiben Sache der späteren Operationsvalidierung. `Polarity::{Positive, Negative}` ist von Retraction getrennt.

`proposition_components()` projiziert exakt `(Subject, PredicateId, Value, Polarity)`. Kontext, Assertion-ID, Validity und `created_revision` sind keine Proposition-Equality-Komponenten. Die Value-Gleichheit selbst bleibt schema-aware: Zeitwerte müssen über das registrierte Timeline-/Unit-Schema verglichen werden; `Value` erhält deshalb keine erfundene strukturelle `Eq`-Semantik.

`AssertionValidityClosure::new` verlangt das konkrete Ziel, eine spätere Transaction-Time-Revision und dieselbe Timeline wie die Assertion-Validity. `AssertionRetraction::new` verlangt ebenfalls das konkrete Ziel und eine spätere Transaction-Time-Revision. Die Retraction besitzt keinen WorldTime-Wert; sie wirkt ab ihrer `created_revision`. Beide Konstruktoren erzeugen zusätzliche Records, sie ändern das Ziel nicht.

## Grenzen und Folgebelege

Die Prüfung, ob ein Closure-Ende im zulässigen Bereich der vorhandenen Validity liegt, die Auflösung mehrerer Lifecycle-Records in einem historischen Snapshot und atomare Commit-/Recovery-Semantik gehören zu M2-05/M4-05. `CorrectAssertion` als atomare Transaktion mit Ersatz-Assertion, Retraction und `Corrects`-Kante folgt M2-15c. Es gibt hier noch keinen Storage- oder Wire-Codec.

Die invariantenspezifischen positiven und negativen Nachweise für `WDB-AST-001` sind in der Evidenzmatrix registriert. `WDB-AST-002` hat jetzt Primärbelege dafür, dass Closure und Retraction zusätzliche Records sind und ungültige Revisions-/Timelinefälle scheitern; `CorrectAssertion` sowie historische und atomare Commit-/Recovery-Projektion bleiben als M2-/M4-/M8-Folgebelege offen. `WDB-PRO-001` hat Primärbelege für die exakte Projektion auf Subject, Predicate, Value und Polarity sowie dafür, dass Kontext, Record-ID, Validity und `created_revision` ausgeschlossen sind. Der schema-aware Vergleich der Values in Proposition-Masking und die zugehörigen Resolution-Properties folgen M2-07/M2-17.

## Belege

- `crates/worlddb-core/src/assertions.rs`: immutable Recordformen, typed IDs, Timeline- und Revisionsprüfung.
- `crates/worlddb-core/src/lib.rs`: öffentliche Exporte und Compile-Fail-Nachweise gegen Mutation sowie gegen das Vertauschen konkreter Lifecycle-IDs.
- `assertions::tests::closure_and_retraction_are_additional_records_on_distinct_axes`: positiver Nachweis, dass Closure und Retraction zusätzliche Records sind und die ursprüngliche Assertion unverändert bleibt.
- `assertions::tests::lifecycle_records_reject_non_later_revisions_and_cross_timeline_closure`: negative Fälle für unzulässige Transaction-Time-Revisionen und Timeline-Mischung.
- `assertions::tests::proposition_projection_contains_exactly_the_equality_components`: Projektion auf genau Subject, Predicate, Value und Polarity.
- `assertions::tests::proposition_projection_excludes_context_identity_validity_and_revision`: zwei Assertions mit gleichen Proposition-Komponenten und unterschiedlichem Kontext, Record-ID, Validity sowie `created_revision` besitzen dieselbe Proposition-Projektion.
- `cargo test --locked --offline --workspace --all-targets`: 51 Core-, 7 Testkit- und 4 xtask-Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 29 Dokumentationstests bestanden (28 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo check --locked --offline --workspace --all-targets`: bestanden; alle Workspace-Targets einschließlich Testtargets kompiliert.
- `cargo fmt --all -- --check`: bestanden.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: Struktur und DAG gültig.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.

## Nachprüfung am 30. September 2026

Der M1-Gate-Precheck hatte fehlende Primärbelege für `WDB-AST-002` und `WDB-PRO-001` sichtbar gemacht. Der zusätzliche Test `assertions::tests::proposition_projection_excludes_context_identity_validity_and_revision` belegt jetzt negativ, dass Kontext, Assertion-ID, Validity und `created_revision` die Proposition-Projektion nicht beeinflussen. Die beiden Tests zur exakten Vier-Komponentenprojektion bestehen.

- `cargo test --locked --offline --workspace --all-targets --message-format short`: 146 bestanden, 0 fehlgeschlagen; der einstündige Decoder-Fuzzer ist der einzige ignorierte Test.
- `cargo xtask verify`: 30 PASS, 1 erwarteter sichtbarer M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `WorldDB_1.0_Plancheck.py`: 236 Tasks, 11 Milestones, 253 Invarianten und 161 Folgepaare strukturell gültig.
- `--gate-precheck M1` lässt jetzt nur M1-18 und die davon abhängigen WDB-WIR-003/005 offen; M1-18 bleibt wegen ausstehender Source-Commitbindung absichtlich `WAITING_EXTERNAL`.
