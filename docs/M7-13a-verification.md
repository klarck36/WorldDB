# M7-13a – Windows-Verifikation

## Abgedeckte Grenze

Der Import-/Exportadapter läuft als separater Windows-Prozess. Der Prozessadapter erstellt ihn angehalten, weist ihn einem Windows-Job zu und setzt Prozess- und aggregierte Committed-Memory-Grenzen sowie ein Limit für aktive Nachkommen, bevor der Adaptercode läuft. Die CLI übergibt nur das digestsichere Manifest und begrenzte Nutzbytes; sie reicht weder Core-/Storage-Handles noch Datenbankpfade durch.

Der Host verhandelt ProtocolMajor/Minor und jede Required-Capability vor dem Senden der Nutzdaten. Er akzeptiert nur einen digestsicheren Outputframe, EOF und einen erfolgreichen Kindprozess-Exit. Fehlende Capability, ungültige Antwort, Timeout, Ressourcenfehler oder Crash geben kein Ergebnis zurück; die CLI schreibt die Outputdatei erst nach erfolgreichem Adapterende.

## Prüfergebnisse auf Windows

- `cargo test --locked --workspace` – bestanden; alle Workspace-Unit-, Integrations- und Rustdoc-Suites.
- `cargo test -p worlddb-process-adapter --all-targets` – drei Windows-Prozess-/Ressourcentests bestanden.
- `cargo test -p worlddb-cli --all-targets` – fünf Protokoll-Unit- und vier Windows-End-to-End-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – bestanden.
- `cargo fmt --all -- --check` – bestanden.
- `cargo check --locked --workspace --all-targets` – bestanden.
- Plancheck, Sourcecheck, Unsafe-, Workspace-Lint-, Exception-, Dependency- und Crate-Graph-Policies – bestanden.
- `cargo xtask verify` – 38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `git diff --check` – bestanden.

Die End-to-End-Tests bestätigen getrennte Prozess-ID, Echo-Roundtrip, Abbruch bei fehlender Capability, unveränderten vorherigen Output nach Crash, Timeout ohne Output und Speicherlimit ohne Output. Der Prozessadaptertest bestätigt zusätzlich, dass verbleibende Nachkommen durch das Job-Objekt beendet werden.

Linux/macOS-Adapter und Plattformabnahmen bleiben in M9-07. Der Prozessadapter weist diese Plattformen derzeit geschlossen ab; es gibt keinen unbegrenzten Ausweichlauf. Die Windows-Prüfung behauptet keine Linux-/macOS-Abnahme.
