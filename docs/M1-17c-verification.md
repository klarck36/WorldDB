# M1-17c verification: Event record codecs

**Status:** DONE\
**Verified:** 2026-09-29 21:19 Mitteleuropäische Sommerzeit Europe/Berlin\
**Scope:** Event, EventMask, EventRelation, and their span-closure and retraction records.\
**Wire format:** WorldDB frame 1.0, canonical TLV/scalar encodings from M1-16, and RecordRef tags from `policy/record-ref-wire-tags.tsv`.

## Stable record assignments

The seven frame kinds `0x1201` through `0x1207` and their closed field maps are registered in `policy/record-wire-kinds.tsv` and implemented by `RecordKind`.

| Kind | Record | Fields |
|---|---|---|
| `0x1201` | Event | `1:id; 2:history_space_id; 3:layer_id; 4:event_kind_id; 5:participants_array; 6:attributes_array; 7:event_time; 8:created_revision` |
| `0x1202` | EventMask | `1:id; 2:history_space_id; 3:layer_id; 4:target_event_id; 5:created_revision` |
| `0x1203` | EventSpanClosure | `1:id; 2:event_id; 3:close_at_event_time; 4:created_revision` |
| `0x1204` | EventRetraction | `1:id; 2:event_id; 3:reason; 4:created_revision` |
| `0x1205` | EventMaskRetraction | `1:id; 2:event_mask_id; 3:reason; 4:created_revision` |
| `0x1206` | EventRelation | `1:id; 2:from_event_id; 3:to_event_id; 4:canonical_relation_kind; 5:created_revision` |
| `0x1207` | EventRelationRetraction | `1:id; 2:event_relation_id; 3:reason; 4:created_revision` |

Every layer-capable Event and EventMask persists an explicit `LayerId`. Participant and attribute arrays encode a minimal item count followed by minimal item lengths. Each participant item is nested TLV `1:event_role_id; 2:entity_id`; participant pairs must be strictly ordered by `(role_id, entity_id)`. Each attribute item is `1:event_attribute_id; 2:value`; attributes must be strictly ordered by EventAttributeId. Duplicate or descending entries fail as noncanonical.

## EventTime and relation codes

EventTime uses a leading closed code: `1` Instant followed by a nested WorldTime; `2` Span followed by nested TLV `1:start_world_time; 2:optional_end_world_time`. WorldTime is nested TLV `1:timeline_id; 2:canonical_signed_nanoseconds`. A missing span end encodes an open span. Closed-span construction enforces matching timelines and `start < end`. Golden vectors cover Instant, closed Span, open Span, and a separate EventSpanClosure record.

Persisted EventRelation kind codes are `1` Before, `2` SameTime, and `3` Causes. `After` has no wire code: the domain constructor normalizes it to the inverse Before edge before encoding. SameTime sorts its EventId endpoints; decoding canonicalizes the pair and the frame roundtrip check rejects reversed wire order. Self-relations and unrecognized relation codes fail closed.

The public RecordRef scalar codec now supports 17 families with fixed vectors: tags `1–6`, `10–19`, and `24`. Source, Evidence, Provenance, and their lifecycle tags (`7–9`, `20–23`) remain explicitly deferred to M1-17d. The complete 24-tag implementation-to-ledger comparison still runs. Archive targets continue to roundtrip exactly the 23 allowed types and reject ArchiveTransition self-targeting.

## Validation boundary

Decoding validates frame integrity, closed fields, typed IDs and Values, nested EventTime shape, canonical participant/attribute order, self-relation rejection, and canonical relation encoding. It preserves all local record fields without requiring the referenced EventKind or target Events to be present. Role membership/cardinality, required attributes and value kinds, schema publication, target existence, retraction order, and graph constraints need project/history context and are checked at the later import or transaction validation boundary. M1-18 remains responsible for decoder allocation and CPU budgets.

## Evidence

- `crates/worlddb-core/tests/data/record-v1.0-golden.tsv` fixes 42 full frames with checksums across all 30 record kinds implemented through M1-17c.
- `crates/worlddb-core/tests/data/record-ref-v1.0-golden.tsv` fixes the 17 RecordRef scalar encodings currently supported by M1-17a/b/c.
- `wire_records::tests::all_implemented_records_match_fixed_golden_frames_and_round_trip` checks exact bytes and canonical decode/re-encode for every frame fixture; the independent frame/TLV oracle checks all 42 vectors.
- `wire_records::tests::domain_record_refs_match_fixed_golden_vectors` covers all 17 current encodings; `runtime_record_ref_tags_match_the_complete_policy_registry` checks all 24 assigned tags.
- `wire_records::tests::event_relations_normalize_after_canonicalize_same_time_and_reject_after_on_wire` proves After normalization, SameTime endpoint order, and rejection of After/self-edge wire forms.
- `wire_records::tests::event_participant_and_attribute_arrays_reject_noncanonical_order` and `event_time_decoder_rejects_unknown_forms_and_invalid_closed_spans` cover malformed local Event payloads.
- Existing event-domain tests cover role/cardinality, required attributes, EventKind time forms, and lifecycle constructors.

## Verification results

- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline --workspace --all-targets`: 130 passed (115 core unit tests, 4 wire-oracle integration tests, 7 testkit tests, and 4 xtask tests); 0 failed.
- `cargo test --locked --offline --doc --package worlddb-core`: 65 passed; 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo deny check all`: advisories, bans, licenses, and sources PASS.
- `cargo xtask verify`: 27 PASS, 1 visible expected `ci-matrix` SKIP required by deferred M0-14, 0 FAIL. The source and plan checks report 236 tasks, 11 milestones, 253 invariants, and 159 follow-up pairs.
- `git diff --check HEAD`: PASS; Git reported only its existing LF-to-CRLF working-copy notices.
