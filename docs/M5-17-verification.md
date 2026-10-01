# M5-17 – Atomisches Required Audit für Mutationen

**Ergebnis:** DONE – Windows-lokal geprüft am 1. Oktober 2026. Plattformprüfungen für Linux/macOS und die Crashmatrix M5-22 bleiben zurückgestellt.

## Umsetzung

`WalPrepareLog::commit_required_audit` nimmt ein nichtleeres kanonisches Aktionspayload und einen typisierten `AuditRecord` entgegen. Beide Bytes liegen gemeinsam in einem Required-Audit-WAL-Envelope innerhalb desselben Prepareframes. Der vorhandene `commit_operation`-Pfad bindet diese Payloadbytes an OperationId, Revision und Commitmarker und liefert den Receipt erst nach dem zweiten WAL-Sync. `commit_audited_manifest_snapshot` verbindet denselben Mechanismus mit dem vollständigen Manifestinventar und seinen vorbereiteten Segmenten.

Vor dem Prepare muss der Auditrecord `Succeeded` melden und `Committed { revision, operation_id }` exakt an die vom WAL zugeteilte Revision und die verwendete OperationId binden. Er muss außerdem eine streng steigende `AuditSequence` tragen. Eine Ablehnung hinterlässt keinen committed WAL-Vorgang und veröffentlicht weder Manifest noch Policyprojektion. Idempotente Wiederholungen vergleichen die ursprünglichen kanonischen Bytes und liefern denselben Receipt, auch nachdem Recovery Stagingdateien publiziert hat.

`committed_required_audit_records` liest Records nur aus einem vollständig verifizierten Commitpräfix. Recovery decodiert alle Required-Audit-Envelope im WAL, prüft deren Bindung und Sequenzfolge und hält bei semantisch ungültigen Records die Datenbank read-only. Eine gültige Policyänderung bleibt als SecurityPolicy-Segment samt `SecurityPolicyRecord` erhalten; eine Audit-Aufbewahrungsänderung bleibt als versionsgebundene Policyhistorie erhalten. Beide Publikationen tragen ihren Required Audit Record im gemeinsamen WAL-Commit.

## Verträge

- `generic_required_audit_commits_with_action_and_rejects_bad_binding_or_sequence` – Action und Auditrecord teilen Revision, OperationId und Commitbeleg; idempotenter Retry liefert denselben Receipt; falsche Bindung und wiederholte Sequenz veröffentlichen nichts.
- `policy_and_audit_configuration_changes_recover_with_their_required_audits` – eine echte Principalregistrierung erhöht die SecurityEpoch, eine spätere Aufbewahrungsänderung bleibt an ihrer Revision; Recovery stellt beide Historien und die zwei zugehörigen Records wieder her.
- `wrong_required_audit_binding_does_not_publish_staged_policy_action` – eine abweichende Audit-Revision lässt WAL-Head und `CURRENT` unverändert.
- `checksum_valid_but_mismatched_audit_record_quarantines_the_committed_prefix` – bei strukturell gültigem WAL, aber semantisch falscher OperationId wird Recovery read-only; künftige WAL-Mutationen werden abgewiesen.

## Windows-Nachweise

- `cargo test --locked -p worlddb-storage-file --all-targets`: 84 bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace --quiet`: 584 bestanden, 2 ignoriert, 0 fehlgeschlagen.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-CI-Matrix-SKIP, 0 FAIL; Plancheck und Sourcecheck PASS.
- M0-13-Testkit-Nachweis `M0-13-20261001T182841Z-d3f55cc17b`: PASS.
- `git diff --check HEAD`: PASS; lokale Git-Prüfung, GitHub wurde nicht verwendet.

Diese Abnahme belegt die Windows-Ausführung. Sie behauptet weder Linux/macOS-Kompatibilität noch Machine-/Power-Loss-Durability; dafür bleiben Plattformprofile und M5-22 erforderlich. GitHub wurde nicht verwendet.
