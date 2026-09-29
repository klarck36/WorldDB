# WorldDB 1.0 – Arbeitsstatus

**Stand:** 29. September 2026  
**Gesamtstatus:** IN PROGRESS – M0-01 bis M0-07 abgeschlossen; M0-02a wartet auf Produktentscheidungen; Produktimplementierung hat noch nicht begonnen (nur MSRV-Spike)

**Nächste Task:** `M0-08` (READY)

**Letzter abgeschlossener Milestone:** keiner  
**Offene Blocker:** M0-02a benötigt Produktentscheidungen zu 52 HARD-Quellenlücken und WDB-HIS-001  

**M0-01 Baseline-Commit:** `0bb3c85` (`chore: establish WorldDB source baseline`).

Die vollständigen Tasktexte stehen in `WorldDB_1.0_Luna_Arbeitsplan.md`; ausführbar ist die erste `READY`-Task des aktuellen Milestones im Taskregister. Nur nach belegter Abnahme darf die entsprechende Checkbox auf `[x]` wechseln. Jede `HARD`-Invariante braucht bis M10 mindestens einen wirksamen Negativnachweis in `WorldDB_1.0_Invariantenabdeckung.tsv`; spätere paarbezogene Nachweise stehen in `WorldDB_1.0_Folgebelege.tsv`.

`WorldDB_1.0_Taskregister.tsv` ist die verbindliche Liste von `depends_on` und Taskstatus. Nach jeder Task `PLANNED` mit erfüllten Abhängigkeiten auf `READY` setzen; `BLOCKED` erst nach Wegfall des konkreten Blockers erneut freigeben. Gültige Zustände: `PLANNED`, `READY`, `RUNNING`, `WAITING_EXTERNAL`, `BLOCKED`, `DONE`. Für `WAITING_EXTERNAL` Run-ID/URL, Besitzer, `last_check` und `next_check` als ISO-8601-Zeitpunkte mit Zeitzonenoffset im Taskregister und hier festhalten; zu Beginn jedes Durchgangs fällige Runs prüfen, echte Ergebnisse/Belege übernehmen und anschließend `DONE` oder `BLOCKED` setzen. Eine unterbrochene `RUNNING`-Task wird vor der nächsten `READY`-Task fortgesetzt; mehrere `RUNNING`-Tasks sind unzulässig. Gibt es keine ausführbare Task und keinen fälligen Run, den frühesten `next_check` oder konkreten Blocker protokollieren und den Durchgang ohne Erfolgsmeldung beenden. Unabhängige `READY`-Tasks desselben Milestones dürfen nach Start eines wartenden Langlaufs weiterlaufen. Ein Milestone-Gate bleibt bis zum Ende aller Langläufe offen. Ein grüner `--gate-precheck` ist nur eine Strukturprüfung; jedes Gate benötigt ein Review realer Test-/Run-Artefakte.

## Laufendes Protokoll

| Task | Status | Commit/Artefakt | Positiver Nachweis | Negativer/Fault-/Security-Nachweis | `cargo xtask verify` | Externer Lauf/letzte Prüfung/nächster Prüftermin | Nächster Schritt |
|---|---|---|---|---|---|---|---|
| M0-01 | DONE | `0bb3c85`; `docs/M0-01-verification.md` | ZIP-Test, 6 Bytevergleiche und Plancheck bestanden | Temporär veränderte Spiegeldatei mit Exitcode 1 abgewiesen | Nicht fällig (M0-01) | – | M0-02 READY |
| M0-02 | DONE | `cc8b190`; `docs/M0-02-verification.md`; `docs/contract/source-errata.json`; `docs/contract/source_gaps.tsv` | `build_contract_sources.py --verify-only`, 253-ID-Abgleich, 149 Haupttextbindungen, Sourcecheck und Plancheck bestanden | Manipulierte Kopie von `source_gaps.tsv` mit Exitcode 1 abgewiesen | Noch nicht fällig | geprüft 2026-09-29T02:00:47+02:00 | M0-03 READY |
| M0-02a | BLOCKED | `docs/contract/source_gaps.tsv`; `docs/contract/source-errata.json` | 54 Lücken präzise klassifiziert; Produktnormen nicht erfunden | Nicht fällig bis Entscheidung | Noch nicht fällig | 2026-09-29T02:00:47+02:00 | Produktentscheidung zu 52 HARD-Lücken und stärkerem WDB-HIS-001-Mastertext erforderlich |
| M0-03 | DONE | `c4cbe8f`; `docs/M0-03-verification.md`; `docs/contract/verify_contract_docs.py` | 22 Typen; Docs Verify sowie Quell- und Plancheck bestanden | Entferntes Pflichtfeld und unregistrierter Typ in temporären Fixtures mit Exitcode 1 abgewiesen | Noch nicht fällig | geprüft 2026-09-29T02:09:32+02:00 | M0-04 DONE |
| M0-04 | DONE | `fa95531`; `docs/M0-04-verification.md`; `docs/contract/ADR-030-entity-perspective.md`; `docs/contract/entity_perspective_contract.md` | Erzeugung, typed references, Typzuweisung, Metadatenhistorie, Retirement und Actions spezifiziert; Master-/Quellabgleich bestanden | Zwei temporäre Fixtures ohne EntityRetirement-Variante bzw. perspective.use mit Exitcode 1 abgewiesen | Nicht fällig (kein Code; M0-04) | geprüft 2026-09-29T02:29:05+02:00 | M0-04a READY |
| M0-04a | DONE | `605d5e3`; `docs/M0-04a-verification.md`; `docs/contract/ADR-031-archive-transfer.md`; `docs/contract/archive_transfer_contract.md` | Archive-/Unarchive- und Transfervertrag mit Revisionen, ID-Maps, Provenienz, Kollisions- und Konfliktregeln spezifiziert; Master-/Quellabgleich bestanden | Drei temporäre Fixtures ohne ArchiveTransition, Relation-ID-Map bzw. Zielkonfliktregel mit Exitcode 1 abgewiesen | Nicht fällig (kein Code; M0-04a) | geprüft 2026-09-29T02:47:33+02:00 | M0-04b DONE |
| M0-04b | DONE | `6a5eb51`; `docs/M0-04b-verification.md`; `docs/contract/ADR-032-constraint-time.md`; `docs/contract/constraint_time_contract.md` | Geschlossene Constraintregeln, Timeline-/TimeUnit-Registrierung, Kalenderarithmetik sowie Schema-/Query-/Migrationsgrenzen spezifiziert; Master-, Quell- und Planchecks bestanden | Drei temporäre Fixtures zu Constraint-Parsing, Zeitregister-Eindeutigkeit und Migrationsparität mit Exitcode 1 abgewiesen | Nicht fällig (kein Code; M0-04b) | geprüft 2026-09-29T03:06:51+02:00 | M0-04c DONE |
| M0-04c | DONE | `576804b`; `docs/M0-04c-verification.md`; `docs/contract/ADR-033-security-policy.md`; `docs/contract/security_policy_contract.md` | PolicyRecords, geschlossene Capabilities, exakte Scope-/Deny-Regeln, SecurityEpoch und auditgebundene Historisierung spezifiziert; SecurityPolicyRecord als 23. First-Class-Typ registriert; Master-, Quell- und Planchecks bestanden | Vier temporäre Fixtures zu Deny-Präzedenz, FieldRead vor Candidate-Erzeugung, Epoch-Overflow und fehlendem SecurityPolicyRecord-Registereintrag mit Exitcode 1 abgewiesen | Nicht fällig (kein Code; M0-04c) | geprüft 2026-09-29T03:32:20+02:00 | M0-04d READY |
| M0-04d | DONE | `3269d04`; `docs/M0-04d-verification.md`; `docs/contract/ADR-034-query-transport.md`; `docs/contract/query_transport_contract.md` | Gemeinsame Query-DTOs, Filter, Search, Comparatoren, Cursorfortsetzung und versionierte CLI-/IPC-Envelope gegen Master spezifiziert; Quell-, Dokumentations- und Planchecks bestanden | Vier temporäre Fixtures zu MatchAll, FieldRead-Reihenfolge, IPC-Versionsaushandlung und EventTime-Sortierung mit Exitcode 1 abgewiesen | Nicht fällig (kein Code; M0-04d) | geprüft 2026-09-29T03:53:12+02:00 | M0-04e READY |
| M0-04e | DONE | `97d3894`; `docs/M0-04e-verification.md`; `docs/contract/ADR-035-correction-actions.md`; `docs/contract/correction_contract.md` | Assertion-Correct mit explizitem Retractionrecord und Corrects-Kante sowie Event-Correct ohne implizite Retraction; Rechte-, Atomizitäts-, Commit- und UI-Regeln spezifiziert; Master-, Quell-, Dokumentations- und Planchecks bestanden | Fünf temporäre Fixtures zu Retraction-Ziel, Event-Lifecycle, AssertionRetract-Recht, Drei-Datensatz-Atomizität und UI-Hinweis mit Exitcode 1 abgewiesen | Nicht fällig (kein Code; M0-04e) | geprüft 2026-09-29T04:11:37+02:00 | M0-05 READY |
| M0-05 | DONE | `ab6355b`; `docs/M0-05-verification.md`; `docs/product/product_workflows.md` | Zwölf Kernabläufe jeweils mit Desktop-/CLI-API-Einstieg und Abnahmeschritt; initiale GM-/Player-Rechte und Commit-/Konflikt-/Backup-Grenzen spezifiziert; Contract-, Quell- und Planchecks bestanden | Erfolgs- und Ablehnungs-/Faultfälle pro Ablauf dokumentiert; nicht gegen Laufzeit ausgeführt (Produktvertrag, M0-05) | Nicht fällig (kein Code; M0-05) | geprüft 2026-09-29T04:21:24+02:00 | M0-06 READY |
| M0-06 | DONE | `bf3ab10`; `docs/M0-06-verification.md`; `docs/M0-06-environment-inventory.md` | Reproduzierbare Repo-Baseline, Lizenz-/CI-/Toolchainstand, drei Plattformprofile, unbesetzte Rollen mit Fälligkeiten und validierter nicht synchronisierter Laufzeitpfad inventarisiert; Quellen-, Vertrags- und Planchecks bestanden | Nur Windows/NTFS-Umgebung vorhanden; kein Rust-Workspace, keine macOS-/Linux-Hosts; keine DB-/Crash-/Performance-Runs; Lizenz und CI offen | Nicht fällig (kein Code; M0-06) | geprüft 2026-09-29T04:29:44+02:00 | M0-07 READY |
| M0-07 | DONE | `42f7ec5`; `docs/M0-07-verification.md`; `docs/contract/ADR-036-rust-msrv.md`; `experiments/msrv-spike/` | MSRV 1.85.0 mit Edition 2024 und Resolver 3 gewählt; gelockter Spike unter Rust 1.85 gebaut und getestet; Änderungspolitik bis 2027-03-29 dokumentiert | Keine deklarierte Dependency-MSRV über 1.85; drei Crates ohne Metadaten unter 1.85 gebaut; nur Windows x86_64 MSVC geprüft, kein Produktworkspace | Noch nicht fällig (CI-Test folgt M0-14) | geprüft 2026-09-29T04:54:50+02:00 | M0-08 READY |

## Entscheidungs- und Release-Gates

| Entscheidung/Gate | Fällig | Status | Nachweis |
|---|---|---|---|
| ODE-001 Rust-MSRV | M0 | DECIDED | `docs/contract/ADR-036-rust-msrv.md` |
| ODE-005 UUIDv7-Implementierung | M0/M1 | OPEN | – |
| ODE-006 macOS Machine-Durability | vor M5 | OPEN | – |
| ODE-003 Performance-/Ressourcenbudgets | M6 | OPEN | – |
| ODE-002 Desktop in-process oder Sidecar | vor Desktopausbau M8 | OPEN | – |
| ODE-004 Schreib-Supportbasis | M5-Gate | OPEN | – |
| ODE-004 zusätzliche Dateisysteme | spätestens RC | OPEN | – |
| M0–M10 Milestone-Gates | jeweils Phasenende | OPEN | – |

## Blocker

**M0-02a:** Es fehlen ausdrückliche Produktentscheidungen für die 52 `HARD`-IDs in `docs/contract/source_gaps.tsv` sowie zur stärkeren Masterregel für `WDB-HIS-001` (lückenlose Revisionen, Genesis = 0, erster Commit = 1, `Revision::MAX` reservieren). Kein Normtext wurde erfunden. Bereits erledigt: vollständige Lückenklassifizierung und reproduzierbare Arbeitskopien aus M0-02. Nächster Schritt: Produktverantwortung entscheidet, ob die jeweilige Norm ergänzt, verworfen oder an eine konkrete Quellenstelle gebunden wird. M0-04c ist abgeschlossen; M0-04d und M0-06 bleiben unabhängig von diesem Blocker ausführbar. `BLOCKED` ist niemals `DONE`.

## Zusatzauftrag GitHub

Die GitHub-Verknüpfung ist auf Nutzervorgabe vom 29. September 2026 vorerst zurückgestellt; der Plan wird lokal unabhängig davon fortgesetzt.
