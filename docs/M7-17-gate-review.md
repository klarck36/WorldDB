# M7-17 – Gate-Review

**Ergebnis:** bestanden auf Windows am 2026-10-03. Alle 38 Invarianten mit `primary_task` aus M7 sind `DONE` und haben positive wie negative Evidenz. Alle 53 Folgebeleg-Paare mit einem M7-Task sind `DONE`. Linux-/macOS-Abnahmen bleiben wie vereinbart M9-07 zugeordnet.

## Gate-Abdeckung

| Prüfbereich | Positive Evidenz | Negativ-/Crash-Evidenz | Ergebnis |
|---|---|---|---|
| Migration und Dry Run | M7-02 bis M7-06; M7-10a; `docs/M7-16c-persistent-migration-commit.md`; `docs/M7-16d-guarded-file-store-integration.md` | Versions-/Fingerprintabweichung, ungültige oder fehlende Adminentscheidungen, verweigerte Rechte, ungültige Zwischenschemata und die acht Reopen-/Resume-Grenzen aus `docs/M7-16e-migration-process-crash-matrix.md` | PASS |
| Restorepoint und Exact Backup | `docs/M7-08-verification.md`; `docs/M7-10a-verification.md`; vollständiger Restore als neue DatabaseId | Fehler beim Zielverify, beschädigte Backups, vorhandene Ziele, fehlender Breaking-Restoreproof und Prozessabbruch vor/nach atomarer Veröffentlichung; siehe M7-16a/b | PASS |
| Auditprofile und Required Audit | `docs/M7-09-verification.md`; `docs/M7-10-verification.md`; M7-10a und M7-15 | Unzulässige Auditsegmentdateien, Profil-/Watermarkabweichung, fehlende Berechtigung sowie Restore-, Migration- und Purge-Faults; sichtbare Ergebnisse besitzen den erforderlichen Auditbeleg | PASS |
| Exportumfang und Import-Remap | `docs/M7-11-verification.md`; `docs/M7-12-verification.md`; `docs/M7-13-verification.md` | Fehlende Auslassmanifestzeile, Rechteentzug, geänderte Exportbytes, Kollision ohne Remap, nicht gelistete oder belegte Ziele, fehlende Referenzen sowie typfremde Remaps | PASS |
| Offline-Purge | `docs/M7-14-verification.md`; `docs/M7-15-verification.md` | Implizite Kaskade und falsche Freigabe werden abgewiesen; Abbruch vor Auditcommit oder Publikation lässt kein Ziel sichtbar. Nach Publikation erscheint nur das vollständig verifizierte Ziel mit Required Audit. Siehe `docs/M7-16g-purge-process-crash-matrix.md`. | PASS |
| Format-/Kompatibilitätsbasis | M7-07-Formatfixtures und die versionierte 24-Dateien-Fixture aus `docs/M7-16h-versioned-fixture-baseline.md` | Unbekannte zukünftige Formate und erforderliche Fähigkeiten fail-closed; Backup, Restore, Migration, Exporte und Purge nach Prozessabbruch sind in `docs/M7-16i-fault-compatibility-matrix.md` zusammengeführt | PASS |

## Schließung der zuvor offenen Evidenz

- `WDB-MIG-012` ist geschlossen: Migrationspläne sind typisiert, kanonisch wire-kodiert, an Fingerprint und Transformer-Version gebunden. Die Core-Ausführung besitzt keinen AI-Callback. Unresolved-Entscheidungen müssen vollständig, aktuell und autorisiert sein; M7-10a konsumiert sie im guarded Commitpfad und schreibt Required Audit. Fehlender Restoreproof oder fehlende Adminaktion blockiert Breaking-Migrationen.
- `WDB-EXP-002` ist geschlossen: Ein Import benötigt einen expliziten, digestgebundenen Remap-Plan. Wiederholung derselben Quelle und Konfiguration bleibt deterministisch. Fehlender Plan bei Kollisionen, unbekannte Quellen, belegte Ziele, fehlende Referenzen und Wechsel zwischen Identitätsfamilien schlagen fehl.
- Ein neuer gezielter Negativtest deckt Entity→Layer und Event→EntityRetirement-RecordRef-Mappings ab: `logical_export::tests::logical_import_rejects_cross_family_and_record_variant_remaps`.
- Die zuvor offenen 15 M7-Folgebeleg-Paare wurden mit Artefakt- und Testreferenzen abgeschlossen; insgesamt sind 53 von 53 M7-Folgebeleg-Paaren `DONE`.

## Abschlussprüfung auf Windows

- Gezielter neuer Import-Negativtest: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.
- Der Workspace-Storage-/Crashlauf bestand mit **86 PASS, 0 FAIL, 1 ignoriertem manuellem 100.000-Punkte-Langlauf**.
- Plancheck und Sourcecheck: **PASS**; 252 Tasks, 11 Milestones, 253 Invarianten und 209 Folgebeleg-Paare; DAG und Quellenbezüge gültig.
- N-1 ist vor der ersten veröffentlichten Alpha nicht anwendbar; Wiederaufnahme ab M9-02.
- Nicht-Windows-Plattformabnahmen bleiben für M9-07 vorgemerkt.
