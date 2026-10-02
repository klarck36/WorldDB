# ADR-040 – Leistungs- und Ressourcenbudgets (ODE-003)

**Status:** Vorläufig angenommen für das lokale M6-Windows-Profil; finale Abnahme bleibt M9-07 vorbehalten
**Entscheidungsdatum:** 2026-10-02
**Entscheidungsebene:** M6-14c technische Zwischenentscheidung
**Evidenz:** `docs/M6-14b-windows-progress.md`

## Kontext

M6-14b hat die versionierte Referenzlast auf Windows 11 mit Rust 1.85.0 gemessen. Die Ergebnisse erfüllen die provisorischen absoluten Latenz- und 1-GiB-Hardziele. Das 512-MiB-Softprofil wird beim vollständigen speicherresidenten Referenzlauf überschritten. Die CPU-cache-verdrängte History-p95 stieg gegenüber der eingefrorenen Baseline um 32,5 %, bleibt mit 96,2 µs aber rund 2.600-mal unter dem absoluten 250-ms-Seitenziel.

Die Messung leert weder den Windows-Dateicache noch simuliert sie einen Prozessneustart, Stromausfall oder typisiertes Manifest-/History-Replay. Linux- und macOS-Messungen wurden auf Anweisung des Product Owners in die spätere Plattformprüfung verschoben.

## Entscheidung

1. Die vorläufigen absoluten Latenzziele bleiben für den Windows-Zwischenstand unverändert. Der gemessene History-p95-Anstieg wird wegen der sehr großen absoluten Reserve und der unveränderten warmen Seitenlatenz für diese Zwischenabnahme akzeptiert. Die relative Abweichung bleibt sichtbar und wird nicht als allgemeine oder plattformübergreifende Freigabe behandelt.
2. Die Prozessdefaults bleiben versioniert auf **512 MiB Soft** und **1 GiB Hard**. Das Softlimit ist eine Druck-/Warnschwelle; das Hardlimit ist die Obergrenze der ausdrücklich reservierten Prozessallokationen. Operation-spezifische Query-, Parser- und Indexgrenzen bleiben zusätzlich verpflichtend.
3. M6-14b gilt als bestanden für seine Windows-Messphase. Die M6-14c-Entscheidung ist ausdrücklich vorläufig und schließt ODE-003 nicht final.
4. M9-07 wiederholt die Regression gegen dieselbe eingefrorene Baseline auf den erforderlichen Plattformen und ergänzt Datei-/OS-Cold-Cache sowie typisiertes Recovery-Replay. Es muss die 32,5-%-Abweichung, die Softlimitüberschreitung und den Peak gegen die dann vollständigen Messungen neu bewerten.

## Folgen

- Für den gemessenen Query-/History-Prozess lag der jüngste Peak Working Set bei 938.717.184 Byte (rund 895 MiB), unter 1 GiB und über 512 MiB.
- M6-15 muss die versionierten Defaults, die kooperative Admission-API und die endlichen Query-/Parser-/Indexgrenzen nachweisen. Die Admission-API ist keine Betriebssystem-RSS-Garantie.
- Die zusätzliche Abnahme bleibt in `docs/M9-07-verification.md` fällig; diese ADR darf dort ersetzt oder revidiert werden.
