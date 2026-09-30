# M1-02 – Zeittypen und Intervalle

**Stand:** 2026-09-29\
**Umfang:** Master §§2.1, 12 und 31.3; ADR-032; WDB-TIM-001–003.

## Umsetzung

- `RecordedAsOf` ist ein eigener Typ auf der `Revision`-Achse. Sein Konstruktor benennt ausdrücklich die Voraussetzung, dass ein History-Reader die Veröffentlichung der Revision geprüft hat.
- `Timeline` kapselt eine `TimelineId`. `WorldTime` trägt diese Timeline und einen schemaaufgelösten `i128`-Nanosekundenkoordinatenwert. `WorldTime` besitzt absichtlich keine globale `Ord`-/`PartialOrd`-Ordnung; `checked_cmp` liefert für unterschiedliche Timelines `IncomparableTimelines`.
- `Duration` ist ein eigener signed-`i128`-Nanosekundenwert. Addition zu `WorldTime` prüft Overflow und bewahrt die Timeline.
- `EventTime` unterscheidet `Instant` und `Span`. Ein Span hat inklusiven Start, optionales offenes Ende und bei geschlossenem Ende zwingend `start < end`.
- `TimeInterval` und `AssertionValidity` teilen die explizite `[start,end)`-Semantik. `None` steht für eine unbeschränkte Startgrenze beziehungsweise ein offenes Ende; Endpunkte auf verschiedenen Timelines und umgekehrte Intervalle werden abgewiesen. Gleiche Grenzen bilden das mathematisch leere Intervall.

`WorldTime` ist hier die bereits auf eine gepinnte Schemaansicht aufgelöste Vergleichskoordinate. Der spätere Core-Value `Time` bewahrt die eingegebenen Ticks und das Einheitssymbol; die Schemaauflösung und geprüfte Normalisierung zu Nanosekunden gehören zu den späteren Value-/Schema-Arbeiten. M1-02 enthält noch keine Timeline-Registry, Kalenderabbildung, Wire-Decoder oder Datenbankprüfung, ob eine Revision veröffentlicht wurde.

## Verifikation

- `cargo test --locked --offline --workspace --all-targets`: 28 Tests bestanden (17 core, 7 testkit, 4 xtask).
- `cargo test --locked --offline --doc --package worlddb-core`: 10 Compile-Fail-Beispiele bestanden, darunter getrennte `RecordedAsOf`/`Revision`, `Duration`/`WorldTime`, `EventTime`/`WorldTime` und `AssertionValidity`/`WorldTime` sowie die fehlende implizite Zeitordnung.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- Kanonischer Abschlusslauf: `cargo xtask verify` – 27 PASS, ein sichtbarer `ci-matrix`-SKIP, 0 FAIL.

Der sichtbare `ci-matrix`-Skip bleibt an M0-14 gebunden. Externe Provider-CI und macOS sind nicht Teil dieses lokalen M1-02-Nachweises.
