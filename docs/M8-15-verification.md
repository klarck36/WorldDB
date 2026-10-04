# M8-15 – Events und Eventrelationen erfassen

Stand: 4. Oktober 2026

## Ergebnis

Events werden append-only und schema-/referenzgeprüft mit expliziten Rollen, Attributen und Weltzeit gespeichert. Instant- und Span-Zeitformen bleiben exakt; ein offener Span wird durch einen eigenen späteren Closure-Record geschlossen. EventMasks verlangen einen strikt spezifischeren HistorySpace-/Layer-Kontext und werden unabhängig vom Ziel-Event archiviert oder zurückgenommen.

Before, After, SameTime und Causes werden als explizite Relationen validiert. After wird kanonisch als umgekehrtes Before persistiert. Widersprüchliche temporale oder kausale Graphen werden vor dem Commit abgelehnt; es gibt keine automatische Inferenz aus Zeit, Überschneidung oder Kausalität. Der UI-Katalog zeigt die gespeicherten Relationen und sichere Konflikthinweise.

## Nachweise

- Die Storage-Regressionsprüfung deckt Rollen-/Attributvalidierung, Instant/Span, einmalige Span-Schließung, kanonisches After, SameTime-Konflikt, Causes-Zyklus, getrennte Relation-Retract-Belege, EventMask und StorageVerify ab. Zusätzlich prüft sie den Mischfall einer archivierten Assertion vor einer EventMask. Dieser Fall fand einen Archivhistorie-Fehler: Die EventMask-Prüfung erfasste nur Event-Ziele, obwohl dieselbe Historie bereits eine Assertion-Archivtransition enthielt. Die gemeinsame projektweite Archivhistorie behebt den Fehler. Vollständige `worlddb-storage-file`-Suite: 105 bestanden, 1 erwartungsgemäß ignoriert, 0 fehlgeschlagen; alle Integrationstests bestanden.
- ODE-Standardprofil: 15 Desktoptests, 23 Engine-Tests und 1 Sidecar-Transporttest bestanden. ODE-Sidecar-Profil: 16 Desktoptests, 23 Engine-Tests und 1 Sidecar-Transporttest bestanden.
- Native Windows-IPC-Smokes bestanden in-process und sidecar. Beide echten Fenster, Projekt-/Schema-/Entity-/Branch-/Policy-Flüsse sowie Event-Anlage, Span-Abschluss, EventMask mit separater Rücknahme, Event-Rücknahme und sichere Graphkonflikte wurden ausgeführt; keine unerwarteten Listener; geordneter Prozessschluss.
- Striktes Clippy mit `-D warnings` besteht für Root-Workspace, ODE-Standardprofil und ODE-Sidecar-Profil. ODE-Standard- und Sidecar-Build bestehen.
- `cargo xtask verify`: 39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. Plancheck, Sourcecheck, `cargo fmt --check`, Node-Syntax, PowerShell-Parser und `git diff --check HEAD` bestehen.
- Linux- und macOS-Abnahmen bleiben vereinbarungsgemäß bis M9-07 zurückgestellt.
