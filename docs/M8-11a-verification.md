# M8-11a – Schema management verification

**Result:** PASS on Windows
**Platform follow-up:** Linux/macOS remain deferred to M9-07, as requested
**Implemented surface:** transactional storage manager, typed ODE engine API, Tauri IPC, historical schema UI, typed definition forms, lifecycle staging

## Behavior delivered

- Current, historical, and explicit schema snapshots are shown from the shared revision history.
- EntityTypes, predicates, and event kinds can be created with host-generated stable identities, typed value/role/attribute details, constraints, and decimal/time metadata.
- Lifecycle changes are staged and published as one audited batch. The UI supports the required `Active → Deprecated → Retired` progression and can retire dependent schema definitions atomically.
- Reads require current `SchemaRead`; writes additionally require current `SchemaManage`. Stale revisions and invalid references are rejected before publication.
- Structural schema changes remain on the migration path. Layer definitions remain visible but are managed by the Branch/Layer task.

## Test results

- `cargo test --locked -p worlddb-storage-file schema_management::tests -- --nocapture` – 3 PASS: audit/history, missing manage permission, and atomic retirement of an EntityType with its dependent predicate.
- `cargo test --locked --manifest-path experiments/ode-002/Cargo.toml -p worlddb-ode-engine --all-targets` – 13 engine unit tests and 1 sidecar protocol integration test PASS.
- `schema::tests::every_typed_constraint_draft_preserves_its_closed_value_kind` – all nine closed constraint forms PASS, including signed/unsigned 128-bit bounds.
- `schema::tests::schema_commands_have_stable_closed_json_shapes` – closed DTO and sidecar request/response round trips PASS.
- `cargo test --locked --workspace --all-targets` – PASS; only previously designated long-running/manual campaigns remain ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – PASS.
- ODE-002 strict Clippy for both `in-process` and `sidecar` profiles – PASS.
- Root and ODE-002 formatting checks – PASS. `node --check` and PowerShell smoke-script parse – PASS.

## Real Windows IPC

The updated `experiments/ode-002/scripts/run-ipc-security-smoke.ps1` was run against a built Tauri app in both modes. Each run created a fresh project, drove the actual schema form and lifecycle UI functions, read current/historical/explicit snapshots, and confirmed the secondary window could read the same schema. Both runs also repeated the existing invalid-session, renderer-path, filesystem-plugin, network-listener, host-principal, distinct-snapshot, and clean-shutdown checks.

| Check | In-Process | Sidecar |
|---|---:|---:|
| Authenticated schema form creates a definition | PASS | PASS |
| Historical and explicit snapshots preserve the expected views | PASS | PASS |
| UI lifecycle flow publishes Deprecated then Retired | PASS | PASS |
| Secondary window reads the shared current schema | PASS | PASS |
| Existing IPC security and shutdown checks | PASS | PASS |

## Source and plan gates

The final gate run for this task records `WorldDB_1.0_Plancheck.py`, `WorldDB_1.0_Sourcecheck.py`, `cargo xtask verify`, and `git diff --check`. The Taskregister marks M8-11a complete and M8-11b ready to start.
