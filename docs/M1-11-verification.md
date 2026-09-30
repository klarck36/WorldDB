# M1-11 – Events und EventMask

**Status:** DONE\
**Datum:** 29. September 2026\
**Taskregister:** `WorldDB_1.0_Taskregister.tsv`

## Ergebnis

`worlddb-core` enthält nun immutable Event-Datentypen und Lebenszyklusrecords in `crates/worlddb-core/src/events.rs`:

- `Participants` sortiert `(EventRoleId, EntityId)` kanonisch und prüft erlaubte Rollen, doppelte Paare sowie minimale und maximale Kardinalität.
- `EventAttributes` sortiert nach `EventAttributeId` und prüft Duplikate, nicht deklarierte und fehlende Pflichtattribute sowie den exakten `ValueKind`.
- `EventDraft` validiert Teilnehmer und Attribute gegen eine konkrete `EventKindDefinition` und prüft die zugelassene EventTime-Form. Geschlossene Spans müssen eine positive Dauer auf derselben Timeline haben.
- `Event::new` stellt sicher, dass die verwendete EventKind-Definition spätestens in der Erstellungsrevision des Events veröffentlicht ist.
- `EventMask`, `EventSpanClosure`, `EventRetraction` und `EventMaskRetraction` besitzen getrennte IDs und private Felder. Eine SpanClosure akzeptiert nur zuvor offen angelegte Spans, eine spätere Transaction-Time-Revision und einen späteren Endzeitpunkt auf derselben Timeline.
- Öffentliche Typen sind über `worlddb-core` re-exportiert. Dokumentationstests belegen unter anderem, dass EventMask und EventRetraction sowie ihre Lifecycle-ID-Familien nicht austauschbar sind.

## Abgrenzung

M1-11 liefert die typisierte Modell- und Konstruktionsvalidierung. Ob das referenzierte Event für eine EventMask im Commit-/Query-Kontext existiert und die erforderliche Präzedenz erfüllt, hängt von späteren Kontextprüfungen ab. EntityType-Zuordnung der Teilnehmer, reichere Constraint-Auswertung von Attributwerten sowie Timeline-Kalenderprofile sind ebenfalls keine Aufgaben dieses Core-Konstruktors. EventMask erzeugt keine Kaskade und nimmt keine Wirkungen zurück.

## Prüfung

- `cargo fmt --all`
- `cargo test --locked --offline --workspace --all-targets` — 73 Tests bestanden (62 Core-, 7 Testkit- und 4 xtask-Tests).
- `cargo test --locked --offline --doc --package worlddb-core` — 38 Dokumentationstests bestanden.
- `cargo xtask verify` — 27 PASS, 1 sichtbarer `ci-matrix`-SKIP (M0-14), 0 FAIL.

Die negativen Fälle umfassen doppelte Teilnehmerpaare und Attribute, unbekannte Rollen und Attribute, unterschrittene/überschrittene Rollen-Kardinalität, fehlende Pflichtattribute, abweichende Werttypen, unzulässige Zeitformen, nichtpositive Spans, eine noch nicht veröffentlichte EventKind-Revision sowie ungültige SpanClosure-Ziele und Endpunkte.

## Nächster Schritt

`M1-12 – EventRelation-Kanonik` ist im Taskregister `READY`.
