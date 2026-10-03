# M8-06 – CLI Export/Import contract

## Commands

```text
worlddb-cli v1 export logical <database> --output <new-artifact>
  --from <revision> --through <revision>
  --history-space <uuid>... --class <RecordKind>...

worlddb-cli v1 export share <database> --output <new-artifact>
  --from <revision> --through <revision>
  --history-space <uuid>... --class <RecordKind>...

worlddb-cli v1 import plan <destination> --input <logical-artifact>
  --output <new-plan> [--map <typed-identity>=<typed-identity>]...

worlddb-cli v1 import prepare <destination> --input <logical-artifact>
  --plan-file <canonical-plan>
```

The revision bounds are inclusive canonical decimal values. At least one explicit HistorySpace and one record class are required. Record classes use their closed PascalCase names from `RecordKind`, and `HistorySpaceDefinition` is required by the logical-export scope. Artifact and plan outputs must be new files outside the database being read. Success reports never contain filesystem paths.

## Logical and sharing export

Logical export checks the current host-bound principal's `DataExport` permission for the selected HistorySpaces and for project-wide classes. It reads a clean, pinned snapshot through `LogicalExportManager` and writes the canonical logical artifact unchanged. The artifact contains the full requested scope, visible HistorySpace ancestry, a row for every record class, and the explicit excluded storage classes. The CLI summary reports the selected HistorySpace IDs and record classes alongside safe manifest totals; the artifact itself retains the complete omission manifest.

Sharing export uses the same explicit scope and current policy. It additionally enforces class, field, relationship, and dependency visibility. A record that fails a field or relationship check is omitted as a whole. The existing sharing manager commits and verifies both required audit boundaries before the CLI writes the artifact. The sharing format intentionally omits source identity, snapshot identity, source totals, and omission counts; CLI output follows that rule and reports the requested HistorySpace IDs, record classes, revision bounds, and included-record count.

Both formats retain the storage layer's 512 MiB artifact and one-million-record bounds. Existing outputs are rejected instead of replaced.

## Import plan and preparation

`import plan` requires current `ProjectRead` and `DataImport` permission on the destination. It creates the existing canonical `LogicalImportPlan`, bound to the exact source artifact bytes and the destination DatabaseId. Mapping tokens have one of these forms:

```text
history-space:<uuid>
layer:<uuid>
perspective:<uuid>
timeline:<uuid>
entity:<uuid>
entity-type:<uuid>
predicate:<uuid>
event-kind:<uuid>
event-role:<uuid>
event-attribute:<uuid>
record:<RecordRef-wire-tag>:<uuid>
```

Each `--map` value contains one source token and one target token separated by `=`. IDs use canonical lowercase WorldDB UUID spelling. The core plan encoder sorts mappings and rejects cross-family mappings, duplicate sources or targets, unchanged mappings, and malformed record-reference tags.

`import prepare` checks current `ProjectRead` and `DataImport`, verifies the destination storage, reads its durable identity inventory, and calls `LogicalImportManager::prepare`. Preparation validates the exact artifact and plan binding, destination identity collisions, reference closure, and the explicit remaps. Its result includes the stable stream fingerprint, scope, and source omission totals.

Preparation is a validation boundary: it does not publish records or mutate the destination. The current M7-13 API returns a prepared stream and mapping resolver; a persistent database import transaction is outside this CLI task. Sharing artifacts are not accepted as logical-import input because they intentionally omit the source and omission metadata required for a remap plan.

## Output and failures

Machine output uses the existing versioned JSONL envelope and closed PublicCode/exit-code mapping. Logical export and import results include no input or output paths. Sharing results do not disclose omitted record counts. An output file is created exclusively and flushed before success is reported; an existing target is never overwritten.
