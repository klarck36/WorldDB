# Logical Export v2 – normative semantics

This document freezes the 1.0 RC meaning of `LogicalExport` format v2. It is
part of the public-contract snapshot at
`contracts/public/v1.0.0-rc.1/manifest.json`.

## Scope and contents

- An export binds the source `DatabaseId`, a pinned `snapshot_revision`, an
  inclusive Transaction-Time range, explicit nonempty HistorySpace selection,
  and explicit nonempty record-kind selection.
- Scope lists are unique and canonically sorted. `HistorySpaceDefinition` is
  required; revisionless migration metadata cannot be selected.
- The manifest records visible HistorySpaces and ancestry cutoffs, every
  registered record kind including zero-count classes, selection flags, and
  the complete omission list. Ancestry definitions needed to interpret a
  selected HistorySpace are marked `HistorySpaceDependency`.
- Records are canonically ordered by revision, record kind, then encoded
  record frame. The scope and omissions round-trip byte-for-byte through the
  canonical decoder/encoder.
- Current `DataExport` authorization applies to the selected scope and each
  included record. Historical grants do not override current denials.

## Explicit omissions

Logical Export is not an exact backup. Every v2 manifest names all five
excluded storage classes and gives no source counts for omitted records:

1. Security-policy history and policy snapshots.
2. Raw-read audit history.
3. Transaction and recovery state, including WAL and writer state.
4. Physical manifests, `CURRENT`, and storage-format profile.
5. Rebuildable index generations and pointers.

Sharing Export remains a separate rights-filtered artifact and must not be
presented as an exact backup or as Logical Export.

## Canonical envelope

- Eight-byte magic: `WDBLEX\0\x02`.
- Header: magic followed by the payload byte length as an unsigned 64-bit
  little-endian integer.
- Payload: the complete manifest, record count, and ordered inclusion-tagged
  record frames.
- Trailer: 32-byte BLAKE3 digest over header and payload, domain-separated by
  `WorldDB.LogicalExport.v2\0`.
- Decode verifies digest, bounds, scope, record closure, omissions, canonical
  order, and exact re-encoding. A digest is an integrity check, not a signature
  or a statement of source authenticity.
- Limits: 512 MiB per artifact, 1,000,000 records, and 65,536 HistorySpaces.

Any change to the meaning of this v2 profile, including its scope, manifest,
ordering, omission, authorization, or digest rules, is a breaking contract
change. A new independently readable export version may be additive while v2
remains supported.
