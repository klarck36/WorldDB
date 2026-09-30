# M1-08 – ContextKey und epistemische Partitionen

Status: **DONE**\
Prüfdatum: 2026-09-29

## Ergebnis

`crates/worlddb-core/src/context.rs` führt die geschlossenen Typen `PerspectiveScope::{World, Perspective(PerspectiveId)}` und `EpistemicMode::{WorldState, Knows, Believes, Claims}` ein. `ContextKey` kombiniert sie mit einer konkreten `HistorySpaceId` und `LayerId`. Damit ist die Layerauswahl bereits auf eine ausdrückliche Layer-ID materialisiert, bevor ein Kontext entsteht.

Der Konstruktor nimmt ausschließlich diese Paare an:

| PerspectiveScope | EpistemicMode |
|---|---|
| `World` | `WorldState` |
| `Perspective(id)` | `Knows`, `Believes` oder `Claims` |

Alle anderen Kombinationen liefern `ContextError::InvalidPerspectiveModePair`. Es gibt keinen Default-Perspective-Fallback. Der Perspective-ID-Wert bleibt typisiert und kann nicht als `PrincipalId` eingesetzt werden.

## Grenzen und Folgebelege

`ContextKey` definiert Partitionen, aber keine Inferenz zwischen `WorldState`, `Knows`, `Believes` und `Claims`. Ebenso implementiert dieser Schritt weder `ContextPrecedence` noch Masking oder Security-Filterung. Die Resolver-/Masking-Regeln folgen M2-04/M3-15; Epistemik-Auswertung folgt M2-11. Abgleich der Perspective-Existenz und des Lifecycle beim Recordbau sowie vollständige Query-Snapshot-Pinnung folgen M3-07/M4-04b.

Event-Records erhalten keine Perspective- oder Epistemic-Achse durch `ContextKey`; ihre Kontext- und Maskingordnung bleibt HistorySpace/Layer-basiert und wird bei den Event-/Resolver-Aufgaben umgesetzt.

## Belege

- `crates/worlddb-core/src/context.rs`: getypte Kontextbestandteile und validierender Konstruktor.
- `crates/worlddb-core/src/lib.rs`: öffentliche Reexports.
- `cargo test --locked --offline --workspace --all-targets`: 48 Core-, 7 Testkit- und 4 xtask-Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 25 Dokumentationstests bestanden (24 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: Struktur und DAG gültig.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.
