# M4-15 environment check — ODE-006

## Local environment found

- Host OS: Microsoft Windows 11 Home.
- Available Linux environment: WSL2 Ubuntu.
- No macOS host or APFS volume is attached to this workspace.

## Execution path

A SHA-pinned GitHub Actions workflow, `.github/workflows/m4-15-apfs-sync.yml`, runs the prepared probe on `macos-latest`. It checks the evidence directory's APFS personality, records macOS build and hardware model, retains diskutil metadata and raw CSV, summarizes `fsync`/`F_FULLFSYNC` returns and latency, and runs the storage crate's injected sync-failure tests. The first attempt, 37771970754, showed that BSD `stat -f %T` reports a file type, not a filesystem type. The follow-up, 37772238303, showed that `diskutil info` needs the volume device rather than a directory path. The workflow now resolves the device with `df` before querying its `File System Personality`; the APFS probe independently verifies the target path with `statfs`.

The corrected retry 37772346504 completed successfully on commit `0cc7db3`. Its artifact (ID 11548094744, SHA-256 `aa865468175e690e1a7224a72151d554da9ea9e4cd885a54c0ddada9aed9832c`) confirms macOS 26.6.2 build 25G83, an arm64 `VirtualMac2,1` hosted runner, and APFS. All 1,000 `fsync` calls and all 1,000 `F_FULLFSYNC` calls succeeded. Median latency was 0.126 ms and 0.977 ms respectively; p95 was 0.387 ms and 2.963 ms; p99 was 11.594 ms and 11.234 ms. Both closed-descriptor negative controls returned `EBADF` (errno 9), and the workflow's injected storage sync-failure tests passed. M4-15's runner evidence and measurement acceptance are complete.

The hosted runner can provide evidence about that runner's APFS profile and API behavior. Its measurements do not prove persistence across power loss or generalize to every Mac model.

## Remaining acceptance limits

M4-15 reports the OS/build, APFS volume, sync operations and return values, latency distribution, and injected storage failure outcomes. The recorded ODE-006 policy is to require `F_FULLFSYNC` for `Durability::Machine` on APFS when supported, and to fail closed for writable Machine-durability mode when that guarantee cannot be established. M5-10 implements and verifies this policy in the macOS adapter. The hosted measurement describes this runner and does not prove persistence across power loss or generalize to every Mac model. The current machine itself still has no Mac/APFS volume.

The probe in tools/m4-15/macos_sync_probe.c and run protocol in docs/M4-15-measurement-kit.md remain the source of the measurement definition.
