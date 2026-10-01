# M1-18 verification: decoder budgets and fuzz campaigns

**Status:** DONE. The Rust and TypeScript campaigns completed without crashes, and every registered target and seed strategy received nonzero calls.

**Source revision for the decoder/transport implementation:** `98fc31f6ffe64f171f39293fec76c24b9a423710`. The later working-tree changes used during these runs only added a decoder-policy test and fixed report output paths; they did not change the Rust decoder or TypeScript transport code under test.

## Commit-bound campaign results

| Campaign | Run ID | Seed | Elapsed | CPU evidence | Coverage and result |
|---|---|---|---:|---:|---|
| Rust main | `M1-18-574f524c44444231-1790754782` | `0x574f524c44444231` | 3,600.001 s | final near-end sample: 3,575.797 s | 3,024,913 rounds; 75/75 targets; 27,224,217 calls per target (2,041,816,275 total); 0 crashes |
| Rust extension | `M1-18-574f524c44444232-1790758461` | `0x574f524c44444232` | 180.002 s | final near-end sample: 172.203 s | 73,684 rounds; 75/75 targets; 663,156 calls per target (49,736,700 total); 0 crashes |
| TypeScript | `M1-18-ts-574f524c44444232-1790754779` | `0x574f524c44444232` | 3,623.416 s | **3,600.439 CPU s** | 4,500,481 rounds; 40,517,529 calls; all 12 seed strategies called; 0 crashes |

The Rust process samples provide a conservative combined lower bound of **3,748.000 CPU seconds** (3,575.797 main + 172.203 extension), exceeding one CPU hour. The campaign reports bind all three runs to the source revision above. Rust cycles nine mutation strategies over the 75 Rust targets; the shared inventory additionally includes the TypeScript JSON-envelope target. The TypeScript report records nonzero calls for all 12 strategies, including the three resource-heavy strategies.

Reports and process samples:

- [Rust main report](M1-18-fuzz-main.json) and [main CPU samples](M1-18-fuzz-cpu-samples-main.tsv)
- [Rust extension report](M1-18-fuzz-extension.json) and [extension CPU samples](M1-18-fuzz-cpu-samples-extension.tsv)
- [TypeScript report](M1-18-fuzz-typescript.json) and [TypeScript CPU samples](M1-18-fuzz-cpu-samples-typescript.tsv)
- [Shared decoder inventory](../policy/decoder-inventory.tsv), [Rust mutation strategies](../policy/decoder-seeds.tsv), and [TypeScript seed strategies](../bindings/typescript/test/transport-fuzz-seeds.tsv)

## Decoder resource policy

`DecoderLimits` is configurable independently of the 1.0 wire contract. Defaults cap frames at 64 MiB, copied string/byte values at 16 MiB, TLV fields at 256, arrays and batches at 1,000,000 items/records, decoded collections at 64 MiB, batches at 256 MiB, and nesting at depth 8. Declared lengths and record counts are checked before narrowing casts or reservations. Decoder collection reservations use fallible allocation APIs, and malformed or over-budget input returns typed errors. `wire::tests::decoder_limit_defaults_are_policy_separate_from_wire_semantics` verifies that changing decoder limits does not change canonical wire bytes or the fixed 1.0 frame version.

## Local verification

- `cargo xtask verify`: 30 PASS, one expected visible `ci-matrix` skip for M0-14, 0 FAIL.
- `cargo test --locked --offline --workspace --all-targets`: 154 PASS, 0 FAIL, 2 intentionally ignored campaigns.
- `cargo test --locked --offline --workspace --doc`: 65 PASS, 0 FAIL.
- TypeScript typecheck, eight transport tests, and fuzz smoke: PASS (included in `cargo xtask verify`).
- Rust format, Clippy with warnings denied, dependency deny/policy, workspace lint, unsafe policy, contract source/doc checks, source check, plan check, and whitespace check: PASS.
- `WorldDB_1.0_Plancheck.py --gate-precheck M1`: recorded in [the M1 gate review](gates/M1.md).

## Decoder source-level budget audit

The production decoder paths in `wire.rs`, `wire_records.rs`, `wire_records/{projects,schema,migrations,lifecycle,events,meta}.rs`, and `audit_wire.rs` were checked for length validation, copied values, and collection reservations. Strings and scalar byte values pass configured size checks before copying; Source content digests and audit fingerprints have explicit byte limits; arrays and nested/outer decoded collections check budgets before `try_reserve`; TLV fields enforce the count limit before growing the field vector. RecordRef decoding is fixed-width and does not allocate. The executable malformed/oversize tests and campaigns provide the associated runtime evidence.
