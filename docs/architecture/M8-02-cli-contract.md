# WorldDB CLI protocol 1.0

## Command grammar

The stable command namespace is `worlddb-cli v1 <command>`. Version 1 commands fail closed when an unknown command or protocol namespace is requested. `--help`/`-h`, `help`, and `version` are available at the root and within `v1`. The existing unversioned `adapter run` spelling remains a compatibility alias for `v1 adapter run`.

Global output selection precedes the command:

```text
worlddb-cli [--format human|jsonl] <command>
worlddb-cli --help
worlddb-cli --version
```

`human` is the default. `jsonl` emits exactly one machine-readable result envelope per invocation. Help documents syntax and meanings; command parsing never echoes unknown argument text, paths, process diagnostics, or operating-system error strings.

## Output envelopes

CLI command results use the closed versioned envelope below. The `outcome` is an adjacent-tagged union (`type` plus `data`); version 1.0 defines `help`, `version`, `adapter_run`, `verify`, `recovery_inspect`, `recovery`, `open_read_only`, `salvage`, and `error` outcomes. Storage outcomes and their read-only/write guarantees are specified in [the M8-03 storage CLI contract](M8-03-cli-storage-contract.md). Query commands use the query `ResponseEnvelope` from [the query transport contract](query_transport_contract.md), with the same protocol version and request ID rules.

```json
{"cli_protocol":{"major":1,"minor":0},"request_id":"00112233-4455-4677-8899-aabbccddeeff","outcome":{"type":"error","data":{"code":"InvalidRequest","retryable":false,"message_key":"InvalidRequest"}}}
```

Every JSONL response carries a canonical UUID request ID. If the operating-system entropy source is unavailable, the invocation fails with `Internal` and uses the nil UUID solely to preserve the response shape. Error codes and optional message keys come from the closed Public Code vocabulary in the query transport contract. Version 1 CLI errors set `retryable` to `false`; dynamic diagnostics are never included. In human mode successful output goes to stdout and errors go to stderr; in JSONL mode both successful outcomes and public errors go to stdout as one JSON object per line. stderr is reserved for a stream-write failure, which returns `Internal`.

The machine result for an adapter run includes only its status, negotiated adapter protocol version, and decimal-string output byte count. It excludes local paths, process IDs, and adapter diagnostics.

## Process exit statuses

| Exit status | Public code(s) |
| ---: | --- |
| 0 | Successful command |
| 2 | `InvalidRequest`, `InvalidQuery` |
| 3 | `UnsupportedProtocolVersion`, `UnsupportedOperation`, `UnsupportedQueryCapability` |
| 4 | `Unauthorized` |
| 5 | `NotFound` |
| 6 | `SnapshotExpired`, `CursorInvalidated` |
| 7 | `CorruptData` |
| 8 | `StorageRead` |
| 9 | `Cancelled` |
| 10 | `BudgetExceeded` |
| 70 | `Internal` or output-stream failure |

Parser failures map to `InvalidRequest`; unknown commands under `v1` map to `UnsupportedOperation`; unknown numeric `vN` namespaces map to `UnsupportedProtocolVersion`. Adapter input, framing, negotiation, budget, and process failures map through a closed table to the Public Code set. Their underlying causes and untrusted text do not cross the CLI boundary.

## Verification scope

`crates/worlddb-cli/tests/cli_contract.rs` verifies versioned help and version responses, stable exit statuses, public-code serialization, and secret canaries in human and JSONL stdout/stderr. Adapter process isolation and no-publication-on-adapter-failure remain covered by the M7-13a adapter contract tests. Query parity and query JSON precision remain governed by the query transport contract and later CLI query tasks.
