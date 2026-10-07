# M9-04a – Rust parser runner preflight

**Status:** PREFLIGHT COMPLETE; LONG CAMPAIGNS ACTIVE; ONE TIMEOUT NOT REPRODUCED
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
manifest remains `FAIL`; it is not campaign evidence. The valid retry is
running outside OneDrive:

- Run ID: `M9-04-cli_arguments-574f524c44444232-20261007T123608Z`
- Source commit: `1b25d6e`; clean working tree
- Started: 7 October 2026, 14:36:08 CEST; 86,400 seconds requested
- Outcome: `FAIL` after 1,515.322 seconds and 16,223,900 rounds. One 41-byte mutation of seed line 3 (`v1 verify --database!C:\worlddb\synthetic`) exceeded the 2-second input timeout. Manifest, crash input, resource samples, and coverage remain under `%LOCALAPPDATA%\WorldDB\fuzz-results\M9-04-cli_arguments-574f524c44444232-20261007T123608Z`.
- Triage replay of that exact input used the same 2-second timeout for 10 seconds: 90,051 rounds, 0 crashes. The timeout was not reproduced; it is an unconfirmed scheduling/runner outlier, not a campaign pass or a confirmed parser defect. A fresh 86,400-second retry remains required and will start after current campaign/resource checks.

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

- Monitor the active `cli_arguments`, `engine_ipc_request`, and
  `typescript.json_envelope` runs, then cover the remaining 91 core/TypeScript
  and 33 additional M9-04a targets using their registered profiles.
- Preserve each run manifest, seed hashes, fuzzer report, resource samples,
  and any crash inputs under `%LOCALAPPDATA%\WorldDB\fuzz-results`.
- Triage crashes and resource-limit outcomes before M9-04d closure.
