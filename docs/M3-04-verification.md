# M3-04 – Autorisierung vor Candidate-Erzeugung

**Ergebnis:** PASS für die lokalen Query- und Meta-History-Referenzpfade; geprüft am 2026-09-30.

`SecurityPolicySnapshot` wertet direkte Principal-Regeln und explizite Rollenregeln mit konjunktivem Ressourcenscope aus. Deny überstimmt Allow; fehlende Grants, unbekannte Principals und inaktive Principals verweigern. Query-Einstiegspunkte binden den authentifizierten Principal über `QueryContext` und verlangen die jeweilige Operations-Capability.

Assertions werden vor Lifecycle-Projektion und Kandidatenerzeugung nach HistorySpace, Layer, Record und allen zur Kandidatenbildung verwendeten Feldern gefiltert. Event-Records und EventMasken werden vor ihrer Lifecycle-/Maskenprojektion geprüft. Assertion-Masken samt Closure und Retraction werden vor der Redaction verworfen, wenn Maskenrecord oder Selector-/Validity-Felder nicht freigegeben sind. Source/Evidence benötigt SourceRead plus Rechte auf sämtliche ausgegebenen Source-Felder, ein autorisiertes Target, EvidenceRead, RelationshipRead und LifecycleRead. EventRelation und Provenance prüfen Kantenrechte und beide sichtbaren Endpunkte. Raw-History verlangt RawHistoryRead pro Record zusätzlich zum HistorySpaceRead.

Versteckte Records und ihre Lifecycle-Records gelangen vor strukturelle Validierung, Masking, Resolution oder Kantenprojektion aus den autorisierten Pfaden. Dadurch erzeugen verborgene Duplikate oder fehlende Grants keine öffentlichen Kandidaten, Kanten oder entsprechenden Validierungsfehler. `PerspectiveId` wählt keine Security-Identität und verleiht keine Capabilities.

## Nachweise

- `crates/worlddb-core/src/security.rs`: Scope-Matching, Deny-Vorrang, aktive Principals, Role-/Principal-Regelvereinigung und Capability-Evaluator.
- `crates/worlddb-core/src/candidate_scan.rs`: QueryResolve-Gate und feldbezogener Assertion-Filter vor Lifecycle-Projektion.
- `crates/worlddb-core/src/mask_projection.rs`: autorisierte Maskenhistorie und Maskenfilter vor Redaction.
- `crates/worlddb-core/src/event_projection.rs`: Event-/EventMask-Filter vor Lifecycle- und Maskenprojektion.
- `crates/worlddb-core/src/source_evidence_projection.rs`: Source-Feld- und Evidence-Endpoint-Schnittmenge vor Metahistory-Validierung.
- `crates/worlddb-core/src/event_relations.rs` und `crates/worlddb-core/src/provenance_graph.rs`: Kanten- und Endpoint-Autorisierung vor Graph-/Lifecycle-Projektion.
- `crates/worlddb-core/src/reference_query.rs`: RawHistoryRead-Filter vor Row-Aufbau und Kanonisierung.
- `candidate_scan::tests::authorization_filters_assertion_before_candidate_construction`: Feld-Deny, fehlende Grants, doppelte verborgene Record-ID und gleicher Perspective-Scope mit anderem Principal.
- `events::tests::event_is_hidden_when_a_candidate_field_is_denied`: Event-Feld-Deny.
- `mask_projection::tests::masks_need_selector_and_validity_field_rights_before_projection`: Mask- und Mask-Feldrechte.
- `source_evidence_projection::tests::source_is_hidden_when_any_emitted_field_is_not_authorized`: Source-Feld, Zielrecord und Evidence-Beziehung werden als gemeinsame Sichtbarkeitsbedingung geprüft.
- `event_relations::tests::event_relation_requires_a_visible_relationship_grant`: typisierte Relationship-Regel und scoped Deny.
- `provenance_graph::tests::provenance_endpoint_requires_record_visibility_and_its_resource_scope`: beide Resource-Achsen und Record-Recht des Provenance-Endpunkts.
- `cargo test --locked --workspace`: PASS, 261 Core-Tests und 72 Rustdoc-Tests.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, ein sichtbarer `ci-matrix`-SKIP aus M0-14, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS; 236 Tasks, 11 Milestones, 253 Invarianten, 169 Follow-up-Paare.
- `git diff --check HEAD`: PASS. GitHub oder Remote-Repository wurde nicht verwendet.

## Abgrenzung

Der Provenance-Projektor erwartet für jeden Endpunkt einen vom Aufrufer aus demselben Query-Snapshot aufgelösten `PolicyTarget`; fehlende oder nicht zum Endpunkt passende Koordinaten schließen die Kante aus. Historisierte Policy-Zeit, SecurityEpoch und AuthorizationAtRevision folgen M3-05. Das vollständige Paarwelt-Testkit für Search, Graph, Count, Explain und Error-Shapes bleibt M3-06.
