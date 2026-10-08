# M9-03 – Versions, capabilities, and safe return paths

**Status:** DONE
**Review:** 6 October 2026, Windows

## Version boundaries

| Surface | Current value and check |
| --- | --- |
| Public contract | `1.0.0-rc.1`, frozen by M9-01. |
| Rust/CLI protocol | `worlddb_core::api::v1` protocol `1.0`; handshake chooses the highest common version and rejects missing required features. CLI output reports its package version separately from `cli_protocol`; an unknown CLI protocol gets a distinct public error. |
| CLI package | Workspace version `0.1.0`; `worlddb-cli version` reports `CARGO_PKG_VERSION` plus protocol `1.0`. |
| Desktop app | The current ODE-002 artifact is explicitly a spike: Tauri app version `0.0.1`, internal shell crate `0.0.0`, product name contains “Spike”, and bundling is disabled. It is not a released application version. M9-10 must assign one explicit release version across CLI and desktop packages and bind it to the RC contract before packaging. |
| Storage | There is no single global database version. The format profile is the component vector: Frame `1.0`, Segment `1.0`, Security Segment `1.0`, plus the Manifest, CURRENT, WAL, audit, and index identifiers. M9-01 pins the public component baseline. |
| Schema | Schema history uses explicit `SchemaRevision` and fingerprints. Every migration plan binds exact source revision/fingerprint, target schema, migration category, and deterministic transformer version. |

`FORMAT` currently declares no required or optional capabilities. Unknown required
capabilities fail closed; unknown optional capability bits are retained
without interpretation. A capability is not inferred from a package version.

## Upgrade and downgrade rules

- Opening, read-only verification, and recovery do not start schema or format
  upgrades. Schema migration requires an explicit plan and run. A Breaking
  migration additionally requires current administrator authorization and an
  ExactDatabaseBackup restored and independently verified as a clone.
- A plan is rejected before journaling when its source schema revision,
  fingerprint, or transformer version differs. Resume applies the same checks.
  The transformer version is part of the canonical plan fingerprint.
- The only implemented storage-format conversion is the explicit `CURRENT`
  pointer v1-to-v2 upgrade. It requires `StorageFormatUpgrade` authorization
  and a verified restore point. Other files, database identity, revision, and
  manifest binding remain unchanged. Interrupted runs reopen at a fully
  verified source or target; they do not expose a partial pointer.
- Unknown format majors and unknown required capabilities are refused without
  rewriting the source. No in-place application or storage downgrade is
  promised. To return after a completed upgrade, restore a verified Exact
  backup into a new clone. To reverse a schema change, use a new explicit
  compensating migration or restore a verified clone; committed history is
  never erased or rewound.
- There is no published Alpha yet. Compatibility for released application
  downgrades is therefore not claimed. A future app version may open a
  database only when it recognizes that database's format profile, schema
  state, and required capabilities; otherwise it must refuse safely.

The earlier M7-10a note that guarded file-store migrations lacked a persistent
adapter was corrected: M7-16c delivered the WAL-backed `FileMigrationCommitBackend`
and M7-16d verified its production file-store binding.

## Verification

- API handshake and required-feature rejection: 1 targeted test passed.
- Schema source/transformer mismatch before journal creation: 1 targeted test
  passed.
- Future format major, unknown required capability, and unchanged supported
  v1 open: 3 M7-07 contract tests passed.
- Explicit storage upgrade/reopen and recognized-profile genesis: 2 targeted
  tests passed.
- CLI version/protocol, unknown-version refusal, read-only open, migration and
  command contracts: 17 CLI contract tests passed.
- Breaking migration restore-proof contract: 1 targeted test passed.

The project-wide Verify run is recorded in the M9-03 task register. Native
macOS/Linux checks remain assigned to M9-07; the desktop release-version check
and package smoke remain assigned to M9-10.
