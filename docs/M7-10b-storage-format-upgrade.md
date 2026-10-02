# M7-10b – Storageformat-Upgrade CURRENT v1 nach v2

**Status:** Implementiert und unter Windows geprüft; der allgemeine M7-Gate bleibt offen.
**Geltungsbereich:** ausschließlicher Formatwechsel des Root-Pointers `CURRENT`; kein fachliches Schema-Upgrade.
**Entscheidungsbeleg:** `docs/contract/ADR-041-storage-format-upgrade-capability.md`.

## Abgeschlossene Transformation

Der erste ausführbare Upgradepfad ändert ausschließlich die Kodierung von `CURRENT` von Version 1 nach Version 2. Alle referenzierten Manifeste, WAL-, History-, Security- und Auditdateien behalten ihre Bytes. `DatabaseId`, logische Revision, Commit-Hash, Manifestgeneration und Manifestdigest bleiben beim Upgrade gleich. Der nächste normale Manifestpublish verwendet weiterhin `CURRENT` v2.

`CURRENT` v1 besteht aus 80 Bytes: acht Magic-Bytes `WDBCUR\0\x01`, acht Little-Endian-Bytes für die Generation, 32 Manifestdigestbytes und einem 32-Byte-BLAKE3-Checksum über die vorherigen 48 Bytes. `CURRENT` v2 besteht aus 112 Bytes: acht Magic-Bytes `WDBCUR\0\x02`, acht Little-Endian-Bytes für die Generation, 32 Manifestdigestbytes, 32 Bytes Profilfingerprint und einem 32-Byte-BLAKE3-Checksum über die vorherigen 80 Bytes. Der v2-Profilfingerprint bindet die bekannte Komponenten-/Versionsbaseline sowie Required- und Optional-Capabilitybits aus `FORMAT`.

Für eine noch nicht publizierte Genesis-Manifestgeneration ist ausschließlich in v2 Generation `0` zusammen mit einem Nulldigest zulässig. Ein v1-Zeiger mit Generation `0` bleibt ungültig. `ManifestStore::read_current` validiert den v2-Profilfingerprint; ein unbekannter oder unpassender Pointer wird nicht still repariert oder geschrieben.

## Preflight und Restorepoint

`StorageUpgradeManager::prepare` arbeitet unter exklusivem Writerlock, verlangt ein schreibbares Recoveryergebnis und führt Storage Verify aus. Der unveränderliche Plan bindet:

- zufällige `UpgradePlanId`, Source- und Targetprofil sowie deren exakte Inventarfingerprints;
- DatabaseId, sichere Revision, Commit-Hash, `FORMAT`-Digest und bisherigen `CURRENT`-Digest;
- die einzige unterstützte Transformation `CurrentPointerV1ToV2`, Transformer- und Verifier-Version;
- endliche Arbeits-, Speicher- und Stagingbudgets sowie einen kanonischen Planfingerprint.

Die stabile Komponenteninventur erfasst UTF-8-Relativepfade, Datei-/Verzeichnistyp und Dateilänge sortiert. Symbolische Links und Spezialdateien sperren den Lauf. `staging`, `quarantine` und der Root-Writerlock werden nicht Teil des Formats; ein leerer nächster WAL-Segmentplatzhalter hat keine Formatbytes und wird ausgelassen. Die Inventur begrenzt Einträge auf 65.536, Pfadbytes auf 8 MiB, Tiefe auf 32 und ihren Speicher auf 64 MiB. Der Walker hält keine unbeschränkte Geschwisterliste im Speicher.

`create_safe_restore_point` verlangt getrennt `BackupCreate` und `BackupRestore`, erzeugt ein Exact Database Backup und restauriert es wirklich als isolierten Clone. Backup, Sourcebindung, neue Restore-DatabaseId, Restore-Revision und unabhängiger Storage-Verify-Bericht werden geprüft und in einem unveränderlichen Beleg an Plan und Profilfingerprints gebunden. Der Restore-Clone enthält seine eigene Restore-Auditlineage; der Beleg behauptet keine Auditvollständigkeit, die das Backupprofil nicht liefert.

`confirm`, `execute` und `resume` verlangen `StorageFormatUpgrade`. Die bestätigte Aktion bindet Plan, Restorebeleg, Akteur, zufällige `UpgradeRunId` und Fingerprint der effektiven aktuellen Rechte. Backup-/Restore-Rechte oder `MigrationExecute` erteilen diese Berechtigung nicht. Jede Ausführung und Fortsetzung prüft die aktuellen Rechte erneut.

## Journal, Veröffentlichung und Wiederaufnahme

Das Sidecar unter `staging/storage-upgrade-<UpgradeRunId>.journal` ist keine normative Datenhistorie. Jeder feste 241-Byte-Datensatz enthält 8 Magicbytes, eine Little-Endian-Sequenz, Phase, Run-/Plan-IDs, fünf Fingerprints/Digests und einen abschließenden BLAKE3-Checksum. Die vier monotonen Phasen sind `Prepared`, `Staged`, `Published`, `Complete`. Ein abgeschnittener Schlussdatensatz darf nur nach erfolgreicher Prüfung seiner Bindungen bei explizitem Resume entfernt werden; vollständige Checksummen- oder Reihenfolgefehler werden abgewiesen.

1. `Prepared` wird synchron geschrieben; bis dahin bleibt `CURRENT` unberührt.
2. Ein vollständiger v2-Zeiger wird unter der Run-ID in `staging` angelegt, synchronisiert und danach `Staged` protokolliert.
3. Der bestehende Windows-Publikationsadapter ersetzt `CURRENT` atomar. Nach Root-Verzeichnissync werden Pointer, unveränderte Manifest-/WAL-Bindung und das Targetprofil neu geprüft.
4. Erst nach erfolgreicher Targetprüfung folgen `Published` und `Complete`.

Bei Sourceprofil sind nur `Prepared` und `Staged` gültige Journalstände. Bei Targetprofil muss mindestens `Staged` vorliegen. Widersprüchliche Pointer-/Journalphasen, andere Profile, geänderte Sourcekomponenten oder ein ungebundener Lauf scheitern fail-closed. Nach einer Unterbrechung ist daher der vollständige verifizierte Source- oder Targetpointer aktiv; normales Öffnen startet nie ein Upgrade.

## Prüfnachweise und Grenzen

`storage_upgrade::tests::current_upgrade_reopens_at_source_or_target_and_resumes_every_publish_boundary` injiziert Fehler vor Journalanlage, nach `Prepared`, nach Staging, vor Pointerpublish und nach atomarem Replace. Es prüft Source-/Target-Reopen, beschädigtes Journal, reparierbaren Torn Tail, Berechtigungsentzug, geändertes Komponenteninventar, unveränderte logische History und nachfolgenden v2-Manifestpublish. `storage_upgrade::tests::v2_genesis_pointer_is_valid_only_for_the_recognized_profile` prüft Genesis-Sentinel und unveränderte Ablehnung eines falschen Profils.

Diese Abnahme gilt für die Windows-Laufzeit in diesem Projektworkspace. Linux/macOS- und Dateisystemabnahmen bleiben wie vom Product Owner festgelegt zurückgestellt; diese lokale Prüfung erteilt keine plattformübergreifende Durability-Freigabe. Die implementierte Transformliste enthält derzeit nur `CURRENT` v1→v2 und ist keine allgemeine Konvertierung beliebiger Komponentenversionen.
