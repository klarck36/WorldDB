# M8-02 – CLI-Grundgerüst

**Ergebnis:** PASS auf Windows am 2026-10-03. Die kanonische Befehlsoberfläche ist `worlddb-cli v1 <command>`; der vorhandene unversionierte Aufruf `adapter run` bleibt als Kompatibilitätsalias erhalten. Hilfe, Versionsanzeige und Adapterlauf unterstützen Human- und JSONL-Ausgabe.

## Gelieferter Vertrag

- `--format human|jsonl` wird vor dem Befehl ausgewertet. JSONL-Ergebnisse tragen CLI-Protokoll 1.0, eine UUIDv4-Request-ID und ein geschlossenes `type`/`data`-Outcome. Query-Befehle verwenden gemäß ADR-034 weiterhin das eigene gemeinsame Query-Response-Envelope.
- Die vollständige Zuordnung aller 14 Public Codes zu stabilen Prozess-Exitcodes steht in `docs/architecture/M8-02-cli-contract.md`. Jeder Parser-, Datei-, Manifest- und Prozessfehler geht auf einen geschlossenen Code; unbekannte Argumente, Pfade, Betriebssystemdetails und Adapterdiagnosen werden nie in der Antwort ausgegeben.
- Die CLI erzeugt Request-IDs über das fallible `getrandom::fill`-API. Ist die Entropiequelle nicht verfügbar, schlägt der Aufruf mit `Internal` fehl und nutzt ausschließlich für die Fehlerantwort die Nil-UUID.
- Die direkte CSPRNG-Abhängigkeit wurde im Abhängigkeits- und Crate-Graph-Register auf die bestehende M0-12-Review zurückgeführt. `worlddb-core` und Storage erhalten keine zusätzliche Rückkante.

## Nachweise

- `cargo test --locked -p worlddb-cli --all-targets`: **17 PASS** (8 Unit-Tests, 4 Adapterprozess-Vertragstests und 5 CLI-Blackbox-Tests).
- Die CLI-Blackbox-Tests prüfen Public Codes, Exitcodes, einzeilige JSONL-Hilfe/Version, unbekannte Protokollversionen und Canary-Texte in stdout/stderr. PowerShell `ConvertFrom-Json` bestätigte zusätzlich reale JSONL-Hilfe- und Fehlerantworten als gültige JSON-Objekte.
- `cargo clippy --locked -p worlddb-cli --all-targets -- -D warnings`: **PASS**.
- Dependencyregister, gepinnter `cargo-deny`-Check, Crategraph samt Negativtests: **PASS**.
- `cargo xtask verify`: **39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**; Plancheck, Sourcecheck und Whitespace-Prüfung enthalten.

Die Linux-/macOS-Abnahmen bleiben wie vereinbart M9-07 zugeordnet.
