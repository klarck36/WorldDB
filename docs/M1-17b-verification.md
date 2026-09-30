# M1-17b verification: assertion and lifecycle record codecs

**Status:** DONE\
**Verified:** 2026-09-29 20:57 Mitteleuropäische Sommerzeit Europe/Berlin\
**Scope:** Assertion, Mask, ReplacementBoundary, ArchiveTransition, and their concrete closure/retraction records.\
**Wire format:** WorldDB frame 1.0, canonical TLV/scalar encodings from M1-16, and RecordRef tags from `policy/record-ref-wire-tags.tsv`.

## Stable record assignments

The ten frame kinds `0x1101` through `0x110a` and their closed top-level field maps are registered in `policy/record-wire-kinds.tsv` and implemented by `RecordKind`. All fields are ascending TLV tags. IDs are validated raw 16-byte UUIDs; revisions, enum values, and reference tags use minimal unsigned LEB128 unless the contained scalar format specifies otherwise. Unknown frame kinds, fields, selector codes, action codes, and RecordRef tags fail closed. Optional frame flags are preserved by the shared record wrapper.

| Kind | Record | Fields |
|---|---|---|
| `0x1101` | Assertion | `1:id; 2:context; 3:subject; 4:predicate; 5:value; 6:polarity; 7:validity; 8:created_revision` |
| `0x1102` | AssertionValidityClosure | `1:id; 2:assertion_id; 3:close_at_world_time; 4:created_revision` |
| `0x1103` | AssertionRetraction | `1:id; 2:assertion_id; 3:reason; 4:created_revision` |
| `0x1104` | Mask | `1:id; 2:context; 3:selector; 4:optional_validity; 5:created_revision` |
| `0x1105` | MaskValidityClosure | `1:id; 2:mask_id; 3:close_at_world_time; 4:created_revision` |
| `0x1106` | MaskRetraction | `1:id; 2:mask_id; 3:reason; 4:created_revision` |
| `0x1107` | ReplacementBoundary | `1:id; 2:context; 3:subject; 4:predicate; 5:optional_validity; 6:created_revision` |
| `0x1108` | ReplacementBoundaryValidityClosure | `1:id; 2:boundary_id; 3:close_at_world_time; 4:created_revision` |
| `0x1109` | ReplacementBoundaryRetraction | `1:id; 2:boundary_id; 3:reason; 4:created_revision` |
| `0x110a` | ArchiveTransition | `1:id; 2:archive_target_record_ref; 3:action; 4:created_revision` |

## Nested encodings

`ContextKey` is nested TLV: field 1 is HistorySpaceId, field 2 is the resolved LayerId, field 3 is scope (`1` World or `2` followed by PerspectiveId), and field 4 is EpistemicMode (`1` WorldState, `2` Knows, `3` Believes, `4` Claims). Context construction rejects invalid World/Perspective and epistemic-mode pairs.

`WorldTime` is nested TLV with field 1 TimelineId and field 2 canonical signed `Int` nanoseconds. Validity is nested TLV with field 1 TimelineId, field 2 optional start, and field 3 optional end; each present endpoint is an encoded WorldTime. Optional structured values use `00` for absent and `01` followed by the nested encoding for present. `TimeInterval` construction checks endpoint timelines and rejects reversed endpoints.

Polarity uses `1` Positive and `2` Negative. A Mask selector is a leading selector code followed by its body: `1` ExactAssertion plus AssertionId; `2` Proposition plus nested fields `1:subject; 2:predicate; 3:value; 4:polarity`; `3` Slot plus nested fields `1:subject; 2:predicate; 3:scope; 4:epistemic_mode`. The Slot constructor and Mask constructor verify valid scope/mode and context agreement. Golden records cover all selector variants, both polarities, bounded and absent optional validity, and both World and Perspective contexts.

An archive target is the minimal unsigned LEB128 RecordRef tag followed by its typed 16-byte ID. The archive-target codec accepts exactly the 23 target families with tags 1–23 and rejects tag 24 (`ArchiveTransition`) to prevent self-targeting. Action is `1` Archive or `2` Unarchive. The prior state is not duplicated in the frame; the decoder reconstructs the state-changing action, while history replay must validate that action against the target's prior operational state.

The public RecordRef scalar codec uses the same tag-plus-ID encoding for the ten currently implemented families: tags `1–3`, `10–15`, and `24`. Assigned Event/source/evidence/provenance lifecycle tags whose record codecs arrive in M1-17c/d return `DeferredRecordRefTag`; unassigned tags return `UnknownRecordRefTag`. All 24 registry entries are checked against the implementation and ledger.

## Validation boundary

Decoding validates frame integrity, field ordering, field presence, closed tags, scalar shape, canonical re-encoding, context pairs, validity interval shape, and local Mask slot/context consistency. The `from_wire_fields` lifecycle constructors restore persisted typed fields without inventing target records. Their cross-record checks—target existence, target revision order, target timeline, published predicate policy, archive state history, and other schema/history facts—belong to the history/import validation boundary, not a standalone frame decoder. This distinction is carried into the later history and import tasks. Decoder allocation and CPU budgets remain in M1-18.

## Evidence

- `crates/worlddb-core/tests/data/record-v1.0-golden.tsv` fixes full frame bytes and checksums for all 30 implemented records: 13 M1-17a records and 17 assertion/lifecycle fixtures.
- `crates/worlddb-core/tests/data/record-ref-v1.0-golden.tsv` fixes the ten current RecordRef scalar encodings.
- `wire_records::tests::all_implemented_records_match_fixed_golden_frames_and_round_trip` checks exact bytes, kind codes, decode, and canonical re-encoding.
- `wire_records::tests::assertion_mask_boundary_and_archive_record_refs_match_fixed_golden_vectors` checks the ten RecordRef vectors; the registry test checks every one of the 24 tags.
- `wire_records::tests::every_closed_archive_target_round_trips_and_archive_cannot_target_itself` round-trips all 23 allowed archive target types and rejects self-targeting and an unknown target code.
- Negative tests cover malformed/noncanonical RecordRefs, deferred and unknown tags, invalid IDs, unknown selector tags and nested fields, invalid polarity, missing record fields, and unknown frame kinds/fields.
- The independent frame and TLV oracle accepts all 30 record golden frames and requires production decode/re-encode to preserve each complete frame.

## Verification results

- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline --workspace --all-targets`: 127 passed (112 core unit tests, 4 wire-oracle integration tests, 7 testkit tests, and 4 xtask tests); 0 failed.
- `cargo test --locked --offline --doc --package worlddb-core`: 65 passed; 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo deny check all`: advisories, bans, licenses, and sources PASS.
- `cargo xtask verify`: 27 PASS, 1 visible expected `ci-matrix` SKIP required by deferred M0-14, 0 FAIL. The source and plan checks report 236 tasks, 11 milestones, 253 invariants, and 159 follow-up pairs.
- `git diff --check HEAD`: PASS; Git reported only its existing LF-to-CRLF working-copy notices.
