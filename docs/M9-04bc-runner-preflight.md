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

Four of the 19 import/recovery targets now have valid 86,400-second campaigns
running from clean commit `7c5e1e3`, using seed `0x574f524c44444232` and output
under `%LOCALAPPDATA%\WorldDB\fuzz-results`:

- M9-04b `cli_import_mapping`: run
  `M9-04-cli_import_mapping-574f524c44444232-20261007T123908Z`, started
  7 October 2026 at 14:39:08 CEST.
- M9-04c `storage_wal_recovery_prefix`: run
  `M9-04-storage_wal_recovery_prefix-574f524c44444232-20261007T123909Z`, started
  7 October 2026 at 14:39:09 CEST.

Two additional targets started from clean commit `681e1e2`, also using the
registered 86,400-second profiles:

- M9-04b `cli_migration_plan_json`: run
  `M9-04-cli_migration_plan_json-574f524c44444232-20261007T124735Z`, started
  7 October 2026 at 14:47:35 CEST.
- M9-04c `storage_wal_payloads`: run
  `M9-04-storage_wal_payloads-574f524c44444232-20261007T124735Z`, started
  7 October 2026 at 14:47:35 CEST.

All four were confirmed live after startup. Eight import targets and seven
recovery targets still need their long campaigns. The separate M9-04a
`engine_ipc_request` campaign is recorded in the M9-04a preflight report.

## Monitoring update — 7 October 2026, 18:23 CEST

All four M9-04b/c processes were alive at the check, with no completion
manifest. Their latest resource samples were written through 18:23 CEST:

| Run | PID | Elapsed | CPU | RSS | Run-folder size |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 13,422.6 s | 15,460.9 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 12,916.7 s | 4,667.1 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 13,418.9 s | 4,628.9 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 12,913.2 s | 14,248.9 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 18:38 CEST.

## Monitoring update — 7 October 2026, 18:38 CEST

All four active M9-04b/c processes were alive at the check, and none of the
run folders contained a completion manifest. Resource samples were current
through 18:38:22 CEST:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 14,308.5 s | 16,487.6 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 13,802.3 s | 5,008.9 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 14,272.2 s | 4,938.2 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 13,766.5 s | 15,200.2 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 18:53 CEST.

## Monitoring update — 7 October 2026, 19:02 CEST

All four active M9-04b/c processes were alive at the check, and none of the
run folders contained a completion manifest. Resource samples were current
through 19:02:32 CEST:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 15,758.8 s | 18,180.6 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 15,252.9 s | 5,600.8 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 15,721.7 s | 5,475.2 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 15,216.0 s | 16,819.2 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 19:17 CEST.

## Monitoring update — 7 October 2026, 19:21 CEST

All four active M9-04b/c processes were alive. No completion manifests were
present; resource samples were current through 19:21 CEST:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 16,864.3 s | 19,455.8 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 16,390.0 s | 6,042.0 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 16,858.2 s | 5,889.4 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 16,352.6 s | 18,075.7 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 19:36 CEST.

## Monitoring update — 7 October 2026, 19:45 CEST

All four active M9-04b/c processes were alive at 19:45:10 CEST. Resource
samples were current through 19:45:07 CEST; none of the run folders contained
a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 18,287.1 s | 21,094.5 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 17,813.2 s | 6,611.6 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 18,277.6 s | 6,407.5 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 17,773.2 s | 19,651.1 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 20:00 CEST.

## Monitoring update — 7 October 2026, 20:00 CEST

All four active M9-04b/c processes were alive at 20:00:25 CEST. Their samples
were current through 20:00:22 CEST; none of the run folders had a completion
manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 19,201.8 s | 22,158.3 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 18,727.5 s | 6,986.6 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 19,191.9 s | 6,748.3 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 18,687.8 s | 20,665.9 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 20:15 CEST.

## Monitoring update — 7 October 2026, 20:20 CEST

All four active M9-04b/c processes were alive at 20:19:58 CEST. Their samples
were current through 20:20:07 CEST; none of the run folders had a completion
manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | 20,401.0 s | 23,547.1 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | 19,895.0 s | 7,464.0 s | 39.1 MiB | 980.5 MB |
| `storage_wal_recovery_prefix` | 28428 | 20,390.4 s | 7,200.4 s | 39.6 MiB | 926.4 MB |
| `storage_wal_payloads` | 7348 | 19,885.9 s | 21,998.4 s | 38.6 MiB | 926.4 MB |

Next process and sample check: 20:35 CEST.

## Monitoring update — 7 October 2026, 20:33 CEST

The active `cli_import_mapping`, `cli_migration_plan_json`, and
`storage_wal_payloads` processes were alive at 20:33:42 CEST. Their samples
were current through 20:33:37 CEST; none had a completion manifest:

| Run | PID | Status | Elapsed | CPU | RSS | Run output |
|---|---:|---|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | RUNNING | 21,223.2 s | 24,468.4 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | RUNNING | 20,717.2 s | 7,792.7 s | 39.1 MiB | 980.5 MB |
| `storage_wal_payloads` | 7348 | RUNNING | 20,677.0 s | 22,850.9 s | 38.6 MiB | 926.4 MB |

`storage_wal_recovery_prefix` (PID 28428) exited during the local C: disk-full
incident at 20:28 CEST. Its `fuzzer-report.json` records 817,647 rounds and an
`os error 112` while writing the registered WAL seed fixture; the archived
input is `crash-00817647.bin`. This is an environment write failure, not a
completed 24-hour campaign or a confirmed parser crash. The run is invalid as
campaign evidence and needs a full retry on D: after the current external runs
finish. The fuzzer report, sample log, and input remain preserved in that run
folder.

Next process and sample check: 20:50 CEST.

## Monitoring update — 7 October 2026, 20:53 CEST

The active `cli_import_mapping`, `cli_migration_plan_json`, and
`storage_wal_payloads` processes were alive at 20:53:23 CEST. Their resource
files were updated through 20:53:02, 20:52:57, and 20:52:44 respectively;
none had a completion manifest:

| Run | PID | Status | Elapsed | CPU | RSS | Run output |
|---|---:|---|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | RUNNING | 22,388.3 s | 25,827.5 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | RUNNING | 21,882.3 s | 8,317.7 s | 39.1 MiB | 980.5 MB |
| `storage_wal_payloads` | 7348 | RUNNING | 21,842.4 s | 24,152.3 s | 38.6 MiB | 926.4 MB |

`storage_wal_recovery_prefix` remains invalid as campaign evidence after its
20:28 CEST fixture-write failure; its full retry is still queued for D: after
the current campaigns finish.

Next process and sample check: 21:08 CEST.


## Monitoring update — 7 October 2026, 21:23 CEST

The active cli_import_mapping, cli_migration_plan_json, and storage_wal_payloads processes were alive at 21:22:53 CEST. Their latest resource samples were updated through 21:22:26, 21:22:52, and 21:22:39 respectively; none had a completion manifest:

| Run | PID | Status | Elapsed | CPU | RSS | Run output |
|---|---:|---|---:|---:|---:|---:|
| cli_import_mapping | 27628 | RUNNING | 24,151.9 s | 27,885.6 s | 39.6 MiB | 980.5 MB |
| cli_migration_plan_json | 31928 | RUNNING | 23,677.8 s | 9,156.4 s | 39.1 MiB | 980.5 MB |
| storage_wal_payloads | 7348 | RUNNING | 23,637.8 s | 26,153.1 s | 38.6 MiB | 926.4 MB |

storage_wal_recovery_prefix remains invalid as campaign evidence after its 20:28 CEST fixture-write failure (os error 112); its full retry is queued on D: until the current campaigns finish.

Next process and sample check: 21:38 CEST.

## Monitoring update — 7 October 2026, 21:39 CEST

The active `cli_import_mapping`, `cli_migration_plan_json`, and
`storage_wal_payloads` processes were alive at 21:38:50 CEST. Their resource
samples were current through 21:38:46, 21:38:42, and 21:38:28 respectively;
none had a completion manifest:

| Run | PID | Status | Elapsed | CPU | RSS | Run output |
|---|---:|---|---:|---:|---:|---:|
| `cli_import_mapping` | 27628 | RUNNING | 25,132.5 s | 28,999.9 s | 39.6 MiB | 980.5 MB |
| `cli_migration_plan_json` | 31928 | RUNNING | 24,627.5 s | 9,593.7 s | 39.1 MiB | 980.5 MB |
| `storage_wal_payloads` | 7348 | RUNNING | 24,586.9 s | 27,191.8 s | 38.6 MiB | 926.4 MB |

`storage_wal_recovery_prefix` (PID 28428) remains invalid as campaign evidence
after its 20:28 CEST fixture-write failure (`os error 112`); its full retry is
queued on D: until the current campaigns finish.

Next process and sample check: 21:53 CEST.
