# M8-05 – CLI migration contract

## Command surface

```text
v1 migration plan <database> --plan-file <canonical-MigrationPlan-record>
v1 migration dry-run <database> --plan-file <record> --step <step-id> [--record <record-frame>]...
v1 migration run <database> --plan-file <record> --run-id <run-id> --step <step-id> --operation-id <operation-id> [--record <record-frame>]... [--omit <record-index>|--replace <record-index> <record-frame>]... [--backup <exact-backup> --restore <new-clone> --confirm-breaking]
v1 migration resume <database> --plan-file <record> --run-id <run-id> --step <step-id> --operation-id <operation-id> [--record <record-frame>]... [--omit <record-index>|--replace <record-index> <record-frame>]... [--backup <exact-backup> --restore <new-clone> --confirm-breaking]
v1 storage upgrade <database> --backup <new-exact-backup> --restore <new-clone> --confirm
```

The plan file contains exactly one canonical `Record::MigrationPlan` frame. A supplied record file contains one canonical WorldDB record frame. Step groups follow plan order and must include every planned step; execution binds a stable operation ID to every step. Record, plan, and replacement files are bounded and charged against the plan's work and memory budgets. Option order is strict, and successful human/JSONL output reports identifiers, revisions, counts, and fingerprints without filesystem paths.

`plan` checks the source schema precondition and current `MigrationPlan` permission. `dry-run` validates the same step input files without writing to the source and reports transform estimates and unresolved decisions. `run` and `resume` require current `MigrationExecute` permission. Run validates the original source revision and schema fingerprint before acquiring its exclusive commit lock. Resume requires an existing Running journal with the same plan, inputs, decisions, run ID, and step operation IDs; it reconciles the committed prefix and rejects missing or completed journals before creating restore artifacts.

## Breaking migrations and persistent policy history

Breaking execution requires all of `--confirm-breaking`, an exact backup, and a verified clone restorepoint. Both target directories must be outside the source project and must not overlap. The retained exact backup is required for resume; resume creates a fresh clone from that backup to re-establish the restore proof after a partial commit. Without explicit confirmation, the CLI exits `InvalidRequest` before creating either artifact.

Each guarded migration step stores its migration commit identity and Required Audit record in the same WAL transaction. Where the project has a valid complete security-policy history, the commit also carries forward the unchanged policy snapshot, epoch, and audit-retention setting to the new data revision in that same manifest transaction. This keeps later current-policy opens and a resumed run valid without inventing a policy change. Pre-policy or legacy file-store fixtures without complete policy coverage retain their existing migration behavior; the policy-gated CLI refuses to open such a project.

## Storage format upgrade

The current CLI exposes the supported current-pointer V1-to-V2 upgrade. It requires current `StorageFormatUpgrade`, `BackupCreate`, and `BackupRestore` permission plus the explicit `--confirm` option. The manager prepares the plan, creates and verifies an exact backup and clone restorepoint, confirms the bound action, and executes the journaled upgrade. Output contains the plan/run IDs, source revision, target profile fingerprint, pointer digest, and whether the manager reconciled an interrupted operation. The CLI does not yet expose a storage-upgrade resume command; its explicit `resume` action applies to schema migrations.

## Windows evidence

`crates/worlddb-cli/tests/cli_contract.rs` exercises read-only plan and dry-run behavior, the no-confirmation gate with no backup or restore artifacts, a confirmed Breaking run with actual backup/clone creation, path-free output, policy-history continuity, and rejection of missing/completed resume journals before restore artifacts are created. The storage crate's guarded-migration crash tests cover process interruption and committed-prefix recovery. Linux/macOS execution remains deferred to M9-07 as requested.
