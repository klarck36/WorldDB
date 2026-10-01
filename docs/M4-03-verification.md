# M4-03 – Transaction-Typestate

**Status:** DONE, lokal verifiziert am 30. September 2026.

## Ergebnis

`WriteTransaction` besitzt nicht klonbares Staging und verwendet private, nicht extern konstruierbare Phasenmarker. `OpenTransaction` erlaubt Staging, konsumierende Engine-Validation und explizites Abort. Nur `ValidatedTransaction` besitzt `commit(self)`. Commit publiziert den gesamten Batch einmal über `RevisionBackend`; Drop publiziert nichts. Ein Basisstand nach dem aktuellen Head wird beim Begin abgelehnt.

Die Fehlerbehandlung unterscheidet Validierungsablehnung, expliziten Abort und den erfolgreichen Modellpublish. `TransactionState::UnknownOutcome` bleibt der Clientkenntnis nach verlorenem Commit-Reply vorbehalten; das reine In-Memory-Modell erzeugt diesen Zustand nicht und verspricht keine Persistenz. OCC-Revalidation und Commit-Status/Dedup bleiben den Folgetasks M4-06/M4-09 zugeordnet.

## Nachweise

- `crates/worlddb-core/src/transaction_flow.rs`
- `transaction_flow::tests::validation_and_commit_consume_each_state_and_publish_one_batch`
- `transaction_flow::tests::abort_validation_failure_and_drop_leave_backend_unpublished`
- `transaction_flow::tests::begin_rejects_a_base_revision_that_is_not_published`
- Drei Rustdoc-Compile-Fail-Tests: Commit vor Validation, Doppelcommit und direktes Klonen einer Transaktion.
- `cargo test --locked --workspace`: 307 Core-, 7 Testkit-, 2 Backend-Contract- und 79 Rustdoc-Tests bestanden; zwei bewusst manuell auszuführende Langläufe ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.

M4-03 prüft Drop auf dem Modellbackend. Der Crash-/Recovery-Folgebeleg für persistente Reparatur bleibt WDB-TX-002/M5-22 zugeordnet.
