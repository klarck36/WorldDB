# WorldDB 1.0 – Produktabläufe

**Status:** akzeptierter UX-Arbeitsvertrag für M0-05
**Grundlage:** Zielbild aus Abschnitt 1 des Arbeitsplans und der versionierten Arbeitskopie `docs/contract/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md`; Detailverträge M0-04 bis M0-04e
**Geltungsbereich:** geplante Desktop-, CLI- und Rust-API-Einstiege; keine Behauptung einer vorhandenen Implementierung

## 1. Gemeinsame Bedienregeln

- Desktop-Aktionen und CLI-/API-Aufträge verwenden dieselben typisierten Operationen, Schema-, Sicherheits- und Fehlerregeln. Die UI ist keine Autorität. Pfade und die angemeldete Identität kommen aus dem vertrauenswürdigen lokalen Host; ein Renderer-DTO setzt weder freie Engine-Dateipfade noch Principal oder Capabilities.
- Jeder Schreibdialog zeigt den wirksamen Projekt-, HistorySpace-, Layer-, Perspektiven- und Schemabezug vor der Bestätigung. Werte, die fachlich erforderlich sind, werden explizit erfasst. Ein UI-Default wird vor der Transaktion in konkrete typisierte Werte aufgelöst.
- Erfolg wird erst nach vollständigem Commitbeleg angezeigt. Ein `Conflict` verlangt eine neue Vorschau und ausdrückliches erneutes Absenden. Bei `UnknownCommitOutcome` fragt die Oberfläche denselben `OperationId`-Status ab; solange der Status ungeklärt ist, kann sie keinen neuen logischen Schreibversuch anbieten.
- Eine nicht sichtbare Ressource und eine nicht vorhandene Ressource behalten die gleiche geschützte öffentliche Fehlerform. Vorschau, Counts und Fehlermeldungen verraten keine verborgene Existenz.
- Ein angezeigter „Branch“ ist ein `HistorySpace`, kein eigener `BranchId`. Layer, Perspektive, EpistemicMode und Principal bleiben getrennte Auswahl- und Rechteachsen.

## 2. Erste Projektanlage und Anfangsrechte

### Entscheidung für den Standardstart

`CreateProject` registriert den vom Host authentifizierten Ersteller als Principal, legt die versionierten Rollen `GM` und `Player` an, speichert ihre expliziten Capability-Regeln und weist zunächst nur den Ersteller der `GM`-Rolle mit Projektscope zu. Das geschieht im vollständigen initialen Policy-Snapshot eines leeren Projekts. Der Ersteller wird nicht durch einen dauerhaften Engine-Bypass privilegiert; ab dem ersten Commit gelten dieselben Policyregeln wie für spätere Aktionen.

Das anfängliche `GM`-Paket ist ein explizites Allow-Bundle für den normalen Projektbetrieb:

| Bereich | Anfangs-Allow-Regeln für `GM` |
|---|---|
| Projekt und Schema | `ProjectRead`, `SchemaRead`, `SchemaManage` |
| HistorySpace und Layer | `HistorySpaceRead`, `HistorySpaceCreate`, `HistorySpaceTransfer`; `LayerRead`, `LayerWrite`, `LayerManage` |
| Entity und Perspective | alle registrierten `Entity(Create, Read, Reference, Retire)`- und `Perspective(Create, Read, Update, Use, Retire)`-Aktionen |
| Assertion und Event | jeweilige Read-/Create-/Correct-/Retract-Rechte; zusätzlich `EventSpanClose` |
| weitere Domain- und Lifecycle-Records | Read/Create/Retract für Mask, EventMask, ReplacementBoundary, Evidence und Provenance; `LifecycleRead`, `Archive`, `Unarchive`; `SourceRead`, `SourceCreate`, `SourceSupersede` |
| Felder und Beziehungen | `FieldRead`, `FieldWrite`, `RelationshipRead`, `RelationshipCreate`, `RelationshipRetract` mit ausdrücklichem Projektscope |
| Queries | `QueryResolve`, `QuerySearch`, `QueryFullText`, `QueryExplain`, `QueryGraphTraverse`, `QueryAggregate`, `RawHistoryRead` |
| Policy, Audit, Backup und eigene Jobs | `SecurityPolicyRead`, `SecurityPolicyManage`, `SecurityPermissionHistoryRead`, `AuditRead`, `BackupCreate`, `BackupRestore`, `JobRead`, `JobCancel` |

Alle Regeln sind getypte Allow-Regeln mit festem Projektscope; sie sind nicht aus dem Symbol `GM` abgeleitet. Ein aktives Deny gewinnt weiterhin. `AdminRawRead`, `Purge`, `DataImport`, `DataExport`, `MigrationPlan`, `MigrationExecute`, `AuditExport`, `AuditConfigure` und `JobManage` werden nicht im Standardpaket erteilt. Eine Query-Fähigkeit wie `QueryFullText` garantiert keine Engine-Unterstützung; ein nicht unterstützter Vorgang liefert `UnsupportedQueryCapability` und wird nicht still ersetzt.

`Player` wird als gewöhnliche Rolle registriert, erhält im Standardstart aber keine Allow-Regel und keine Principal-Zuweisung. Abwesendes Allow bedeutet Deny. Der Ersteller kann im Zugriffsdialog später einem vom Host authentifizierten Principal die Rolle zuweisen und zusätzliche getypte, auf HistorySpace, Layer, Record, Feld oder Beziehung beschränkte Grants hinzufügen. Kein Dialog kann einen Sicherheitsdeny umgehen.

### Ablauf und Abnahme

**Desktop:** `Datei > Neues Projekt` → Host-Dateiauswahl → Projektname → Zusammenfassung des anfänglichen GM-Bundles und der unberechtigten Player-Rolle → Erstellen.
**CLI/API:** Host-Operation `CreateProject` mit hostseitig ausgewähltem Ziel und authentifiziertem Ersteller; die Requestdaten liefern keine Principal- oder Capabilityauswahl.

**Abnahmeschritt:** Ein neues Projekt enthält eine DatabaseId, einen registrierten Ersteller, beide Rollen, das beschriebene GM-Bundle und die Zuweisung des Erstellers in einem vollständigen Initialzustand. `Player` kann vor einer expliziten Zuweisung und Freigabe nichts lesen oder schreiben. Der Bootstrap kann eine bestehende Datenbank nicht nachträglich umschreiben.

## 3. Projekt öffnen

**Desktop:** `Datei > Öffnen` → Host-Dateiauswahl → Statusseite mit Format, Datenbankidentität, Recoverystatus und erteilter Rolle.
**CLI/API:** hostseitiger `OpenProject`-Auftrag; danach werden Reads und Writes über die authentifizierte Session und den Policy-Evaluator ausgeführt.

Die App öffnet nur ein erkanntes WorldDB-Projekt. Sie zeigt unterstützte Format-/Schema- und Recoveryzustände, bevor sie Arbeitsfunktionen freigibt. Erfordert die Datei Recovery oder ist der sichere Präfix eingeschränkt, sind Read-only, Verify, Restore oder die ausdrücklich definierte Recovery-Aktion getrennte Entscheidungen. Öffnen startet keine stille Schema-/Formatmigration und repariert keine Daten automatisch.

**Abnahmeschritt:** Ein gültiges Projekt öffnet mit gebundener DatabaseId und Berechtigungen. Eine fremde, inkompatible oder beschädigte Datei wird mit sicherem Fehler bzw. Recoveryoption angezeigt; der Open-Auftrag verändert sie nicht still.

## 4. Schema bearbeiten

**Desktop:** `Projekt > Schema` mit getrennten Registern für Predicate, EntityType, EventKind/Rollen/Attribute, Layer und Migrationspläne.
**CLI/API:** typisierte Schemaänderung in `WriteTransaction`; Migrationen als `plan`, `dry-run`, `run` und `resume` gemäß späterer CLI-Oberfläche.

Der Editor zeigt stabile IDs, Feldtypen/Constraints, Lebenszyklus, SchemaRevision und betroffene Referenzen. Eine kompatible Ergänzung kann mit Daten in derselben Transaktion stehen. Restriktive oder Breaking-Änderungen beginnen mit einem unveränderlichen Migrationsplan, Dry-Run, Budget und Restorepoint; mehrteilige Läufe zeigen jeden gültigen Zwischenschemastand. Öffnen führt keine Migration automatisch aus. Deprecated/Retired IDs bleiben historisch erhalten und werden nicht neu vergeben.

**Abnahmeschritt:** Eine neue Definition wird mit stabiler ID im Schemaverlauf sichtbar und kann für einen neuen Record ausgewählt werden. Eine ungültige Constraint-Änderung oder nicht bestätigte Breaking-Migration veröffentlicht keine Teiländerung und fordert keinen stillen Rewrite bestehender Historie.

## 5. Entity anlegen und verwalten

**Desktop:** `Katalog > Entities > Neue Entity`; Entity-Detail zeigt die unveränderliche Identität, den EntityType und verknüpfte Assertions.
**CLI/API:** `CreateEntity(EntityTypeId)` und getrennt `RetireEntity`; Entity-Fakten werden als Assertions erfasst.

Die Erstellung verlangt einen aktiven EntityType und erzeugt lokal eine neue typed `EntityId`. Eine Entity besitzt kein implizites Namensfeld und wird in 1.0 nicht umtypisiert. Namen und Beschreibungen sind normale, historisierte Assertions. Retirement ist terminal, verhindert neue Referenzen, ändert oder löscht aber keine vorhandenen Aussagen und Events.

**Abnahmeschritt:** Eine Entity mit aktivem Typ kann referenziert werden; unbekannter/retired Typ oder fehlendes `entity.create`-/`entity.reference`-Recht führt zu keiner Teilanlage. Nach Retirement bleiben ältere Fakten lesbar, während neue Referenzen scheitern.

## 6. Assertion erfassen oder korrigieren

**Desktop:** `Daten > Aussage hinzufügen` oder eine sichtbare Aussage öffnen und `Korrigieren` wählen. Das Formular zeigt Subject, Predicate, getypten Value, Polarity, Validity, HistorySpace, Layer, PerspectiveScope und EpistemicMode.
**CLI/API:** `CreateAssertion(AssertionDraft)` oder `CorrectAssertion` aus M0-04e über dieselbe Transaktionsgrenze.

Der Nutzer wählt `WorldState` mit `World` oder einen aktiven Perspective mit genau einem passenden epistemischen Modus. `Unknown`, `False` und `Conflict` bleiben unterschiedliche fachliche Zustände. Eine neue Aussage überschreibt keine vorhandene. Korrektur erzeugt atomar die neue Assertion, das ausdrückliche Retractionrecord am Original und `Corrects`-Provenance. Der Vorschauzustand ist keine Zusage, dass die neue Aussage anschließend als `Known` aufgelöst wird.

**Abnahmeschritt:** Eine gültige typed Assertion erhält eine Revision und wird im gewählten Kontext abgefragt. Ungültiger Value/Scope, fehlendes Feld-/Entity-/Perspective- oder Aktionsrecht veröffentlicht nichts. Bei Korrektur werden genau die drei vertraglich genannten Domainrecords gemeinsam sichtbar; eine alleinige `Corrects`-Kante retractet nichts.

## 7. Branch/HistorySpace und Layer verwenden

**Desktop:** Kontextleiste `HistorySpace` (mit UI-Bezeichnung „Branch“ für Children) und unabhängige Layerauswahl; `Neuer Branch` öffnet einen Create-Dialog.
**CLI/API:** `CreateHistorySpace(parent, base_revision)` und Record-Commands mit expliziter `LayerId`; Query verwendet `LayerSelection`.

Ein HistorySpace hat null oder genau einen Parent und einen festen `base_revision`. Ein Child sieht Parent-Inhalte nur bis zu diesem Cutoff. Erstellung verlangt einen vorhandenen Parent und eine gepinnte Revision. Layer werden projektweit im Schema definiert, besitzen `precedence_rank` und sind unabhängig von HistorySpace-Zugehörigkeit. Vor dem Schreiben wird der ausgewählte Layer in eine konkrete `LayerId` aufgelöst. Transfer zwischen HistorySpaces ist eine separate, previewte Kopieraktion mit ID-Map; es gibt keinen impliziten Merge.

**Abnahmeschritt:** Ein Child kann Parent-History bis zum gewählten Cutoff lesen und nicht spätere Parent-Commits. Ein Schreibvorgang ohne konkrete LayerId wird abgewiesen. Ein Layerwechsel verschiebt keine Daten; ein nicht bestätigter Transfer ändert weder Quelle noch Ziel.

## 8. Perspective definieren

**Desktop:** `Katalog > Perspectives` plus Kontextauswahl im Assertion- und Query-Dialog.
**CLI/API:** `CreatePerspective`, `UpdatePerspective`, `RetirePerspective`; Queries und Assertions über typed `PerspectiveScope` plus `EpistemicMode`.

Eine Perspective beschreibt eine Sicht innerhalb der Welt und ist niemals der angemeldete Principal. `WorldState` hat keine Perspective. `Knows`, `Believes` und `Claims` benötigen eine aktive Perspective und bilden getrennte Räume. Änderungen an Name/Beschreibung schreiben eine neue Definition unter derselben ID; Retirement verhindert neue Uses, ohne bestehende Assertions umzuschreiben.

**Abnahmeschritt:** Ein perspektivengebundener Record mit aktiver Perspective und passendem Modus wird akzeptiert. Fehlende, retired oder nicht erlaubte Perspective und jede ungültige World/Mode-Kombination scheitern atomar. Ein Wechsel der Perspective ändert keine Security-Rolle.

## 9. Event erfassen oder korrigieren

**Desktop:** `Daten > Event hinzufügen`; im Event-Detail getrennte Befehle für `Korrigieren`, Span schließen und ausdrücklich Retraction/Mask/Relation.
**CLI/API:** `CreateEvent(EventDraft)`, `CorrectEvent`, sowie jeweils eigene typed Lifecycle- und Relation-Operationen.

Das Formular bindet EventKind, Rollen/Kardinalitäten, Attribute, EventTime, HistorySpace und Layer. Eine Korrektur erzeugt einen neuen Event plus `Corrects`; das Original bleibt aktiv. Sie erzeugt weder EventRetraction noch SpanClosure, EventMask, `Before`, `SameTime` oder `Causes`. Solche Effekte sind eigene Befehle und erhalten ihre eigene Vorschau und Rechteprüfung.

**Abnahmeschritt:** Ein schema-valider Event wird mit expliziten Teilnehmern, Attributen und Zeit gespeichert. Ungültige Rolle, Zeit oder Referenz hinterlässt keine Teilrecords. Nach `CorrectEvent` bleiben beide Events in der History sichtbar und das Original aktiv, bis eine ausdrückliche Lifecycle-Aktion committed ist.

## 10. Source, Evidence und Provenance

**Desktop:** `Quellen > Neue Source`; aus einem Record heraus `Evidence verknüpfen`; Detailansichten zeigen nur autorisierte Felder und Endpunkte.
**CLI/API:** `CreateSource`, `CreateEvidence(source, target, relation)` und getrennte typed Provenance-Aktionen.

Eine Source beschreibt Herkunft, behauptet allein aber nichts über einen Datensatz. Eine Evidence-Kante ordnet genau eine Source einem konkreten, berechtigten Domain- oder Lifecycle-Record mit `Supports`, `Contradicts` oder `Documents` zu. Kanten verändern weder Assertion-Resolution noch reaktivieren sie Targets. Korrekte Source-Metadaten bleiben unveränderlich: falsche Angaben werden durch neue Source plus zulässige `Corrects`-Kante abgelöst. Locator, Digest, Metadaten und Beziehungsendpunkte folgen Feld-/Relationship-Rechten.

**Abnahmeschritt:** Eine Source kann ohne Evidence existieren und wird nicht als Beleg für einen Record angezeigt. Eine explizite Evidence-Kante ist nach Commit mit sichtbaren, erlaubten Endpunkten abrufbar. Doppelte aktive Kante, fehlendes Endpoint-Recht oder versteckter Target-Endpunkt wird ohne teilweise Kante und ohne Existenzleck abgewiesen.

## 11. Query ausführen

**Desktop:** `Abfragen` mit Raw History, Resolved View, Explain, Wortsuche, Graphdurchlauf, COUNT, EXISTS und COUNT nach Polarity; Kontextleiste für Snapshot, RecordedAsOf, HistorySpace, LayerSelection, WorldTime, Perspective/EpistemicMode und SchemaMode.
**CLI/API:** ein gemeinsames typed `QueryRequest`; CLI-/IPC-Adapter setzen dieselben Pflichtfelder und Ergebnis-/Fehler-DTOs.

Vor Ausführung zeigt die Query-Oberfläche alle gebundenen Kontextwerte und endliche Candidate-/Work-/Result-Budgets. `Current` wird einmal zu einem Snapshot gebunden; Folgeseiten verwenden denselben Cursor und Snapshot. Die Resolved View stellt `Known`, `Unknown` und `Conflict` als getrennte fachliche Ergebnisse dar; technische Query-/Storagefehler bleiben separate Fehlerzustände. Fehlende FieldRead- oder Recordrechte filtern Kandidaten vor Resolution, Counts, Sortierung oder Explain.

Die Wortsuche ist eine exakte, indexfreie TokenSearch über Stringwerte. Der Nutzer wählt, ob alle Suchwörter oder mindestens eines vorkommen müssen. Treffer enthalten nur Record-ID, Recordfamilie und gefundene Feldnamen; Textausschnitte werden nicht ausgegeben. Seiten verwenden einen opaken, an Anfrage und Snapshot gebundenen Cursor. Die Oberfläche benennt unvollständige Ergebnisse, zeigt die Cursorfrist von 60 Sekunden und bietet nach Ablauf einen Neustart der Suche an. Die Trefferanzahl pro Seite ist begrenzt.

Der Graphdurchlauf startet an einem explizit ausgewählten Record und bindet Beziehungstypen, Ein-/Auswärtsrichtung, maximale Tiefe, Knoten-/Kantenzahlen und Zyklusverhalten. Ergebnis und erreichte Tiefe zeigen die Grenzen der Traversierung. COUNT und EXISTS sowie COUNT nach Polarity werten die vollständigen, im gewählten Querykontext sichtbaren Resolution-Contributors aus; die Gruppierung liefert Gruppen nach vorhandener Polarity. Endliche Budgets werden in jeder Antwort mitgeführt. Such-, Graph- und Aggregatmodi umgehen weder Kontextbindung noch Securityfilter. FullText wird nicht still durch TokenSearch ersetzt.

**Abnahmeschritt:** Eine identische Anfrage am selben Snapshot liefert dieselbe geordnete logische Seite. WorldState darf perspektivfrei sein, epistemische Modes benötigen ihre passende Perspective. Fehlender Pflichtkontext, nicht unterstützte Queryfähigkeit, Budgetende, Cancellation, unvollständige Suche oder ungültiger/abgelaufener Cursor erscheinen als getrennte Endzustände; verborgene Records ändern kein Resultat, Count oder öffentliches Fehlerdetail.

## 12. Rollen und Zugriffsrechte verwalten

**Desktop:** `Projekt > Zugriff & Audit` zeigt Principals, Rollen, Zuweisungen, getypte Allow-/Deny-Regeln, Scope, Policyrevision und Auditstatus.
**CLI/API:** typed Security-Policy-Operationen in einer normalen `WriteTransaction`; keine direkte Dateibearbeitung und keine rendererseitige Capabilityauswahl.

Ein Grant-Editor verlangt Capability, Effect, Subject und jeden Scope-Selector ausdrücklich. `Deny` gewinnt vor Allow; fehlendes Allow verweigert. Rollen erben nicht, und die Namen `GM`/`Player` sind keine Privilegien. Schreibvorgänge prüfen Operation, Records, Felder, Beziehungen, Entity-Referenzen und Perspective-Use gemeinsam und werden zum Commit mit aktueller Policy erneut bewertet. Policyänderung und erforderlicher AuditRecord werden atomar committed; ein Auditfehler verhindert die Änderung.

**Abnahmeschritt:** Ein expliziter scoped Grant erlaubt nur passende Operationen; ein passender Deny überstimmt ihn. Eine Policyänderung erscheint mit Actor, Revision und SecurityEpoch in der Policy-/Auditansicht. Ein verweigerter Schreibversuch hinterlässt keinen Teildatensatz und eine historische Leseberechtigung gewährt keinen Schreibzugriff.

## 13. Backup und Restore

**Desktop:** `Datei > Sichern/Wiederherstellen` unterscheidet `ExactDatabaseBackup`, `AuditCompleteBackup`, Logical Export und Teilen-Export sichtbar nach Namen und Umfang.
**CLI/API:** `BackupCreate(profile, destination)`, Ziel-Verify und `BackupRestore(verified_backup, empty_target, mode)`.

Vor dem Start zeigt der Dialog Backup-Profil, Ziel, `safe_revision`, Audit-Scope und ob Auditvollständigkeit mitgesichert wird. Erfolg folgt erst nach vollständigem Ziel-Verify von Einträgen und Gesamtinventar. Restore schreibt in ein leeres Ziel und publiziert erst nach Integritäts-/Kompatibilitätsprüfung und Recovery; In-place-Überschreiben ist kein Modus. Der Nutzer wählt ausdrücklich Clone mit neuer DatabaseId oder Disaster-Recovery mit erhaltener ID und exklusivem Original-/Restore-Betrieb. Ein Backup-Digest wird nicht als Herkunftssignatur bezeichnet.

**Abnahmeschritt:** Ein erzeugtes Backup besteht Ziel-Verify und wird mit Profil/Revision/`audit_scope` quittiert. Fehlende, geänderte oder vertauschte Inventarpositionen scheitern vor Erfolg. Restore in ein leeres Ziel erhält verifizierte Daten; ein belegtes Ziel, inkompatibles Format oder nicht konsistente Auditlineage wird nicht überschrieben oder freigegeben. Ein Teilen-Export kann in keiner Oberfläche als exaktes Backup erscheinen.

## 14. Zielbild-Abdeckung

| Fähigkeit aus Abschnitt 1 des Arbeitsplans | Bedienablauf |
|---|---|
| Lokales WorldDB-Projekt erstellen und öffnen | 2–3 |
| Schema und typisierte Identitäten | 4–5 |
| Typisierte Assertions und Ereignisse | 6, 9 |
| HistorySpaces, Layer und Perspektiven | 7–8 |
| Quellen, Evidenz und Provenienz | 10 |
| Historische und aufgelöste Abfragen mit `Known`/`Unknown`/`Conflict` | 6, 9–11 |
| Rechte und Auditierbarkeit | 2, 12 |
| Verifizierbare Sicherung und Restore | 13 |
| Lokale Produktgrenze ohne 1.0-Netzwerkdienst oder stille Migration | 1, 3–4 |

Diese Schritte sind die späteren manuellen Desktop-/CLI-End-to-End-Abnahmen. M0-05 prüft ihre Dokumentation und Contract-Referenzen; Windows-UX, Accessibility, Tastaturabläufe und Laufzeitresultate sind in M8-26a/M8-26d belegt. APFS/ext4 und die 18 plattformübergreifenden Invariant-Folgebelege sind M9-07 zugeordnet.
