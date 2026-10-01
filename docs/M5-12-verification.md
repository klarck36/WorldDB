# M5-12 – Idempotentes Replay und Journal

**Ergebnis:** DONE für die lokale Windows-/NTFS-Abnahme. Der Recovery-Manager setzt unterbrochene Tail-Reparaturen nach erneutem Öffnen fort und spielt typisierte WAL-Manifest-Snapshots idempotent wieder ein. Linux- und macOS-Nachweise sind zurückgestellt.

## Implementierung

- `RecoveryJournal` protokolliert feste, gechecksumte Append-only-Einträge in der Reihenfolge `Intent → Quarantined → Truncated`. Ein unvollständiger letzter Journaleintrag wird auf den gültigen Präfix zurückgeführt; vollständige Checksum- oder Übergangskorruption führt fail-closed zum Fehler.
- Vor Tail-Reparatur prüft `RecoveryManager` den read-only-Scan. Sichere Korruption wird abgewiesen und nicht verändert. Bei einem reparierbaren Tail werden Absicht und Quarantänekopie dauerhaft geschrieben, die Kopie anhand Länge und Digest geprüft und erst danach die WAL auf den sicheren Offset gekürzt.
- Eine Unterbrechung zwischen Journalphasen wird beim nächsten Aufruf fortgesetzt. Wiederholte Recovery erzeugt keine zweite Quarantänekopie und ändert eine bereits reparierte WAL nicht erneut.
- Typisierte `commit_manifest_snapshot`-WAL-Einträge tragen Manifestreferenzen und die Menge der neu gestagten Segmente. Replay validiert und publiziert unveränderliche History- und Policysegmente vor Manifest und `CURRENT`; der WAL bleibt die Quelle für Revision und Receipt. Wiederholte `OperationId` mit identischem Payload liefert dasselbe Receipt, abweichender Payload wird abgewiesen.
- Generische opake WAL-Payloads werden nicht als fachliche Manifest-Snapshots interpretiert. Ein echter Prozessabbruch, Stromausfall, Datenträgerfehler und plattformübergreifende Durabilitätsmessungen sind damit nicht nachgewiesen.

## Vertragsprüfungen

- `nested_restart_after_each_tail_checkpoint_is_idempotent` – Unterbrechung nach jedem der fünf Journal-/Tail-Prüfpunkte, erneutes Öffnen, genau eine Quarantänekopie, korrekte Journalfolge und unveränderte zweite Recovery.
- `altered_quarantine_copy_stops_before_wal_truncation` – veränderte Quarantäne wird erkannt; die ursprüngliche WAL-Länge bleibt erhalten.
- `committed_staged_history_replays_manifest_and_operation_receipt_after_restart` – staged History- und Securitysegmente, TransferLineage, LayerSchemaSnapshot, SecurityPolicyRecord und SecurityEpoch werden nach erneutem Öffnen wiederhergestellt; Receipts überstehen den Neustart; Payloadabweichung wird abgewiesen.
- `safe_area_corruption_is_never_repaired_by_replay` – Beschädigung eines bereits sicheren Commitmarkers wird weder repariert noch verändert.

## Prüfergebnisse auf Windows

- `cargo test --locked -p worlddb-storage-file` – **PASS**, 56 Tests bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace` – **PASS**, 556 Tests bestanden, 0 fehlgeschlagen, 2 lang laufende Präzisions-/Fuzzkampagnen ignoriert.
- `cargo clippy --locked -p worlddb-storage-file --all-targets -- -D warnings` – **PASS**.
- `cargo fmt --all -- --check` – **PASS**.
- `cargo xtask verify` – **PASS**, 32 Prüfschritte bestanden, 1 vorgesehener M0-14-`SKIP`, 0 Fehler. M0-13-Lauf `M0-13-20261001T155046Z-914b886bdf` – **PASS**.

## Grenzen

Die Restart-Vertragsprüfung öffnet Manager, Writer-Lock und Datenbank erneut innerhalb des Testprozesses; sie simuliert keinen Betriebssystem-Prozessabbruch oder Stromausfall. Die vollständige Crash-/Durabilitätsmatrix folgt M5-22/M5-23. Es wurden ausschließlich Prüfungen auf Windows ausgeführt; Linux/macOS werden später separat geprüft.
