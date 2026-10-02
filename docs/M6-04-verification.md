# M6-04 – Zeit-/Mask-/Boundary-Indizes

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-core/src/mask_time_indexes.rs` ergänzt vier getrennte Indexmodelle:

- `AssertionValidityIndex` ordnet Assertion-, Mask- und ReplacementBoundary-Validitäten nach Timeline und Startzeit. Treffer folgen dem halboffenen Intervall `[start, end)`; optionale Validitäten ohne Intervall gelten auf allen WorldTimes.
- `MaskSelectorIndex` hat getrennte Postings für `ExactAssertion`, `Proposition` und `Slot`. Propositionen werden erst nach Subject, Predicate und Polarity eingegrenzt; Werte nutzen kanonischen Vergleich oder den vom Aufrufer gebundenen schema-temporalen Comparator. Slot-Postings enthalten Perspective- und Epistemic-Partition.
- `ContextPrecedenceIndex` bindet die HistorySpace-Ahnen und Layer-Ranks eines Katalog-/Schemasnapshots vorab. Fehlende Koordinaten werden über die Referenzberechnung aufgelöst, damit auch deren Fehlerverhalten erhalten bleibt.
- `ReplacementBoundaryIndex` grenzt Boundary-Datensätze nach Subject, Predicate und epistemischer Partition ein. Zeit, Layer, Archivsichtbarkeit, Lifecycle und HistorySpace-Ancestry bleiben eigenständige nachgelagerte Filter.

Die Indexe sind als abgeleitete Strukturen für spätere Queryintegration exportiert. Diese Task schaltet keine produktiven Querypfade um; Indexauswahl und Full-Scan-/Budget-Fallback folgen in M6-08.

## Nachweise

- Fünf gezielte `mask_time_indexes::tests` bestanden.
- Der Validitätstest vergleicht Assertionen, Masken und Boundaries mit einem unabhängigen Scan an Start-, End- und Außenpunkten; er deckt offene Enden und mehrere Zeitbereiche ab.
- Selector-Postings werden für alle drei geschlossenen Selector-Arten gegen einen separaten Selector-Scan geprüft.
- Die integrierte Maskprojektion vergleicht die zusammengesetzten Selector-, Validitäts- und Precedence-Indexe gegen `apply_assertion_masks` bei WorldTimes 0, 2, 10 und 20. Der Korpus enthält Strict-Precedence über Root/Child und Base/Overlay, halb offene Zeitgrenzen, eine wirksame Closure, eine Retraktion, ein zukünftiges Mask-Record, eine archiv-unsichtbare Maske, einen Geschwister-Branch und einen Selector-Treffer mit niedrigerer Precedence.
- Der Boundary-Slotindex stimmt mit dem Scan überein; die gefilterten Boundaries und der vollständige Boundary-Stream liefern dieselben `MultiValueReplace`-Contributor-Assertions.
- `cargo test --locked --workspace`: PASS; 416 Core-Unit-Tests und 82 Rustdoc-Tests bestanden. Die separat markierten Decoder-Fuzz-, Präzisions- und Windows-Crashkampagnen blieben ignoriert und zählen nicht als bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -X utf8 WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, 253 Invarianten und 174 Folgebelegpaare strukturell gültig.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py`: PASS; alle sechs Contract-Spiegel bytegleich.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.

## Abgrenzung

Die Ergebnisse belegen Indexstrukturen und ihre Differentialgleichheit mit Referenzscans auf Windows. Die vollständige Query-Differential-Suite für `WDB-IDX-001` bleibt M6-13. Linux- und macOS-Prüfungen sowie das M5-23-Plattformgate bleiben wie vom Product Owner zurückgestellt offen.
