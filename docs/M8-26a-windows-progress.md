# M8-26a – Windows-E2E-Ergebnis

## Vorläufe

Der erste vollständige Windows/NTFS-Lauf vom 2026-10-05 (`m8-26a-20261005T012844Z-823933db`) endete mit `FAIL`. Builds, IPC-Smokes (56/56 je Profil) und konkurrierende Schreiber (4 In-Process-, 5 Sidecar-Prüfungen) bestanden. Die Tastatur- und Commit-Crashfälle scheiterten damals beim UI-Automation-Aufruf `SetFocus`, bevor Tastatureingabe oder Commit-Failpoint erreicht wurden.

Ein weiterer Lauf (`m8-26a-20261005T134338Z-c303fb1a`) bestätigte Builds, beide IPC-Profile, beide Writer-Lock-Fälle und die Commit-Recovery. Er endete mit drei Fehlern beim Aufräumen temporärer WebView2-Profile. Die Keyboard-Resultatdateien meldeten Eingabe, Tab/Enter und den Commit-Crash samt read-only Recovery bereits als `PASS`; die Profile waren beim Löschen noch kurz gesperrt. Das Manifest dieses Laufs bleibt als Fehlerbeleg erhalten.

## Korrekturen

- Der Keyboard-Smoke setzt `WORLDDB_ODE_PROJECT_SMOKE_ROOT` auf den erwarteten Projektpfad und deaktiviert den automatischen Startup-Smoke. `WORLDDB_ODE_DATABASE` wird entfernt, weil es das Projekt bereits beim App-Start öffnet und damit die Erstellungsprüfung stört.
- Der Sidecar-Modus legt das Projekt nun in einem host-authentifizierten Sidecar-Prozess an. Der WAL-Failpoint beendet damit den tatsächlichen Engine-Prozess.
- Die Bereinigung des isolierten Testverzeichnisses wiederholt Löschversuche bis zu 15 Sekunden. Bei Fehlern wird das flüchtige `webview-profile` nicht ins Fehlerarchiv kopiert.

## Vollständiger Windows-Lauf

Das Manifest `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T140119Z-29c16683/manifest.json` meldet auf Windows 11/NTFS `PASS`: 11 von 11 Fällen, 138 Prüfschritte, keine Fehler und kein übersprungener Fall. Es wurde auf Commit `73b65dbfae2f27761471f7d012761e11d6fcb07b` mit sauberem Arbeitsbaum ausgeführt.

- IPC: je 56 Prüfungen für In-Process und Sidecar bestanden.
- Konkurrierende Schreiber: 4 In-Process- und 5 Sidecar-Prüfungen bestanden.
- Tastatur: Texteingabe, Tab-Navigation und Enter-Erstellen in beiden Modi bestanden.
- Commit-Recovery: In-Process-App und Sidecar-Engine wurden nach dem WAL-Sync mit Exitcode 86 beendet; die anschließende read-only Recovery-Prüfung bestand in beiden Modi.

## Wiederholung auf dem aktuellen Branch

Der Lauf `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T154837Z-2b012994/manifest.json` wurde auf Commit `01ab573a620ade99f96ced7e46249734a2283c36` mit sauberem Arbeitsbaum ausgeführt und meldet `FAIL`: 7/11 Fälle bestanden, 0 nicht ausgeführt. Builds, beide IPC-Fälle (56/56 je Profil) und beide Writer-Lock-Fälle (4 bzw. 5 Prüfschritte) bestanden. Die vier Fälle `keyboard_navigation` und `commit_crash_recovery` brachen in beiden Modi vor jeder Eingabe ab.

Die gemeinsame Ursache war `SetCursorPos`: Windows meldete `false`, obwohl der geprüfte Titelleistenpunkt innerhalb des WorldDB-Fensters lag. Eine isolierte Wiederholung meldete Fehlercode 0 bei Ziel `(268,218)`, Fenstergrenzen `(208,208,704,567)` und Cursorposition `(355,480)`. Zusätzlich lieferte ein Setzen auf die bereits aktuelle Cursorposition `false`; der Eingabe-Desktop war `Default`, und weder `LockApp` noch ein Speed-/Auto-Klickerprozess lief. Die native Computer-Use-Schnittstelle fand beide WorldDB-Fenster, konnte das Primärfenster nach einer frischen Erfassung aber nicht aktivieren. In keinem der vier fehlgeschlagenen Fälle wurden Tastatureingaben gesendet.

Der frühere vollständige PASS bleibt ein gültiger Laufbeleg für den getesteten Build auf Commit `73b65db`; der aktuelle FAIL ist als abweichender Lauf auf HEAD archiviert und ersetzt ihn nicht. Die Windows-Tastaturfälle sind mit dem aktuellen Desktop-/Eingabezustand nicht erneut nachgewiesen. Das Manifest und alle erzeugten Rohprotokolle bleiben erhalten.

## Separater Computer-Use-Diagnoseversuch

Am 05.10.2026 wurde auf Commit `142794d` eine frische, isolierte In-Process-Testinstanz mit eigenem temporärem Projektpfad gestartet. `sky.list_apps()` lieferte genau ein WorldDB-Appobjekt mit einem Primär- und einem Sekundärfenster; das Primärfenster wurde anhand der zurückgegebenen ID und des Titels `WorldDB ODE-002 Primary` ausgewählt. `sky.activate_window()` schlug fehl mit `failed to activate captured window`. Nach erneutem `list_apps()` und einem frischen `get_window()` schlug der einmalige Wiederholungsversuch mit derselben Meldung fehl. Es wurden keine Klicks oder Tastenanschläge gesendet; der isolierte Testprozess wurde beendet. Dieser Diagnoseversuch ist kein zusätzlicher E2E-Fall und ändert den Suite-Stand von 7/11 nicht. Die native Eingabeprüfung bleibt offen.

Auf HEAD `8144905` funktionierte ein weiterer Screenshot-first-Aufruf von `get_window_state()` für eine isolierte In-Process-Instanz. Der Task-Manager lag sichtbar über dem WorldDB-Primärfenster. Ein Klick auf die sichtbare WorldDB-Titelleiste, `Raise` und `Tab` ergaben keine sichtbare Zustandsänderung; nach erneuter Aufnahme war kein WorldDB-Fokuselement gemeldet. Das Aktivieren des Task-Managers gelang, aber Minimize-Klick und `Alt+Space` änderten dessen sichtbaren Zustand ebenfalls nicht. Es wurde kein Projekt angelegt. Die Testinstanz läuft isoliert weiter, damit das Primärfenster manuell in den Vordergrund gebracht werden kann. Dies ist weiterhin nur Diagnostik, kein E2E-Nachweis; zur Fortsetzung ist ein vom Windows-Desktop angenommener Eingabefokus erforderlich.

## Manueller CUA-Tastaturversuch

Am 05.10.2026 wurde auf HEAD `82329e8` eine frische In-Process-Instanz mit dem Projektpfad `KeyboardSuiteProject` im isolierten Temp-Verzeichnis geprüft. Das frisch erfasste Primärfenster nahm den per Tastatur eingegebenen Namen an; nach `Tab` und `Enter` zeigte die App `Projekt wurde angelegt.` und `Stand: 1`. Die Datenbankmarker `CURRENT`, `DATABASE_ID`, `FORMAT` und `LOCK` sowie die erwarteten Speicherverzeichnisse waren vorhanden. Eine separate read-only Recovery-Prüfung bestätigte `safe_revision=1`, `disposition=Clean`, null Findings und `source_modified=false`. Das isolierte App-Fenster wurde danach geschlossen. Die Details stehen in `experiments/ode-002/evidence/native-e2e/m8-26a-manual-results-20261005/cua-probe-82329e8.json`.

Dieser manuelle Versuch bestätigt die Tastatureingabe und Projekterstellung im In-Process-Profil. Der CUA-Fokusbericht blieb nach `Tab` auf dem WebArea; deshalb ist das genaue Tab-Ziel durch diesen Zusatzversuch nicht unabhängig belegt und er ersetzt keinen offiziellen E2E-Fall.

Der automatisierte Keyboard-Smoke wurde inzwischen so geändert, dass er die UIA-Fokusmethode für das Eingabefeld und native `Tab`/`Enter`-Eingaben verwendet; die Cursorpositionierung und Mausklicks entfallen. Die PowerShell-Syntaxanalyse und `git diff --check` bestanden. Der vollständige native Windows-Lauf mit diesem Runner ist noch auszuführen; M8-26a bleibt bis zu dessen Ergebnis offen.

## Vollständiger Lauf mit UIA-Fokus-Runner

Das Manifest `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T200619Z-f2edcd1b/manifest.json` meldet `PASS`: 11/11 Fälle, 140 Prüfschritte, null Fehler und kein übersprungener Fall. Beide IPC-Profile bestanden je 56 Prüfungen; die Writer-Locks bestanden mit 4 In-Process- und 5 Sidecar-Prüfungen. Keyboard-Navigation sowie Commit-Crash und read-only Recovery bestanden in beiden Modi. Der Runner verwendet für den Eingabefokus UI Automation und sendet Tab/Enter nativ, ohne Cursorpositionierung.

Der Lauf erfasste Commit `82329e8` mit `dirty=true`, weil Runner, Doku und Register noch uncommittet waren. Das Manifest entsprach dem JSON-Schema; alle elf gehashten Artefakte stimmten bei erneutem SHA-256-Abgleich.

## Saubere Wiederholung

Die Wiederholung `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T202012Z-69fd53ff/manifest.json` erfasste den sauberen Commit `c23af94e5ad5016beef2d0f9109536e78b171d45` mit `dirty=false`. Sie endete mit `PASS`: 11/11 Fälle, 140 Prüfschritte, keine Fehler und kein übersprungener Fall. Das JSON-Schema ist gültig; alle elf gehashten Artefakte stimmen bei erneutem SHA-256-Abgleich. Texteingabe, Tab/Enter-Erstellung sowie WAL-Crash und read-only Recovery bestanden in In-Process und Sidecar.

Zusätzlich bestand `cargo xtask verify` mit 39 Schritten, einem vorgesehenen `ci-matrix`-Skip und null Fehlern. Der Plancheck, Sourcecheck, PowerShell-Parser und `git diff --check` bestanden ebenfalls.

## Status und Plattformgrenze

Der Windows/NTFS-Profillauf ist auf dem sauberen Commit `c23af94` bestanden. Die Wiederholung mit dem gemeinsamen Szenario-/Treiberkatalog auf sauberem Commit `5536b0b2bb71be5784795bd19891a9d9123a0e71` ist unter `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T210756Z-d1c7f167/manifest.json` archiviert und endete mit `FAIL`: 10/11 Fälle bestanden; nur der In-Process-IPC-Fall überschritt beim Warten auf den abschließenden Branch/Layer-Ablauf das 20-Sekunden-Zeitfenster. Ein gezielter In-Process-IPC-Wiederholungslauf bestand alle 56 Prüfpunkte, nachdem genau dieses begrenzte Zeitfenster auf 60 Sekunden erhöht wurde. Die vollständige saubere Wiederholung `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T214149Z-8fe0f706/manifest.json` besteht auf Commit `f0655830971602899ec8f4bbb27b5fb36130f231` mit `dirty=false`: 11/11 Fälle, 140 Prüfpunkte, null Fehler und null ausgelassene Fälle. Schema, SHA-256 und Bytezahl der elf Artefakte stimmen. Der Registerpunkt bleibt bis zur Erfüllung der übergeordneten M8-26-Abhängigkeit offen. Die nativen APFS- und ext4-Läufe sowie ihre Startbarkeit bleiben gemäß Nutzervorgabe bis M9-07 zurückgestellt; M8-26d und das M8-Gate bleiben daher offen.
