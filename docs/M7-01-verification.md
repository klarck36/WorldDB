# M7-01 – Migrationskategorien und unveränderlicher Plan

**Stand:** 2026-10-02
**Ergebnis:** Windows-Prüfung bestanden; plattformübergreifende Nachweise bleiben gemäß Projektvorgabe M9-07 vorbehalten.

## Umsetzung

- `MigrationCategory` ist geschlossen auf `MetadataOnly`, `Additive`, `CompatibleConstraintChange`, `Restrictive` und `Breaking`; jede Kategorie ordnet sich einer kompatiblen, restriktiven oder brechenden Schemaauswirkung zu.
- `SchemaIdentityTransition` bindet Quelle und Ziel an typisierte Schema-IDs (`Layer`, `EntityType`, `Predicate`, `EventKind`). MetadataOnly, CompatibleConstraintChange und Restrictive behalten die vorhandene Identität. Additive kann eine neue Definition ohne Vorgänger einführen oder die bestehende Identität behalten. Ein Bedeutungsbruch verlangt eine neue ID derselben Familie; eine Restrictive-Transition darf eine bestehende Schemaidentität nicht stillschweigend löschen.
- `MigrationPlan` ist nach Validierung unveränderlich. Er bindet Quellrevision und Quellfingerprint, Zielrevision und Zielfingerprint, eindeutige geordnete Schritte, kanonisch geordnete Schemaänderungen, positive Transformer-Version, endliches Arbeits-/Speicherbudget und einen domänengetrennten kanonischen BLAKE3-Fingerprint.
- Doppelte Quell-/Zielidentitätspaare werden auch dann abgewiesen, wenn sie mit verschiedenen Kategorien eingetragen wurden.
- Source-Revision und -Fingerprint werden vor Start oder Fortsetzung über `validate_source_schema` exakt verglichen. Die konkrete Ausführungsintegration von Start/Resume gehört zu M7-06 und bleibt dort nachzuweisen.
- Wire-Kind `0x100b` speichert alle Planfelder in geschlossener Form; der Golden-Datensatz wurde auf das vollständige Format aktualisiert. Plan- und Run-Identitäten bleiben getrennt.

## Nachweise

- `migration::tests`: geschlossene Kategorien und Auswirkungsabbildung, Identitätsregeln für alle Änderungsarten, neue gleichfamiliäre ID bei Breaking, Planfelder/Fingerprint-Bindung, Source-Precondition, Zielreihenfolge, Duplikate, Null-Transformer und getrennte Run-Identität.
- `wire_records::tests::migration_plan_decoder_reports_precise_invalid_fields`: unbekannte und falsch klassifizierte Kategorie, Null-Arbeits-/Speicherbudget sowie ungültige verschachtelte Kategorie werden am konkreten Feld abgewiesen.
- Compile-fail-Rustdoc-Nachweise zeigen, dass Planfelder nicht mutierbar sind und eine `MigrationId` nicht als `MigrationRunId` verwendet werden kann.
- `cargo test --locked --workspace`: **PASS**; 458 Core-Unit-Tests und alle weiteren Workspace-, Contract- und Rustdoc-Suites bestanden. Die 2 bekannten Langzeitkampagnen bleiben wie zuvor separat gekennzeichnet.
- `cargo test --locked -p worlddb-core --doc`: **PASS**, 84 Rustdoc-Tests.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo fmt --all -- --check`: **PASS**.

- `python -B -X utf8 WorldDB_1.0_Plancheck.py` und `WorldDB_1.0_Sourcecheck.py`: **PASS**, 243 Tasks, 253 Invarianten und 174 Folgebelegpaare; alle Contract-Spiegel stimmen bytegenau.
- `docs/contract/build_contract_sources.py --verify-only` und `docs/contract/verify_contract_docs.py`: **PASS**; 253-ID-Einstufung, 24 First-Class-Typen und 52 bestätigte HARD-Lücken geprüft.
- `tools/check_exceptions.py`: **PASS**; registrierte lokale Ausnahmen gültig.
- `cargo xtask verify`: **36 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.
- `git diff --check`: **PASS**.

Linux/macOS und echte Datei-/OS-Cold-Cache-Nachweise werden nicht als Windows-Ergebnis behauptet; sie bleiben M9-07 vorbehalten.
