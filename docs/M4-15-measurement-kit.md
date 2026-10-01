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

The probe's closed-descriptor cases must return `-1` with `EBADF`; retain their actual values. Before closing M4-15, also exercise the selected storage-backend sync abstraction with injected sync failures, including an I/O failure, and show that a failed required sync never reports Machine durability or a committed receipt. Keep injected failures clearly separated from errors observed on APFS hardware. The backend-level injection is still pending because the persistent storage backend is implemented in M5.

## Decision record

Use the measured API returns and latency distribution together with the device/OS metadata to decide the `Durability::Machine` behavior. The working recommendation remains: require the stronger full-sync operation where supported; reject writable Machine-durability mode if that guarantee cannot be established. Do not mark ODE-006 resolved from this probe alone: power-loss claims and backend failure handling need their own evidence.

References: [Apple `fcntl(2)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html), [Apple `fsync(2)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html), [Apple's disk-write guidance](https://developer.apple.com/documentation/xcode/reducing-disk-writes).
