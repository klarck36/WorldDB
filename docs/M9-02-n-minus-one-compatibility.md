# M9-02 – N-1-Kompatibilität

**Status:** DONE
**Abnahme:** 6. Oktober 2026, Windows
**N-1-Entscheid:** NOT APPLICABLE – es wurde noch keine Alpha veröffentlicht.

## Veröffentlichungsstand

Am Prüftag listete die öffentliche
[GitHub-Releases-Seite](https://github.com/klarck36/WorldDB/releases) keine
Releases. `git ls-remote --tags origin` lieferte ebenfalls keine Tags. Es gibt
damit keine veröffentlichte Alpha-Version, deren N-1-Dateien geöffnet oder
migriert werden könnten. N-1 ist ausdrücklich **nicht anwendbar**, kein
Kompatibilitäts-Pass. Nach Veröffentlichung der ersten Alpha muss diese
Abnahme erneut laufen.

## Vor-Alpha-Fixtures gegen den aktuellen Stand

Der versionierte M7-16h-Korpus hat 24 Dateien mit 15.553 Bytes. Das aktuelle
Manifest ist an BLAKE3
`e055937e2cc6a7613fc9d8c40a5a2e6ea9e18910c727d4eb6b3fcaa99cadea33` gebunden.
Der gezielte Lauf
`cargo test --locked -p worlddb-storage-file --test m7_16h_fixture_baseline -- --exact versioned_pre_alpha_fixture_baseline --nocapture`
bestand. Er öffnet und verifiziert die Datenbank-Fixture, restauriert das
ExactDatabaseBackup als neue DatabaseId mit gebundenem Required Audit, liest
und reproduziert den Logical Export v2 bytegenau, prüft den Sharing Export und
führt die kanonische Restrictive-Zwei-Schritt-Migration bis zu Revision 4 aus.
Die Migration öffnet ihr Completed-Journal wieder und bestätigt beide Required
Audits sowie das erwartete Logical-Export-Golden.

Das M7-16h-Dokument und die M7-16i-Matrix enthielten noch den Manifestdigest
vor dem M8-14a Logical-Export-v2-Refresh. Beide Nachweise und die
Projektstatusübersicht wurden auf den aktuellen Digest korrigiert. Der
physische Storage-Fixture blieb bytegleich.

Da `AuditCompleteBackup` unterstützt wird, wurden zusätzlich die Profil- und
Restoreverträge gezielt geprüft:

- `m7_09_audit_complete_backup_contract`: 3 Tests bestanden; Exact- und
  AuditComplete-Profile sowie getrennte Daten-/Audit-Wasserstände geprüft.
- `m7_10_restore_contract`: 4 Tests bestanden; AuditComplete-Restore erhält
  getrennte Auditlineage und Exact-Restore erzeugt den gebundenen
  RestorePublication-Auditbeleg.

## Ergebnis und Folgearbeit

Die versionierten Vor-Alpha-Fixtures bleiben mit dem aktuellen Windows-Build
lesbar und reproduzierbar. Für veröffentlichte Vorgängerversionen ist M9-02
nicht anwendbar, da noch keine Alpha-Releases oder Tags existieren. Die native
macOS-/Linux-Abnahme bleibt gemäß Nutzervorgabe M9-07 zugeordnet. Nach der
ersten veröffentlichten Alpha wird M9-02 um deren konkrete Fixtures erweitert
und als echte N-1-Kompatibilitätsrunde wiederholt.
