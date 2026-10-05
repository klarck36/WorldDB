# M8-26a – Windows-E2E-Zwischenstand

## Ausgangslauf

Der vollständige Windows/NTFS-Lauf vom 2026-10-05 (`m8-26a-20261005T012844Z-823933db`) endete mit `FAIL`. Builds, IPC-Smokes (56/56 je Profil) und konkurrierende Schreiber (4 In-Process-, 5 Sidecar-Prüfungen) bestanden. Die vier Tastatur- und Commit-Crashfälle scheiterten beim UI-Automation-Aufruf `SetFocus`, bevor Tastatureingabe oder Commit-Failpoint erreicht wurden.

## Korrekturen

- Der Keyboard-Smoke setzt nun `WORLDDB_ODE_PROJECT_SMOKE_ROOT` auf den erwarteten Projektpfad und deaktiviert den automatischen Startup-Smoke. `WORLDDB_ODE_DATABASE` wird entfernt, weil es das Projekt bereits beim App-Start öffnet und damit die Erstellungsprüfung stört.
- Der Sidecar-Modus legte das Projekt bisher im Desktop-Prozess an und startete den Engine-Sidecar erst danach. Der Bootstrap-Commit läuft nun in einem host-authentifizierten Sidecar-Prozess; nach dessen Erfolg öffnet der Desktop das Projekt und startet den dauerhaften Engine-Prozess. Der WAL-Failpoint kann damit den tatsächlichen Sidecar-Prozess beenden.

## Verifikation am 2026-10-05

- `cargo fmt --manifest-path experiments/ode-002/Cargo.toml --all -- --check`: PASS.
- Workspace-Builds für `in-process` und `sidecar`: PASS.
- Striktes Clippy für beide Desktopprofile: PASS.
- `cargo xtask verify`: 39 PASS, 1 erwarteter CI-Matrix-SKIP, 0 FAIL; Plancheck und Sourcecheck: PASS.
- Manuelle native In-Process-Tastaturprüfung: PASS. Texteingabe, Tab, Enter, Projektname und read-only Recovery wurden geprüft.
- In-Process-Commit-Crash: Desktopprozess endete mit Exitcode 86 nach `after_wal_commit_sync`. Read-only Recovery meldete `safe_revision=1`, `RecoveryRequired`, `source_modified=false`.
- Sidecar-Bootstrap: normaler Prozesslauf endete mit 0 und read-only Recovery meldete `safe_revision=1`, `Clean`. Mit aktiviertem Failpoint endete der Engine-Prozess mit 86; Recovery meldete `safe_revision=1`, `RecoveryRequired`, `source_modified=false`.
- Die gekürzten Resultate stehen in `experiments/ode-002/evidence/native-e2e/m8-26a-manual-results-20261005/manual-supplement.json`. Das offizielle Vollmanifest folgt nach dem vollständigen Runnerlauf.

## Status

`M8-26a` bleibt `PLANNED`, bis der vorbereitende Task `M8-26` abgeschlossen ist. Danach fehlen für M8-26a der erneute vollständige Native-E2E-Lauf, der Sidecar-Tastaturfall und die erneute IPC-/Writer-Lock-Prüfung. Der historische `FAIL`-Lauf bleibt als unveränderte Ausgangsevidenz erhalten. macOS/APFS und Linux/ext4 bleiben wie vereinbart bis M9-07 zurückgestellt.
