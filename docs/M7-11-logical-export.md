# M7-11 – Logical Export

## Zweck und Umfang

`LogicalExportManager` erzeugt ein eigenständiges, versioniertes Artefakt aus einem expliziten Ausschnitt der logischen History. Der Aufrufer muss einen inklusiven Transaktionsrevisionsbereich, mindestens einen HistorySpace und mindestens eine Record-Klasse angeben. `HistorySpaceDefinition` ist zusätzlich erforderlich, damit die ausgewählten Räume samt ihrer für die Interpretation nötigen Vorfahren im Ergebnis enthalten sind. Doppelte IDs oder Klassen, ein umgekehrter Revisionsbereich, ein unbekannter Raum und Klassen ohne dauerhafte Revisionskoordinate werden zurückgewiesen.

Der Export enthält die persistente `DatabaseId`, die Revision des geprüften Quell-Snapshots, den angeforderten Revisionsbereich, die explizit ausgewählten HistorySpaces und Klassen sowie die sichtbare HistorySpace-Abstammung mit den jeweiligen Revisions-Cutoffs. Datensätze aus einem HistorySpace oberhalb seines Cutoffs oder außerhalb des angeforderten Bereichs werden nicht aufgenommen. HistorySpace-Definitionen haben im aktuellen Datenmodell keine Transaktionsrevision; ausgewählte Räume und ihre Vorfahren werden daher ausdrücklich als `HistorySpaceDependency` gekennzeichnet, statt eine Revision zu erfinden.

Alle implementierten Domain-, Schema-, Metadaten- und Lifecycle-Record-Klassen mit dauerhafter Revisionskoordinate können explizit ausgewählt werden. `MigrationPlan`, `MigrationRun` und `MigrationStepCommitIdentity` sind mangels dauerhafter Revisionskoordinate nicht auswählbar. Ein ausgewählter Event- oder Transferzusammenhang wird nur aufgenommen, wenn seine HistorySpace-Endpunkte vollständig in der sichtbaren Abstammung liegen. Lifecycle- und Archive-Einträge erben den Geltungsbereich des referenzierten Datensatzes.

## Autorisierung und Snapshot

Vor dem Lesen von Quelldatensätzen prüft der Export die aktuelle Policy: Für jeden ausdrücklich ausgewählten HistorySpace muss `DataExport` erlaubt sein. Werden projektweite Klassen ausgewählt, ist zusätzlich eine projektweite `DataExport`-Freigabe erforderlich. Eine frühere Policy-Freigabe überstimmt eine aktuelle Ablehnung nicht.

Danach öffnet der Export das Datenbanklayout erneut, hält den Writer-Lock für Storage-Verify und Snapshot-Bindung, und verlangt einen sauberen Verify-Bericht sowie übereinstimmende WAL- und CURRENT-Manifest-Köpfe. Alle Referenzsegmente werden als Export gepinnt; zusätzlich schützt ein dauerhafter Pin die Segmente prozessübergreifend vor Reclamation. Erst nach erfolgreicher Pin-Anlage wird der Writer-Lock freigegeben und die unveränderliche History gelesen. Jeder Segmentdigest wird gegen die Manifestreferenz geprüft.

## Kanonisches Artefakt

Das Format beginnt mit `WDBLEX\0\x01`, einer 64-Bit-Payloadlänge und einem BLAKE3-Digest mit dem Kontext `WorldDB.LogicalExport.v1\0`. Die Records werden nach `(Transaktionsrevision, RecordKind-Nummer, kanonische Framebytes)` sortiert; HistorySpace-Abhängigkeiten ohne Revisionsfeld verwenden für die Sortierung den Genesis-Wert und behalten ihre explizite Inclusion-Markierung. Optionale Wire-Flags bleiben erhalten.

Die Decodergrenzen liegen bei 512 MiB pro Artefakt, einer Million Records und 65.536 ausgewählten HistorySpaces. Der Decoder prüft Digest, Längen, Record-Klassen, vollständige Klassenmanifestierung, ausgewählten Scope, Einfügungsmarkierungen, die exakte Übereinstimmung der manifestierten Abstammung mit den HistorySpaceDefinition-Abhängigkeiten und strikte kanonische Reihenfolge. Er kodiert danach erneut und verlangt Bytegleichheit. Der Digest belegt Integrität gegen unbeabsichtigte oder nicht neu signierte Änderungen; er authentifiziert weder Urheber noch Quelle.

Das Manifest enthält eine Zeile für jede geschlossene Record-Klasse. Pro Klasse werden Auswahlstatus und die Zahl der tatsächlich exportierten Records angegeben. Für ausgelassene bzw. nicht ausgewählte Quelldaten werden keine Quell-Counts veröffentlicht, damit das Manifest keine Größe anderer HistorySpaces verrät. Fünf nichtlogische Speicherklassen werden immer ausdrücklich als ausgelassen ausgewiesen: Security-Policy-History, Audit-History, Transaktions-/Recoveryzustand, physisches Manifest-/Format und abgeleitete Indizes.

## Abgrenzung

`LogicalExport::decode` und erneutes Kodieren prüfen den Format-Roundtrip; M7-11 importiert keine Records in eine Datenbank und remappt keine IDs. Import und ID-Remap folgen M7-13. Teilen-Exporte mit ihren strengeren Regeln zu verborgenen IDs, Counts, Metadaten und dauerhafter Audit-Pflicht folgen M7-12. Exact-/Audit-Backup-Artefakte sind davon getrennte Formate.
