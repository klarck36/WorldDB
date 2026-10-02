# M6-14b – Windows-Messungen (Zwischenstand)

**Status:** Teilmessungen auf Windows abgeschlossen; M6-14b bleibt offen. Geprüft am 2. Oktober 2026.

## Host und Messbasis

Windows 11 Home Build 26200, NTFS auf Samsung SSD 970 EVO Plus 1TB NVMe, Intel Core i5-14600K (14 physische/20 logische Kerne), 68.448.346.112 Byte RAM, Rust 1.85.0. Ausgabe und temporäre Daten lagen unter `LOCALAPPDATA`, außerhalb des Repositories und OneDrive. Der verwendete M6-14a-Korpus hat Seed `0x574f524c44444232` und Dataset-SHA-256 `a8f623e9f82329e5b626ca21a6ada1b1af1e765184e278a9a711549066d962e5`.

## Ergebnisse

| Messung | Stichproben | p50 | p95 | p99 | Ergebnis |
|---|---:|---:|---:|---:|---|
| WAL-Kleinstcommit, 64-Byte-Payload, `commit_operation` inklusive beider `sync_all`-Aufrufe | 35 | 14,13 ms | 15,95 ms | 16,00 ms | p95 liegt unter dem vorläufigen 50-ms-Ziel für diesen Warmfall. |
| RecoveryManager-WAL-Scan, 360.000.000 Byte Provenance-Payload in 11 Commit-Frames | 30 | 1,291 s | 1,430 s | 1,470 s | Der langsamste Lauf entspricht 233,6 MiB/s; der Durchsatz liegt damit in allen Stichproben über 100 MiB/s. |

Gemessener Peak Working Set des Probeprozesses: 750.739.456 Byte (rund 716 MiB). Das überschreitet das vorläufige Soft-Profil von 512 MiB, bleibt aber unter 1 GiB. Der Peak entsteht im Recovery-Probeprozess, der die großen WAL-Payloads beim Verifizieren puffert; er ist kein separater Point-Query-Wert.

Alle erfassten Läufe sind Warmmessungen. Der Windows-Dateicache wurde nicht geleert; für diesen Zwischenstand wird kein Cold-Wert behauptet. Die Recovery-Messung prüft einen großen committed WAL mit generischen Operation-Payloads. Sie misst den vollständigen WAL-Scan/Recovery-Aufruf, aber weder das Replay eines typisierten Manifest-/History-Snapshots noch einen echten Prozessneustart oder Stromausfall.

## Rohdaten und Wiederholung

Die 65 Einzelmessungen stehen in [`docs/perf/M6-14b/windows-storage-probe.csv`](perf/M6-14b/windows-storage-probe.csv). Der Windows-spezifische Runner ist `tools/m6-14b/windows_probe.py`; der Rust-Probe ist `crates/worlddb-storage-file/examples/m6_14b_windows_probe.rs`. Der Runner verifiziert den vollständigen M6-14a-Korpus vor dem Lauf und hält alle temporären WAL-Dateien außerhalb des Repositories.

## Noch offen

- Point Resolution warm/kalt auf dem vollen Assertion-Korpus.
- 1.000 History-Seiten mit mindestens 30 Messungen pro Berichtspfad und p50/p95/p99 je Seite.
- Linux- und macOS-Läufe; diese bleiben auf Wunsch bis zu den späteren Systemprüfungen zurückgestellt.
- Ein Recovery-Lauf mit typisiertem Manifest-/History-Replay und ein belastbarer Cold-Cache-Aufbau.

Diese Stichproben sind ein messbarer Windows-Zwischenstand für Commit/Sync und WAL-Scan. Sie schließen M6-14b oder ODE-003 noch nicht ab.
