# M8-13 — Perspektivenansicht und Kontextbindung

**Stand:** 3. Oktober 2026

**Ergebnis:** Abgeschlossen auf Windows; Linux/macOS-Nachweise bleiben wie vereinbart M9-07 zugeordnet.

## Umsetzung

Der bestehende Vertrag in `docs/contract/entity_perspective_contract.md` ist über die Desktopanwendung bedienbar. Der Projektkatalog zeigt aktuelle, historische und explizite Revisionen. Perspektiven können angelegt, mit append-only Metadaten aktualisiert und stillgelegt werden; historische Definitionen bleiben lesbar. Katalogänderungen laufen durch die authentisierte Engine und die bestehende gemeinsame Metadaten-/WAL-/Audit-Publikation.

Eingabe- und Abfragekontext haben getrennte Auswahlfelder und Zustände. `WorldState` wird stets ohne Perspektive geprüft. `Knows`, `Believes` und `Claims` verlangen eine ausdrücklich gewählte aktive Perspektive. Es gibt keine automatische Auswahl; stillgelegte Perspektiven können keinen neuen Kontext binden. Die Oberfläche bezeichnet Perspektiven als Wissensstände, nie als Benutzerrollen, und erklärt: „Fehlendes Wissen ist Unbekannt, nicht falsch.“ Die Auswahl bereitet spätere Eingabe- und Abfragefunktionen vor und erzeugt selbst keine Aussagen oder Abfrageergebnisse.

## Prüfergebnisse

- Perspektiven-Engine-Tests: 2 bestanden, einschließlich CRUD, Revisionstreue, Retirement und ungültiger Kontextpaare.
- Root-Workspace: `cargo test --workspace --all-targets` bestanden.
- ODE-002: `cargo test --workspace --all-targets` bestanden (In-Process) und mit `--no-default-features --features sidecar` bestanden.
- Striktes Clippy mit `-D warnings`: Root-Workspace, ODE-002 In-Process und ODE-002 Sidecar bestanden.
- Echte native Tauri-IPC-Smokes: In-Process und Sidecar bestanden. Beide Läufe prüften zwei authentisierte Fenster, aktuelle/historische/explizite Perspektivenansichten, Erstellen/Ändern/Stilllegen, korrekte und abgelehnte Kontextbindungen, getrennte Eingabe-/Abfrageauswahl, ungültige Sitzungen und Pfade, fehlende Netzwerklistener sowie sauberes Herunterfahren.
- `node --check` für die Desktopoberfläche und PowerShell-Parserprüfung des IPC-Smokes bestanden.
- `cargo fmt` für beide Workspaces und `git diff --check` bestanden.

`cargo xtask verify` bestanden: 39 PASS, ein vorgesehener M0-14-`ci-matrix`-SKIP, 0 FAIL. Der Lauf umfasst unter anderem Plancheck, Sourcecheck, Ausnahme- und Formatierungsprüfungen.
