# WorldDB – Consolidation Trace

**Stand:** 20. September 2026  
**Basis:** aktuelle Masterfassung, die im Arbeitsauftrag vollständig wiedergegebenen früheren WorldDB-Verträge, die regelweisen Nacharbeitslisten und `pasted.txt` mit Mias stabilen Regeln. Eine eigenständige Datei der Gesamtspezifikation v3.1 war nicht verfügbar; sie wurde nicht simuliert.  
**Statuswerte:** `PRESERVED`, `CHANGED`, `RETIRED`, `SUPERSEDED`, `DUPLICATE`.

Jede Tabellenzeile ist eine einzelne Quellregel. Bereichszeilen und Regelbereiche als Ersatz für Einzelzuordnung sind unzulässig.

## 1. Fachlicher Baseline-Vertrag

| Previous Rule ID | Previous normative statement | Final location | Status | New Rule ID | ADR if changed | Test evidence |
|---|---|---|---|---|---|---|
| LEGACY-HIS-01 | Revision-History ist monoton. | Master §2.1 | PRESERVED | WDB-HIS-001 | – | SM, CR |
| LEGACY-HIS-02 | Revision ist Transaction Time. | Master §§1, 2.1 | PRESERVED | WDB-HIS-002 | – | UT, DF |
| LEGACY-HIS-03 | Revision bestimmt keine fachliche Precedence. | Master §§1, 2.1.2 | PRESERVED | WDB-HIS-002 | – | DF |
| LEGACY-HIS-04 | Historical Queries sind reproduzierbar. | Master §2.1 | PRESERVED | WDB-HIS-004 | – | DF, CT |
| LEGACY-HIS-05 | Committed History wird normal nicht rückwirkend überschrieben. | Master §§2.1, 15.3 | PRESERVED | WDB-HIS-003 | – | SM, CR |
| LEGACY-BRA-01 | Child erbt Parent nur bis base_revision. | Master §2.1.1 | PRESERVED | WDB-BRA-001 | – | PT, DF |
| LEGACY-BRA-02 | Spätere Parent-Änderungen fließen nicht automatisch ein. | Master §2.1 | PRESERVED | WDB-BRA-002 | – | SM, DF |
| LEGACY-BRA-03 | Sibling-HistorySpaces sind isoliert. | Master §2.1 | PRESERVED | WDB-BRA-003 | – | SM, DF |
| LEGACY-BRA-04 | Child mutiert Parent-History nicht. | Master §§2.1.1, 8 | PRESERVED | WDB-BRA-001/002, WDB-TX-003 | – | SM, CR |
| LEGACY-BRA-05 | Branches verändern Story-History, nicht Schemaidentitäten. | Master §§2.1, 2.4 | PRESERVED | WDB-BRA-004 | – | UT, CT |
| LEGACY-BRA-06 | Branch und HistorySpace waren getrennt benannt. | Master §2.1.1 | CHANGED | WDB-BRA-005 | ADR-021 | CF, API snapshot |
| LEGACY-TIM-01 | RecordedAsOf, WorldTime, EventTime und AssertionValidity sind getrennt. | Master §§2.1, 12 | PRESERVED | WDB-TIM-001 | – | CF, UT |
| LEGACY-TIM-02 | Keine globale Ordnung über inkompatible Timelines. | Master §2.1 | PRESERVED | WDB-TIM-002 | – | CF, PT |
| LEGACY-TIM-03 | Intervalle sind halb-offen `[start,end)`. | Master §§2.1, 31.1 | PRESERVED | WDB-TIM-003 | – | PT, DF |
| LEGACY-AST-01 | Assertions sind immutable. | Master §2.2 | PRESERVED | WDB-AST-001 | – | API, SM |
| LEGACY-AST-02 | Close World-Time Validity ist eigener Record. | Master §2.3.2 | PRESERVED | WDB-AST-002 | – | SM, DF |
| LEGACY-AST-03 | Retraction liegt auf Transaction Time. | Master §§2.2–2.3.2 | PRESERVED | WDB-AST-003 | – | Truth table, DF |
| LEGACY-AST-04 | Correct erzeugt neue Assertion plus Retraction/Provenance. | Master §§2.3.2, 31.2 | PRESERVED | WDB-AST-002, WDB-PRV-002 | – | SM, DF |
| LEGACY-MASK-01 | Masking ist keine Negation. | Master §§2.2–2.3.2 | PRESERVED | WDB-MSK-001 | – | Truth table |
| LEGACY-MASK-02 | MaskSelector ist ExactAssertion, Proposition oder Slot. | Master §2.3.2 | PRESERVED | WDB-MSK-003 | – | UT, FZ |
| LEGACY-MASK-03 | Mask wirkt nur auf strikt niedrigere ContextPrecedence. | Master §2.1.2 | PRESERVED | WDB-MSK-002 | – | Truth table |
| LEGACY-MASK-04 | Mask erzeugt keinen positiven oder negativen Fakt. | Master §2.2 | PRESERVED | WDB-MSK-001 | – | Truth table, DF |
| LEGACY-MASK-05 | Ohne verbleibenden Kandidaten ist das Ergebnis Unknown. | Master §2.2 | PRESERVED | WDB-MSK-004 | – | Truth table |
| LEGACY-VAL-01 | Core Value ist geschlossen auf Bool/Int/UInt/Decimal/String/Symbol/Entity/Time/Duration/Bytes. | Master §§2.3, 12 | PRESERVED | WDB-VAL-001 | – | CF, FZ |
| LEGACY-VAL-02 | Null, Float, JSON, Array und Map sind keine Core Values. | Master §§2.3, 12 | PRESERVED | WDB-VAL-001 | – | CF, FZ |
| LEGACY-VAL-03 | Equality ist typisiert und ohne Coercions. | Master §2.3 | PRESERVED | WDB-VAL-002 | – | PT |
| LEGACY-VAL-04 | Decimal ist exakt und kanonisch. | Master §12 | PRESERVED | WDB-VAL-003 | – | PT, CT |
| LEGACY-VAL-05 | Value besitzt kein globales fachliches Ord. | Master §3.2 | PRESERVED | WDB-VAL-004 | – | CF |
| LEGACY-PRO-01 | Proposition Equality besteht aus Subject, Predicate, Value, Polarity. | Master §2.2 | PRESERVED | WDB-PRO-001 | – | PT |
| LEGACY-RES-01 | Fachoutcomes sind Known, Unknown, Conflict. | Master §2.2 | PRESERVED | WDB-RES-001 | – | CF, UT |
| LEGACY-RES-02 | Technische ResolutionErrors bleiben getrennt. | Master §§2.2, 5 | PRESERVED | WDB-RES-001 | – | CF, UT |
| LEGACY-RES-03 | Policies sind SingleValueReplace, MultiValueOverlay, MultiValueReplace. | Master §§2.1.2, 31.3 | PRESERVED | WDB-SCH-007 | – | Truth table |
| LEGACY-RES-04 | Keine Last-Write-Wins-Semantik. | Master §§2.1.2, 9 | PRESERVED | WDB-RES-002 | – | Truth table, DF |
| LEGACY-RES-05 | Securityfilter liegt vor Masking und Resolution. | Master §§2.2, 17 | PRESERVED | WDB-RES-003 | – | NI, DF |
| LEGACY-RES-06 | ReplacementBoundary gilt nur MultiValueReplace. | Master §§2.2–2.3.2 | PRESERVED | WDB-RES-004 | – | UT, FZ |
| LEGACY-RES-07 | ReplacementBoundary kann eine vollständige leere Menge ausdrücken. | Master §2.2 | PRESERVED | WDB-RES-005 | – | Truth table |
| LEGACY-EPI-01 | WorldState ist kein Character-Belief. | Master §2.3.1 | PRESERVED | WDB-EPI-001 | – | Truth table |
| LEGACY-EPI-02 | Perspective ist kein Principal. | Master §§2.3.1, 17 | PRESERVED | WDB-EPI-003, WDB-SEC-001 | – | CF, NI |
| LEGACY-EPI-03 | EpistemicMode ist keine Precedence-Rangfolge. | Master §2.1.2 | PRESERVED | WDB-LAY-002 | – | Truth table |
| LEGACY-EPI-04 | Knows(P) impliziert nicht WorldState(P). | Master §2.3.1 | PRESERVED | WDB-EPI-001 | – | Truth table |
| LEGACY-EPI-05 | WorldState(P) impliziert nicht Knows(P). | Master §2.3.1 | PRESERVED | WDB-EPI-001 | – | Truth table |
| LEGACY-EPI-06 | Claims(P) impliziert nicht Believes(P). | Master §2.3.1 | PRESERVED | WDB-EPI-001 | – | Truth table |
| LEGACY-EPI-07 | Believes(P) impliziert nicht Claims(P). | Master §2.3.1 | PRESERVED | WDB-EPI-001 | – | Truth table |
| LEGACY-EPI-08 | NOT Believes(P) ist nicht Believes(NOT P). | Master §2.3.1 | PRESERVED | WDB-EPI-002 | – | Truth table |
| LEGACY-EPI-09 | NOT Knows(P) ist nicht Knows(NOT P). | Master §2.3.1 | PRESERVED | WDB-EPI-002 | – | Truth table |
| LEGACY-EPI-10 | Fehlendes Knows(P) bedeutet Unknown. | Master §2.3.1 | PRESERVED | WDB-EPI-002 | – | Truth table |
| LEGACY-EPI-11 | Fehlendes Believes(P) bedeutet Unknown. | Master §2.3.1 | PRESERVED | WDB-EPI-002 | – | Truth table |
| LEGACY-LAY-01 | Layer ist orthogonal zu HistorySpace, Perspective und Security. | Master §2.1.2 | PRESERVED | WDB-LAY-001 | ADR-022 | Schema/API tests |
| LEGACY-LAY-02 | ContextPrecedence ist HistorySpace-Spezifität vor LayerRank. | Master §2.1.2 | PRESERVED | WDB-LAY-002 | ADR-022 | Truth table, DF |
| LEGACY-LAY-03 | Perspective/EpistemicMode partitionieren und ranken nicht. | Master §2.1.2 | PRESERVED | WDB-LAY-002/003 | – | Truth table |
| LEGACY-LAY-04 | Layerrechte verändern Precedence nicht. | Master §§2.1.2, 17 | PRESERVED | WDB-LAY-004 | – | NI, DF |
| LAY-BASE-01 | Jeder historische Schemastand besitzt genau einen bezeichneten Base-Layer. | Master §2.1.2 | CHANGED | WDB-LAY-005/007 | ADR-022 | Schema golden, DF |
| LAY-BASE-02 | Der Base-Layer ist der eindeutig niedrigste aktive PrecedenceRank. | Master §2.1.2 | CHANGED | WDB-LAY-008 | ADR-022 | PT, schema validation |
| LAY-BASE-03 | Layerfähige Records tragen auch im Base-Layer eine ausdrückliche LayerId. | Master §2.1.2 | PRESERVED | WDB-LAY-001/009 | ADR-022 | FZ, CT |
| LAY-BASE-04 | Base-Layer-Wechsel ist atomar historisiert und verschiebt keine Records. | Master §2.1.2 | CHANGED | WDB-LAY-010 | ADR-022 | SM, migration golden |
| LAY-BASE-05 | BaseOnly wird gegen den gepinnten Query-Schemasnapshot aufgelöst. | Master §2.1.2 | CHANGED | WDB-LAY-011 | ADR-022 | DF, historical query |
| LEGACY-LFC-01 | Close Validity ist keine Retraction. | Master §2.3.2 | PRESERVED | WDB-LFC-001 | – | SM, DF |
| LEGACY-LFC-02 | EventMask ist keine EventRetraction. | Master §§2.3.2, 31.1 | PRESERVED | WDB-MSK-005, WDB-EVT-013 | – | Truth table |
| LEGACY-LFC-03 | Corrects löst keine Retraction aus. | Master §31.2 | PRESERVED | WDB-PRV-002 | – | SM, DF |
| LEGACY-LFC-04 | ResultedFrom löst keine Cascade aus. | Master §31.2 | PRESERVED | WDB-PRV-004 | – | Truth table |
| LEGACY-LFC-05 | Archive, Retract und Purge sind verschieden. | Master §§2.3.2, 15.3 | PRESERVED | WDB-LFC-001 | – | SM, DF |

## 2. Event-Detailvertrag

| Previous Rule ID | Previous normative statement | Final location | Status | New Rule ID | ADR if changed | Test evidence |
|---|---|---|---|---|---|---|
| EVT-01 | Participants sind rollenbasiert und Rollen schemaerlaubt. | Master §31.1 | PRESERVED | WDB-EVT-004 | – | PT, FZ |
| EVT-02 | Role Cardinality ist zu validieren. | Master §31.1 | PRESERVED | WDB-EVT-005 | – | PT, FZ |
| EVT-03 | Identische role/entity-Paare sind keine Mehrfachteilnahme. | Master §31.1 | PRESERVED | WDB-EVT-006 | – | PT, CT |
| EVT-04 | Participant-Reihenfolge hat keine Semantik. | Master §31.1 | PRESERVED | WDB-EVT-006 | – | PT, CT |
| EVT-05 | EventKind-Schema definiert Rollen, Attribute und Zeitconstraints. | Master §§31.1, 31.3 | PRESERVED | WDB-EVT-007, WDB-SCH-009 | – | CT, FZ |
| EVT-06 | Erlaubte/erforderliche Attribute werden strukturell geprüft. | Master §31.1 | PRESERVED | WDB-EVT-007 | – | FZ |
| EVT-07 | EventTime kennt Instant und Span. | Master §31.1 | PRESERVED | WDB-EVT-008 | – | PT, SM |
| EVT-08 | Offene Spans werden durch EventSpanClosure geschlossen. | Master §31.1 | PRESERVED | WDB-EVT-008 | – | SM |
| EVT-09 | Gleiche WorldTime erzeugt keine implizite Ordnung. | Master §31.1 | PRESERVED | WDB-EVT-009 | – | PT, DF |
| EVT-10 | Eventrelationen sind explizite Records. | Master §31.1 | PRESERVED | WDB-EVT-014 | ADR-029 | CF, CT |
| EVT-11 | Causes wird nicht aus zeitlicher Nähe inferiert. | Master §31.1 | PRESERVED | WDB-EVT-015 | ADR-029 | DF, Truth table |
| EVT-12 | Self-Relations und gerichtete Zyklen sind geregelt. | Master §31.1 | SUPERSEDED | WDB-EVT-016/017 | ADR-029 | SM, PT |
| EVT-13 | Keine automatische Event-Deduplizierung. | Master §31.1 | PRESERVED | WDB-EVT-010 | – | SM, DF |
| EVT-14 | Witness impliziert kein Knowledge. | Master §31.1 | PRESERVED | WDB-EVT-011 | – | Truth table |
| EVT-15 | Erinnerung/Behauptung ist kein perspektivisches Event. | Master §31.1 | PRESERVED | WDB-EVT-011 | – | Truth table, DF |
| EVT-16 | Eventkorrektur mutiert keinen Eventrecord. | Master §31.1 | PRESERVED | WDB-EVT-012 | – | SM, DF |
| EVT-17 | EventMask ist keine Retraction. | Master §31.1 | PRESERVED | WDB-EVT-013 | – | Truth table |
| EVT-18 | EventMask cascadiert nicht zu Assertions. | Master §31.1 | PRESERVED | WDB-EVT-013 | – | Truth table |
| EVT-19 | EventMask nimmt Eventwirkungen nicht automatisch zurück. | Master §31.1 | PRESERVED | WDB-EVT-013 | – | DF |
| EVT-20 | After ist der inverse Eingabealias von Before und wird nicht separat persistiert. | Master §31.1 | CHANGED | WDB-EVT-019 | ADR-029 | PT, FZ, wire golden |
| EVT-21 | SameTime ist kanonisches ungeordnetes Paar und bildet eine Äquivalenzrelation. | Master §31.1 | CHANGED | WDB-EVT-020 | ADR-029 | PT, SM, DF |
| EVT-22 | Before wird nach SameTime-Komponentenkollaps auf Widerspruch und Zyklen geprüft. | Master §31.1 | CHANGED | WDB-EVT-022 | ADR-029 | SM, PT, truth table |
| EVT-23 | Causes bleibt von zeitlicher Ordnung orthogonal und impliziert kein Before. | Master §31.1 | PRESERVED | WDB-EVT-015/023 | ADR-029 | Truth table, DF |

## 3. Evidence und Provenance

| Previous Rule ID | Previous normative statement | Final location | Status | New Rule ID | ADR if changed | Test evidence |
|---|---|---|---|---|---|---|
| PROV-01 | Corrects bezeichnet Datenkorrektur. | Master §31.2 | PRESERVED | WDB-PRV-002 | – | SM, DF |
| PROV-02 | Corrects ist keine World-Time-Nachfolge. | Master §31.2 | PRESERVED | WDB-PRV-002 | – | DF |
| PROV-03 | Corrects erzeugt keine Retraction. | Master §31.2 | PRESERVED | WDB-PRV-002 | – | SM |
| PROV-04 | Corrects-Endpunkte müssen kompatibel sein. | Master §31.2.1 | PRESERVED | WDB-REF-004 | ADR-026 | PT, FZ |
| PROV-05 | DerivedFrom ist informationelle/logische Ableitung. | Master §31.2 | PRESERVED | WDB-PRV-003 | – | Truth table |
| PROV-06 | DerivedFrom ist keine Story-Kausalität. | Master §31.2 | PRESERVED | WDB-PRV-003 | – | DF |
| PROV-07 | ResultedFrom ist fachliche/kausale Wirkung. | Master §31.2 | PRESERVED | WDB-PRV-004 | – | Truth table |
| PROV-08 | ResultedFrom ist keine logische Ableitung. | Master §31.2 | PRESERVED | WDB-PRV-004 | – | DF |
| PROV-09 | ResultedFrom cascadiert nicht. | Master §31.2 | PRESERVED | WDB-PRV-004 | – | DF |
| PROV-10 | Self-Loops sind unzulässig. | Master §31.2 | PRESERVED | WDB-PRV-006 | – | PT, FZ |
| PROV-11 | Direkte und tiefere Zyklen werden verhindert. | Master §31.2 | SUPERSEDED | WDB-PRV-007/009/011–013 | ADR-026 | SM, PT |
| PROV-12 | Aktive Duplikatkanten sind unzulässig. | Master §31.2 | PRESERVED | WDB-PRV-008 | – | PT, SM |
| PROV-13 | Endpoint-Kompatibilität ist relationstypisch geschlossen. | Master §31.2.1 | CHANGED | WDB-REF-003–005 | ADR-026 | CF, PT, FZ |
| EVI-01 | Evidence-Duplikate sind unzulässig. | Master §31.2 | PRESERVED | WDB-EVI-007 | – | PT, SM |
| EVI-02 | Evidence darf historische/retracted Targets dokumentieren. | Master §31.2 | PRESERVED | WDB-EVI-006 | – | SM, DF |
| EVI-03 | Evidence besitzt keine World-Time-Validity. | Master §31.2 | PRESERVED | WDB-EVI-002 | – | DF, CT |
| PROV-14 | Provenance besitzt keine World-Time-Validity. | Master §31.2 | PRESERVED | WDB-PRV-005 | – | DF, CT |
| PROV-15 | Cross-Relation-Zyklen werden über eine gemeinsame Dependency-Projektion geprüft. | Master §31.2 | CHANGED | WDB-PRV-011/012 | ADR-026 | SM, PT, truth table |
| PROV-16 | Batchkanten werden gegen den atomaren Post-Transaction-Zustand validiert. | Master §31.2 | CHANGED | WDB-PRV-013 | ADR-026 | SM, OCC integration |
| META-01 | Source/Evidence/Provenance sind projektweite Meta-History. | Master §31.2 | PRESERVED | WDB-EVI-002, WDB-PRV-005 | – | DF, CT |
| META-02 | RecordedAsOf filtert Meta-History. | Master §31.2 | PRESERVED | WDB-EVI-003 | – | DF, SM |
| META-03 | base_revision beschneidet Targetsichtbarkeit, nicht globale Meta-History. | Master §31.2 | CHANGED | WDB-EVI-004 | ADR-026 | DF, NI |
| META-04 | Meta-History ist security-gefiltert. | Master §31.2 | PRESERVED | WDB-EVI-005, WDB-PRV-010 | – | NI |
| META-05 | Graph Traversal ist bounded. | Master §§16, 31.2 | PRESERVED | WDB-PRV-009 | – | PF, SM |

## 4. Schema und Migration

| Previous Rule ID | Previous normative statement | Final location | Status | New Rule ID | ADR if changed | Test evidence |
|---|---|---|---|---|---|---|
| SCH-01 | PredicateDefinition ist vollständig typisiert. | Master §31.3 | PRESERVED | WDB-SCH-005 | – | CT, API test |
| SCH-02 | Subject Constraint ist Teil des Predicates. | Master §31.3 | PRESERVED | WDB-SCH-005 | – | CT |
| SCH-03 | ValueKind/ObjectConstraint sind getrennt. | Master §31.3 | PRESERVED | WDB-SCH-005/006 | – | PT, FZ |
| SCH-04 | Cardinality und ResolutionPolicy sind kompatibel. | Master §31.3 | PRESERVED | WDB-SCH-007 | – | Truth table, FZ |
| SCH-05 | Single verhindert keine widersprüchlichen Assertions. | Master §31.3 | PRESERVED | WDB-SCH-008 | – | Truth table, DF |
| SCH-06 | Structural Validation ist kein ResolutionConflict. | Master §31.3 | PRESERVED | WDB-SCH-008 | – | CF, UT |
| SCH-07 | EventKind-Schema enthält Rollen/Attribute/Zeitconstraint. | Master §31.3 | PRESERVED | WDB-SCH-009 | – | CT, golden |
| SCH-08 | Deprecated-Writes benötigen Opt-in/Capability/Warning. | Master §31.3 | CHANGED | WDB-SCH-010 | – | E2E, NI |
| SCH-09 | Retired verbietet neue Writes, historische Reads bleiben. | Master §31.3 | PRESERVED | WDB-SCH-010 | – | E2E, DF |
| SCH-10 | Schema und Daten sind atomar. | Master §§8, 31.3 | PRESERVED | WDB-SCH-011 | – | SM, CR |
| SCH-11 | Referenzen gelten gegen Post-Transaction-Schema. | Master §31.3 | PRESERVED | WDB-SCH-011 | – | SM |
| SCH-12 | SchemaDependencies werden am Commit revalidiert. | Master §§9, 31.3 | PRESERVED | WDB-SCH-012 | – | SM, DF |
| SCH-13 | Breaking und Compatible werden nach Semantikwirkung klassifiziert. | Master §31.3 | PRESERVED | WDB-SCH-014 | – | Migration golden |
| SCH-14 | Bedeutungsbruch benötigt neue stabile Schema-ID. | Master §31.3 | PRESERVED | WDB-SCH-015 | – | golden, DR |
| SCH-15 | SchemaSnapshot ist derived/cacheable. | Master §31.3 | PRESERVED | WDB-SCH-013 | – | DF, CT |
| SCH-16 | Schemahistorie ist normative Wahrheit. | Master §§2.4, 31.3 | PRESERVED | WDB-SCH-001/013 | – | DF, CT |
| SCH-17 | SchemaFingerprint ist kanonisch. | Master §31.3 | PRESERVED | WDB-SCH-013 | – | PT, CT |
| SCH-18 | Beschädigtes Historical Schema fällt nicht auf Current zurück. | Master §31.3 | PRESERVED | WDB-SCH-017 | – | Corruption E2E |
| SCH-19 | Historical/Current/Explicit sind bewusste Modi. | Master §31.3 | PRESERVED | WDB-SCH-016 | – | API, DF |
| SCH-20 | Historische Daten werden nicht still coerced. | Master §31.3 | PRESERVED | WDB-SCH-016 | – | DF, FZ |
| MIG-01 | Kategorien sind MetadataOnly/Additive/CompatibleConstraintChange/Restrictive/Breaking. | Master §31.4 | PRESERVED | WDB-MIG-006 | – | CF, CT |
| MIG-02 | MigrationPlan und MigrationRun sind verschieden. | Master §31.4 | PRESERVED | WDB-MIG-007 | – | CF, API test |
| MIG-03 | MigrationId bleibt fachliche Identität. | Master §31.4 | PRESERVED | WDB-MIG-017 | – | CF, CT |
| MIG-04 | Source-Schema-Precondition ist verbindlich. | Master §31.4 | PRESERVED | WDB-MIG-008/009 | – | SM, E2E |
| MIG-05 | Plan Fingerprint ist kanonisch. | Master §31.4 | PRESERVED | WDB-MIG-008 | – | PT, CT |
| MIG-06 | Restrictive/Breaking besitzen Dry Run. | Master §§2.4, 31.4 | PRESERVED | WDB-MIG-002 | – | E2E |
| MIG-07 | Dry Run verwendet dieselbe Transformationslogik. | Master §31.4 | PRESERVED | WDB-MIG-010 | – | DF |
| MIG-08 | Transformer ist deterministisch. | Master §31.4 | PRESERVED | WDB-MIG-011 | – | ST, DF |
| MIG-09 | Commitpfad nutzt keine Uhr/Zufall/Locale/AI. | Master §31.4 | PRESERVED | WDB-MIG-011 | – | ST, DF |
| MIG-10 | AI darf Vorschläge, aber keinen nondeterministischen Commit erzeugen. | Master §31.4 | PRESERVED | WDB-MIG-012 | – | E2E, CT |
| MIG-11 | Keine versteckten Coercions. | Master §31.4 | PRESERVED | WDB-MIG-012 | – | FZ, DF |
| MIG-12 | UnresolvedMigrationItem ist eigener Zustand. | Master §31.4 | PRESERVED | WDB-MIG-013 | – | CF, API test |
| MIG-13 | Warning, Unresolved und Error sind verschieden. | Master §31.4 | PRESERVED | WDB-MIG-013 | – | CF, UT |
| MIG-14 | Representation Migration verändert keine World-Time-Gültigkeit. | Master §31.4 | PRESERVED | WDB-MIG-015 | – | DF, golden |
| MIG-15 | Historische/retracted Records werden nur bei explizitem Plan umgeschrieben. | Master §31.4 | PRESERVED | WDB-MIG-015 | – | DF, golden |
| MIG-16 | Migration Steps verwenden OCC. | Master §31.4 | PRESERVED | WDB-MIG-016 | – | SM, CR |
| MIG-17 | MigrationId/StepId/RunId/OperationId sind getrennt. | Master §31.4 | PRESERVED | WDB-MIG-017 | – | CF, CT |
| MIG-18 | Steps sind idempotent und Unknown Outcome wird geklärt. | Master §31.4 | PRESERVED | WDB-MIG-018 | – | CR, E2E |
| MIG-19 | Expand → Migrate → Contract ist Default. | Master §31.4 | PRESERVED | WDB-MIG-019 | – | SM, golden |
| MIG-20 | Jeder Zwischenstand ist gültig und historisch lesbar. | Master §§2.4, 31.4 | PRESERVED | WDB-MIG-004/019 | – | SM, CR |
| MIG-21 | Kein normales Rollback committed History. | Master §31.4 | PRESERVED | WDB-MIG-020 | – | SM, CR |
| MIG-22 | Rücknahme ist Compensating Migration. | Master §31.4 | PRESERVED | WDB-MIG-021 | – | E2E, DF |
| MIG-23 | Resume nach Crash nutzt Run Journal und OperationId. | Master §31.4 | PRESERVED | WDB-MIG-018/022 | – | CR, E2E |
| MIG-24 | Run Journal ist Koordinationsmetadatum, nicht normative History. | Master §31.4 | PRESERVED | WDB-MIG-022 | – | CR, CT |
| MIG-25 | Storage-Format-Migration bleibt getrennt. | Master §§2.4, 31.4 | PRESERVED | WDB-MIG-023 | – | E2E, CT |

## 5. Rust-, Error- und Observability-Regeln aus `pasted.txt`

| Previous Rule ID | Previous normative statement | Final location | Status | New Rule ID | ADR if changed | Test evidence |
|---|---|---|---|---|---|---|
| MIA-PHIL-01 | Idiomatic Rust ist kein eigenständiges Optimierungsziel. | Master §§0, 23–25 | PRESERVED | WDB-PHIL-001 | ADR-019 | ST, DR |
| MIA-PHIL-02 | Convenience darf relevante Semantik nicht verbergen. | Master Zentraler Grundsatz, §30 | PRESERVED | WDB-PHIL-001 | ADR-019 | ST, negative API tests |
| MIA-PHIL-03 | Borrow-Probleme sind zuerst Architekturfeedback. | Master §6 | PRESERVED | WDB-OWN-002/003 | ADR-019 | ST, CF |
| OWN-01 | Lesende Eingaben werden geliehen, sofern kein Besitztransfer nötig ist. | Master §6 | PRESERVED | WDB-OWN-003 | – | CF, API review |
| OWN-02 | Mutation ist exklusiv und sichtbar. | Master §6 | PRESERVED | WDB-CON-002 | – | CF, SM |
| OWN-03 | Move macht Besitztransfer sichtbar. | Master §§6–8 | PRESERVED | WDB-TX-001 | – | CF |
| OWN-04 | Borrowed-to-Owned ist an der Boundary sichtbar. | Master §6 | PRESERVED | WDB-OWN-003 | ADR-019 | ST, API test |
| OWN-05 | Plain clone ist kein Borrow-Checker-Reparaturwerkzeug. | Master §6 | PRESERVED | WDB-OWN-002 | ADR-019 | ST, CF |
| OWN-06 | Shared Ownership wird mit Arc::clone/Rc::clone sichtbar. | Master §6 | PRESERVED | WDB-OWN-001 | ADR-019 | ST |
| OWN-07 | Kein künstliches static. | Master §6 | PRESERVED | WDB-OWN-002 | – | CF, ST |
| OWN-08 | Kein universelles Arc&lt;Mutex&lt;_&gt;&gt;. | Master §6 | PRESERVED | WDB-CON-002, WDB-OWN-001 | – | CF, ST |
| OWN-09 | Reader/Writer besitzen explizite Lebenszyklen. | Master §§6, 10, 19 | PRESERVED | WDB-CON-001, WDB-SNP-001 | ADR-008 | SM, CR |
| ERR-01 | Konkrete Error-Domänen. | Master §5.1 | PRESERVED | WDB-ENG-002 | ADR-014 | API, CT |
| ERR-02 | Keine universelle Error-Erasure im Enginevertrag. | Master §§5, 24 | PRESERVED | WDB-ENG-002 | ADR-014 | dependency/API lint |
| ERR-03 | Fremdfehler nicht früh stringifizieren. | Master §§5.2–5.4 | PRESERVED | WDB-ERR-003 | – | UT |
| ERR-04 | Kausalketten über source() erhalten. | Master §5.2 | PRESERVED | WDB-ERR-003 | – | UT |
| ERR-05 | From nur bei eindeutiger Semantik. | Master §5.2 | PRESERVED | WDB-ERR-003 | – | CF, UT |
| ERR-06 | Operationskontext an der wissenden Call-Site. | Master §5.2 | PRESERVED | WDB-ERR-003 | – | UT |
| ERR-07 | Result-Aliasse bleiben lokal. | Master §5.4 | PRESERVED | WDB-ENG-002 | – | API test |
| ERR-08 | Display ist leak-sicher. | Master §5.2 | PRESERVED | WDB-ERR-002 | – | snapshot, NI |
| ERR-09 | Debug ist kein zweiter Leakkanal. | Master §5.2 | PRESERVED | WDB-ERR-004 | – | snapshot, NI |
| ERR-10 | Formatting und Error-Mechanik sind side-effect-free. | Master §§5.2, 30.1 | PRESERVED | WDB-ERR-001 | – | UT, ST |
| ERR-11 | Logging liegt an verantwortlicher Boundary. | Master §§5.2, 18.1 | PRESERVED | WDB-OBS-001 | – | capture test |
| ERR-12 | Diagnose ist strukturiert. | Master §18.1 | PRESERVED | WDB-OBS-002 | – | capture, CT |
| ERR-13 | Diagnosewerte sind opt-in. | Master §18.1 | PRESERVED | WDB-OBS-004 | – | capture, NI |
| ERR-14 | Nur reine Observability ist fail-open. | Master §§18.1–18.2 | PRESERVED | WDB-OBS-001, WDB-AUD-001 | ADR-010 | fault, CR |
| ERR-15 | Error-Code-Stabilität ist phasenabhängig. | Master §5.1 | PRESERVED | WDB-ERR-002 | – | CT |
| ERR-16 | Klassifikationsachsen bleiben orthogonal. | Master §5.2 | PRESERVED | WDB-ERR-006 | ADR-020 | type/API tests |
| ERR-17 | Korrektheitsrelevante Mappings sind exhaustiv. | Master §5.4 | PRESERVED | WDB-ERR-005 | – | CF |
| ERR-18 | Fachliche Ambiguität ist nicht automatisch Error. | Master §§2.2, 5.3 | PRESERVED | WDB-RES-001, WDB-OCC-003 | ADR-001 | CF, UT |
| ERR-19 | Public Error Mapping respektiert Security. | Master §§5.4, 17 | PRESERVED | WDB-ERR-002, WDB-SEC-003 | – | NI |
| ERR-20 | Error-Macro erst nach stabiler Wiederholung. | Master §5.4 | PRESERVED | WDB-ENG-007 | ADR-014 | ST, DR |
| ERR-21 | Error-Codegen bleibt dependency-arm und azyklisch. | Master §§24–25 | PRESERVED | WDB-ENG-001/007 | – | crate graph lint |
| ERR-22 | Errorverträge testen Darstellung, Ursachen, Mappings und Leakage. | Master §§22, 32 | PRESERVED | WDB-ERR-002–006 | – | UT, CT, NI |
| SEC-DIAG-01 | Redaction ersetzt Security Filtering nicht. | Master §§17, 18.1 | PRESERVED | WDB-SEC-002 | – | NI |
| SEC-DIAG-02 | Späteres Omitted erlaubt keinen Boundary-Übertritt. | Master §§17, 18.1 | PRESERVED | WDB-SEC-002, WDB-OBS-004 | – | NI |
| OBS-01 | Reine Diagnose darf fail-open sein. | Master §18.1 | PRESERVED | WDB-OBS-001 | – | fault test |
| OBS-02 | Audit besitzt eigenen Vertrag. | Master §§18.2, 31.6 | SUPERSEDED | WDB-AUD-001–011 | ADR-010/027 | CR, E2E |
| OBS-03 | Fail-open bleibt bounded. | Master §18.1 | PRESERVED | WDB-CON-003 | – | PF |
| OBS-04 | Core hängt nicht vom Observability-Adapter ab. | Master §§18.1, 25 | PRESERVED | WDB-ENG-001 | – | crate graph lint |
| OBS-05 | Kein Span-Guard über await. | Master §18.1 | PRESERVED | WDB-OBS-003 | – | ST |
| OBS-06 | Formatting/Conversion loggt nicht automatisch. | Master §§5.2, 18.1 | PRESERVED | WDB-ERR-001, WDB-OBS-001 | – | UT, capture |

## 6. Nacharbeitsregeln zu Cursor, Audit, Storage und Nachweisstruktur

| Previous Rule ID | Previous normative statement | Final location | Status | New Rule ID | ADR if changed | Test evidence |
|---|---|---|---|---|---|---|
| CUR-01 | MAC schützt Integrität, nicht Vertraulichkeit. | Master §§16, 31.5 | CHANGED | WDB-API-006 | ADR-024 | NI, FZ |
| CUR-02 | Cursor darf keine sensitiven IDs/SortKeys offenlegen. | Master §§16, 31.5 | PRESERVED | WDB-API-006 | ADR-024 | NI |
| CUR-03 | Cursor bindet effektiven Authorization Context. | Master §31.5 | PRESERVED | WDB-API-007 | ADR-024 | NI, E2E |
| CUR-04 | Securitywechsel invalidiert Cursor. | Master §31.5 | PRESERVED | WDB-API-008 | ADR-024 | NI, SM |
| CUR-05 | AuthorizationNow wird vor jeder Page geprüft. | Master §31.5 | PRESERVED | WDB-API-008 | ADR-024 | NI, SM |
| AUD-READ-01 | Reines Raw-Read-Audit wird im getrennten Audit-Subsystem gespeichert. | Master §31.6 | CHANGED | WDB-AUD-004 | ADR-027 | CT, CR |
| AUD-READ-02 | Reines Raw-Read-Audit erzeugt keine WorldDB-Datenrevision. | Master §31.6 | PRESERVED | WDB-AUD-004 | ADR-027 | CT, CR |
| AUD-READ-03 | Audit besitzt eine eigene monotone AuditSequence. | Master §31.6 | CHANGED | WDB-AUD-004 | ADR-027 | SM, CT |
| AUD-READ-04 | Read-Audit besitzt einen eigenen WAL und Commitmarker. | Master §31.6 | CHANGED | WDB-AUD-004 | ADR-027 | CR |
| AUD-READ-05 | Audit-Recovery läuft vor Freigabe auditpflichtiger Reads. | Master §31.6 | PRESERVED | WDB-AUD-008/009 | ADR-027 | CR, E2E |
| AUD-READ-06 | Der Auditrecord ist vor jeder Page-Ausgabe durable. | Master §31.6 | PRESERVED | WDB-AUD-005 | ADR-027 | CR, E2E |
| AUD-READ-07 | Auditlocking verwendet einen eigenen serialisierten Writer ohne Datenwriter-I/O-Lockkopplung. | Master §31.6 | PRESERVED | WDB-AUD-008 | ADR-027 | SM, CR |
| AUD-READ-08 | Nur AuditCompleteBackup enthält und verifiziert Auditlog, Auditmanifest und audit_safe_sequence. | Master §§15.1, 31.6 | CHANGED | WDB-AUD-010/012 | ADR-027 | E2E, DF |
| AUD-READ-09 | Audit-Retention ist policy-versioniert, mindestbegrenzt und selbst auditiert. | Master §31.6 | PRESERVED | WDB-AUD-011 | ADR-027 | E2E |
| AUD-READ-10 | Auditappend/-sync-Fehler blockiert die Raw-Read-Ausgabe. | Master §31.6 | PRESERVED | WDB-AUD-005 | ADR-027 | fault, CR |
| AUD-READ-11 | Crash zwischen durablem Attempt und Ausgabe bleibt als Versuch ohne behauptete Ausgabe interpretierbar. | Master §31.6 | PRESERVED | WDB-AUD-007 | ADR-027 | CR |
| AUD-READ-12 | Mehrfachversuche behalten ClientRequestId, erhalten aber je Versuch neue AuditOperationId. | Master §31.6 | PRESERVED | WDB-AUD-007 | ADR-027 | SM, CR |
| BKP-AUD-01 | ExactDatabaseBackup behauptet Daten-/Schemaexaktheit, nicht Auditvollständigkeit. | Master §15.1 | CHANGED | WDB-BKP-005 | ADR-027 | CT, restore DF |
| BKP-AUD-02 | AuditCompleteBackup besitzt eine vom safe_revision unabhängige audit_safe_sequence. | Master §§15.1, 31.6 | CHANGED | WDB-AUD-012 | ADR-027 | SM, restore DF |
| BKP-AUD-03 | Restore eines AuditCompleteBackup verifiziert Daten- und Auditnamespace getrennt. | Master §15.1 | CHANGED | WDB-AUD-013 | ADR-027 | CR, E2E |
| SEG-01 | SegmentId ist zufällige stabile Identität. | Master §§13.1, 31.7 | CHANGED | WDB-STO-004 | ADR-028 | PT, CT |
| SEG-02 | ContentDigest ist separater kryptographischer Hash. | Master §§13.1, 31.7 | CHANGED | WDB-STO-005 | ADR-028 | PT, CT, FZ |
| TEST-01 | Evidence Class und konkrete Testimplementierung sind getrennt. | Master §32 | CHANGED | Testregister-Schema | – | ST |
| TEST-02 | invariants_vNext.toml ist maschinenlesbar. | Master §32 | PRESERVED | Testregister-Schema | – | parser validation |
| TYPE-01 | Entity ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Entity | – | ST, DR |
| TYPE-02 | Predicate ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: PredicateDefinition | – | ST, DR |
| TYPE-03 | Assertion ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Assertion | – | ST, DR |
| TYPE-04 | Mask ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Mask | – | ST, DR |
| TYPE-05 | ReplacementBoundary ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: ReplacementBoundary | – | ST, DR |
| TYPE-06 | Event ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Event | – | ST, DR |
| TYPE-07 | EventMask ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: EventMask | – | ST, DR |
| TYPE-08 | Source ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Source | – | ST, DR |
| TYPE-09 | Evidence ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Evidence | – | ST, DR |
| TYPE-10 | ProvenanceEdge ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: ProvenanceEdge | – | ST, DR |
| TYPE-11 | Layer ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: LayerDefinition | – | ST, DR |
| TYPE-12 | HistorySpace ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: HistorySpace | – | ST, DR |
| TYPE-13 | Perspective ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Perspective | – | ST, DR |
| TYPE-14 | Schema records sind vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: SchemaRecord | – | ST, DR |
| TYPE-15 | Migration records sind vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: MigrationRecord | – | ST, DR |
| TYPE-16 | Lifecycle records sind vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: LifecycleRecord | – | ST, DR |
| TYPE-17 | Transaction ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Transaction | – | ST, DR |
| TYPE-18 | Snapshot ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Snapshot | – | ST, DR |
| TYPE-19 | Job ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Job | – | ST, DR |
| TYPE-20 | Principal ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: Principal | – | ST, DR |
| TYPE-21 | AuditRecord ist vollständig registriert. | Master §33 | PRESERVED | First-Class-Register: AuditRecord | – | ST, DR |
| TYPE-22 | EventRelation als wiederhergestellter First-Class-Typ ist vollständig registriert. | Master §33 | CHANGED | First-Class-Register: EventRelation | ADR-029 | ST, DR |
| TERM-01 | BranchId ist nur historisch/negativ zulässig. | Master §34.3 | PRESERVED | WDB-ID-004 | ADR-021 | terminology lint |
| TERM-02 | SchemaVersionId ist nur historisch/negativ zulässig. | Master §34.3 | PRESERVED | WDB-ID-004 | – | terminology lint |
| TERM-03 | typgelöschtes LifecycleRecordId ist nur historisch/negativ zulässig. | Master §34.3 | PRESERVED | WDB-ID-004 | ADR-023 | terminology lint |

## 7. Ergebnis

- `PRESERVED`: Regel semantisch vollständig vorhanden.
- `SUPERSEDED`: ältere breitere Regel durch präzisere, mindestens gleich starke Regeln ersetzt.
- `CHANGED`: bewusste Präzisierung mit ausgewiesenem ADR, soweit architekturrelevant.
- `RETIRED`: keine stabile fachliche Regel wurde still retired; der einzige verworfene Domain-Typ `BranchId` ist als `CHANGED` mit ADR-021 nachvollzogen.
- `DUPLICATE`: semantische Wiederholungen wurden in den Tabellen der Primärregel zugeordnet; keine Regel wurde allein wegen Dopplung aus dem Nachweis entfernt.

Für alle in den verfügbaren Quellen ausdrücklich stabil gekennzeichneten oder als MUST/HARD formulierten Regeln existiert eine Einzelzeile. Ein später vorgelegtes eigenständiges v3.1-Dokument muss als zusätzliche Quelle regelweise ergänzt werden; dieser Trace behauptet nicht, eine nicht verfügbare Datei geprüft zu haben.
