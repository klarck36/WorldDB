# M6-02 – Record-/Operation-/Schema-Indizes

**Ergebnis:** PASS für die vier getypten Lookup-Familien auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-core/src/record_indexes.rs` ergänzt vier getrennte Indexmodelle:

- `RecordIdIndex` adressiert Domain-History-Einträge über die geschlossene `RecordRef`-Familie.
- `OperationIdIndex` löst eine `OperationId` zum dauerhaften Status (`Indeterminate` oder `Committed` mit Revision) und Storage-Locator auf.
- `SchemaIdRevisionIndex` löst `SchemaRecordRef` exakt nach ID und `SchemaRevision` auf und liefert die aufsteigende Revisionshistorie. Der Builder liest konkrete Schema-Definitionen; Rollen und Attribute verwenden die Revision ihrer `EventKindDefinition`. Ein `LayerSchemaSnapshot` erzeugt keine zusätzliche Schemaidentität.
- `LifecycleIndex` ordnet unveränderliche Closure-/Retraction-Records dem jeweils getypten Zielrecord zu und liefert dessen Lifecycle-Historie bis einschließlich einer angefragten Revision. Der Record-Stream-Builder übernimmt den ursprünglichen Record-Locator.

Jede Indexfamilie lehnt doppelte Schlüssel ab. Der Lifecycle-Builder weist außerdem familienfremde Zielverknüpfungen ab. Indexpositionen werden bei der Auflösung geprüft; eine intern ungültige Locatorposition wird als Nichttreffer behandelt und verursacht keinen Panikzugriff.

## Nachweise

- 7 gezielte `record_indexes::tests` bestanden. Treffer und Nichttreffer für alle vier Lookup-Arten stimmen mit unabhängigen Full-Scan-Filtern über dieselben Quellzeilen überein.
- Die Tests prüfen exakte Schema-ID-/Revisionssuche, aufsteigende Schemahistorie, Lifecycle-`as_of`-Grenzen, Schema- und Record-Stream-Extraktion, doppelte Schlüssel sowie falsche Lifecycle-Zielfamilien.
- `cargo test --locked --workspace`: PASS; die vorgesehenen Langkampagnen bleiben ignoriert und zählen nicht als bestanden.
- `cargo clippy --locked -p worlddb-core -p worlddb-storage-file --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -X utf8 WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, 253 Invarianten und 174 Folgebelegpaare strukturell gültig.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py`: PASS; alle sechs Contract-Spiegel bytegleich.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf. Plan- und Sourcecheck bestanden nach dem Abschluss von M6-02 erneut.

## Abgrenzung

Diese Task baut getypte Lookup-Strukturen und vergleicht deren Treffer mit dem indexfreien Scan-Orakel. Sie schaltet noch keine produktiven Querypfade um; die sichere Full-Scan-/Budgetausführung gehört zu M6-08. Die vollständige Query-Differential-Suite für `WDB-IDX-001` bleibt M6-13. Linux- und macOS-Prüfungen sowie das M5-23-Plattformgate bleiben wie vom Product Owner zurückgestellt offen.
