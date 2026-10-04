# M8-14a – Timeline-/TimeUnit-Schemaregister

**Ergebnis:** DONE auf Windows  
**Normative Grundlage:** ADR-032  
**Plattform-Folgeprüfung:** Linux/macOS bleibt wie vereinbart bis M9-07 zurückgestellt.

## Gelieferter Umfang

- `TimelineDefinition` und `TimeUnitDefinition` sind revisionierte Schema-Records mit kanonischem Wireformat. Timeline-Kalenderprofil/Epoch und TimeUnit-Skala bleiben für ihre Identität unveränderlich; Lifecycle-Änderungen folgen nur Active → Deprecated → Retired.
- `SchemaSnapshot` löst Definitionen aus dem gebundenen Schemahistorienstand auf und normalisiert `Time` mit geprüfter Multiplikation in Nanosekunden zu `WorldTime`. Unbekannte, nicht aktive oder überlaufende Zeitreferenzen scheitern geschlossen.
- `FileSchemaManager` persistiert beide Recordfamilien, lädt sie nach Reopen wieder und prüft Duplikate, Symbolgrammatik, Mapping-/Skalenänderungen und Lifecycle-Übergänge.
- Die Schreibvalidierung verwendet denselben versionierten Schema-Snapshot statt externer Timeline-/Unit-Maps. Schema-Records sind in Wire-, Decoder-, Write-Path-, Export-, Import- und Verify-Inventaren registriert.
- Das Logical-Export-Envelope wurde auf v2 gehoben, weil sein geschlossenes Klassenmanifest nun 38 statt 36 Recordvarianten enthält. Die synthetische M7-16h-Fixture ist als Vor-Alpha-Version 2 neu gebunden; der eingefrorene physische Storage-Snapshot blieb bytegleich. Neues Manifest-BLAKE3: `e055937e2cc6a7613fc9d8c40a5a2e6ea9e18910c727d4eb6b3fcaa99cadea33`.

## Gezielte Nachweise

| Bereich | Ergebnis |
| --- | --- |
| `schema_history::tests` | 8 PASS; exakte Normalisierung, Überlauf, unveränderliche Mappings und monotone Lifecycle-Regeln |
| `write_reference_validation::tests` | 11 PASS; aktive registrierte Timeline-/Unit-Verwendung und Ablehnung unbekannter/retired Referenzen |
| Persistenz | PASS; Timeline-/TimeUnit-Veröffentlichung, Reopen, Normalisierung und unveränderliche Mappings |
| Wireformat | PASS; alle 38 Recordvarianten stimmen mit festen Golden-Frames überein und round-trippen; Nullskala und unbekanntes Kalenderprofil werden abgewiesen |
| Zeit-Invarianten | PASS; RecordedAsOf bleibt revisionsgebunden, Timeline-Ordnung bleibt explizit, Zeitintervalle bleiben halb-offen |
| M7-16h-Fixture | PASS; Logical-Export v2 und Migration-Golden reproduzieren; ursprüngliche Storage- und Backup-Dateipfade bleiben erhalten |

## Gesamtprüfung

- `cargo fmt --all -- --check` – PASS.
- `cargo xtask verify` auf Windows – **39 PASS, 1 erwarteter `M0-14 ci-matrix`-SKIP, 0 FAIL**. Der Lauf umfasst Workspace-/CLI-Prüfungen, Decoderinventar, TypeScript-Transport-Smoke, striktes Clippy, Policy-/Quellen-/Planchecks, M0-13-Evidence, SQLite-Referenzvertrag und M5-22-Windows-/Storage-Contracts.
- `WorldDB_1.0_Plancheck.py` – PASS: 257 Tasks, 253 Invarianten, 230 Folgebelegpaare; DAG und Referenzen gültig.
- `WorldDB_1.0_Sourcecheck.py` – PASS; Quellenkopien und Audit-ZIP bytegenau bestätigt.
- `git diff --check HEAD` – PASS.

M8-14b ist nach erfüllter Abhängigkeit READY. Nicht-Windows-Prüfungen bleiben für M9-07 vorgemerkt.
