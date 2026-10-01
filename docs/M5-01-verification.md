# M5-01 – StorageBackend-Contract-Suite

**Status:** DONE, lokal auf Windows verifiziert am 1. Oktober 2026.

## Ergebnis

`worlddb_core::StorageBackend<T>` ist ein versiegelter interner Capability-Port über dem Revisionsvertrag. Das geschlossene Feature-Set benennt konsistente Snapshots, atomares Publish, historische Reads, Snapshot-Pins, Verify und Recovery. `ProductionStorage<B>` lässt eine Backendinstanz nur zu, wenn sie `Machine`-Durability und sämtliche Pflichtfähigkeiten meldet. Die Zulassungsprüfung akzeptiert synthetisch vollständige Machine-Profile, ist aber kein Plattformnachweis für ein reales Dateibackend.

`InMemoryRevisionBackend<T>` meldet ausschließlich das logische Referenzprofil mit `Memory`-Durability. Es meldet weder Verify noch Recovery und kann daher nicht über `ProductionStorage` für produktive Commits zugelassen werden. Sein historischer Read bleibt geliehen und die In-Memory-History wird nicht reclaimed; daraus folgt keine Aussage über dauerhafte Segmente oder Crash-Recovery.

Die Testkit-Contract-Suite prüft den vollständigen Batch auf genau einer Revision und auf unveränderte historische Reads. Ein absichtlich fehlerhafter Backendadapter, der einen Batch auf mehrere Revisionen aufteilt, wird abgewiesen. Ein Engine-Fixture weist getrennt nach, dass fachliche Validierung vor jedem Backendaufruf stattfindet und ein gültiger Batch genau einen Publish-Aufruf auslöst.

## Nachweise

- `crates/worlddb-core/src/storage_contract.rs`
- `crates/worlddb-core/src/revision_backend.rs`
- `crates/worlddb-testkit/src/backend_contract.rs`
- `crates/worlddb-testkit/tests/revision_backend_contract.rs`
- `StorageBackend`-Compile-fail-Beispiel: der Port kann nicht aus einem Drittcrate implementiert werden.
- `cargo test --locked --workspace`: 396 Core-, 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; zwei lange CPU-Kampagnen bleiben absichtlich ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. M0-13-Lauf `M0-13-20261001T105827Z-df55540f03`: PASS.
- Linux/ext4, macOS/APFS und externe CI wurden gemäß Arbeitsvorgabe nicht ausgeführt und bleiben spätere Nachweise.

## Abgrenzung

M5-01 schließt den logischen Port und seine Contract-Tests. Es gibt weiterhin kein persistentes Dateibackend und keinen realen Machine-Durability-Nachweis. Windows-Dateilayout, Format-Capabilities und Writerlock beginnen in M5-02; Plattform-Crash-, ext4-, APFS- und externe CI-Nachweise bleiben ihren späteren Aufgaben zugeordnet.
