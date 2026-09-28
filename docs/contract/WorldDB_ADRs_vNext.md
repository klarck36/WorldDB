# WorldDB – Architecture Decision Records vNext

**Stand:** 20. September 2026  
**Status:** akzeptierte Architekturentscheidungen. Jede Änderung benötigt einen neuen ADR; alte Einträge werden nicht umgeschrieben.

## ADR-001 – TransactionConflict ist ein Outcome

**Status:** Accepted  
**Kontext:** Optimistic Concurrency erzeugt erwartbare fachliche Konkurrenz, während Storage-/Integritätsfehler die Ausführung verhindern.  
**Entscheidung:** `commit()` liefert `CommitOutcome::Committed` oder `CommitOutcome::Conflict`. Validierungs-, Security- und Storageprobleme bleiben `CommitError`.  
**Folgen:** UI kann Konflikte normal darstellen; Metrics trennen Konfliktlast von Fehlern. ConflictReport wird security-gefiltert.  
**Verworfen:** Konflikt als generischer Error; automatisches Last Write Wins.

## ADR-002 – OperationId trennt Idempotency von TransactionId

**Status:** Accepted  
**Kontext:** Nach verlorenem Response kann ein Commit bereits durable sein. Eine neue Wiederholung darf keine zweite fachliche Wirkung erzeugen.  
**Entscheidung:** TransactionId bezeichnet eine konkrete Ausführung; OperationId eine logische Commitabsicht. Deduplizierung ist persistent und payloadgebunden.  
**Folgen:** `UnknownCommitOutcome` enthält OperationId; Statusabfrage ist Pflicht vor Retry.

## ADR-003 – WAL-Sync ist Commitpoint, Manifest darf nachlaufen

**Status:** Accepted  
**Kontext:** Manifestpublication auf drei Betriebssystemen ist komplex; ein einziger klarer durable Punkt vereinfacht Recovery.  
**Entscheidung:** Prepare syncen, Commitmarker schreiben, WAL erneut syncen. Der zweite Sync ist Commitpoint. Manifest/Segmente werden anschließend materialisiert und können aus WAL rekonstruiert werden.  
**Folgen:** Antwort darf vor vollständiger Manifestmaterialisierung, aber nie vor Commitpoint erfolgen. Recovery muss committed WAL replayen.  
**Verworfen:** Manifestrename allein als Commitpoint; dualer unklarer Commitpoint.

## ADR-004 – Kanonisches eigenes TLV statt Serde-Format

**Status:** Accepted  
**Kontext:** Persistente History braucht langfristige, sprachunabhängige und fuzzbare Bytes. Serde-Implementierungsformate sind kein stabiler WorldDB-Vertrag.  
**Entscheidung:** versioniertes framed TLV mit kanonischen Integers/Decimals, Checksums und Limits.  
**Folgen:** mehr initialer Code und Golden Tests; klare Kompatibilität und TypeScript-Transport.  
**Verworfen:** bincode/postcard als unversionierter kanonischer Vertrag; JSON als Fileformat.

## ADR-005 – SegmentedFileBackend ist kanonisch, SQLite Referenz

**Status:** Accepted  
**Kontext:** WorldDB verlangt immutable Historie, explizite WAL-/Manifest-Recovery und reproduzierbare Segmente. SQLite ist wertvoll als robuste Referenz, bildet den gewünschten physischen Vertrag aber nicht automatisch ab.  
**Entscheidung:** Custom Segmented Backend als 1.0-Ziel; SQLite als Differential-/Prototype-Backend.  
**Folgen:** Custom Storage erreicht 1.0 nur nach Crashmatrix. Scheitert der Gate, wird 1.0 verschoben oder der Storage-ADR neu entschieden; Semantik wird nicht abgeschwächt.

## ADR-006 – Öffentliche Queryresultate sind owned

**Status:** Accepted  
**Kontext:** Borrowed Results koppeln Nutzer an Locks, Memory Maps und Segmentpins und erschweren async/IPC.  
**Entscheidung:** Borrowing bleibt synchron backendintern; öffentliche Engine-/API-Ergebnisse sind owned oder chunk-owned.  
**Folgen:** mögliche Kopierkosten werden gemessen; Lifecycle und Reclamation bleiben eindeutig.  
**Verworfen:** öffentliche `&Record`-Iteratoren über Backendmemory.

## ADR-007 – Typestate nur für kontrollierte konsumierende Übergänge

**Status:** Accepted  
**Kontext:** Typestate kann Commit-vor-Validate verhindern, aber Plattenzustand, Rechte und Recovery werden erst zur Laufzeit entdeckt.  
**Entscheidung:** Typestate für WriteTransaction und wenige Builder; Runtime-Enums für Storage, Recovery, Jobs und Security.  
**Folgen:** Compile-Time-Beweise dort, wo ehrlich; keine PhantomData-Architektur ohne Nutzen.

## ADR-008 – Ein Writer, N Snapshot-Reader

**Status:** Accepted  
**Kontext:** Revisionpublication, OCC und WAL-Reihenfolge benötigen einen eindeutigen Owner.  
**Entscheidung:** ein WriterCoordinator pro geöffneter Datenbank; beliebig viele budgetierte Snapshotreader.  
**Folgen:** einfachere Commitordnung und Recovery; Write Throughput wird gebatcht/gemessen. Multi-Process-Write ist 1.0 ausgeschlossen.

## ADR-009 – Security vor Candidate Resolution

**Status:** Accepted  
**Kontext:** Nachträgliches Filtern verrät Daten durch Konflikte, Masking, Counts, Explain und Timing.  
**Entscheidung:** Security begrenzt CandidateStream vor Masking/Resolution; Derived Indices erben Schutzklassen.  
**Folgen:** Queries und Indizes müssen Policy-aware sein; Non-interference-Testkorpus ist Releasegate.

## ADR-010 – Audit und Telemetrie sind getrennte Systeme

**Status:** Accepted  
**Kontext:** Telemetrie muss bei Exportfehlern weiterarbeiten, verpflichtendes Security-Audit darf eine Policyaktion nicht unprotokolliert zulassen.  
**Entscheidung:** Telemetrie bounded/fail-open; durable Audit für definierte Aktionen transaktional/fail-closed.  
**Folgen:** getrennte Queues, Retention, Rechte und Fehler. Kein „wir loggen einfach alles“.

## ADR-011 – Purge als Offline-Rewrite

**Status:** Accepted  
**Kontext:** In-place-Purge widerspricht immutable Segmenten, Backups und nachvollziehbarer History und ist crashriskant.  
**Entscheidung:** 1.0-Purge erzeugt eine neue verifizierte Datenbank mit neuer DatabaseId und PurgeReport.  
**Folgen:** zusätzlicher Speicher und Downtime; klare Referenz-/Backupgrenzen. Secure Erase wird nicht garantiert.

## ADR-012 – Current-Schema ist eine bewusste Queryoption

**Status:** Accepted  
**Kontext:** Historical Data mit heutigem Schema kann andere Validität/Interpretation ergeben.  
**Entscheidung:** Historical ist Default. Current/Explicit pinnt eine separate konkrete SchemaRevision und wird im QueryHash festgehalten.  
**Folgen:** keine schleichende Bedeutungsänderung historischer Queries.

## ADR-013 – Keine in-process Extensions in 1.0

**Status:** Accepted  
**Kontext:** Custom Querycode und Plugins vergrößern Determinismus-, Security-, ABI- und Crashfläche stark.  
**Entscheidung:** nur deklaratives Schema und isolierte Import/Export-Adapter. Storage-Port bleibt sealed.  
**Folgen:** kleiner 1.0-Umfang; spätere Extension Runtime braucht eigenen Threat Model/ADR.

## ADR-014 – Kein Error-Derive-Framework im Domain-Core

**Status:** Accepted  
**Kontext:** WorldDB verlangt konkrete Taxonomie, bewusste Causes/Mapping und keine Library als fachlichen Vertrag.  
**Entscheidung:** Error-Enums und `std::error::Error` zunächst manuell; kein anyhow/thiserror/failure im Core.  
**Folgen:** mehr Boilerplate, aber sichtbarer Vertrag. Eigenes Macro erst nach belegter stabiler Wiederholung.

## ADR-015 – Workspaceweite Enforcement-Ausnahmen sind registriert

**Status:** Accepted  
**Kontext:** Lokale `allow`-Attribute und Agenten können zentrale Regeln still umgehen.  
**Entscheidung:** jede Ausnahme referenziert `WDB-EXC-*`, zentral mit Owner/Ablauf/Test. Kanonisches `cargo xtask verify` prüft Code und Register.  
**Folgen:** weniger flexible Ad-hoc-Ausnahmen; Regeln bleiben auditierbar.

## ADR-016 – Engine-Core synchron, I/O-Orchestrierung async

**Status:** Accepted  
**Kontext:** Resolution und Domainvalidierung sollen deterministisch/testbar bleiben; Desktop, I/O und Cancellation brauchen Async.  
**Entscheidung:** pure/synchrone Domain- und Resolutionlogik, async an Adapter-/Job-/IPC-Grenzen.  
**Folgen:** klare Taskownership und keine Lockguards über await; blockierendes I/O läuft in begrenzten Pools.

## ADR-017 – Historical Security standardmäßig mit heutigen Rechten

**Status:** Accepted  
**Kontext:** Eine historische Query soll nicht automatisch frühere, heute entzogene Rechte wieder aktivieren.  
**Entscheidung:** Default `AuthorizationNow`; `AuthorizationAtRevision` nur explizit administrativ und nur bei historisierter Policy.  
**Folgen:** sichere Defaultsemantik; Revisionsaudits benötigen Capability.

## ADR-018 – Desktopfrontend ist nicht vertrauenswürdig

**Status:** Accepted  
**Kontext:** Webview/TypeScript kann kompromittiert oder fehlerhaft sein.  
**Entscheidung:** Core validiert Pfade, Rechte, Budgets und DTOs; UI besitzt keine direkte Storage-/Filesystem-Autorität.  
**Folgen:** versionierte IPC, sichere Error-Mappings, lokale Engine ohne Netzwerklistener.

## ADR-019 – Relevante Unterscheidungen sind ein Architektur-Gate

**Status:** Accepted  
**Kontext:** WorldDBs Fachmodell trennt bewusst Unknown/False, Conflict/Error, Zeitachsen, Perspektive/Security und weitere Zustände. Convenience-Abstraktionen können dieselben Unterschiede im Rust-Code wieder verwischen.  
**Entscheidung:** `WDB-PHIL-001` wird übergreifende HARD-Invariante. Jede zusammenfassende Abstraktion benötigt einen Verlustfreiheitsnachweis für Semantik, History, Security, Recovery, Retry, Ownership und Diagnose.  
**Folgen:** Reviews prüfen Informationsverlust vor Boilerplate-Reduktion. Sentinels, pauschale Error-Erasure, implizite Coercions und Catch-all-Mappings scheitern am Architektur-Gate.

## ADR-020 – Error Facts und Response Policy bleiben getrennt

**Status:** Accepted  
**Kontext:** Niedrige Fehler können einen Zustand wie `HistoryGap` zuverlässig beschreiben, kennen aber nicht automatisch Nutzerrolle, Betriebsmodus oder geeignete UI-Reaktion.  
**Entscheidung:** Domain-/Storage-Errors tragen Fakten, Ursachen und niedrige Klassifikation. Security-aware Engine/Application-Mapping entscheidet Audience, RecoveryPolicy und SuggestedAction.  
**Folgen:** derselbe interne Fakt kann je nach autorisierter Boundary sicher unterschiedlich präsentiert werden, ohne UI-Policy in den Storage-Core einzubauen.

## ADR-021 – HistorySpace ist der einzige Branch-Domain-Typ

**Status:** Accepted  
**Kontext:** Frühere Texte verwendeten Branch und HistorySpace teils nebeneinander, ohne zwei vollständige Lifecycles zu definieren.  
**Entscheidung:** Variante A. `HistorySpaceId + parent_history_space_id: Option<HistorySpaceId> + base_revision` bildet die Verzweigung vollständig ab. `BranchId` existiert nicht; Branch ist UI-Terminologie.  
**Folgen:** keine Doppelidentität, alle Branch-Regeln gelten für HistorySpace, Merge bleibt explizite provenance-erhaltende Transaktion/Migration.

## ADR-022 – Layer bleibt orthogonales First-Class-Konzept

**Status:** Accepted  
**Kontext:** ContextPrecedence ohne Layerdefinition wäre unvollständig; Layer darf zugleich nicht mit Perspective, Security oder HistorySpace verschmelzen.  
**Entscheidung:** Projektweite historisierte LayerDefinitionen mit stabiler LayerId und precedence_rank. Jeder Schemastand bezeichnet genau einen aktiven Base-Layer als eindeutig niedrigsten Rang. Records tragen HistorySpaceId und LayerId; es gibt keine stille Base-Zuweisung. `BaseOnly` wird gegen den gepinnten Schemastand aufgelöst. Precedence ist HistorySpace-Spezifität vor LayerRank; Perspective/EpistemicMode partitionieren und ranken nicht.  
**Folgen:** Masking ist deterministisch, Layerrechte ändern keine fachliche Rangfolge, Storage/Wire/Query führen Layer explizit. Base-Wechsel sind atomare Schemaänderungen und bewegen vorhandene Records nicht.

## ADR-023 – Persistierter RecordRef ist eine geschlossene Enum

**Status:** Accepted  
**Kontext:** Ein abstraktes `RecordRef<K>` reicht für heterogene Evidence-, Provenance- und Lifecycle-Kanten nicht; reine Type Erasure würde exhaustive Matches verlieren.  
**Entscheidung:** Persistenz/Wire verwenden eine geschlossene `RecordRef`-Enum mit eigener Variante je First-Class-Record. Interne homogene APIs dürfen sealed `TypedRecordRef<K>` verwenden.  
**Folgen:** exhaustive Mapping und getypte IDs; neue Recordklassen benötigen bewusste Format-/Capabilityänderung.

## ADR-024 – Cursor sind 1.0 sessionlokal

**Status:** Accepted  
**Kontext:** Ein MAC schützt Integrität, aber nicht die Vertraulichkeit eines Payloads mit SortKeys oder unsichtbaren IDs. Persistente Cursor würden zusätzlich Schlüsselaufbewahrung, Rotation und Updatekompatibilität erfordern.  
**Entscheidung:** Der Wirecursor ist ein zufälliger opaker Handle mit MAC; sensitiver Payload liegt ausschließlich im bounded EngineSession-State. Er bindet Principal, effektive Capabilities, SecurityEpoch, Snapshot, QueryHash, Session und Expiry. AuthorizationNow wird vor jeder Seite neu geprüft.  
**Folgen:** keine sensitiven decodierbaren Cursorfelder, keine Fortsetzung nach Neustart oder relevantem Securitywechsel. Persistenz benötigt einen neuen ADR.

## ADR-025 – CalendarPeriod bleibt außerhalb von Value

**Status:** Accepted  
**Kontext:** Kalendarische Jahre/Monate sind keine fixe Duration; ein Core-Value würde den geschlossenen skalaren Valuekatalog erweitern.  
**Entscheidung:** Variante C. `CalendarPeriod` ist strukturierter Schema-/Query-Typ, kein speicherbarer Assertion-Value in 1.0.  
**Folgen:** keine versteckte Map/Struct-Escape-Hatch; Fakten verwenden explizite Predicate-Struktur.

## ADR-026 – Evidence- und Provenance-Endpunkte sind relationstypisch geschlossen

**Status:** Accepted  
**Kontext:** „Teilmenge von RecordRef“ ließ offen, welche Recordfamilien pro Relation zulässig sind und hätte uneinheitliche Adapter erlaubt.  
**Entscheidung:** §31.2.1 ist die normative Zulässigkeitsmatrix. Corrects verlangt Familienkompatibilität; DerivedFrom und ResultedFrom besitzen unterschiedliche geschlossene Endpoint-Enummen. Alle drei Relationen werden mit ausdrücklich definierter Kantenrichtung in einen gemeinsamen Abhängigkeitsgraphen projiziert; Self-Loops, aktive Duplikate sowie relationsinterne und gemischte Zyklen im atomaren Post-Transaction-Zustand werden abgewiesen.  
**Folgen:** kein Any-/String-/Other-Escape-Hatch; neue Recordklassen benötigen Format-, Invarianten- und Migrationserweiterung. Cross-Relation-Zyklen können nicht durch Relationstyp- oder Batchgrenzen verborgen werden.

## ADR-027 – Reines Raw-Read-Audit besitzt eine getrennte Sequenz und WAL

**Status:** Accepted  
**Kontext:** Ein vorgeschaltetes durables Audit für reine Reads darf keine Storyrevision erzeugen, muss aber vor Datenausgabe crashfest sein.  
**Entscheidung:** Eigenes append-only Audit-Subsystem mit AuditSequence, WAL, Commitmarker, Recovery und fail-closed Page-Attempt-Record. Mutierende Aktionen bleiben atomar im Domaincommit.  
**Folgen:** reine Reads verändern WorldDB-History nicht; Backup, Retention, Recovery und Korruptionsverhalten des Auditlogs sind eigenständig testbar. `ExactDatabaseBackup` bezeichnet nur Daten-/Schemaexaktheit; `AuditCompleteBackup` ergänzt Auditmanifest, Auditsegmente und eine unabhängige `audit_safe_sequence`.

## ADR-028 – SegmentId und ContentDigest sind getrennte Identitäten

**Status:** Accepted  
**Kontext:** „content-addressed innerhalb einer zufälligen SegmentId“ vermischte logische Segmentidentität und Inhaltsintegrität.  
**Entscheidung:** SegmentId ist zufällig und stabil; ContentDigest hasht kanonische Segmentbytes. Manifest, Recovery und Backup führen beide.  
**Folgen:** identische Bytes dürfen verschiedene SegmentIds besitzen; Digestgleichheit erzeugt keine fachliche Identität.

## ADR-029 – Explizite Eventrelationen bleiben First-Class

**Status:** Accepted  
**Kontext:** Before/After/SameTime/Causes waren in der verdichteten Masterfassung nicht mehr vollständig sichtbar. Zeitnähe darf insbesondere keine Kausalität erzeugen.  
**Entscheidung:** EventRelation ist ein immutable Record mit konkreter ID und Retraction. `After(A,B)` wird kanonisch als `Before(B,A)` persistiert, `SameTime` als ungeordnetes Paar und Äquivalenzkomponente ausgewertet. Der Before-Graph wird nach SameTime-Komponentenkollaps auf Widerspruch und Zyklen geprüft. Causes bleibt ein separater azyklischer Graph und impliziert keine zeitliche Ordnung.  
**Folgen:** Kausalität und Ordnung sind explizit, historisch nachvollziehbar und ohne automatische Story-Inference; inverse Aliasformen und SameTime-Widersprüche besitzen genau eine deterministische Prüfung.


---
