# M8-04 – CLI Backup/Restore contract

## Command surface

The versioned CLI accepts:

```text
v1 backup create <source> --output <new-directory> --profile exact --audit-scope excluded
v1 backup create <source> --output <new-directory> --profile audit-complete --audit-scope included
v1 backup verify <backup-directory> --profile exact --audit-scope excluded
v1 backup verify <backup-directory> --profile audit-complete --audit-scope included
v1 restore clone <backup-directory> --authorize-with <current-project> --output <new-database> --profile <profile> --audit-scope <scope>
```

The profile and matching audit scope are mandatory, positional option order is strict, and a mismatch returns `InvalidRequest` before any path is opened. `ExactDatabase` means `audit_scope=Excluded`. `AuditComplete` includes the supported raw-read audit prefix. Verification checks either profile independently of project authorization and reports its profile, audit scope, DatabaseId, revision, item count, authenticity state, target verification, and audit watermark when present.

Successful create and restore output omits local paths and MAC key identifiers. Restore output distinguishes the source DatabaseId/revision from the clone DatabaseId/revision. Backup creation and clone restore do not modify the source project.

## Host and current-policy authorization

The CLI obtains the current Windows account SID from the process token and derives the WorldDB Principal using the shared domain-separated host-account mapping. It accepts no caller-supplied Principal and does not use GitHub identity. On platforms where this Windows identity provider is unavailable, these policy-gated commands fail closed.

Exact backup creation requires current `ProjectRead` and `BackupCreate`. AuditComplete creation additionally requires `AuditRead` and `AuditExport`. The storage manager acquires the source project's writer lock, verifies the source, loads the current policy from its manifest security segments, checks those capabilities, and captures the policy-authorized audit prefix before it touches the output target.

Restore requires `--authorize-with <current-project>`. The CLI keeps a shared read-only lock on that clean project, selects the current policy for the process Principal, and requires `ProjectRead` and `BackupRestore`; AuditComplete restore also requires `AuditRead` and `AuditExport`. The backup's DatabaseId must match the authorization project's DatabaseId. Restore is clone-only, verifies the backup before publishing, gives the clone a new DatabaseId, and rejects a destination that overlaps the authorization project. Existing destinations are never replaced.

Same-identity disaster recovery is not exposed because the storage contract cannot guarantee exclusive control over the original database during an in-place recovery.

## Windows evidence scope

`crates/worlddb-cli/tests/cli_contract.rs` covers unauthenticated/insufficient policy denial before source or backup access, scope mismatch, profile verification, and authorized end-to-end Exact and AuditComplete creation plus clone restore. It verifies that audit watermark and profile are reported, paths are omitted from machine output, and a restore destination within the source project is rejected. Linux/macOS execution remains deferred to M9-07 as requested.
