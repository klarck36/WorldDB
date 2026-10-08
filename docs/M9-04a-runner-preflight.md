# M9-04a – Rust parser runner preflight

**Status:** PREFLIGHT COMPLETE; ONE LONG CAMPAIGN ACTIVE ON D:; REMAINING CAMPAIGNS INCOMPLETE
**Date:** 7 October 2026
**Platform:** Windows

The four Rust campaign entry points and shared parser harness were exercised
for every additional target assigned to M9-04a. Each smoke ran for one second
with the registered seed corpus and deterministic seed `0x574f524c44444232`.
They completed **35/35 routes**, 632,274 total parser calls, and **0 crashes**.
These are short route and seed checks; none counts as a 24-hour campaign.

The reports were generated under `%LOCALAPPDATA%\WorldDB\fuzz-results`
from a working tree with uncommitted M9-04a changes. The runner report's source
revision therefore names the base commit and is not authoritative for the
implementation. Long runs must use `tools/fuzz/run-target.ps1` from a clean
commit so the run manifest can bind the complete source-tree hash and seed
hashes. The runner now stores Rust LLVM LCOV or Node V8 coverage output and
archives complete crash inputs with per-file hashes. Separate one-second
artifact probes verified the Rust and TypeScript coverage outputs and the Rust
binary crash archive helper; these checks do not count as long campaigns.

| Target | Rounds | Accepted | Rejected | Corpus inputs |
| --- | ---: | ---: | ---: | ---: |
| `cli_arguments` | 22,244 | 3,478 | 18,766 | 4 |
| `cli_policy_history` | 440 | 0 | 440 | 1 |
| `cli_adapter_manifest` | 7,076 | 2,910 | 4,166 | 1 |
| `cli_adapter_payload` | 23,671 | 14,795 | 8,876 | 1 |
| `cli_adapter_handshake` | 20,924 | 8,504 | 12,420 | 1 |
| `cli_adapter_output` | 5,684 | 825 | 4,859 | 1 |
| `cli_adapter_frame` | 23,209 | 3,759 | 19,450 | 1 |
| `storage_format_probe` | 24,065 | 4,206 | 19,859 | 1 |
| `storage_current_manifest` | 365 | 52 | 313 | 2 |
| `storage_manifest_name` | 182 | 29 | 153 | 4 |
| `storage_history_segment` | 404 | 63 | 341 | 1 |
| `storage_security_segment` | 472 | 108 | 364 | 1 |
| `storage_wal_segment` | 15,963 | 4,621 | 11,342 | 1 |
| `storage_backup_manifest` | 20,155 | 312 | 19,843 | 9 |
| `storage_index_generation` | 23,027 | 4,042 | 18,985 | 1 |
| `storage_index_pointer` | 23,755 | 3,391 | 20,364 | 1 |
| `storage_index_generation_name` | 24,369 | 1,600 | 22,769 | 3 |
| `storage_compaction_pin_manifest` | 21,556 | 3,368 | 18,188 | 1 |
| `storage_migration_run_journal` | 21,955 | 3,667 | 18,288 | 1 |
| `storage_guarded_migration_journal` | 21,432 | 3,580 | 17,852 | 1 |
| `storage_storage_upgrade_journal` | 22,608 | 3,188 | 19,420 | 1 |
| `engine_ipc_request` | 22,132 | 5,694 | 16,438 | 1 |
| `engine_fact_command` | 21,840 | 5,615 | 16,225 | 1 |
| `engine_schema_command` | 21,057 | 5,361 | 15,696 | 1 |
| `engine_branch_layer_command` | 21,566 | 6,033 | 15,533 | 1 |
| `engine_entity_command` | 21,060 | 5,362 | 15,698 | 1 |
| `engine_perspective_command` | 21,302 | 5,422 | 15,880 | 1 |
| `engine_security_policy_command` | 22,726 | 5,844 | 16,882 | 1 |
| `engine_history_space_transfer_command` | 22,224 | 6,304 | 15,920 | 1 |
| `engine_query_cursor` | 23,276 | 2,187 | 21,089 | 2 |
| `engine_job_journal` | 20,744 | 2,638 | 18,106 | 1 |
| `engine_stream_id` | 24,434 | 2,255 | 22,179 | 2 |
| `desktop_sidecar_request_response` | 20,565 | 5,693 | 14,872 | 1 |
| `desktop_backup_dto` | 21,129 | 5,436 | 15,693 | 1 |
| `desktop_transfer_ids` | 24,663 | 2,279 | 22,384 | 2 |

`cli_policy_history` returned false for all smoke inputs because the M7-16h
fixture's security history does not extend through the fixture manifest's
committed revision. The decoder reports `IncompleteThroughCommittedRevision`;
the target completed without a crash. This is recorded as rejection, not as a
passing parse. The other binary and JSON seeds were corrected where necessary,
including the canonical adapter frames, format probe, history segment ID,
compaction pin manifest, perspective/transfer commands, query cursor, and job
journal. The adapter host now sends the canonical manifest frame once. Its reader
rebuilds a checked frame after validating the received payload, and the
separate-process round-trip passes.

## Long-run campaign status

The first `cli_arguments` attempt was terminated after 239 seconds because the
original output path was inside the OneDrive-synchronized workspace. Its
manifest remains `FAIL`; it is not campaign evidence. A valid retry then ran
outside OneDrive but failed on a parser timeout:

- Run ID: `M9-04-cli_arguments-574f524c44444232-20261007T123608Z`
- Source commit: `1b25d6e`; clean working tree
- Started: 7 October 2026, 14:36:08 CEST; 86,400 seconds requested
- Outcome: `FAIL` after 1,517.037 seconds and 16,223,900 rounds. One 41-byte mutation of seed line 3 (`v1 verify --database!C:\worlddb\synthetic`) exceeded the 2-second input timeout. Manifest, crash input, resource samples, and coverage remain under `%LOCALAPPDATA%\WorldDB\fuzz-results\M9-04-cli_arguments-574f524c44444232-20261007T123608Z`.
- Triage replay of that exact input used the same 2-second timeout for 10 seconds: 90,051 rounds, 0 crashes. The timeout was not reproduced; it is an unconfirmed scheduling/runner outlier, not a campaign pass or a confirmed parser defect. A further fresh 86,400-second retry remains required after the active campaigns finish and their resource evidence is collected.

A second M9-04a Rust campaign is running from the clean `7c5e1e3` source:

- Target: `engine_ipc_request`
- Run ID: `M9-04-engine_ipc_request-574f524c44444232-20261007T123909Z`
- Started: 7 October 2026, 14:39:09 CEST; 86,400 seconds requested

The first TypeScript long-run attempt (`M9-04-typescript.json_envelope-574f524c44444232-20261007T123909Z`)
failed before fuzzing because Node requires a `URL` object for a `file:` worker
module; the attempt is retained as `FAIL` and is not campaign evidence. The
worker now uses `new URL(import.meta.url)`, and the `typescript_cpu` input limit
is 67,108,880 bytes so the registered 64-MiB boundary seed fits. `pnpm verify`
and a one-second post-fix smoke passed with 24 parser calls, no crashes, and two
Node V8 coverage files. The corrected 86,400-second TypeScript campaign is
running:

- Run ID: `M9-04-typescript.json_envelope-574f524c44444232-20261007T124618Z`
- Source commit: `e53c8c7`; clean working tree
- Started: 7 October 2026, 14:46:18 CEST

## Pending campaign work

- Monitor the active `engine_ipc_request` and `typescript.json_envelope` runs.
  After the active campaigns finish and resource checks are collected, restart
  a full 86,400-second `cli_arguments` campaign, then cover the remaining 91
  core/TypeScript and 33 additional M9-04a targets using their registered
  profiles.
- Preserve each run manifest, seed hashes, fuzzer report, resource samples,
  and any crash inputs under `%LOCALAPPDATA%\WorldDB\fuzz-results`.
- Triage crashes and resource-limit outcomes before M9-04d closure.

## Monitoring update — 7 October 2026, 18:23 CEST

Both active M9-04a processes were alive at the check, with no completion
manifest. Their latest resource samples were written through 18:23 CEST:

| Run | PID | Elapsed | CPU | RSS | Run-folder size |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 13,425.9 s | 15,295.8 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 13,055.9 s | 12,844.7 s | 329.8 MiB | 14.8 KiB |

The separate `cli_arguments` retry remains queued until these active campaigns
finish and their final resource evidence is collected. Next process and sample
check: 18:38 CEST.

## Monitoring update — 7 October 2026, 18:38 CEST

Both active M9-04a processes were alive at the check, and neither run folder
contained a completion manifest. Resource samples were current through
18:38:22 CEST:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 14,282.6 s | 16,281.2 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 13,909.8 s | 13,687.3 s | 309.0 MiB | 15.8 KiB |

The separate `cli_arguments` retry remains queued until the active campaigns
finish and their final resource evidence is collected. Next process and sample
check: 18:53 CEST.

## Monitoring update — 7 October 2026, 19:02 CEST

Both active M9-04a processes were alive at the check, and neither run folder
contained a completion manifest. Resource samples were current through
19:02:26 CEST:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 15,738.0 s | 17,957.1 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 15,357.6 s | 15,115.5 s | 351.3 MiB | 17.4 KiB |

Next process and sample check: 19:17 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 19:21 CEST

Both active M9-04a processes were alive at the check. Neither run folder had a
completion manifest; resource samples were current through 19:21 CEST:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 16,846.7 s | 19,221.7 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 16,492.9 s | 16,231.1 s | 308.5 MiB | 18.3 KiB |

Next process and sample check: 19:36 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 19:45 CEST

Both active M9-04a processes were alive at 19:45:10 CEST. Their resource
samples were current through 19:44:53 CEST; neither run folder contained a
completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 18,273.7 s | 20,857.1 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 17,911.5 s | 17,627.8 s | 330.2 MiB | 20.3 KiB |

Next process and sample check: 20:00 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 20:00 CEST

Both active M9-04a processes were alive at 20:00:25 CEST. Their samples were
current through 20:00:06 CEST; neither run folder had a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 19,191.4 s | 21,915.9 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 18,824.5 s | 18,529.8 s | 330.1 MiB | 21.4 KiB |

Next process and sample check: 20:15 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 20:20 CEST

Both active M9-04a processes were alive at 20:19:58 CEST. Resource samples
were current through 20:20:04 CEST; neither run folder contained a completion
manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 20,393.7 s | 23,298.4 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 20,022.7 s | 19,712.1 s | 351.1 MiB | 22.7 KiB |

Next process and sample check: 20:35 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 20:33 CEST

Both active M9-04a processes were alive at 20:33:42 CEST. Samples were current
through 20:33:16 CEST; neither active run folder had a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 21,188.3 s | 24,179.7 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 20,811.9 s | 20,480.1 s | 351.6 MiB | 23.6 KiB |

Next process and sample check: 20:50 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 20:53 CEST

Both active M9-04a processes were alive at 20:53:23 CEST. Their resource files
were updated through 20:52:45 (`engine_ipc_request`) and 20:53:08
(`typescript.json_envelope`); neither run folder had a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 22,357.1 s | 25,528.0 s | 54.5 MiB | 1.21 GB |
| `typescript.json_envelope` | 18840 | 22,007.1 s | 21,660.5 s | 330.4 MiB | 24.4 KiB |

Next process and sample check: 21:08 CEST. The fresh `cli_arguments` retry
remains queued until the active campaigns finish.


## Monitoring update — 7 October 2026, 21:23 CEST

Both active M9-04a processes were alive at 21:22:53 CEST. Samples were current through 21:22:46 (engine_ipc_request) and 21:22:31 (typescript.json_envelope); neither had a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| engine_ipc_request | 30620 | 24,157.9 s | 27,605.2 s | 54.5 MiB | 1.21 GB |
| typescript.json_envelope | 18840 | 23,769.4 s | 23,394.1 s | 329.9 MiB | 26.3 KiB |

Next process and sample check: 21:38 CEST. The fresh cli_arguments retry remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 21:39 CEST

Both active M9-04a processes were alive at 21:38:50 CEST. Resource samples
were current through 21:38:38 (`engine_ipc_request`) and 21:38:50
(`typescript.json_envelope`); neither run had a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| engine_ipc_request | 30620 | 25,110.5 s | 28,676.2 s | 54.5 MiB | 1.21 GB |
| typescript.json_envelope | 18840 | 24,749.0 s | 24,344.9 s | 330.1 MiB | 28.1 KiB |

Next process and sample check: 21:53 CEST. The fresh cli_arguments retry remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 21:54 CEST

Both active M9-04a processes were alive at 21:53:04 CEST. Their samples were
current through 21:53:00 (`engine_ipc_request`) and 21:52:42
(`typescript.json_envelope`); neither run had a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| engine_ipc_request | 30620 | 25,972.4 s | 29,638.3 s | 54.5 MiB | 1.21 GB |
| typescript.json_envelope | 18840 | 25,580.8 s | 25,155.0 s | 330.0 MiB | 29.0 KiB |

Next process and sample check: 22:08 CEST. The fresh cli_arguments retry remains queued until the active campaigns finish.

## Monitoring update — 7 October 2026, 22:09:07 CEST

Both active M9-04a processes were alive at 22:09:07 CEST. Their latest
resource samples were written at 22:08:23 (engine) and 22:08:33 (TypeScript);
neither run folder contains a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| engine_ipc_request | 30620 | 26,895.3 s | 30,691.6 s | 47.8 MiB | 1,210,718,931 B |
| typescript.json_envelope | 18840 | 26,532.0 s | 26,094.3 s | 219.3 MiB | 30,077 B |

The fresh cli_arguments retry remains queued until the active campaigns finish.
C: has 65.86 GiB free. Next process and sample check: 22:24 CEST.

## Monitoring update — 8 October 2026, 12:56 CEST

Both active M9-04a processes were alive at 12:53 CEST. Samples were refreshed through 12:55:58; neither run folder contains a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 80,146.1 s | 91,893.6 s | 63.3 MiB | 1,210,785,669 B |
| `typescript.json_envelope` | 18840 | 79,776.2 s | 78,625.8 s | 220.6 MiB | 90,310 B |

C: had 367.43 GiB free and D: 215.81 GiB free. The fresh `cli_arguments` retry remains queued until these campaigns finish. Next process and sample check: 13:10 CEST.
## Monitoring update — 8 October 2026, 13:10 CEST

Both active M9-04a campaigns were still alive at 13:10 CEST. Their latest samples were written at 13:10:08; neither active run has a completion manifest:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 81,000.2 s | 92,875.4 s | 63.3 MiB | 1,210,786,739 B |
| `typescript.json_envelope` | 18840 | 80,626.4 s | 79,464.6 s | 220.6 MiB | 91,270 B |

C: remained at 367.43 GiB free and D: at 215.81 GiB free. `cli_arguments` stays queued until the active campaigns finish. Next check: 13:25 CEST.

## Monitoring update — 8 October 2026, 13:55 CEST

Both M9-04a campaigns remain active. Their resource samples were written at 13:55:12–13:55:25; neither run folder contains `completion.json`:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 83,717.4 s | 95,993.0 s | 63.3 MiB | 1,210,790,157 B |
| `typescript.json_envelope` | 18840 | 83,330.5 s | 82,136.7 s | 220.5 MiB | 94,342 B |

C: remains at 367.45 GiB free and D: at 215.81 GiB free. The sample files are about 91–102 KiB each; the queued `cli_arguments` retry remains unchanged. Next check: 14:10 CEST.

## Monitoring update — 8 October 2026, 14:17 CEST

Both M9-04a campaigns were still alive at 14:17 CEST. Their resource samples were refreshed at 14:17:00 and 14:16:40; neither run has `completion.json`:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 85,012.2 s | 97,481.9 s | 63.3 MiB | 1,210,791,787 B |
| `typescript.json_envelope` | 18840 | 84,650.4 s | 83,441.0 s | 220.6 MiB | 95,843 B |

C: had 367.43 GiB free and D: 215.81 GiB free. The `cli_arguments` retry remains queued pending completion and evidence collection. Next process and sample check: 14:32 CEST.


## Monitoring update — 8 October 2026, 14:36 CEST

Both M9-04a campaigns remained active. Samples were refreshed through 14:35:58 and 14:36:05; neither run has `completion.json`:

| Run | PID | Elapsed | CPU | RSS | Run output |
|---|---:|---:|---:|---:|---:|
| `engine_ipc_request` | 30620 | 86,150.1 s | 98,767.9 s | 63.3 MiB | 1,210,793,216 B |
| `typescript.json_envelope` | 18840 | 85,783.6 s | 84,551.2 s | 241.9 MiB | 97,133 B |

C: had 366.34 GiB free and D: 215.81 GiB free. The `cli_arguments` retry remains queued until current runs finish. Next process and sample check: 14:50 CEST.


## Monitoring update — 8 October 2026, 14:40 CEST

`engine_ipc_request` completed its 24-hour campaign. Its `fuzzer-report.json` records 86,400.000 s, 798,789,612 rounds, 205,597,273 accepted inputs, 593,192,339 rejected inputs, and no crashes; `run.stdout.log` reports the test as `ok`. `typescript.json_envelope` remains active; the latest sample at 14:40:16 records 86,034.7 s elapsed, 84,799.0 s CPU, 199.1 MiB RSS, and 97,421 B output, with no report yet.

C: had 366.34 GiB free and D: 215.81 GiB free. The `cli_arguments` retry remains queued. Next process and sample check: 14:50 CEST.

## Follow-up — 8 October 2026, 21:44 CEST

`engine_ipc_request` produced a 24-hour report with 798,789,612 rounds and no
crashes, but its manifest is `TIMEOUT`, so it is not a passing campaign.
`typescript.json_envelope` completed as `PASS_LOCAL` with 20,549,633 rounds and
no crashes. The `cli_arguments` retry failed after 1,517.037 seconds when a
mutation exceeded the 2-second input timeout. No M9-04a process is active.
These results supersede the 14:40 monitoring snapshot above.

## Safe retry — 8 October 2026, 21:59 CEST

`M9-04-cli_arguments-574f524c44444232-20261008T195913Z` is running from clean
commit `00c41b0`. Its output root and `CARGO_HOME` are on `D:`; host inventory
confirms `D:` is the separate WDC HDD while `C:` is the Samsung 970 EVO Plus
SSD. The `small_text` profile requests 86,400 seconds, permits 87,000 seconds
wall time, caps the run at 1 GiB RSS and 2 GiB temporary disk, and uses one
worker. The manifest is `RUNNING`; the Cargo build cache is confined to this
run's directory on D:. Between the preflight snapshots C: free space changed by
about 4 MiB while D: decreased by about 166 MiB during initial dependency/build
work. The next check is 9 October 2026, 22:15 CEST.
