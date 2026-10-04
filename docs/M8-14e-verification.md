# M8-14e – Erfassungsoberfläche für Assertions, Masks und ReplacementBoundaries

Stand: 4. Oktober 2026

## Ergebnis

Die ODE-Desktopoberfläche erfasst Assertions, Masks und ReplacementBoundaries mit geschlossen validierten Eingaben. Assertions binden den gewählten Kontext, WorldTime, Polarity, Layer und Predicate. Masks bieten die unterstützten exakten, Proposition- und Slot-Selectoren. ReplacementBoundaries erfassen die MultiValue-Grenze. Die Writes laufen über die persistente, autorisierte Storage-API.

Die Auflösungsvorschau bleibt davon getrennt: Sie kann einen konkreten Zeitpunkt oder alle Zeiten abfragen und zeigt `Known`, `Unknown`, `Conflict` und `CompleteEmpty` als unterschiedliche Ergebnisse an. Die UI aktualisiert den Katalog anhand der aktuellen Projektrevision, sodass aufeinanderfolgende Writes und Vorschauen denselben Projektstand verwenden.

## Nachweise

- Der native Windows-IPC-Smoke bestand im In-Process- und Sidecar-Modus. Er legte eine Assertion an und prüfte Punkt- und All-Times-Vorschau, exakte/Proposition/Slot-Masks sowie eine ReplacementBoundary.
- `worlddb-ode-engine`: 23 Unit-Tests und der Sidecar-Transporttest bestanden.
- `worlddb-storage-file`: 103 Tests bestanden.
- Striktes Clippy bestand für den Root-Workspace, das ODE-Workspace-Standardprofil und das ODE-Sidecar-Profil.
- `cargo xtask verify` bestand auf Windows: 39 PASS, ein erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- Rust-Formatprüfung in beiden Workspaces, Node-Syntaxprüfung, PowerShell-Parser, Plancheck, Sourcecheck und `git diff --check HEAD` bestanden.
- Linux- und macOS-Nachweise bleiben vereinbarungsgemäß bis M9-07 zurückgestellt.
