# M7-12 – Verifikationsnachweis

## Umfang

M7-12 ergänzt `SharingExportScope`, `SharingExport`, `SharingExportManager` und ein separates kanonisches Wire-Format. Record-, Feld- und Beziehungsrechte werden fail-closed geprüft. Das Artefakt enthält nur explizit ausgewählte Scope-Koordinaten und autorisierte Records; Quell-DatabaseId, Snapshot-Revision, Auslassmanifest und Quellzählungen fehlen.

Authorization und Completion werden jeweils zusammen mit einem unveränderten Manifest-Snapshot und einem `Required Audit Record` atomar committed. Recovery, StorageVerifier und Required-Audit-Receipt werden vor jeder Grenze erneut geprüft. Die Bytes werden erst nach erfolgreich verifiziertem Completion zurückgegeben.

## Gezielte Fälle

- `sharing_export::tests::sharing_export_roundtrips_and_commits_both_audit_boundaries`: kanonischer Roundtrip, keine Quell-DatabaseId, beide Auditgrenzen mit geteilter AuditOperationId.
- `sharing_export::tests::denied_source_field_is_excluded_without_an_omission_count`: expliziter Feld-Deny unterdrückt den ganzen Source-Record und den vertraulichen Locator.
- `sharing_export::tests::failure_after_authorization_never_commits_completion_or_returns_artifact`: Fehler nach Authorization gibt kein Artefakt zurück und erzeugt keinen Completion-Record.
- `sharing_export::tests::process_crash_after_authorization_commit_recovers_only_that_audit_boundary`: Prozessabbruch direkt nach dem Authorization-WAL-Commit; Neustart stellt genau diesen Required-Audit-Record und einen sauberen Manifeststand wieder her.
- `sharing_export::tests::exact_field_grants_work_without_project_wide_field_read`: explizite Freigaben einzelner Felder genügen; pauschales `FieldRead` auf Projektebene wird nicht vorausgesetzt.

## Windows-Prüfergebnisse

- `cargo test --workspace --locked --quiet`: PASS; 501 Core-Tests, 64 Storage-File-Unit-Tests und 84 Rustdoc-Tests bestanden; ein ausdrücklich ignorierter Storage-Test und die markierten Langläufe blieben übersprungen.
- `cargo clippy --workspace --locked --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo check --workspace --locked --all-targets`: PASS.
- `WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, 11 Milestones, 253 Invarianten und 177 Folgebeleg-Paare.
- `WorldDB_1.0_Sourcecheck.py`: PASS; alle normalisierten Quelldateien stimmen bytegenau mit den geprüften Quellen überein.
- `cargo xtask verify`: 38 PASS, 1 erwarteter `M0-14 ci-matrix`-SKIP, 0 FAIL.
- `git diff --check HEAD`: PASS.

Linux/macOS sind nicht Teil dieses Laufs und bleiben auftragsgemäß für M9-07 zurückgestellt.
