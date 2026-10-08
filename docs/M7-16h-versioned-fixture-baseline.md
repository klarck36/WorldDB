# M7-16h – Versionierte Pre-Alpha-Fixture-Baseline

**Stand:** 3. Oktober 2026, Windows
**Ergebnis:** abgeschlossen; Linux/macOS-Nachweise bleiben für M9-07 vorgemerkt.

## Eingefrorener Korpus

Der synthetische Korpus liegt unter `crates/worlddb-storage-file/tests/fixtures/m7-16h/`. Sein Manifest enthält 24 Dateien mit zusammen 15.553 Bytes. Die Testkonstante bindet die vollständige Manifestdatei an diesen BLAKE3-Digest:

`e055937e2cc6a7613fc9d8c40a5a2e6ea9e18910c727d4eb6b3fcaa99cadea33`

M8-14a upgraded the frozen Logical Export golden to v2 and refreshed this
manifest digest. The physical storage fixture remained byte-identical.

Jeder Manifesteintrag bindet relativen Pfad, Bytezahl und BLAKE3-Dateidigest. Der Test lehnt geänderte Bytes, zusätzliche oder fehlende Dateien, unsortierte und unsichere Pfade sowie Symlinks ab. Ohne das ausdrückliche Capture-Flag schreibt der Test keine Fixtures.

| Artefakt | Geprüftes Ergebnis |
| --- | --- |
| `storage/` | Vollständige Datenbank mit zwei Revisionen öffnet unter der festgelegten `DatabaseId` und besteht Storage Verify. |
| `exact-backup/` | ExactDatabaseBackup der Revision 2 wird verifiziert und als exakter Klon mit neuer `DatabaseId` wiederhergestellt; die `RestorePublication` ist durch das passende Required Audit gebunden. |
| `exports/logical-export.wdbx` | Decodiert und stimmt Byte für Byte mit einem frischen, scoped Logical Export der Storage-Fixture überein. |
| `exports/sharing-export.wdbs` | Decodiert und stimmt Byte für Byte mit einem frischen autorisierten Sharing Export einer Kopie der Storage-Fixture überein. |
| `migration/plan.record`, `step-1.record`, `step-2.record` | Kanonischer Restrictive-Zwei-Schrittplan und Eingaben decodieren; Ausführung committet Revisionen 3 und 4, öffnet anschließend ein `Completed`-Run-Journal und bestätigt beide Required Audits. |
| `migration/expected-logical-export.wdbx` | Der logische Export nach der Migration reproduziert exakt das eingefrorene Golden-Artefakt. |

Das README im Fixture-Verzeichnis dokumentiert den beabsichtigten Capture-Ablauf. Eine Änderung des Korpus ist eine explizite Fixture-Versionierung und muss Manifest, Testdigest und Review der geänderten Binärdateien gemeinsam aktualisieren.

## N-1 und Release-Status

Am Prüftag hatte das öffentliche Repository keine veröffentlichten Releases und keine Git-Tags. Die Baseline ist daher **vor der ersten Alpha nicht als N-1 anwendbar**; daraus wird kein N-1-Pass abgeleitet. Die Release-Seite zeigte „There aren’t any releases here“ ([GitHub-Releases](https://github.com/klarck36/WorldDB/releases)); `git ls-remote --tags origin` lieferte keine Tags. Nach Veröffentlichung der ersten Alpha ist N-1 gemäß M9-02 neu zu prüfen.

## Windows-Nachweise

- `cargo test --locked -p worlddb-storage-file --test m7_16h_fixture_baseline -- --nocapture` – 1 PASS.
- `cargo fmt --all -- --check` – PASS im vollständigen Verify-Lauf.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – PASS.
- `cargo xtask verify` – 38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. Enthält Workspace-Check, alle Windows-Storage-Tests, Plancheck, Sourcecheck und Whitespace-Prüfung.

Linux/macOS werden entsprechend der Projektvorgabe erst in M9-07 geprüft.
