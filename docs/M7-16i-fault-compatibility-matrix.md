# M7-16i – Fault- und Kompatibilitätsmatrix

**Stand:** 3. Oktober 2026, Windows
**Ergebnis:** M7-16a bis M7-16i sind für die Windows-Phase abgeschlossen; Linux/macOS bleiben für M9-07 vorgemerkt.

## Bewertungsregel

Bei Backup, Restore und Purge ist ein Ziel nach einem Prozessabbruch entweder nicht veröffentlicht, ausdrücklich unvollständig oder vollständig verifiziert. Keine Prüfung behandelt einen sichtbaren Teilbestand als Erfolg. Die Quelle bleibt gültig und unverändert; die M7-16e-Schema-Migration ist die Ausnahme im Modell: Sie schreibt absichtlich in die Quelldatenbank, hält nach jedem WAL-Commit aber nur einen vollständigen, wieder lesbaren Präfix und setzt denselben Run ohne doppelte Schritte fort. Exportartefakte werden erst nach Rückgabe an den Test-Harness sichtbar.

Kindprozesse verwenden Exitcode 86. Die jeweilige Ergebnisspalte beschreibt den Zustand nach erneutem Öffnen bzw. Recovery, nicht nur einen synchron zurückgegebenen Fehler.

## Prozessabbruchmatrix

### Exact Backup – M7-16a

| Abbruchphase | Reopen-/Recovery-Ergebnis | Quelle und Ziel | Konkreter Test |
|---|---|---|---|
| Layout erstellt, Marker noch nicht geschrieben; Markerbytes vor Dateisync; Marker nach Dateisync | Jedes Ziel wird als `IncompleteTarget` erkannt. | Quelle öffnet und besteht Verify mit gleicher DatabaseId/Revision; kein Ziel gilt als Backup. | `backup::tests::process_crash_at_each_backup_boundary_leaves_only_incomplete_or_verified_target` |
| Erstes Zielobjekt leer; erster 8-Byte-Teilblock vor Dateisync | Das partiell kopierte Inventar bleibt `IncompleteTarget`. | Quelle unverändert; Retry darf das vorhandene Ziel nicht überschreiben. | Derselbe Backup-Grenzentest |
| Nach Dateisync jedes einzelnen Inventarobjekts; Manifest vor Dateisync, nach Dateisync und nach Zielverzeichnis-Sync | Trotz vollständiger Teilbestände oder Manifesteintrag bleibt der gesetzte Incomplete-Marker maßgeblich. | Quelle sauber; Ziel nicht als vollständig verifizierbar. | Derselbe Backup-Grenzentest |
| Prepublication-Verify abgeschlossen, Marker noch vorhanden | Reopen verweigert den unvollständigen Zielbestand. | Keine Teilpublication; Quelle weiter sauber. | Derselbe Backup-Grenzentest |
| Marker entfernt; danach Zielverzeichnis-Sync; danach finale unabhängige Verify | `verify_exact_backup` bestätigt ursprüngliche Identität/Revision, Itemzahl und sauberen Storage-Report. | Ziel ist vollständig verifiziert; Quelle ist unverändert. Retry liefert `TargetAlreadyExists`. | Derselbe Backup-Grenzentest |
| Separater Live-Pin-Abbruch nach Retirement eines gepinnten Segments | Solange der Kindprozess lebt, verhindert sein Pin die Reclamation. Nach Exit entfernt Reopen den stale Pin und reclaimed erst dann das nicht mehr referenzierte Segment. | Gültige Quelle und Live-Pin-Schutz bleiben erhalten; keine Backupbytes werden still überschrieben. | `backup::tests::process_crash_releases_stale_pin_after_reopen_without_reclaiming_live_snapshot_segment` |

Der Haupttest beendet den Kindprozess an jeder genannten dauerhaften Grenze. Das Testlimit von 8 Bytes erzeugt den partiellen Kopierfall; das Produktionslimit bleibt 64 KiB.

### Restore – M7-16b

| Backup-Profil / Abbruchphase | Reopen-/Recovery-Ergebnis | Quelle, Backup und Ziel | Konkreter Test |
|---|---|---|---|
| ExactDatabase vor atomarer Publication | Zielpfad fehlt. | Quelle bleibt sauber; Backupinventar- und Manifestdigests bleiben unverändert. | `backup::restore::tests::process_crash_before_or_after_atomic_publication_leaves_only_absent_or_verified_audited_target` |
| ExactDatabase nach atomarem Directory-Move und Parent-Sync, vor finaler Verify | Restore-Recovery öffnet einen vollständigen Clone mit neuer DatabaseId, sauberer Storage Verify, sicherem `CURRENT` und genau einem gebundenen `RestorePublication` Required Audit. | Ziel gültig oder Recovery schlägt sichtbar fehl; kein Teil-Clone wird akzeptiert. Quelle und Backup unverändert. | Derselbe Restore-Grenzentest |
| AuditComplete vor atomarer Publication | Zielpfad fehlt. | Quelle und verifiziertes Backup unverändert. | Derselbe Restore-Grenzentest |
| AuditComplete nach atomarem Directory-Move und Parent-Sync | Clone öffnet sauber; AuditComplete-Manifest und Raw-Read-WAL-Bytes stimmen mit der geprüften Backup-Lineage überein; RestorePublication-Audit ist gebunden. | Ziel vollständig und auditiert, neue DatabaseId; Quelle und Backup unverändert. | Derselbe Restore-Grenzentest |
| Kindprozess versucht Restore auf bereits existierendes Ziel | `TargetAlreadyExists`; Sentinel und Zielinhalt bleiben bytegleich. | Bestehendes Ziel wird nicht verändert. | Derselbe Restore-Grenzentest |

Die Post-Publish-Unterbrechung liegt vor der abschließenden Reopen/Verify im Restore-Aufruf; der Elternprozess öffnet und prüft deshalb den tatsächlich veröffentlichten Zielpfad.

### Guarded Schema Migration – M7-16c, M7-16d und M7-16e

M7-16c bindet jeden Schema-Schritt, seine kanonische Migration-Action, Required Audit und Replay-Snapshot an denselben synchronisierten WAL-Commitmarker. M7-16d führt Restrictive und Breaking über einen real geöffneten Datei-Store mit WriterLock, aktueller Autorisierung, OCC, Sidecar-Journal und Restoreproof aus. Das Journal bleibt außerhalb normativer History. Der Adapter unterstützt Genesis- und Head-Reads; ältere Nicht-Genesis-Reads failen bewusst geschlossen. Der M7-16e-Crashlauf startet an Revision 1; Schritt 1 committet Revision 2, Schritt 2 Revision 3.

| Abbruchcheckpoint in M7-16e | Dauerhafter Zustand beim Exit | Zustand nach Reopen/Resume | Konkreter Test |
|---|---|---|---|
| `running` | Run `Running`, beide Steps `Pending`, Head unverändert, keine Migrationmarker/Audits. | Derselbe OperationId-Run committet beide Schritte genau einmal. | `guarded_migration::tests::process_crash_at_each_guarded_migration_boundary_resumes_without_duplicate_steps` |
| `prepared_step_1` | Step 1 `Prepared`, vor WAL-Publication; Head unverändert, keine Marker/Audits. | Prepared OperationId wird wiederholt, beide Schritte werden genau einmal committed. | Derselbe Migration-Crash-Test |
| `wal_commit_step_1` | Step-1-WALmarker und Required Audit dauerhaft; Sidecar noch `Prepared`; Head auf Revision 2. | Reopen prüft Marker- und Auditbytes, reconciled Sidecar und setzt Step 2 fort, ohne Step 1 zu duplizieren. | Derselbe Migration-Crash-Test |
| `committed_step_1` | Step 1 ist im Sidecar und in History `Committed`; Step 2 `Pending`; Head Revision 2. | Derselbe Run erhält den gültigen Präfix und committet Step 2 genau einmal. | Derselbe Migration-Crash-Test |
| `prepared_step_2` | Step 1 committed; Step 2 prepared, aber noch nicht publiziert; Head und Auditcount enthalten nur Step 1. | Step 2 wird erneut ausgeführt; Step 1 bleibt unverändert. | Derselbe Migration-Crash-Test |
| `wal_commit_step_2` | Beide WALmarker und Required Audits dauerhaft; Sidecar von Step 2 noch `Prepared`; Head Revision 3. | Reopen reconciled Step 2 und schließt den Run ohne Doppelpublication ab. | Derselbe Migration-Crash-Test |
| `committed_step_2` | Beide Steps committed; finaler Head Revision 3, zwei Marker und zwei Required Audits dauerhaft. | Resume zeichnet `Completed` auf, ohne einen Schritt erneut zu publizieren. | Derselbe Migration-Crash-Test |
| `completed` | Zwei committed Steps und `Completed`-Sidecar dauerhaft. | Replay wird als `RunAlreadyCompleted` abgewiesen; History-/Auditcounts bleiben gleich. | Derselbe Migration-Crash-Test |
| Eingabe von Step 2 nach `wal_commit_step_1` geändert | Step 1 ist gültiger committed Präfix; geänderte Eingabe würde die Runidentität ändern. | Resume wird als `JournalIdentityMismatch` abgewiesen, ohne Head/Marker/Audit zu verändern; Originaleingabe kann denselben Run vervollständigen. | Derselbe Migration-Crash-Test |

Die passende nominale Produktionsintegration ist mit vier Restrictive-/Breaking-Fällen belegt. Verweigerte Rechte und veraltete OCC-Quellrevision erzeugen weder Journal noch Publikation; Breaking verlangt echten Restoreproof und explizite Adminentscheidung. M7-16c deckt zusätzlich beide reconcilierten WAL-Unknown-Outcome-Fälle ab: vorhandener Commitmarker wird als committed erkannt; nach Recovery sicher fehlender Marker wird als nicht committed bewiesen.

### Logical und Sharing Export – M7-16f

| Abbruchphase | Reopen-/Recovery-Ergebnis | Artefakt, Quelle und Audit | Konkreter Test |
|---|---|---|---|
| Logical Export: `pin_acquired`, vor History-Reads | Kein Sentinel-/Exportpfad; Recovery sauber; stale Pin wird entfernt; genau zwei ersetzte History-Segmente können danach reclaimed werden. | Kein Artefakt sichtbar; Quelle bleibt lesbar. | `logical_export::tests::process_crash_during_logical_export_returns_no_artifact_and_reclaims_stale_pin` |
| Logical Export: `before_return`, nach Artefaktbau und Validierung | Kein Artefaktpfad; Recovery sauber; stale Pin und dieselben ersetzten Segmente werden sicher bereinigt. | Ein vollständig im Speicher gebautes Artefakt entweicht nicht vor erfolgreicher API-Rückgabe. | Derselbe Logical-Export-Test |
| Sharing Export nach `ExportAuthorization`-WAL-Commit | Genau ein erfolgreiches Authorization-Audit, kein `ExportCompletion`; kein Artefaktpfad. | Quelle recovered sauber; fehlende Completion wird nicht unterstellt. | `sharing_export::tests::process_crash_after_authorization_commit_recovers_only_that_audit_boundary` |
| Sharing Export nach Rückkehr des inneren Logical Exports, vor Filter/Completion | Genau ein Authorization-Audit; innerer Logical-Pin ist freigegeben; Pin-Verzeichnis leer; kein Artefaktpfad. | Kein unvollständiges Sharing-Artefakt wird ausgegeben. | `sharing_export::tests::process_crash_after_logical_return_or_completion_commits_no_incomplete_artifact` |
| Sharing Export nach commit-verifizierter `ExportCompletion`, vor API-Rückgabe | Genau Authorization dann Completion, beide mit gleicher `AuditOperationId`; kein Artefaktpfad. | Abgeschlossener Inhalt wurde vor Completion validiert; der abgestürzte Aufrufer erhält dennoch kein Artefakt. | Derselbe Sharing-Crash-Test |

Ein zusätzlicher Fehlerfall belegt: Fehler nach Authorization schreibt keine Completion und gibt kein Artefakt zurück (`sharing_export::tests::failure_after_authorization_never_commits_completion_or_returns_artifact`). Der Test-Harness schreibt den Sentinel erst nach Rückkehr des Exporters.

### Offline Purge – M7-16g

| Abbruchphase | Reopen-Ergebnis | Quelle und Ziel | Konkreter Test |
|---|---|---|---|
| Vor Required-Audit-Commit | Quelle hat unveränderte DatabaseId, Revision, saubere Storage Verify und bytegleichen Logical Export; Ziel fehlt. | Kein publiziertes Ziel. Ein verborgenes Stage-Verzeichnis kann bestehen bleiben. | `purge_rewrite::tests::process_crashes_reopen_source_and_publish_only_verified_target` |
| Nach Zielverify/Audit/Report, vor atomarer Verzeichnis-Publication | Quelle bleibt sauber und unverändert; Zielpfad fehlt. | `.worlddb-purge-stage-*` ist nicht als Ziel sichtbar. Automatische Stage-Waisen-Bereinigung wurde nicht ergänzt; nur der Testbereich räumt sie nach Assertions weg. | Derselbe Purge-Crash-Test |
| Nach atomarer Publication und Parent-Directory-Sync, vor API-Receipt | Quelle bleibt bytegleich. Ziel öffnet mit neuer DatabaseId, sauberer Verify, genau passendem `PurgePublication` Required Audit und vollständigem digestgültigem Report. Secure-Erasure-Claim ist `0`. | Ziel ist vollständig gültig und auditiert. | Derselbe Purge-Crash-Test |

Die zurückgegebenen Fault-Contract-Tests ergänzen die Kindprozessmatrix: vor Audit/Publication ist kein Ziel sichtbar; nach Publication ist es geprüft und auditiert (`purge_rewrite::tests::publication_faults_never_expose_an_unaudited_database`; `m7_15_offline_purge_contract::offline_rewrite_publishes_a_new_verified_database_with_required_audit_and_report`). Es wird an keiner Stelle sichere physische Löschung behauptet.

## Fixture- und Hashbaseline

| Baseline | Gepinnte Identität | Erwartete Kompatibilität |
|---|---|---|
| M7-07 einzelne Storage-Formatprobe `format-v1.0.bin` | 72 Bytes, SHA-256 `ABA3763318D243BD554B3A8EB5EE88C646756DFB0CE0F09667995D7FB318C2C4` | Aktueller Frame 1.0 öffnet ohne die Probe umzuschreiben. |
| M7-07 `format-v2.0-unsupported.bin` | 72 Bytes, SHA-256 `68ED0C42EE9EC387CCFC848FD87B42BB33177DE677DF222362662BDB486EC207` | Gültig checksummierte zukünftige Major-Version wird als `UnsupportedVersion` abgewiesen; Bytes bleiben gleich. |
| M7-07 `format-v1.0-unknown-required-capability.bin` | 72 Bytes, SHA-256 `874FBF1B7F675E6689FA3E0E83DCCC0E806AAB379F8217D6884FBA4CBD387D18` | Nicht unterstützte Required Capability wird abgewiesen; Bytes bleiben gleich. |
| M7-16h vollständiger synthetischer Korpus | 24 Dateien, 15.553 Bytes; BLAKE3 des kompletten `manifest.tsv`: `e055937e2cc6a7613fc9d8c40a5a2e6ea9e18910c727d4eb6b3fcaa99cadea33` | Storage öffnet/verify; ExactDatabaseBackup verifiziert und restauriert als neuer ID mit Required Audit; Logical/Sharing Export sind bytegleich reproduzierbar; Restrictive-Schema-Migration committet Revisionen 3/4, eröffnet `Completed` und beide Audits; `FORMAT` bleibt bytegleich. |

Der M7-16h-Test fixiert zusätzlich alle 24 Dateipfade, Größen und BLAKE3-Dateihashes, schließt das Inventar gegen fehlende/zusätzliche Dateien und weist unsichere Pfade sowie Symlinks ab. Die M7-07-Proben sind einzelne Formatproben und kein vollständiges N-1-Datenbankbackup.

## Testmanifeste und vollständige Windows-Prüfung

| Task | Exakte Windows-Prüfung | Ergebnis |
|---|---|---|
| M7-16a | `cargo test --locked -p worlddb-storage-file backup::tests -- --nocapture` | 2 PASS; alle Backup-Grenzen und stale/live Pin-Recovery. |
| M7-16b | `cargo test --locked -p worlddb-storage-file backup::restore::tests::process_crash_before_or_after_atomic_publication_leaves_only_absent_or_verified_audited_target -- --nocapture` | 1 PASS; vier Prozess-Exits über zwei Profile sowie bestehendes Ziel. |
| M7-16c | `cargo test --locked -p worlddb-storage-file --lib migration_commit::tests::guarded_steps_and_canonical_audit_actions_reopen_from_file_store_commits -- --nocapture` | PASS; WAL Action, Required Audit und zwei Schritte werden nach Reopen rekonstruiert. |
| M7-16d | `cargo test --locked -p worlddb-storage-file --lib guarded_migration::tests:: -- --nocapture` | 4 PASS; Restrictive, Deny, OCC und Breaking-Proof. |
| M7-16e | `cargo test --locked -p worlddb-storage-file --lib guarded_migration::tests:: -- --nocapture` | 5 PASS; acht Kindprozessgrenzen plus geänderte Eingabe und Resume. |
| M7-16f | `cargo test --locked -p worlddb-storage-file --lib logical_export::tests:: -- --nocapture`; `cargo test --locked -p worlddb-storage-file --lib sharing_export::tests:: -- --nocapture` | 14 + 6 PASS. |
| M7-16g | `cargo test --locked -p worlddb-storage-file --lib purge_rewrite::tests:: -- --nocapture`; `cargo test --locked -p worlddb-storage-file --test m7_15_offline_purge_contract -- --nocapture` | 2 + 2 PASS. |
| M7-16h | `cargo test --locked -p worlddb-storage-file --test m7_16h_fixture_baseline -- --nocapture` | 1 PASS; hashgebundenes Inventar, Restore, Exporte, Migration und unverändertes `FORMAT`. |

Final workspace check: `cargo xtask verify` — **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**. Der Lauf umfasst Format, Workspace-Check, striktes Clippy, Storage-/Core-/TypeScript-Verträge, Plancheck, Sourcecheck und Whitespace-Prüfung. Die separat ausgeführten Plancheck- und Sourcecheck-Läufe bestätigten **252 Tasks, 253 Invarianten und 209 Folgebeleg-Paare** sowie die bytegleichen Source-/ZIP-Artefakte.

## N-1, Plattformumfang und M7-17-Handoff

Am 3. Oktober 2026 weist das öffentliche Repository keine veröffentlichten Releases auf ([GitHub-Releases](https://github.com/klarck36/WorldDB/releases)); `git ls-remote --tags --refs origin` gibt keine Tags zurück. Damit existiert keine veröffentlichte Vorgängerversion für einen echten N-1-Lauf. Das Ergebnis lautet **N-1 vor erster Alpha nicht anwendbar**, ausdrücklich **nicht bestanden und nicht als bestanden behauptet**. M9-02 prüft erneut, sobald eine Alpha tatsächlich veröffentlicht ist.

Alle obigen Ergebnisse sind Windows-Nachweise. Linux/macOS werden auf Nutzerwunsch erst in M9-07 geprüft. M7-17 übernimmt `docs/M7-16-fault-compatibility-inventory.md`, die Nachweise M7-16a bis M7-16i, die M7-16h Fixture-Manifeste sowie diesen Aggregatbericht als explizite Eingaben; der M7-17-Eintrag im Taskregister führt die vollständigen M7-16-Artefaktpfade.
