# M6-05 – Eventindizes

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-core/src/event_indexes.rs` ergänzt vier Indexmodelle:

- `EventSearchIndex` indiziert EventKind, ParticipantRole und exakte ParticipantRole-/Entity-Paare.
- `EventTimeIndex` legt Eventzeiten nach Timeline und Startpunkt ab. Instant- und Span-Abfragen respektieren die halboffenen Grenzen. Ein späterer `EventSpanClosure` kann aus einem gespeicherten offenen Span einen zusätzlichen Kandidaten machen; die bestehende Eventprojektion bleibt der abschließende Filter. Events anderer Timelines bleiben zur Referenzprüfung als Kandidaten erhalten, damit sichtbare inkompatible Zeiten weiterhin denselben Fehler auslösen.
- `EventMaskIndex` indiziert EventMasks nach ihrem konkreten Target-Event. Archive, Retractions und strikte ContextPrecedence bleiben eigenständige Lifecycle-/Queryfilter.
- `EventRelationIndex` indiziert ausschließlich gespeicherte direkte Endpunkte und RelationKinds. Es erzeugt weder Relationskanten aus EventTime noch transitive Before-/Causes-Datensätze. As-of- und Retractionfilter bleiben bei der Relationprojektion.

Die Indexe liefern Identitätskandidaten und werden noch nicht in produktive Querypfade geschaltet; das folgt mit M6-08.

## Nachweise

- Drei gezielte Eventindex-Tests bestanden.
- EventKind-, ParticipantRole- und Teilnehmerpostings stimmen mit unabhängigen Filtern der Eventrecords überein.
- EventTime-Kandidaten an Instant-/Span- und halb offenen Zeitgrenzen wurden gegen `full_scan_event_candidates` geprüft. Nach dem indexbasierten Kandidatenfilter liefert die Referenzprojektion dieselben Ergebnisse; eine durch Closure verkürzte offene Spanne wird korrekt durch den Endfilter ausgeschlossen. Auch sichtbare Events auf inkompatiblen Timelines bleiben erhalten, sodass derselbe Referenzfehler auftritt.
- EventMask-Targetpostings stimmen mit einem direkten Scan nach `target_event` überein; doppelte Mask-IDs werden abgewiesen.
- EventRelation-Postings stimmen mit dem Scan expliziter direkter Kanten überein. Im Causes-Pfad A→B→C gibt die Suche für A nur A→B zurück; zwischen A und C wird keine synthetische Kante erzeugt. Doppelte Relation-IDs werden abgewiesen.
- `cargo test --locked --workspace`: PASS; 419 Core-Unit-Tests und 82 Rustdoc-Tests bestanden. Die separat markierten Decoder-Fuzz-, Präzisions- und Windows-Crashkampagnen blieben ignoriert und zählen nicht als bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -X utf8 WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, 253 Invarianten und 174 Folgebelegpaare strukturell gültig.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py`: PASS; alle sechs Contract-Spiegel bytegleich.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.

## Abgrenzung

Diese Ergebnisse belegen die Kandidatenindexe und ihre Differentialgleichheit mit den Referenzscans auf Windows. Die Produktivintegration bleibt M6-08. Linux- und macOS-Prüfungen sowie das M5-23-Plattformgate bleiben wie vom Product Owner zurückgestellt offen.
