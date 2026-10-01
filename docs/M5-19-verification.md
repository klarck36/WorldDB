# M5-19 – Audit-Recovery und Retention

**Ergebnis:** DONE – am 1. Oktober 2026 lokal unter Windows geprüft. Linux/macOS und die Fault-/Crashmatrix M5-22 bleiben zurückgestellt.

## Umsetzung

`RawReadAuditWal` prüft und recovered einen unvollständigen oder nicht committeten WAL-Tail, bevor ein Writer für Admin-Raw-Freigaben bereitsteht oder ein autorisierter Lesezugriff die Historie erhält. Die Recovery bewahrt den verworfenen Tail zuerst in `audit/wal/quarantine`, synchronisiert danach ein Digest-gebundenes Intent, kürzt und synchronisiert die aktive WAL auf den vollständig verifizierten Commitpräfix und veröffentlicht ein Done-Marker. Ein Neustart vervollständigt ein vorhandenes Intent idempotent. Frames innerhalb des committed Präfixes werden nicht repariert; ihr Fehler bleibt fail-closed und die aktiven WAL-Bytes bleiben unverändert.

Committed Raw-Read-Attempts sind über öffentliche Storage-APIs nur mit `AuditRead` lesbar. Export verlangt zusätzlich `AuditExport`; ein Exportgrant allein genügt nicht. Der Writer veröffentlicht keine ungefilterte Audit-Historie. Änderungen an `AuditRetentionPolicy` verlangen `AuditConfigure`; die Validierung weist eine kürzere konfigurierte Mindestfrist und ein Recordlimit unterhalb des derzeit gespeicherten Bestands ab. Autorisierte Änderungen werden über die bereits implementierte SecurityPolicy-Historie versioniert und gemeinsam mit ihrem Required Audit Record im Shared-WAL-Commit gespeichert (M5-06a/M5-17).

Auditsequenz und WAL bleiben ein eigener Wasserstand. Tail-Recovery und Raw-Read-Audit legen weder `CURRENT` noch Daten-WAL-Commits an.

## Windows-Verträge

- `startup_quarantines_a_complete_uncommitted_prepare_before_opening_the_writer` – Start-Recovery sichert einen vollständigen uncommitteten Prepare-Tail, behält die letzte Auditsequenz und setzt danach mit der nächsten Sequenz fort.
- `startup_finishes_an_interrupted_journaled_tail_recovery` – Recovery nach persistiertem Intent und vor dem Done-Marker wird beim nächsten Öffnen beendet; der committed Präfix bleibt lesbar.
- `committed_prefix_corruption_is_read_only_and_byte_preserving` – Writer, autorisiertes Audit-Lesen und explizites Recover scheitern an committed Korruption; WAL-Bytes und Quarantäne bleiben unangetastet.
- `audit_read_export_and_retention_configuration_require_distinct_current_capabilities` – AuditRead, AuditExport und AuditConfigure werden getrennt ausgewertet; die Mindestfrist kann nicht verkürzt und das Recordlimit nicht unter den aktuellen Bestand gesenkt werden.
- M5-18-Regression: fünf Verträge bestanden weiterhin; Historie wird nur über autorisierte Lese-/Export-APIs gelesen.
- M5-17-Integration: `policy_and_audit_configuration_changes_recover_with_their_required_audits` belegt persistierte Policy-/Retentionversionen und zugehörige Required Audit Records nach Recovery.

## Prüfungen

- `cargo test --locked -p worlddb-storage-file --all-targets`: 93 bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace --quiet`: 593 bestanden, 2 bewusst ignoriert, 0 fehlgeschlagen.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL; Plancheck und Sourcecheck PASS.
- M0-13-Lauf `M0-13-20261001T192456Z-f657245513`: PASS.
- `git diff --check HEAD`: PASS.

Linux/macOS wurden nicht ausgeführt. GitHub wurde nicht verwendet. Die Windows-Läufe behaupten keine Maschinen-/Power-Loss-Durability; dafür bleibt M5-22 maßgeblich.
