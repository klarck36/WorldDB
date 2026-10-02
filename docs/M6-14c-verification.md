# M6-14c – ODE-003 Windows-Zwischenentscheidung

**Ergebnis:** PASS für den lokalen Windows-Zwischenstand; finale ODE-003-Abnahme bleibt M9-07 vorbehalten. Geprüft am 2. Oktober 2026.

## Entscheidung

`docs/contract/ADR-040-performance-resource-budgets.md` hält fest, dass die vorläufigen absoluten Windows-Latenzziele und das 1-GiB-Hardziel unverändert bleiben. Der vollständige speicherresidente Query-/History-Referenzlauf überschritt das 512-MiB-Softprofil, blieb aber mit 938.717.184 Byte unter 1 GiB. Die jüngste CPU-cache-verdrängte History-p95 lag 32,5 % über der eingefrorenen Baseline, zugleich aber bei 96,2 µs gegenüber dem absoluten 250-ms-Ziel. Diese Abweichung ist nur für den Windows-Zwischenstand akzeptiert; M9-07 muss sie erneut bewerten.

## Nachweise

- `docs/M6-14b-windows-progress.md`: versionierter Messaufbau, Rohdateien, Quantile, Working Set, CPU-Cache-Modus, Grenzen und M9-07-Folgeprüfungen.
- Drei Wiederholungen nach dem Teilen unveränderlicher Assertion-Payloads: CPU-cache-verdrängte History-p95 100,4 / 86,5 / 96,2 µs; höchste seitenbezogene p95 215,7 µs.
- Warm-History-p95 42,1 µs; Point Resolution p95 9,943 ms warm und 10,561 ms CPU-cache-verdrängt; WAL-Kleinstcommit p95 15,95 ms; Recovery-Mindestdurchsatz 233,6 MiB/s.
- Der Softwert von 512 MiB wurde überschritten; der 1-GiB-Hardwert nicht.
- `WorldDB_1.0_Plancheck.py`: nach Aktualisierung der Task- und Statusregister erneut auszuführen.

## Abgrenzung

Dies ist keine finale Plattformfreigabe und keine Aussage über RSS-Begrenzung durch das Betriebssystem. Linux/macOS, Datei-/OS-Cold-Cache und typisiertes Recovery-Replay bleiben wie angewiesen für M9-07 offen. M6-15 prüft als nächstes, dass produktive Allokationspfade durch endliche Budgets abgesichert sind.
