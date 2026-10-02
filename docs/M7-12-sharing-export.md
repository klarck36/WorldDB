# M7-12 – Security-gefilterter Teilen-Export

## Zweck und Abgrenzung

`SharingExport` ist ein eigenes, versioniertes Exportprofil zum Teilen ausgewählter Daten. Es ist weder ein Logical Export noch ein Exact Backup. Es enthält ausschließlich Records, für die der aktuelle Principal die nötigen Klassen-, Feld- und Beziehungsrechte besitzt.

Der Scope nennt einen inklusiven Transaction-Time-Bereich, explizite HistorySpaces und Recordklassen. Implizite HistorySpace-Ancestors führen zum Abbruch; dadurch gelangen keine nicht ausgewählten Space-IDs als Abhängigkeit in das Artefakt. `HistorySpaceDefinition`-Records dienen intern zur Snapshot- und Ancestry-Prüfung und werden nicht serialisiert.

## Sicherheitsfilter

Vor dem Lesen wird der Scope gegen die aktuelle `SecurityPolicyView` geprüft. Der Export verlangt DataExport und HistorySpaceRead für die ausgewählten Spaces sowie DataExport und ProjectRead, wenn projektweite Klassen ausgewählt sind. Nach dem gepinnten Snapshot werden Klassen-, Feld- und Beziehungsrechte für jeden Record am exakten Record-/Space-/Layer-Ziel geprüft; ein nicht autorisierter Record gelangt nicht ins Artefakt.

Jeder Feld-Deny entfernt den ganzen Record. Das Format redigiert keine Teilfelder; gezielte Freigaben einzelner Felder genügen ohne pauschales `FieldRead` auf Projektebene. Lifecycle-, Event-, Evidence-, Provenance-, Mask- und Transferbeziehungen erscheinen nur, wenn ihre referenzierten Records ebenfalls im sichtbaren Export enthalten sind. Ein verdecktes Ziel wird daher nicht über eine Kante oder Lifecycle-Referenz offengelegt. Ein expliziter Mask-Target-Verweis wird ebenfalls ausgelassen, wenn seine Assertion nicht sichtbar ist.

## Wire-Format

Das Format hat einen eigenen Magic-Wert und eine domain-separierte BLAKE3-Integritätsprüfung. Es bewahrt die kanonischen Recordframes und optionale Wireflags. Die Scope-Koordinaten stammen ausschließlich aus der Anfrage. Das Format enthält keine Quell-DatabaseId, keine Snapshot-Revision, kein Logical-Export-Auslassmanifest, keine ausgelassenen Recordklassen und keine Quell- oder Verweigerungszählungen. Der notwendige Recordcount zählt nur die tatsächlich enthaltenen autorisierten Frames.

Grenzen: höchstens 512 MiB, 1.000.000 Records und 65.536 explizite HistorySpaces. Migration-Records ohne dauerhafte Revision sind bereits im gemeinsamen Scope-Validator ausgeschlossen.

## Dauerhaftes Audit

Nach erfolgreicher Scope-Autorisierung wird `ExportAuthorization` mit einem `Required Audit Record` als atomare WAL-/Manifest-Operation committed und nach Recovery verifiziert. Erst danach werden die Quelldaten gelesen. Nach vollständiger Filterung und Artefaktvalidierung wird `ExportCompletion` als zweite atomare Operation committed und verifiziert; erst dann gibt die API das Artefakt zurück.

Beide Auditrecords teilen eine AuditOperationId und haben jeweils eine eigene OperationId, Revision und streng steigende AuditSequence. Der opake Policy-Fingerprint bindet aktuelle Rechte und den expliziten Scope, ohne Recordinhalte oder ausgeblendete Größen offenzulegen. Ein Fehler nach der Autorisierungsgrenze liefert kein Artefakt und erzeugt keinen erfolgreichen Completion-Beleg.

## Abnahme

Die Windows-Prüfung und die Fault-/Recovery-Fälle sind in [`M7-12-verification.md`](M7-12-verification.md) festgehalten. Linux/macOS bleiben entsprechend Projektvorgabe für M9-07 zurückgestellt.
