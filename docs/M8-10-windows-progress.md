# M8-10 – Projektstart und Multiwindow auf Windows

**Stand:** 2026-10-03
**Ergebnis:** Windows-Nachweis bestanden; Linux/macOS werden wie vereinbart bei M9-07 geprüft
**Entscheidung:** `docs/contract/ADR-043-project-host-binding.md`

## Umsetzung

- Die Host-SID wird aus dem Windows-Prozesstoken gelesen. Eine gemeinsame Domain-separated BLAKE3-Ableitung liefert eine stabile UUID-v8-Principal-ID; rohe SID-Daten und frei wählbare Principal-IDs verlassen den Host nicht.
- Der Desktophost nimmt Namen und Ordner nur über geschlossene DTOs beziehungsweise native Windows-Ordnerauswahl entgegen. Rendererfelder `project_path` und `principal` werden abgewiesen. Beim Erstellen wird der Elternordner kanonisiert.
- Das erste Projektcommit enthält Genesis, die registrierten Rollen `gm`/`player`, den GM-Auftrag an das angemeldete Konto, die initiale Policy und den geforderten SecurityPolicy-Audit. Der Ersteller erhält kein `AdminRawRead`.
- Öffnen prüft Reparse-Point, kanonischen Pfad, Datenbankformat, Manifest und Security-History, exklusiven Writerlock sowie den sauberen Recoveryzustand. Öffnen repariert, migriert oder schreibt nicht; ein unvollständiger WAL-Endbereich bleibt bytegleich und führt zu `recovery_required`.
- Die Anwendung hält eine Engine und ein Projekt. Beide nativen Fenster teilen das Handle und die Revision, bekommen aber unabhängige zufällige Snapshot-IDs. Ein anderes Projekt erfordert zuerst das anwendungsweite Schließen.
- Im Sidecar übergibt der Host keine Principal-ID. Der Sidecar liest sein eigenes Windows-Prozesstoken; ein gesetzter Principal-Umgebungswert wird entfernt. Der ungebundene Enginepfad ist auf Debug-Benchmarks begrenzt und wird im Release-Build verweigert.

## Windows-Nachweise

Der echte Tauri-Smoke `experiments/ode-002/scripts/run-ipc-security-smoke.ps1` startet beide Fenster. Das primäre Fenster erstellt ein frisches Projekt, das sekundäre öffnet dasselbe Projekt. Die Marker bestätigen gleiche Datenbank-ID, Revision 1 und Rolle `gm` bei zwei unterschiedlichen Snapshot-IDs. Der Sidecar-Lauf setzt zusätzlich eine fremde Principal-Umgebungsvariable; das Projekt öffnet trotzdem nur mit der vom Sidecar selbst gelesenen Windows-Identität.

| Prüfung | In-Process | Sidecar |
|---|---:|---:|
| Projekt vom Hostkonto erstellt/geöffnet | PASS | PASS |
| Gleiches Projekt, eigene Fenster-Snapshots | PASS | PASS |
| Rendererpfad und Rendererprincipal abgewiesen | PASS | PASS |
| Principal nicht über Umgebungsvariable wählbar | PASS | PASS |
| Ungültige Sitzung und Dateisystemplugin abgewiesen | PASS | PASS |
| Netzwerklistener an App-/Engineprozessen | keiner | keiner |
| Prozesse sauber beendet | PASS | PASS |

Gezielte Windows-Tests bestätigen außerdem: ein zweiter autorisierter Enginewriter wird gesperrt; falscher Principal erhält keinen Zugriff; bestehende Ziele werden nicht neu initialisiert; ein leeres/ungültiges Projekt wird ohne Änderung abgewiesen; ein NTFS-Junction-Projektstamm wird abgewiesen; ein beschädigter WAL-Tail wird nicht automatisch repariert oder verändert.

## Build- und Testnachweise

- `cargo fmt --manifest-path experiments/ode-002/Cargo.toml --all -- --check` – PASS.
- `cargo check --locked --offline --manifest-path experiments/ode-002/Cargo.toml --workspace` und dasselbe mit `--no-default-features --features sidecar` – PASS.
- In-Process: `cargo test --locked --offline --manifest-path experiments/ode-002/Cargo.toml --workspace` – 15 Desktop-Shell-, 11 Engine- und 1 Sidecar-Protokolltest bestanden.
- Sidecar: derselbe Workspace-Test mit `--no-default-features --features sidecar` – 16 Desktop-Shell- (einschließlich Timeout/Reconnect), 11 Engine- und 1 Sidecar-Protokolltest bestanden.
- Striktes Clippy (`--workspace --all-targets -- -D warnings`) für beide Profile – PASS.
- Release-Engine ohne den authentisierten Hostmodus gestartet – Exitcode 73, kein Projektordner angelegt: PASS.
- Reale IPC-Smokes für In-Process und Sidecar – beide Fenster, Projektanlage/-öffnung, Snapshottrennung, Sicherheits-Negativtests, keine Netzwerklistener und sauberer Shutdown: PASS.
- `node --check experiments/ode-002/crates/desktop-shell/frontend/main.js` – PASS.
- `cargo xtask verify` – 39 Schritte bestanden, 1 vorgesehener Skip (`ci-matrix`, separater M0-14-Schritt), 0 fehlgeschlagen.

## Grenzen und Übergabe

- Hostkontoabbildung nutzt die Windows-SID. Andere Plattformen bleiben bis M9-07 fail-closed und sind hier nicht verifiziert.
- Eine ACL-spezifische Verweigerung durch Windows wurde nicht separat simuliert; Betriebssystemfehler bei Verzeichnis-/Formatzugriff werden ohne Reparatur oder Berechtigungsanhebung abgewiesen.
- Nächster freigegebener Planpunkt: M8-04 CLI-Backup/Restore, das ab jetzt die hostgebundene aktuelle Policy verwenden kann.
