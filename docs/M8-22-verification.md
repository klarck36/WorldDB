# M8-22 – Migrationsoberfläche

**Status:** DONE für den lokalen Windows-Arbeitsumfang. Linux/macOS bleiben wie vereinbart bis M9-07 ausstehend.

## Ergebnis

Die Desktopoberfläche kann im geschlossenen Projektzustand einen lokalen Quellordner und einen MigrationPlan über native Dateidialoge auswählen. Quelldateien werden pro Schritt ausgewählt, größen- und arbeitsbudgetbegrenzt geprüft und in ein privates Staging-Verzeichnis kopiert. Renderer-IPC nimmt keine Dateisystempfade an. Plan, Quelle, Revisionen und Fingerprints werden über denselben `worlddb-cli`-/Core-Pfad validiert, der auch die Ausführung autorisiert.

Der Dry Run zeigt Kategorie, IDs, Revisionen, Plan-/Input-/Outputfingerprints, Transformer, Ressourcenbudget, Record- und Bytezahlen, Ausgabe-/Speicherbedarf, Fehler, Warnungen, unresolved Items sowie Contract- und Restorepoint-Status. Fehler sperren die Ausführung. Jedes unresolved Item muss einzeln ausdrücklich ausgelassen werden; die Entscheidung wird gegen exakt den aktuellen Dry-Run geprüft und vom CLI vor der Ausführung erneut validiert. Nicht auflösbare oder diagnostisch verdeckte Items bleiben fail-closed.

Eine Breaking-Migration verlangt eine separate ausdrückliche Adminbestätigung sowie Ziele für ein Exact Backup und einen Restore-Klon. Der CLI-Pfad erstellt und verifiziert den Restorepoint, bevor er die irreversible Contract-Phase ausführt. Die UI zeigt Preview (`preview_only_no_commit`), erforderlichen Restorepoint und den endgültigen Commitstatus getrennt. Run-ID und Operation-IDs bleiben bei unklarem Ausgang erhalten; Resume und Journalabgleich verhindern einen blinden zweiten Start.

## Verifikation auf Windows

- ODE Desktop-Migrationstests: 18 bestanden.
- Striktes Clippy für ODE In-Process und Sidecar bestanden.
- `cargo fmt --all -- --check`, `node.exe --check crates/desktop-shell/frontend/main.js` und `git diff --check` bestanden.
- `cargo xtask verify`: 39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL.
- Native Zwei-Fenster-IPC-Smoke In-Process bestanden, einschließlich Recovery-Inspektion, Projektbootstrap und Renderer-/Session-Sicherheitsprüfungen.
- Native Zwei-Fenster-IPC-Smoke Sidecar bestanden, einschließlich derselben Prüfungen.
- Linux/macOS wurden nicht ausgeführt; wie vereinbart sind diese Plattformprüfungen M9-07 vorbehalten.

## Grenzen

Unresolved Items werden in dieser Oberfläche ausdrücklich ausgelassen; eine neue Zielzuordnung wird hier nicht erfasst. Deshalb bleibt jeder nicht ausdrücklich ausgelassene unresolved Eintrag ein Ausführungsblocker. Backup-Auswahl und allgemeiner Restoreablauf folgen in M8-23a.
