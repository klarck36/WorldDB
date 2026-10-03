# M8-04 – CLI Backup/Restore contract

## Command surface

The versioned CLI accepts:

```text
v1 backup create <source> --output <new-directory> --profile exact --audit-scope excluded
v1 backup create <source> --output <new-directory> --profile audit-complete --audit-scope included
v1 backup verify <backup-directory> --profile exact --audit-scope excluded
v1 backup verify <backup-directory> --profile audit-complete --audit-scope included
v1 restore clone <backup-directory> --output <new-database> --profile <profile> --audit-scope <scope>
```

The profile/scope pair is mandatory, positional order is strict, and mismatched pairs fail with `InvalidRequest` before opening a path. Every successful backup result repeats the profile and scope. It includes only the DatabaseId, revision, item count, optional audit watermark, authenticity state, and target-verify result; filesystem paths and MAC key identifiers are omitted.

`ExactDatabase` means `audit_scope=Excluded`. Verification accepts either supported profile when its requested profile and scope match the manifest. Backup creation requires the current `BackupCreate` capability for both profiles. Since the standalone CLI has no trusted host policy context, it rejects both create requests with `Unauthorized` before opening either path; it does not treat local file access as a policy grant.

## Trusted authorization boundary

AuditComplete creation needs a current host-authenticated policy view granting `BackupCreate`, `AuditRead`, and `AuditExport`. Clone restore needs current `BackupRestore`, and an AuditComplete clone additionally needs `AuditRead` and `AuditExport`. The standalone CLI has no trusted host identity provider or authenticated policy view. It therefore returns `Unauthorized` for every backup creation and clone restore before opening the source or creating the destination. It does not accept a user-selected principal, synthesize a system principal, or create a permissive policy.

The storage contract does not expose same-identity disaster recovery because it cannot guarantee exclusive control over the original. The CLI documents this limitation and does not advertise a destructive overwrite mode. Restore remains clone-only once trusted host authorization is integrated.

These policy-dependent CLI operations remain open for integration with the trusted host context from M8-09 and per-project Principal binding from M8-10. M8-04 depends on M8-10 and remains `PLANNED` until both are complete; the currently available profile verification is not counted as completion of the whole task. The task order is intentionally changed because accepting a caller-selected Principal, synthesizing one, or treating GitHub authentication as a WorldDB identity would violate the trusted-host boundary.

## Windows evidence scope

`crates/worlddb-cli/tests/cli_contract.rs` verifies Exact and AuditComplete profile verification, explicit scope reporting, fail-closed Exact/AuditComplete creation, scope mismatch rejection, and that unauthorized clone restore neither inspects an arbitrary backup path nor touches its destination. The JSONL results for both profiles parse successfully. Linux/macOS checks remain deferred to M9-07 as requested.
