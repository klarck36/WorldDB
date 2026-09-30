# M1-06 – LayerDefinition und Auswahl

Status: **DONE**\
Prüfdatum: 2026-09-29

## Ergebnis

`crates/worlddb-core/src/layers.rs` ergänzt die Projekt-Schema-API um `LayerDefinition`, `LayerSchemaSnapshot` und die geschlossene `LayerSelection`-Auswahl. Definitionen tragen eine stabile `LayerId` und `Symbol`, optionale Beschreibung, `precedence_rank: i32`, Lifecycle und `SchemaRevision`. Symbole verwenden dieselbe validierte Grammatik `[a-z][a-z0-9_]*` wie die übrigen Schema-Symbole.

`LayerSchemaSnapshot` speichert `base_layer_id` als explizite Schemadaten. Sein validierender Konstruktor verlangt genau einen vorhandenen, aktiven und eindeutig niedrigsten aktiven Base-Layer. Rangwerte sind `i32`; der kleinere Wert liegt niedriger. Definition-IDs und Symbole sind eindeutig, Definitionen dürfen nicht jünger als ihr Snapshot sein.

`LayerSchemaSnapshot::revise` nimmt nur einen späteren Schemastand an und prüft die vollständige Post-Änderung gemeinsam. Bereits vorhandene Layer-Definitionen und Symbole bleiben erhalten, neue Layer beginnen in der Revision, die sie einführt, und Lifecycle bewegt sich nur `Active → Deprecated → Retired`. Ein Base-Wechsel kann samt Rangänderung und Deprecation des bisherigen Base-Layers in einem gültigen Post-Stand erfolgen. Retirement folgt als eigener späterer Stand. Die vorherige unveränderliche Ansicht behält ihre frühere Base-Bezeichnung und Layer-Ränge.

`LayerSelection` ist geschlossen: `BaseOnly`, `AllActive` oder `Explicit(NonEmptySet<LayerId>)`. `resolve` verwendet ausschließlich die übergebene Layer-Schemaansicht: `BaseOnly` löst deren historische Base-ID auf, `AllActive` liefert deren aktive IDs, und `Explicit` weist IDs zurück, die dort unbekannt sind. Explizite Auswahl kann retired Layer enthalten, damit erhaltene historische Records lesbar bleiben; Schreibfähigkeit retired/deprecated Layer wird an späteren Schreibpfaden geprüft.

## Grenzen und Folgebelege

`LayerSchemaSnapshot` ist der typisierte Layer-Anteil eines Schema-Snapshots; es implementiert noch keine Schema-Historienablage, Datenbankpublikationsprüfung, Transaktion, QueryContext oder Storage-Codec. Das atomare Post-State-Modell bewahrt ältere Werte und verändert keine Records, aber der Storage-/Transaktionspfad muss später beweisen, dass Base-Wechsel atomar publiziert und Records dabei nicht verschoben werden (`M4-04c`, `M4-05a`).

Lifecycle-Sequenzen und Base-Schutz sind hier validiert. Das Zurückweisen neuer Records auf deprecated/retired Layer sowie die dauerhafte Aufbewahrung historischer Records werden durch spätere Schreib-/Storagepfade ergänzt (`M4-05a`). `LayerId` als Pflichtfeld jedes konkreten Records sowie explizite IDs in Decoder, Import und Storage folgen den Record-/Codec-Arbeiten (`M1-17d`, `M5-06`, `M7-13`). HistorySpace-vor-Layer-Precedence, epistemische Partitionierung und Security-Filter bleiben bei ihren eigenen Resolver- und Security-Tasks.

Die `SchemaRevision` wird als Position auf der gemeinsamen Revisionsachse behandelt. Ob eine angegebene Revision tatsächlich publiziert wurde, prüfen späterer History-/Schema-Reader und Writer.

## Belege

- `crates/worlddb-core/src/layers.rs`: Definitionen, Lifecycle- und Revisionsvalidierung, vollständige Base-Validierung und Auswahlauflösung.
- `crates/worlddb-core/src/lib.rs`: öffentliche Reexports und Compile-Fail-Beispiel für leere Explicit-Auswahl.
- `cargo test --locked --offline --workspace --all-targets`: 42 Core-, 7 Testkit- und 4 xtask-Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 23 Dokumentationstests bestanden (22 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: Struktur und DAG gültig.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.
