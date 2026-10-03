# M8-11a – Schema management contract

**Status:** implemented and verified on Windows
**Scope:** project-wide `EntityType`, `Predicate`, and `EventKind` definitions in the ODE-002 desktop host
**Normative source:** WorldDB vNext sections 2.4 and the schema, revision, authorization, and audit contracts

## Read contract

Schema reads return one immutable schema snapshot from the database's shared revision history. The host supports:

- `current`: the schema at the live database head;
- `historical`: the schema effective at a supplied recorded-as-of revision;
- `explicit`: the schema at an exact published schema revision.

The current mode follows data-only commits along the common revision axis. Historical and explicit reads do not fall back when their requested history is unavailable. Layer definitions remain visible as part of the full snapshot; their lifecycle and structural changes belong to Branch/Layer management.

## Definition contract

The schema editor creates stable IDs on the host for `EntityType`, `Predicate`, `EventKind`, event roles, and event attributes. User symbols follow the confirmed grammar `[a-z][a-z0-9_]*`. New definitions start `Active`.

Predicates carry subject and optional object type constraints, one closed value kind, cardinality, same-precedence resolution policy, an optional value-kind-specific constraint, and optional decimal display metadata. Event kinds carry typed roles and attributes, cardinality bounds, required-attribute flags, event-time form, optional maximum calendar span, and attribute constraints/decimal metadata.

Constraint values remain typed end to end. Signed/unsigned 128-bit values, durations, decimals, and time ticks cross the UI boundary as decimal strings; times also bind a timeline identity and unit. Empty or malformed bounds fail closed in the engine. Symbol sets use the same confirmed symbol grammar.

Structural edits to an existing identity are not made by this editor. They require the migration contract. Existing identities accept only forward lifecycle revisions:

```text
Active → Deprecated → Retired
```

Deprecated and Retired retain historical reads. Retired definitions cannot be referenced by active or deprecated schema definitions. If an EntityType has dependents, the dependents and EntityType must be retired in the same validated lifecycle batch. The UI stages that batch before publication.

## Authorization and publication

The native host binds each command to its authenticated project session and host-selected principal. The renderer supplies neither a project path nor a principal. `SchemaRead` is checked against current policy when the manager opens; `SchemaManage` is checked again immediately before publication.

Each create or lifecycle batch names its expected base revision. The manager checks both its loaded head and the live WAL head. A stale base is rejected without publication. A successful operation publishes schema history, carries forward the required policy-history reference on the shared revision axis, writes the Required Audit record (`SchemaManagement` / `SchemaDefinition`), and commits the manifest through one WAL transaction. A commit whose durable outcome cannot be proved is reported as unknown and is reconciled through recovery.

The desktop boundary exposes only the closed typed commands and readable definition views. It does not expose a raw record/JSON editor or canonical wire bytes. Renderer-facing failures use the stable `schema_rejected` code.

## Verification contract

Windows verification covers typed constraint conversion, request/response wire shapes, audited publication, historical retention, current authorization, dependency-safe retirement, and authenticated two-window IPC in both in-process and sidecar modes. Linux/macOS execution is intentionally deferred to M9-07 per the project instruction.
