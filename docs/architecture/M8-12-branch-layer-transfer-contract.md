# M8-12 – Branch-, Layer- und Inhaltstransfer-Vertrag

**Status:** implementiert; Windows-Abnahme in `docs/M8-12-verification.md`
**Umfang:** projektweite HistorySpace- und Layer-Verwaltung sowie expliziter same-database Inhaltstransfer im ODE-002-Desktop
**Normative Grundlage:** Master §§2.1.1, 2.3.2, 15.2–15.3, 31.2 und 31.4; M0-04 und M2-02a; Archive-/Transfer-Vertrag in `docs/contract/archive_transfer_contract.md`

## HistorySpaces und gemeinsame Revision

Ein Child-Branch ist eine unveränderliche HistorySpace-Definition mit einem existierenden Parent und festem `base_revision`. Der Cutoff liegt mindestens auf dem Cutoff des Parents und höchstens auf einer bereits veröffentlichten gemeinsamen Datenrevision. Das Anlegen fügt die Definition append-only hinzu; vorhandene Branches, Parent-Historie, Geschwister und bereits veröffentlichte Daten bleiben unverändert.

Katalogansichten unterstützen `current`, `historical` (`RecordedAsOf`) und `explicit` (exakte veröffentlichte Revision). Eine fehlende historische Projektion wird nicht durch einen aktuellen Stand ersetzt. Branch-Katalog, Layer-Katalog, Entity-Katalog und Schemaansicht bleiben an dieselbe gemeinsame Revisionsachse gebunden.

Child-Anlage verlangt `HistorySpaceRead` und `HistorySpaceCreate` am ausgewählten Parent. Bei einem neuen Root gilt der explizite Projekt-Scope. Der vollständige Metadatentransaktionsvalidator prüft `HistorySpaceCreate` erneut am Parent, bevor irgendein Record publiziert wird. Der Parent-Cutoff wird gegen den veröffentlichten Head und die unveränderliche Ancestry validiert.

## Layer

Layer sind projektweite Schema-Metadaten mit host-erzeugter Identität. Der neue Layer beginnt `Active`. Ein gültiger Layer-Snapshot hat genau einen aktiven Basis-Layer mit dem niedrigsten Rang. Höhere Ränge liegen weiter oben; ein Basiswechsel tauscht die Ränge des alten und neuen Basis-Layers in derselben Transaktion. Andere Ränge ändern sich nicht.

Beschreibung, Rang, Lifecycle und Basiszuordnung werden als neue historische Layer-Definitionen und vollständiger Layer-Snapshot angehängt. Lifecycle-Änderungen folgen `Active → Deprecated → Retired`; ein stillgelegter Layer kann nicht erneut aktiviert oder als Basis verwendet werden. Ein Basiswechsel und alle dazu erforderlichen Definitionen werden gemeinsam validiert und publiziert.

Kataloglesung verlangt `HistorySpaceRead` und `LayerRead`. Layer-Änderungen passieren nur mit `LayerManage` für die betroffenen exakten Layer-Identitäten. Der Child-Branch-Schreibvorgang und jeder Layer-Schreibvorgang binden ihren erwarteten gemeinsamen Head; ein veralteter Head wird ohne Veröffentlichung abgewiesen.

## Expliziter HistorySpace-Inhaltstransfer

Transfer kopiert eine nichtleere, ausdrücklich ausgewählte Menge sichtbarer `HistorySpaceContentRef`-Records und optional sichtbare projektweite `EventRelation`-Records innerhalb derselben Datenbank zwischen zwei verschiedenen, bereits existierenden HistorySpaces. Er legt weder einen Branch an noch verschiebt oder verändert er Quelldaten. Ein Child erbt weiterhin nur bis zum ursprünglichen Parent-Cutoff.

Der hostseitige Plan bindet Quelle, gepinnte Quellrevision, Ziel, Ziel-Head, jede konkrete Record-Familie, neu erzeugte Zielidentitäten und die explizite Richtlinie für externe Referenzen. Interne Referenzen folgen der vollständigen ID-Abbildung. Externe Referenzen werden standardmäßig abgewiesen; `RetainVisible` hält sie nur, wenn die Referenz bei Zielvalidierung sichtbar und zulässig ist. Cross-database Transfer gehört zum Export-/Import-Vertrag.

Jede Kopie erhält eine neue ID derselben konkreten Record-Familie und den Ziel-HistorySpace-Kontext. Zugelassene `DerivedFrom`-Familien erhalten eine Provenance-Lineage; für `Mask`, `ReplacementBoundary` und `EventMask` wird entsprechend der bestätigten Entscheidung ein eigenes `TransferLineage`-Record angelegt. EventRelations werden separat neu identifiziert; Endpoint-Remapping und die vollständige Ereignisgraphprüfung erfolgen vor Publication.

Die Vorschau meldet effektive Assertion-/Event-Lifecycle-Wirkungen, Relation-Retraktionen und archivierte Quellen. Lebenszyklus-Records selbst sind nicht auswählbar, weil ihre Zielkopie im selben Commit die erforderliche spätere Revision nicht einhalten könnte. Effektive Lebenszykluswirkungen werden nur nach ausdrücklich bestätigter Vorschau ausgelassen. Alle neuen Kopien starten **unarchiviert**; eine gewünschte Archivierung muss strikt später als die Zielanlage in einem eigenen Commit erfolgen.

Eine Vorschau wird an ein hostseitig erzeugtes, opakes Einmal-Ticket gebunden, läuft nach fünf Minuten ab und wird höchstens in 64 gleichzeitig offenen Plänen gehalten. Commit verbraucht das Ticket und validiert Berechtigungen, Quellstand, Ziel-Head, Referenzen, Ereignisgraph und IDs erneut. Copies, Remapping, Transfer-/Provenance-Lineage, ausgewählte Beziehungen, Policy-Historie, Required-Audit-Record (`HistorySpaceTransfer`) und Manifest erscheinen in einem WAL-Commit. Ein Konflikt oder eine unklare dauerhafte Commit-Ausgabe wird nicht als Erfolg ausgegeben.

## Desktop- und IPC-Grenze

Die native Host-Sitzung bindet IPC an die geöffnete Datenbank und den vom Host gewählten Principal. Der Renderer liefert weder Dateipfade noch Principals, Ziel-IDs noch Record-Bytes. Die geschlossene Command-Familie bietet Branch-/Layer-Katalog, Child-Anlage, Layer-Anlage/-Revision sowie Transfer-Katalog, Vorschau und Commit. Fresh IDs und Transferpläne bleiben hostseitig.

Benutzeroberflächen zeigen Branch-Namen aus der Ancestry, Layer-Symbole, Revisionen, Record-Familien und Vorschauauswirkungen. Technische IDs erscheinen nur als interne Auswahlwerte, nicht als Beschriftungen. Historische Katalogstände haben keine aktiven Schreibkontrollen. Schreibfehler verwenden stabile IPC-Fehlercodes; ein Project-State-Event aktualisiert weitere offene Fenster nach erfolgreicher Publication.

## Grenzen

Pro Transfer sind höchstens 1.024 Records und 1.024 EventRelations auswählbar; Quellinventar und dekodierte Bytes haben zusätzliche feste Speichergrenzen. Records mit nicht unterstützten verpflichtenden oder optionalen Wire-Flags werden nicht transferiert. Der Native-IPC-Smoke prüft den authentifizierten Katalogpfad in beiden Desktop-Profilen. Die dauerhafte Inhaltskopie ist durch den Storage-End-to-End-Test geprüft; die UI-Commitstrecke erhält mit späteren Record-Erfassungsaufgaben native Inhaltsfixtures. Linux-/macOS-Läufe bleiben auf Wunsch bis M9-07 zurückgestellt.
