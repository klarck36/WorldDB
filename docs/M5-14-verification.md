# M5-14 – Salvage-Verifikation

**Stand:** 2026-10-01
**Plattform:** Windows 11 / NTFS
**Status:** abgeschlossen für den lokalen Storage-Vertrag; Linux/macOS-Nachweise bleiben zurückgestellt.

## Ergebnis

`SalvageManager` erzeugt außerhalb der Quelldatenbank ein ausdrücklich markiertes Fork-Archiv mit einer neu generierten `DatabaseId`. Salvage scannt Quelle und WAL read-only. Es repariert weder WAL noch `CURRENT`, Manifest oder Quellsegmente und überschreibt kein vorhandenes Ziel.

Die Kandidatenliste stammt aus dem neuesten gültigen typed Snapshot im verifizierten committed WAL-Präfix oder – wenn dessen Revision neuer ist – aus einem aktuellen Manifest, dessen Commit-Hash an den verifizierten WAL-Präfix gebunden ist. Bei gleicher Revision und widersprüchlichem Manifest hat der WAL-Snapshot Vorrang. Ist nur ein älterer Snapshot verifizierbar, wird er verwendet und die neuere beschädigte Snapshot-Payload als Unsicherheit dokumentiert.

Jedes referenzierte History- oder SecurityPolicy-Segment wird vollständig dekodiert, gegen Identität, Revision und Inhaltsdigest geprüft und bytegenau aus dem finalen Segmentpfad kopiert. Nur bei einem im WAL ausdrücklich als staged benannten Segment darf auf den Stagingpfad zurückgegriffen werden. Unlesbare oder beschädigte Kandidaten werden ausgelassen und im Bericht einzeln mit ID, Digest, `through_revision`, Einheit und Fehlergrund ausgewiesen. Historyeinheiten zählen Records; SecurityPolicyeinheiten zählen vollständige Policy-Snapshots.

Das Fork-Archiv enthält `SALVAGE_REPORT.tsv`, einen BLAKE3-gebundenen Abschlussmarker `SALVAGE` und während des Aufbaus `SALVAGE_BUILDING`. Die Reportzeilen nennen Recovery-Findings, Snapshotquelle, ausgelassene/unsichere WAL-Payloads sowie alle nicht referenzierten Einträge in Segment- und Staging-Verzeichnissen. Diese nicht autoritativen Einträge werden nicht übernommen. Der getrennte Audit-WAL-/Segment-Namespace ist im Bericht ausdrücklich ausgeschlossen; Audit-Salvage bleibt bei M5-18/M5-19.

Das Ziel muss neu sein, außerhalb der Quelle liegen und einen bereits vorhandenen Elternordner haben. Das Ergebnis ist ein markiertes Salvage-Archiv zur späteren Import-Anbindung; es ist noch kein unmittelbar mit `DatabaseLayout::open` zu öffnendes Datenbankverzeichnis. Die CLI-/Recovery-Oberfläche und Importanbindung gehören zu M8-03.

## Verifikation

- `cargo fmt --all -- --check` – bestanden.
- `cargo test -p worlddb-storage-file --all-targets --quiet` – 64 bestanden, 0 fehlgeschlagen, 0 ignoriert.
- `cargo test --workspace --quiet` – 564 bestanden, 0 fehlgeschlagen, 2 absichtlich ignoriert.
- `cargo clippy --workspace --all-targets -- -D warnings` – bestanden.
- `cargo xtask verify` – 32 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- M0-13-Evidenzlauf im finalen Verifier: `M0-13-20261001T164928Z-83bd90d659`, PASS.

Die fünf neuen Windows-Vertragstests belegen:

1. Vollständig gültige Referenzen werden mit identischen Segmentbytes kopiert; Berichtsdigest und Abschlussmarker stimmen.
2. Ein beschädigtes Segment wird einzeln ausgelassen, ein gesundes Segment bleibt kopierbar; Grund und Identität stehen im Verlustbericht.
3. Ein WAL-benannter staged Snapshot kann ohne Recovery kopiert werden; der Quellstand bleibt unverändert.
4. Bei `ManifestSnapshotMismatch` wird das committed WAL-Inventar ausgewählt; zusätzliche unreferenzierte Dateien werden als unsicher ausgewiesen.
5. Zielpfade innerhalb der Quelldatenbank und existierende Ziele werden abgewiesen.

Die Tests vergleichen den gesamten Quelldateibaum einschließlich `LOCK` vor und nach Salvage bytegenau. Linux- und macOS-Ausführung, echte Prozess-/Stromausfallmatrix sowie die spätere CLI-Importintegration wurden nicht ausgeführt.

## Generator-Ausnahme

M5-14 ist der erste Produktaufrufer der Core-UUIDv7-Policy. Die eng begrenzte interne Core-Funktion `generate_salvage_fork_database_id` verwendet den bestehenden falliblen UUIDv7-Generator; die generische Funktion wird nicht als normale Public API exponiert. Die Dead-Code-Ausnahme `WDB-EXC-0001` wurde entfernt.
