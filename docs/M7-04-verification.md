# M7-04 verification: unresolved migration decisions

**Status:** implemented and verified on Windows. Linux/macOS checks remain assigned to M9-07.

## Delivered behavior

- A dry-run report binds the plan fingerprint, migration category, transformer version, source schema revision/fingerprint, and a domain-separated fingerprint of the exact ordered source frames.
- Each unresolved item carries its source record index, closed reason, and fingerprint of that exact source frame. Error details and unresolved items share a bounded diagnostic-memory allowance; omitted unresolved details are counted and make the report incomplete.
- Warning, unresolved item, and transform/preflight error remain separate report states. Warnings do not resolve an item; an administrator decision cannot override a transform error.
- An explicit decision either supplies exact canonical record bytes or explicitly omits the source record. It is bound to the exact report, source schema, transformer version, ordered input fingerprint, and unresolved item.
- Validation requires exactly one decision for every retained unresolved item, rejects missing, duplicate, extra, stale, malformed, unauthorized, or over-budget decisions, and requires `MigrationExecute` for the supplied target scope. The validated object records the actor and effective-rights fingerprint.

`ValidatedMigrationDecisions` is a decision-validation result, not a commit permit. M7-05 and M7-10a must consume it while rechecking current authorization, source OCC, target-schema semantics, backup, audit, and commit requirements. The current version-1 transformer copies canonical records exactly and therefore emits no semantic unresolved items; unit tests construct internal unresolved reports to exercise the decision protocol until a transformer version can emit those outcomes.

## Evidence

- Ten migration dry-run tests pass. They cover plan/source/input binding; warning/unresolved/error separation; exact per-item completeness; duplicate, missing, stale, invalid, and unauthorized decisions; transform errors; and omitted unresolved details.
- `cargo test --locked --workspace`: PASS (480 core unit tests, 84 Rustdoc tests, all other workspace suites PASS; existing manual long campaigns remain ignored).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`, `cargo check --locked --workspace --all-targets`, Plancheck, Sourcecheck, and Git diff check: PASS.
- `cargo xtask verify`: 38 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL.

## Scope retained for later tasks

WDB-MIG-013 is evidenced here. WDB-MIG-014 remains owned by M7-10a, which must prove that restrictive/breaking execution cannot commit unresolved items without this validated decision set. This task does not add a durable decision wire record or execute a migration.
