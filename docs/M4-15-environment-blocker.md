# M4-15 environment check — ODE-006

## Local environment found

- Host OS: Microsoft Windows 11 Home.
- Available Linux environment: WSL2 Ubuntu.
- No macOS host or APFS volume is attached to this workspace.

## Execution path

A SHA-pinned GitHub Actions workflow, .github/workflows/m4-15-apfs-sync.yml, runs the prepared probe on macos-latest. It checks that the evidence directory is APFS, records macOS build and hardware model, retains diskutil metadata and raw CSV, summarizes fsync/F_FULLFSYNC returns and latency, and runs the storage crate's injected sync-failure tests. It was held until the active M8-26b APFS baseline finished; run 37767019927 has now completed, so the workflow is ready to run with the current macOS fixes.

The hosted runner can provide evidence about that runner's APFS profile and API behavior. Its measurements do not prove persistence across power loss or generalize to every Mac model.

## Remaining acceptance limits

M4-15 requires the measurement kit to report the OS/build, APFS volume, sync operations and return values, latency distribution, and injected failure outcomes. The probe includes a closed-descriptor EBADF negative control; backend-level injected I/O failure evidence comes from the storage tests. Then decide whether Durability::Machine uses Full Sync when supported and how the backend behaves when it is unavailable. No GitHub connection is required after the workflow result is collected; the current machine itself still has no Mac/APFS volume.

The probe in tools/m4-15/macos_sync_probe.c and run protocol in docs/M4-15-measurement-kit.md remain the source of the measurement definition.
