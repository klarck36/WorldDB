# M1-18 verification: decoder budgets and fuzz campaign

**Status:** Rust and TypeScript campaigns passed locally with zero crashes. Source-commit binding is still deferred by the user, so M1-18 remains `WAITING_EXTERNAL`.\
**Main run:** `M1-18-574f524c44444231-1790715056`, started 2026-09-29T22:50:56+02:00.\
**Seed:** `0x574f524c44444231`.\
**Supplement:** `M1-18-574f524c44444232-1790718684`, seed `0x574f524c44444232`.\
**TypeScript campaign:** `M1-18-ts-574f524c44444232-1790722644`, seed `0x574f524c44444232`, started 2026-09-30T00:57:24+02:00; completed with 3,615.070 elapsed seconds, 3,600.156 CPU seconds, 4,935,681 rounds, 44,435,604 decoder calls, and zero crashes (`PASS_LOCAL`). The report records all 12 seed strategies with nonzero call counts.\
**Source revision:** The TypeScript report identifies `UNCOMMITTED_GIT_DEFERRED`, following the user's explicit instruction to defer Git integration. M1-18 requires the campaign result to be bound to a source commit, so its status remains `WAITING_EXTERNAL` until that binding is possible.

## Decoder resource policy

`DecoderLimits` is configurable independently of the 1.0 wire contract. Defaults cap frames at 64 MiB, copied string/byte values at 16 MiB, TLV fields at 256, arrays and batches at 1,000,000 items/records, decoded collections at 64 MiB, batches at 256 MiB, and nesting at depth 8. Declared lengths and record counts are checked before narrowing casts or reservations. Decoder collection reservations use fallible allocation APIs, and malformed or over-budget input returns typed errors.

The shared inventory at `policy/decoder-inventory.tsv` lists 76 targets: 75 Rust decoder targets (35 record kinds, 24 RecordRef variants, 10 scalar families, both frame forms, TLV, record batches, and both audit formats) plus `typescript.json_envelope`. The Rust runner executes the nine strategies in `policy/decoder-seeds.tsv` over its 75 core targets. The TypeScript runner reads `bindings/typescript/test/transport-fuzz-seeds.tsv` and exercises the registered JSON envelope target; all 12 strategies passed the smoke run and the one-hour CPU campaign. The TypeScript artifacts are archived in [the report](M1-18-fuzz-typescript.json) and [the five-second CPU samples](M1-18-fuzz-cpu-samples-typescript.tsv).

## Long campaign

The ignored campaign was launched with:

```powershell
$env:CARGO_TARGET_DIR = 'target/fuzz-hour'
$env:WORLDDB_DECODER_FUZZ_SECONDS = '3600'
$env:WORLDDB_DECODER_FUZZ_SEED = '0x574f524c44444231'
cargo test --locked --offline -p worlddb-core --test decoder_fuzz -- --ignored --exact decoder_budget_fuzz_campaign --nocapture
```

The main campaign completed in 3,600.001 seconds: 1,672,551 rounds and 0 crashes across all 75 targets. It made 1,128,971,925 decoder calls and accumulated 3,488.969 CPU seconds by its final sample. The 180.001-second supplement completed 124,289 rounds, made 83,895,075 calls across all 75 targets, and had 0 crashes; its measured CPU time was 175.391 seconds. Together the runs used **3,780.002 elapsed seconds and 3,664.359 CPU seconds**, covering 1,796,840 rounds and 1,212,867,000 decoder calls. The detailed per-target results are archived in the [main report](M1-18-fuzz-main.json) and [supplement report](M1-18-fuzz-extension.json); five-second process CPU samples are archived for the [main campaign](M1-18-fuzz-cpu-samples-main.tsv) and [supplement](M1-18-fuzz-cpu-samples-extension.tsv).

## Local verification

- `cargo test --locked --offline --workspace --all-targets`: 146 passed, 0 failed, 1 ignored; the Rust one-hour campaign is ignored in the ordinary suite.
- Rust inventory test: PASS; 75 Rust targets and nine Rust mutation strategies are exercised by the runner. The shared inventory additionally registers the TypeScript JSON-envelope target.
- `pnpm --dir bindings/typescript verify`: PASS; TypeScript typecheck, 8 tests, and 12-seed fuzz smoke.
- TypeScript CPU campaign: `PASS_LOCAL`; 3,600.156 CPU seconds, 4,935,681 rounds, 44,435,604 calls, 12/12 seed strategies exercised, zero crashes.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: PASS, 30 passed, 1 expected visible skip for deferred M0-14 CI, 0 failed.
- `cargo deny check all`: PASS.
- `WorldDB_1.0_Plancheck.py`: PASS; 236 tasks, 11 milestones, 253 invariants, and 161 follow-up pairs.

**Final local check:** 2026-09-30T02:10:03+02:00. The M1 gate precheck remains open as expected: M1-18 is not `DONE`, and WDB-WIR-003/005 cannot be closed until their primary evidence is bound to the required source commit. `cargo xtask verify` reran at this checkpoint with 30 passes, 1 expected CI-matrix skip, and 0 failures.

## Supplementary source-level budget audit

**Checked:** 2026-09-30T00:06:35+02:00. The production decoder paths in `wire.rs`, `wire_records.rs`, `wire_records/{projects,schema,migrations,lifecycle,events,meta}.rs`, and `audit_wire.rs` were reviewed for length checks, copied values, and collection reservations. No unbounded variable-size decoder copy or collection reservation was found: strings and scalar byte values pass their configured size checks before copying; Source content digests and both audit fingerprints have explicit `max_string_or_bytes` checks; arrays and nested/outer decoded collections check item/count or collection budgets before `try_reserve`; TLV fields enforce their count limit before growing the field vector. RecordRef decoding is fixed-width and does not allocate. This is a code review supplement to the executable malformed/oversize tests and fuzz campaigns, not a substitute for binding the run reports to a source commit.

## TypeScript JSON-envelope decoder

The shared inventory also registers `typescript.json_envelope`. Its M1-19 parser enforces the 64 MiB UTF-8 frame bound and preflights the closed schema before `JSON.parse`, limiting object/property/string counts and lengths and rejecting arrays. The 16 MiB Bytes limit is enforced before Base64url output allocation. Eight negative/roundtrip tests, the 12-seed smoke campaign, and the one-hour CPU campaign pass locally. The CPU campaign report and 525 five-second samples are archived with this verification record.

## Remaining gate condition

The passing run results still need to be tied to the exact source commit before M1-18 or `WDB-WIR-003` can be marked DONE. Git/GitHub integration and commits were explicitly deferred by the user, so the plan remains `WAITING_EXTERNAL` until that integration is available. M1-20 remains gated on M1-18.
