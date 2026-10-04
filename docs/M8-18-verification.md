# M8-18 – Suche, Graph und Aggregation anzeigen

Stand: 4. Oktober 2026

## Ergebnis

Die Query-Oberfläche ergänzt Raw History, Resolved und Explain um die indexfreie Wortsuche, einen begrenzten Graphdurchlauf und die Aggregate COUNT, EXISTS und COUNT nach Polarity. Alle Modi binden dieselben expliziten Queryachsen einschließlich RecordedAsOf, SchemaMode, HistorySpace, Layer, Perspektive/EpistemicMode und WorldTime sowie endliche Kandidaten-, Arbeits- und Ergebnisbudgets.

Die Wortsuche unterstützt die Trefferregeln „Alle Wörter“ und „Mindestens ein Wort“. Treffer geben nur Recordfamilie, Record-ID und gefundene Feldnamen zurück; Textausschnitte werden nicht offengelegt. Ergebnisse sind seitenweise und mit einem opaken Cursor an Queryidentität und Snapshot gebunden. Die Oberfläche zeigt an, wenn weitere Treffer ausstehen, weist auf die 60-Sekunden-Gültigkeit des Cursors hin und verlangt nach Ablauf einen Neustart der Suche.

Der Graphdurchlauf nimmt einen typisierten Startrecord, optionale Beziehungstypen, Richtung, Tiefe, Knoten-/Kantenlimits und ein Zyklusverhalten entgegen. Antwort und Oberfläche zeigen die erreichte Tiefe sowie die wirksamen Traversierungsgrenzen. Aggregationen werten sichtbare Resolved-Contributors aus; die Gruppierung erfolgt nach Polarity. Berechtigungsfilter wirken vor Trefferlisten, Auflösung und Aggregation. Das API- und UI-Resultat bleibt frei von Suchtextauszügen.

## Nachweise

- Storage-Regression `query_explorer_pages_search_and_returns_complete_graph_and_aggregates`: **1 bestanden**. Zwei passende Assertions werden über zwei Seiten vollständig gesucht; Graphwurzel, COUNT, EXISTS und Polarity-Gruppierung werden auf sichtbare Contributors geprüft.
- ODE-002 In-Process-Profil: **15 Desktop-, 25 Engine- und 1 Sidecar-Transporttest bestanden**.
- ODE-002 Sidecar-Profil: **16 Desktoptests bestanden**.
- Striktes ODE-Clippy (`-D warnings`) bestanden für Standard-/In-Process- und Sidecar-Profil; Root-Clippy ebenfalls bestanden.
- Native Windows-Smokes mit zwei authentisierten Fenstern bestanden in-process und sidecar. Beide führten die vollständigen bestehenden Projekt-, Schema-, Entity-, HistorySpace-, Perspektiven-, Rechte-, Fakten-, Event-, Source- und Provenance-Flüsse aus. M8-18 bestätigt je Lauf zwei TokenSearch-Seiten plus Abschluss, den Cursorablaufhinweis, Graphtraversierung mit Grenzen, COUNT, EXISTS und Gruppierung nach Polarity. Security-Proben, Netzwerklistenerkontrolle und geordnetes Prozessende bestanden ebenfalls.
- Node-Syntax für `frontend/main.js` und PowerShell-Parser für `run-ipc-security-smoke.ps1` bestanden.
- `cargo xtask verify`: **39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**. Der Skip verweist auf die separat laufende Plattformmatrix. Plancheck, Sourcecheck, Workspace-Tests, Root-Clippy und `git diff --check` bestanden.
- Linux-/macOS-Laufzeitnachweise bleiben vereinbarungsgemäß bis M9-07 zurückgestellt.
