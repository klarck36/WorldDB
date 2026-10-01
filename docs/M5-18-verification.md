# M5-18 – Separates Raw-Read-Audit-WAL

**Ergebnis:** DONE – lokal unter Windows geprüft am 1. Oktober 2026. Linux-/macOS-Prüfungen und die umfassende Crash-/Faultmatrix M5-22 bleiben zurückgestellt.

## Umsetzung

Der Datei-Adapter stellt mit `RawReadAuditWal` und `RawReadAuditWriter` einen eigenen Auditpfad unter `audit/wal/raw-read.wal` bereit. Er verwendet die unabhängige Sperre `audit/LOCK`; die Auditsequenz beginnt bei 1 und entwickelt sich ohne Datenrevision. Ein Raw-Read-Versuch enthält ausschließlich `AuditRecordId`, `AuditSequence`, `AuditOperationId`, `ClientRequestId`, Principal, Scope-Fingerprint, Snapshot, aktuelle SecurityEpoch und PageOrdinal. Nutzdaten oder eine Behauptung über tatsächlich ausgelieferte Bytes sind nicht Teil des Records.

Für jeden Versuch wird zuerst ein kanonischer Prepareframe geschrieben und synchronisiert. Danach folgt ein Commitmarker mit Payload-Hash, vorherigem Commit-Hash und Commit-Hash; der Writer antwortet erst nach dessen Sync erfolgreich. Der Core-Admin-Raw-Pfad wartet synchron auf diesen Erfolg, bevor er die Seite als `OwnedQueryResult` zurückgibt. Fehler blockieren die Freigabe. Nach einem Append-/Syncfehler wird der Writer bis zur erneuten Prüfung gesperrt. Eine unvollständige WAL-Endung wird niemals stillschweigend als committed behandelt. M5-19 quarantänisiert und recovered einen nicht committeten Tail vor dem Öffnen des Writers; Korruption im committed Präfix bleibt unverändert fail-closed.

Wiederholungen mit derselben `ClientRequestId` werden als neue Versuche gespeichert: Sequenz und `AuditOperationId` sind frisch. Ein Prozessende nach dem Commit-Sync kann als dauerhafter Versuch ohne behauptete Seitenausgabe rekonstruiert werden. Die Auditoperation verändert weder `CURRENT` noch das Daten-WAL.

## Windows-Verträge

- `durable_raw_pages_keep_retry_identity_and_use_an_independent_audit_sequence` – zwei tatsächliche Freigaben über den Core-Admin-Raw-Pfad behalten dieselbe ClientRequestId, erhalten neue AuditOperationIds und Sequenzen und binden die erwarteten Principal-, Scope-, Snapshot-, Epoch- und Seitenwerte. Die Audit-WAL-Sperre ist separat und der Dateiadapter benötigt keine neue Datenrevision.
- `raw_audit_failure_blocks_the_admin_page_before_release` – ein ungültiger Audit-WAL-Pfad lässt `release_admin_raw_page` mit `AuditAuthorization` fehlschlagen; es gibt keinen Seitenerfolg.
- `process_exit_after_attempt_sync_recovers_an_attempt_without_claimed_output` – ein Kindprozess beendet sich direkt nach erfolgreichem Sync. Nach Prozessende ist der Attempt lesbar, aber `CURRENT` wurde nicht angelegt und der Record behauptet keine Auslieferung.
- `partial_audit_tail_is_never_silently_treated_as_a_committed_attempt` – ein angeschnittener Tail wird nicht als Versuch gezählt; die M5-19-Recovery entfernt ihn erst nach gesicherter Quarantänekopie.
- `raw_read_audit_child_stops_after_durable_attempt` – isolierter Kindprozess für den abrupten Stop nach Sync.

## Prüfungen

- `cargo test --locked -p worlddb-storage-file --all-targets`: 89 bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace --quiet`: 589 bestanden, 2 bewusst ignoriert, 0 fehlgeschlagen.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-CI-Matrix-SKIP, 0 FAIL.
- Plancheck, Sourcecheck und `git diff --check HEAD`: PASS.
- M0-13-Testkit-Lauf `M0-13-20261001T185543Z-53f7ffd311`: PASS.

Linux/macOS wurden auf ausdrückliche Projektvorgabe nicht ausgeführt und bleiben Plattformnachweisen vorbehalten. Ein erfolgreicher lokaler Datei-Sync belegt keine Hardware-/Power-Loss-Durability; dafür bleibt M5-22 maßgeblich. GitHub wurde nicht verwendet.
