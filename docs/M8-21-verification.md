# M8-21 – Recovery-Oberfläche

**Status:** DONE für den lokalen Windows-Arbeitsumfang. Linux/macOS bleiben wie vereinbart bis M9-07 ausstehend.

## Ergebnis

Die Desktop-Oberfläche bietet im geschlossenen Projektzustand einen eigenen Recovery-Einstieg. Sie zeigt die sichere Revision als exakten Dezimaltext, den validierten Speicherbestand, Manifestzustand, klassifizierte Befunde und deren zulässige Folgeaktionen. Eine unabhängige Verify-Prüfung öffnet die Quelle read-only. Das normale Öffnen bleibt fail-closed und repariert kein Recovery-bedürftiges Projekt.

Die Aktionen sind getrennt:

- **Read-only beibehalten:** Die Quelle bleibt unverändert.
- **Journalisierte Recovery:** Nur für vom Verify-Bericht zugelassene Recovery; erfordert eine ausdrückliche Bestätigung, prüft unter exklusivem Lock erneut und verifiziert anschließend den Zustand.
- **Restore:** Als Wiederherstellung in ein neues Ziel ausgewiesen. Die Auswahl und Ausführung des verifizierten Backups liegen im separaten Backup-/Restore-Task M8-23a; der Recovery-Button verändert die Quelle nicht.
- **Salvage:** Erstellt ein markiertes Archiv in einem neuen Ziel und lässt die Quellbytes unverändert.
- **Sauberen Stand öffnen:** Wird nur bei sauberem Verify-Bericht angeboten.

Renderer-DTOs enthalten keine frei wählbaren Pfade oder Principal-Identitäten. Dateisystemdialoge bleiben hostseitig; Revisions- und Zählerwerte werden ohne JavaScript-Zahlverlust übertragen. Die Recovery-Sidecar-Route öffnet keinen normalen schreibenden Engine-Host.

## Verifikation auf Windows

- ODE In-Process: 17 Desktop-Tests, 34 Engine-Tests und 1 Sidecar-Transporttest bestanden. Darin enthalten sind unveränderte read-only Diagnose, explizite Tail-Recovery, quellenbewahrendes Salvage, geschlossene Recovery-DTOs und verpflichtende Bestätigung.
- ODE Sidecar: 18 Desktop-Tests, 34 Engine-Tests und 1 Sidecar-Transporttest bestanden.
- Native Zwei-Fenster-IPC-Smoke In-Process bestanden. Der Smoke endet erst nach sauberem Projekt-Shutdown und erfolgreicher read-only Recovery-Inspektion.
- Native Zwei-Fenster-IPC-Smoke Sidecar bestanden, einschließlich derselben Recovery-Inspektion.
- Striktes Clippy für beide ODE-Profile bestanden.
- `cargo xtask verify`: 39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL. Der Skip ist die nicht auf diesem Einzelrechner ausführbare CI-Matrix-Abnahme.
- Plancheck, Sourcecheck, `cargo fmt --all -- --check`, `node --check` für `frontend/main.js`, PowerShell-Parserprüfung des Smoke-Skripts und `git diff --check` bestanden.

Die nativen Windows-Smokes wurden mit `scripts/run-ipc-security-smoke.ps1` im Modus `in-process` und anschließend `sidecar` ausgeführt. Linux/macOS wurden nicht ausgeführt, entsprechend der Vereinbarung, diese Plattformprüfungen bis M9-07 aufzuschieben.

## Grenzen

M8-21 stellt den Restore als klar getrennte, nicht überschreibende Folgeaktion dar. Auswahl, Profilprüfung, Zielauswahl und erfolgreicher Restore eines verifizierten Backups werden in M8-23a umgesetzt. Das ist ausdrücklich kein stiller oder automatischer Restore.
