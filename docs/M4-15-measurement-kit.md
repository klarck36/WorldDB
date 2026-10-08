# M4-15 macOS/APFS measurement kit

The probe in `tools/m4-15/macos_sync_probe.c` is ready for a real macOS run. It refuses to run unless `statfs` reports APFS. For each sample it writes a fresh 4 KiB payload, times one `fsync` or `F_FULLFSYNC` call with a monotonic clock, and records the return value and `errno` in CSV. It also sends both operations an intentionally closed descriptor as an `EBADF` negative control.

The negative control checks API error reporting only. It is not an injected APFS device error and does not prove error propagation through the future storage backend. Do not treat successful calls or readback as proof of survival after power loss.

## Run on the target Mac

Choose a writable directory on the exact APFS volume and retain the output alongside machine metadata:

```sh
mkdir -p "$HOME/worlddb-m4-15"
clang -std=c11 -Wall -Wextra -Werror -O2 \
  tools/m4-15/macos_sync_probe.c -o "$HOME/worlddb-m4-15/macos_sync_probe"
sw_vers > "$HOME/worlddb-m4-15/metadata.txt"
sysctl -n hw.model >> "$HOME/worlddb-m4-15/metadata.txt"
diskutil info "$HOME/worlddb-m4-15" >> "$HOME/worlddb-m4-15/metadata.txt"
"$HOME/worlddb-m4-15/macos_sync_probe" "$HOME/worlddb-m4-15" 1000 \
  > "$HOME/worlddb-m4-15/samples.csv" \
  2> "$HOME/worlddb-m4-15/probe.txt"
```

Repeat on each target device class and macOS version. Preserve unedited CSV and metadata. Summarize sample count, failures and `errno` values, median, p95 and p99 latency for both operations. Record whether `F_FULLFSYNC` is rejected or returns an error separately from latency statistics. The 4 KiB probe measures API cost for this workload; it is not a general database benchmark.

Generate the machine-readable summary after preserving the raw CSV:

```sh
python3 tools/m4-15/summarize_sync_probe.py \
  "$HOME/worlddb-m4-15/samples.csv" \
  > "$HOME/worlddb-m4-15/summary.json"
```

The summarizer reports the median and nearest-rank p95/p99 latency for successful calls, groups failures by return value and `errno`, and exits nonzero if either closed-descriptor `EBADF` control does not return exactly `-1`/`EBADF`.

## Required error-injection evidence

The probe's closed-descriptor cases must return `-1` with `EBADF`; retain their actual values. Run 37772346504 completed the storage crate's sync-failure test suite successfully, providing injected storage-abstraction evidence separately from the APFS measurements. The Mac-specific adapter still needs to implement the chosen full-sync policy and cover unsupported-operation behavior in M5-10; these measurements do not establish that adapter behavior.

## Decision record

On the macOS 26.6.2 arm64 hosted APFS runner, all 1,000 `fsync` and all 1,000 `F_FULLFSYNC` calls succeeded. Median latency was 0.126 ms for `fsync` and 0.977 ms for `F_FULLFSYNC`; p95 was 0.387 ms and 2.963 ms; p99 was 11.594 ms and 11.234 ms. Both closed-descriptor controls returned `-1`/`EBADF`. The storage crate sync-failure tests also passed in run 37772346504.

Decision for ODE-006: use the stronger full-sync operation for `Durability::Machine` on APFS when supported; reject writable Machine-durability mode if that guarantee cannot be established. M4-15's measurement and policy decision are complete. M5-10 must implement and test this policy in the macOS adapter. This hosted probe does not prove survival after power loss or generalize to all Macs.

## Repeat on the corrected PR head

Run `37776375333` on PR head `ab1bdf2` also passed, including the storage sync-failure tests. Artifact `11550735391` is 13,375 bytes with SHA-256 `873b07754518a8981187bdde36549042feb4200b56d2977b67a5844c4fd72fa3`. It again identifies macOS 26.6.2 build 25G83, arm64 `VirtualMac2,1`, and APFS. All 1,000 `fsync` and 1,000 `F_FULLFSYNC` calls succeeded; median/p95/p99 were 0.067/0.195/0.337 ms for `fsync` and 0.615/1.188/1.857 ms for `F_FULLFSYNC`. Both negative controls returned `-1`/`EBADF`. The repeat confirms the earlier APFS measurement on this hosted runner; the ODE-006/M5-10 boundary remains unchanged.

References: [Apple `fcntl(2)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html), [Apple `fsync(2)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html), [Apple's disk-write guidance](https://developer.apple.com/documentation/xcode/reducing-disk-writes).
