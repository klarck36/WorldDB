# ADR-042 – Versionierte Desktop-IPC und Host-Sitzungen

**Status:** Für den Windows-Zwischenstand angenommen; Linux/macOS-Implementierung und -Abnahme folgen M9-07
**Entscheidungsdatum:** 2026-10-03
**Task:** M8-09
**Evidenz:** `docs/M8-09-windows-progress.md`

## Kontext

M8-08 hat Tauri 2.11.5 mit In-Process-Engine und Sidecar verglichen. M8-09 muss die Fenstergrenze als versionierte, begrenzte IPC implementieren. Der Renderer darf weder den WorldDB-Principal festlegen noch Dateipfade, Storagehandles oder unbeschränkte Datenoperationen an den Host übergeben. Die Betriebssystemidentität muss vom nativen Host kommen.

## Entscheidung

1. **Vertrag v1:** Jede Renderer-JSON-Anfrage und -Antwort trägt `protocol_version: 1`; Binärblöcke binden dieselbe Version in einem festen IPC-Header. DTOs weisen unbekannte Felder zurück. Öffentliche Fehler enthalten nur einen stabilen Fehlercode und die Protokollversion.
2. **Hostidentität:** Unter Windows liest der Host die SID aus dem Prozesstoken (`OpenProcessToken`/`GetTokenInformation`). Ein opaker Zufallsticket bindet die Hostidentität und das native Tauri-Fensterlabel. Tickets laufen nach 15 Minuten ab; höchstens 16 sind gleichzeitig gültig. Nicht unterstützte Fenster oder fehlende Identität werden abgewiesen. Die SID und der WorldDB-Principal werden nie als Rendererargument akzeptiert. M8-10 löst die projektspezifische Principalbindung auf.
3. **Capabilities:** Hostoperationen verlangen serverseitig `health.read` oder `transfer.write`. Beide freigegebenen Fenster können diese Fähigkeiten erhalten. Die Rendererrequest bestimmt weder Capability noch Fensterlabel.
4. **Minimale Rendererrechte:** Tauri-Capability `default` erteilt keine Pluginberechtigungen. Der Desktop-Shell hängt kein Dateisystem- oder Shellplugin ein. Das Frontend lädt ausschließlich lokale Skripte; CSP beschränkt Ziele auf die lokale Seite und Tauri-IPC. Alle fachlichen Zugriffe sind auf die registrierten Hostcommands begrenzt.
5. **Begrenzter Bulkstream:** Ein Transfer enthält höchstens 100 MiB, jeder Rohblock höchstens 256 KiB. Es sind maximal 4 Transfers je Sitzung und 16 insgesamt aktiv; ein Transfer verfällt nach 5 Minuten. Eine streng steigende Sequenznummer bindet jeden Block. Der Host bestätigt erst nach erfolgreicher Engineannahme; das Frontend wartet auf jedes ACK, bevor es den nächsten Block sendet. Damit liegt höchstens ein nicht bestätigter Block pro Sender an.
6. **Cancellation und Integrität:** Der Renderer darf zwischen bestätigten Blöcken abbrechen. Der Engine-Consumer akzeptiert jeden Präfix vor dem Gesamtumfang, entfernt den Transfer danach und meldet Länge und BLAKE3-Digest. Vollständiger Abschluss verlangt exakt die angekündigte Bytezahl. M8-09 stellt damit den IPC-Transport bereit; Speicherung/Projektzugriff werden erst an die M8-10-Projektbindung angeschlossen.
7. **Sidecar:** Der lokale Pipe-Handshake ist Protokollversion 1. Steuerung läuft als JSONL; Binärdaten laufen in begrenzten Frames. Jeder Sidecar-Aufruf wartet höchstens 5 Sekunden. Ein Timeout oder Pipefehler beendet und reapet das Kind; der nächste Hostaufruf startet es neu und prüft den Writerlock, bevor er weiterarbeitet. Der fehlgeschlagene Aufruf wird nicht wiederholt und erhält einen generischen Fehler.
8. **Netzwerk:** Die Desktop-Engine öffnet standardmäßig keinen Netzwerklistener. Die Windows-End-to-End-Prüfung kontrolliert die Prozess-IDs von App und Sidecar.
9. **Plattformumfang:** Die Windows-SID-Implementierung ist für nicht unterstützte Betriebssysteme fail-closed. Plattformauthentisierung, CSP-/WebView-Verhalten und Paketierung unter Linux/macOS werden im vereinbarten M9-07 nachgewiesen.

## Folgen

- Der Renderer kann keine Datenbank oder Quelldatei über einen freien Pfad auswählen. M8-10 muss den Projektpfad aus einem vertrauenswürdigen Host-Dialog übernehmen, validieren und intern an die Engine binden.
- Raw-Binärdaten sind nur als begrenzte Nutzlast eines autorisierten, versionierten Transfers zulässig. Es gibt keinen Raw-Storage-, SQL-, Dateisystem- oder Principal-Command.
- Sequenzfehler, fremde Sitzungen, Fensterwechsel, falsche Protokollversion, abgelaufene Tickets und zu große Blöcke scheitern vor einem fachlichen Enginezugriff.
- Nach Timeout wird kein unklarer Stream automatisch fortgesetzt. Der Renderer muss ihn als abgebrochen anzeigen und einen neuen Transfer starten.

## Implementierungsreferenzen

- Tauri Raw Request Body und Request-Header: [Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)
- Tauri Content Security Policy: [CSP](https://v2.tauri.app/security/csp/)
