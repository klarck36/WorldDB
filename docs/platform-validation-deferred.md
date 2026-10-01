# Deferred platform validation

The user directed that work requiring Linux or macOS be verified later, after Windows work can proceed. This scheduling decision does not waive platform requirements or permit unsupported durability claims.

## Current execution rule

- Windows/NTFS work and tests run on the current host.
- Linux/ext4 and macOS/APFS implementation validation, hardware measurements, and CI evidence remain open until a suitable host is available.
- M5-22a passed its Windows evidence review on 2026-10-02 and releases local M6-M8 development; it does not complete the full M5-23 platform gate.
- A platform without a completed durability profile must not open for Machine-durable writes. Its adapter reports the missing capability or rejects writable open.
- The deferred platform evidence remains a prerequisite for the final storage support matrix and M5 release gate.

## Deferred tasks

| Task | Deferred evidence | Work that can proceed on Windows |
|---|---|---|
| M0-14 | External Linux/macOS CI artifacts and provider run | Local CI contract, Windows verification, and feature matrix |
| M4-15 | APFS `fsync`/`F_FULLFSYNC` measurements and injected failures | M4 local gate with unverified platforms fail-closed |
| M5-08 | Linux/ext4 lock, sync, rename, and fault behavior | Independent Windows adapter and format work |
| M5-10 | macOS/APFS adapter and ODE-006 decision | Windows adapter, generic recovery, and storage logic |
| M5-21 through M5-22a | Windows/NTFS profile, crash matrix, and local M6–M8 pre-gate passed 2026-10-02 | `windows_ntfs_fixed_local_v1` allows local M6–M8 development; this gives no Machine-durability or power-loss claim |
| M5-23 | Full cross-platform storage gate | Remains open until M4-15, M5-08, and M5-10 are complete; required before M9-13b and M10-10 |

## Release rule

Deferred tasks remain visibly open. Windows results do not count as Linux or macOS evidence. The completed M5-22a pre-gate does not close M5-23. The full M5-23 gate and platform-specific tests remain mandatory before the RC architecture audit and final 1.0 publication.
