# Deferred platform validation

The user directed that work requiring Linux or macOS be verified later, after Windows work can proceed. This scheduling decision does not waive platform requirements or permit unsupported durability claims.

## Current execution rule

- Windows/NTFS work and tests run on the current host.
- Linux/ext4 and macOS/APFS implementation validation, hardware measurements, and CI evidence remain open until a suitable host is available.
- A platform without a completed durability profile must not open for Machine-durable writes. Its adapter reports the missing capability or rejects writable open.
- The deferred platform evidence remains a prerequisite for the final storage support matrix and M5 release gate.

## Deferred tasks

| Task | Deferred evidence | Work that can proceed on Windows |
|---|---|---|
| M0-14 | External Linux/macOS CI artifacts and provider run | Local CI contract, Windows verification, and feature matrix |
| M4-15 | APFS `fsync`/`F_FULLFSYNC` measurements and injected failures | M4 local gate with unverified platforms fail-closed |
| M5-08 | Linux/ext4 lock, sync, rename, and fault behavior | Independent Windows adapter and format work |
| M5-10 | macOS/APFS adapter and ODE-006 decision | Windows adapter, generic recovery, and storage logic |
| M5-21–M5-23 | Cross-platform write-support matrix and crash gate | Windows/NTFS candidate is recorded in `docs/M5-21-windows-ntfs-profile.md`; M5-22 Windows crash evidence can proceed; final gate stays open |

## Release rule

Deferred tasks remain visibly open. Windows results do not count as Linux or macOS evidence. Final 1.0 support claims require the platform-specific tests and measurements from the original plan.
