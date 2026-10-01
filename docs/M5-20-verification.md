# M5-20 – SQLite-Referenzbackend

**Ergebnis:** lokal unter Windows geprüft am 1. Oktober 2026 mit Rust 1.85.0. Der Adapter ist ein optionaler Test-/Prototyp-Baustein und weder WorldDB-1.0-Dateiformat noch Maschinen-Durability-Nachweis.

## Umsetzung

`worlddb-testkit` bietet hinter dem nicht standardmäßig aktivierten Feature `sqlite-reference` ein In-Memory-SQLite-Backend für `RevisionBackend<u8>`. Es implementiert nicht den versiegelten produktiven `StorageBackend`. Der Werteumfang ist bewusst klein; Segmentdateien, WAL, Schemas und persistierte Domain-Records werden hier nicht abgebildet.

SQLite speichert Revisionsnummern als acht Byte große Big-Endian-BLOBs. SQLite sortiert gleich lange BLOBs lexikographisch, wodurch der vollständige gültige `u64`-Revisionsraum korrekt geordnet bleibt, einschließlich Werte oberhalb von `i64::MAX`; `u64::MAX` bleibt wie im Core reserviert. Ein erfolgreicher Publish schreibt Batch und Head innerhalb derselben SQL-Transaktion. Die Abbruch-API beginnt ihren Commitpoint unmittelbar vor dem SQLite-Commit. Historische Reads prüfen Head und sämtliche geordneten SQL-Zeilen gegen den Referenzcache, bevor sie Werte zurückgeben.

## Windows-Verträge

- `sqlite_reference_satisfies_the_revision_backend_contract` – der Adapter erfüllt denselben Contract wie das Core-Referenzbackend.
- `sqlite_and_in_memory_backends_return_identical_logical_history` – identische Batches (einschließlich leerem Commit) erzeugen dieselben Revisionen und historische Antworten an jedem geprüften Zeitpunkt.
- `sql_constraint_failure_rolls_back_the_entire_batch_and_head` – eine injizierte SQL-Constraint-Verletzung veröffentlicht weder Teilbatch noch neuen Head; ein Retry kann dieselbe nächste Revision verwenden.
- `cancellation_before_commitpoint_publishes_nothing` und `cancellable_publish_enters_and_completes_the_sql_commitpoint` – Abbruch vor dem Commitpoint ändert nichts; ein akzeptierter Commitpoint endet mit vollständigem Batch und bestätigtem Status.
- `historical_read_fails_when_sqlite_and_cache_disagree` – geänderte SQL-Nutzdaten werden vor Rückgabe als Backendfehler erkannt.
- `revision_order_and_publish_cross_the_signed_sqlite_integer_boundary` – Big-Endian-BLOBs halten die Reihenfolge und Veröffentlichung über `i64::MAX` hinweg.

## Prüfungen

- `cargo test --locked -p worlddb-testkit --all-targets --features sqlite-reference`: 19 bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace --quiet`: 593 bestanden, 2 absichtlich ignoriert, 0 fehlgeschlagen.
- `cargo clippy --locked -p worlddb-testkit --all-targets --features sqlite-reference -- -D warnings`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo deny --config .cargo/deny.toml --workspace --locked check all`: PASS; SQLite-/Zlib-Lizenz, exakte Default-Feature-Ausnahmen und eng begrenzte Quellskript-Ausnahmen dokumentiert.
- `tools/check_dependency_policy.py`, `tools/check_crate_graph.py` und `tools/test_crate_graph.py`: PASS; SQLite bleibt optional und nur im Testkit referenziert.
- `cargo xtask verify`: 33 PASS, 1 erwarteter M0-14-CI-Matrix-SKIP, 0 FAIL; `testkit-evidence` Run `M0-13-20261001T200124Z-3460e1502c` PASS.
- Plancheck, Sourcecheck und `git diff --check HEAD`: PASS.

Linux und macOS wurden gemäß Arbeitsvorgabe nicht getestet und bleiben spätere Plattformnachweise. Es wird keine Maschinen- oder Power-Loss-Durability behauptet. GitHub wurde nicht verwendet.
