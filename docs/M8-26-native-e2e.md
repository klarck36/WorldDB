# M8-26 – Native E2E-Suite

## Korpus und Belegformat

`experiments/ode-002/e2e/native-suite.json` definiert die gemeinsamen Fall-IDs für Desktopabläufe, getrennte Fenster, konkurrierende Prozesse, Renderer-Manipulation, Recovery, Tastaturbedienung und einen Abbruch am Commitpunkt. Der Windows-Einstieg ist `experiments/ode-002/scripts/run-native-e2e.ps1`. Er baut In-Process- und Sidecar-Binärdateien getrennt und führt beide Profile auf dem echten Tauri-Desktop aus.

Jeder Fall benennt ein plattformneutrales `scenario` und getrennte `drivers` für Windows, macOS und Linux. Der Windows-Einstieg löst Fall-ID, Prozessmodus und Treiber aus diesem Katalog auf, statt die PowerShell-Treiber zusätzlich fest zu verdrahten. `tools/check_native_e2e_suite.py` prüft eindeutige IDs, vollständige Szenario-/Prozessmodusabdeckung, OS-/Dateisystembindungen, sichere Treiberpfade sowie Übereinstimmung zwischen verfügbaren Plattformen und vorhandenen Treibern. Neun Mutationsprüfungen in `tools/test_native_e2e_suite.py` schützen diese Regeln; beide Prüfungen laufen in `cargo xtask verify`. Der Gesamtverifier bestand auf Windows mit 41 Schritten, einem vorgesehenen Skip und null Fehlern.

Für Windows/NTFS sind die nativen Treiber verfügbar. macOS/APFS und Linux/ext4 haben im gemeinsamen Katalog weiterhin `state: deferred`, keinen Suite-Einstieg und keine Falltreiber. Das ist die vereinbarte Verschiebung der nativen Ausführung und Belege bis M9-07; die portablen Szenariodefinitionen bleiben bereits jetzt gemeinsam.

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

Eine Wiederholung auf dem aktuellen Branch (`01ab573a620ade99f96ced7e46249734a2283c36`) ist im Manifest `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T154837Z-2b012994/manifest.json` als `FAIL` archiviert: 7/11 Fälle bestanden. Builds, IPC und Writer-Locks bestanden; Tastatur- und Commit-Recovery-Fälle brachen vor der Eingabe ab, weil `SetCursorPos` im aktuellen Windows-Eingabezustand `false` zurückgab. Die isolierte Wiederholung konnte denselben Win32-Aufruf sogar an der schon aktuellen Cursorposition nicht ausführen. Das frühere 11/11-Ergebnis bleibt ein Nachweis für seinen Build; der neue Lauf belegt keine Regression im WorldDB-Ablauf, lässt aber die Tastaturfälle auf dem aktuellen Host unbestätigt.

Ein manueller In-Process-Probeversuch auf HEAD `82329e8` bestätigte danach native Tastatureingabe und Enter-Erstellung eines isolierten Projekts. Die read-only Recovery-Prüfung meldete Revision 1 als `Clean`, ohne Findings und ohne Änderung der Quelle. Weil der CUA-Fokusbericht nach `Tab` weiterhin das WebArea meldete, ist dieser Versuch nur ein Zusatzbeleg und bestätigt nicht unabhängig das Tab-Ziel. Er ist dokumentiert unter `experiments/ode-002/evidence/native-e2e/m8-26a-manual-results-20261005/cua-probe-82329e8.json`.

Der Keyboard-Smoke umgeht die Cursor- und Mausklickpfade jetzt über UIA-Fokus des Eingabefeldes sowie native `Tab`-/`Enter`-Tasten. Der vollständige Lauf `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T200619Z-f2edcd1b/manifest.json` auf Commit `82329e8` meldet `PASS`: 11/11 Fälle, 140 Prüfschritte, null Fehler, null übersprungene Fälle. In-Process und Sidecar bestanden jeweils Keyboard-Navigation und WAL-Commit-Crash mit read-only Recovery. Dieser erste Lauf markiert den Arbeitsbaum als geändert.

Die saubere Wiederholung `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T202012Z-69fd53ff/manifest.json` meldet auf Commit `c23af94e5ad5016beef2d0f9109536e78b171d45` mit `dirty=false` ebenfalls `PASS`: 11/11 Fälle, 140 Prüfschritte, null Fehler und null übersprungene Fälle. Schema und SHA-256-Artefakte wurden geprüft. `cargo xtask verify` bestand mit 39 Schritten, einem vorgesehenen Skip und null Fehlern. M8-26a ist auf Windows/NTFS damit belegt; die übergeordnete M8-26-Aufgabe bleibt wegen der bis M9-07 verschobenen macOS-/Linux-Profile und Runner offen.

Nach der Umstellung auf den gemeinsamen Szenario-/Treiberkatalog bestand `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T204554Z-dbbf7db0/manifest.json` auf Windows 11/NTFS erneut mit `PASS`: 11/11 Fälle, 140 Prüfungen, null Fehler und null nicht ausgeführte Fälle. Der Lauf verwendete den Windows-Treiber aus der jeweiligen `drivers.windows`-Angabe und war wegen der noch uncommitteten Katalogänderung `dirty=true`. Das Manifest entspricht `docs/schemas/native-e2e-evidence.schema.json`; alle 11 aufgeführten Artefakte stimmten bei SHA-256 und Bytezahl. Eine saubere Wiederholung auf dem Commit mit dem neuen Katalog folgt.

Der Windows/NTFS-Profillauf ist auf sauberem Commit bestanden. Die nativen APFS- und ext4-Läufe sowie ihre Startbarkeit bleiben gemäß Nutzervorgabe bis M9-07 zurückgestellt. M8-26 und die gemeinsame Abnahme M8-26d bleiben bis zur Umsetzung dieser Profile offen.

## Plattformstatus

Windows/NTFS ist lokal ausgeführt und bestanden, einschließlich des aktuellen Runnerlaufs aus dem gemeinsamen Katalog. macOS/APFS und Linux/ext4 bleiben gemäß Projektvorgabe bis M9-07 zurückgestellt; dieser Rechner kann deren native Läufe nicht belegen. M8-26 und das M8-Gate bleiben bis zur Umsetzung und Ausführung der fehlenden Plattformtreiber sowie der gemeinsamen Abnahme offen.

## Ausführung

Im Repository-Root:

```powershell
.\experiments\ode-002\scripts\run-native-e2e.ps1
```

Ein Lauf speichert seine Belege unter `experiments/ode-002/evidence/native-e2e/<run-id>/`. `PASS` in einem M8-26a-Manifest bedeutet, dass alle Windows-Fälle bestanden sind. Ein vollständiger M8-26-Abschluss erfordert zusätzlich eigenständige APFS- und ext4-Manifeste. `INCOMPLETE` bedeutet, dass mindestens ein Fall im aktuellen Profil noch nicht automatisiert oder ausgeführt wurde; `FAIL` bedeutet, dass ein ausgeführter Fall oder ein erforderlicher Build fehlgeschlagen ist. Rohdaten aus fehlgeschlagenen Fällen bleiben lokal und sind von Git ausgeschlossen.
