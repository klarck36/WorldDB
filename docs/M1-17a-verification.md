# M1-17a verification: schema and project record codecs

**Status:** DONE\
**Verified:** 2026-09-29 20:33 Mitteleuropäische Sommerzeit Europe/Berlin\
**Scope:** the project, schema, layer, and migration DTO types already present in `worlddb-core`.\
**Wire format:** WorldDB frame 1.0 and the canonical TLV/scalar encodings implemented in M1-16.

## Stable record assignments

The top-level frame kinds and field maps are registered in `policy/record-wire-kinds.tsv`. The assignments are fixed within major version 1.0 and are implemented by `RecordKind` in `crates/worlddb-core/src/wire_records.rs`.

All M1-17a records are closed. An unassigned frame kind returns `UnknownRecordKind`; an unknown top-level or nested field returns `UnknownField`. Unknown optional frame bits are retained in `DecodedRecord` and survive re-encoding. Unknown core `Value` tags continue to fail closed under the M1-16 decoder.

Record fields use the ascending TLV tags in the registry. IDs are validated raw 16-byte UUIDs; revisions, enum codes, counts, and lengths are minimal unsigned LEB128 unless a field explicitly says otherwise. Text and symbols use a minimal byte-length prefix followed by exact UTF-8; symbols additionally pass the fixed `[a-z][a-z0-9_]*` grammar. Optional strings and optional structured values encode as `00` for absent or `01` followed by the value encoding for present. Optional parent IDs use an empty field for absent and 16 bytes for present.

Arrays encode a minimal item count followed by each item's minimal byte length and payload. Ordered migration steps retain input order. `LayerSchemaSnapshot` definitions are ID-sorted by the domain constructor. Predicate constraints, EventKind roles, and EventKind attributes are also emitted in their domain-defined canonical order. Decoding reconstructs through the validating domain constructors and compares the complete frame to its canonical re-encoding; alternate orderings are rejected as `NonCanonicalRecord`.

## Nested schema assignments

These code points are local to their containing schema record and stable for major version 1:

| Type | Codes |
|---|---|
| Lifecycle | `1` Active, `2` Deprecated, `3` Retired |
| EntityTypeConstraint | `1` AnyEntity, `2` Exact followed by raw EntityTypeId |
| ValueKind | `1` Bool, `2` Int, `3` UInt, `4` Decimal, `5` String, `6` Symbol, `7` Entity, `8` Time, `9` Duration, `10` Bytes |
| Cardinality | `1` Single, `2` Multi |
| ResolutionPolicy | `1` SingleValueReplace, `2` MultiValueOverlay, `3` MultiValueReplace |
| EventTimeForm | `1` InstantOnly, `2` SpanOnly, `3` InstantOrSpan, `4` OpenSpanAllowed |
| MigrationCategory | `1` MetadataOnly, `2` Additive, `3` CompatibleConstraintChange, `4` Restrictive, `5` Breaking |
| MigrationRunState | `1` Planned, `2` Running, `3` Completed, `4` Failed |
| ValueConstraint | `1` BoolSet, `2` IntRange, `3` UIntRange, `4` DecimalRange, `5` StringByteLength, `6` SymbolSet, `7` TimeRange, `8` DurationRange, `9` BytesLength |

Each constraint is a nested TLV record with field 1 for its fixed kind code and field 2 for its body. Ranges use nested fields 1 and 2 for optional lower and upper values. Endpoints use the existing canonical `Value` encoding, then are checked against the declared constraint variant. Sets are arrays of canonical `Value` encodings. Decimal metadata and EventTime constraints use closed nested TLV records. Event roles and attributes are nested TLV records with fixed field assignments in the codec; the top-level registry identifies their containing array fields.

Nested field assignments are also fixed: Decimal metadata uses fields `1:optional_display_precision`, `2:optional_measurement_precision`, `3:optional_currency_scale`; an EventRole uses `1:event_role_id`, `2:symbol`, `3:entity_type_constraint`, `4:role_cardinality` (whose fields are `1:min`, `2:optional_max`); an EventAttribute uses `1:event_attribute_id`, `2:symbol`, `3:value_kind`, `4:optional_object_constraint`, `5:constraints_array`, `6:optional_decimal_metadata`, `7:required_bool`; an EventTimeConstraint uses `1:form`, `2:optional_calendar_period` (whose fields are `1:years`, `2:months`, `3:days`). Nested TLV fields are strictly increasing and closed as well.

## Persistence boundary

`EntityCatalogSnapshot`, `PerspectiveCatalogSnapshot`, `HistorySpaceCatalog`, and other aggregate catalog views remain reconstructible in-memory views. The codec persists their constituent immutable definitions and retirement records. `LayerSchemaSnapshot` is encoded as a complete schema state because its explicit `base_layer_id` is a critical designation and cannot be reconstructed from layer definitions alone.

Migration codecs cover the `MigrationPlan`, `MigrationRun`, and `MigrationStepCommitIdentity` DTOs available in M1-15. Source-schema preconditions, target schema payloads, fingerprints, transformers, and budgets remain part of the later M7 migration planner contract.

M1-17a does not enforce decoder memory or CPU budgets. Pre-allocation limits and the decoder inventory/fuzz campaign remain assigned to M1-18.

## Evidence

- `crates/worlddb-core/tests/data/record-v1.0-golden.tsv` fixes complete frame bytes for all 13 record kinds, including checksums.
- `wire_records::tests::all_m1_17a_records_match_fixed_golden_frames_and_round_trip` checks each encoding against its fixed vector, decodes it, and requires identical re-encoding.
- Tests exercise every closed `ValueConstraint` variant, optional frame flag preservation, missing fields, unknown kinds, unknown fields, malformed arrays, and an unknown nested constraint code.
- `WorldDB_1.0_Invariantenabdeckung.tsv` records the WDB-WIR evidence for this task.

## Verification results

- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline --workspace --all-targets`: 122 passed (107 core unit tests, 4 wire-oracle integration tests, 7 testkit tests, and 4 xtask tests); 0 failed.
- `cargo test --locked --offline --doc --package worlddb-core`: 65 passed; 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo deny check all`: advisories, bans, licenses, and sources PASS.
- `cargo xtask verify`: 27 PASS, 1 visible expected `ci-matrix` SKIP required by deferred M0-14, 0 FAIL. The plan check reports 236 tasks, 11 milestones, 253 invariants, and 159 follow-up pairs.
- `git diff --check HEAD`: PASS; Git reported only its existing LF-to-CRLF working-copy notices.
