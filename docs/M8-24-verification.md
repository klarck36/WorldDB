# M8-24 — Fehler- und Diagnoseoberfläche

**Ergebnis:** bestanden auf Windows für In-Process und Sidecar. Linux/macOS bleiben gemäß Projektentscheidung bis M9-07 zurückgestellt.

## Umsetzung

- `IpcErrorV1` enthält nur Protokollversion, stabilen Public Code, Lokalisierungsschlüssel, nächste Aktion sowie optionale sichere OperationId und DatabaseId. Technische Ursachen erscheinen weder in JSON noch in der manuellen `Debug`-Darstellung.
- Die Oberfläche ordnet nur bekannte Public Codes festen deutschen Meldungen zu. Unbekannte oder renderer-manipulierte `message`, `detail`, Lokalisierungs- und Aktionsfelder werden nicht angezeigt; Fehlertext enthält Public Code, Lokalisierungsschlüssel und nächste Aktion. Eine OperationId wird nur nach UUID-Validierung angezeigt.
- Technische Ursachen verbleiben im Hostspeicher. Der Diagnose-Ring ist auf 128 Einträge und 4 KiB je Ursache begrenzt und exportiert nur Einträge der aktiven DatabaseId.
- Diagnoseexport erfordert die aktuellen `AuditRead`- und `AuditExport`-Rechte. DatabaseId und Rechte werden vor und nach dem nativen Speicherdialog erneut geprüft. Renderer können keinen Zielpfad angeben. Exporte werden ohne Überschreiben angelegt, auf 1 MiB begrenzt und erhalten einen BLAKE3-Digest.
- Die Diagnoseoberfläche informiert darüber, dass der autorisierte technische Export interne Ursachen und lokale Pfade enthalten kann.

## Nachweise

- Desktoptests: 36 In-Process bestanden; 37 Sidecar bestanden.
- Striktes Clippy (`-D warnings`): In-Process und Sidecar bestanden.
- Nativer Windows-Zwei-Fenster-Smoke: In-Process bestanden, alle Prüfpunkte PASS, einschließlich `diagnostic_public_error_canary_rejected` und `diagnostic_renderer_paths_rejected_before_host_dialogs`.
- Nativer Windows-Zwei-Fenster-Smoke: Sidecar bestanden, dieselben Diagnoseprüfpunkte PASS.
- Im nativen Smoke speichert der Host einen internen Canary und liefert nur den öffentlichen Fehlervertrag zurück. Die Rendereranzeige wird zusätzlich mit manipulierten `detail`, `technical_detail`, `message`, `message_key` und `next_action_key` geprüft. Ein manipuliertes Diagnoseexport-Request mit Rendererpfad wird vor dem Hostdialog zurückgewiesen.
- `cargo fmt --all -- --check`, `node --check experiments/ode-002/crates/desktop-shell/frontend/main.js`, PowerShell-Parserprüfung des Smoke-Skripts und `git diff --check`: bestanden.
- `WorldDB_1.0_Plancheck.py` und `WorldDB_1.0_Sourcecheck.py`: bestanden; Quellarchiv und Quellspiegel stimmen bytegenau.
- `cargo xtask verify`: 39 PASS, 1 erwarteter `ci-matrix`-SKIP wegen M0-14, 0 FAIL.
- `git diff --check`: bestanden.

## Ausgeführte Prüfungen

```text
cargo test -p worlddb-ode-desktop-shell --no-default-features --features in-process
cargo test -p worlddb-ode-desktop-shell --no-default-features --features sidecar
cargo clippy -p worlddb-ode-desktop-shell --no-default-features --features in-process --all-targets -- -D warnings
cargo clippy -p worlddb-ode-desktop-shell --no-default-features --features sidecar --all-targets -- -D warnings
./experiments/ode-002/scripts/run-ipc-security-smoke.ps1 -Mode in-process -ExecutablePath ./experiments/ode-002/target/debug/worlddb-ode-desktop-shell.exe -KeepArtifacts
./experiments/ode-002/scripts/run-ipc-security-smoke.ps1 -Mode sidecar -ExecutablePath ./experiments/ode-002/target/debug/worlddb-ode-desktop-shell.exe -EngineExecutablePath ./experiments/ode-002/target/debug/worlddb_ode_engine.exe -KeepArtifacts
```
