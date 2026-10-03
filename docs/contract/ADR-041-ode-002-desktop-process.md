# ADR-041 – Desktop-Prozessgrenze (ODE-002)

**Status:** Vorläufig angenommen für den Windows-Zwischenstand; finale Plattformentscheidung bleibt M9-07 vorbehalten
**Entscheidungsdatum:** 2026-10-03
**Task:** M8-08
**Evidenz:** `docs/M8-08-windows-progress.md`

## Kontext

ODE-002 verlangt einen nativen Vergleich zwischen einer Engine im Tauri-Prozess und einem lokalen Engine-Prozess. Der Vergleich muss Mehrfensterbetrieb, exklusiven Writerlock, Engine-Panic, Neustart, einen 100-MiB-Datenstrom, Cancellation und Updateverhalten einschließen. Dieser Zwischenstand wurde auf Windows mit Rust 1.85.0 und Tauri 2.11.5 erhoben. macOS und Linux bleiben wie vereinbart M9-07 zugeordnet.

## Entscheidung

1. Der Desktop verwendet **Tauri 2 mit einer separaten lokalen Rust-Engine als Sidecar**. Alle Fenster teilen denselben Engine-Prozess und dieselbe Engine-Sitzung; nur die Engine hält den exklusiven WorldDB-Writerlock. Für die Desktop-Engine wird kein Netzwerklistener geöffnet.
2. Der Sidecar ist eine **Crash- und Updategrenze, keine Sicherheits- oder Privilegiengrenze**. Er läuft unter demselben angemeldeten Betriebssystemkonto und besitzt dieselben Betriebssystemrechte wie die Anwendung. Die Prozessaufteilung gewährt weder dem Renderer noch einem Projekt zusätzliche Rechte.
3. Die Produktions-IPC wird in M8-09 als versionierter, typisierter Vertrag umgesetzt. Der Renderer darf weder den Principal wählen noch freie Dateipfade, Storagehandles oder Raw-Storagezugriff vorgeben. Der Host bindet Sitzungen an die authentisierte Betriebssystemidentität; M8-10 bindet das Projekt an den WorldDB-Principal. Die verbindliche Umsetzung steht in ADR-042; der ursprüngliche Prozessspike allein gilt nicht als gehärtete Produktions-IPC.
4. Der Produktions-IPC-Vertrag muss endliche Nachrichten- und Chunklimits, Cancellation, Backpressure, Fehler-/Timeoutbehandlung, Protokollkompatibilität und sichere Wiederverbindung festlegen. M8-09 ergänzt und prüft diese Grenzen; `docs/M8-09-windows-progress.md` enthält die Windows-Nachweise. Der hier beschriebene ODE-002-Matrixlauf bleibt der Nachweis für Prozessgrenze, Prozessneustart und Digests.
5. Ein Engine-Panic beendet im Sidecar-Modus nur den Engine-Prozess. Die Anwendung darf den Childprozess kontrolliert neu starten und den Writerlock erneut erwerben; bis dahin können die Fenster keinen Engine-Schreibzugriff anbieten. Beim In-Process-Modus ist der Enginezustand nach einem Panic nicht wiederherstellbar und die Desktop-Anwendung muss vollständig neu gestartet werden.
6. Ein Sidecar-Update wird als versionierter Binärwechsel nach geordnetem Stop des alten Engine-Prozesses durchgeführt. Die Tauri-Anwendung kann dabei weiterlaufen. Ein In-Process-Update ersetzt die Desktop-Anwendung und erfordert deren Neustart. Das Produktionspaket muss Updates verifizieren, Kompatibilität vor dem Wechsel prüfen, einen vorherigen Build für Rollback erhalten und die Version atomar aktivieren; Signatur- und Installationsintegration sind M9-10 zu belegen.
7. Die Windows-Spike-Matrix misst fünf vollständige 100-MiB-Läufe und fünf bei 8 MiB abgebrochene Läufe je Prozessmodus. Beide Modi liefern in allen Fällen denselben BLAKE3-Digest. Die beiden Matrixwiederholungen zeigen keine stabile Geschwindigkeitsreihenfolge: vollständige Laufmediane lagen bei 678.725 µs gegen 685.948 µs sowie 720.420 µs gegen 694.508 µs (In-Process gegen Sidecar). Die wenigen lokalen Messläufe begründen daher keine allgemeine Leistungsbehauptung.
8. Die Prozesswahl gilt vorläufig für den Windows-Zwischenstand. M9-07 wiederholt die Fenster-, Lock-, Crash-, Streaming-, Cancellation-, Neustart- und Updateprüfungen auf macOS/Linux und bestätigt oder revidiert ADR-041 samt Packagingfolgen.

## Folgen

- Renderer und Tauri-Anwendung besitzen keinen direkten Storagezugriff; alle fachlichen Aktionen laufen über den Engine-/Hostvertrag.
- Die Engine kann unabhängig von der laufenden Fensteranwendung neu gestartet und aktualisiert werden. Projektzugriffe bleiben durch genau einen Engine-Writerlock serialisiert.
- Prozessisolation ersetzt keine Hostautorisierung, Renderer-Capabilityprüfung, Principalbindung, sichere Fehlerbehandlung oder Pfadvalidierung.
- Windows protokollierte beim WebView2-Abbau `Chrome_WidgetWin_0 / 1412`. Die Prozess- und Lockprüfungen bestanden; diese Meldung wurde nicht unterdrückt und bleibt als beobachtete Hostdiagnose dokumentiert.
- `docs/M8-08-windows-progress.md` enthält die Matrixausgaben, Kommandos und Grenzen des lokalen Nachweises.
