# M9-05 – Disk- und Prozess-Stresstests

**Stand:** 7. Oktober 2026  
**Profil:** Windows 11 / NTFS  
**Ergebnis:** bestanden für das lokale Profil

## Fehler- und Wiederherstellungsmatrix

| Fall | Injektion und Nachweis | Ergebnis |
|---|---|---|
| Disk-full | Der WAL-Schreiber schreibt einen Teilframe und erhält danach einen `StorageFull`-Fehler. `m9_05_disk_full_fault_preserves_safe_prefix_and_audit_continuity` scannt und reconciled den WAL erneut. | `safe_revision=1`, `audit_sequence=1`, Recovery `clean`, Status `write_failed`. |
| Quota | Der Commitmarker wird teilweise geschrieben; danach wird ein Quota-I/O-Fehler gemeldet. `m9_05_quota_fault_preserves_safe_prefix_and_audit_continuity` prüft den unklaren Commitstatus über OperationId und Recovery. | `safe_revision=1`, `audit_sequence=1`, Recovery `clean`, Status `outcome_reconciled`. |
| Permission-Änderung | Das Read-only-Attribut des aktiven WAL-Segments wird während des Laufs gesetzt. Der Append wird abgewiesen; nach Rücksetzen des ursprünglichen Attributs gelingt der nächste Commit. | Abgewiesener Schreibversuch lässt Revision und AuditSequence bei 1; der nächste Commit erzeugt Revision 2 und AuditSequence 2. |
| Uhr-Anomalie | Der UUIDv7-Zeitstempel wird mit zwei kontrollierten `SystemTime`-Werten vor und nach einem Rücksprung erzeugt; ein Wert vor der Unix-Epoche wird ebenfalls geprüft. | Rücksprung ist als nicht-monotone ID-Metadaten zulässig; vor der Epoche wird `ClockBeforeUnixEpoch` geliefert. Die Testinjektion verändert nicht die Betriebssystemuhr. |
| Writer-/Engine-Panic | Frischer In-Process-/Sidecar-Lauf `m9-05-process-matrix-20261007T130956Z-325817f`. Der In-Process-Panic vergiftet den Enginezustand und verlangt einen App-Neustart; der Sidecar-Panic ersetzt den Enginekindprozess. | Beide Modi bestanden. In-Process erwarb nach dem App-Neustart den Writer-Lock wieder. Sidecar blieb verfügbar, erwarb den Lock erneut und beendete die Kindprozesse beim Shutdown. |
| Rendererfehler | `renderer-health.js` meldet unbehandelte `error`- und `unhandledrejection`-Ereignisse in einem zugänglichen Statusfeld. Drei Node-Tests prüfen Integration, Statusmeldung und das Auslassen technischer Fehlerdetails. | Alle 3 Tests bestanden. Die Meldung fordert WAL-Statusprüfung vor einem Wiederholungsversuch. |
| Geordneter Shutdown | Der bestehende Windows-Nachweis aus [M8-20](M8-20-verification.md) prüft Job-Drain, Restartstatus und Shutdown in In-Process und Sidecar; der aktuelle IPC-Lauf weist `background_job_shutdown_drain` und `process_shutdown` als `PASS` aus. | Vollständiger Drain und Prozessende bestanden; ein nicht abgeschlossener Job wird nicht als Erfolg ausgegeben. |
| Wiederholte Recovery unter Schreiblaster | `m9_05_repeated_recovery_under_audited_write_load_has_no_sequence_gaps` führt 64 Zyklen mit jeweils einem auditierten Commit und einem teilweise geschriebenen, nicht committed Tail aus; jeder Zyklus öffnet und scannt erneut. | 64/64 Recovery-Zyklen `clean`, 64 committed Auditrecords, zusammenhängender sicherer Präfix und keine AuditSequence-Lücken. |

Zusätzliche Crash-Recovery-Grenzen einschließlich Required Audit sind in [M5-22](M5-22-windows-crash-matrix.md) belegt: Ein Kindprozessabbruch nach gemeinsam committed Aktion/Audit ergibt genau Revision 1, denselben Operationstatus und genau einen gebundenen Required-Auditrecord. Die dortigen 100.000 Recovery-Unterbrechungen sind ausdrücklich Hook-Unterbrechungen; separate echte Prozessabbrüche decken die repräsentativen Grenzen ab.

## Ausgeführte Prüfungen

- `cargo test --locked -p worlddb-core -p worlddb-storage-file --lib m9_05_ -- --nocapture` – **5 bestanden**.
- `node --test experiments/ode-002/scripts/test-renderer-health.mjs` – **3 bestanden**.
- `cargo clippy --locked -p worlddb-core -p worlddb-storage-file --all-targets -- -D warnings` – **bestanden**.
- `cargo fmt --all -- --check`, Node-Syntaxprüfung, PowerShell-Parser und `git diff --check` – **bestanden**.
- `python -B WorldDB_1.0_Plancheck.py` – **257 Tasks, 11 Meilensteine, 253 Invarianten und 230 Folgepaare; DAG und Referenzen gültig**.
- Prozessmatrix mit fünf 100-MiB-Streams und fünf Abbrüchen bei 8 MiB je Modus – **bestanden**, Digests zwischen In-Process und Sidecar identisch.

Die gehashte Prozessmatrix liegt unter `experiments/ode-002/evidence/m9-05/m9-05-process-matrix-20261007T130956Z-325817f/`; `manifest.json` bindet sie an sauberen Commit `325817ffff382888747663ccfd261a366b34d792`, Windows 11 Build 26200 und NTFS. Die Cargo-Builds liefen außerhalb des OneDrive-Arbeitsbaums unter `%LOCALAPPDATA%\WorldDB\m9-05-target`.

## Grenzen

Disk-full und Quota werden als partielle WAL-Schreibvorgänge mit injizierten I/O-Fehlern reproduziert; das Volume wurde nicht physisch gefüllt und keine NTFS-Quota eingerichtet. Der Berechtigungstest setzt das Read-only-Attribut des Segments, keine NTFS-ACL. Die Uhrprüfung injiziert Zeitwerte, sie stellt die Systemuhr nicht um. Der Renderer-Nachweis deckt unbehandelte JavaScript-Fehler und -Rejections ab, keinen erzwungenen WebView2-Prozessabbruch. Der Prozess-, Speicher- und Recovery-Nachweis gilt für Windows/NTFS; APFS und ext4 bleiben M9-07 zugeordnet.
