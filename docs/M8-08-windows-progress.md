# M8-08 – ODE-002 Windows-Nachweis

**Stand:** 2026-10-03
**Ergebnis:** Windows-Zwischenstand bestanden; Linux/macOS für M9-07 vorgemerkt
**Entscheidung:** `docs/contract/ADR-041-ode-002-desktop-process.md`

## Umfang und Umgebung

Der Spike vergleicht Tauri 2.11.5 mit Rust 1.85.0 in zwei getrennten Builds: Engine in-process und lokale Engine als Sidecar. Tauri erstellt zwei native Fenster (`primary`, `secondary`), die automatisierten Fensterläufe bleiben unsichtbar. Beide Modi öffnen dasselbe WorldDB-Dateilayout und beanspruchen den echten exklusiven `WriterLock` aus `worlddb-storage-file`.

Der Sidecar erhält beim Start den Datenbankpfad und kommuniziert in diesem Spike über private Standard-I/O-Pipes: JSONL für Steuerung, danach Datenframes mit 1-Byte-Typ, 4-Byte-Länge und maximal 1 MiB Payload. Der Gesamtdatenstrom ist auf 100 MiB begrenzt. Der Testpfad erzeugt Daten in 256-KiB-Chunks und hasht sie mit BLAKE3. Diese Implementierung ist ein Prozess- und Transportspike; M8-09 muss den sicheren Produktionsvertrag für Renderer-IPC, Autorisierung, Backpressure, Cancellation, Fehler und Wiederverbindung umsetzen.

## Ergebnisse

| Prüfung | In-Process | Sidecar |
|---|---:|---:|
| Zwei native Fenster | PASS | PASS |
| Erster Writerlock | PASS | PASS |
| Zweiter gleichzeitiger Schreiber abgewiesen | PASS | PASS |
| Reopen nach App-Ende | PASS | PASS |
| Fünf vollständige 100-MiB-Streams | PASS | PASS |
| Fünf Cancels exakt bei 8 MiB | PASS | PASS |
| Identische Digests beider Modi | PASS | PASS |
| Engine-Panic | vergifteter Zustand erkannt; App-Neustart nötig | Kindprozess beendet; Engine neu gestartet; App blieb bestehen |
| In-Process-Appupdate | anderer App-Build gestartet; Writerlock erneut erworben | entfällt |
| Sidecar-Update | entfällt | anderer Engine-Build gestartet; App-PID blieb gleich |
| Childprozesse nach App-Ende | entfällt | alle geprüft und beendet |

Pro Modus werden für den vollständigen Stream und den abgebrochenen Stream je fünf Laufzeiten erfasst. Drei Wiederholungen der vollständigen Prozessmatrix ergaben:

| Matrixlauf | In-Process: 100 MiB Median | Sidecar: 100 MiB Median | In-Process: Cancel-Median | Sidecar: Cancel-Median |
|---|---:|---:|---:|---:|
| 1 | 678.725 µs | 685.948 µs | 56.480 µs | 55.019 µs |
| 2 | 720.420 µs | 694.508 µs | 57.538 µs | 56.121 µs |
| 3 (nach M8-09-Pipe-Timeout-/Reconnect-Änderung) | 722.531 µs | 709.057 µs | 57.205 µs | 56.998 µs |

Die vollständige Streamzeit wechselte zwischen den Wiederholungen in der Reihenfolge; diese kleine Stichprobe zeigt keinen stabilen Geschwindigkeitsvorteil. Die Prozesswahl beruht daher auf Panic-Isolation und Engine-Update ohne App-Neustart, nicht auf einem Performanceclaim.

## Verifikation

- `cargo check --locked --manifest-path experiments/ode-002/Cargo.toml --workspace` – PASS.
- `cargo check --locked --manifest-path experiments/ode-002/Cargo.toml -p worlddb-ode-desktop-shell --no-default-features --features sidecar` – PASS.
- `cargo test --locked --manifest-path experiments/ode-002/Cargo.toml -p worlddb-ode-engine` – aktuell 5 PASS, 0 FAIL.
- `cargo clippy --locked --manifest-path experiments/ode-002/Cargo.toml --workspace --all-targets -- -D warnings` – PASS.
- Sidecar-Feature-Clippy mit `--no-default-features --features sidecar --all-targets -- -D warnings` – PASS.
- `cargo fmt --manifest-path experiments/ode-002/Cargo.toml --all` und erneuter Formatcheck – PASS.
- `scripts/run-writer-lock-smoke.ps1 -Mode in-process` – zwei Fenster, Writerlock, konkurrierender Prozess, Reopen PASS.
- `scripts/run-writer-lock-smoke.ps1 -Mode sidecar` – dieselben Prüfpunkte und Childprozessbereinigung PASS.
- `scripts/run-process-matrix.ps1` – drei vollständige Matrixläufe; der dritte nach M8-09-Pipe-Timeout-/Reconnect-Änderung. Beide Streampfade, Digestgleichheit, Panic, App-/Engineupdate, Lock-Reacquisition und Childbereinigung PASS.

Beim Herunterfahren meldete WebView2 in einzelnen Läufen `Chrome_WidgetWin_0 / 1412`. Der App-Prozess beendete sich, der Writerlock wurde anschließend wieder erworben und der Sidecar wurde bereinigt. Der Spike unterdrückt diese Ausgabe nicht.

## Grenzen

Dieser Nachweis gilt ausschließlich für die lokale Windows-Umgebung. Er belegt keine Linux-/macOS-Fenster-, Lock-, Packaging-, Update- oder Plattformgarantie. M9-07 holt diese Matrix nach. Die zuerst gezeigte Pipe war keine Sicherheitsgrenze. M8-09 ergänzt den versionierten, sitzungs- und capabilitygebundenen Renderer-IPC, begrenzte ACK-Streams, Timeouts und Sidecar-Reconnect; Details stehen in ADR-042 und `docs/M8-09-windows-progress.md`. M8-10 bindet Projekte und Principals. Produktionssignatur, Installation und Rollback des Sidecars müssen mit M9-10 belegt werden.
