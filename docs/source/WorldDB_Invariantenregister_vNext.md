# WorldDB – Invariantenregister vNext

**Stand:** 20. September 2026  
**Status:** normativ. Kennungen werden nach Veröffentlichung nicht neu belegt. Entfernte Regeln bleiben als `RETIRED` dokumentiert.

## 1. Testlegende

- `UT`: Unit Test
- `PT`: Property Test
- `CF`: Compile-Fail/trybuild
- `SM`: State-Machine/Model Test
- `DF`: Differential gegen Referenzmodell
- `CR`: Crash-/Fault-Injection
- `FZ`: Fuzzing/Malformed Input
- `NI`: Security Non-Interference
- `CP`: Cross-Platform
- `CT`: Compatibility/Golden
- `PF`: Performance-/Ressourcengate
- `E2E`: Desktop/API End-to-End
- `ST`: statischer Architektur-/Policy-Nachweis
- `DR`: Dokumentations-/Entscheidungsnachweis

Bezeichnungen wie Review, Policy, API snapshot, Miri, Truth table, Architecture lint, Contract test, Custom lint, Benchmark review, Documentation test und CI manifest sind konkrete Implementierungen und werden in `invariants_vNext.toml` getrennt von der Evidence Class erfasst. Jede HARD-Invariante MUSS vor 1.0 mindestens einen negativen Test besitzen, der ohne Enforcement fehlschlägt. Reiner Review ohne maschinenlesbaren oder ausführbaren Nachweis ist keine Abdeckung.

## Übergreifende Invariante

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-PHIL-001 | HARD | Abstraktionen erhalten jede für Semantik, History, Security, Recovery, Retry, Ownership oder Diagnose relevante Unterscheidung, sofern kein Verlustfreiheitsnachweis vorliegt. | Architecture lint, negative API tests |

## 2. Historie, HistorySpaces und Zeit

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-HIS-001 | HARD | Veröffentlichte Revisionen steigen monoton und sind eindeutig. | SM, CR |
| WDB-HIS-002 | HARD | Revision ist Transaction Time und bestimmt keine fachliche Precedence. | UT, DF |
| WDB-HIS-003 | HARD | Committed History wird nur durch expliziten Offline-Purge physisch entfernt. | SM, CR |
| WDB-HIS-004 | HARD | Historical Query bindet Daten- und Schema-Snapshot reproduzierbar. | DF, CT |
| WDB-BRA-001 | HARD | Child sieht Parent höchstens bis `base_revision`. | PT, DF |
| WDB-BRA-002 | HARD | Spätere Parent-Commits fließen nicht automatisch in Child. | SM, DF |
| WDB-BRA-003 | HARD | Siblings sind ohne explizite Migration/Merge isoliert. | SM, DF |
| WDB-BRA-004 | HARD | HistorySpace-Operationen ändern keine Schemaidentitäten. | UT, CT |
| WDB-BRA-005 | HARD | HistorySpace ist der einzige Branch-Domain-Typ; `BranchId` existiert nicht. | CF, API snapshot |
| WDB-LAY-001 | HARD | Jeder layerfähige Record trägt genau eine LayerId; Layer ist orthogonal zu HistorySpace, Perspective und Security. | Schema/API tests |
| WDB-LAY-002 | HARD | ContextPrecedence ordnet HistorySpace-Spezifität vor LayerRank; Perspective/EpistemicMode sind keine Ränge. | Truth table, DF |
| WDB-LAY-003 | HARD | Masking überschreitet weder Perspective- noch EpistemicMode-Partitionen. | Truth table, NI |
| WDB-LAY-004 | HARD | Layerrechte verändern keine fachliche Precedence. | NI, DF |
| WDB-LAY-005 | HARD | Jeder wirksame historische Schemastand bezeichnet genau einen aktiven Base-Layer. | Schema validation, migration golden |
| WDB-LAY-006 | HARD | LayerDefinition folgt `Active → Deprecated → Retired`; Retirement verhindert neue Records, erhält aber Definition und historische Records. | SM, DF, migration golden |
| WDB-LAY-007 | HARD | `base_layer_id` ist historisierte Projektschemadaten und weder hardcodierte ID noch Name. | Schema golden, DF |
| WDB-LAY-008 | HARD | Der bezeichnete Base-Layer besitzt den eindeutig niedrigsten aktiven PrecedenceRank und kann während der Bezeichnung nicht deprecated/retired sein. | PT, schema validation |
| WDB-LAY-009 | HARD | Jeder persistierte layerfähige Record trägt eine ausdrückliche LayerId; Decoder, Import und Storage setzen nie still den Base-Layer ein. | FZ, CT, import negative |
| WDB-LAY-010 | HARD | Base-Layer-Wechsel ist eine atomare Schemaänderung, erhält historische Bezeichnungen und verschiebt keine Records automatisch. | SM, migration golden, DF |
| WDB-LAY-011 | HARD | LayerSelection ist `BaseOnly | AllActive | Explicit`; BaseOnly wird gegen den gepinnten Query-Schemasnapshot aufgelöst. | CF, DF, historical query |
| WDB-TIM-001 | HARD | RecordedAsOf, WorldTime, EventTime und AssertionValidity sind getrennte Typen. | CF, UT |
| WDB-TIM-002 | HARD | Inkompatible Timelines erhalten keine implizite globale Ordnung. | CF, PT |
| WDB-TIM-003 | HARD | Intervallgrenzen verwenden eine einzige dokumentierte Halb-offen-Semantik `[start,end)`. | PT, DF |

## 3. Assertions, Masking und Resolution

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-AST-001 | HARD | Assertion-Records sind immutable. | API, SM |
| WDB-AST-002 | HARD | Close, Retract und Correct erzeugen neue Lifecycle-/Domain-Records. | SM, DF |
| WDB-AST-003 | HARD | Retraction bedeutet Korrektur auf Transaction Time, nicht World-Time-Ende. | Truth table, DF |
| WDB-PRO-001 | HARD | Proposition Equality besteht exakt aus Subject, Predicate, Value, Polarity. | PT |
| WDB-MSK-001 | HARD | Masking ist kein positiver oder negativer Fakt. | Truth table, DF |
| WDB-MSK-002 | HARD | Mask wirkt nur gegen strikt niedrigere `ContextPrecedence`. | Truth table |
| WDB-MSK-003 | HARD | Mask-Scope ist genau ExactAssertion, Proposition oder Slot. | UT, FZ |
| WDB-MSK-004 | HARD | Ohne verbleibenden Kandidaten resultiert grundsätzlich Unknown. | Truth table |
| WDB-MSK-005 | HARD | EventMask ist keine EventRetraction und cascadiert nicht zu Assertions. | Truth table, DF |
| WDB-RES-001 | HARD | Fachoutcomes sind Known, Unknown oder Conflict; Technikfehler separat. | CF, UT |
| WDB-RES-002 | HARD | Keine ResolutionPolicy verwendet implizites Last Write Wins. | Truth table, DF |
| WDB-RES-003 | HARD | Securityfilter läuft vor Masking und Resolution. | NI, DF |
| WDB-RES-004 | HARD | ReplacementBoundary ist nur für MultiValueReplace gültig. | UT, FZ |
| WDB-RES-005 | HARD | ReplacementBoundary kann eine explizit vollständige leere Menge erzeugen. | Truth table |
| WDB-RES-006 | HARD | Resolutionausgabe besitzt deterministische kanonische Ordnung. | PT, CT |

## 4. Werte und Domain Types

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-VAL-001 | HARD | Core-Value enthält weder Null, Float, JSON, Array noch Map. | CF, FZ |
| WDB-VAL-002 | HARD | Equality ist typisiert und coercion-frei. | PT |
| WDB-VAL-003 | HARD | Decimal besitzt genau eine kanonische Darstellung. | PT, CT |
| WDB-VAL-005 | HARD | Decimal-Scale ist nicht Teil numerischer Identität; Darstellungs-/Messpräzision liegt in Schema/Metadaten. | PT, CT |
| WDB-VAL-006 | HARD | CalendarPeriod ist 1.0 kein Assertion-Value. | CF, Wire FZ |
| WDB-VAL-004 | HARD | `Value` besitzt kein globales fachliches `Ord`. | CF |
| WDB-ID-001 | HARD | Domänen-IDs sind nicht vertauschbare Newtypes. | CF |
| WDB-ID-002 | HARD | IDs sind 128 Bit; Zero und All-FF sind ungültig. | PT, FZ |
| WDB-ID-003 | HARD | ID-Zeitanteile definieren keine fachliche Zeit oder Authentizität. | UT, Review |
| WDB-ID-004 | HARD | BranchId, SchemaVersionId und typgelöschte LifecycleRecordId existieren nicht. | CF, API snapshot |
| WDB-REF-001 | HARD | Persistierter/Wire-RecordRef ist eine geschlossene exhaustive Enum. | CF, CT, FZ |
| WDB-REF-002 | HARD | Evidence-, Provenance- und Lifecycle-Targets sind validierte RecordRef-Teilmengen. | PT, FZ |
| WDB-REF-003 | HARD | EvidenceTargetRef erlaubt ausschließlich die in §31.2.1 mit J markierten Varianten. | CF, PT, FZ |
| WDB-REF-004 | HARD | Corrects verlangt dieselbe kompatible Recordfamilie und konkrete Lifecycle-Variante. | PT, FZ |
| WDB-REF-005 | HARD | DerivedFrom- und ResultedFrom-Endpunkte folgen den geschlossenen Matrizen ohne Other-/String-Escape-Hatch. | CF, PT, FZ |
| WDB-SEN-001 | HARD | Kein Sentinel codiert Abwesenheit, Fehler oder Lifecycle. | Policy, FZ |
| WDB-SEN-002 | HARD | Option, Result und Domain-Enum bleiben semantisch getrennt. | API review, CF |
| WDB-TYP-001 | HARD | Fremddaten umgehen keine validierenden Konstruktoren. | FZ, API test |
| WDB-TYP-002 | HARD | Handles/Transactions/Snapshots sind nur bei expliziter Semantik klonbar. | CF |

## 5. Schema, Events, Evidence und Migration

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-SCH-001 | HARD | Es gibt genau eine projektweite Schemahistorie. | SM, CT |
| WDB-SCH-002 | HARD | `SchemaMode::Historical` ist Default und nutzt SchemaAt(RecordedAsOf). | API, DF |
| WDB-SCH-003 | HARD | Alle Schema-IDs, einschließlich PredicateId, EntityTypeId, EventKindId, EventRoleId, EventAttributeId und LayerId, bleiben über Active/Deprecated/Retired stabil. | Migration golden |
| WDB-SCH-004 | HARD | ResolutionPolicy ist Predicate-Semantik und historisiert. | DF, CT |
| WDB-SCH-005 | HARD | PredicateDefinition enthält SubjectConstraint, ValueKind/ObjectConstraint, Cardinality, ResolutionPolicy, Constraints und Lifecycle. | CT, API test |
| WDB-SCH-006 | HARD | Entity Object Constraint ist genau für Entity-Values zulässig und wird strukturell validiert. | PT, FZ |
| WDB-SCH-007 | HARD | Single ist nur mit SingleValueReplace, Multi nur mit MultiValueOverlay/MultiValueReplace kompatibel. | Truth table, FZ |
| WDB-SCH-008 | HARD | Single verhindert nicht widersprüchliche Assertions; StructuralValidationError und ResolutionConflict bleiben getrennt. | Truth table, DF |
| WDB-SCH-009 | HARD | EventKindDefinition historisiert Rollen, Cardinalities, Attribute und Event-Time-Constraint vollständig. | CT, Migration golden |
| WDB-SCH-010 | HARD | Deprecated Writes verlangen Opt-in, Capability und Warning; Retired verbietet neue Writes. | E2E, NI |
| WDB-SCH-011 | HARD | Schema und Daten sind atomar; Referenzen validieren gegen das Post-Transaction-Schema. | SM, CR |
| WDB-SCH-012 | HARD | SchemaDependencies werden am Commitpoint gegen den aktuellen Head revalidiert. | SM, DF |
| WDB-SCH-013 | HARD | SchemaSnapshot ist derived/cacheable; Schemahistorie ist Wahrheit und Fingerprint ist kanonisch. | DF, CT |
| WDB-SCH-014 | HARD | Schemaänderungen werden nach Auswirkung als kompatibel, restrictive oder breaking klassifiziert. | Migration golden |
| WDB-SCH-015 | HARD | Bedeutungsinkompatible Änderungen benötigen eine neue Schema-ID; reine additive/metadatenbezogene Änderungen nicht. | Migration golden, ADR test |
| WDB-SCH-016 | HARD | Historical, Current und Explicit sind bewusste SchemaModes; keine stille Current-Coercion. | DF, API test |
| WDB-SCH-017 | HARD | Beschädigtes benötigtes Historical Schema scheitert explizit und fällt nicht auf Current zurück. | Corruption E2E, NI |
| WDB-EVT-001 | HARD | Event und Assertion sind getrennte First-Class-Records. | CF, UT |
| WDB-EVT-002 | HARD | Event erzeugt keine automatische Assertion/Inference. | DF |
| WDB-EVT-003 | HARD | EventMask cascadiert nicht automatisch zu Assertions. | Truth table |
| WDB-EVT-004 | HARD | Participants verwenden ausschließlich im EventKind erlaubte Rollen. | PT, FZ |
| WDB-EVT-005 | HARD | Role Cardinality und Required-Rollen werden strukturell validiert. | PT, FZ |
| WDB-EVT-006 | HARD | Participant-Reihenfolge ist bedeutungslos; identische Role/Entity-Paare sind unzulässige Duplikate. | PT, CT |
| WDB-EVT-007 | HARD | EventKind-Schema bestimmt erlaubte/erforderliche Rollen, Attribute, Typen und Zeitconstraint. | CT, FZ |
| WDB-EVT-008 | HARD | EventTime ist Instant oder halb-offener Span; offene Spans schließen nur durch EventSpanClosure. | PT, SM |
| WDB-EVT-009 | HARD | Gleiche WorldTime erzeugt keine implizite Eventordnung. | PT, DF |
| WDB-EVT-010 | HARD | Die Engine dedupliziert Events nicht automatisch. | SM, DF |
| WDB-EVT-011 | HARD | Witness impliziert kein Wissen; subjektive Erinnerung/Behauptung ist kein perspektivisches Event. | Truth table, DF |
| WDB-EVT-012 | HARD | Eventkorrektur erzeugt neuen Event plus Corrects und mutiert/retractet nicht implizit. | SM, DF |
| WDB-EVT-013 | HARD | EventMask ist keine Retraction, nimmt Wirkungen nicht zurück und cascadiert nicht. | Truth table, DF |
| WDB-EVT-014 | HARD | EventRelation ist ein eigener immutable Record mit Before/After/SameTime/Causes. | CF, CT |
| WDB-EVT-015 | HARD | Zeitwerte inferieren keine Eventrelation; Causes entsteht niemals aus zeitlicher Nähe. | DF, Truth table |
| WDB-EVT-016 | HARD | Event-Self-Relations sind unzulässig; SameTime wird symmetrisch kanonisiert. | PT, FZ |
| WDB-EVT-017 | HARD | Der normalisierte Before-Graph und der separate Causes-Graph sind azyklisch; aktive Duplikatkanten sind unzulässig. | SM, PT |
| WDB-EVT-018 | HARD | EventRelation besitzt keine World-Time-Validity und wird nur durch konkreten Retraction-Record korrigiert. | SM, CT |
| WDB-EVT-019 | HARD | `After(A,B)` ist nur Eingabealias und wird vor Equality/Persistenz zu `Before(B,A)` normalisiert; After wird nicht persistiert. | PT, FZ, wire golden |
| WDB-EVT-020 | HARD | SameTime wird als geordnetes EventId-Paar kanonisiert und als Äquivalenzrelation ausgewertet, ohne transitive Records zu materialisieren. | PT, SM, DF |
| WDB-EVT-021 | HARD | Duplikatprüfung verwendet ausschließlich den normalisierten Relation-Key und erfasst dadurch inverse Before/After-Duplikate. | PT, import negative |
| WDB-EVT-022 | HARD | Der Before-Graph ist nach SameTime-Komponentenkollaps azyklisch; Before innerhalb einer Komponente und SameTime über einen Before-Pfad sind Konflikte. | SM, PT, truth table |
| WDB-EVT-023 | HARD | Causes ist zeitlich orthogonal, impliziert kein Before und besitzt nur seine separate Self-/Duplikat-/Zyklusprüfung, sofern Schema keinen strengeren Constraint setzt. | Truth table, DF, schema test |
| WDB-EPI-001 | HARD | WorldState, Knows, Believes und Claims implizieren einander nicht automatisch. | Truth table, DF |
| WDB-EPI-002 | HARD | Fehlendes Knows/Believes und negiertes Knows/Believes bleiben verschieden. | Truth table |
| WDB-EPI-003 | HARD | PerspectiveId und PrincipalId sind nicht austauschbar. | CF, NI |
| WDB-SRC-001 | HARD | Source ist ein eigener immutable Herkunftsrecord; ohne Evidence-Relation behauptet er nichts über einen Domain-Record. | UT, DF |
| WDB-SRC-002 | HARD | Source-Felder werden vor Public-Ausgabe autorisiert; Auslassung und Fehlermapping leaken keine Source-Existenz. | NI, leak corpus |
| WDB-EVI-001 | HARD | Evidence verändert Resolution nicht automatisch. | DF |
| WDB-PRV-001 | HARD | Provenance ist erklärend und nicht imperativ/cascading. | DF |
| WDB-EVI-002 | HARD | Evidence ist projektweite immutable Meta-History ohne eigene World-Time-Validity. | DF, CT |
| WDB-EVI-003 | HARD | RecordedAsOf filtert Evidence nach Erzeugungs-/Retraction-Revision. | DF, SM |
| WDB-EVI-004 | HARD | base_revision beschneidet Targetsichtbarkeit, nicht die globale Evidence-History. | DF, NI |
| WDB-EVI-005 | HARD | Evidence wird nur ausgegeben, wenn Target und Sourcefelder autorisiert sichtbar sind. | NI |
| WDB-EVI-006 | HARD | Evidence darf historische/retracted Targets dokumentieren, reaktiviert sie aber nicht. | SM, DF |
| WDB-EVI-007 | HARD | Aktive Evidence-Duplikate gleichen Source/Target/Relation-Tupels sind unzulässig. | PT, SM |
| WDB-PRV-002 | HARD | Corrects bezeichnet Datenkorrektur, keine World-Time-Nachfolge, und retractet nicht automatisch. | SM, DF |
| WDB-PRV-003 | HARD | DerivedFrom bezeichnet informationelle Ableitung und keine Story-Kausalität. | Truth table, DF |
| WDB-PRV-004 | HARD | ResultedFrom bezeichnet fachliche Wirkung, keine logische Ableitung, und cascadiert nicht. | Truth table, DF |
| WDB-PRV-005 | HARD | Provenance ist projektweite Meta-History ohne ContextPrecedence oder Resolutionwirkung. | DF, CT |
| WDB-PRV-006 | HARD | Provenance-Self-Loops sind unzulässig. | PT, FZ |
| WDB-PRV-007 | HARD | Der gemeinsame Provenance-Abhängigkeitsgraph ist relationsintern und relationsübergreifend azyklisch. | SM, PT |
| WDB-PRV-008 | HARD | Aktive Provenance-Duplikate gleichen From/To/Relation-Tupels sind unzulässig. | PT, SM |
| WDB-PRV-009 | HARD | Graphvalidierung und Traversal sind bounded; unentscheidbare Insertprüfung scheitert fail-closed. | PF, SM |
| WDB-PRV-011 | HARD | Dependency-Projektion ist Corrects(new,old): new→old; DerivedFrom(source,derived): derived→source; ResultedFrom(cause,effect): effect→cause. | Truth table, CT |
| WDB-PRV-012 | HARD | Gemischte Zyklen über Corrects, DerivedFrom und ResultedFrom werden abgewiesen. | SM, PT |
| WDB-PRV-013 | HARD | Zyklusvalidierung prüft den atomaren Post-Transaction-Zustand einschließlich aller Batchkanten. | SM, OCC integration |
| WDB-PRV-010 | HARD | Meta-History-Security verwendet die Schnittmenge der Endpoint-/Feldrechte und leakt keine versteckten Kanten. | NI |
| WDB-LFC-001 | HARD | Close Validity, Retraction, Archive und Purge sind getrennte Record-/Operationsverträge. | SM, DF |
| WDB-LFC-002 | HARD | Jeder Lifecycle-Record besitzt eine konkrete getypte ID und ist über RecordRef referenzierbar. | CF, CT |
| WDB-MIG-001 | HARD | Migration überschreibt keine committed History. | SM, CR |
| WDB-MIG-002 | HARD | Restrictive/Breaking besitzen Dry Run. | E2E |
| WDB-MIG-003 | HARD | Breaking verlangt Adminaktion und standardmäßig Restore Point. | Security E2E |
| WDB-MIG-004 | HARD | Jeder Multi-Transaction-Zwischenstand ist gültig und historisch lesbar. | SM, CR |
| WDB-MIG-005 | HARD | Storageformat- und Schema-Migration sind getrennte Protokolle. | CT, E2E |
| WDB-MIG-006 | HARD | MigrationCategory ist exakt MetadataOnly, Additive, CompatibleConstraintChange, Restrictive oder Breaking. | CF, CT |
| WDB-MIG-007 | HARD | MigrationPlan und MigrationRun sind getrennte Typen mit eigenen Identitäten. | CF, API test |
| WDB-MIG-008 | HARD | Plan enthält Source-Schema-Precondition, Target, Steps, Fingerprint, Transformer-Version und Budgets. | CT, FZ |
| WDB-MIG-009 | HARD | Start und Resume weisen eine abweichende Source-Schema-Precondition zurück. | SM, E2E |
| WDB-MIG-010 | HARD | Dry Run und Execution verwenden dieselbe Transformationslogik. | DF |
| WDB-MIG-011 | HARD | Commit-Transformer sind deterministisch und verwenden weder Uhr, Zufall, Locale, Netzwerk noch AI. | ST, DF |
| WDB-MIG-012 | HARD | AI-Vorschläge werden nur als versionierter kanonischer Plan committed; versteckte Coercions bleiben verboten. | E2E, CT |
| WDB-MIG-013 | HARD | Warning, UnresolvedMigrationItem und Error sind getrennte Zustände. | CF, API test |
| WDB-MIG-014 | HARD | Restrictive/Breaking committen unresolved Items nur mit expliziter validierter Adminentscheidung. | E2E, NI |
| WDB-MIG-015 | HARD | Representation Migration ändert keine World-Time-Semantik; historische/retracted Records nur bei explizitem Plan. | DF, Migration golden |
| WDB-MIG-016 | HARD | Jeder Stepcommit ist eine normale OCC-Transaction. | SM, CR |
| WDB-MIG-017 | HARD | MigrationId, StepId, RunId und OperationId besitzen getrennte Semantik. | CF, CT |
| WDB-MIG-018 | HARD | Step-Resume klärt Unknown Outcome über OperationId und ist inputfingerprint-idempotent. | CR, E2E |
| WDB-MIG-019 | HARD | Expand → Migrate → Contract ist Default und jeder Zwischenstand ist gültig. | SM, Migration golden |
| WDB-MIG-020 | HARD | Committed Migration-History wird nicht zurückgerollt. | SM, CR |
| WDB-MIG-021 | HARD | Fachliche Rücknahme erfolgt als neue Compensating Migration. | E2E, DF |
| WDB-MIG-022 | HARD | Migration Run Journal ist recoverbares Koordinationsmetadatum, nicht normative History. | CR, CT |
| WDB-MIG-023 | HARD | Storage-Format-Migration bleibt vom fachlichen Migrationsprotokoll getrennt. | E2E, CT |

## 6. Transactions, OCC und Snapshots

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-TX-001 | HARD | Commit konsumiert eine validierte Transaktion genau einmal. | CF, SM |
| WDB-TX-002 | HARD | Drop führt weder Commit noch persistente Reparatur aus. | UT, CR |
| WDB-TX-003 | HARD | Schema, Daten, Event, Assertion und Provenance können atomar committed werden. | SM, CR |
| WDB-TX-004 | HARD | OperationId dedupliziert einen logischen Commitversuch persistent. | CR, E2E |
| WDB-TX-005 | HARD | Gleiche OperationId mit abweichendem Payload wird abgelehnt. | UT, CR |
| WDB-TX-006 | HARD | Commitantwort wird erst nach durablem Commitpoint erzeugt. | CR |
| WDB-OCC-001 | HARD | Write/Write- und relevante Write/Read-Konflikte werden erkannt. | SM, DF |
| WDB-OCC-002 | HARD | Range-/Predicate-Dependencies erkennen Phantoms. | SM, DF |
| WDB-OCC-003 | HARD | TransactionConflict ist erwartbares Outcome, kein Storagefehler. | API snapshot |
| WDB-OCC-004 | HARD | Auto-Retry gilt nur für ReplaySafe ohne externe Side Effects und ist begrenzt. | SM |
| WDB-OCC-005 | HARD | UnknownCommitOutcome wird vor Retry über OperationId geklärt. | CR, E2E |
| WDB-SNP-001 | HARD | Snapshot pinnt Daten, Schema, HistorySpace, LayerSelection samt historischen LayerDefinitionen und Securitymodus gemeinsam. | SM |
| WDB-SNP-002 | HARD | Snapshot bewegt sich nie still auf eine neue Revision. | DF |
| WDB-SNP-003 | HARD | Gepinnte Segmente werden nicht reclaimed. | CR, PF |
| WDB-SNP-004 | HARD | Öffentliche Queryresultate borgen nicht aus Lock oder Memory Map. | CF, Miri |

## 7. Storage, Format und Recovery

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-STO-001 | HARD | Backend publiziert höchstens eine neue Revision atomar. | Contract SM, CR |
| WDB-STO-002 | HARD | Engine- und Backendgarantien sind getrennt und testbar. | Contract suite |
| WDB-STO-003 | HARD | Produktionscommit verlangt Machine-Durability. | CR, CP |
| WDB-STO-004 | HARD | SegmentId ist eine zufällige stabile Identität ohne Content-Semantik. | PT, CT |
| WDB-STO-005 | HARD | ContentDigest hasht kanonische Segmentbytes getrennt von SegmentId. | PT, CT, FZ |
| WDB-STO-006 | HARD | Manifest, Backup und Recovery verifizieren SegmentId und ContentDigest getrennt. | CR, E2E |
| WDB-WIR-001 | HARD | Persistentes Format ist explizit versioniert und serde-implementierungsunabhängig. | CT |
| WDB-WIR-002 | HARD | Varints, Felder und Decimal sind kanonisch; alternative Encodings werden abgewiesen. | PT, FZ |
| WDB-WIR-003 | HARD | Decoder prüft Limits und Overflows vor Allokation/Cast. | FZ, PF |
| WDB-WIR-004 | HARD | TypeScript transportiert i128/u128/Decimal/Revision verlustfrei als Strings. | E2E, PT |
| WDB-WIR-005 | HARD | Semantische Hard-Limits, Defaultprofile und provisorische Performancegates sind getrennt klassifiziert. | Spec/policy test |
| WDB-WAL-001 | HARD | Commitmarker wird erst nach synchronisiertem Prepare geschrieben. | CR |
| WDB-WAL-002 | HARD | Zweiter WAL-Sync ist Commitpoint. | CR |
| WDB-WAL-003 | HARD | Manifest referenziert nie Daten oberhalb des sicheren WAL-Präfix. | CR, SM |
| WDB-WAL-004 | HARD | Historysegmente sind immutable. | Contract test |
| WDB-WAL-005 | HARD | Publication berücksichtigt Datei- und Verzeichnissync pro Plattform. | CP, CR |
| WDB-REC-001 | HARD | Recovery macht nur vollständig verifizierten committed Präfix sichtbar. | CR, FZ |
| WDB-REC-002 | HARD | Uncommitted Tail wird niemals als Commit interpretiert. | CR |
| WDB-REC-003 | HARD | Korruption im sicheren Bereich wird nicht automatisch repariert. | Corruption E2E |
| WDB-REC-004 | HARD | Recovery ist über wiederholte Abstürze idempotent. | Nested crash test |
| WDB-REC-005 | HARD | Salvage überschreibt nie das Original und berichtet Verluste. | E2E |

## 8. Backup, Export und Purge

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-BKP-001 | HARD | ExactDatabaseBackup enthält alle für den Daten-/Schemasnapshot erforderlichen Bytes und History. | Restore DF |
| WDB-BKP-002 | HARD | Backup gilt erst nach Ziel-Verify als erfolgreich. | Corruption test |
| WDB-BKP-003 | HARD | Restore publiziert nur in ein leeres Ziel und erst nach Recovery/Verify. | CR, E2E |
| WDB-BKP-004 | HARD | Backup-Digest beweist Integrität; Authentizität benötigt separat MAC oder Signatur. | CT, tamper tests |
| WDB-BKP-005 | HARD | ExactDatabaseBackup und AuditCompleteBackup sind getrennte Profile; fehlende Auditdaten werden ausdrücklich als `audit_scope=Excluded` manifestiert. | CT, restore DF |
| WDB-EXP-001 | HARD | Logical Export manifestiert Scope und ausgelassene Datenklassen. | CT |
| WDB-EXP-002 | HARD | Import remappt IDs nur über expliziten protokollierten Plan. | PT, E2E |
| WDB-PRG-001 | HARD | Purge 1.0 ist Offline-Rewrite, kein In-place-History-Umschreiben. | E2E, CT |
| WDB-PRG-002 | HARD | Secure Erase wird für SSD/COW/Backups nicht behauptet. | Documentation test |

## 9. API, Security, Observability und Audit

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-API-001 | HARD | Streamende ist `None`; Konstruktion-, Item-, Cancel- und Budgetfehler sind getrennt. | API UT |
| WDB-API-002 | HARD | Paginationcursor bindet Snapshot und Queryhash. | PT, E2E |
| WDB-API-003 | HARD | Ungeordnete Resultate erhalten kanonische Ordnung. | PT, CT |
| WDB-API-004 | HARD | Unvollständige Aggregation wird nie als vollständig markiert. | Budget E2E |
| WDB-API-005 | HARD | Cursor bindet Snapshot, QueryHash und EngineSession und verfällt beim Engine-Neustart. | E2E, key rotation tests |
| WDB-API-006 | HARD | Cursorwire enthält ausschließlich public-safe Daten; sensitiver Payload bleibt im bounded Session-State. | NI, FZ |
| WDB-API-007 | HARD | Cursor bindet Principal, effektive Capabilities, SecurityEpoch, Snapshot, QueryHash, Session und Expiry. | NI, E2E |
| WDB-API-008 | HARD | AuthorizationNow und SecurityEpoch werden vor jeder Seite neu geprüft; relevante Änderung invalidiert den Cursor. | NI, SM |
| WDB-API-009 | HARD | Öffentliche Cursorfehler vereinheitlichen unbekannt, abgelaufen, manipuliert und security-invalidiert, wenn Differenzierung leaken würde. | NI |
| WDB-SEC-001 | HARD | Perspective ist kein Security Principal. | CF, NI |
| WDB-SEC-002 | HARD | Authorization erfolgt vor Redaction und Resolution. | NI |
| WDB-SEC-003 | HARD | Unsichtbare Records beeinflussen öffentliche Shape/Codes/Counts/Explain nicht. | NI |
| WDB-SEC-004 | HARD | Graphsuche exponiert weder versteckte Knoten noch versteckte Kantenzahlen. | NI |
| WDB-SEC-005 | HARD | Historical Permission Mode ist explizit; Default ist AuthorizationNow. | API, NI |
| WDB-ERR-001 | HARD | `Display`, `Debug`, `Hash`, `Eq`, `Ord`, `Drop` haben keine fachlichen Side Effects. | Policy, UT |
| WDB-ERR-002 | HARD | Öffentliche Errors enthalten stabile Codes und keine geheimen Ursachen. | Snapshot, NI |
| WDB-ERR-003 | HARD | `source()` erhält interne Ursachen ohne sie öffentlich zu serialisieren. | UT |
| WDB-ERR-004 | HARD | `Debug` öffentlicher/operativer Errors respektiert dieselbe Leak-Grenze wie `Display`. | Snapshot, canary NI |
| WDB-ERR-005 | HARD | Security-, Recovery-, Retry- und Public-API-Mappings sind exhaustive und besitzen keinen semantischen Catch-all. | Compile/API tests |
| WDB-ERR-006 | HARD | Niedrige Errors enthalten Error Facts; SuggestedAction/RecoveryPolicy wird an höherer Boundary entschieden. | Type/API tests |
| WDB-OBS-001 | HARD | Telemetriefehler blockieren keine Domainoperation. | Fault test |
| WDB-OBS-002 | HARD | Logs enthalten standardmäßig keine Values, Querytexte, Pfade oder Principalnamen. | Capture/NI |
| WDB-OBS-003 | HARD | Kein Sync-/Span-Guard wird über `.await` gehalten. | Clippy/custom lint |
| WDB-OBS-004 | HARD | Nicht klassifizierte Diagnosewerte sind `Omitted`; `Shown` und `Hashed` sind opt-in. | Capture/NI |
| WDB-AUD-001 | HARD | Durable-Audit-Policyaktionen sind fail-closed. | Fault/CR |
| WDB-AUD-002 | HARD | Audit und Telemetrie besitzen getrennte Retention und Berechtigungen. | E2E |
| WDB-AUD-003 | HARD | Auditpflichtige Domain Action und Required Audit Record erreichen atomar denselben Commitpoint. | CR, E2E |
| WDB-AUD-004 | HARD | Reines Raw-Read-Audit verwendet eigene AuditSequence/WAL und erzeugt keine WorldDB-Datenrevision. | CT, CR |
| WDB-AUD-005 | HARD | Vor jeder Raw-Read-Seitenausgabe ist RawReadAttempt durable; Auditfehler blockiert Ausgabe. | CR, E2E |
| WDB-AUD-006 | HARD | RawReadAttempt bindet Principal, Scope, Snapshot, SecurityEpoch und PageOrdinal ohne sensitive Nutzdaten. | NI, CT |
| WDB-AUD-007 | HARD | Retries besitzen neue AuditOperationId, behalten ClientRequestId und bleiben als Mehrfachversuche sichtbar. | SM, CR |
| WDB-AUD-008 | HARD | Auditwriter ist serialisiert, hält keinen Datenwriter über I/O und wird vor Raw-Reads recovered. | SM, CR |
| WDB-AUD-009 | HARD | Korruption im committed Auditpräfix sperrt auditpflichtige Operationen und wird nicht automatisch repariert. | Corruption E2E, CR |
| WDB-AUD-010 | HARD | Nur AuditCompleteBackup enthält und verifiziert Auditlog, Auditmanifest und deklarierte audit_safe_sequence. | Restore DF, E2E |
| WDB-AUD-011 | HARD | Audit-Retention und -Zugriff sind policy-versioniert, separat autorisiert und selbst auditiert. | E2E, NI |
| WDB-AUD-012 | HARD | audit_safe_sequence ist vom WorldDB-safe_revision unabhängig; Raw-Read-Audit erzeugt keine künstliche Datenrevision. | SM, restore DF |
| WDB-AUD-013 | HARD | Restore eines AuditCompleteBackup recovered Daten- und Auditnamespace separat und gibt Auditoperationen erst nach verifizierter Auditlineage frei. | CR, E2E |

## 10. Concurrency, Performance und Engineering

| ID | Klasse | Normative Aussage | Primäre Tests |
|---|---|---|---|
| WDB-CON-001 | HARD | Genau ein Writer publiziert; N Reader lesen gepinnte Snapshots. | Stress, SM |
| WDB-CON-002 | HARD | Kein Lockguard wird über I/O, Callback, IPC oder await gehalten. | Loom/custom lint |
| WDB-CON-003 | HARD | Queues und Pools besitzen harte Grenzen und Backpressure. | Load, PF |
| WDB-CON-004 | HARD | Cancellation nach Commitpoint macht Commit nicht teilweise rückgängig. | CR, E2E |
| WDB-OWN-001 | HARD | Shared Handles werden mit `Arc::clone`/`Rc::clone` von kopierten Fachwerten unterscheidbar gemacht. | Custom lint, review test |
| WDB-OWN-002 | HARD | Clone, `'static` und Taskownership dürfen keine ungeklärte Ownership kaschieren. | Policy, compile/API tests |
| WDB-OWN-003 | HARD | Borrowed-to-Owned-Konversionen bleiben an Boundaries sichtbar. | API review, custom lint |
| WDB-IDX-001 | HARD | Indexresultate sind differential gleich zum Full-Scan-Orakel. | DF |
| WDB-IDX-002 | HARD | Staler/fehlender Index liefert nie still unvollständige Daten. | Fault, DF |
| WDB-IDX-003 | HARD | Indexgeneration wird atomar vollständig publiziert. | CR |
| WDB-PER-001 | GUARDED | Performanceoptimierung benötigt reproduzierbare Messung. | Benchmark review |
| WDB-ENG-001 | HARD | Core hängt nicht von UI, konkretem Storage oder Observability-Adapter ab. | Crate graph lint |
| WDB-ENG-002 | HARD | Core-Domainerrors verwenden keine universelle Error-Erasure. | Dependency/API lint |
| WDB-ENG-003 | HARD | Produktionspfade paniken nicht auf falliblen Fremddaten. | Lint, FZ |
| WDB-ENG-004 | HARD | `unsafe` ist außerhalb des genehmigten Plattformadapters verboten. | Workspace lint |
| WDB-ENG-005 | HARD | Der kanonische Verify-Pfad ist lokal und in CI identisch. | CI manifest |
| WDB-ENG-006 | GUARDED | Lokale Lint-Ausnahme besitzt ID, Owner, Grund und Ablaufdatum. | Policy test |
| WDB-ENG-007 | HARD | Crates werden nur nach bestandenem Boundary-Gate extrahiert; der Repositorybaum ist Zielhypothese. | Crate graph/policy test |
| WDB-DEP-001 | HARD | Releasebuilds verwenden Lockfile und genehmigte Sources/Lizenzen. | CI, SBOM |
| WDB-DEP-002 | HARD | Dependencytypen leaken nicht unabsichtlich in stabile Public API. | API diff |
| WDB-EXT-001 | HARD | 1.0 führt keinen untrusted Extensioncode in-process aus. | E2E security |
| WDB-DES-001 | HARD | Desktopfrontend kann Storage/Dateisystem nicht außerhalb versionierter IPC umgehen. | E2E security |
| WDB-DES-002 | HARD | Core lauscht standardmäßig auf keinem Netzwerkport. | E2E |

## 11. Abdeckungs- und Freigaberegel

Das Repository führt `invariants.toml` als maschinenlesbaren Spiegel mit Owner, Status und Testnamen. `cargo xtask verify-invariants` scheitert, wenn eine aktive HARD-Invariante keinen registrierten Test besitzt, ein Testname nicht existiert oder eine Kennung doppelt ist. Diese Zuordnung beweist nicht automatisch Testqualität; Review und Mutation/Fault Testing prüfen, ob der Test die Regel tatsächlich brechen kann.


---
