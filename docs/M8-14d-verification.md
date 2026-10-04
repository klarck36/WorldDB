# M8-14d – Auflösungsvorschau

Stand: 4. Oktober 2026

## Ergebnis

`ProductiveQueryEngine::resolution_preview` wertet Assertions, Masks und ReplacementBoundaries mit dem übergebenen, gepinnten `QueryContext` und der zugehörigen Policy-Historie aus. `WorldTimeSelector::At` liefert genau eine Punktvorschau; `AllTimes` bildet pro sichtbarer Timeline eine vollständige Folge halb offener Zeitintervalle und löst jede Zelle mit derselben produktiven Kandidaten-, Masken- und Boundary-Auflösung auf. Ereignisfenster sind kein Eingabefeld dieser Abfrage.

Die All-Times-Planung berücksichtigt Assertions nur dann als Zeitquelle, wenn sie am ausgewählten `RecordedAsOf` unarchiviert und für den Principal sichtbar sind. Gültigkeitsenden und gültige Boundary-Intervalle schneiden die Zellen. Eine ReplacementBoundary kann allein eine Timeline festlegen und im aktiven Bereich ein bekanntes leeres MultiValue-Set (`Known { values: [] }`) liefern.

Die Ausgaben unterscheiden `Known`, `Unknown`, `Conflict` und `CompleteEmpty`. `CompleteEmpty` bedeutet, dass die vollständige All-Times-Abfrage weder eine sichtbare Assertion-Timeline noch eine explizite Boundary-Zeitdomäne findet. Ein Punkt ohne Kandidaten bleibt `Unknown`. Scan-, Ergebnis-, Arbeits-, Speicher-, Abbruch- und Datenfehler bleiben technische `QueryEngineError`; die Methode gibt bei einem solchen Fehler kein Teilergebnis zurück. Die vollständige Vorschau ist als `OwnedQueryResult` an Kontext und Policy gebunden.

## Nachweise

- `query_engine::tests::all_times_preview_partitions_validity_and_preserves_unknown_regions`: offene Ränder und aktives Assertion-Intervall ergeben Unknown/Known/Unknown.
- `query_engine::tests::all_times_preview_preserves_conflict_outcomes`: All-Times bewahrt einen fachlichen Conflict.
- `query_engine::tests::all_times_preview_reports_boundary_defined_empty_set`: Boundary ohne Assertions ergibt im aktiven Intervall ausdrücklich `Known { values: [] }`.
- `query_engine::tests::complete_empty_history_is_distinct_from_point_unknown_and_budget_errors`: All-Times `CompleteEmpty`, Punkt-`Unknown`, Full-Scan- und Prozessspeicherfehler bleiben getrennt; nach Speicherfehler sind Reservierungen freigegeben.
- `query_engine::tests::all_times_preview_excludes_archived_assertion_timelines`: archivierte Assertions erzeugen keine Ergebniszeitdomäne.
- Die Query-Engine-Testgruppe (`cargo test --locked -p worlddb-core query_engine::tests::`) bestand mit 18 Tests.
- `cargo test --locked -p worlddb-core --all-targets` bestand: 519 Core-Unit-Tests und alle 5 Wire-Oracle-Tests bestanden; die drei langen manuellen Kampagnen blieben wie markiert ignoriert.
- Striktes Clippy für `worlddb-core` mit `--all-targets -- -D warnings` bestand.
- `cargo xtask verify` bestand auf Windows: 39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- Plancheck, Sourcecheck und `git diff --check HEAD` bestanden.
- Linux- und macOS-Nachweise bleiben vereinbarungsgemäß bis M9-07 zurückgestellt.
