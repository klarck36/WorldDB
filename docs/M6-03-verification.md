# M6-03 – Assertion-Point-/History-Index

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-core/src/assertion_point_index.rs` ergänzt `AssertionPointHistoryIndex` mit drei getypten Lookup-Pfaden:

- `point` adressiert einen exakten Entity-/Predicate-Punkt.
- `entity_history` liefert Assertions für einen Subject innerhalb eines konkreten Kontexts.
- `predicate_history` liefert Assertions für ein Predicate innerhalb eines konkreten Kontexts.

Jedes Posting enthält seine ursprüngliche HistorySpace, Layer, Perspective, EpistemicMode, Subject, Predicate und aufgezeichnete Revision. Die Generation merkt sich ihre inklusive `indexed_through`-Revision. Beim Lesen läuft der Index von der gewählten HistorySpace durch deren Ahnen; auf jeden Parent werden die unveränderlichen Cutoffs der Child-Definition angewandt. Indiziert werden nur lokale Branch-Deltas. Parent-Datensätze werden nicht in Child-Partitionen kopiert.

Abfragen vor dem Branch-Basisstand, nach der abgedeckten Indexrevision oder für eine unbekannte HistorySpace schlagen explizit fehl. Beim Aufbau werden doppelte Assertion-IDs, abweichende gespeicherte Revisionen und ein Kontext mit falscher Owner-HistorySpace abgewiesen. Ungültige interne Posting-Positionen werden als Nichttreffer behandelt.

## Nachweise

- Drei gezielte `assertion_point_index::tests` bestanden. Point-, Entity-History- und Predicate-History-Ergebnisse wurden gegen unabhängige Filter des indexfreien HistorySpace-Scans verglichen.
- Der Branch-Korpus deckt Root, Child, Sibling und Grandchild, lokale Deltas, einen veralteten Parent-Cutoff, einen Parent-Commit nach Branch-Erstellung sowie abweichende Layer-/Perspective-/Mode-Koordinaten ab.
- Negative Tests weisen doppelte Assertion-IDs und einen unpassenden Kontext-Owner ab; unbekannte HistorySpaces, Reads vor dem Branch-Basisstand und Queries nach dem Generationsstand schlagen geschlossen fehl.
- `cargo test --locked --workspace`: PASS; 411 Core-Unit-Tests und 82 Rustdoc-Tests bestanden. Die separat markierten Langkampagnen wurden nicht als bestanden gezählt.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -X utf8 WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, 253 Invarianten und 174 Folgebelegpaare strukturell gültig.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py`: PASS; alle sechs Contract-Spiegel bytegleich.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.

## Abgrenzung

Diese Task baut den Assertion-Index und belegt dessen Point-/History-Ergebnisse auf dem Referenzmodell. Sie schaltet noch keine produktiven Querypfade um; sichere Indexwahl und Full-Scan-/Budget-Fallback folgen in M6-08. Die vollständige Query-Differential-Suite für `WDB-IDX-001` bleibt M6-13. Linux- und macOS-Prüfungen sowie das M5-23-Plattformgate bleiben wie vom Product Owner zurückgestellt offen.
