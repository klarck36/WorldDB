# M5-11 – Recovery-Scanner

**Ergebnis:** DONE für die lokale Windows-Abnahme. Der Scanner liest `CURRENT`, dessen Manifestgeneration und das sequenzierte WAL unter gehaltenem Datenbank-Writerlock. Er verändert weder den Pointer noch Manifest-, WAL- oder Quarantänedateien. Linux/ext4- und macOS/APFS-Prüfungen wurden gemäß Anweisung nicht ausgeführt.

## Implementierung

- `RecoveryScanner` liefert `safe_revision` und den zugehörigen Commit-Hash ausschließlich bis zum letzten vollständigen, korrekt gehashten Commitmarker.
- Die WAL-Segmentfolge wird auf fortlaufende Nummerierung und in-root reguläre Dateien geprüft. Begrenzte Dateilesevorgänge und frameweise Prefixdecodierung erhalten den sicheren Commitstand vor einem fehlerhaften Frame.
- Der Scanner prüft Framing und Framechecksumme, Prepare-/Commitmarker-Verknüpfung, OperationId-Eindeutigkeit, Payloaddigest, fortlaufende Revision, vorherigen Commit-Hash und den kanonischen Commit-Hash.
- `CURRENT` und die referenzierte Manifestgeneration werden strukturell und über ihren Pointerdigest geprüft. Zusätzlich muss der Manifeststand höchstens `safe_revision` sein und seinen Commit-Hash aus genau dieser WAL-Revision binden.
- `RecoveryFinding` trennt `TornTail`, `UncommittedTail` und `SafeCorruption`. Ein ungültiges `CURRENT`, ein Manifest oberhalb des sicheren Präfixes und ein abweichender Manifest-Commit-Hash werden ebenfalls ausdrücklich gemeldet.
- Ein vollständiger Prepare ohne Marker, eine unvollständige Framefolge und checksum- oder hashkettenwidrige Daten werden nicht als Commit gezählt. Bei Korruption wird nur der bereits vorher vollständig verifizierte Präfix gemeldet.

## Vertragsprüfungen

- `an_empty_database_scans_as_clean_genesis_without_a_manifest` – leere Datenbank bleibt bei Genesis und meldet den erwarteten fehlenden Pointer.
- `a_current_manifest_binds_to_the_verified_wal_commit_prefix` – gültiger Pointer, Manifestdigest und Commit-Hash werden an denselben WAL-Stand gebunden.
- `a_complete_uncommitted_prepare_is_reported_without_advancing_safe_revision` – Prepare ohne Marker bleibt außerhalb des sicheren Präfixes.
- `a_torn_tail_is_distinguished_and_keeps_only_prior_commits_safe` – unvollständiger Frame wird als Torn Tail klassifiziert; ein vorheriger Prepare ohne Marker bleibt kenntlich.
- `complete_corruption_stops_at_the_prior_verified_commit` – beschädigter vollständiger Commitmarker stoppt nach dem vorherigen verifizierten Commit; die WAL-Datei bleibt unverändert.
- `a_sequence_gap_is_safe_corruption_after_the_contiguous_prefix` – Segmentlücke wird als sichere Korruption klassifiziert, ohne den vorigen Commitstand zu verwerfen.
- `a_corrupt_current_pointer_does_not_inflate_the_safe_wal_revision` – beschädigtes `CURRENT` wird gemeldet und nicht repariert.
- `a_manifest_ahead_of_a_corrupt_wal_prefix_is_reported_as_untrusted` – eine noch lesbare Manifestgeneration oberhalb des sicheren WAL-Präfixes wird ausdrücklich als nicht vertrauenswürdig markiert.
- `wal::tests::recovery_scan_stops_at_a_checksum_valid_commit_chain_mismatch` – absichtlich neu gechecksumter, aber falsch verketteter Commitmarker wird als Commitkettenkorruption erkannt.

## Prüfergebnisse

- `cargo test --locked -p worlddb-storage-file` – **PASS**, 52 Tests, 0 fehlgeschlagen.
- `cargo clippy --locked -p worlddb-storage-file --all-targets -- -D warnings` – **PASS**.
- `cargo fmt --all -- --check` – **PASS**.
- `cargo test --locked --workspace` – **PASS**, 552 Tests bestanden, 0 fehlgeschlagen, 2 lang laufende Präzisions-/Fuzzkampagnen bewusst ignoriert.
- `cargo xtask verify` – **PASS**, 32 Prüfschritte bestanden, 1 vorgesehener M0-14-`SKIP`, 0 Fehler; M0-13-Lauf `M0-13-20261001T145722Z-16668e997d` mit Status PASS.

## Grenzen

Der Scanner ist eine read-only Präfixbestimmung, kein Replay, kein Quarantäne-/Truncation-Protokoll und keine Reparatur. Idempotentes Replay und ein Recovery-Journal folgen M5-12; der Korruptions-/Read-only-Modus folgt M5-13. Vollständige Crash- und Hardwaredurabilitätsaussagen folgen M5-22/M5-23. Linux/ext4 und macOS/APFS wurden nicht geprüft.
