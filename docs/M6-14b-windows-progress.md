# M6-14b – Windows-Messungen (Zwischenstand)

**Status:** Windows-Teilproben abgeschlossen; M6-14b bleibt offen. Geprüft am 2. Oktober 2026.

## Host und Messbasis

Windows 11 Home Build 26200, NTFS auf Samsung SSD 970 EVO Plus 1TB NVMe, Intel Core i5-14600K (14 physische/20 logische Kerne), 68.448.346.112 Byte RAM, Rust 1.85.0. Ausgabe und temporäre Daten lagen unter `LOCALAPPDATA`, außerhalb des Repositories und OneDrive. Der verwendete M6-14a-Korpus hat Seed `0x574f524c44444232` und Dataset-SHA-256 `a8f623e9f82329e5b626ca21a6ada1b1af1e765184e278a9a711549066d962e5`.

## Ergebnisse

| Messung | Stichproben | p50 | p95 | p99 | Ergebnis |
|---|---:|---:|---:|---:|---|
| WAL-Kleinstcommit, 64-Byte-Payload, `commit_operation` inklusive beider `sync_all`-Aufrufe | 35 | 14,13 ms | 15,95 ms | 16,00 ms | p95 liegt unter dem vorläufigen 50-ms-Warmziel. |
| RecoveryManager-WAL-Scan, 360.000.000 Byte Provenance-Payload in 11 Commit-Frames | 30 | 1,291 s | 1,430 s | 1,470 s | Langsamster Lauf: 233,6 MiB/s; alle Läufe liegen über 100 MiB/s. |
| Point Resolution, voller 1-Mio.-Assertion-Korpus, warm | 30 | 9,638 ms | 10,115 ms | 10,232 ms | p95 liegt unter dem vorläufigen 20-ms-Warmziel; jede Probe ergab eine Assertion. |
| Point Resolution nach Verdrängung des CPU-Caches | 30 | 10,216 ms | 10,952 ms | 15,316 ms | p95 liegt unter dem vorläufigen 150-ms-Ziel. 64 MiB RAM wurden berührt; Windows-Dateicache blieb unverändert. |
| Historyseiten, warm, 1.089 Seiten mit je 30 Proben | 32.670 | 35,9 µs | 44,0 µs | 76,8 µs | Höchstes seitenbezogenes p95: 174,5 µs auf Seite 234; unter 250 ms. |
| Historyseiten nach Verdrängung des CPU-Caches | 32.670 | 52,6 µs | 72,6 µs | 118,6 µs | Höchstes seitenbezogenes p95: 199,8 µs auf Seite 683; unter 250 ms, Windows-Dateicache blieb unverändert. |

Der Query-/History-Probeprozess erreichte einen Peak Working Set von 1.226.633.216 Byte (rund 1,14 GiB) und lag damit über dem vorläufigen 1-GiB-Hardlimit. Der Wert umfasst den vollständigen In-Memory-Referenzkorpus, das Archivinventar und den gebauten Index im selben Prozess; er ist kein marginaler Einzelabfragewert. M6-15 muss das Ressourcenbudget am produktiven Speicher-/Indexmodell prüfen. Der getrennte WAL-/Recovery-Probeprozess erreichte 750.739.456 Byte (rund 716 MiB); sein Peak entstand beim Puffern der großen WAL-Payloads während der Verifikation.

Der Query-/History-Lauf verwendete den versionierten M6-14a-Korpus mit 1.000.000 Assertions, 100 HistorySpaces und 69.653 Zeilen in der ausgewählten HistorySpace-Abstammung. Die Cache-Proben verdrängen den CPU-Cache mit einem berührten, speicherresidenten 64-MiB-Puffer; der Windows-Dateicache wurde nicht geleert. Das sind keine Messungen eines kalten Datenträgers. Die Recovery-Messung prüft einen großen committed WAL mit generischen Operation-Payloads. Sie misst den vollständigen WAL-Scan/Recovery-Aufruf, aber weder das Replay eines typisierten Manifest-/History-Snapshots noch einen echten Prozessneustart oder Stromausfall.

## Rohdaten und Wiederholung

Die 65 WAL-/Recovery-Messungen stehen in [`docs/perf/M6-14b/windows-storage-probe.csv`](perf/M6-14b/windows-storage-probe.csv). Die Point-/History-Rohwerte, p50/p95/p99 jeder Seite und Host-/Compiler-/Korpusmetadaten stehen in [`docs/perf/M6-14b/windows-query-history.csv`](perf/M6-14b/windows-query-history.csv), [`docs/perf/M6-14b/windows-query-history-page-summary.csv`](perf/M6-14b/windows-query-history-page-summary.csv) und [`docs/perf/M6-14b/windows-query-history-summary.json`](perf/M6-14b/windows-query-history-summary.json). Der Windows-WAL-Runner ist `tools/m6-14b/windows_probe.py`; der Query-/History-Runner ist `tools/m6-14b/windows_query_history_probe.py`. Beide verifizieren den vollständigen M6-14a-Korpus; temporäre Daten liegen außerhalb des Repositories und OneDrive.

## Noch offen

- Windows-Queryprozess überschreitet mit 1,14 GiB den vorläufigen 1-GiB-Hardwert; M6-15 muss dies am produktiven Ressourcenbudget prüfen.
- Echter Datei-/OS-Cold-Cache-Test und typisiertes Manifest-/History-Replay bei Recovery.
- Linux- und macOS-Läufe; sie bleiben auf Wunsch bis zu den späteren Systemprüfungen zurückgestellt.

Die gemessenen Windows-Fälle liegen bei Point Resolution, Historyseiten, Kleinstcommit und WAL-Scan innerhalb der vorläufigen Latenzziele. M6-14b bleibt wegen der noch ausstehenden Plattformläufe sowie zusätzlicher Cache- und Recovery-Fälle offen; ODE-003 wird erst danach entschieden.
