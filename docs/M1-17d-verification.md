# M1-17d verification: meta and separate audit codecs

**Status:** DONE — final local verification passed 2026-09-29T21:50:13+02:00\
**Scope:** Source, Evidence, Provenance, both meta-edge lifecycle records, all 24 RecordRef variants, and the separate AuditRecord / RawReadAttempt frames.\
**Persistent record format:** WorldDB frame 1.0 with the canonical TLV and scalar rules from M1-16. Audit frames share the verified frame primitive but use a separate kind registry and codec API; neither audit payload is a `Record` or a `RecordRef`.

## Closed WorldDB record catalog

The record-kind catalog now contains 35 closed kinds: 13 project/schema/migration kinds (`0x1001–0x100d`), 10 assertion/lifecycle kinds (`0x1101–0x110a`), 7 event/lifecycle kinds (`0x1201–0x1207`), and 5 source/provenance kinds (`0x1301–0x1305`). `policy/record-wire-kinds.tsv` is checked against the runtime enum and the golden-frame kind set. Every registered kind has at least one exact frame vector.

| Kind | Record | Closed fields |
|---|---|---|
| `0x1301` | Source | `1:id; 2:source_kind_symbol; 3:optional_locator; 4:optional_content_digest_bytes; 5:source_metadata_array; 6:created_revision` |
| `0x1302` | Evidence | `1:id; 2:source_id; 3:evidence_target_record_ref; 4:relation; 5:created_revision` |
| `0x1303` | Provenance | `1:id; 2:from_record_ref; 3:to_record_ref; 4:relation; 5:created_revision` |
| `0x1304` | EvidenceRetraction | `1:id; 2:evidence_id; 3:reason; 4:created_revision` |
| `0x1305` | ProvenanceRetraction | `1:id; 2:provenance_id; 3:reason; 4:created_revision` |

Source metadata is a canonical array of nested `1:symbol_key; 2:typed_value` entries. Keys must be strictly increasing; duplicate or descending entries fail. Locator and digest have explicit option markers and cannot encode empty values. Evidence targets and both Provenance endpoints encode the central tag-plus-typed-ID RecordRef form. Relation codes are closed: Evidence `1=Supports, 2=Contradicts, 3=Documents`; Provenance `1=Corrects, 2=DerivedFrom, 3=ResultedFrom`.

The fixed frame corpus contains 47 vectors across those 35 kinds. `policy/record-ref-wire-tags.tsv` has fixed vectors for all 24 variants. Unknown tags, nonminimal tags, invalid IDs, and malformed lengths fail closed.

## Endpoint matrix and explicit layers

The Source/Evidence/Provenance wire decoders reconstruct the same closed domain types used by constructors, so the normative endpoint rules apply before a record can be returned:

- Evidence accepts 21 of the 24 RecordRef variants. Source, Evidence, and EventRelation are rejected as targets; all other registered variants, including ArchiveTransition and concrete lifecycle records, remain typed.
- Generic Provenance accepts 23 of 24 endpoint variants; EventRelation is excluded. `Corrects` requires the same concrete endpoint family on both sides, `DerivedFrom` has a narrower `from` set, and `ResultedFrom` allows only Assertion/Event endpoints on either side. Self-loops fail.
- The codec matrix test exercises all Evidence targets and 6,912 Provenance endpoint/relation combinations, including distinct IDs for compatible `Corrects` pairs. It compares wire decode admission with the domain constructor result.
- The five layer-capable record families are Assertion, Mask, ReplacementBoundary, Event, and EventMask. The first three carry `LayerId` inside their explicit Context field; Event and EventMask carry it as field 3. A frame with the Event `LayerId` removed is rejected for the missing field. Source, Evidence, and Provenance remain project-wide metadata and do not acquire a synthetic layer.

## Separate audit formats

Audit frame kind assignments are kept out of `RecordKind` and `record-wire-kinds.tsv`:

| Audit kind | Record | Fields |
|---|---|---|
| `0xa001` | AuditRecord | `1:audit_record_id; 2:audit_sequence; 3:audit_operation_id; 4:actor_principal_id; 5:action_code; 6:object_class_code; 7:outcome_code; 8:commit_context_union; 9:security_epoch; 10:policy_fingerprint_bytes` |
| `0xa002` | RawReadAttempt | `1:audit_record_id; 2:audit_sequence; 3:audit_operation_id; 4:client_request_id; 5:principal_id; 6:scope_fingerprint_bytes; 7:snapshot_id; 8:security_epoch; 9:page_ordinal` |

`policy/audit-wire-kinds.tsv` and `policy/audit-wire-values.tsv` fix the kinds, enum codes, and commit-context union codes. Audit payload fields are closed. Fingerprints must be nonempty, unknown enum codes and fields fail, and audit frames reject optional flags instead of discarding them. AuditRecord commits are distinct from RawReadAttempt: the latter has no WorldDB Revision or read payload, preserving the separate append-only raw-read audit protocol.

Three audit golden frames cover a committed AuditRecord, an uncommitted/rejected AuditRecord, and a page-zero RawReadAttempt. A separate frame/TLV oracle parses and verifies each; the production audit decoders roundtrip the exact bytes. Audit WAL durability, append ordering, recovery, and authorization are not codec behavior and remain in their later storage/security tasks.

## Verification evidence

- `wire_records::tests::all_implemented_records_match_fixed_golden_frames_and_round_trip`: exact bytes and canonical decode/re-encode for all 47 record vectors.
- `wire_records::tests::every_registered_record_kind_has_a_fixed_roundtrip_golden_vector`: exact kind-set equality across 35 registry entries and golden vectors.
- `wire_records::tests::domain_record_refs_match_fixed_golden_vectors`: all 24 RecordRef scalar vectors; `runtime_record_ref_tags_match_the_complete_policy_registry` checks all 24 tags.
- `wire_records::tests::source_meta_decoder_enforces_the_closed_evidence_and_provenance_matrices` and `source_decoder_rejects_noncanonical_metadata_and_empty_optional_values`: disallowed targets/endpoints, relation constraints, noncanonical metadata, and empty option payloads fail.
- `wire_records::tests::every_layer_capable_record_wire_payload_has_an_explicit_layer_id`: all five layer-capable kinds carry one explicit ID; removing Event's LayerId is rejected.
- `audit_wire::tests::audit_frames_match_fixed_golden_vectors_and_round_trip`, the two audit registry tests, and `independent_frame_and_tlv_oracle_accepts_every_separate_audit_golden`: exact separate audit frames and closed kind/value registries.
- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline --workspace --all-targets`: 140 passed (124 core, 5 wire-oracle, 7 testkit, 4 xtask); 0 failed.
- `cargo test --locked --offline --doc --package worlddb-core`: 65 passed; 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo deny check all`: PASS (advisories, bans, licenses, sources).
- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: PASS (236 tasks, 11 milestones, 253 invariants, 159 follow-up pairs).
- `cargo xtask verify`: 27 PASS, 1 expected visible `ci-matrix` SKIP from M0-14, 0 FAIL. The independent contract/source checks, M0-13 evidence replay, crate graph, feature matrix, policy tests, and whitespace gate all passed.
- `git diff --check HEAD`: PASS; Git emitted only configured LF/CRLF conversion advisories for changed files.
