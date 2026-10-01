# M3-03 – Principal/Role/Capability-Modell

**Ergebnis:** PASS für die typisierte Security-Modellschicht und die expliziten Startpakete; geprüft am 2026-09-30.

`worlddb_core::security` enthält jetzt getrennte Typen für `Principal`, `PrincipalState`, `RoleDefinition`, `RoleAssignment`, `PolicySubject`, `CapabilityRule` und `PolicyScope`. Principals werden als vom Host authentifizierte Identitäten verstanden; das Core-Modell authentifiziert selbst keine Zugangsdaten. Rolle, Regelwirkung, Capability und Scope sind explizit und getypt.

Der Capability-Katalog ist geschlossen und einzeln typisiert. Field- und Relationship-Selectoren folgen dem Security-Policy-Vertrag. `PolicyScope::project()` ist der ausdrückliche leere Projektscope; gesetzte Scope-Dimensionen bleiben getrennte Werte, deren konjunktive Auswertung die Autorisierungsaufgabe M3-04 übernimmt. Rollen-Symbole validieren die bestätigte Grammatik `[a-z][a-z0-9_]*`.

`PolicyBundle::standard_gm()` enthält die expliziten 68 Allow-Regeln des genehmigten Standardpakets. `PolicyBundle::standard_player()` enthält keine Grants. `AdminRawRead` wird separat von `RawHistoryRead` modelliert und fehlt im GM-Paket. Purge, Import/Export, Migration, Audit-Konfiguration/-Export und JobManage sind ebenfalls keine Standard-GM-Grants. Der Evaluator darf weder Symbolnamen noch Perspective, UI-Zustand oder implizite Engine-Rechte als Bypass verwenden.

## Nachweise

- `crates/worlddb-core/src/security.rs`: Modell und Standardbundles.
- `crates/worlddb-core/src/lib.rs`: öffentliche Exporte und Compile-Fail-Beweis, dass `PerspectiveId` nicht als Principal registriert werden kann.
- `security::tests::admin_raw_read_is_independent_of_raw_history_and_role_names`: GM hat `RawHistoryRead`, aber kein `AdminRawRead`; Player erhält keines von beiden.
- `security::tests::duplicate_rules_and_invalid_symbols_are_rejected`: doppelte Regeln und ungültige Symbole werden abgewiesen.
- `security::tests::initial_roles_are_plain_symbols_with_explicit_bundles`: Standard-GM hat exakt 68 Regeln, Player ist leer.
- `cargo test --locked --workspace`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 sichtbarer `ci-matrix`-SKIP aus M0-14, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS; 236 Tasks, 11 Milestones, 253 Invarianten, 169 Folgebeleg-Paare.
- `git diff --check HEAD`: PASS. Die ausgegebenen LF/CRLF-Hinweise stammen aus der Windows-Arbeitskopie.

## Abgrenzung

M3-03 legt Policy-Daten fest, wertet sie aber noch nicht aus. Deny-Vorrang, Scope-Matching, Principal-/Role-Union und Non-Interference an Query-/Write-Grenzen werden in M3-04 und folgenden Security-Aufgaben ausgeführt und getestet. M3-07 bindet Security in den vollständigen QueryContext. Es existiert kein GitHub- oder Remote-Gate.
