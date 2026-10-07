# M9-04 – Long-running fuzz target inventory

**Status:** DONE
**Review:** 7 October 2026, Windows

## Inventory

The registry covers 92 core and TypeScript decoder targets plus 54 additional
foreign-data parser targets. The 54 additional targets route to four campaign
families:

| Runner family | Targets |
| --- | ---: |
| CLI and adapter protocol | 10 |
| Storage, import, and recovery | 27 |
| ODE engine | 11 |
| Desktop shell | 6 |
| **Total** | **54** |

The extra targets are classified as 1 CLI parser, 10 import parsers, 17
decoders, 5 adapter decoders, 9 recovery scanners, and 12 IPC decoders. Each
entry identifies source entry points, seed inputs, a resource profile, a runner,
and the follow-up campaign task. `tools/check_fuzz_targets.py` rejects missing
or duplicate targets, invalid source references, code files used as raw inputs,
oversized or malformed seeds, resource profiles shorter than 24 hours, and
incorrect task routing.

## Seed, resource, and run records

- `policy/decoder-inventory.tsv` and `policy/decoder-seeds.tsv` describe the 92
  existing core/TypeScript decoder targets and mutation strategies.
- `policy/fuzz-targets.tsv` describes all 54 additional parsers. Seeds are
  synthetic inputs or versioned M7-16h fixtures. The generated migration-run
  journal seed is archived as bytes and the core decoder test checks it against
  the deterministic generator.
- `policy/fuzz-runners.tsv` and `policy/fuzz-resource-profiles.tsv` define six
  runner configurations and seven resource profiles. Every campaign requests
  86,400 seconds with one worker; the profiles include input, process RSS, temp
  disk, per-input timeout, and wall limits.
- `tools/fuzz/run-target.ps1` writes versioned run manifests conforming to
  `docs/fuzz/run-manifest.schema.json`, including source revision, seed hashes,
  requested duration, resource samples, and result. Rust runs merge LLVM
  instrumentation into LCOV using the active Rust toolchain; TypeScript runs
  retain Node V8 coverage JSON. Parser failures archive the complete input
  bytes, and each run writes a hashed crash-corpus manifest. Results default to
  `%LOCALAPPDATA%\WorldDB\fuzz-results`, outside the synchronized workspace.

The core decoder and TypeScript transport runners already have executable
campaign tests. The four additional Rust runner entry points now cover all 54
additional targets. The M9-04a group passed a one-second route/seed preflight
with no crashes and 632,274 calls; M9-04b/c passed 19 more routes with no
crashes and 166,878 calls. The first `cli_arguments` run attempt was stopped
after 239 seconds because its output path was under OneDrive; its manifest is
retained as FAIL and it is not campaign evidence. The valid retry started on
commit `1b25d6e` at 14:36:08 CEST. Three more clean-commit Rust campaigns started
on `7c5e1e3`: `engine_ipc_request`, `cli_import_mapping`, and
`storage_wal_recovery_prefix`. All four Rust campaigns are currently RUNNING
under M9-04a/b/c and write outside the synchronized workspace. The initial
TypeScript long-run attempt failed on Node's worker URL type check and is not
evidence. The worker now receives `new URL(import.meta.url)`; the TypeScript
input cap was raised to 67,108,880 bytes for the registered 64-MiB envelope
boundary. The fix passed `pnpm verify` and a one-second smoke (24 parser calls,
no crashes, two V8 coverage files); its valid long-run retry is now RUNNING: `M9-04-typescript.json_envelope-574f524c44444232-20261007T124618Z`, clean commit `e53c8c7`, started 14:46:18 CEST.
M9-04a/b/c own the long runs; M9-04d owns crash triage and closure. Preflight
results are recorded in [M9-04a runner preflight](M9-04a-runner-preflight.md)
and [M9-04b/c runner preflight](M9-04bc-runner-preflight.md).

## Verification

- `python -B tools/check_fuzz_targets.py`: 92 decoder targets and 54 additional
  parser targets accepted.
- `python -B tools/test_fuzz_targets.py`: 9 tests passed, including missing
  target, duplicate target, malformed/oversized seed, shortened campaign, and
  wrong follow-up mutations.
- `cargo test --locked -p worlddb-core --test decoder_fuzz`: inventory and
  canonical-seed smoke passed; the 24-hour campaign remains ignored for its
  dedicated task.
- `pwsh -File tools/fuzz/run-target.ps1 -TargetId <id> -Seed
  0x574f524c44444232 -PlanOnly` passed for all **146 registered targets** across
  all six runner configurations.
- One-second Rust preflights: **54/54 additional targets**, 799,152 rounds,
  0 crashes. These are route/seed smokes only; they do not satisfy the
  24-hour campaigns.
- Artifact-path probes passed: a one-second Rust target produced three LLVM
  raw profiles and a 3.68 MB LCOV report; the TypeScript runner produced two
  V8 coverage JSON files. Rust crash-corpus helper tests preserve the full
  binary input byte-for-byte. These probes validate collection only and are
  not campaign evidence.
- `git diff --check`: passed.

The latest `cargo xtask verify` run after the import and recovery runners were
added ended with **43 PASS, 1 expected `ci-matrix` SKIP, and 2 FAIL**. Both
failures are the `public-contracts` check and its matching test: the frozen
`1.0.0-rc.1` fingerprint for `crates/worlddb-storage-file/src/manifest.rs`
does not match the current tracked source. The structured public surfaces and
the other three persistent-format fingerprints match. The RC baseline was not
rewritten as part of this fuzz work. Plancheck, fuzz-inventory checks, fmt,
strict Clippy, the new target dispatch/preflight runs, and artifact-path probes
pass. The first production campaign attempt was discarded; all valid 24-hour
campaigns remain pending.
