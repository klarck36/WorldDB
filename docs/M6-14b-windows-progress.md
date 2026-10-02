# M6-14b – Windows-Messungen (Zwischenstand)

**Status:** Windows-Phase abgeschlossen; Linux/macOS sowie Datei-/OS-Cold-Cache und typisiertes Recovery-Replay bleiben für M9-07 vorgemerkt. Geprüft am 2. Oktober 2026.

## Host und Messbasis

Windows 11 Home Build 26200, NTFS auf Samsung SSD 970 EVO Plus 1TB NVMe, Intel Core i5-14600K (14 physische/20 logische Kerne), 68.448.346.112 Byte RAM, Rust 1.85.0. Ausgabe und temporäre Daten lagen unter `LOCALAPPDATA`, außerhalb des Repositories und OneDrive. Der verwendete M6-14a-Korpus hat Seed `0x574f524c44444232` und Dataset-SHA-256 `a8f623e9f82329e5b626ca21a6ada1b1af1e765184e278a9a711549066d962e5`.

## Ergebnisse

| Messung | Stichproben | p50 | p95 | p99 | Ergebnis |
|---|---:|---:|---:|---:|---|
| WAL-Kleinstcommit, 64-Byte-Payload, `commit_operation` inklusive beider `sync_all`-Aufrufe | 35 | 14,13 ms | 15,95 ms | 16,00 ms | p95 liegt unter dem vorläufigen 50-ms-Warmziel. |
| RecoveryManager-WAL-Scan, 360.000.000 Byte Provenance-Payload in 11 Commit-Frames | 30 | 1,291 s | 1,430 s | 1,470 s | Langsamster Lauf: 233,6 MiB/s; alle Läufe liegen über 100 MiB/s. |
| Point Resolution, voller 1-Mio.-Assertion-Korpus, warm | 30 | 9,636 ms | 9,943 ms | 10,257 ms | p95 liegt unter dem vorläufigen 20-ms-Warmziel; jede Probe ergab eine Assertion. |
| Point Resolution nach Verdrängung des CPU-Caches | 30 | 9,848 ms | 10,561 ms | 11,318 ms | p95 liegt unter dem vorläufigen 150-ms-Ziel. 64 MiB RAM wurden berührt; Windows-Dateicache blieb unverändert. |
| Historyseiten, warm, 1.089 Seiten mit je 30 Proben | 32.670 | 36,9 µs | 42,1 µs | 73,3 µs | Höchstes seitenbezogenes p95: 144,9 µs auf Seite 758; unter 250 ms. |
| Historyseiten nach Verdrängung des CPU-Caches | 32.670 | 54,4 µs | 96,2 µs | 129,6 µs | Höchstes seitenbezogenes p95: 203,6 µs auf Seite 148; unter 250 ms, Windows-Dateicache blieb unverändert. |

Der jüngste Query-/History-Probeprozess erreichte einen Peak Working Set von 938.717.184 Byte (rund 895 MiB). Damit blieb er rund 129 MiB unter dem vorläufigen 1-GiB-Hardlimit, überschritt aber das 512-MiB-Softprofil. Der Vergleichslauf vor dem Teilen unveränderlicher Assertion-Payloads lag bei 1.226.633.216 Byte; die gemeinsame `Arc`-Speicherung in Historie und Index senkte den Peak um rund 275 MiB. Der getrennte WAL-/Recovery-Probeprozess erreichte 750.739.456 Byte (rund 716 MiB); sein Peak entstand beim Puffern der großen WAL-Payloads während der Verifikation.

Die CPU-cache-verdrängte History-p95 lag in drei Wiederholungen nach dem Speicherumbau bei 100,4, 86,5 und 96,2 µs, gegenüber 72,6 µs in der eingefrorenen Vorher-Baseline. Die jüngste Wiederholung liegt damit relativ 32,5 % höher; p50 stieg 3,4 %, p99 9,3 %. Die warmen Historyseiten verbesserten sich gegenüber der Baseline bei p95 um 4,3 %. Selbst das jüngste kalte p95 bleibt rund 2.600-mal unter dem absoluten 250-ms-Seitenziel; das höchste seitenbezogene p95 der drei Wiederholungen lag bei 215,7 µs. Der gemessene `run_page_pass`-Pfad (`ProductiveQueryEngine::stream_page` mit residenten Adapterzeilen) wurde nicht geändert und misst keinen Datenträger-Cold-Cache. ODE-003 akzeptiert diese kleine absolute, nur im CPU-cache-verdrängten p95 sichtbare Abweichung ausdrücklich für das Windows-Zwischenprofil. M9-07 muss sie auf drei Plattformen erneut gegen die eingefrorene Baseline prüfen.

Der Query-/History-Lauf verwendete den versionierten M6-14a-Korpus mit 1.000.000 Assertions, 100 HistorySpaces und 69.653 Zeilen in der ausgewählten HistorySpace-Abstammung. Die Cache-Proben verdrängen den CPU-Cache mit einem berührten, speicherresidenten 64-MiB-Puffer; der Windows-Dateicache wurde nicht geleert. Das sind keine Messungen eines kalten Datenträgers. Die Recovery-Messung prüft einen großen committed WAL mit generischen Operation-Payloads. Sie misst den vollständigen WAL-Scan/Recovery-Aufruf, aber weder das Replay eines typisierten Manifest-/History-Snapshots noch einen echten Prozessneustart oder Stromausfall.

## Rohdaten und Wiederholung

Die 65 WAL-/Recovery-Messungen stehen in [`docs/perf/M6-14b/windows-storage-probe.csv`](perf/M6-14b/windows-storage-probe.csv). Die jüngsten Point-/History-Rohwerte, p50/p95/p99 jeder Seite und Host-/Compiler-/Korpusmetadaten stehen in [`docs/perf/M6-14b/windows-query-history.csv`](perf/M6-14b/windows-query-history.csv), [`docs/perf/M6-14b/windows-query-history-page-summary.csv`](perf/M6-14b/windows-query-history-page-summary.csv) und [`docs/perf/M6-14b/windows-query-history-summary.json`](perf/M6-14b/windows-query-history-summary.json). Die erste und zweite Wiederholung nach dem Speicherumbau sind separat als `windows-query-history-shared-memory-run1*` und `windows-query-history-shared-memory-run2*` archiviert. Der Windows-WAL-Runner ist `tools/m6-14b/windows_probe.py`; der Query-/History-Runner ist `tools/m6-14b/windows_query_history_probe.py`. Beide verifizieren den vollständigen M6-14a-Korpus; temporäre Daten liegen außerhalb des Repositories und OneDrive.

## Noch offen

- Das 512-MiB-Softprofil wird im Full-Corpus-Referenzlauf überschritten; M6-15 muss Warn-/Admission-Semantik des versionierten Ressourcenprofils nachweisen. Der jüngste Messwert bleibt unter 1 GiB.
- Echter Datei-/OS-Cold-Cache-Test und typisiertes Manifest-/History-Replay bei Recovery.
- Linux- und macOS-Läufe; sie bleiben auf Wunsch bis zu den späteren Systemprüfungen zurückgestellt.

Die Windows-Fälle liegen bei Point Resolution, Historyseiten, Kleinstcommit, WAL-Scan und dem 1-GiB-Hardlimit innerhalb der vorläufigen absoluten Grenzwerte. Das 512-MiB-Softprofil wird im Full-Corpus-Referenzlauf überschritten. ODE-003 ist mit ADR-040 für den Windows-Zwischenstand entschieden; die vollständige Drei-Plattform-Abnahme und die zusätzlichen Cache-/Recovery-Fälle bleiben M9-07.
