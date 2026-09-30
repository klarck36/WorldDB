# M1-07 – HistorySpace, Entity und Perspective

Status: **DONE**\
Prüfdatum: 2026-09-29

## Ergebnis

`crates/worlddb-core/src/catalog.rs` stellt projektweite Katalogtypen bereit. `HistorySpaceDefinition` enthält eine typisierte `HistorySpaceId`, optional genau eine Parent-ID und `base_revision`. Ein Root muss Genesis als Base verwenden; Selbstreferenzen scheitern beim Definitionsbau. `HistorySpaceCatalog` sortiert Einträge kanonisch und verwirft doppelte IDs, fehlende Parent-IDs sowie Zyklen. Das bleibt ein HistorySpace-Modell; `BranchId` wird nicht eingeführt.

`Entity` ist die unveränderliche Identitätszeile aus `EntityId`, genau einer `EntityTypeId` und `created_revision`. Sie enthält keine freien Namen oder Maps; Entity-Fakten und Labels bleiben Assertions. `EntityRetirement` ist ein eigenes Ereignis mit konkreter `EntityRetirementId` und direkt typisiertem `EntityId`-Ziel. `EntityCatalogSnapshot` prüft eindeutige Identitäten, bekannte Retirement-Ziele, Revisionen und höchstens eine Retirement je Entity. Folgestände dürfen Identitäten, Type-Zuweisungen und Retirement-History nicht entfernen oder ändern.

`PerspectiveDefinitionRevision` speichert optionale Anzeige-/Beschreibungsfelder unter stabiler `PerspectiveId` auf der gemeinsamen Revisionsachse. Vorhandene Felder dürfen nicht leer sein; fehlende Anzeige-Metadaten bleiben zulässig. `PerspectiveRetirement` hat eine eigene konkrete ID und ein direktes `PerspectiveId`-Ziel. `PerspectiveCatalogSnapshot` verlangt aufsteigende Metadatenrevisionen, erhält alle früheren Revisionen und Retirement-Ereignisse und verhindert Metadatenänderungen nach Retirement. `PerspectiveId` und `PrincipalId` bleiben verschiedene Typen; ein Compile-Fail-Beispiel prüft die Grenze.

## Grenzen und Folgebelege

Der HistorySpace-Katalog kann Parent-Existenz und Zyklen prüfen. Er kann noch nicht feststellen, ob `base_revision` publiziert ist oder innerhalb des Parent-Heads liegt; die historisch gepinnte Vererbung und Cutoff-Auswertung folgen M2-02. M1-07 prüft den typisierten Perspective-/Principal-Abstand, während die vollständige Security-Non-Interference-Prüfung für WDB-EPI-003/WDB-SEC-001 M3-03 bleibt.

EntityType-Existenz und -Lifecycle, Capability-/Referenzberechtigung, Generator-/Importkollisionen, Speicherung und Codec-Roundtrips hängen von Schema-, Writer-, Security- und Codec-Kontexten ab und sind hier nicht simuliert. Diese Grenzen folgen den Tasks M3-03, M4-04b, M4-05a, M1-17a/M1-17b und M5-06. Rust-Strings sind gültiges UTF-8; es existiert noch kein eigener gemeinsamer Bytehöchstwert für Perspective-Metadaten, daher validiert dieser Typ nur, dass vorhandene Felder nicht leer sind.

Die Katalog-Snapshots modellieren veröffentlichte Revisionsstände, prüfen aber nicht selbst, ob eine übergebene `Revision` tatsächlich publiziert wurde. Diese Prüfung gehört an den History-/Transaktionspfad.

## Belege

- `crates/worlddb-core/src/catalog.rs`: HistorySpace-Graph, Entity-/Perspective-Definitionen, Katalogansichten und Retirement-Validierung.
- `crates/worlddb-core/src/lib.rs`: öffentliche Reexports sowie Compile-Fail-Grenzen für `BranchId`, Entity/Perspective-IDs und `PerspectiveId`/`PrincipalId`.
- `cargo test --locked --offline --workspace --all-targets`: 47 Core-, 7 Testkit- und 4 xtask-Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 25 Dokumentationstests bestanden (24 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: Struktur und DAG gültig.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.
