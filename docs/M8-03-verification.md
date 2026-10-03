# M8-03 – CLI Verify, Recovery und Salvage

**Stand:** 2026-10-03  
**Plattform:** Windows 11 / NTFS  
**Status:** abgeschlossen auf Windows; Linux/macOS bleiben gemäß Projektvorgabe für M9-07 zurückgestellt.

## Ergebnis

Die CLI stellt `v1 verify`, `v1 recovery inspect`, `v1 recovery run --apply`, `v1 open --read-only` und `v1 salvage` bereit. Verify-, Inspect-, Read-only-Open- und Salvage-Aufrufe verwenden einen gemeinsamen Leselock auf einer bereits vorhandenen `LOCK`-Datei. Fehlt die Datei, kommt `StorageRead` zurück; der Aufruf legt keine Datei an. Der gemeinsame Lock blockiert parallele WorldDB-Writer.

Die Berichte enthalten `safe_revision`, Recovery-Einstufung, Befundzahl, alle sechs Schadensklassen und die Vereinigung der sicheren Folgeaktionen. JSONL gibt pro Aufruf genau einen versionierten Bericht aus und verschweigt Quell-/Zielpfade sowie dynamische Storage-Diagnosen. Salvage meldet nur die neue Datenbankidentität und aggregierte Kopier-/Auslasszahlen; der vollständige Verlust- und Unsicherheitsbericht liegt im markierten Fork-Archiv.

Recovery repariert ausschließlich nach `--apply`. Der CLI-Vertragstest beschädigt einen uncommitted WAL-Tail, weist die Ablehnung ohne `--apply` nach, führt die journalisierte Reparatur explizit aus und prüft danach eine saubere Revision erneut. Read-only-Kommandos und Salvage lassen den vollständigen Quelldateibaum unverändert.

## Windows-Verifikation

- `cargo test --locked -p worlddb-cli --all-targets` – 20 bestanden, 0 fehlgeschlagen.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – bestanden.
- `cargo fmt --all -- --check` – bestanden.
- `cargo test --locked -p worlddb-storage-file --test m5_11_recovery_contract --test m5_14_salvage_contract` – 13 bestanden, 0 fehlgeschlagen.
- JSONL-Ausgaben aus sechs tatsächlichen CLI-Aufrufen wurden mit einem JSON-Parser geparst; alle waren gültig.
- Vollständiger Lauf `cargo xtask verify` – 39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.

Die Integrationstests prüfen fehlende `LOCK`-Datei ohne Seiteneffekt, Leselock gegen parallelen Writer, saubere Verify-/Recovery-Inspect-/Read-only-Open-Berichte, einen `Truncation`-Befund samt `PreserveOriginal` und `RunJournaledTailRecovery`, unveränderte Quelldateien nach Verify und Salvage, Zielarchiv-Erzeugung sowie die explizite Recovery-Freigabe.
