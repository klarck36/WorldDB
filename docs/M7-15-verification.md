# M7-15 – Windows-Verifikation

**Stand:** 2026-10-03
**Host:** Windows
**Ergebnis:** bestanden; Linux/macOS-Nachweise folgen gemäß Projektentscheidung in M9-07.

## Automatisierte Nachweise

- `m7_15_offline_purge_contract::offline_rewrite_publishes_a_new_verified_database_with_required_audit_and_report`
  erstellt eine Dateidatenbank mit mehreren Revisionen, genehmigt die vollständige Cascade und
  prüft eine neue `DatabaseId`, die entfernten IDs, erhaltene Historie, das `PurgeReport`, die
  indexfreien Rebuild-Hinweise sowie den Required Audit Record. Anschließend werden Quellrevision
  und kanonischer Quell-Export mit dem Stand vor dem Rewrite verglichen.
- `m7_15_offline_purge_contract::purge_rewrite_fails_closed_without_approval_or_current_purge_permission`
  belegt, dass fehlende Cascade-Freigabe und fehlende aktuelle `Purge`-Berechtigung kein Ziel
  anlegen.
- `purge_rewrite::tests::publication_faults_never_expose_an_unaudited_database` injiziert einen
  Fehler vor dem Auditcommit und vor der Zielveröffentlichung; in beiden Fällen bleibt das
  Zielverzeichnis aus. Ein Fehler nach dem atomaren Rename wird als `PublishedOutcomeUnknown`
  gemeldet; das sichtbare Ziel besteht Storage Verify und enthält `PurgePublication` im Required
  Audit.
- Der Integrationstest prüft `secure_erase_claimed() == false`; der kanonisch kodierte
  `PurgeReport` schreibt außerdem ein festes Null-Bit für den Secure-Erase-Anspruch.

## Prüfläufe

- `cargo test --locked --workspace --quiet` – bestanden: 501 Core-Tests, 72 Storage-Unit-Tests,
  Integrations- und Dokumentationstests sowie 84 Rustdoc-Tests bestanden; registrierte lange
  Windows-NTFS- und Performancekampagnen blieben erwartungsgemäß ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – bestanden.
- `cargo fmt --all -- --check` – bestanden.
- `cargo check --locked --workspace --all-targets` – bestanden.
- `python -X utf8 WorldDB_1.0_Plancheck.py` – bestanden: 243 Tasks, 11 Milestones, 253 Invarianten,
  177 Folgebeleg-Paare.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py` – bestanden; alle geprüften Quellspiegel und das
  Konsolidierungs-ZIP stimmen bytegenau.
- `cargo xtask verify` – 38 PASS, 1 erwarteter `M0-14 ci-matrix`-SKIP, 0 FAIL. Der Lauf enthält
  Format-, Workspace-, Clippy-, Vertrags-, Quellen-, Plan- und Windows-Storageprüfungen.
- `git diff --check HEAD` – bestanden.

`M7-16` ist als nächste Task freigegeben. Dort folgt die breitere Prozessabbruch-/Kompatibilitäts-
Runde; dieses M7-15-Faultset deckt gezielt die Purge-Audit- und Verzeichnisveröffentlichungsgrenze
ab.
