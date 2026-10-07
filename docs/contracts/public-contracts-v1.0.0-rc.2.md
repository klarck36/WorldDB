# WorldDB public contracts – 1.0.0-rc.2

**Status:** Frozen RC baseline
**Snapshot:** `contracts/public/v1.0.0-rc.2/manifest.json`
**Machine check:** `python -B -X utf8 tools/check_public_contracts.py`

## Relationship to rc.1

The immutable `1.0.0-rc.1` snapshot remains at
`contracts/public/v1.0.0-rc.1/manifest.json`. This second snapshot is required
by the existing conservative rule for changed implementation-source
fingerprints. The changes that triggered it were additions to test modules and
test-only process-crash coordination in files that rc.1 fingerprints; the
structured contract values extracted by the verifier remain unchanged. The
versioned snapshot therefore records the reviewed source state without
rewriting rc.1 or claiming a change to the API protocol, wire registries,
public error mappings, persistent format, or Logical Export v2 semantics.

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
conservatively `BREAKING`. A future RC change must use another versioned
baseline rather than overwrite this snapshot.

## Scope

This is a contract snapshot, not a cross-platform release approval. Native
macOS/APFS and Linux/ext4 evidence and the final RC gate remain assigned to
M9-07 and M9-14.
