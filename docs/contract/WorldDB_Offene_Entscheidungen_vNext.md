# WorldDB – Offene Entscheidungen vNext

**Stand:** 20. September 2026  
**Regel:** Diese Datei enthält nur Entscheidungen, die ohne Implementierungs- oder Messnachweis nicht seriös geschlossen werden können. Der fachliche 1.0-Vertrag ist davon nicht abhängig. Jeder Punkt besitzt Gate, Owner und spätesten Entscheidungszeitpunkt.

Der Lossless Consolidation Audit hat keine neue fachliche ODE erzeugt. Endpoint-Matrizen, Cross-Relation-Zyklusregeln, Eventrelations-Kanonisierung, Base-Layer-Semantik, Cursor-Confidentiality/Authorization, Raw-Read-Auditpersistenz, Backup-/Auditprofile sowie SegmentId/ContentDigest sind normativ geschlossen und erscheinen daher nicht in diesem Register.

## ODE-001 – Konkrete MSRV

**Problem:** Der Arbeitsauftrag nennt keine vorhandene Toolchain oder Abhängigkeiten. Eine konkrete Rust-Versionsnummer ohne Repository-Spike wäre erfunden.  
**Optionen:**

1. älteste stabile Version mit Edition 2024 und allen nachweislich benötigten Standardfunktionen;
2. aktuelle Stable beim ersten Code-Commit;
3. Stable minus zwei Releasezyklen.

**Auswirkungen:** Ältere MSRV erweitert Nutzbarkeit, erhöht Dependency-/Backportdruck. Aktuelle Stable vereinfacht Start, erzeugt stärkere Updatepflicht.  
**Empfehlung:** Option 1; `rust-version` und `rust-toolchain.toml` auf denselben Wert setzen, Resolver 3 verwenden.  
**Benötigter Nachweis:** minimaler Workspace mit finalem Dependencyset baut und testet unter Kandidat; Dependencybaum enthält keine höhere MSRV.  
**Entscheidung bis:** Ende M0.  
**Owner:** Toolchain Maintainer.

## ODE-002 – Desktop Engine in-process oder Sidecar

**Problem:** Beide Varianten können dieselbe API erfüllen. Crashisolation, Packaging, Filelocks und IPC-Kosten sind plattformabhängig.  
**Optionen:**

1. Rust-Core in Tauri-Prozess;
2. separater lokaler Engine-Sidecar mit authentisiertem lokalen IPC.

**Auswirkungen:** In-process ist einfacher und schneller; ein Corepanic beendet die App. Sidecar isoliert Renderer/App besser, erhöht Lifecycle-, Packaging- und Authentisierungskomplexität.  
**Empfehlung:** In-process für den ersten Spike; Sidecar nur wählen, wenn Crash-/Mehrfenster-/Recoverytests einen konkreten Vorteil zeigen.  
**Benötigter Nachweis:** native Spike-Matrix auf Windows/macOS/Linux: Start, zwei Fenster, Writerlock, Enginepanic, Apprestart, 100-MiB-Stream, Cancellation, Update.  
**Entscheidung bis:** Architektur-Gate M8 vor Desktopausbau.  
**Owner:** Desktop Lead.

## ODE-003 – Bestätigte Performance- und Ressourcenbudgets

**Problem:** Zielwerte sind festgelegt, Hardware- und Datencorpusmessungen fehlen.  
**Optionen:** Ziele bestätigen, pro Workflow differenzieren oder nach begründetem Produktentscheid anpassen.  
**Auswirkungen:** Zu enge Ziele fördern riskante Optimierung; zu weite Ziele machen Regressionserkennung wertlos.  
**Empfehlung:** Spezifikationswerte als vorläufige Gates nutzen und nach Full-Scan- sowie erstem Indexprototyp einfrieren.  
**Benötigter Nachweis:** versionierter 1M-/10M-Korpus, drei OS, kalt/warm, mindestens 30 Messungen, p50/p95/p99 und Peak RSS.  
**Entscheidung bis:** Ende M6.  
**Owner:** Performance Lead/Product.

## ODE-004 – Plattformunterstützung jenseits lokaler Standarddateisysteme

**Problem:** Netzwerk-, synchronisierte und exotische Dateisysteme können Sync-, Lock- und Rename-Garantien abweichend implementieren.  
**Optionen:** 1.0 strikt auf lokales NTFS/APFS/ext4 begrenzen; weitere Dateisysteme per bestandener Capability-Matrix freischalten.  
**Auswirkungen:** Breite Freigabe ohne Beweis gefährdet Durability; enge Freigabe begrenzt portable Projekte.  
**Empfehlung:** lokale Standarddateisysteme als 1.0-Supportmatrix; andere Targets standardmäßig read-only/unsupported und einzeln qualifizieren.  
**Benötigter Nachweis:** Power-loss-/Crashmatrix, Locktests, Directory-Sync, Freespace/Quota, Antivirus/Sync-Client-Interaktion.  
**Entscheidung bis:** Release Candidate.  
**Owner:** Storage Lead.

## ODE-005 – UUIDv7-Implementierung

**Problem:** Der logische ID-Vertrag steht, aber Eigenimplementierung versus geprüfte Dependency ist offen.  
**Optionen:** gut gepflegte UUID-Crate mit v7/RNG-Features; kleiner interner Generator über OS-CSPRNG.  
**Auswirkungen:** Dependency reduziert Kryptographie-/Bitlayoutfehler, vergrößert Supply Chain; Eigenbau reduziert Dependency, erhöht Beweislast.  
**Empfehlung:** etablierte, auditierbare Dependency ohne Serde-Leak in Public API; Domainnewtype kapselt sie vollständig.  
**Benötigter Nachweis:** Lizenz/MSRV/unsafe/transitive Prüfung, RFC-konforme Vektoren, Collision-/Monotonicity-Properties, Fuzzing.  
**Entscheidung bis:** M0/M1.  
**Owner:** Domain Lead.

## ODE-006 – Stärkerer macOS-Durabilitypfad

**Problem:** `fsync` und `F_FULLFSYNC` besitzen unterschiedliche Kosten/Garantien; tatsächliches Verhalten hängt von OS/Medium ab.  
**Optionen:** immer stärkerer Full Sync; adaptiv per Capability; normales fsync mit dokumentierter schwächerer Stufe.  
**Auswirkungen:** Full Sync kann Commitlatenz stark erhöhen; schwächere Stufe darf nicht als Machine-Durability ausgegeben werden.  
**Empfehlung:** für `Durability::Machine` Full Sync, sofern unterstützt; andernfalls Schreiböffnung ablehnen oder bewusst niedrigere Durability nur in nicht-produktivem Modus.  
**Benötigter Nachweis:** APFS-Hardwaretests, Error Injection und dokumentierte API-Rückgaben.  
**Entscheidung bis:** Ende M4 vor Storage-Gate.  
**Owner:** macOS Storage Maintainer.

## Nicht offen

Folgende Kernfragen sind geschlossen und dürfen nicht in die Implementierung verschoben werden: TransactionConflict als Outcome, OperationId-Idempotency, zweiter WAL-Sync als Commitpoint, Historical Schema Default, Security vor Resolution, Offline-Purge, owned Public Results, ein Writer/N Reader, kein in-process Extensioncode, keine universelle Error-Erasure im Core, HistorySpace als einziger Branch-Domain-Typ, Layer als orthogonales First-Class-Konzept, geschlossener RecordRef, sessionlokale Cursor und CalendarPeriod außerhalb von Value.
