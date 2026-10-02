# M6-11 – COUNT, EXISTS und Gruppierung

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`ProductiveQueryEngine::aggregate` verbindet den M3-Aggregatport mit der produktiven Query-Engine. Eingabe ist ein `OwnedQueryResult` mit bereits aufgelösten Ergebniszeilen und vollständiger Query-/Security-Bindung. Der M3-Port vergleicht diese Bindung mit dem aktuellen Kontext, prüft die Security-Epochen und verlangt `QueryAggregate`.

Vor dem Beitrag einer Zeile werden RecordRead und – für `GroupedCount` – FieldRead erneut geprüft. Unsichtbare Records und Zeilen mit einem nicht lesbaren Gruppierungsfeld tragen weder zum Zähler noch zu Gruppen bei. Fehlende Gruppenwerte bilden einen eigenen `Missing`-Schlüssel; vorhandene Schlüssel enthalten ValueKind und schema-normalisierte kanonische Bytes. Gruppen werden deterministisch sortiert.

Kandidaten-, Arbeits- und Gruppenergebnislimits sowie Zählerüberlauf liefern terminale Fehler. Kein Zwischenstand wird als fertiges Aggregat zurückgegeben. Cancellation wird auch vor und nach der Aggregation geprüft; dadurch wird ein leerer, bereits abgebrochener Lauf nicht fälschlich als `Count(0)` oder `Exists(false)` ausgegeben. Ein nicht abgebrochener Lauf über eine gültige leere Eingabe liefert dagegen das reguläre leere COUNT/EXISTS-Ergebnis.

Der Ausführungspfad ist `FullScan`: der Aggregator prüft alle bereitgestellten, bereits aufgelösten Eingabezeilen. Die Erzeugung vollständiger aufgelöster Ergebniszeilen bleibt Aufgabe des vorgelagerten Query-Pfads.

## Nachweise

- Vier gezielte Aggregationstests bestanden: COUNT/EXISTS/GroupedCount über den produktiven Engine-Port; versteckte Zeilen verändern COUNT und Gruppen nicht; Missing- und typisierte Schlüssel; Kandidaten-/Gruppenbudget ohne Teilergebnis; leeres EXISTS; Cancellation bei leerer Eingabe.
- `cargo test --locked --workspace --quiet`: PASS; 431 Core-Tests sowie alle übrigen Workspace- und Rustdoc-Suites bestanden. Drei separat markierte Langkampagnen blieben ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 34 PASS, 1 erwarteter SKIP, 0 FAIL. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.
- Plancheck und Sourcecheck bestanden; Linux- und macOS-Prüfungen bleiben wie vom Product Owner zurückgestellt offen.

## Abgrenzung

M6-11 aggregiert bereits aufgelöste Zeilen. Es baut noch keinen allgemeinen Multi-Record-Resolved-Querypfad; dafür bleiben die Query-Planner-/Streaming-Integrationen der Folgeaufgaben zuständig. Eingabebindung und Rechteprüfung verhindern, dass Zeilen aus einem anderen Querykontext oder verborgene Records in die Aggregation einfließen.
