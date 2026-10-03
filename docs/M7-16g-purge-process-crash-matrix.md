# M7-16g – Offline-Purge-Prozessabbrüche

**Stand:** 3. Oktober 2026  
**Plattform:** Windows  
**Ergebnis:** abgeschlossen; Linux/macOS bleiben für M9-07 vorgemerkt.

## Geprüfte Grenzen

Ein eigener Kindprozess wird mit `process::exit(86)` beendet, damit Destruktoren und reguläre Fehlerbereinigung nicht laufen. Er bricht an drei Stellen im Offline-Rewrite ab:

| Grenze | Dauerhafter Zustand beim Prozessende | Ergebnis nach erneutem Öffnen |
| --- | --- | --- |
| Vor dem Auditcommit | Bericht und Stage sind vorbereitet; `PurgePublication` ist noch nicht committed | Quelle hat dieselbe DatabaseId, Revision, saubere Storage-Verify und bytegleichen logischen Export. Ziel fehlt. |
| Vor der Verzeichnisveröffentlichung | Stage ist vollständig geprüft, auditiert und enthält den Bericht; atomare Veröffentlichung hat noch nicht stattgefunden | Quelle bleibt unverändert und sauber. Ziel fehlt. |
| Nach der Verzeichnisveröffentlichung | Stage wurde atomar zum Ziel verschoben und das Zielverzeichnis synchronisiert; die API hat noch keine Receipt zurückgegeben | Quelle bleibt unverändert. Ziel hat eine neue DatabaseId, saubere Storage-Verify, genau ein passendes `PurgePublication`-Required-Audit und einen vollständigen, digestgültigen Bericht. Audit- und Bericht-IDs sowie Revision stimmen überein. Das Secure-Erasure-Bit ist `0`. |

Die ersten zwei Abbrüche können ein verborgenes `.worlddb-purge-stage-*`-Verzeichnis zurücklassen. Es wird nicht als Ziel veröffentlicht. Diese Runde prüft die sichere Sichtbarkeit von Quelle und Ziel; sie fügt keine automatische Bereinigung verwaister Stage-Verzeichnisse hinzu. Der Testbereich entfernt sie nach den Assertions. Es wird keine sichere Löschung behauptet.

## Windows-Nachweise

- `cargo test --locked -p worlddb-storage-file --lib process_crashes_reopen_source_and_publish_only_verified_target -- --nocapture` – PASS; drei Kindprozesse mit dem erwarteten Exitcode 86.
- `cargo test --locked -p worlddb-storage-file --lib purge_rewrite::tests:: -- --nocapture` – 2 PASS.
- `cargo test --locked -p worlddb-storage-file --test m7_15_offline_purge_contract -- --nocapture` – 2 PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – PASS.
- `cargo xtask verify` – 38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.

`cargo xtask verify` umfasste außerdem Format, Workspace-Check, alle Windows-Storage-Tests, Plancheck, Sourcecheck und Whitespace-Prüfung. Linux/macOS werden gemäß Projektvorgabe erst in M9-07 nachgewiesen.
