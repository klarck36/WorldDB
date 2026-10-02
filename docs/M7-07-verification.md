# M7-07 – Storageformat-Migration vorbereiten

**Status:** lokal unter Windows geprüft. Es wird kein Storageformat-Upgrade ausgeführt. Ein veröffentlichtes N-1-Release liegt noch nicht vor; Linux-/macOS-Prüfungen bleiben gemäß Arbeitsplan zurückgestellt.

## Gelieferte Spezifikation

- `docs/M7-07-storage-format-upgrade-plan.md` trennt den Storageupgradeplan mit eigener Identität, Komponentenvektor, Fingerprints, deterministischen Steps und Budgets vom fachlichen Schema-Migrationsprotokoll.
- Die Spezifikation stellt fest, dass `FORMAT` nur die generische Frameversion samt Capabilities bindet und keine globale Datenbank-Formatversion darstellt. Das künftige Storageprofil muss deshalb die tatsächlichen Versionen aller Komponenten binden.
- Ein erfolgreicher Backup-Digest reicht nicht für `SafeRestorePoint`: gefordert sind ein gebundener ExactDatabaseBackup, vollständiger Verify, ein realer Restore in ein isoliertes Ziel, logischer Vergleich und ein auf Source, Target und Plan fingerprintgebundener Beleg.
- Normales Öffnen und Recovery führen keinen Upgrade aus. Der spätere explizite Plan-/Run-/Staging-/Verify-/atomare-Publish-/Resume-Pfad gehört M7-10b; M7-07 implementiert und committed diesen Pfad nicht.
- Da vor Alpha kein echtes N-1 existiert, pinnen drei versionierte `FORMAT`-Fixtures die aktuelle Erstbaseline, einen unbekannten zukünftigen Major und ein unbekanntes Required-Capability. Die Dokumentation klassifiziert v1.0 nicht fälschlich als bereits veröffentlichtes N-1. Ein echtes Alpha-Datenbankfixture wird bei M9-02 ergänzt.

## Nachweise

- `m7_07_format_upgrade_contract::initial_v1_format_fixture_opens_without_rewriting_the_probe`: unterstützte Baseline öffnet; Fixturebytes bleiben exakt erhalten.
- `m7_07_format_upgrade_contract::unsupported_future_format_is_rejected_without_rewriting_the_probe`: ein gültig gecheckter Major 2.0 wird ausdrücklich abgewiesen; die Source-Bytes bleiben unverändert.
- `m7_07_format_upgrade_contract::unsupported_required_capability_is_rejected_without_rewriting_the_probe`: unbekannte Required-Capability wird abgewiesen; die Source-Bytes bleiben unverändert.
- `cargo test --locked --workspace`: **PASS** (498 Core-Tests, 45 Storage-File-Unit-Tests, 3 neue M7-07-Contract-Tests, 84 Rustdoc-Tests; alle übrigen Workspace-Suites bestanden; ein bestehender manueller Langlauf blieb ignoriert).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` und `cargo check --locked --workspace --all-targets`: **PASS**.
- Plancheck, Sourcecheck und `git diff --check`: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.

## Grenzen

Es wurde kein Upgradeplan ausgeführt, kein Datenbestand verändert und kein Formatversionswechsel veröffentlicht. Erst M7-08/M7-10 liefern die Backup- und Restore-Nachweise; M7-10b baut darauf den ausführbaren, crash-resumierbaren Upgradepfad. N-1 bleibt bis zur ersten veröffentlichten Alpha ausdrücklich nicht anwendbar.
