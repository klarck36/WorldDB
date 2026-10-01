# M3-07 – QueryContext

**Ergebnis:** PASS für den verpflichtenden, validierten QueryContext; geprüft am 2026-09-30.

`QueryContext` wird ausschließlich mit einem vollständigen `QueryContextInput` gebaut. Er bindet einen konkreten `SnapshotRef` samt publizierter Revision, `RecordedAsOf`, HistorySpace, schema-validierte Layer, WorldTime-Auswahl, Perspective/EpistemicMode, eine vollständige `HistoricalQueryBinding`, die vom vertrauenswürdigen Host gesetzte Principal-/Autorisierungszeitbasis, endliche Budgets und einen CancellationToken. Es gibt keine Default-Konstruktion mit ausgelassenen Achsen.

`ValidatedLayerSelection` kann nur durch Auflösung gegen einen `LayerSchemaSnapshot` entstehen und bindet dessen Revision. Der Context verlangt, dass diese Revision mit dem vollständigen Schema-Binding einschließlich `SchemaMode` und Fingerprint übereinstimmt. `RecordedAsOf` darf weder nach der Snapshotrevision liegen noch vom Schema-Binding abweichen. Ungültige Perspective-/Epistemikpaare sowie eine Policy-Historie nach dem Snapshot werden abgewiesen.

QueryBudgets sind für Kandidaten, Work Units und Ergebnisse jeweils positiv und dürfen die festgelegten Engine-Höchstwerte nicht überschreiten. Der CancellationToken ist geteilter Out-of-Band-Zustand des vertrauenswürdigen Hosts; Cancellation wird weder als Budgetende behandelt noch in ein erfolgreiches Ergebnis umgedeutet. `WorldTimeSelector` bindet die bestätigte Auswahl `AllTimes` oder einen konkreten bereits schemaaufgelösten `WorldTime`-Zeitpunkt.

## Nachweise

- `crates/worlddb-core/src/query_context.rs`: vollständige Context-Typen, Validierung, finite Budgets und CancellationToken.
- `crates/worlddb-core/src/lib.rs`: öffentliche Exporte sowie Compile-Fail-Nachweis, dass PerspectiveId weder Principal noch SecurityContext-Identität sein kann.
- `query_context::tests::valid_context_binds_every_axis_and_pinned_layers`: vollständiger gültiger Context und konkrete Pins.
- `query_context::tests::context_rejects_future_data_and_policy_revisions`: Zukunftsrevisionen fail closed.
- `query_context::tests::context_rejects_invalid_partitions_and_schema_pins`: Perspektive/Epistemik- und Schemaabweichungen fail closed.
- `query_context::tests::layer_selection_must_resolve_against_the_bound_schema`: unbekannte Layer werden vor Context-Bau abgewiesen.
- `query_context::tests::budgets_are_positive_finite_and_bounded`: Nullbudgets und Überschreitung der Hard Maxima werden abgewiesen.
- `query_context::tests::cancellation_is_shared_and_distinct_from_budget`: Cancellation bleibt ein separater geteilter Hostzustand.
- `cargo test --locked --workspace`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.

## Abgrenzung

Die Context-Konstruktion verlangt bereits aufgelöste Daten-/Schema-Snapshots und eine hostseitig authentifizierte PrincipalId. Tatsächliche Snapshotauswahl im Engine-Session, Security-Auswertung, Queryoperation und das Durchreichen von Cancellation/Budgets durch Candidate-Streams werden von den nachfolgenden M3-Aufgaben umgesetzt. Es gibt keine GitHub- oder Remote-Abhängigkeit.
