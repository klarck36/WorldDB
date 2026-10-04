# M8-19 – Commitkonflikte und unklare Ausgänge

Stand: 4. Oktober 2026

## Ergebnis

Jede verwaltete Änderung erhält eine stabile OperationId, die vom Desktop durch den In-Process- oder Sidecar-Aufruf bis zum dauerhaften WAL-Commit weitergereicht wird. Der Commitstatus kann danach mit derselben ID als committed, not committed oder indeterminate abgefragt werden. Sichere Konfliktberichte nennen nur die erlaubten Konfliktfakten sowie erwartete und aktuelle Revision; die Oberfläche zeigt bei einem Versionskonflikt ausdrücklich, dass nichts gespeichert wurde.

Der lokale Schreibstatus wird je Datenbank und OperationId dauerhaft und fensterübergreifend vorgemerkt. Ein beschädigtes oder nicht verfügbares Journal sperrt weitere Änderungen fail-closed. Unbekannte Ausgänge bleiben vorgemerkt und werden beim erneuten Abgleich mit dem WAL aufgelöst; bestätigte Commits oder bestätigte Nicht-Commits geben die Schreiboberfläche frei. Projekt-Bootstrap verwendet ebenfalls die vom Aufrufer erzeugte OperationId. Ein unbekannter Bootstrap-Ausgang enthält OperationId und – soweit verfügbar – DatabaseId, sodass die spätere Projektöffnung denselben WAL-Eintrag prüfen kann.

## Nachweise

- Engine-Tests: **28 bestanden**; zusätzlich Sidecar-Transferintegration: **1 bestanden**.
- Desktoptests: **16 In-Process bestanden**, **17 Sidecar bestanden**. Darin enthalten ist die Prüfung, dass ein unbekannter Bootstrap-Commit OperationId und DatabaseId zurückliefert.
- Native Windows-Zwei-Fenster-Smokes: **vollständig bestanden in-process und sidecar**. Beide bestätigen den stale-Commit-Konflikt samt sicherem „nichts gespeichert“-Bericht und injizieren anschließend einen verlorenen Reply nach erfolgreichem WAL-Commit. Die Antwort wird unter derselben OperationId als committed samt Revision 32 wiedergefunden.
- Striktes Clippy (`-D warnings`): Root-Workspace, ODE-Standardprofil und ODE-Sidecarprofil bestanden. Rust-Formatprüfung und Node-/PowerShell-Syntaxprüfungen bestanden.
- `cargo xtask verify`: **39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**. Plancheck, Sourcecheck und `git diff --check HEAD` bestanden.
- Linux-/macOS-Laufzeitprüfungen bleiben wie vereinbart bis M9-07 zurückgestellt.
