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
| M5-08 | Linux/ext4 lock, sync, rename, directory-sync, and fault behavior | Use an ext4 checkout after the active M8-26b run releases the single RUNNING slot |
| M5-10 | macOS/APFS adapter behavior under the M4-15 policy | Implement the APFS full-sync path and test fail-closed behavior for unsupported/error results on macOS CI |
| M5-21 through M5-22a | Windows/NTFS profile, crash matrix, and local M6-M8 pre-gate passed 2026-10-02 | The existing windows_ntfs_fixed_local_v1 profile permits local development; it makes no cross-platform durability or power-loss claim |
| M5-23 | Full cross-platform storage gate | Complete after M4-15, M5-08, and M5-10 |

## Release rule

Deferred tasks remain visibly open until their evidence is complete. Windows results do not count as Linux or macOS evidence. The completed M5-22a pre-gate does not close M5-23. The full M5-23 gate and platform-specific tests remain mandatory before the RC architecture audit and final 1.0 publication.
