# M3-13a – Getrennter Admin-Raw-Port

**Status:** lokal implementiert und verifiziert 2026-09-30.

## Vertrag und Implementierung

- `release_admin_raw_page` ist ein separater Admin-Raw-Ausgabepfad. Die bestehende `full_scan_authorized_raw_history`-Abfrage bleibt auf `HistorySpaceRead` und `RawHistoryRead` pro sichtbarem Record gefiltert und verwendet `AdminRawRead` nicht.
- Der Admin-Pfad verlangt `HistorySpaceRead`, `RawHistoryRead` und `AdminRawRead` auf dem ausgewerteten Policystand. `AdminRawRead` wird zusätzlich immer gegen die aktuelle Policy geprüft, damit eine historische AuthorizationAtRevision keinen inzwischen entzogenen Adminzugriff erhält.
- Vor der Ausgabe übergibt der Port eine payloadfreie `AdminRawAuthorizeRequest` an den vorgeschalteten `AdminRawAuditAuthorizer`. Diese bindet Principal, opaken Scope-Fingerprint, Snapshot, aktuelle/ausgewertete SecurityEpoch und PageOrdinal. Auditfehler führen zu keiner Ausgabe.
- Ein privater, nur nach erfolgreicher Auditautorisierung erzeugter Release-Permit bindet die vollständige `QueryContextBinding`, Principal, Snapshot, beide Epochs und PageOrdinal. Die ausgegebene `OwnedQueryResult` bindet zusätzlich die Query-/Schema-/Securityachsen.
- `Ok(())` des M3-Audit-Authorize-Ports bedeutet erfolgreiche synchrone Audit-Autorisierung durch den Host. Der dauerhafte `RawReadAttempt`-Append-/Sync-Commitpoint vor jeder Seite bleibt M5-18 vorbehalten; M3 behauptet keine Persistenz.

## Sicherheitsnachweise

- `admin_raw::tests::admin_path_requires_both_raw_capabilities_and_does_not_call_audit_when_denied`: fehlendes `AdminRawRead` wird vor dem Audit-Aufruf abgewiesen.
- `admin_raw::tests::audit_failure_fails_closed_before_page_is_returned`: Auditfehler liefert keine Seite.
- `admin_raw::tests::successful_audit_authorization_is_bound_to_page_scope_context_and_epochs`: erfolgreiche Ausgabe ist an Snapshot und Kontext gebunden.
- Die vorhandenen Raw-History-Tests in `reference_query` prüfen die normale gefilterte Raw-Abfrage unabhängig vom Adminpfad.

## Validierung

- `cargo test --locked --workspace`: 298 Core-Tests und 75 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS (Git meldet bestehende LF/CRLF-Konvertierungshinweise).
