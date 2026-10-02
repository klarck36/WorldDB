# M6-13 – Differential-Querysuite

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung und Abnahme

`WDB-IDX-001` bleibt wortgleich: „Indexresultate sind differential gleich zum Full-Scan-Orakel.“ Die produktive Indexauswahl wird gegen den unabhängigen M2-Kandidaten- und Auflösungspfad geprüft.

Der neue Test `query_engine::tests::m6_13_seeded_scaled_indexed_queries_match_the_full_scan_oracle` verwendet drei feste Seeds. Jede Welt enthält 148 Assertions, darunter 128 deterministisch generierte Datensätze mit variierenden Subjects, Predicate-Familien, Polaritäten und halboffenen Zeitgrenzen. Für acht Abfragepunkte und zwei Policy-Sichten vergleicht er sowohl `resolved_point` als auch `explain_point` direkt mit dem M2 Full Scan: 96 indizierte Engine-Ausführungen stimmen in Ergebnisart, Wert beziehungsweise Conflict und Contributor-IDs überein.

Zusätzlich werden zwei Gegenfälle geprüft:

- Ohne verfügbaren Index wählt die produktive Query den vollständigen Scan; Ergebnis und Contributors stimmen mit demselben Orakel überein.
- Eine absichtlich entfernte Indexzeile für einen Konfliktpunkt verändert das Ergebnis. Die Differentialkontrolle erkennt und verwirft diesen repräsentativen unvollständigen Treffer.

## Abdeckung der Indexfamilien

| Pfad | Differentialnachweis |
|---|---|
| Record-ID, Operation-ID, Schema-ID/Revision und Lifecycle | `record_indexes::tests::{record_id_hits_and_misses_match_the_full_scan_oracle, operation_id_hits_and_misses_match_the_full_scan_oracle, schema_id_revision_and_history_match_the_full_scan_oracle, lifecycle_history_and_type_safety_match_the_full_scan_oracle}` |
| Assertion Point, Entity-History und Predicate-History über Branches | `assertion_point_index::tests::point_and_both_history_indexes_match_full_scan_across_pinned_branches` sowie der neue produktive M6-13-Test |
| Validity, Mask-Selector, Context-Precedence und ReplacementBoundary | `mask_time_indexes::tests::{validity_index_matches_half_open_full_scan_for_assertions_masks_and_boundaries, mask_selector_postings_match_closed_selector_full_scan, mask_projection_composes_selector_validity_precedence_indexes_match_full_scan, cached_context_precedence_matches_slow_ancestry_calculation, boundary_slot_index_matches_scan_and_preserves_replace_results}` |
| Eventsuche/-zeit, EventMask und EventRelation | `events::tests::{event_kind_role_and_time_indexes_preserve_reference_candidates, event_mask_index_returns_only_the_concrete_target_mask}` und `event_relations::tests::relation_index_returns_only_explicit_direct_edges` |
| Evidence-Nachbarschaft und Provenance-Adjazenz | `source_provenance_indexes::tests::{evidence_neighborhood_matches_authorized_reference_and_hides_targets, provenance_adjacency_matches_authorized_scan_and_drops_hidden_endpoint_edges}` |
| Produktive Punkt-/Explain-Abfragen | `query_engine::tests::m6_13_seeded_scaled_indexed_queries_match_the_full_scan_oracle`; fehlender Index und unvollständige Treffer werden dort ebenfalls geprüft |

Der Quellpfad-Audit findet keine weiteren produktiven `Indexed`-Querypfade. Raw History, TokenSearch, Graph Traversal und COUNT/EXISTS/Gruppierung melden explizit `FullScan`. Paging bindet einen geordneten Snapshot-Quellstrom des vertrauenswürdigen Adapters und wählt selbst keinen Index. Ihre bisherigen Tests belegen diese Ausführungspfade; die Indexfamilien oben liefern ihre eigenen Orakelvergleiche, bevor sie künftig produktiv ausgewählt werden.

## Prüfung

- `cargo test --locked --workspace --quiet`: PASS; 438 Core-Tests und 82 Rustdoc-Tests bestanden. Markierte Langkampagnen blieben ignoriert und werden nicht als bestanden gezählt.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: PASS.
- `cargo xtask verify`: PASS mit 34 PASS, 1 erwartetem M0-14-`ci-matrix`-SKIP und 0 FAIL.
- Linux- und macOS-Prüfungen bleiben wie angeordnet zurückgestellt.

## Abgrenzung

Die M6-13-Ausführung vergleicht die derzeit produktiv ausgewählte Assertion-Point-Indexroute und alle vorbereiteten Indexfamilien mit ihren M2-Scans. Search-, Graph-, Aggregate- und Paging-Indizes werden noch nicht produktiv ausgewählt; sie bleiben deshalb auf dem vollständigen Scan- beziehungsweise gebundenen Adapterpfad. Die Performance-Korpus- und Plattformmessungen folgen in M6-14a/b.
