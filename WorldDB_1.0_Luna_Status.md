# WorldDB 1.0 – Arbeitsstatus

**Stand:** 29. September 2026  
**Gesamtstatus:** IN PROGRESS – Quelleninventar M0-01 abgeschlossen; Software-Implementierung hat noch nicht begonnen  

**Nächste Task:** `M0-02`  

**Letzter abgeschlossener Milestone:** keiner  
**Offene Blocker:** keine im Arbeitsplan; GitHub-Zusatzauftrag wartet auf Browser-Anmeldung  

**M0-01 Baseline-Commit:** `0bb3c85` (`chore: establish WorldDB source baseline`).

Die vollständigen Tasktexte stehen in `WorldDB_1.0_Luna_Arbeitsplan.md`; ausführbar ist die erste `READY`-Task des aktuellen Milestones im Taskregister. Nur nach belegter Abnahme darf die entsprechende Checkbox auf `[x]` wechseln. Jede `HARD`-Invariante braucht bis M10 mindestens einen wirksamen Negativnachweis in `WorldDB_1.0_Invariantenabdeckung.tsv`; spätere paarbezogene Nachweise stehen in `WorldDB_1.0_Folgebelege.tsv`.

`WorldDB_1.0_Taskregister.tsv` ist die verbindliche Liste von `depends_on` und Taskstatus. Nach jeder Task `PLANNED` mit erfüllten Abhängigkeiten auf `READY` setzen; `BLOCKED` erst nach Wegfall des konkreten Blockers erneut freigeben. Gültige Zustände: `PLANNED`, `READY`, `RUNNING`, `WAITING_EXTERNAL`, `BLOCKED`, `DONE`. Für `WAITING_EXTERNAL` Run-ID/URL, Besitzer, `last_check` und `next_check` als ISO-8601-Zeitpunkte mit Zeitzonenoffset im Taskregister und hier festhalten; zu Beginn jedes Durchgangs fällige Runs prüfen, echte Ergebnisse/Belege übernehmen und anschließend `DONE` oder `BLOCKED` setzen. Eine unterbrochene `RUNNING`-Task wird vor der nächsten `READY`-Task fortgesetzt; mehrere `RUNNING`-Tasks sind unzulässig. Gibt es keine ausführbare Task und keinen fälligen Run, den frühesten `next_check` oder konkreten Blocker protokollieren und den Durchgang ohne Erfolgsmeldung beenden. Unabhängige `READY`-Tasks desselben Milestones dürfen nach Start eines wartenden Langlaufs weiterlaufen. Ein Milestone-Gate bleibt bis zum Ende aller Langläufe offen. Ein grüner `--gate-precheck` ist nur eine Strukturprüfung; jedes Gate benötigt ein Review realer Test-/Run-Artefakte.

## Laufendes Protokoll

| Task | Status | Commit/Artefakt | Positiver Nachweis | Negativer/Fault-/Security-Nachweis | `cargo xtask verify` | Externer Lauf/letzte Prüfung/nächster Prüftermin | Nächster Schritt |
|---|---|---|---|---|---|---|---|
| M0-01 | DONE | `0bb3c85`; `docs/M0-01-verification.md` | ZIP-Test, 6 Bytevergleiche und Plancheck bestanden | Temporär veränderte Spiegeldatei mit Exitcode 1 abgewiesen | Nicht fällig (M0-01) | – | M0-02 READY |
| M0-02 | RUNNING | – | Quellen-/Normtextvergleich in Arbeit | – | Noch nicht fällig | – | TOML-Parser und 253-ID-Abgleich erstellen |

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

Keine Plan-Task blockiert. Neue Plan-Blocker nur mit Task-ID, exaktem fehlendem Nachweis, bereits erledigter unabhängiger Arbeit und nächster erforderlicher Aktion eintragen. `BLOCKED` ist niemals `DONE`.

## Zusatzauftrag GitHub

Noch nicht verknüpft: der Codex-Browser zeigt die GitHub-Anmeldeseite. Nach manueller Anmeldung kann das private Remote `klarck36/WorldDB-1.0` angelegt und der Baseline-Commit veröffentlicht werden.
