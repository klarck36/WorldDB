# M4-02 – Single-Writer-Koordinator

**Status:** DONE, lokal verifiziert am 30. September 2026.

## Ergebnis

`spawn_single_writer` startet genau einen benannten Worker, der den Enginezustand in seiner `FnMut`-Closure besitzt. `DatabaseHandle` exponiert nur Submission und Antwortempfang; gemeinsam genutzt wird `Arc<DatabaseHandle>`, nicht eine klonbare Engine oder ein öffentlicher `Arc<Mutex<WorldDb>>`. Die Mailbox ist begrenzt und bremst Sender bei voller Queue.

`SnapshotPublisher` tauscht unter einem kleinen `RwLock` genau einen `Arc`-Zeiger auf den vollständigen unveränderlichen Wert aus. `read()` klont diesen Zeiger und gibt den Lock frei, bevor der Aufrufer den Snapshot nutzt. Dadurch bleiben alte Leserwerte stabil, während eine neue Publikation stattfindet. Poisoning des Pointerlocks wird als Fehler gemeldet.

Die Contract-Tests starten acht gleichzeitige Producer mit insgesamt 256 Aufträgen und weisen nach, dass höchstens ein Auftrag zur selben Zeit im Writer läuft und alle Aufträge verarbeitet werden. Weitere acht Leser halten je einen alten Snapshot, während der neue Snapshot publiziert wird; jeder Leser behält danach seinen alten vollständigen Wert. Ein Rustdoc-Compile-Fail-Test verhindert das direkte Klonen der `DatabaseHandle`-Ressource.

## Nachweise

- `crates/worlddb-core/src/writer.rs`
- `writer::tests::cloned_handles_submit_to_one_sequential_writer`
- `writer::tests::concurrent_snapshot_readers_hold_old_values_without_blocking_publish`
- Rustdoc-Compile-Fail-Test auf direktem `DatabaseHandle`-Clone.
- `cargo test --locked --workspace`: 304 Core-, 7 Testkit-, 2 Backend-Contract- und 76 Rustdoc-Tests bestanden; zwei bewusst manuell auszuführende Langläufe ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.

Die Snapshot-Referenz ist noch kein vollständiger `SnapshotLease` mit Security-/Schema-/HistorySpace-Pins oder Lebenszeitbudgets. Das folgt M4-10. Backendpublikation und Machine-Durability bleiben durch ihre getrennten Verträge beschrieben.
