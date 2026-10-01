# M3-13 – Audit-Port und Policy

**Status:** DONE, lokaler In-Memory-Referenzpfad verifiziert 2026-09-30.

## Implementierung

- `RequiredAuditPort` akzeptiert ein vollständiges typisiertes Auditrecord oder scheitert ohne Record-Anhang.
- `InMemoryRequiredAuditPort` ist separat vom `BoundedDiagnostics`-Port, hat eine feste Kapazitätsgrenze und einen injizierbaren einmaligen Required-Auditfehler.
- `apply_required_policy_change` verlangt ein gültiges `SecurityPolicyChange`-Record, passende SecurityPolicy-Klasse, Actor, aktuellen Epoch und aktuellen Policy-Fingerprint sowie die aktuelle Capability `SecurityPolicyManage`. Erst nach erfolgreichem Audit-Anhang wird der nächste Policy-Snapshot mit inkrementiertem SecurityEpoch veröffentlicht.
- Required-Audit-Fehler und fehlende Berechtigung lassen Snapshot, Fingerprint und Epoch unverändert. Im In-Memory-Pfad ist die Policyzuweisung nach dem Audit-Anhang infallibel. Durable Backends müssen beides in M5 an denselben Commitpoint binden.
- `AuditRetentionPolicy` ist ein eigenes Konfigurationsmodell für Mindestaufbewahrungsdauer und Recordlimit. Die In-Memory-Auditqueue kann ausdrücklich daran gebunden werden; automatische Zeitablauf-/Löschdurchsetzung ist kein M3-Feature.
- `AuditAccessPermissions` wertet `AuditRead`, `AuditExport` und `AuditConfigure` unabhängig aus. Die Typen und Rechte bleiben von der fail-open Telemetrie getrennt.
- Auditrecords enthalten nur den typisierten Actor, Aktion/Objektklasse, Ergebnis, getrennten SecurityEpoch, unabhängige Audit-IDs/Sequence und opaken Policy-Fingerprint; Policypayload wird nicht kopiert.

## Sicherheitsnachweise

- `policy_audit::tests::required_audit_failure_blocks_policy_change_without_partial_effect`: simuliertes Required-Auditversagen hinterlässt keine Auditzeile und ändert weder Policyberechtigung, Fingerprint noch Epoch.
- `policy_audit::tests::policy_change_requires_security_policy_manage_before_audit_or_mutation`: nicht autorisierte Policyänderung wird vor Audit-Anhang und Mutation abgewiesen.
- `policy_audit::tests::accepted_required_audit_precedes_the_infallible_policy_publication`: bei Erfolg enthält der Auditdatensatz den alten Policykontext, und danach wechselt der Snapshot samt Epoch/Fingerprint.
- `policy_audit::tests::telemetry_failure_is_independent_of_required_audit_failure`: volle fail-open Telemetriequeue und fehlgeschlagenes fail-closed Required Audit sind getrennte Effekte.
- `policy_audit::tests::audit_retention_and_read_export_configure_rights_are_independent`: Retention ist getrennt parametriert; AuditRead impliziert weder AuditExport noch AuditConfigure.

## Validierung

- `cargo test --locked --workspace`: 295 Core-Tests und 75 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS (mit LF/CRLF-Konvertierungswarnungen von Git).

## Grenze

Der In-Memory-Port weist den fail-closed Policyaktionspfad nach, nicht die dauerhafte Atomizität. Recovery, Retention-Durchsetzung, persistente Zugriffsprüfungen und ein gemeinsamer WAL-Commitpoint bleiben M5/M8-Folgeaufgaben.
