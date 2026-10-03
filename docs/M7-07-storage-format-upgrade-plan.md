# Storageformat-Upgradeplan v1

**Stand:** Spezifikation vor Alpha; kein ausführbarer Upgradepfad und kein Upgrade-Commit.

## Versionsmodell

Das bestehende Dateiformat besitzt derzeit **keine einzelne globale Datenbank-Formatversion**. Die Datei `FORMAT` ist ein WorldDB-Frame-1.0-Probe mit Required-/Optional-Capabilities; diese Frameversion ist nicht automatisch die Version aller Datenbankdateien. Segmente, Manifeste, `CURRENT`, Security-/Auditdateien, Recovery-Journale und Indexpointer führen eigene Magic-, Versions- oder Capabilitykennungen. Ein Storage-Profil muss deshalb den vollständigen versionierten Komponentenvektor plus Root-Capabilities binden. Ein Magic-Byte allein oder die Version der `FORMAT`-Framehülle genügt nicht als Profilidentität.

Die aktuelle Komponentenbaseline aus den vorhandenen Writer-/Decoderverträgen lautet:

| Komponente | Aktuelle On-Disk-Kennung |
|---|---|
| `FORMAT` | WorldDB-Frame 1.0, Kind `0x57444246`; Required-Capabilities `0`, Optional-Capabilities werden opak erhalten |
| Domain-WAL und Raw-Read-Audit-WAL | WorldDB-Frame 1.0; Prepare-/Commit-/Recoveryframearten sind getrennte geschlossene Tags |
| History-Segment | Magic `WDBSEG\0\x01`, Segment Major/Minor `1.0`; kanonischer Frameindex und Records |
| Manifest | Magic `WDBMAN\0\x01`, Manifest Major/Minor `1.0` |
| `CURRENT` | Magic `WDBCUR\0\x01` |
| Security-Policy-Segment | Gemeinsame Segmenthülle; Security Major/Minor `1.0` |
| Required-Audit-Payload | Magic `WDBAUD\0\x01` |
| WAL-Replay-Snapshot-Payload | Magic `WDBRPL\0\x01` |
| Recoveryjournal | Magic `WDRJ`, Recordversion `1` |
| Index-Generationspointer | Magic `WDBIDXCP`, Pointerversion `1` |
| Abgeleitete Indexgeneration | WorldDB-Frame 1.0, Indexformat-, Schemasnapshot- und Buildversion separat gebunden; Indexdaten bleiben rekonstruierbar |

Das künftige Profil muss die Kennungen zusätzlich zu Vorhandensein/Abwesenheit der optionalen Dateien und den tatsächlich gesetzten Capabilities kanonisch erfassen. Ein bloßer Hardcode `storage_format=1.0` würde diese unabhängigen Versionsräume vermischen.

Das erste Fixtureprofil `worlddb-storage-initial-v1` beschreibt die aktuelle vor Alpha festgeschriebene Baseline. Seine Probe bytes liegen unter `crates/worlddb-storage-file/tests/fixtures/m7-07/`. Weil noch keine Alpha mit älterem Storageprofil veröffentlicht wurde, gibt es derzeit kein echtes N-1-Release. Die v1.0-Bytes werden erst dann als N-1-Eingabe klassifiziert, wenn eine spätere Profilversion veröffentlicht ist. Bis dahin darf M9-02 N-1 nicht als bestanden melden; seine eigene Abnahme verlangt ausdrücklich `N-1 not applicable` vor der ersten Alpha. Die Fixtures für Segment, Manifest, WAL, Audit, Recoveryjournal und Indizes bleiben zusätzlich an die jeweiligen vorhandenen Komponentenprüfungen gebunden. M7-16h baut die versionierte Erstfixture-Baseline über diese FORMAT-Proben hinaus aus.

## Getrenntes Planmodell

Ein späterer Storageupgradeplan ist eine eigene, unveränderliche Spezifikation mit mindestens:

- eigener storage-spezifischer `UpgradePlanId`, unabhängig von `MigrationId`, `MigrationRunId`, `StepId` und Schema-`OperationId`;
- exakt erkanntem Source-Komponentenvektor und dessen Fingerprint, explizitem Target-Komponentenvektor und einer geschlossenen, geordneten Liste von Formattransforms;
- stabilen Transform-/Verifier-Versionen, begrenzten Work-/Speicher-/Platzbudgets und einem kanonischen Planfingerprint;
- einem Read-only-Preflight, der unbekannte Required-Capabilities, fehlende oder widersprüchliche Komponenten, beschädigte WAL-/Manifest-/Segmentpräfixe und nicht unterstützte Downgrades vor jeder Mutation abweist.

Der Plan schreibt keine Schema-Records, ändert keine normative WorldDB-History und verwendet nicht `MigrationPlan`, `MigrationCategory` oder deren Transaktionsprotokoll. Eine reine Formattransformation darf die logische History, `DatabaseId`, normative Revision und Commitreihenfolge nicht verändern. Eine fachliche Änderung bleibt eine eigene Schema-Migration. Die Storageformat-Operation besitzt später einen eigenen Run-Journal- und Recoveryvertrag.

## Safe-Restore-Point: zwingende Vorbedingungen

Ein Backup-Digest allein ist kein Safe Restore Point. Vor jedem zukünftigen formatbrechenden Commit müssen alle folgenden Bedingungen am selben eingefrorenen Source-Stand belegt sein:

1. Der aktuelle Komponentenvektor und die Source-Profilidentität sind exakt erkannt; der Reader kann die vollständige Source-Form lesen.
2. Exklusive Writer-Sperre und abgeschlossene Recovery liegen vor. WAL-/Auditpräfixe, Manifeste, referenzierte Segmente und Checkpoints sind vollständig geprüft; Korruption, unklare Commitausgänge oder nicht abgeschlossene Recovery sperren die Freigabe.
3. Ein erfolgreicher `ExactDatabaseBackup` bindet dieselbe `DatabaseId`, Source-Profilidentität, normative Revision/Commitbindung und das vollständige erforderliche Inventar einschließlich aller Restore-bytes. Der Zielbackup ist vollständig verifiziert; Einzel- und Gesamtintegrität sind belegt. Optionales MAC/Signaturmaterial und Schlüsselidentität werden getrennt von Integrität und Digeststatus ausgewiesen.
4. Der Backup wird tatsächlich in ein isoliertes leeres Ziel restauriert. Das restaurierte Ziel besteht Storage-Verify und einen logischen Vergleich der Records, Schemahistorie, Revision, OperationId-Dedup und WAL-Präfixe gegen den gebundenen Source-Stand. Falls ein Profil Auditvollständigkeit behauptet, wird zusätzlich die unabhängige `audit_safe_sequence` geprüft; ein Datenbackup ohne diese Abdeckung bleibt explizit `audit_scope=Excluded`.
5. Ein `SafeRestorePoint`-Beleg bindet Backup-Inventardigest, Source-Profilfingerprint, DatabaseId, Revision/Commit-Hash, Restore-Zielidentität, Verifybericht und Ergebnis des realen Restore-Tests. Der Beleg muss zum Source- und Targetprofil sowie zum Fingerprint des geplanten Upgradeplans passen.
6. Die Upgradeaktion ist eine separat autorisierte explizite Aktion. Normales `open`, Reopen, Recovery, Resave, CLI-Startup oder App-Update darf das Upgrade nie starten oder stillschweigend bestätigen.

Die vollständige Backup-Verifikation kommt aus M7-08/M7-09; der reale Restoretest und sein Receipt aus M7-10. M7-07 definiert diese Vorbedingungen, erzeugt aber noch keinen solchen Beleg.

## Vorgeschriebener späterer Ausführungspfad

Der erste ausführbare Upgradepfad aus M7-10b muss Sourceprofil und Upgradeplan read-only erkennen, die Safe-Restore-Point-Bedingungen samt expliziter Autorisierung prüfen und alle Transforminputs vorab budgetieren. Er transformiert in eine isolierte Staginggeneration und verifiziert dort jedes Zielformat sowie die unveränderte logische History. Erst danach darf ein dedizierter, crash-resumabler atomarer Root-Publishpoint das Zielprofil aktivieren. Ein plan- und run-gebundenes Storage-Journal liegt außerhalb normativer History. Vor Publish bleibt die Source-Datei-/Manifestgeneration unverändert; nach jedem Crash ist ausschließlich ein vollständig verifiziertes Source- oder Targetprofil offen, sonst fail-closed. Der genaue Pointer, das Journalformat, Faultpunkte und Resume-Verhalten werden in M7-10b spezifiziert und getestet.

`DatabaseLayout::open` bleibt ein read-only Probe-/Kompatibilitätspfad. Es darf nur unterstützte Versionen öffnen und muss bei unbekanntem Major oder unbekanntem Required-Capability explizit scheitern, ohne Bytes zu schreiben. Das heute bereits vorhandene `resave_format` ist ein expliziter Capability-Resave mit Writerlock und Recoveryprüfung, kein Formatupgrade.
