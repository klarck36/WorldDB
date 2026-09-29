# ADR-033 – Versioned security policy and capability evaluation

**Status:** Accepted for the WorldDB 1.0 working contract

**Task:** M0-04c

**Decision owner:** Luna (contract author)

**Normative detail:** [Security policy supplement](security_policy_contract.md)

## Context

Master §17 fixes the security order and scope levels, and ADR-017 makes `AuthorizationNow` the default for historical data. The cursor and audit contracts already bind `SecurityEpoch` and require durable audit for policy changes, but the policy record shape, capability composition, scope matching, and field/relationship evaluation were not explicit. M0-04 also requires Entity and Perspective actions to remain distinct authorization operations.

## Decision

1. Security policy is append-only revisioned project history in a separate security namespace. Typed `SecurityPolicyRecord`s, principals, roles, assignments, and closed CapabilityRules commit with ordinary transactions; they are not Domain `RecordRef`s.
2. `Capability` is a closed typed catalog. Principal and Role grants share exact scope matching; any matching Deny overrides, absent Allow denies, and there are no implicit grants, role hierarchy, wildcard strings, UI privileges, or name-based GM/Player bypasses.
3. Record, field, relationship, and operation rights compose as explicit AND requirements. Security filtering occurs before candidate creation, redaction, Masking, Resolution, aggregation, Explain, search, graph expansion, or output.
4. Historical authorization is either current `AuthorizationNow` or explicitly privileged `AuthorizationAtRevision`. `SecurityEpoch` increments once per policy-changing transaction and is atomically bound to policy history, required audit, snapshots, and cursor invalidation.
5. Every committed policy change carries a durable AuditRecord in the same commitpoint. Raw-read audit keeps its separate sequence/WAL and does not create a WorldDB data revision.

## Consequences

- M0-05 must create an explicit initial Principal and policy bundle during project bootstrap; there is no unrecorded superuser path.
- M3 implements capability evaluation and non-interference; M4 revalidates rights at commit; M5 persists policy history; M6 proves paired-world security; M8 exposes only versioned DTOs that preserve these decisions.
- Any future operation or relationship kind adds an explicit Capability variant and compatibility review. Unknown policy or scope tags fail closed.

## Verification obligations

Use paired-world checks for hidden records, fields, and edges; truth tables for allow/deny/role composition; revision vectors for historical permission modes and `SecurityEpoch`; and injected failures around policy/audit commit to prove no partial update. CLI, API, IPC, and desktop requests must exercise the same engine authorization contract.
