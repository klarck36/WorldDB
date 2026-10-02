# M6-06 – Evidence-/Provenance-Nachbarschaft

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-core/src/source_provenance_indexes.rs` ergänzt zwei typisierte Nachbarschaftsmodelle:

- `EvidenceNeighborhoodIndex` bindet die berechtigte Source-/Evidence-Projektion an Source-, typed-target- und Relation-Postings. Abfragen nach Source, Ziel, direktem Source/Ziel-Paar und EvidenceRelation liefern EvidenceId-sortierte Einträge. Sichtbare, bereits revozierte Evidence bleibt mit ihrem As-of-Status auffindbar.
- `ProvenanceAdjacencyIndex` indiziert aktive, berechtigte Provenance-Kanten nach gespeichertem From-/To-Endpunkt und Relation. Es unterstützt Outgoing, Incoming, Both, direkte From/To-Paare und direkte Nachbarknoten. Die Relationsemantik wird nicht in zusätzliche oder transitive Kanten umgedeutet.
- Beide Indizes werden erst aus dem autorisierten Referenzpfad aufgebaut. Eine Provenance-Kante mit fehlendem, nicht zugeordnetem oder verborgenem Endpunkt wird vollständig vor dem Aufbau der Nachbarschaft entfernt. Für einen anderen QueryContext oder PolicySnapshot wird die sichtbare Projektion neu aufgebaut.

Die Module stellen Kandidatenindizes für spätere Enginepfade bereit; die produktive Queryauswahl und ihr Full-Scan-/Budgetfallback bleiben M6-08.

## Nachweise

- Zwei gezielte Differentialtests bestanden.
- Source-/Target-/Relation-Treffer stimmen mit `full_scan_authorized_source_evidence` überein. Der Test enthält eine aktive und eine zum Queryzeitpunkt revozierte Evidence sowie ein verborgenes Assertion-Ziel; die verborgene Zielkante ist weder per Target-Abfrage noch im Source-Ergebnis sichtbar.
- Provenance-Endpunkt- und Nachbarabfragen stimmen mit `project_authorized_provenance_edges` überein. Ein verborgenes Endziel entfernt die gesamte Kante. Der sichtbare Graph bleibt identisch, wenn die verborgene Kante aus dem Eingabestand entfernt wird.
- `cargo test --locked --workspace --quiet`: PASS; 421 Core- und 82 Rustdoc-Tests bestanden. Drei separat markierte Langkampagnen blieben ignoriert und zählen nicht als bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -X utf8 WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, DAG und Referenzen gültig.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py`: PASS; alle sechs Contract-Spiegel bytegleich.
- `git diff --check`: PASS.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf. Der Windows-Crashvertrag und die übrigen lokalen Verify-Schritte bestanden.

## Abgrenzung

Diese Ergebnisse belegen autorisierte Nachbarschaftstreffer gegen die Referenzscans auf Windows. Sie aktivieren keine produktiven Querypfade. Linux- und macOS-Prüfungen sowie das M5-23-Plattformgate bleiben wie vom Product Owner zurückgestellt offen.
