# M8-17 – Historie, Resolved und Explain anzeigen

Stand: 4. Oktober 2026

## Ergebnis

Die Desktop-Abfrage bindet RecordedAsOf, SchemaMode, QueryMode, HistorySpace, Layerauswahl, Perspective, epistemischen Modus und WorldTime ausdrücklich. SchemaMode unterstützt Historical, Current und Explicit. Ein Historical-Schema muss exakt an RecordedAsOf gebunden sein; Revisionen bleiben bis in den IPC-DTO kanonische Dezimalzeichenfolgen, damit JavaScript große Werte nicht verändert.

History, Resolved und Explain laufen über dieselbe autorisierte Query-Ausführung. Die Abfrageantwort trennt den live gelesenen Snapshot von RecordedAsOf und zeigt alle gebundenen Kontextachsen. Historische Fakten- und WAL-Daten werden an derselben ausgewählten Revision gelesen.

Raw History stellt die gespeicherten Assertion-, Assertion-Retraction-, Mask-, Mask-Retraction-, ReplacementBoundary- und Boundary-Retraction-Inhalte bereit. Explain zeigt Kandidaten und Contributors sowie tatsächlich angewandte Masks und ReplacementBoundaries. Resolved zeigt punktuelle und vollständige All-Times-Ergebnisse einschließlich explizit leerer Ergebnisse.

## Nachweise

- Storage-Regression `query_slot_binds_recorded_revision_and_explains_applied_mask_and_boundary`: **1 bestanden**. Sie deckt ältere RecordedAsOf-Werte bei neuerem Live-Snapshot, Rohdatensatzinhalte, Explain-Beiträge, Historical/Current/Explicit SchemaMode und Rechteverweigerung ab.
- ODE-002 In-Process-Profil: **15 Desktop-, 25 Engine- und 1 Sidecar-Transporttest bestanden**; die zwei Query-DTO-Tests gehören zur Engine-Suite.
- ODE-002 Sidecar-Profil: **16 Desktoptests bestanden**. Striktes Clippy mit `-D warnings` besteht für Engine/Desktop In-Process und Desktop Sidecar.
- Native Windows-IPC-Smokes bestanden für In-Process und Sidecar. Beide bestätigen Raw History, Resolved/Explain, RecordedAsOf und SchemaMode sowie Projekt-/Fensterautorisierung, Event-/Provenienzfälle, keine Core-Netzwerklistener und sauberen Prozessabschluss.
- `cargo xtask verify`: **39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**. Darin liegen Workspace-Tests, striktes Root-Clippy, Plancheck, Sourcecheck und `git diff --check`.
- Node-Syntaxprüfung der Oberfläche und PowerShell-Parserprüfung des IPC-Smokes bestanden.
- Linux- und macOS-Prüfungen bleiben gemäß Vorgabe bis M9-07 zurückgestellt.
