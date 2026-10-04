# M8-14b – Timeline-/TimeUnit-Verwaltung in ODE

## Ergebnis

M8-14b ergänzt die authentisierte Schema-API und das ODE-Desktopformular für `Timeline` und `TimeUnit`. Beide Definitionen werden über denselben revisionsgebundenen Schema-Commit veröffentlicht und erscheinen in aktuellen, historischen und expliziten Schemaansichten.

`Timeline` erhält eine von der Anwendung erzeugte stabile `TimelineId`. Das Formular bietet das Profil ohne zivilen Kalender oder `proleptic_gregorian_utc` mit vorzeichenbehafteter Epoch in Nanosekunden. `TimeUnit` verwendet sein validiertes ASCII-Symbol als stabile Identität und speichert Nanosekunden pro Tick als positive `u64`-Skala. Epoch und Skala laufen als Dezimaltext über UI und IPC, damit keine JavaScript-Zahlengenauigkeit verloren geht. Die Schemaansicht zeigt Profil, Epoch, Skala und Lifecycle.

Die API parst Symbole über `Symbol::new`, Epochs als `i128` und Skalen als `u64`; null, Überläufe und ungültige Symbole werden vor dem Commit abgewiesen. `Deprecated` und `Retired` werden als append-only Lifecycle-Revisionen für beide Familien veröffentlicht. ODE zeigt diese Zustände samt den zugehörigen Schreibregeln an.

## Windows-Nachweise

| Prüfung | Ergebnis |
| --- | --- |
| `schema::tests::timeline_and_time_unit_drafts_validate_and_publish_exact_views` | PASS; i128-Minimum und u64-Maximum exakt, ungültiges Symbol, Epoch-Überlauf, Nullskala und u64-Überlauf abgewiesen |
| `schema::tests::schema_commands_have_stable_closed_json_shapes` | PASS; geschlossene Timeline-/TimeUnit-IPC-Drafts und exakte Dezimalstrings |
| ODE-002 `cargo test --locked --offline --workspace --all-targets` | PASS; 15 Desktop-, 21 Engine- und 1 Sidecar-Transporttest |
| ODE-002 Sidecar `cargo test --locked --offline --workspace --all-targets --no-default-features --features sidecar` | PASS; 16 Desktop-, 21 Engine- und 1 Sidecar-Transporttest |
| ODE-002 strict Clippy mit `-D warnings`, In-Process und Sidecar | PASS |
| `cargo xtask verify` | PASS; 39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL |
| Plancheck, Sourcecheck und `git diff --check HEAD` | PASS |
| `run-ipc-security-smoke.ps1 -Mode in-process` | PASS; authentisierte Fenster, Timeline- und TimeUnit-Formulare, exakte Werte, Fehlergrenzen, historische/explizite Ansichten und sichtbare Lifecycle-Wechsel |
| `run-ipc-security-smoke.ps1 -Mode sidecar` | PASS; dieselbe Ende-zu-Ende-Abnahme über den Sidecar |
| `node --check experiments/ode-002/crates/desktop-shell/frontend/main.js` | PASS |
| PowerShell-Parser für `run-ipc-security-smoke.ps1` | PASS |

Der native Smoke-Autostopp wurde auf 90 Sekunden angehoben, damit die gesamte Desktopabfolge einschließlich der zusätzlichen Zeitregisterprüfungen fertigläuft.

Linux- und macOS-Prüfungen bleiben gemäß Projektentscheidung bis M9-07 zurückgestellt.
