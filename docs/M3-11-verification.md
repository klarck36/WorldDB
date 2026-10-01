# M3-11 – Search/Graph/Aggregation-Ports

**Status:** DONE, lokale Referenzpfade verifiziert 2026-09-30T20:16:30+02:00.

## Implementierung

- `query_search.rs` implementiert typisierte exakte Token-Suche über schemafreigegebene Textfelder. Rechte auf Query, Record und Feld werden geprüft, bevor Tokens verarbeitet oder Budgets verbraucht werden. Ergebnisse sind kanonisch geordnet und an den vollständigen `QueryContext` gebunden.
- `query_graph.rs` implementiert typisierte Graphrequests und indexfreie Traversierung mit Depth-, Node-, Edge- und deterministischem Work-Budget. Autorisierte Endpunkte und Relationship-/Provenance-Rechte werden vor Traversal und Budgetprojektion ausgewertet. Abbruch und Budgetende geben keinen Teilgraphen zurück.
- `query_aggregate.rs` implementiert Count, Exists und typisierte GroupedCount ausschließlich über gebundene, aufgelöste Resolved-Ergebnisse. Record- und Feldrechte werden erneut geprüft. Fehlende Felder bilden eine eigene Gruppe; Budgetende und Cancellation liefern keinen Teilerfolg.
- `QueryContextBinding` bindet die vollständigen semantischen Queryachsen (Snapshot/Revision, RecordedAsOf, Space/Layer, WorldTimeSelector, Perspektive/epistemischer Modus, Schema, Security und Budget). `OwnedQueryResult` trägt diese Bindung.

## Sicherheits- und Fehlernachweise

- `query_search::tests::token_search_is_exact_typed_and_excludes_hidden_candidates_before_budgets` vergleicht die öffentliche Antwort mit und ohne verstecktes Suchdokument. Treffer und Ergebnisbindung bleiben gleich.
- `query_graph::tests::hidden_nodes_and_edges_are_filtered_before_budgets_and_traversal` vergleicht denselben sichtbaren Graphen mit einem Kandidatensatz, der zusätzlich einen versteckten Knoten, eine versteckte Kante und eine verweigerte Beziehung enthält. Sichtbare Knoten/Kanten und deren Anzahl bleiben gleich; versteckte Kandidaten verbrauchen nicht den Traversal-Budgetpfad.
- `query_aggregate::tests::count_exists_and_grouped_count_consume_only_visible_resolved_rows` vergleicht Count, Exists und GroupedCount mit und ohne versteckte Resolved-Zeile. Werte und Gruppen bleiben identisch.
- `query_aggregate::tests::grouped_count_budget_exhaustion_returns_no_partial_aggregate` beweist fail-closed Budgetverhalten; `query_search::tests::search_returns_no_partial_result_on_budget_or_cancellation` und `query_graph::tests::zero_depth_returns_only_visible_roots_and_bounds_fail_without_partial_results` decken die entsprechenden Such- und Graphgrenzen ab.

## Vertragsgrenzen

Die Referenzimplementierung nutzt deterministische Work-Budgets und Cancellation. Ein wall-clock timeout wird nicht als Query-Semantik eingeführt. TokenSearch ist unterstützt; FullTextSearch wird nicht vorgetäuscht und muss von einem späteren Transport als nicht unterstützte Fähigkeit abgewiesen werden. Die eigenständige wiederverwendbare Paarwelt-Testkit-Infrastruktur bleibt M3-06; optimierte Backend-/Langzeitnachweise bleiben den registrierten M6/M8-Folgebelegen zugeordnet.

## Validierung

- `cargo test --locked -p worlddb-core query_`: 26 PASS.
- `cargo test --locked --workspace`: 285 Core-Tests und 73 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS (Git meldet lediglich vorhandene LF/CRLF-Konvertierungswarnungen).
