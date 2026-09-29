# M0-02a – beschlossener Normergänzungsanhang

**Status:** Accepted for the WorldDB 1.0 working contract
**Decision record:** [ADR-039](ADR-039-source-gap-resolution.md)
**Product decision:** Product Owner confirmed all 52 HARD source statements as normative intent and confirmed the stronger existing WDB-HIS-001 Master rule on 2026-09-29.

## 1. Authority, exact scope, and test ownership

Each HARD rule below is normative. Its wording is byte-for-byte the source_statement confirmed from source_gaps.tsv. The confirmed statement is the complete requirement for that ID. The sole scoped clarification is the expressly accepted WDB-RES-006 binding to the existing Master §16 typed-key ordering rule; all other linked immutable source lines are context only and do not add semantics. Any future semantic expansion or change requires a separate versioned product decision.

For each rule, the owner of the listed primary task owns implementation evidence; the Product Owner owns any change in semantic scope. The named task must provide automated evidence in the listed evidence class that asserts the rule and rejects a representative violating outcome where one is representable. The test and artifact must be named in WorldDB_1.0_Invariantenabdeckung.tsv. This records future verification duties and does not claim that implementation tests have already run.

## 2. Confirmed HARD source statements

### WDB-API-001

**Normintention (wortgleich):** Streamende ist `None`; Konstruktion-, Item-, Cancel- und Budgetfehler sind getrennt.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:548 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-08; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-08, automated UT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-08; Evidenzklasse UT; vorhandene Test-/Implementierungsangabe API UT; Folgebelege keine eingetragen.

### WDB-API-002

**Normintention (wortgleich):** Paginationcursor bindet Snapshot und Queryhash.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:563 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-10a; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-10a, automated PT,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-10a; Evidenzklasse PT,E2E; vorhandene Test-/Implementierungsangabe PT, E2E; Folgebelege M8-26d.

### WDB-API-003

**Normintention (wortgleich):** Ungeordnete Resultate erhalten kanonische Ordnung.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:561 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-09; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-09, automated PT,CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-09; Evidenzklasse PT,CT; vorhandene Test-/Implementierungsangabe PT, CT; Folgebelege keine eingetragen.

### WDB-API-004

**Normintention (wortgleich):** Unvollständige Aggregation wird nie als vollständig markiert.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:567 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-11; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-11, automated E2E,PF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-11; Evidenzklasse E2E,PF; vorhandene Test-/Implementierungsangabe Budget E2E; Folgebelege M6-11,M8-26d.

### WDB-API-005

**Normintention (wortgleich):** Cursor bindet Snapshot, QueryHash und EngineSession und verfällt beim Engine-Neustart.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:565 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-10a; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-10a, automated E2E,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-10a; Evidenzklasse E2E,ST; vorhandene Test-/Implementierungsangabe E2E, key rotation tests; Folgebelege M8-26d.

### WDB-CON-001

**Normintention (wortgleich):** Genau ein Writer publiziert; N Reader lesen gepinnte Snapshots.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:606 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M4-02; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M4-02, automated SM,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M4-02; Evidenzklasse SM,ST; vorhandene Test-/Implementierungsangabe Stress, SM; Folgebelege keine eingetragen.

### WDB-DES-001

**Normintention (wortgleich):** Desktopfrontend kann Storage/Dateisystem nicht außerhalb versionierter IPC umgehen.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:2010 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M8-09; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M8-09, automated NI,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M8-09; Evidenzklasse NI,E2E; vorhandene Test-/Implementierungsangabe E2E security; Folgebelege keine eingetragen.

### WDB-DES-002

**Normintention (wortgleich):** Core lauscht standardmäßig auf keinem Netzwerkport.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:2011 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M8-09; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M8-09, automated E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M8-09; Evidenzklasse E2E; vorhandene Test-/Implementierungsangabe E2E; Folgebelege keine eingetragen.

### WDB-ENG-004

**Normintention (wortgleich):** `unsafe` ist außerhalb des genehmigten Plattformadapters verboten.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:686 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M0-11; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M0-11, automated ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M0-11; Evidenzklasse ST; vorhandene Test-/Implementierungsangabe Workspace lint; Folgebelege M9-13b.

### WDB-ENG-005

**Normintention (wortgleich):** Der kanonische Verify-Pfad ist lokal und in CI identisch.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:700 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M0-14; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M0-14, automated NI,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M0-14; Evidenzklasse NI,ST; vorhandene Test-/Implementierungsangabe CI manifest; Folgebelege M9-13b.

### WDB-ENG-007

**Normintention (wortgleich):** Crates werden nur nach bestandenem Boundary-Gate extrahiert; der Repositorybaum ist Zielhypothese.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:714 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M0-09; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M0-09, automated CR,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M0-09; Evidenzklasse CR,ST; vorhandene Test-/Implementierungsangabe Crate graph/policy test; Folgebelege M9-13b.

### WDB-ERR-006

**Normintention (wortgleich):** Niedrige Errors enthalten Error Facts; SuggestedAction/RecoveryPolicy wird an höherer Boundary entschieden.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:320 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-02; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-02, automated CT,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-02; Evidenzklasse CT,ST; vorhandene Test-/Implementierungsangabe Type/API tests; Folgebelege keine eingetragen.

### WDB-EXP-001

**Normintention (wortgleich):** Logical Export manifestiert Scope und ausgelassene Datenklassen.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:538 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M7-11; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M7-11, automated CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M7-11; Evidenzklasse CT; vorhandene Test-/Implementierungsangabe CT; Folgebelege keine eingetragen.

### WDB-EXP-002

**Normintention (wortgleich):** Import remappt IDs nur über expliziten protokollierten Plan.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:538 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M7-13; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M7-13, automated PT,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M7-13; Evidenzklasse PT,E2E; vorhandene Test-/Implementierungsangabe PT, E2E; Folgebelege keine eingetragen.

### WDB-EXT-001

**Normintention (wortgleich):** 1.0 führt keinen untrusted Extensioncode in-process aus.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:656 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M7-13a; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M7-13a, automated NI,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M7-13a; Evidenzklasse NI,E2E; vorhandene Test-/Implementierungsangabe E2E security; Folgebelege keine eingetragen.

### WDB-ID-004

**Normintention (wortgleich):** BranchId, SchemaVersionId und typgelöschte LifecycleRecordId existieren nicht.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:228 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-01; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-01, automated CF,CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-01; Evidenzklasse CF,CT; vorhandene Test-/Implementierungsangabe CF, API snapshot; Folgebelege keine eingetragen.

### WDB-IDX-001

**Normintention (wortgleich):** Indexresultate sind differential gleich zum Full-Scan-Orakel.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:618 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M6-13; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M6-13, automated DF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M6-13; Evidenzklasse DF; vorhandene Test-/Implementierungsangabe DF; Folgebelege keine eingetragen.

### WDB-IDX-002

**Normintention (wortgleich):** Staler/fehlender Index liefert nie still unvollständige Daten.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:618 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M6-01; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M6-01, automated DF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M6-01; Evidenzklasse DF; vorhandene Test-/Implementierungsangabe Fault, DF; Folgebelege keine eingetragen.

### WDB-IDX-003

**Normintention (wortgleich):** Indexgeneration wird atomar vollständig publiziert.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:632 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M6-07; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M6-07, automated CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M6-07; Evidenzklasse CR; vorhandene Test-/Implementierungsangabe CR; Folgebelege keine eingetragen.

### WDB-OBS-001

**Normintention (wortgleich):** Telemetriefehler blockieren keine Domainoperation.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:592 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-12; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-12, automated ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-12; Evidenzklasse ST; vorhandene Test-/Implementierungsangabe Fault test; Folgebelege keine eingetragen.

### WDB-OBS-002

**Normintention (wortgleich):** Logs enthalten standardmäßig keine Values, Querytexte, Pfade oder Principalnamen.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:588 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-12; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-12, automated PT,NI evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-12; Evidenzklasse PT,NI; vorhandene Test-/Implementierungsangabe Capture/NI; Folgebelege keine eingetragen.

### WDB-OBS-004

**Normintention (wortgleich):** Nicht klassifizierte Diagnosewerte sind `Omitted`; `Shown` und `Hashed` sind opt-in.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:590 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M3-12; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M3-12, automated PT,NI evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M3-12; Evidenzklasse PT,NI; vorhandene Test-/Implementierungsangabe Capture/NI; Folgebelege keine eingetragen.

### WDB-OCC-001

**Normintention (wortgleich):** Write/Write- und relevante Write/Read-Konflikte werden erkannt.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:406 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M4-06; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M4-06, automated SM,DF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M4-06; Evidenzklasse SM,DF; vorhandene Test-/Implementierungsangabe SM, DF; Folgebelege keine eingetragen.

### WDB-OCC-002

**Normintention (wortgleich):** Range-/Predicate-Dependencies erkennen Phantoms.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:406 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M4-07; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M4-07, automated SM,DF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M4-07; Evidenzklasse SM,DF; vorhandene Test-/Implementierungsangabe SM, DF; Folgebelege keine eingetragen.

### WDB-OCC-003

**Normintention (wortgleich):** TransactionConflict ist erwartbares Outcome, kein Storagefehler.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:328 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M4-08; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M4-08, automated CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M4-08; Evidenzklasse CT; vorhandene Test-/Implementierungsangabe API snapshot; Folgebelege keine eingetragen.

### WDB-PRG-001

**Normintention (wortgleich):** Purge 1.0 ist Offline-Rewrite, kein In-place-History-Umschreiben.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:542 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M7-14; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M7-14, automated CT,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M7-14; Evidenzklasse CT,E2E; vorhandene Test-/Implementierungsangabe E2E, CT; Folgebelege keine eingetragen.

### WDB-PRG-002

**Normintention (wortgleich):** Secure Erase wird für SSD/COW/Backups nicht behauptet.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:544 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M7-15; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M7-15, automated ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M7-15; Evidenzklasse ST; vorhandene Test-/Implementierungsangabe Documentation test; Folgebelege keine eingetragen.

### WDB-REC-001

**Normintention (wortgleich):** Recovery macht nur vollständig verifizierten committed Präfix sichtbar.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:499 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-11; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-11, automated FZ,CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-11; Evidenzklasse FZ,CR; vorhandene Test-/Implementierungsangabe CR, FZ; Folgebelege keine eingetragen.

### WDB-REC-002

**Normintention (wortgleich):** Uncommitted Tail wird niemals als Commit interpretiert.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:517 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-11; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-11, automated CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-11; Evidenzklasse CR; vorhandene Test-/Implementierungsangabe CR; Folgebelege keine eingetragen.

### WDB-REC-003

**Normintention (wortgleich):** Korruption im sicheren Bereich wird nicht automatisch repariert.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:519 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-13; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-13, automated PT,CR,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-13; Evidenzklasse PT,CR,E2E; vorhandene Test-/Implementierungsangabe Corruption E2E; Folgebelege M8-26d.

### WDB-REC-004

**Normintention (wortgleich):** Recovery ist über wiederholte Abstürze idempotent.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:520 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-12; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-12, automated CR,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-12; Evidenzklasse CR,ST; vorhandene Test-/Implementierungsangabe Nested crash test; Folgebelege keine eingetragen.

### WDB-REC-005

**Normintention (wortgleich):** Salvage überschreibt nie das Original und berichtet Verluste.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:524 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-14; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-14, automated E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-14; Evidenzklasse E2E; vorhandene Test-/Implementierungsangabe E2E; Folgebelege M8-26d.

### WDB-REF-001

**Normintention (wortgleich):** Persistierter/Wire-RecordRef ist eine geschlossene exhaustive Enum.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:250;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:278 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-14; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-14, automated CF,CT,FZ evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-14; Evidenzklasse CF,CT,FZ; vorhandene Test-/Implementierungsangabe CF, CT, FZ; Folgebelege M1-17d,M3-09,M7-13.

### WDB-REF-002

**Normintention (wortgleich):** Evidence-, Provenance- und Lifecycle-Targets sind validierte RecordRef-Teilmengen.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:278 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-14; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-14, automated PT,FZ evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-14; Evidenzklasse PT,FZ; vorhandene Test-/Implementierungsangabe PT, FZ; Folgebelege keine eingetragen.

### WDB-RES-006

**Normintention (wortgleich):** Resolutionausgabe besitzt deterministische kanonische Ordnung.

**Eng begrenzte Konkretisierung:** Im vorhandenen Ergebnisvertrag ResolvedResult(Outcome, List<ContributorDto>) werden die Contributors nach dem in Master §16 geltenden kanonischen typisierten Schlüssel sortiert (Quellzeile 561). Die Outcome-Variante wird nicht gerangordnet; die Contributor-Menge wird dadurch nicht verändert.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:555; docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:561 (unveränderliche Quellzeilen; §16 typed-key order is the accepted clarification).

**Verantwortlich:** Luna (task owner of M2-16; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M2-16, automated PT,CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The confirmed source_statement; clarified only by the existing Master §16 typed-key ordering rule at source line 561. It orders Resolved View contributors and does not rank Outcome variants or change which contributors are selected.

**Prüfzuordnung:** Primäraufgabe M2-16; Evidenzklasse PT,CT; vorhandene Test-/Implementierungsangabe PT, CT; Folgebelege keine eingetragen.

### WDB-SEN-002

**Normintention (wortgleich):** Option, Result und Domain-Enum bleiben semantisch getrennt.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:243;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:244;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:245 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-15; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-15, automated CF,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-15; Evidenzklasse CF,ST; vorhandene Test-/Implementierungsangabe API review, CF; Folgebelege keine eingetragen.

### WDB-STO-001

**Normintention (wortgleich):** Backend publiziert höchstens eine neue Revision atomar.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:431 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-01; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-01, automated SM,CT,CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-01; Evidenzklasse SM,CT,CR; vorhandene Test-/Implementierungsangabe Contract SM, CR; Folgebelege keine eingetragen.

### WDB-STO-002

**Normintention (wortgleich):** Engine- und Backendgarantien sind getrennt und testbar.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:439 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-01; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-01, automated CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-01; Evidenzklasse CT; vorhandene Test-/Implementierungsangabe Contract suite; Folgebelege keine eingetragen.

### WDB-STO-003

**Normintention (wortgleich):** Produktionscommit verlangt Machine-Durability.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:432 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-01; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-01, automated CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-01; Evidenzklasse CR; vorhandene Test-/Implementierungsangabe CR, CP; Folgebelege keine eingetragen.

### WDB-TYP-002

**Normintention (wortgleich):** Handles/Transactions/Snapshots sind nur bei expliziter Semantik klonbar.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:239 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M4-10; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M4-10, automated CF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M4-10; Evidenzklasse CF; vorhandene Test-/Implementierungsangabe CF; Folgebelege keine eingetragen.

### WDB-VAL-004

**Normintention (wortgleich):** `Value` besitzt kein globales fachliches `Ord`.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:239 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-04; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-04, automated CF evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-04; Evidenzklasse CF; vorhandene Test-/Implementierungsangabe CF; Folgebelege keine eingetragen.

### WDB-VAL-005

**Normintention (wortgleich):** Decimal-Scale ist nicht Teil numerischer Identität; Darstellungs-/Messpräzision liegt in Schema/Metadaten.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:463 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-05; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-05, automated PT,CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-05; Evidenzklasse PT,CT; vorhandene Test-/Implementierungsangabe PT, CT; Folgebelege keine eingetragen.

### WDB-VAL-006

**Normintention (wortgleich):** CalendarPeriod ist 1.0 kein Assertion-Value.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:469 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-04; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-04, automated CF,FZ evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-04; Evidenzklasse CF,FZ; vorhandene Test-/Implementierungsangabe CF, Wire FZ; Folgebelege keine eingetragen.

### WDB-WAL-001

**Normintention (wortgleich):** Commitmarker wird erst nach synchronisiertem Prepare geschrieben.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:491;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:492 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-04; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-04, automated CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-04; Evidenzklasse CR; vorhandene Test-/Implementierungsangabe CR; Folgebelege M5-12,M5-22.

### WDB-WAL-002

**Normintention (wortgleich):** Zweiter WAL-Sync ist Commitpoint.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:493 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-04; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-04, automated CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-04; Evidenzklasse CR; vorhandene Test-/Implementierungsangabe CR; Folgebelege M5-12,M5-22.

### WDB-WAL-003

**Normintention (wortgleich):** Manifest referenziert nie Daten oberhalb des sicheren WAL-Präfix.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:499 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-07; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-07, automated SM,CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-07; Evidenzklasse SM,CR; vorhandene Test-/Implementierungsangabe CR, SM; Folgebelege keine eingetragen.

### WDB-WAL-004

**Normintention (wortgleich):** Historysegmente sind immutable.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:505 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-06; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-06, automated CT,ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-06; Evidenzklasse CT,ST; vorhandene Test-/Implementierungsangabe Contract test; Folgebelege M5-12.

### WDB-WAL-005

**Normintention (wortgleich):** Publication berücksichtigt Datei- und Verzeichnissync pro Plattform.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:495;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:496;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:503 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M5-07; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M5-07, automated CR evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M5-07; Evidenzklasse CR; vorhandene Test-/Implementierungsangabe CP, CR; Folgebelege keine eingetragen.

### WDB-WIR-001

**Normintention (wortgleich):** Persistentes Format ist explizit versioniert und serde-implementierungsunabhängig.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:447 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-16; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-16, automated CT evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-16; Evidenzklasse CT; vorhandene Test-/Implementierungsangabe CT; Folgebelege keine eingetragen.

### WDB-WIR-002

**Normintention (wortgleich):** Varints, Felder und Decimal sind kanonisch; alternative Encodings werden abgewiesen.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:447;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:462;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:463;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:471 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-16; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-16, automated PT,FZ evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-16; Evidenzklasse PT,FZ; vorhandene Test-/Implementierungsangabe PT, FZ; Folgebelege keine eingetragen.

### WDB-WIR-004

**Normintention (wortgleich):** TypeScript transportiert i128/u128/Decimal/Revision verlustfrei als Strings.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:477 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-19; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-19, automated PT,E2E evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-19; Evidenzklasse PT,E2E; vorhandene Test-/Implementierungsangabe E2E, PT; Folgebelege M8-26d.

### WDB-WIR-005

**Normintention (wortgleich):** Semantische Hard-Limits, Defaultprofile und provisorische Performancegates sind getrennt klassifiziert.

**Fundstellenkontext:** docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:636;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:637;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:638;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:639;docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md:640 (unveränderliche Quellzeilen; keine Erweiterung der Norm).

**Verantwortlich:** Luna (task owner of M1-18; implementation evidence); Product Owner (semantic scope).

**Testpflicht:** At M1-18, automated ST evidence must assert the exact statement above and reject a representative violating outcome where one is representable; record the named test and artifact in WorldDB_1.0_Invariantenabdeckung.tsv.

**Quellgrenze:** The exact source_statement is the complete confirmed requirement; source_context_refs are context only and do not enlarge its semantics.

**Prüfzuordnung:** Primäraufgabe M1-18; Evidenzklasse ST; vorhandene Test-/Implementierungsangabe Spec/policy test; Folgebelege M6-14c.

<a id="confirmed-wdb-his-001"></a>

## 3. WDB-HIS-001 – stärkere Masterregel bestätigt

Die Produktentscheidung bestätigt die bestehende stärkere Masterregel unverändert. Der kürzere Registerwortlaut bleibt für den bytegenauen Quellenvergleich erhalten; die wirksame Normbindung lautet:

- [HARD; WDB-HIS-001] Revisionsnummern steigen pro Datenbank monoton und lückenlos für veröffentlichte Commits. Reservierte, aber nicht veröffentlichte Nummern werden nicht sichtbar.

- [HARD; WDB-HIS-001, WDB-SEN-001] `Revision(u64)` beginnt bei 0 als leerer Genesis-Stand; der erste Commit veröffentlicht 1. `Revision::MAX` ist kein Sentinel und wird wegen Overflowreserve nicht vergeben.

Diese Bindung umfasst ausschließlich monotone, lückenlose veröffentlichte Revisionen, nicht sichtbare Reservierungen, Genesis Revision 0, den ersten Commit Revision 1 und die nicht vergebene Revision::MAX als Overflowreserve. Verantwortlich für die künftigen Nachweise ist Luna als Owner von M2-01 und M5-22; Evidenzklassen: SM, CR.

## 4. Registerkorrekturen und offene GUARDED-Quellenlücken

Die bestätigten Fundstellen sind: WDB-DES-001/002 → ADR-018; WDB-VAL-004 → Master §3.2; WDB-WIR-005 → Master §20.2; WDB-RES-006 → Master §16 plus dieser Anhang für die vollständige kanonische Resolutionausgabe. Die übrigen 47 Regeln sind einzeln in diesem Anhang an die im Beschlussregister angegebenen Quellkontexte gebunden.

WDB-ENG-006 und WDB-PER-001 bleiben als GUARDED-Quellenlücken offen. Diese Entscheidung hebt ihre Ausnahme- und Messpflichten nicht auf.

Das maschinenlesbare Register source_gap_bindings.tsv enthält pro Regel Normanker, Quellkontext, Verantwortlichen, Quellgrenze und Testpflicht. source_gaps.tsv erhält weiterhin die unveränderte Herkunftsklassifikation und weist den M0-02a-Abschluss separat aus.
