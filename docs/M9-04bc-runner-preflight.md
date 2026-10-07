# M9-04b/c – Import and recovery runner preflight

**Status:** PREFLIGHT COMPLETE; LONG CAMPAIGNS ACTIVE
**Date:** 7 October 2026
**Platform:** Windows

The import and recovery targets are connected to the Rust campaign runners.
Each target ran for one second with its registered seed corpus and a fixed
deterministic seed. All 19 routes completed with zero crashes. The harness
recorded 166,878 parser/probe calls: 76,812 returned accepted and 90,066
returned rejected. These counters are smoke results only; they do not satisfy
the required 86,400-second campaign for any target.

| Task | Target | Rounds | Accepted | Rejected | Corpus inputs | Crashes |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| M9-04b | `cli_import_mapping` | 21,711 | 2,744 | 18,967 | 4 | 0 |
| M9-04b | `cli_migration_plan_json` | 681 | 110 | 571 | 1 | 0 |
| M9-04b | `cli_migration_step_records` | 551 | 551 | 0 | 1 | 0 |
| M9-04b | `storage_logical_export` | 13,121 | 2,596 | 10,525 | 1 | 0 |
| M9-04b | `storage_logical_import_plan` | 21,281 | 1,770 | 19,511 | 2 | 0 |
| M9-04b | `storage_import_prepare` | 13,252 | 1,292 | 11,960 | 2 | 0 |
| M9-04b | `storage_sharing_export` | 16,976 | 2,977 | 13,999 | 1 | 0 |
| M9-04b | `desktop_export_import_dto` | 16,856 | 3,329 | 13,527 | 4 | 0 |
| M9-04b | `desktop_migration_plan` | 792 | 128 | 664 | 1 | 0 |
| M9-04b | `desktop_purge_report` | 310 | 104 | 206 | 1 | 0 |
| M9-04c | `storage_wal_recovery_prefix` | 88 | 88 | 0 | 1 | 0 |
| M9-04c | `storage_wal_payloads` | 18,442 | 18,442 | 0 | 1 | 0 |
| M9-04c | `storage_audit_wal` | 94 | 12 | 82 | 1 | 0 |
| M9-04c | `storage_recovery_journal` | 86 | 86 | 0 | 1 | 0 |
| M9-04c | `storage_recovery_pipeline` | 76 | 22 | 54 | 9 | 0 |
| M9-04c | `storage_verify_pipeline` | 62 | 62 | 0 | 9 | 0 |
| M9-04c | `storage_salvage_scanner` | 32 | 32 | 0 | 1 | 0 |
| M9-04c | `storage_required_audit_payload` | 20,905 | 20,905 | 0 | 1 | 0 |
| M9-04c | `storage_replay_payload` | 21,562 | 21,562 | 0 | 1 | 0 |

Recovery probes write mutated bytes only into temporary databases or an
isolated fixture copy. Temporary files, database directories, salvage output,
and the modified fixture segment are removed or restored after each probe.
Valid logical-import-plan and desktop DTO/report seeds were added to the
registered corpora so those targets receive canonical examples as well as
mutations.

Reports are under `%LOCALAPPDATA%\WorldDB\fuzz-results\m9-04b-smoke` and
`%LOCALAPPDATA%\WorldDB\fuzz-results\m9-04c-smoke`. They were generated from a
working tree with uncommitted changes; the recorded base Git revision is not a
source-tree identity for these edits. The long campaign runner requires a clean
commit and writes a source-tree hash in its run manifest. The smoke reports do
not contain campaign coverage artifacts or a long-run manifest; future
campaigns will create those in their own output directories.

The full verification run completed with 43 PASS, one expected `ci-matrix`
SKIP, and two FAIL. Both failures are the existing RC source-fingerprint
mismatch for `manifest.rs` (the `public-contracts` step and its baseline test);
no other verification step failed. The frozen RC baseline was left unchanged.

The long-run harness now collects LLVM LCOV for Rust and Node V8 coverage JSON
for TypeScript. On 7 October, a separate one-second storage target produced
three Rust raw profiles and a 3.68 MB LCOV report; the TypeScript smoke wrote
two V8 coverage files. Byte-preservation checks for the Rust crash archive
helpers pass. These artifact probes do not count as 24-hour runs. Each actual
campaign will write its own hashed crash-corpus manifest under its run folder.

## Active long-run campaigns

Two of the 19 import/recovery targets now have valid 86,400-second campaigns
running from clean commit `7c5e1e3`, using seed `0x574f524c44444232` and output
under `%LOCALAPPDATA%\WorldDB\fuzz-results`:

- M9-04b `cli_import_mapping`: run
  `M9-04-cli_import_mapping-574f524c44444232-20261007T123908Z`, started
  7 October 2026 at 14:39:08 CEST.
- M9-04c `storage_wal_recovery_prefix`: run
  `M9-04-storage_wal_recovery_prefix-574f524c44444232-20261007T123909Z`, started
  7 October 2026 at 14:39:09 CEST.

Both were confirmed live after startup. The remaining nine import targets and
eight recovery targets still need their long campaigns. The separate M9-04a
`engine_ipc_request` campaign is recorded in the M9-04a preflight report.
