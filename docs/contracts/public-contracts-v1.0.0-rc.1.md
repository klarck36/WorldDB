# WorldDB public contracts – 1.0.0-rc.1

**Status:** Frozen RC baseline  
**Snapshot:** `contracts/public/v1.0.0-rc.1/manifest.json`  
**Machine check:** `python -B -X utf8 tools/check_public_contracts.py`

## Frozen surfaces

| Surface | Frozen version or source | Classification boundary |
| --- | --- | --- |
| Rust application API | Numbered `worlddb_core::api::v1`; protocol `1.0` | Removing or changing a public item is breaking. A reviewed compatible extension requires a minor protocol increase. The API source fingerprint is pinned for RC review. |
| Scalar, record, reference, and audit wire tags | Current assignments in `policy/record-wire-kinds.tsv`, `policy/record-ref-wire-tags.tsv`, `policy/audit-wire-kinds.tsv`, `policy/audit-wire-values.tsv`, and `ValueTag` | These grammars are closed. Any addition, removal, renumbering, or payload-field change is breaking for the frozen 1.0 wire contract; values are never reused. |
| Public error codes | The 14 closed API transport codes, core `WDB-*` identifiers, and core-to-API mapping | Adding a stable error code is additive with a protocol-minor increase. Removal, renaming, or remapping an existing code is breaking. |
| Persistent storage format | Frame major/minor, segment and security-segment major/minor, and manifest/current-pointer magics | A major or magic change is breaking. A compatible minor extension is additive only when existing closed records and required capabilities retain their meaning. |
| Logical Export | Version 2; semantics in `docs/contracts/logical-export-v2.md` | Changing v2 semantics is breaking. A new export version is additive only while v2 remains readable and supported. |

The Rust API is intentionally narrower than the crate's compatibility exports:
only numbered `api` modules are supported application boundaries. The facade
does not expose Storage traits, locks, filesystem paths, authenticated
Principals, Capability decisions, or secret-bearing types.

## Machine-visible diff policy

The baseline manifest stores the exact versions, closed registries, error-code
sets and mappings, format identifiers, semantic-document digest, and
fingerprints of the source files that implement those contracts. The verifier
compares the live tree with this frozen snapshot and prints every detected
change with a `BREAKING` or `ADDITIVE` classification. A changed implementation
fingerprint with no more specific structured diff is conservatively classified
as `BREAKING` pending review. The RC verification passes only when there are no
unapproved contract differences.

The classification rules are versioned in
`contracts/public/classification-rules.tsv`; synthetic tests exercise each
classification. The current RC snapshot has zero post-freeze differences.

## Scope of this freeze

This is the public-contract baseline for the 1.0 RC. It does not claim a
cross-platform release approval, N-1 compatibility result, or final RC gate.
Those checks remain assigned to M9-02 and M9-14.
