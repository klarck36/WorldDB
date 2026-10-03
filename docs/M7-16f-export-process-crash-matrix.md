# M7-16f – Logical and Sharing Export process-crash matrix

**Checked:** 2026-10-03, Windows

## Scope

The tests run export operations in child processes and terminate them at five lifecycle boundaries. The test harness writes a sentinel artifact only after the exporter returns; every child killed before return leaves that path absent. The parent reopens each database, runs recovery, and checks storage, audit, and pin state.

## Crash points

| Export and child exit | Reopen result |
|---|---|
| Logical Export: `pin_acquired`, before history reads | No artifact path exists. Recovery is clean. Compaction detects the process-released file lock, removes the stale durable pin, and reclaims exactly the two replaced History segments. |
| Logical Export: `before_return`, after full artifact construction and validation | No artifact path exists even though a complete export had been built in memory. Recovery is clean, and compaction again removes the stale pin and reclaims exactly the replaced History segments. |
| Sharing Export: after `ExportAuthorization` WAL commit, before recovery | No artifact path exists. Recovery reports a clean committed prefix with exactly one successful `ExportAuthorization`; there is no `ExportCompletion`. |
| Sharing Export: after the Logical Export returns, before filtering/completion | No artifact path exists. Recovery is clean with exactly one `ExportAuthorization`; the logical durable pin has already been released and the pin directory is empty. |
| Sharing Export: after `ExportCompletion` commits and verifies, before API return | No artifact path exists. Recovery is clean with exactly two audits in order: `ExportAuthorization` then `ExportCompletion`, sharing one `AuditOperationId`. The completed artifact was materialized and validated before the completion boundary, but process exit prevented the caller from receiving it. |

Logical Export does not publish a file itself; the sentinel verifies that a caller cannot write one before a complete return value exists. A process exit while its pin guard is live leaves an unlocked lease that the normal reclamation scan removes after reopen. A process exit after the Logical Export returns observes the RAII lease already removed.

Sharing Export keeps its two durable audit boundaries exact. A crash after authorization recovers only that boundary. A crash after successful completion recovers both records, never a partial audit record. Existing error coverage also verifies that failure after authorization returns no artifact and writes no completion audit.

## Windows verification

- `cargo test --locked -p worlddb-storage-file --lib logical_export::tests:: -- --nocapture` — 14 passed, 0 failed.
- `cargo test --locked -p worlddb-storage-file --lib sharing_export::tests:: -- --nocapture` — 6 passed, 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- `cargo xtask verify` — 38 passed, 1 expected M0-14 `ci-matrix` skip, 0 failed.
- Plancheck, Sourcecheck, format, and whitespace checks — passed.

Linux/macOS execution is deferred to M9-07 per project instruction.
