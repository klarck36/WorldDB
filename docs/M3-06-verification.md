# M3-06 – Non-Interference-Testkit

**Status:** DONE, lokaler Referenzpfad verifiziert 2026-09-30.

## Testkit

`crates/worlddb-core/src/non_interference.rs` liefert ein wiederverwendbares, nur für Tests kompiliertes Paarweltmodell. Eine Paarwelt teilt ihren sichtbaren Zustand und variiert zwei verborgene Zustände. Der Beobachter projiziert jede Ausführung auf die öffentliche Ausgabe. Das Kit vergleicht Ergebniswerte und Counts, Antwortform, stabile Fehlercodes, Fehlerform und Cursorverhalten einzeln. Ein Fehlschlag nennt nur die abweichende Dimension und formatiert keine verglichenen Werte.

Die Fixture-Erstellung verantwortet die Prämisse, dass sich die beiden Welten nur in verborgenen Daten unterscheiden. Der generische Vergleicher behauptet nicht, diese Prämisse automatisch aus beliebigen Datenbanken zu beweisen.

## Angewandte Paarwelt- und Grenztests

- Search: `query_search::tests::token_search_is_exact_typed_and_excludes_hidden_candidates_before_budgets` vergleicht die Referenzantwort mit und ohne verborgenes Suchdokument.
- Graph: `query_graph::tests::hidden_nodes_and_edges_are_filtered_before_budgets_and_traversal` vergleicht sichtbare Graphwerte und Kantenanzahl mit/ohne versteckten Knoten, Kanten und verweigerte Beziehungen.
- Count: `query_aggregate::tests::count_exists_and_grouped_count_consume_only_visible_resolved_rows` vergleicht Count/Exists/GroupedCount sowie die Count-Paarwelt nur über sichtbare resolved Rows.
- Result/Conflict: `query_ports::tests::resolved_view_cannot_expose_a_hidden_contributor` rejectet einen Konfliktbeitrag außerhalb des sichtbaren Assertion-Sets.
- Explain: `query_ports::tests::explain_boundary_rejects_hidden_candidates_and_applied_records` rejectet versteckte Kandidaten und angewandte Records aus Explain.
- Fehler: `errors::tests::public_error_views_hide_secret_causes_and_preserve_typed_sources` und `errors::tests::error_codes_are_stable_and_domain_specific` belegen stabile Public-Codes und Secret-Redaktion; das Testkit prüft Code-/Shape-Differenzen als eigene öffentliche Vergleichsdimension.
- Cursor: `cursor::tests::expiry_unknown_and_restart_share_cursor_invalidated` und `cursor::tests::wire_handle_hides_payload_and_tampering_has_uniform_invalidation` belegen gleiche öffentliche Invalidierung für unbekannte, abgelaufene, sitzungsfremde und manipulierte Cursor. Das Testkit kann Continuation/Ende/Invalidierung als Paarweltdimension vergleichen.
- `non_interference::tests::paired_world_compares_values_shapes_public_errors_and_cursor_behavior` übt die Vergleichsdimensionen mit absichtlich abweichenden Projektionen positiv und negativ aus.

## Abgrenzung

Die M3-Tests üben die indexfreien Referenzpfade und Security-Projektionsgrenzen aus. Die vollständige kombinierte M3-Vertragstruth-table und Wiederholung für produktive Adapter-/optimierte Indexpfade bleiben in M3-15/M3-16 bzw. M6 registriert. Funktionale Non-Interference wird geprüft; eine Constant-Time-Garantie wird nicht behauptet.

## Validierung

- `cargo test --locked --workspace`: 286 Core-Tests und 73 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- Plancheck: PASS.
- `git diff --check HEAD`: PASS.
