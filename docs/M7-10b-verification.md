# M7-10b – Verifikation des Storageformat-Upgrades

**Ergebnis:** PASS für den lokalen Windows-Arbeitsumfang.
**Geprüft:** 2026-10-02; der M7-Milestone bleibt bis zu seinen übrigen Aufgaben offen.

## Fachlicher Nachweis

`docs/M7-10b-storage-format-upgrade.md` spezifiziert Pointerbytes, Profilbindung, Restorepoint, Rechte, Journal und Wiederaufnahme. Implementiert ist der eng begrenzte Wechsel von `CURRENT` v1 zu v2; Domain-History, DatabaseId, Revision, Commit-Hash und Manifestgeneration/-digest bleiben unverändert. `StorageFormatUpgrade` ist eine explizite, separat überprüfte Capability (ADR-041). Ein echter Exact-Backup-/Clone-Restore mit unabhängigem Storage Verify ist Vorbedingung.

Die gezielte Suite `cargo test --locked -p worlddb-storage-file storage_upgrade::tests:: -- --nocapture` bestand mit **2 bestanden, 0 fehlgeschlagen**. Sie deckt den realen Restore, Berechtigungsentzug, geändertes Sourceinventar, beschädigte Prüfsumme, Torn-Tail-Reparatur, die gültigen Journal-/Pointerphasen, alle fünf Unterbrechungspunkte, unveränderte normative History, den v2-Genesis-Pointer und spätere Manifestpublikation im v2-Format ab.

## Workspace-Prüfungen

- `cargo test --workspace --locked` – PASS, einschließlich Rustdoc-Compile-Fail-Tests.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – PASS.
- `cargo check --locked --workspace --all-targets` – PASS.
- `cargo fmt --all -- --check` – PASS.
- `cargo xtask verify` – **38 PASS, 1 erwarteter SKIP, 0 FAIL**. Der SKIP ist M0-14 `ci-matrix`, da externe CI-Artefakte nötig sind.
- `python -B -X utf8 docs/contract/build_contract_sources.py --verify-only` und `python -B -X utf8 docs/contract/verify_contract_docs.py` – PASS.
- `python -B -X utf8 WorldDB_1.0_Sourcecheck.py` – PASS; alle geschützten Quellen bytegleich, Konsolidierungs-ZIP gültig.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py` – PASS; 243 Tasks, 11 Milestones, 253 Invarianten und 177 Folgebelegpaare strukturell gültig.
- `git diff --check HEAD` – PASS.

Es wurden keine Linux- oder macOS-Läufe behauptet. Diese plattformbezogenen Nachweise bleiben gemäß Projektvorgabe für die spätere Abnahme zurückgestellt. Der erfolgreiche lokale Verify-Lauf erteilt keine plattformübergreifende Durability-Freigabe.
