# M4-15 environment check — ODE-006

## Local environment found

- Host OS: Microsoft Windows 11 Home.
- Available Linux environment: WSL2 Ubuntu.
- No macOS host or APFS volume is attached to this workspace.

## Blocker

M4-15 requires measurements of `fsync` and `F_FULLFSYNC` on APFS hardware, with error injection and recorded API returns. Windows/NTFS and WSL/Linux cannot provide evidence about Apple’s APFS sync path or real-device durability, so this task cannot make the ODE-006 choice from the current machine. A macOS-only probe and run protocol are now prepared in `tools/m4-15/macos_sync_probe.c` and `docs/M4-15-measurement-kit.md`; neither substitutes for the hardware run. The probe is intentionally macOS-only and has not been compiled on this Windows host; the first build is part of the target-Mac runbook.

## Resume requirement

Run the measurement kit on a real macOS/APFS system and record its OS/build, APFS device class, sync operations and return values, latency distribution, and injected failure outcomes. The probe includes a closed-descriptor `EBADF` negative control; backend-level injected I/O failure evidence remains required once the M5 sync abstraction exists. Then decide whether `Durability::Machine` uses Full Sync when supported and how the backend behaves when it is unavailable. No GitHub connection is needed; the blocker is the absent macOS/APFS test host.
