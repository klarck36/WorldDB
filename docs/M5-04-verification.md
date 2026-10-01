# M5-04 – Commitmarker und Commitpoint

**Ergebnis:** DONE auf Windows; lokale Vertragsabnahme am 2026-10-01.

## Umsetzung

`WalPrepareLog::commit_prepared` schreibt einen Commitmarker nur für ein vollständig gesynctes Prepare, dessen Referenz der Aufrufer erst nach erfolgreichem erstem `File::sync_all` erhält. Der Marker liegt unmittelbar hinter seinem Prepare und bindet Segment, Offset, Frame-Länge, `OperationId`, Commitrevision, Payloadhash, vorherigen Commithash und neuen Commithash. Ein älteres Prepare, auf das später bereits ein weiterer WAL-Frame folgte, kann nicht mehr committed werden.

Der Payloadhash ist BLAKE3 über die exakten Prepare-Payloadbytes. Die Commit-Hashkette verwendet den BLAKE3-Derive-Key-Kontext `worlddb.wal.commit-chain.v1` und die kanonische Konkatenation aus `segment_sequence`, `prepare_offset`, `prepare_frame_length`, `revision` (jeweils little-endian `u64`), 16-byte-`OperationId`, 32-byte-Payloadhash und 32-byte-vorherigem Commithash. Damit bindet der Commitdigest auch die konkrete WAL-Lineage des Prepare. Genesis ist Revision 0 mit einem Nullhash; der erste Commit erhält Revision 1. Der Scanner prüft lückenlose Revisionen, Payloadhash, Vorgängerhash und Commitdigest.

Der zweite `File::sync_all` nach Append des Commitmarkers ist der einzige Commitpoint. `WalCommitReceipt` wird erst nach erfolgreichem Sync zurückgegeben. Fehler beim Marker-Append oder beim zweiten Sync ergeben `UnknownCommitOutcome` samt `OperationId`; ein möglicher Marker wird nicht fälschlich als sicherer Erfolg quittiert. `read_segment` validiert Marker-/Prepare-Paarung und klassifiziert Einträge; `commit_head` verifiziert zusätzlich die komplette Hashkette.

## Windows-Nachweise

- Zwei Commits ergeben Revisionen 1 und 2; der zweite Commit verweist auf den Hash des ersten. Der Head entspricht anschließend Revision 2 und dem letzten Commitdigest.
- Payloadbytes, OperationIds, Prepare-Referenzen und Commitreceipts werden exakt zurückgelesen; ein Prepare ohne Marker bleibt `PreparedUncommitted`.
- Ein nicht mehr am WAL-Segmentende liegendes Prepare wird vor dem Marker-Append als `PrepareNotAppendTail` abgewiesen; der Commithead bleibt unverändert.
- Fault-Hook beim zweiten WAL-Sync bestätigt, dass der Marker bereits geschrieben ist; der injizierte Syncfehler liefert `UnknownCommitOutcome` und keinen Erfolgsreceipt.
- `cargo test --locked --workspace`: 396 Core-, 15 Storage-File- (5 Unit-, 10 Integrationstests), 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; zwei CPU-Langläufe bleiben absichtlich ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: bestanden.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. M0-13-Evidenzlauf `M0-13-20261001T121256Z-87317afac2` ist PASS.
- Plancheck, Sourcecheck, Formatcheck, Policychecks und `git diff --check HEAD`: bestanden.

## Abgrenzung

Nur Windows wurde in dieser Task ausgeführt. Linux/ext4- und macOS/APFS-Tests bleiben für die späteren Plattformaufgaben zurückgestellt. Der lokale Sync-Fault-Hook belegt die Commitantwortgrenze im Codepfad, aber keine Machine-Durability bei Stromausfall oder Kernel-/Gerätefehlern. Restart-Recovery, sicherer Präfix und OperationStatus folgen M5-12/M5-22; GitHub wurde nicht verwendet.
