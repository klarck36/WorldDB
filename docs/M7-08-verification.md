# M7-08 – Exact/Hot Backup

## Ergebnis

`ExactBackupManager` erzeugt ein `ExactDatabaseBackup` aus einem kurz gepinnten, unabhängig verifizierten Datenbanksnapshot. Nach dem WAL-Checkpoint dürfen weitere Transaktionen weiterlaufen; der Backupjob kopiert dennoch genau den gesicherten Revisionspräfix. Der Zielordner wird erst nach Inventarprüfung und Storage Verify als vollständig veröffentlicht.

## Persistente Datenbankidentität

Neu angelegte Dateidatenbanken erhalten eine UUIDv7 in `DATABASE_ID`. `DatabaseLayout::open` validiert Größe, UUID-Form, Dateityp und Pfad. Ältere Layouts ohne diese Datei bleiben lesbar; ein Exact-Backup scheitert für sie ausdrücklich mit `DatabaseIdentityMissing`, weil es keine belastbare Datenbankidentität erfinden darf. Die ID-Datei gehört zum Backupinventar.

## Snapshot, WAL und Segmentpins

- Vor dem Schnitt müssen Recovery Scanner und Storage Verify einen sauberen Stand bestätigen; Manifestrevision und WAL-Kopf müssen samt Commit-Hash übereinstimmen.
- `WalPrepareLog::checkpoint` synchronisiert die WAL-Dateien, bindet die exakte Marker-Endposition des gesicherten Commits und öffnet bei Bedarf ein neues aktives WAL-Segment. Das Backup nimmt nur geschlossene Dateien und Bytes bis zu dieser Grenze auf.
- Alle Manifestsegmente werden vor Freigabe des Writer-Locks als Backupsegmente gepinnt. Neben dem prozesslokalen Pin bleibt während des Kopierens ein checksummierter Pinbeleg mit gemeinsamem Dateilock bestehen. Kompaktierung erkennt aktive Belege auch aus anderen Prozessen; nach einem Prozessabbruch entfernt sie einen verwaisten Beleg erst, wenn sie dessen exklusiven Lock erhalten hat.
- Ein deterministischer Fortschritts-Callback meldet den gepinnten Stand vor dem Kopieren. Dadurch kann ein Writer sofort weiterarbeiten; die Abnahme lässt während der Kopie einen zweiten Commit und eine ersetzende Kompaktierung laufen.

## Inventar und Integrität

`EXACT_BACKUP` ist ein versioniertes, kanonisches Binärmanifest. Es bindet Datenbank-ID, Formatfähigkeiten, Revision, Commit-Hash, Manifestgeneration sowie jeden Inventareintrag mit Pfad, geschlossener Dateiklasse, exakter Bytezahl und BLAKE3-Dateidigest. Ein Inventardigest bindet die geordnete Liste samt Größen und Klassen; ein weiterer Digest bindet den gesamten Metadaten- und Inventarpayload.

Das Profil nimmt `FORMAT`, `DATABASE_ID`, `CURRENT`, die ausgewählte Manifestgeneration, alle darin referenzierten History- und Security-Policy-Segmente sowie exakt den WAL-Präfix auf. Das Manifest setzt `audit_scope=Excluded`; Audit-WAL und Auditsegmente sind nicht Teil dieses Profils. `AuditCompleteBackup` bleibt M7-09.

Digests weisen Byteintegrität nach, behaupten aber keine Herkunft. Optional bindet ein BLAKE3-Keyed-MAC die gesamten Metadaten und das Inventar. Algorithmus (`BLAKE3-KEYED`), Schlüssel-ID und Status werden getrennt ausgewiesen: nicht beansprucht, beansprucht aber ungeprüft, verifiziert, falsche Schlüssel-ID oder ungültiger MAC.

## Zielveröffentlichung und Verify

Der Zielordner wird nur neu angelegt. Eine `EXACT_BACKUP.INCOMPLETE`-Markierung bleibt während des Aufbaus bestehen. Vor ihrer Entfernung prüft ein unabhängiger Durchlauf alle Itemdigests, das Inventar gegen die tatsächlich vorhandenen Daten-Dateien, DatabaseId/Formatbindung sowie Recovery-, WAL-, Manifest-, OperationId- und History-Verify. Danach wird die Markierung entfernt, das Verzeichnis synchronisiert und die öffentliche Backupprüfung ein zweites Mal ausgeführt. Ein fehlgeschlagener Durchlauf liefert keinen Erfolg und lässt den Ordner als unvollständig erkennbar.

## Nachweise

- `m7_08_exact_backup_contract::hot_backup_stays_at_its_pinned_revision_while_source_writes_continue`: der Quellstand schreitet während des Kopierens von Revision 1 auf 2 fort; das Backup enthält ausschließlich Revision 1 und genau das vor dem Checkpoint geschlossene WAL-Segment.
- `m7_08_exact_backup_contract::backup_pin_protects_retired_snapshot_segments_until_copy_completion`: ein ersetztes Quellsegment bleibt bis zum Abschluss des Backups durch Kompaktierung geschützt.
- `m7_08_exact_backup_contract::integrity_only_and_mac_authenticity_are_reported_separately`: Digest-only behauptet keine Herkunft; ungeprüfter MAC, falsche Schlüssel-ID, falsches Schlüsselmaterial und verändertes Inventar erhalten unterschiedliche Ergebnisse.
- `m7_08_exact_backup_contract::target_verify_failure_keeps_the_backup_marked_incomplete`: Änderung eines kopierten Inventaritems vor Ziel-Verify verhindert die Veröffentlichung.
- `compaction::tests::durable_backup_pin_protects_history_until_its_lease_is_released`: der durable Cross-Process-Pin schützt die angegebene Segmentidentität und fällt nach Freigabe weg.

## Abgrenzung

Diese Task implementiert einen keyed MAC, keine asymmetrische Signatur. Ein echter Restore samt neuer Clone-ID oder Disaster-Recovery-Publikation bleibt M7-10. Auditvollständigkeit bleibt M7-09. Linux- und macOS-Plattformnachweise bleiben wie vereinbart für M9-07 zurückgestellt.
