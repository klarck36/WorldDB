# M8-20 – Job-, Abbruch- und Shutdown-Ansicht

Stand: 4. Oktober 2026

## Ergebnis

Die Desktopoberfläche bietet für ein geöffnetes Projekt eine versionierte, autorisierte Jobliste mit Phase, Status, echten Fortschrittszählern oder „unbestimmt“, Ressourcenbudget, belegtem Speicher, Resume-Metadaten und Abbruchzustand. Die Commitpoint-Race entscheidet atomar: Gewinnt der Commitpoint, meldet die Abbruchaktion `too_late` und die vollständige Veröffentlichung läuft weiter; gewinnt die Abbruchanforderung, kann der Worker den Commitpoint nicht mehr betreten.

Der Engine-Host persistiert begrenzte, gehashte Jobzustände als unveränderliche Generationen im Projekt-Jobjournal. Queued- und Running-Jobs werden beim erneuten Öffnen als unterbrochen gezeigt, wobei der letzte dauerhaft gesicherte Phase-, Fortschritts- und Resume-Stand erhalten bleibt. Der Status wird nicht als Erfolg ausgegeben. Ein nicht lesbares oder nicht beschreibbares Journal nimmt keine neuen Hintergrundjobs an. Die Oberfläche zeigt sichere Shutdown-Drain-Ergebnisse; bei nicht abgeschlossenen Jobs bleibt das Projekt geöffnet und kann erneut geordnet geschlossen werden.

Die Windows-Zweifenster-Smokes prüfen in-process und sidecar die authentisierte Jobliste, die Darstellung von determiniertem und unbestimmtem Fortschritt, Budget, Resume-Hinweis, `too_late` und unterbrochenem Neustartzustand sowie den vollständigen Projekt-Drain. Ein echter Engine-Job wird im Restart-Test bis zu Phase, Zählern und Checkpoint angehalten; nach erneutem Öffnen erscheinen diese Belege mit Status `interrupted`.

## Nachweise

- Core-JobSupervisor: **10 Tests bestanden**, einschließlich Abbruch/Commitpoint-Race, `too_late`, Shutdown-Drain, Deadline-Wiederholung und bounded Progress/Resume-Metadaten.
- ODE In-Process: **16 Desktoptests + 31 Engine-Tests + 1 Sidecar-Transporttest bestanden**.
- ODE Sidecar: **17 Desktoptests + 31 Engine-Tests + 1 Sidecar-Transporttest bestanden**.
- Engine-Tests `durable_job_journal_shows_last_evidence_as_interrupted_after_restart`, `engine_cancel_reports_too_late_and_persists_commitpoint_evidence` und `live_job_changes_roll_back_when_the_durable_checkpoint_fails` bestanden.
- Native Windows IPC-Smokes: **vollständig bestanden in-process und sidecar**, einschließlich leerer Jobliste, Rendererzuständen und Shutdown-Drain.
- Striktes Clippy (`-D warnings`): Root-Workspace, ODE In-Process und ODE Sidecar bestanden.
- Node-Syntax, PowerShell-Parser, Rust-Formatierung und `git diff --check` bestanden.
- `cargo xtask verify`: **39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**.
- Linux-/macOS-Laufzeitprüfungen bleiben wie vereinbart bis M9-07 zurückgestellt.

## Umfang

Der Jobdienst und die Ansicht sind integriert. Bereits vorhandene Migrations-, Backup- und Indexoberflächen übergeben ihre langlaufenden Vorgänge in späteren Aufgaben an `submit_job`; dieser Nachweis nutzt für die Restart-Prüfung einen echten kontrollierten Engine-Job, ohne eine noch nicht implementierte Produktoperation vorzutäuschen.
