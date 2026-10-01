# M5-13 – Korruptionsmodus und Read-only-Enforcement

**Ergebnis:** DONE für die lokale Windows-Abnahme. Eine sichere Verifikation klassifiziert den Datenbankzustand als `Clean`, `RecoveryRequired` oder `QuarantinedReadOnly`. Beschädigte referenzierte History wird nicht automatisch repariert und kann über die Storage-Schreib-APIs nicht weiter verändert werden. Linux- und macOS-Nachweise sind zurückgestellt.

## Implementierung

- `RecoveryScanner` prüft den WAL-Präfix, `CURRENT`, das aktuelle Manifest und dessen History- sowie SecurityPolicy-Segmente. Außerdem prüft er die Segmentreferenzen des neuesten committed typisierten Snapshots, einschließlich noch nicht final publizierter staged Segmente.
- Ein fehlender oder zurückliegender `CURRENT` nach einem gültigen committed Snapshot ist `RecoveryRequired`: normale Schreibwege bleiben gesperrt, bis `RecoveryManager` den Snapshot kontrolliert materialisiert und erneut verifiziert hat.
- Beschädigte Segmentreferenzen, ein malformed committed Replay-Payload oder ein Manifest mit abweichendem Segmentbestand bei derselben Revision führen zu `QuarantinedReadOnly`. `RecoveryManager` beendet sich dann vor jeder Reparatur oder Publikation.
- `DatabaseLayout::try_writer_lock` führt den Scan vor der Freigabe des Locks aus. Der Lock erlaubt normale Änderungen erst nach einem sauberen Scan. WAL-, History-, Security-, Manifest- und Formatänderungen prüfen dieses Schreibrecht erneut. Eine unterbrochene Recovery lässt das Schreibrecht gesperrt; nur eine erfolgreich beendete Recovery öffnet es wieder.
- `RecoveryReport` bietet eine knappe, payloadfreie Textdarstellung mit Einstufung, `safe_revision` und Befunden. Der Scanner schreibt oder kürzt keine Datenbankdateien. Beschädigte Originalsegmente bleiben bytegleich erhalten; es wird keine automatische Kopie sicherer Korruption angelegt.

## Vertragsprüfungen

- `corrupted_referenced_history_is_quarantined_read_only_and_cannot_be_written` – beschädigte History- und SecurityPolicy-Segmente werden erkannt; WAL-, History-, Policy-, Manifest- und Format-Schreibversuche werden gesperrt; Recovery lässt alle beschädigten Bytes unverändert.
- `same_revision_manifest_inventory_mismatch_is_never_repaired` – ein checksumgültiges Manifest mit vom committed Snapshot abweichender Referenzliste wird quarantänisiert, `CURRENT` wird nicht verändert und Recovery repariert es nicht.
- `recoverable_tail_blocks_writes_until_recovery_finishes` – ein uncommitted WAL-Tail blockiert neue Writes; erst nach erfolgreicher Tail-Recovery werden sie wieder zugelassen.

## Prüfergebnisse auf Windows

- `cargo test --locked -p worlddb-storage-file` – **PASS**, 59 Tests bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace` – **PASS**, 559 Tests bestanden, 0 fehlgeschlagen, 2 lang laufende Präzisions-/Fuzzkampagnen ignoriert.
- `cargo clippy --locked -p worlddb-storage-file --all-targets -- -D warnings` – **PASS**.
- `cargo fmt --all -- --check` – **PASS**.
- `cargo xtask verify` – **PASS**, 32 Prüfschritte bestanden, 1 vorgesehener M0-14-`SKIP`, 0 Fehler. M0-13-Lauf `M0-13-20261001T161357Z-109dc13592` – **PASS**.

## Grenzen

`QuarantinedReadOnly` ist eine Schreibsperre plus Verify-Befund; M5-13 verschiebt beschädigte Originale nicht in ein anderes Verzeichnis und erstellt keinen Salvage-Fork. Salvage folgt M5-14. Der Schreibschutz gilt für die Datei-Storage-APIs unter dem gehaltenen WorldDB-Lock; er ist keine Betriebssystem-Sandbox gegen externe Programme, die Dateien außerhalb dieses Protokolls direkt verändern. Es wurden ausschließlich Prüfungen auf Windows ausgeführt.
