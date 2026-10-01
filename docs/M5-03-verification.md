# M5-03 – WAL-Prepare

**Ergebnis:** DONE auf Windows; lokale Vertragsabnahme am 2026-10-01.

## Umsetzung

`WalPrepareLog` schreibt nummerierte, append-only WAL-Segmente (`segment-<20-stellige-sequenz>.wal`). Ein Prepare-Frame enthält die persistente `OperationId` und die exakten Payloadbytes in einem streng sortierten TLV-Frame mit Checksumme. Jeder Append verlangt einen Writerlock für genau denselben Datenbankroot, schreibt das Frame und ruft anschließend `File::sync_all` auf. Der Commitmarker und die Zuweisung einer Revision gehören zu M5-04.

Das Segmentziel beträgt 16 MiB; ein einzelnes Frame darf bis zur vorhandenen 64-MiB-Decodergrenze reichen und erhält dann ein eigenes Segment. Vor einem Append werden vorhandene Segmente und Frames geprüft. Unbekannte Segmentnamen, Sequenzlücken, ungültige Frames und zerrissene Tails verhindern ein weiteres Append. Readback liefert `PreparedUncommitted`, da M5-03 noch keinen Commitmarker schreibt.

## Windows-Nachweise

- Append mit OperationId und Payload, exakter Readback, Offsetfolge und expliziter Uncommitted-Klassifikation bestanden.
- Sync-Fehler-Fault-Injection wird als Fehler zurückgegeben; das vorhandene Prepare-Frame wird beim Readback weiterhin als uncommitted erkannt.
- Segmentrotation nach Überschreiten des 16-MiB-Ziels bestanden.
- Lock aus einer anderen Datenbank wird abgewiesen, bevor eine WAL-Datei entsteht.
- Zerrissener Tail verhindert ein späteres Append; die Dateilänge bleibt unverändert.
- `cargo test --locked --workspace`: 396 Core-, 13 Storage-File- (4 Unit-, 5 M5-02- und 4 M5-03-Integrationstests), 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; zwei CPU-Langläufe bleiben absichtlich ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` und `cargo fmt --all -- --check`: bestanden.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. M0-13-Evidenzlauf `M0-13-20261001T114559Z-7b82cb157b` ist PASS.
- `WorldDB_1.0_Plancheck.py`, `WorldDB_1.0_Sourcecheck.py`, Dependency-Policy, `cargo-deny`, Crategraph-Policy und `git diff --check HEAD`: bestanden.

## Abgrenzung

Nur Windows wurde in dieser Task ausgeführt. Linux/ext4 und macOS/APFS bleiben für die späteren Plattformaufgaben zurückgestellt. Der Sync-Fault-Test belegt Fehlerweitergabe und Prepare-Klassifikation, aber keine Machine-Durability. Restart-Recovery und sichere Präfixbestimmung folgen in M5-11/M5-12/M5-22.
