# M6-08 – Produktive Raw/Resolved/Explain-Engine

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-core/src/query_engine.rs` ergänzt die öffentliche `ProductiveQueryEngine` und verbindet die M3-Ports mit den gespeicherten Query-Snapshots:

- Raw History bindet ein vollständiges, autorisiertes Ergebnis. Da keine Indexfamilie die vollständige HistorySpace-Aufzählung liefert, verwendet dieser Pfad einen vollständigen Scan und verlangt dafür ein verfügbares Scanbudget.
- Resolved und Explain führen Punktabfragen über das versions- und revisionsgeprüfte Assertion-Point-Index aus. Fehlende, inkompatible oder beschädigte Generationen wählen einen vollständigen Scan, sofern das Budget reicht. Bei erschöpftem Budget endet die Abfrage vor Ausgabe eines Teilergebnisses.
- Der Indexpfad und der Scan-Fallback wenden dieselbe Slot-, Branch-, Archive-, Lifecycle-, Gültigkeits- und Sicherheitsprojektion an. Ergebnisse behalten die kanonische Assertion-ID-Reihenfolge.
- Kandidaten, Masken und Replacement Boundaries werden vor ihrer Verwendung anhand von Record-, Capability- und FieldRead-Regeln gefiltert. Raw History verlangt zusätzlich zum RawHistory-Recht die zum RecordRef passende Leseberechtigung.
- Explain bindet die M2-Oracle-Stufen. Die Trace führt nur sichtbare Masken auf, die tatsächlich Kandidaten entfernt haben, und nur autorisierte Replacement Boundaries, die den wirksamen Cutoff gesetzt haben.
- Die öffentliche API exportiert Store-/Request-Bindungen, Indexzugriff, Ausführungspfad und typisierte Fehler.

`mask_projection.rs` und `multi_value_resolution.rs` liefern die für Explain erforderlichen angewandten Record-IDs, ohne die bestehenden Resolver-Ergebnisse zu ändern. `candidate_scan.rs` wendet denselben Punktfilter auf Index- und Scanpfade vor der semantischen Projektion an.

## Nachweise

- Vier gezielte Engine-Tests bestanden: Index/Fallback/M2-Oracle-Gleichheit, FieldRead-Filterung, Raw-History-Recordrechte, Mask-/Boundary-Explain und Budgetfehler ohne Teilergebnis.
- `cargo test --locked --workspace --quiet`: PASS; 425 Core-Tests, 43 Storage-Tests, 5 Wire-Oracle-Tests, 82 Rustdoc-Tests und die übrigen Workspace-Suites bestanden. Drei separat markierte Langkampagnen blieben ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.
- Windows-Plattformprofil; Linux- und macOS-Prüfungen bleiben wie vom Product Owner zurückgestellt offen.

## Abgrenzung

Der derzeitige M2-Referenzspeicher kann nur geordnet nach Revision gelesen werden. Deshalb filtert der Indexpfad zunächst Assertion-IDs über den Point-Index und prüft sie danach im gespeicherten History-Modell auf dieselben Lebenszyklus-, Archiv- und Branchregeln wie der Orakelscan. Diese Task belegt die semantische Engineintegration und sichere Fallbackauswahl; sie behauptet keinen direkten Point-Read aus einem persistenten Record-Backend.

Resolved und Explain erfordern derzeit einen konkreten `WorldTime`-Zeitpunkt. Raw History unterstützt den `AllTimes`-Selector. Die Zeitreihenauflösung über alle Weltzeiten ist nicht Teil dieses Tasks.
