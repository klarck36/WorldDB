# M8-16 – Quellen, Belege und Provenienz erfassen

Stand: 4. Oktober 2026

## Ergebnis

Sources sind unveränderliche, typisierte Projekt-Metadatenrecords. Eine Ersetzung erzeugt den neuen Source und seine explizite `DerivedFrom`-Lineage gemeinsam in einer Transaktion; der alte Source bleibt erhalten. Evidence bindet einen Source an ein geschlossenes, autorisiertes Ziel. Provenance-Relationen verwenden ebenfalls geschlossene Endpunktfamilien und die validierten Relationstypen.

Evidence und Provenance werden unabhängig vom Zielrecord explizit zurückgenommen. Persistente Writes validieren vor dem Commit aktuelle Source-Feld-, Record-, Endpunkt- und Beziehungsrechte, die typbezogenen Zielregeln sowie den gemeinsamen Provenance-Graphen. Unzulässige Ziele und Zyklen werden ohne Revisionserhöhung abgewiesen. Snapshots und Desktop-Auswahllisten enthalten nur vollständig lesbare Sources und Endpunkte.

Die Desktopoberfläche bietet Source-Felder, Source-Ersetzung mit Lineage, autorisierte Evidence-/Provenance-Endpunkte und getrennte Rücknahmeaktionen. Die Versionierung des IPC-Vertrags bleibt geschlossen.

## Nachweise

- Vollständige Storage-Suite: **106 bestanden, 1 erwartungsgemäß ignoriert, 0 fehlgeschlagen**; alle zusätzlichen Storage-Testziele bestanden. Der gezielte M8-16-Regressionsfall deckt atomare Writes, falsche Relationstypen, gemischte Graphzyklen, Wiederöffnung, Rechteentzug und das Verbergen nicht autorisierter Endpunkte ab.
- ODE-002 In-Process-Profil: **15 Desktop-, 23 Engine- und 1 Sidecar-Transporttest bestanden**.
- ODE-002 Sidecar-Profil: **16 Desktop-, 23 Engine- und 1 Sidecar-Transporttest bestanden**.
- Native Windows-IPC-Smokes bestanden in-process und sidecar. Sie führen Source-Erstellung und -Ersetzung, Evidence, Provenance, getrennte Rücknahmen, Fenster-/Principal-Autorisierung, geschlossene Endpunktlisten und die bestehenden Fakten-/Event-Negativfälle über die echte Desktopoberfläche aus. Beide Prozesse schließen mit Exitcode 0 und ohne Netzwerk-Listener.
- Striktes Clippy mit `-D warnings` bestanden für Root-Workspace, ODE-002 In-Process und ODE-002 Sidecar.
- `cargo xtask verify`: **39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**. Plancheck, Sourcecheck, Rust-Formatierung, Node-Syntax, PowerShell-Parser, Ausnahme-Policy und `git diff --check` bestanden.
- Linux- und macOS-Prüfungen bleiben gemäß Vorgabe bis M9-07 zurückgestellt.
