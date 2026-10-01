# M4-01 – In-Memory-Backendvertrag

**Status:** DONE, lokal verifiziert am 30. September 2026.

## Ergebnis

`worlddb_core::RevisionBackend<T>` trennt den Engine-Auftrag vom Backend: Die Engine verantwortet Validierung und Autorisierung; das Backend weist einem vollständigen Batch genau eine Revision zu und liest nur publizierte Historie. `InMemoryRevisionBackend<T>` ist der synchrone, nicht persistente Modelladapter über dem bestehenden indexfreien Revisionslog. Er behauptet weder Crash-Durability noch Concurrent-Reader-Koordination; diese Eigenschaften gehören zu späteren Aufgaben.

`worlddb_testkit::backend_contract::assert_revision_backend_contract` ist als gemeinsamer Contract-Helfer verfügbar. Er prüft Genesis, Sichtbarkeit nur publizierter Revisionen, fortlaufende Revisionszuweisung, einen vollständigen Batch unter einer Revision sowie unveränderte historische Reads. Ein getrenntes Engine-Fixture weist nach, dass ein Engine-Validierungsfehler den Backendaufruf und damit die Publikation verhindert.

## Nachweise

- `crates/worlddb-core/src/revision_backend.rs`
- `crates/worlddb-testkit/src/backend_contract.rs`
- `crates/worlddb-testkit/tests/revision_backend_contract.rs`
- `cargo test --locked --workspace`: 302 Core-, 7 Testkit-, 2 Backend-Contract- und 75 Rustdoc-Tests bestanden; zwei bewusst manuell auszuführende Langläufe ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: durch `cargo xtask verify` PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.

Die zu WDB-STO-001/002 gehörigen normativen Primärnachweise bleiben M5-01 zugeordnet. Dieser M4-01-Nachweis belegt den In-Memory-Modellvertrag, nicht Machine-Durability eines Produktionsbackends.
