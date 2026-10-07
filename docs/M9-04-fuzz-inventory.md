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
  requested duration, resource samples, and result.

The core decoder and TypeScript transport runners already have executable
campaign tests. The four additional Rust runner test entry points are registered
for M9-04a implementation. Their campaigns have **not** been started; M9-04
completes the inventory and preparation only. M9-04a/b/c own the 24-hour runs;
M9-04d owns crash triage and closure.

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
- `cargo xtask verify`: **45 PASS, 1 expected `ci-matrix` SKIP, 0 FAIL**.
- `git diff --check`: passed.

The first full verify attempt exposed formatting and an unchecked test index in
the decoder harness. Both were fixed; the repeated full verify passed.
