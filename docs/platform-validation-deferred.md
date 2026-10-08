# Open platform validation

The earlier scheduling deferral for Linux and macOS work was superseded on 7 October 2026 by the user's instruction to complete the full plan. Platform-specific tasks are active again. This does not relax the evidence requirements or permit unsupported durability claims.

## Current execution rule

- Windows/NTFS work and tests run on the current host.
- Linux/ext4 evidence must come from a checkout and evidence directory confirmed as ext4, using WSL2 or a hosted Linux runner.
- macOS/APFS evidence runs on a macOS runner and must record the detected filesystem and host metadata.
- The M4-15 probe measures APFS API returns and latency; it does not prove survival after power loss. Keep that limit explicit and keep Machine durability fail-closed until the applicable profile is supported.
- The completed M5-22a pre-gate releases local M6-M8 development; it does not complete the full M5-23 platform gate.
- A platform without a completed durability profile must not open for Machine-durable writes.

## Platform task status

| Task | Required evidence | Current next action |
|---|---|---|
| M0-14 | External Linux/macOS CI artifacts and provider run | Run the full GitHub Actions matrix on the current product head after platform validation |
| M4-15 | APFS fsync/F_FULLFSYNC measurements, injected sync failures, and ODE-006 decision | Complete: run 37772346504 and artifact 11548094744 record measurements, negative controls, storage failure tests, and the policy decision in `docs/M4-15-environment-blocker.md` |
| M5-08 | Linux/ext4 lock, sync, rename, directory-sync, and fault behavior | Run the storage library and M5-02 writer-lock contracts in the ext4-verified M8-26c hosted workflow |
| M5-10 | macOS/APFS adapter behavior under the M4-15 policy | The F_FULLFSYNC path and focused APFS/error tests are implemented locally; run the APFS workflow on the updated head |
| M5-21 through M5-22a | Windows/NTFS profile, crash matrix, and local M6-M8 pre-gate passed 2026-10-02 | The existing windows_ntfs_fixed_local_v1 profile permits local development; it makes no cross-platform durability or power-loss claim |
| M5-23 | Full cross-platform storage gate | Complete after M4-15, M5-08, and M5-10 |

## Latest native platform run — 8 October 2026

GitHub Actions run `37830005391` used PR head `771ffa1cd4e126f727d3e4234cc78033a3c628b8`.
Its clean merge checkout was `64ec5afd4b53d856b46323e98e4ab5859e630208`.

- M8-26b succeeded on macOS 26.6.2 arm64/APFS: all 14 cases passed. Artifact
  `11573963311`, SHA-256
  `b1bd9f6615447796939751724397bd74d67cc2baa2ca47a793a0d329900e0a9a`.
- M8-26c verified both the checkout and runner temp directory as ext4, then
  failed before native cases because the in-process Tauri build selected no
  `rfd` dialog backend. The manifest records 2 PASS, 1 FAIL, and 8 NOT_RUN.
  Artifact `11573884041`, SHA-256
  `ef92e6f9dfb4f4f7cfd2afca0019192abb5ec5e91d4380059bb83a5d059f465d`.
  The desktop-shell manifest now explicitly enables the GTK3 backend; the next
  M8-26c run also executes the storage and writer-lock contracts on ext4.

## Follow-up native run — 8 October 2026

Run `37834652977` tested head `8bf75adb49a425414351ffca1389e009a511250c`.
The macOS job stopped at `Fetch locked Rust dependencies`; the native cases did
not run and the Linux job was skipped. Enabling `gtk3` and adding the macOS
`libc` adapter dependency required four additional locked dependency entries.
They are now recorded in `experiments/ode-002/Cargo.lock`. Locked, dependency-
free Cargo metadata and `git diff --check` pass; no local build was run. A new
hosted run is pending this lockfile correction.

## Release rule

Deferred tasks remain visibly open until their evidence is complete. Windows results do not count as Linux or macOS evidence. The completed M5-22a pre-gate does not close M5-23. The full M5-23 gate and platform-specific tests remain mandatory before the RC architecture audit and final 1.0 publication.
