# M1-16 verification: TLV frame and scalar encoding

**Status:** DONE\
**Verified:** 2026-09-29 20:00 Mitteleuropäische Sommerzeit Europe/Berlin\
**Scope:** Version 1.0 frame, checksum, canonical TLV fields, and the closed scalar `Value` set. The original §12 fixes the frame shape and scalar rules but leaves the magic and value-tag numbers unspecified; this task assigns them so implementations can exchange identical bytes.

## Version 1.0 assignments

- Frame magic is the eight bytes `WorldDB\0` (seven ASCII bytes followed by NUL). Version is `1.0`; the header remains the specified 40-byte little-endian layout. The 32-byte BLAKE3 digest covers the complete header and payload, excluding the digest itself. This checksum detects byte changes; it does not authenticate a writer.
- No required capability bits are defined in 1.0. A nonzero required bit is rejected. Optional capability bits are preserved exactly, even when unknown. The frame kind remains a raw `u32` for later record-codec assignments.
- TLV field tags, lengths, and core `Value` tags use minimal unsigned LEB128. The encoder and decoder require field tags to be strictly increasing, which rejects duplicates and descending fields. The generic field decoder exposes unknown field bytes unchanged; a closed core `Value` decoder rejects unknown value tags.

| ValueTag | Value | Payload |
|---:|---|---|
| 1 | Bool | Exactly one byte: `00` or `01` |
| 2 | Int | Minimal ZigZag `i128` varint |
| 3 | UInt | Minimal `u128` varint |
| 4 | Decimal | Sign byte, minimal unsigned coefficient varint, `i32le` scale |
| 5 | String | Minimal byte-length varint, exact valid UTF-8 bytes |
| 6 | Symbol | Minimal byte-length varint, ASCII `[a-z][a-z0-9_]*` bytes |
| 7 | Entity | Validated raw 16-byte `EntityId` |
| 8 | Time | Raw 16-byte `TimelineId`, signed ticks varint, length-prefixed unit symbol |
| 9 | Duration | Signed nanoseconds as a minimal ZigZag `i128` varint |
| 10 | Bytes | Minimal byte-length varint and exact opaque bytes |

The tags are stable for format major 1. Decimal normalization is delegated to `Decimal`; zero has positive sign and scale zero. Strings retain their original UTF-8 bytes without Unicode normalization. Time keeps its source timeline, tick count, and unit; it does not infer a timezone or compare unresolved timelines.

## Implementation and evidence

- `crates/worlddb-core/src/wire.rs` implements the fixed header, BLAKE3 checksum verification, raw typed-ID conversion, scalar encodings, and ordered TLV reader/writer. Integer varints share the canonical implementation in `numbers.rs`.
- `crates/worlddb-core/tests/data/wire-v1.0-golden.tsv` holds two complete frame vectors and ten scalar vectors. `tests/wire_oracle.rs` independently parses the frame header, checksum boundary, LEB128 values, TLV order, and core value tags. It checks its output against the same fixed vectors and the production decoder/encoder.
- `WDB-WIR-001` is supported by exact-version and capability rejection tests, checksum verification, and the independent frame oracle.
- `WDB-WIR-002` is supported by all ten scalar vectors, integer boundary roundtrips, nonminimal-varint rejection, duplicate/descending-field rejection, noncanonical Decimal rejection, and unknown core-tag rejection.
- M1-16 does not define decoder resource budgets. M1-18 remains responsible for pre-allocation hard limits and the longer fuzz run.

## Dependency review

BLAKE3 is pinned to `1.8.7` with default features disabled. Its lockfile closure, source, declared license alternatives, MSRV, build-time `cc` path, and safe API use were recorded in `policy/dependencies.tsv`. `cargo-deny` keeps its general interpreted-script and external-default-feature bans; only the three exact BLAKE3 release-script paths and `shlex 2.0.1`'s `default`/`std` features required by `cc` are scoped exceptions in `.cargo/deny.toml`.

## Verification results

- `cargo test --locked --offline --workspace --all-targets`: 113 passed (99 core unit tests, 3 independent wire-oracle tests, 7 testkit tests, and 4 xtask tests), 0 failed. CLI and file-storage crates contain no unit tests.
- `cargo test --locked --offline --doc --package worlddb-core`: 65 passed, 0 failed.
- `cargo fmt --all -- --check` and `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo-deny 0.20.2 check all`: PASS. The dependency register check covers 29 locked external packages and 0 forbidden feature pairs.
- `cargo xtask verify`: 27 PASS, 1 visible `ci-matrix` SKIP required by deferred M0-14 GitHub/macOS CI, 0 FAIL. This includes the crate-graph, contract-source, source, and plan checks.
- `WorldDB_1.0_Plancheck.py`: 236 tasks, 11 milestones, 253 invariants, and 159 follow-up pairs; structure valid.
- `git diff --check HEAD`: PASS; Git reported only its existing LF-to-CRLF working-copy notices.

**Next task:** M1-17a, schema and project codecs.
