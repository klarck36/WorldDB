# M5-22 – Windows/NTFS-Crashmatrix

**Status:** M5-22 für das in M5-21 festgelegte aktive Profil `windows_ntfs_fixed_local_v1` abgeschlossen. Linux/ext4 und macOS/APFS bleiben gemäß Projektvorgabe spätere Plattformprüfungen; der M5-Gate bleibt davon getrennt offen.

## Profil und Testgrenzen

Die temporären Datenbanken liegen unter `%LOCALAPPDATA%\Temp` auf dem in M5-21 inventarisierten festen lokalen NTFS-Volume C:. Der Workspace unter OneDrive wird nicht als Datenbankvolume verwendet. Die Tests beenden gezielt einen Kindprozess mit `ExitProcess`-äquivalentem `std::process::exit`, öffnen denselben Datenbankordner mit einem neuen Prozess erneut und prüfen den WAL-/Auditpräfix.

Ein sauberer Prozessabbruch nach erfolgreichem `sync_all` belegt keine Wiederherstellung nach Kernelabsturz, Stromverlust oder Gerätecache-Verlust. Das Profil erhält daraus keinen Machine-Durability- oder Power-Loss-Claim.

## Windows-Prozessabbruch- und Faultpunkte

| Bereich | Injizierte Stelle | Prüfung nach Neustart |
|---|---|---|
| Daten-WAL | Nach Prepare-Sync sowie nach Commitmarker-Sync, bevor der Kindprozess ein Receipt zurückgeben kann | Ein Prepare ohne Marker bleibt bei Genesis; ein synchronisierter Commit wird genau als Revision 1 wiederhergestellt und erhält sein Manifest. |
| Manifest/CURRENT | Nach Manifestdatei-Sync, Generation-Publish, Manifestverzeichnis-Sync, CURRENT-Datei-Sync, CURRENT-Replacement und Datenbankverzeichnis-Sync | Der sichere Stand bleibt bei Revision 2; ein verwaistes unveränderliches Manifest hebt den bestätigten Präfix nicht an. |
| Daten-Recovery | Nach Intent, Quarantänekopie, Quarantänejournal, WAL-Abschneiden und Journalabschluss; jeweils als separater Prozessabbruch | Nach jedem Abbruch bleibt der fünfrevisionige, lückenlose Präfix sichtbar; die Quarantäne bleibt eindeutig und erneute Recovery ist idempotent. |
| Staged History-Segment | Nach Staging-Dateianlage, Header-/ID-/Digest-/Inhaltsschreibvorgängen, Datei-Sync und Staging-Verzeichnis-Sync | Genesis bleibt sicher; verwaiste, unvollständige Staging-Dateien werden nicht als committed History ausgegeben. |
| Staged Security-Segment | Nach denselben sieben Dateigrenzen wie beim History-Segment | Genesis und Policyhistorie bleiben unverändert; ein nicht im WAL referenziertes Segment öffnet keine Policyversion. |
| History-Reclamation | Nach Segmentlöschung und nach Synchronisierung des Segmenteverzeichnisses | Manifestrevision 2 mit leerem Inventar bleibt gültig; erneuter Reclamation-Aufruf ist idempotent, wenn die Datei bereits fehlt. |
| WAL-gebundener History-/Security-Replay | Nach Veröffentlichung des committed History-Segments und nach Manifestpublikation; beide Segmente liegen in demselben Commit | History- und Security-Snapshot werden gemeinsam materialisiert, das Manifest enthält beide Referenzen und das originale Operationreceipt wird nach dem Prozessabbruch rekonstruiert. Gleiche OperationId/Payload liefert dasselbe Receipt ohne WAL-Änderung; abweichender Payload wird abgelehnt. |
| Audit-Recovery | Nach Quarantänekopie, Intent, Audit-WAL-Abschneiden und Abschlussmarker; als aufeinanderfolgende Prozessabbrüche | AuditSequence 1 und der eine committed Auditversuch bleiben erhalten; ein uncommitted Tail wird quarantänisiert und nicht als Versuch ausgegeben. |
| Raw-Read-Audit | Kindprozessabbruch direkt nach dem synchronisierten Attempt | Der committed Versuch wird wiederhergestellt; das Audit behauptet keine erfolgte Seitenausgabe und ändert den Daten-WAL nicht. |
| Required Audit | Kindprozessabbruch nach erfolgreichem gemeinsamen Action-/Audit-Commit | Recovery liefert genau Revision 1, denselben Operationstatus und genau einen daran gebundenen Required Audit Record. |
| Manifest-I/O-Fehler | Fehlerantwort unmittelbar nach jedem der sechs Manifest-Publish-Schritte | Recovery materialisiert den bereits committed WAL-Präfix; Pointer- und Generationzustand bleiben verifizierbar. |
| Korruptionsklassen | M5-16-Contracts für Bitflip, Truncation, Reorder, Duplicate Frame und checksumgültige semantische Invalidität | `safe_revision` bleibt beim letzten gültigen Commitpräfix; sichere Korruption wird nicht automatisch repariert. |

Der 100.000-Punkte-Lauf wählt mit festem Seed `0x5744422d4d352d32` deterministisch einen der fünf Daten-Recovery-Punkte. Er arbeitet auf 1.000 frischen Windows/NTFS-Datenbanken mit je einem committed Präfix und 100 aufeinanderfolgenden Tail-Recovery-Unterbrechungen. Jeder Punkt schreibt einen neuen uncommitted Tail, löst über den Checkpoint-Hook `RecoveryError::Interrupted` aus, schließt die Handles, öffnet den Datenbankordner erneut und prüft `safe_revision` sowie die WAL-Präfixlänge. Der normale Seedtest bestätigt die reproduzierbare Verteilung `[20_037, 19_920, 19_762, 20_059, 20_222]`. Die 100.000 Hook-Unterbrechungen sind keine 100.000 separaten Betriebssystem-Prozessabbrüche; echte Kindprozess-Abbrüche werden repräsentativ für jede Grenze separat ausgeführt, auch über einen committed fünfrevisionigen Präfix. Der reproduzierbare Windows-Aufruf lautet:

```powershell
cargo test --locked -p worlddb-storage-file --lib windows_ntfs_100000_deterministic_recovery_crash_points -- --ignored --nocapture
```

## Ergebnis und Grenzen

- `cargo test --locked -p worlddb-storage-file --all-targets --quiet` auf Windows/NTFS: **105 bestanden, 0 fehlgeschlagen, 1 bewusst ignorierter Langzeittest**. Der ignorierte Test ist die separate 100.000-Punkte-Kampagne, die explizit ausgeführt wurde.
- `windows_ntfs_100000_deterministic_recovery_crash_points`: **PASS**, 100.000/100.000 Hook-Unterbrechungen über 1.000 Datenbanken; jeder Wiederanlauf behielt den vollständigen committed Präfix und die erwartete WAL-Präfixlänge. Der normale Seedtest bestätigt die reproduzierbare Punktverteilung `[20_037, 19_920, 19_762, 20_059, 20_222]`.
- Clippy (`-D warnings`), Formatprüfung, Plancheck, Sourcecheck und CI-Matrixprüfung bestanden.
- Die automatisierte Matrix deckt Daten-WAL, Manifest/CURRENT, Recovery, History- und Security-Segmentdateien, Reclamation, WAL-gebundenen History-/Security-Replay, Required Audit, Raw-Read-Audit und die fünf M5-16-Korruptionsklassen ab. Salvage-Archivdateien besitzen keinen M5-22-Absturzpunkt und bleiben außerhalb dieser Matrix; ein späterer M5-14-/CLI-Nachweis kann deren unvollständige Zielarchive separat behandeln.
- Echte Hardware-Power-Loss-/Cache-Verlustmessungen sind nicht automatisiert und wurden nicht ausgeführt. SSD-Firmwarecache und Power-Loss-Protection sind nicht gemessen.
- Linux/ext4 und macOS/APFS bleiben bis zu ihren eigenen Plattformläufen zurückgestellt. Das Windows-Ergebnis erteilt diesen Profilen keine Schreibfreigabe.
