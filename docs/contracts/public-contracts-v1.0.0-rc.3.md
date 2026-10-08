# WorldDB public contracts – 1.0.0-rc.3

**Status:** Frozen RC baseline
**Snapshot:** `contracts/public/v1.0.0-rc.3/manifest.json`
**Machine check:** `python -B -X utf8 tools/check_public_contracts.py`

## Relationship to earlier snapshots

The immutable `1.0.0-rc.1` and `1.0.0-rc.2` snapshots remain unchanged. This
third snapshot records the reviewed source state after the macOS durability
path began using `F_FULLFSYNC` for regular-file publication, as selected by
ODE-006. The conservative verifier fingerprints the full persistent-format
implementation source, so this reviewed implementation change requires a new
versioned baseline.

The structured contract values remain unchanged: API protocol, wire tags,
public error mappings, persistent-format versions and magics, and Logical
Export v2 semantics match rc.2. This snapshot does not claim that native
macOS/APFS or Linux/ext4 release gates have passed.

## Frozen surfaces

| Surface | Frozen version or source | Classification boundary |
| --- | --- | --- |
| Rust application API | Numbered `worlddb_core::api::v1`; protocol `1.0` | Removing or changing a public item is breaking. A reviewed compatible extension requires a minor protocol increase. |
| Scalar, record, reference, and audit wire tags | Current assignments in the policy registries and `ValueTag` | These grammars are closed. Any addition, removal, renumbering, or payload-field change is breaking for the frozen 1.0 wire contract; values are never reused. |
| Public error codes | The 14 closed API transport codes, core `WDB-*` identifiers, and core-to-API mapping | Adding a stable error code is additive with a protocol-minor increase. Removal, renaming, or remapping an existing code is breaking. |
| Persistent storage format | Frame, segment, security-segment and manifest/current-pointer versions and magics | A major or magic change is breaking. A compatible minor extension is additive only when existing closed records and required capabilities retain their meaning. |
| Logical Export | Version 2; semantics in `docs/contracts/logical-export-v2.md` | Changing v2 semantics is breaking. A new export version is additive only while v2 remains readable and supported. |

The verifier compares the live tree with this immutable snapshot. Structured
contract changes and unclassified source-fingerprint changes follow
`contracts/public/classification-rules.tsv`; source-fingerprint changes are
conservatively `BREAKING`. Later RC changes must use another versioned baseline
rather than overwrite rc.1, rc.2, or rc.3.

## Scope

This is a contract snapshot, not a cross-platform release approval. Native
macOS/APFS and Linux/ext4 evidence and the final RC gate remain assigned to
M9-07 and M9-14.
