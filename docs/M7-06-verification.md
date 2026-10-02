# M7-06 – Crash-Resume und Compensating Migration

**Status:** lokal abgeschlossen und unter Windows geprüft. Linux-/macOS-Nachweise bleiben gemäß Arbeitsplan bei M9-07.

## Gelieferter Vertrag

- Das Run-Journal bindet einen Lauf an `MigrationRunId`, Planfingerprint, Source-Schema-Precondition, Transformer-Version und die geordnete Step-Liste. Jeder Step speichert seine eigene `StepId`, stabile `OperationId`, den Fingerprint der exakten kanonischen Eingabe und die erwartete Zielrevision. Die getrennten Identitäten bleiben typisiert.
- Jeder Step durchläuft monotone Journalzustände `Pending → Prepared → Committed`; nach allen Steps wird der Lauf `Completed`. Versionierte, längenbegrenzte Binärkodierung mit BLAKE3-Prüfsumme verwirft beschädigte, abgeschnittene, übergroße oder unzulässig geordnete Daten.
- `MigrationRunJournalStore` ist ein Koordinationsport. Die File-Store-Implementierung speichert versionierte Sidecars unter `migration-runs/`, außerhalb der normativen Records und History. Schreiben erfolgt über eine synchronisierte Stagingdatei und atomare Ersetzung; Laden und Speichern prüfen die monotone Nachfolge.
- Start und Resume prüfen Plan, Source-Revision und -Fingerprint, Transformer-Version, eindeutige OperationIds, sämtliche kanonischen Eingaben und Gesamtbudgets, bevor ein weiterer Commit möglich ist. Ein abweichender Plan oder eine geänderte Eingabe wird fail-closed abgewiesen.
- Nach einem unklaren Commit-Ergebnis sucht Resume in der vollständigen normativen History nach der stabilen `OperationId` und validiert die zugehörige Lauf-, Step-, Eingabe- und Zielrevisionsidentität. Ein gefundener Commit wird mit dem Journal abgeglichen, ohne ihn erneut zu publizieren. Ein vorbereiteter Step ohne History-Marker kann nur am erwarteten Head wiederholt werden. Head-Drift, widersprüchliche oder verwaiste Marker sowie fehlende Marker für bereits als committed geführte Steps stoppen den Lauf.
- `query_migration_step_status` ermittelt den Status über die normative History und weist doppelte Operationsmarker zurück. Das Sidecar ersetzt oder erweitert die History nicht.
- Fachliche Kompensation ist ein eigener Plan mit neuer `MigrationId` und neuem Lauf. Sie wird nach dem bestehenden committed Prefix angehängt; der Executor verändert oder rollt frühere History nicht zurück.
- `MigrationStepCommitIdentity` erhält den optionalen kanonischen Eingabefingerprint in einem neuen Wire-Feld. Ältere Identitäten ohne dieses Feld bleiben dekodierbar; das feste Golden-Frame und Wire-Orakel wurden auf 51 implementierte Frames aktualisiert.

## Nachweise

- Drei Journal-Codec-/Übergangstests prüfen Roundtrip aller Zustände, Prüfsummen- und Reihenfolgefehler sowie monotone Übergänge und unveränderliche Eingaben.
- `migration_execution::tests::resume_reconciles_commit_when_journal_update_fails_after_publication` simuliert den unbekannten Ausgang nach normativem Commit und bestätigt die Wiederaufnahme ohne Doppelcommit.
- `migration_execution::tests::resume_rejects_changed_input_fingerprint_before_another_commit` sowie `migration_execution::tests::start_rejects_source_and_transformer_version_mismatch_before_journaling` prüfen fail-closed Eingabe-, Source- und Versionsbindung vor dem nächsten Commit bzw. vor Journalanlage.
- `migration_execution::tests::status_query_rejects_duplicate_operation_markers` lehnt mehrdeutige normative Marker ab. `migration_execution::tests::compensation_is_a_new_migration_appended_after_the_committed_prefix` belegt neue Migrationsidentität, zusätzlichen Run und unveränderten ursprünglichen Prefix.
- Zwei File-Store-Tests prüfen persistentes Wiederöffnen monotoner Sidecars sowie die Ablehnung einer Statusumkehr.
- `cargo test --locked --workspace`: **PASS** (497 Core-Unit-Tests; alle übrigen Workspace- und Rustdoc-Suites bestanden; ein vorhandener manueller Langlauf blieb ignoriert).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, Workspace-Check, Plancheck, Sourcecheck und `git diff --check`: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.

## Bewusst spätere Arbeit

Restrictive-/Breaking-Ausführung mit validiertem Commit-Gate bleibt M7-10a. Der getrennte Storageformat-Upgradeplan und dessen N-1-/Restorepoint-Nachweise gehören zu M7-07 und den folgenden M7-Tasks. Linux-/macOS-Prüfungen folgen bei M9-07.
