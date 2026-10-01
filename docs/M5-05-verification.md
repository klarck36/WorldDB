# M5-05 – Persistente Idempotenz

**Ergebnis:** DONE auf Windows; lokale Vertragsabnahme am 2026-10-01.

## Umsetzung

Der persistente OperationId-Index wird aus allen validierten WAL-Segmenten rekonstruiert. Ein vollständiger Scan prüft zuerst Segmentfolge, Frames und Commit-Hashkette und bildet dann einen `BTreeMap` von `OperationId` zu BLAKE3-Fingerprint der exakten kanonischen Payloadbytes und Status. Der Index selbst braucht keine zweite persistente Datei; die WAL-Prepare- und Commitmarker sind seine rekonstruierbare Quelle.

`operation_status` meldet `Committed` mit dem Original-`WalCommitReceipt`, `Indeterminate` für ein vorhandenes Prepare ohne verifizierten Commitmarker und `NotCommitted` nur, wenn der vollständige gültige WAL-Scan keine Prepare-OperationId enthält. Mehrere WAL-Einträge mit derselben OperationId werden fail-closed als `DuplicateOperationId` abgewiesen.

`commit_operation` hängt für eine neue Operation ein gesynctes Prepare und den Commitmarker an. Eine Wiederholung mit gleicher OperationId und gleicher Payload liefert nach erneuter Indexrekonstruktion exakt denselben Receipt ohne zusätzliche WAL-Bytes. Andere Payloadbytes ergeben `IdempotencyMismatch`. Bereits vorhandene uncommitted Prepares bleiben `Indeterminate`; weder `commit_operation` noch der niedrigere `append_prepare`-Pfad führen sie erneut aus.

Nach einem fehlgeschlagenen Commitmarker-Append oder zweiten Sync speichert eine prozessweite, flüchtige Unsicherheitsmarkierung Datenbankroot, OperationId und Payloadfingerprint. Solange dieser Prozess läuft, bleibt der Status `Indeterminate`, selbst wenn ein vollständiger Marker bereits lesbar ist; gleiches Payload-Replay bleibt blockiert, abweichendes Payload wird weiterhin als Mismatch abgewiesen. Diese Markierung verhindert falschen Erfolg im laufenden Prozess. Restart-Recovery und Beweis, wann ein solcher Marker nach einem Neustart committed ist, gehören zu M5-12/M5-22 und sind hier nicht nachgewiesen.

## Windows-Nachweise

- Neuer WAL-Index meldet eine fehlende OperationId als `NotCommitted`; nach Commit liefert ein neuer `WalPrepareLog`-Wert aus denselben Dateien den ursprünglichen Receipt.
- Gleiche OperationId und gleiche Payload liefern bitgleiche Receipt-Felder und ändern die WAL-Dateilänge nicht.
- Geänderte Payloadbytes ergeben `IdempotencyMismatch`; direkte Prepare-Duplikate einer committed Operation werden ebenfalls abgewiesen.
- Ein synced Prepare ohne Marker bleibt nach Neuaufbau des Logobjekts `Indeterminate`; Replay fügt keine WAL-Bytes hinzu.
- Zweiter-Sync-Fault: Obwohl der Marker strukturell lesbar ist, bleibt der Status im laufenden Prozess `Indeterminate`; Wiederholung mit gleicher Payload wird blockiert, abweichende Payload abgewiesen.
- `cargo test --locked --workspace`: 396 Core-, 17 Storage-File- (5 Unit-, 12 Integrationstests), 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; zwei CPU-Langläufe bleiben absichtlich ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` und `cargo fmt --all -- --check`: bestanden.
- Plancheck und Sourcecheck: jeweils 242 Tasks, 11 Milestones, 253 Invarianten und 170 Folgepaare strukturell gültig.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. M0-13-Evidenzlauf `M0-13-20261001T123400Z-2366e89949` ist PASS.
- `git diff --check HEAD`: bestanden.

## Abgrenzung

Nur Windows wurde in dieser Task ausgeführt. Linux/ext4- und macOS/APFS-Tests bleiben für spätere Plattformaufgaben zurückgestellt. Das erneute Erzeugen eines Logobjekts belegt WAL-Rekonstruktion, aber keinen Prozess-/Maschinenneustart. Es gibt keinen separat materialisierten Dedupindex; die vollständige WAL-Suche ist derzeit die autoritative Abfrage. GitHub wurde nicht verwendet.
