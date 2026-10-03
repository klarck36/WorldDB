# M8-09 – Sichere Desktop-IPC auf Windows

**Stand:** 2026-10-03
**Ergebnis:** Windows-Zwischenstand bestanden; Linux/macOS werden wie vereinbart bei M9-07 geprüft
**Vertrag:** `docs/contract/ADR-042-desktop-ipc.md`

## Umsetzung

- Tauri 2.11.5 verwendet eine enge CSP, lokale Frontend-Skripte und eine leere Plugin-Permissionsliste. Das Desktop-Shell hängt kein Dateisystemplugin ein.
- `open_host_session` bindet ein opakes 128-Bit-Ticket an die SID des Windows-Prozesstokens und das native Fensterlabel. Capability-Prüfungen schützen Health- und Transfercommands.
- Health-, Beginn-, Chunk- und Abschlussnachrichten sind versioniert. Geschlossene DTOs weisen unbekannte Felder ab; öffentliche Fehler enthalten nur Protokollversion und stabilen Code.
- Transfers sind auf 100 MiB pro Lauf, 256 KiB pro Rendererblock, vier Transfers pro Sitzung, 16 insgesamt und fünf Minuten Laufzeit begrenzt. Monotone Sequenznummern und ACKs setzen Backpressure um; Abbruch ist zwischen bestätigten Blöcken möglich.
- Beide Engine-Modi verarbeiten dieselben Transfers. Sidecar-Nachrichten verwenden lokale Pipes mit Protokollhandshake; eine ausbleibende Antwort löst nach 5 Sekunden Prozessende und Reconnect beim nächsten Aufruf mit Writerlockprüfung aus.
- Der Transport konsumiert in diesem Task die Daten im begrenzten Stream-Consumer und meldet Digest/Länge. Projektpfade, dauerhafte Projekt-Storageoperationen und Principalbindung bleiben M8-10 vorbehalten.

## Windows-Nachweise

`scripts/run-ipc-security-smoke.ps1` startet die echte Tauri-Anwendung verborgen mit zwei Fenstern. Im Smoke-Modus probiert jedes Fenster zunächst eine ungültige Sitzung, ein `project_path`-Zusatzfeld und den direkten Aufruf `plugin:fs|read_text_file` gegen `C:\Windows\win.ini`. Der Lauf schreibt den Erfolgsmarker erst nach Ablehnung aller drei Probeaufrufe und einem anschließend erfolgreichen autorisierten Health-Aufruf. Beide Fenster schlossen danach normal. Der Prozesscheck fand weder bei der In-Process-App noch bei App und Sidecar einen TCP-Listener.

| Nachweis | In-Process | Sidecar |
|---|---:|---:|
| Zwei Fenster: autorisierte Version-1-IPC | PASS | PASS |
| Ungültige Sitzung abgewiesen | PASS | PASS |
| Renderergewählter Pfad abgewiesen | PASS | PASS |
| Dateisystempluginaufruf abgewiesen | PASS | PASS |
| TCP-Listener an Core-Prozess-IDs | keiner | keiner |
| Sauberes App-Ende | PASS | PASS |

Die gezielten Rust-Negativtests prüfen außerdem Fremdfenster-/Fremdsitzungsbindung, Ablauf und Tabellenlimits, fehlende/ungültige Header, falsche Version, Überschreitung des Chunk-/Transferlimits, übersprungene oder doppelte Sequenzen, unvollständigen Abschluss, Cancellation sowie Sidecar-Timeout und Reconnect.

## Tests und Builds

- In-Process: `cargo test --locked --offline --workspace` – 12 Desktop-Shell-Tests, 5 Engine-Tests und 1 Sidecar-Protokollintegrationstest bestanden.
- Sidecar: `cargo test --locked --offline --workspace --no-default-features --features sidecar` – 13 Desktop-Shell-Tests (einschließlich 5-Sekunden-Timeout/Reconnect), 5 Engine-Tests und 1 Sidecar-Protokollintegrationstest bestanden.
- Beide Profile: `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` und Sidecar entsprechend mit `--no-default-features --features sidecar` – PASS.
- `cargo fmt --all -- --check` – PASS.
- Tauri IPC-Smoke (`scripts/run-ipc-security-smoke.ps1`) – beide Profile und beide Fenster PASS; direkte Dateisystem- und Rendererpfadversuche abgewiesen; keine Netzwerklistener.
- Vollständige ODE-002-Prozessmatrix nach der Sidecar-Timeout-/Reconnect-Änderung – beide 100-MiB-Modi, beide Cancellation-Modi, Digests, Panic, Updates, Writerlock-Reacquisition und Childprozessbereinigung PASS. Fünf Messläufe je Fall: In-Process/Sidecar Vollstream-Median 722.531/709.057 µs, Cancel-Median 57.205/56.998 µs. Die Messung begründet keinen allgemeinen Performancevorteil.

## Grenzen und Übergabe

- Die aktuellen Transfer-Digests beweisen Transport und Engineannahme; sie schreiben noch keine Projektdaten. M8-10 darf den Hostpfad auswählen und öffnen, validiert ihn unter der bestätigten Policy und bindet den aktuellen WorldDB-Principal, ohne Rendererpfad oder Principal zu übernehmen.
- Die SID-Ermittlung ist auf Windows implementiert. Linux/macOS folgen M9-07. Sidecar-Signierung, Installation und Rollback bleiben M9-10.
- Die Sidecar-Prozessmatrix und Messwerte stehen zusätzlich in `docs/M8-08-windows-progress.md`.
