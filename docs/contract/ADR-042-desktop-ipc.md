# ADR-042 – Versionierte Desktop-IPC und Host-Sitzungen

**Status:** Für Windows angenommen; Hostauthentisierung des nativen Desktophosts unter macOS/Linux in M8-26b/c implementiert; Plattformabnahme folgt M8-26b/c und M9-07
**Entscheidungsdatum:** 2026-10-03
**Task:** M8-09
**Evidenz:** `docs/M8-09-windows-progress.md`

## Kontext

M8-08 hat Tauri 2.11.5 mit In-Process-Engine und Sidecar verglichen. M8-09 muss die Fenstergrenze als versionierte, begrenzte IPC implementieren. Der Renderer darf weder den WorldDB-Principal festlegen noch Dateipfade, Storagehandles oder unbeschränkte Datenoperationen an den Host übergeben. Die Betriebssystemidentität muss vom nativen Host kommen.

## Entscheidung

1. **Vertrag v1:** Jede Renderer-JSON-Anfrage und -Antwort trägt `protocol_version: 1`; Binärblöcke binden dieselbe Version in einem festen IPC-Header. DTOs weisen unbekannte Felder zurück. Öffentliche Fehler enthalten nur einen stabilen Fehlercode und die Protokollversion.
2. **Hostidentität:** Unter Windows liest der Host die SID aus dem Prozesstoken (`OpenProcessToken`/`GetTokenInformation`). Unter macOS liest er die effektive UID und `IOPlatformUUID` aus der IOKit-`IOResources`-Property; unter Linux liest er die effektive UID und `/etc/machine-id`. Der Prozessadapter validiert und taggt die Werte vor der gemeinsamen Principalableitung. Host und Sidecar lesen die OS-Identität jeweils selbst. Ein opakes Zufallsticket bindet die Hostidentität und das native Tauri-Fensterlabel. Tickets laufen nach 15 Minuten ab; höchstens 16 sind gleichzeitig gültig. Nicht unterstützte Fenster oder fehlende/ungültige Identität werden abgewiesen. Kein Rendererwert und keine Umgebungsvariable kann die Identität auswählen.
3. **Capabilities:** Hostoperationen verlangen serverseitig `health.read` oder `transfer.write`. Beide freigegebenen Fenster können diese Fähigkeiten erhalten. Die Rendererrequest bestimmt weder Capability noch Fensterlabel.
4. **Minimale Rendererrechte:** Tauri-Capability `default` erteilt keine Pluginberechtigungen. Der Desktop-Shell hängt kein Dateisystem- oder Shellplugin ein. Das Frontend lädt ausschließlich lokale Skripte; CSP beschränkt Ziele auf die lokale Seite und Tauri-IPC. Alle fachlichen Zugriffe sind auf die registrierten Hostcommands begrenzt.
5. **Begrenzter Bulkstream:** Ein Transfer enthält höchstens 100 MiB, jeder Rohblock höchstens 256 KiB. Es sind maximal 4 Transfers je Sitzung und 16 insgesamt aktiv; ein Transfer verfällt nach 5 Minuten. Eine streng steigende Sequenznummer bindet jeden Block. Der Host bestätigt erst nach erfolgreicher Engineannahme; das Frontend wartet auf jedes ACK, bevor es den nächsten Block sendet. Damit liegt höchstens ein nicht bestätigter Block pro Sender an.
6. **Cancellation und Integrität:** Der Renderer darf zwischen bestätigten Blöcken abbrechen. Der Engine-Consumer akzeptiert jeden Präfix vor dem Gesamtumfang, entfernt den Transfer danach und meldet Länge und BLAKE3-Digest. Vollständiger Abschluss verlangt exakt die angekündigte Bytezahl. M8-09 stellt damit den IPC-Transport bereit; das Projektbootstrap und die Projektöffnung verwenden die Hostbindung aus M8-10/ADR-043.
7. **Sidecar:** Der lokale Pipe-Handshake ist Protokollversion 1. Steuerung läuft als JSONL; Binärdaten laufen in begrenzten Frames. Jeder Sidecar-Aufruf wartet höchstens 5 Sekunden. Ein Timeout oder Pipefehler beendet und reapet das Kind; der nächste Hostaufruf startet es neu und prüft den Writerlock, bevor er weiterarbeitet. Der fehlgeschlagene Aufruf wird nicht wiederholt und erhält einen generischen Fehler.
8. **Netzwerk:** Die Desktop-Engine öffnet standardmäßig keinen Netzwerklistener. Die Windows-End-to-End-Prüfung kontrolliert die Prozess-IDs von App und Sidecar.
9. **Plattformumfang:** Für Unix ist die Identität hostlokal: dieselbe effektive UID auf einem anderen OS-Host erhält einen anderen Principal. macOS bezieht die Plattform-UUID über IOKit; `kern.uuid` wird nicht verwendet, weil XNU damit die UUID des Kernel-Images bereitstellt. Fehlt eine gültige Host-UUID, bleibt der Start fail-closed. Die native IPC-, CSP-/WebView- und Paketierungsabnahme unter Linux/macOS bleibt M9-07 vorbehalten. Die CLI-Backup-/Restore-Identität bleibt bis zu einer eigenen Vertragsprüfung Windows-only.

## Folgen

- Der Renderer kann keine Datenbank oder Quelldatei über einen freien Pfad auswählen. M8-10 übernimmt Projektordner über native Hostdialoge, validiert sie und bindet den aktuellen Konto-Principal intern an die Engine; Einzelheiten stehen in ADR-043.
- Raw-Binärdaten sind nur als begrenzte Nutzlast eines autorisierten, versionierten Transfers zulässig. Es gibt keinen Raw-Storage-, SQL-, Dateisystem- oder Principal-Command.
- Sequenzfehler, fremde Sitzungen, Fensterwechsel, falsche Protokollversion, abgelaufene Tickets und zu große Blöcke scheitern vor einem fachlichen Enginezugriff.
- Nach Timeout wird kein unklarer Stream automatisch fortgesetzt. Der Renderer muss ihn als abgebrochen anzeigen und einen neuen Transfer starten.

## Hostidentitätsquellen

- Apple beschreibt `IOPlatformUUID` als Maschinen-UUID und dokumentiert die IOKit-Registry-Lese-APIs: [IOKit keys](https://github.com/apple-oss-distributions/xnu/blob/main/iokit/IOKit/IOKitKeys.h#L228-L232), [Plattform-UUID-Erzeugung](https://github.com/apple-oss-distributions/xnu/blob/main/iokit/Kernel/IOPlatformExpert.cpp#L2090-L2143), [Property lesen](https://developer.apple.com/documentation/iokit/1514293-ioregistryentrycreatecfproperty), [Service finden](https://developer.apple.com/documentation/iokit/1514535-ioservicegetmatchingservice).
- XNU bindet `kern.uuid` an `kernel_uuid_string`; dieser Wert ist die UUID des Kernel-Images und damit keine Hostidentität: [XNU sysctl registration](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sysctl.c#L1764-L1766).
- POSIX `geteuid()` liefert die effektive User-ID des aufrufenden Prozesses: https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/unistd.h.html.
- systemd initialisiert `/etc/machine-id` als lokale Maschinen-ID und dokumentiert deren Bereitstellung für Systeme und Container: https://www.freedesktop.org/software/systemd/man/250/systemd-machine-id-setup.html.

## Implementierungsreferenzen

- Tauri Raw Request Body und Request-Header: [Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)
- Tauri Content Security Policy: [CSP](https://v2.tauri.app/security/csp/)
