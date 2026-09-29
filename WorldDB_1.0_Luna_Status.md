# WorldDB 1.0 – Arbeitsstatus

**Stand:** 29. September 2026  
**Gesamtstatus:** IN PROGRESS – M0-01 und M0-02 abgeschlossen; M0-02a wartet auf Produktentscheidungen; Software-Implementierung hat noch nicht begonnen  

**Nächste Task:** `M0-03`  

**Letzter abgeschlossener Milestone:** keiner  
**Offene Blocker:** M0-02a benötigt Produktentscheidungen zu 52 HARD-Quellenlücken und WDB-HIS-001; GitHub-Remote kann mit dem verfügbaren Connector nicht angelegt werden und der Codex-Browser wartet auf Anmeldung  

**M0-01 Baseline-Commit:** `0bb3c85` (`chore: establish WorldDB source baseline`).

Die vollständigen Tasktexte stehen in `WorldDB_1.0_Luna_Arbeitsplan.md`; ausführbar ist die erste `READY`-Task des aktuellen Milestones im Taskregister. Nur nach belegter Abnahme darf die entsprechende Checkbox auf `[x]` wechseln. Jede `HARD`-Invariante braucht bis M10 mindestens einen wirksamen Negativnachweis in `WorldDB_1.0_Invariantenabdeckung.tsv`; spätere paarbezogene Nachweise stehen in `WorldDB_1.0_Folgebelege.tsv`.

`WorldDB_1.0_Taskregister.tsv` ist die verbindliche Liste von `depends_on` und Taskstatus. Nach jeder Task `PLANNED` mit erfüllten Abhängigkeiten auf `READY` setzen; `BLOCKED` erst nach Wegfall des konkreten Blockers erneut freigeben. Gültige Zustände: `PLANNED`, `READY`, `RUNNING`, `WAITING_EXTERNAL`, `BLOCKED`, `DONE`. Für `WAITING_EXTERNAL` Run-ID/URL, Besitzer, `last_check` und `next_check` als ISO-8601-Zeitpunkte mit Zeitzonenoffset im Taskregister und hier festhalten; zu Beginn jedes Durchgangs fällige Runs prüfen, echte Ergebnisse/Belege übernehmen und anschließend `DONE` oder `BLOCKED` setzen. Eine unterbrochene `RUNNING`-Task wird vor der nächsten `READY`-Task fortgesetzt; mehrere `RUNNING`-Tasks sind unzulässig. Gibt es keine ausführbare Task und keinen fälligen Run, den frühesten `next_check` oder konkreten Blocker protokollieren und den Durchgang ohne Erfolgsmeldung beenden. Unabhängige `READY`-Tasks desselben Milestones dürfen nach Start eines wartenden Langlaufs weiterlaufen. Ein Milestone-Gate bleibt bis zum Ende aller Langläufe offen. Ein grüner `--gate-precheck` ist nur eine Strukturprüfung; jedes Gate benötigt ein Review realer Test-/Run-Artefakte.

## Laufendes Protokoll

| Task | Status | Commit/Artefakt | Positiver Nachweis | Negativer/Fault-/Security-Nachweis | `cargo xtask verify` | Externer Lauf/letzte Prüfung/nächster Prüftermin | Nächster Schritt |
|---|---|---|---|---|---|---|---|
| M0-01 | DONE | `0bb3c85`; `docs/M0-01-verification.md` | ZIP-Test, 6 Bytevergleiche und Plancheck bestanden | Temporär veränderte Spiegeldatei mit Exitcode 1 abgewiesen | Nicht fällig (M0-01) | – | M0-02 READY |
| M0-02 | DONE | `cc8b190`; `docs/M0-02-verification.md`; `docs/contract/source-errata.json`; `docs/contract/source_gaps.tsv` | `build_contract_sources.py --verify-only`, 253-ID-Abgleich, 149 Haupttextbindungen, Sourcecheck und Plancheck bestanden | Manipulierte Kopie von `source_gaps.tsv` mit Exitcode 1 abgewiesen | Noch nicht fällig | geprüft 2026-09-29T02:00:47+02:00 | M0-03 READY |
| M0-02a | BLOCKED | `docs/contract/source_gaps.tsv`; `docs/contract/source-errata.json` | 54 Lücken präzise klassifiziert; Produktnormen nicht erfunden | Nicht fällig bis Entscheidung | Noch nicht fällig | 2026-09-29T02:00:47+02:00 | Produktentscheidung zu 52 HARD-Lücken und stärkerem WDB-HIS-001-Mastertext erforderlich |
| M0-03 | READY | – | – | – | Noch nicht fällig | – | 22 Typen aus Master §33 erfassen und eigenständigen Docs-Verify bauen |

## Entscheidungs- und Release-Gates

| Entscheidung/Gate | Fällig | Status | Nachweis |
|---|---|---|---|
| ODE-001 Rust-MSRV | M0 | OPEN | – |
| ODE-005 UUIDv7-Implementierung | M0/M1 | OPEN | – |
| ODE-006 macOS Machine-Durability | vor M5 | OPEN | – |
| ODE-003 Performance-/Ressourcenbudgets | M6 | OPEN | – |
| ODE-002 Desktop in-process oder Sidecar | vor Desktopausbau M8 | OPEN | – |
| ODE-004 Schreib-Supportbasis | M5-Gate | OPEN | – |
| ODE-004 zusätzliche Dateisysteme | spätestens RC | OPEN | – |
| M0–M10 Milestone-Gates | jeweils Phasenende | OPEN | – |

## Blocker

**M0-02a:** Es fehlen ausdrückliche Produktentscheidungen für die 52 `HARD`-IDs in `docs/contract/source_gaps.tsv` sowie zur stärkeren Masterregel für `WDB-HIS-001` (lückenlose Revisionen, Genesis = 0, erster Commit = 1, `Revision::MAX` reservieren). Kein Normtext wurde erfunden. Bereits erledigt: vollständige Lückenklassifizierung und reproduzierbare Arbeitskopien aus M0-02. Nächster Schritt: Produktverantwortung entscheidet, ob die jeweilige Norm ergänzt, verworfen oder an eine konkrete Quellenstelle gebunden wird. Unabhängige M0-03- und M0-06-Arbeit läuft weiter. `BLOCKED` ist niemals `DONE`.

## Zusatzauftrag GitHub

Der GitHub-Connector ist als `klarck36` authentifiziert und kann Repositories lesen; darunter ist noch kein Repository `WorldDB-1.0`. Der Connector bietet in dieser Sitzung keine Funktion zum Erstellen eines Repositories, und lokal ist weder ein Remote konfiguriert noch `gh` installiert. Der Codex-Browser zeigt separat die GitHub-Anmeldeseite. Das private Remote `klarck36/WorldDB-1.0` bleibt daher offen; anschließend müssen die lokalen Commits veröffentlicht und die Verbindung geprüft werden.
