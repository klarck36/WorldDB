# M1-12 – EventRelation-Kanonik

**Status:** DONE\
**Datum:** 29. September 2026\
**Taskregister:** `WorldDB_1.0_Taskregister.tsv`

## Ergebnis

`crates/worlddb-core/src/event_relations.rs` stellt immutable EventRelation-Records und ihre getrennte Retraction bereit:

- `EventRelationKind` ist der geschlossene persistierte Katalog `Before | SameTime | Causes`; `After` existiert darin nicht.
- `EventRelationInputKind` nimmt `After` als Eingabealias an. Der Konstruktor normalisiert `After(A, B)` vor der Record-Erzeugung zu `Before(B, A)`.
- `SameTime(A, B)` wird stets als geordnetes `(min(EventId), max(EventId))` gespeichert. `Before` und `Causes` bleiben gerichtet.
- `EventRelationKey` enthält den normalisierten logischen Schlüssel ohne Record-ID oder Erstellungsrevision.
- `EventRelationBatch` sortiert Kandidaten nach diesem Schlüssel und lehnt doppelte normalisierte Kanten ab.
- Self-Relations scheitern unabhängig von der Eingabeart. `EventRelationRetraction` hat eine eigene Lifecycle-ID, einen Grund und eine Transaction-Time-Revision nach dem Zielrecord.
- Die öffentlichen Typen sind über `worlddb-core` re-exportiert; Compile-Fail-Dokumentation prüft, dass `After` kein persistierter Kind ist, Lifecycle-IDs nicht austauschbar sind und Relation-Felder nicht mutiert werden können.

## Abgrenzung

Die Batch-Prüfung erkennt Duplikate unter den gemeinsam vorgeschlagenen Kanten. Die Prüfung gegen bereits aktive Historie muss vorhandene Retractions und den Graphsnapshot berücksichtigen; diese Referenzmodell- und Transaktionsprüfung folgt M2-13a. SameTime-Komponentenkollaps, Before-Widerspruchs-/Zyklusprüfung und der getrennte Causes-Graph folgen M2-13b. Zeitwerte erzeugen keine implizite Relation.

## Prüfung

- `cargo fmt --all`
- `cargo test --locked --offline --workspace --all-targets` — 79 Tests bestanden (68 Core-, 7 Testkit- und 4 xtask-Tests).
- `cargo test --locked --offline --doc --package worlddb-core` — 41 Dokumentationstests bestanden.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py` — Struktur, Abhängigkeiten und Referenzen gültig.
- `cargo xtask verify` — 27 PASS, 1 sichtbarer `ci-matrix`-SKIP (M0-14), 0 FAIL.

## Nächster Schritt

`M1-13 – Source, Evidence und Provenance` ist im Taskregister `READY`.
