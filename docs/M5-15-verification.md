# M5-15 – Compaction und Reclamation

**Stand:** 1. Oktober 2026
**Plattform:** Windows 11 / NTFS
**Status:** lokaler Storage-Vertrag abgeschlossen; Linux-/macOS-Prüfungen bleiben wie vereinbart zurückgestellt.

## Ergebnis

`CompactionManager` liest das aktuelle Manifest und schreibt dessen History-Inventar in neue immutable Segmente. Records, kanonische Frames und optionale Wire-Flags bleiben erhalten. Die Kompaktion bündelt Eingabesegmente nur innerhalb der registrierten Record- und Bytebudgets und setzt für jeden Ausgabechunk den höchsten `through_revision` seiner Eingabesegmente. SecurityPolicy-Referenzen bleiben unverändert im Manifest. Ohne Verringerung der Segmentanzahl wird keine Wartungstransaktion publiziert.

Die Wartung wird als typisierter WAL-Snapshot committed und durch `RecoveryManager` materialisiert. Erst nach sauberem Recovery und Prüfung des neuen `CURRENT`-Inventars dürfen ersetzte History-Dateien reclaimed werden. Reclamation nimmt nur explizit angegebene, nicht mehr referenzierte History-IDs an, validiert Dateiart, Datenbankpfad und Inhaltsdigest und synchronisiert das Segmentverzeichnis. Aktuelle Referenzen, SecurityPolicy-Segmente und unbekannte Dateien bleiben unberührt.

Der prozessweite Pin-Registry-Schlüssel ist der kanonische Datenbankpfad; weitere `CompactionManager`-Instanzen desselben Prozesses teilen ihn. Pinaufnahme und Reclamation serialisieren die Prüfung über dieselbe Mutex. Ein Pin kann nur für eine exakte Referenz des aktuellen Manifests angelegt werden und wird beim Drop gelöst. Vertrags-Pins für Snapshot, Backup, Recovery-Checkpoint und Export schützen jeweils ein ersetztes Quellsegment, bis der Pin freigegeben und Reclamation erneut aufgerufen wird. Die realen Backup-/Export-Lifecyclebindungen folgen in M7-08/M7-11.

Recovery dekodiert weiterhin jeden committed typisierten WAL-Snapshot. Da jeder Snapshot ein vollständiges Inventar bindet, materialisiert und validiert Replay nur das neueste vollständige Inventar. Damit müssen nach einer späteren, verifizierten Kompaktion keine bereits ersetzten Segmente aus älteren WAL-Snapshots fortbestehen. Ein Neustarttest bestätigt nach der Reclamation einen sauberen Recovery-Scan und identische History-Frames.

## Verifikation auf Windows

- `cargo fmt --all -- --check` – bestanden.
- `cargo test --locked -p worlddb-storage-file --all-targets --quiet` – 69 bestanden, 0 fehlgeschlagen, 0 ignoriert.
- `cargo test --locked --workspace --quiet` – 569 bestanden, 0 fehlgeschlagen, 2 absichtlich ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – bestanden.
- `cargo xtask verify` – 32 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL; M0-13-Lauf M0-13-20261001T172837Z-29faca4e85 PASS.

Die vier neuen Vertragstests belegen:

1. Alle vier Pinarten halten ersetzte Segmente; nach Drop werden die Segmente beim erneuten Versuch reclaimed. Pinaufnahme und Kompaktion verwenden in diesem Test getrennte Manager-Instanzen.
2. Ohne Pins bleiben alle kanonischen History-Frames inklusive optionaler Flags identisch; die alten Dateien werden nach Recovery entfernt und die neue Datenbank besteht den Neustart-Scan.
3. Ein noch aktuelles Segment wird nicht reclaimed; nach Kompaktion lässt es sich nicht mehr als aktuell pinnen.
4. Ein SecurityPolicy-Segment wird von der History-Reclamation abgewiesen.

Ein interner Budgettest prüft die Grenzen für Recordanzahl und Segmentbytes. Linux/macOS-Läufe, echte Backup-/Exportpins sowie die Prozess-/Crashmatrix aus M5-22 wurden nicht ausgeführt und bleiben offen.
