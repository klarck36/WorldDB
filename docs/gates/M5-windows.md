# M5-22a — Windows local development pre-gate

**Result:** PASS for local M6–M8 development on the Windows/NTFS candidate profile only. Checked 2026-10-02T01:29:52+02:00.

## Scope and evidence review

The review covered the completed Windows-scoped storage work M5-01 through M5-07, M5-09, and M5-11 through M5-22, including the candidate profile `windows_ntfs_fixed_local_v1` and the registered crash matrix. The checked profile is Windows 11 Home Build 26200, x86_64-pc-windows-msvc, on a fixed local NTFS volume. Database test data is kept on local %LOCALAPPDATA% storage; the OneDrive workspace is not treated as the database volume.

The versioned M5-22 record, docs/M5-22-windows-crash-matrix.md, reports the deterministic 100,000-point recovery campaign as PASS across 1,000 databases and records the five-point distribution [20_037, 19_920, 19_762, 20_059, 20_222]. It distinguishes hook-injected recovery interruption from operating-system process crashes and does not claim hardware power-loss coverage.

## Verification on this review

- cargo test --locked -p worlddb-storage-file --all-targets --quiet: 105 passed, 0 failed, 1 ignored. The ignored test is the separate 100,000-point campaign.
- cargo xtask verify: 34 passed, 1 expected skip, 0 failed. The skip is the deferred M0-14 ci-matrix provider run. The M5 storage crash-contract step passed with 30 tests and 1 ignored long campaign.
- cargo clippy --locked -p worlddb-storage-file --all-targets -- -D warnings: PASS.
- Workspace Clippy, format, workspace check, policy, dependency, TypeScript transport, and contract-source steps in cargo xtask verify: PASS.
- cargo fmt --all -- --check, WorldDB_1.0_Plancheck.py, WorldDB_1.0_Sourcecheck.py, tools/check_ci_matrix.py, and git diff --check HEAD: PASS.
- M0-13 evidence run M0-13-20261001T232157Z-67f8803119: PASS.
- Original contract source SHA-256: d5ba2016741f4475c982039d7c9022b1e8fb037e7307de99b3e80e7471ef2064.

A fresh repeat of the 100,000-point campaign was started during this review and stopped before completion after an extended run. That attempt is not counted as a pass or as gate evidence; this review relies on the previously completed, versioned M5-22 result cited above.

## Platform and durability boundary

This pre-gate does not complete M5-23. At the time of this pre-gate, M4-15 (APFS measurements), M5-08 (Linux/ext4), and M5-10 (macOS/APFS) remained open. M4-15 later completed on 8 October 2026 with APFS runner measurements and the ODE-006 policy; M5-08 and M5-10 still require their adapter evidence. No hardware power-loss result, device-cache-loss result, or production file-backend approval is granted here. The current file-storage crate does not implement StorageBackend; ProductionStorage::try_new continues to require Machine durability.

## Gate decision

M5-22a is DONE and releases local M6–M8 development on the reviewed Windows candidate profile. M6-01 is READY. M5-23 remains PLANNED and mandatory before M9-13b, the RC gate, and M10-10 publication.
