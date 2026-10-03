# M8-12 – Branch, Layer, and content transfer verification

**Result:** PASS on Windows
**Platform follow-up:** Linux/macOS remains deferred to M9-07, as requested
**Contract:** `docs/architecture/M8-12-branch-layer-transfer-contract.md`

## Delivered behavior

- Authenticated project-wide HistorySpace and Layer catalogs support current, historical, and exact-revision views.
- Child HistorySpaces preserve their parent and historical cutoff. Creation is authorized against the selected parent and leaves existing branches and data unchanged.
- Layer creation and revision are append-only, audited, bound to the expected shared head, and validate a unique active base Layer. A base switch and its rank exchange publish atomically.
- The desktop offers explicit same-database HistorySpace content-transfer selection, preview, and confirmation. The host creates destination identities, rewrites internal references, checks external references and the event graph, and revalidates source/target heads and authorization before publication.
- Lifecycle effects omitted by a transfer preview require explicit acknowledgement. New copies start unarchived; later archive transitions remain strictly later. Transfer lineage uses the dedicated `TransferLineage` record where source `DerivedFrom` is forbidden.
- A committed transfer publishes copied records, reference mappings, lineage, policy history, required audit, and manifest through one WAL transaction. A stale or uncertain commit is not reported as success.
- Renderer input contains no filesystem paths, principals, destination IDs, or record bytes. Transfer plans remain host-side and are held by bounded, expiring, one-use tickets.

## Automated checks

- `cargo test --locked --workspace --all-targets` – PASS; 510 Core tests, storage 95 passed with one intentionally ignored 100,000-point NTFS campaign, plus the remaining workspace targets.
- `cargo test --locked --workspace --doc` – 87 Rustdoc tests passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – PASS.
- Storage end-to-end test `durable_transfer_rewrites_identity_owner_and_publishes_required_audit` – PASS; verifies durable copied content, remapped identities and owner, audit publication, and reopened storage verification.
- ODE-002 `cargo test --workspace --all-targets` – PASS in both in-process and sidecar profiles (15/16 desktop tests, 18 engine tests, and the sidecar transfer protocol test in each profile).
- ODE-002 strict Clippy – PASS in both in-process and sidecar profiles.
- Root and ODE-002 formatting checks, `node --check` for the desktop UI, and PowerShell parser validation for the Windows smoke script – PASS.
- Plancheck and Sourcecheck – PASS (252 tasks, 11 milestones, 253 invariants, 209 follow-up pairs; source mirrors and audit ZIP byte-match).
- Exception policy – PASS (29 local allow attributes; registered exceptions valid).
- `cargo xtask verify` – 39 PASS, one expected M0-14 `ci-matrix` SKIP, zero FAIL.

## Real Windows Tauri IPC

`experiments/ode-002/scripts/run-ipc-security-smoke.ps1` ran against a freshly built native Windows desktop application in both profiles. Each run used a fresh project and two authenticated native windows.

| Check | In-process | Sidecar |
|---|---:|---:|
| Authenticated primary and secondary windows | PASS | PASS |
| Project bootstrap and transactional schema/entity workflows | PASS | PASS |
| Branch and Layer creation, base switch, current/historical reads | PASS | PASS |
| Stale Branch write rejected without publication | PASS | PASS |
| Authenticated HistorySpace transfer catalog | PASS | PASS |
| Invalid session and renderer-selected path rejected in both windows | PASS | PASS |
| Filesystem plugin command rejected; host principal not selected by environment | PASS | PASS |
| Core network listener check and clean process shutdown | PASS | PASS |

The native smoke verifies the authenticated transfer catalog path. The durable content-copy commit is covered by the storage end-to-end test; the desktop commit path receives native content fixtures with the later record-entry work.

## Plan and source gates

The contract and this evidence are linked from `WorldDB_1.0_Taskregister.tsv`. The project remains on Windows for this task; Linux/macOS verification stays assigned to M9-07.
