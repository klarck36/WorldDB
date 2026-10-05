# M8-26 – Native E2E-Suite

## Korpus und Belegformat

`experiments/ode-002/e2e/native-suite.json` definiert die gemeinsamen Fall-IDs für Desktopabläufe, getrennte Fenster, konkurrierende Prozesse, Renderer-Manipulation, Recovery, Tastaturbedienung und einen Abbruch am Commitpunkt. Der Windows-Einstieg ist `experiments/ode-002/scripts/run-native-e2e.ps1`. Er baut In-Process- und Sidecar-Binärdateien getrennt und führt beide Profile auf dem echten Tauri-Desktop aus.

Jeder Lauf erhält eine eindeutige Run-ID und ein `manifest.json` nach `docs/schemas/native-e2e-evidence.schema.json`. Das Manifest hält Commit und Dirty-Zustand, OS-Version, Dateisystem, Toolchain, SHA-256-Hashes der Builds, Fallresultate und gehashte Ausgabedateien fest. Die nativen Smoke-Skripte archivieren ihre temporären Projektdaten und Prozesslogs im Run-Ordner. Fehler behalten dadurch die Eingaben und Logs, die zur Reproduktion benötigt werden.

## Windows-Fälle

- IPC-Kernabläufe umfassen zwei echte Tauri-Fenster, Projekt-/Schema-/Faktenabläufe, Rendererpfadmanipulation sowie read-only Recovery und ausdrücklich gestartete Recovery.
- Der Prozessfall weist einen zweiten Schreiber ab, öffnet nach sauberem Shutdown erneut und prüft beim Sidecar das Beenden des Engine-Kinds.
- Die Keyboard-Suite zeigt die nativen Fenster nur in Debug-Builds, prüft Prozessbaum und Vordergrundfenster vor jeder Tastatureingabe und isoliert Datenbank sowie WebView2-Profil pro Lauf. Sie schreibt den Projektnamen mit Tastatureingaben, navigiert mit Tab zum Erstellen-Knopf und löst ihn mit Enter aus. Das Testprojekt liegt unter einem eindeutigen temporären Verzeichnis.
- Der Crashfall verwendet denselben nativen Erstellen-Ablauf und einen Debug-only-Failpoint direkt nach dem `sync_all` des WAL-Commitmarkers. Die In-Process-App oder der Sidecar-Engineprozess muss sich mit Exitcode 86 beenden. Danach prüft `worlddb-cli v1 recovery inspect` den gespeicherten sicheren Präfix ohne Reparatur.

## Windows-Zwischenstand

Der erste vollständige Windows-Lauf steht in `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T012844Z-823933db/manifest.json`. Builds, IPC-Smokes (56/56 je Modus) und konkurrierende Schreiber (4 In-Process, 5 Sidecar) bestehen. Tastatur- und Commit-Crashfälle bestehen noch nicht: Der UI-Automation-Fokus erreichte das Eingabefeld nicht, daher wurden diese Fälle im Voll-Lauf als `FAIL` erfasst.

Folgeprüfungen haben den WebView2-Eingabeprozess dem gestarteten Desktopprozess zugeordnet, das Primärfenster erst nach bestätigtem Vordergrund aktiviert und offscreen liegende Bedienelemente über `ScrollItemPattern` sichtbar gemacht. Die simulierte Eingabe hält im Accessibility-Wertmuster weiterhin nur das jeweils letzte Zeichen; Tab lässt den Fokus auf dem Eingabefeld. Ein nativer Erstellenversuch erreichte keinen WAL-Commitmarker; die anschließende read-only Recovery-Prüfung meldete Revision 0 und `Clean`. Der Storage-Crash-Failpoint selbst besteht separat mit einem Test. Diese Teilnachweise ersetzen die fehlenden nativen Desktopfälle nicht.

`M8-26a` bleibt offen, bis beide Windows-Modi für Keyboard- und Crash-Recovery-Fälle belegt sind. `docs/M8-26a-windows-progress.md` enthält den Detailstand. Linux/macOS bleiben gemäß Projektvorgabe bis M9-07 zurückgestellt.

## Plattformstatus

Windows/NTFS ist der lokal ausführbare Profil. macOS/APFS und Linux/ext4 bleiben gemäß Projektvorgabe bis M9-07 zurückgestellt; dieser Rechner kann deren native Läufe nicht belegen. Ein vollständiger M8-26-Abschluss und das M8-Gate dürfen daher erst nach den fehlenden Plattformläufen und der gemeinsamen Abnahme erfolgen.

## Ausführung

Im Repository-Root:

```powershell
.\experiments\ode-002\scripts\run-native-e2e.ps1
```

Ein Lauf speichert seine Belege unter `experiments/ode-002/evidence/native-e2e/<run-id>/`. `PASS` in einem M8-26a-Manifest bedeutet, dass alle Windows-Fälle bestanden sind. Ein vollständiger M8-26-Abschluss erfordert zusätzlich eigenständige APFS- und ext4-Manifeste. `INCOMPLETE` bedeutet, dass mindestens ein Fall im aktuellen Profil noch nicht automatisiert oder ausgeführt wurde; `FAIL` bedeutet, dass ein ausgeführter Fall oder ein erforderlicher Build fehlgeschlagen ist. Rohdaten aus fehlgeschlagenen Fällen bleiben lokal und sind von Git ausgeschlossen.
