# M8-11b – Entity management verification

**Result:** PASS on Windows
**Platform follow-up:** Linux/macOS remains deferred to M9-07, as requested
**Delivered:** audited Entity catalog storage, typed ODE/sidecar API, authenticated Tauri IPC, historical/current/explicit UI, two-window refresh

## Behavior delivered

- Entities are created with a host-generated identity and one permanent EntityType assignment. The interface has no implicit name or generic metadata field; it directs names and facts to typed Assertions.
- Current, historical, and exact shared-revision views show Entities together with their EntityType and lifecycle at that revision.
- Deprecated types require explicit opt-in and the additional current `SchemaManage` permission; a typed warning is returned. Retired EntityTypes reject new Entities.
- Retirement is terminal and append-only. It is audited on the same shared revision axis and leaves prior catalog and domain history intact.
- Entity identity values remain internal to the UI and are used only in typed IPC requests.

## Automated checks

- `cargo test --locked -p worlddb-storage-file schema_management::entity::tests -- --nocapture` – 5 PASS: create/retire history and audit, Deprecated/Retired type behavior, read/create permissions, stale live-head conflict, and retirement permission denial.
- `cargo test --locked --manifest-path experiments/ode-002/Cargo.toml -p worlddb-ode-engine` – 15 PASS, including closed Entity request/response JSON shapes.
- `cargo test --locked --workspace --all-targets` – PASS on Windows.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – PASS.
- ODE-002 `cargo test --workspace --all-targets` – PASS in both default in-process and `--no-default-features --features sidecar` profiles.
- ODE-002 strict Clippy – PASS in both in-process and sidecar profiles.
- Root and ODE-002 formatting checks, `node --check` for the desktop UI, and PowerShell parser validation for the Windows smoke script – PASS.

## Real Windows Tauri IPC

`experiments/ode-002/scripts/run-ipc-security-smoke.ps1` ran against the built native desktop app in both profiles. Each run used a fresh project and two authenticated native windows.

| Check | In-process | Sidecar |
|---|---:|---:|
| Authenticated primary and secondary windows | PASS | PASS |
| Create Active and Deprecated EntityType Entities | PASS | PASS |
| Explicit Deprecated warning | PASS | PASS |
| Current, historical, and explicit Entity views | PASS | PASS |
| Terminal retirement with preserved historical state | PASS | PASS |
| Secondary-window Entity catalog refresh | PASS | PASS |
| Existing IPC security, network-listener, and clean-shutdown checks | PASS | PASS |

## Plan and source gates

The final task record also references the successful Plancheck, Sourcecheck, `cargo xtask verify`, and `git diff --check` runs. Linux/macOS verification remains assigned to M9-07.
