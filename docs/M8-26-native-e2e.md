# M8-26 – Native E2E-Suite

## Korpus und Belegformat

`experiments/ode-002/e2e/native-suite.json` definiert die gemeinsamen Fall-IDs für Desktopabläufe, getrennte Fenster, konkurrierende Prozesse, Renderer-Manipulation, Recovery, Tastaturbedienung und einen Abbruch am Commitpunkt. Der Windows-Einstieg ist `experiments/ode-002/scripts/run-native-e2e.ps1`. Er baut In-Process- und Sidecar-Binärdateien getrennt und führt beide Profile auf dem echten Tauri-Desktop aus.

Jeder Lauf erhält eine eindeutige Run-ID und ein `manifest.json` nach `docs/schemas/native-e2e-evidence.schema.json`. Das Manifest hält Commit und Dirty-Zustand, OS-Version, Dateisystem, Toolchain, SHA-256-Hashes der Builds, Fallresultate und gehashte Ausgabedateien fest. Die nativen Smoke-Skripte archivieren ihre temporären Projektdaten und Prozesslogs im Run-Ordner. Fehler behalten dadurch die Eingaben und Logs, die zur Reproduktion benötigt werden.

## Windows-Fälle

- IPC-Kernabläufe umfassen zwei echte Tauri-Fenster, Projekt-/Schema-/Faktenabläufe, Rendererpfadmanipulation sowie read-only Recovery und ausdrücklich gestartete Recovery.
- Der Prozessfall weist einen zweiten Schreiber ab, öffnet nach sauberem Shutdown erneut und prüft beim Sidecar das Beenden des Engine-Kinds.
- Die Keyboard-Suite zeigt die nativen Fenster nur in Debug-Builds, prüft Prozessbaum und Vordergrundfenster vor jeder Tastatureingabe und isoliert Datenbank sowie WebView2-Profil pro Lauf. Sie schreibt den Projektnamen mit Tastatureingaben, navigiert mit Tab zum Erstellen-Knopf und löst ihn mit Enter aus. Das Testprojekt liegt unter einem eindeutigen temporären Verzeichnis.
- Der Crashfall verwendet denselben nativen Erstellen-Ablauf und einen Debug-only-Failpoint direkt nach dem `sync_all` des WAL-Commitmarkers. Die In-Process-App oder der Sidecar-Engineprozess muss sich mit Exitcode 86 beenden. Danach prüft `worlddb-cli v1 recovery inspect` den gespeicherten sicheren Präfix ohne Reparatur.

## Windows-Ergebnis

Der erste vollständige Windows-Lauf steht in `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T012844Z-823933db/manifest.json`. Builds, IPC-Smokes und Writer-Locks bestanden; die Tastaturfälle scheiterten damals, bevor Eingabe oder Commit-Failpoint erreicht wurden.

Nach den Korrekturen bestanden Tastatureingabe, Tab/Enter und Commit-Crash samt read-only Recovery. Ein Zwischenlauf scheiterte nur beim Löschen noch gesperrter temporärer WebView2-Profile. Der Keyboard-Smoke wartet nun begrenzt auf die Profilfreigabe und archiviert den flüchtigen WebView2-Cache bei Fehlern nicht.

Der vollständige Wiederholungslauf `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T140119Z-29c16683/manifest.json` meldet auf Windows 11/NTFS `PASS`: 11/11 Fälle, 138 Prüfschritte, null Fehler. Beide IPC-Profile, konkurrierende Schreiber, Tastaturabläufe und Crash-Recovery-Fälle bestanden. `docs/M8-26a-windows-progress.md` enthält die Einzelergebnisse und den historischen Verlauf.

Der Windows-Lauf ist damit belegt. Die nativen APFS- und ext4-Läufe sowie ihre Startbarkeit bleiben gemäß Nutzervorgabe bis M9-07 zurückgestellt. M8-26 und die gemeinsame Abnahme M8-26d bleiben bis zur Umsetzung dieser Profile offen.

## Plattformstatus

Windows/NTFS ist lokal ausgeführt und bestanden. macOS/APFS und Linux/ext4 bleiben gemäß Projektvorgabe bis M9-07 zurückgestellt; dieser Rechner kann deren native Läufe nicht belegen. M8-26 und das M8-Gate bleiben bis zu den fehlenden Plattformläufen und der gemeinsamen Abnahme offen.

## Ausführung

Im Repository-Root:

```powershell
.\experiments\ode-002\scripts\run-native-e2e.ps1
```

Ein Lauf speichert seine Belege unter `experiments/ode-002/evidence/native-e2e/<run-id>/`. `PASS` in einem M8-26a-Manifest bedeutet, dass alle Windows-Fälle bestanden sind. Ein vollständiger M8-26-Abschluss erfordert zusätzlich eigenständige APFS- und ext4-Manifeste. `INCOMPLETE` bedeutet, dass mindestens ein Fall im aktuellen Profil noch nicht automatisiert oder ausgeführt wurde; `FAIL` bedeutet, dass ein ausgeführter Fall oder ein erforderlicher Build fehlgeschlagen ist. Rohdaten aus fehlgeschlagenen Fällen bleiben lokal und sind von Git ausgeschlossen.
