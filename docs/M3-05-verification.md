# M3-05 verification: historical security

## Decision and implementation

`AuthorizationNow` evaluates the policy projection at the latest committed revision and remains the default in `SecurityContext`. Historical data queries therefore continue to use current policy; an old data revision does not restore a permission that is currently denied.

`SecurityPolicyHistory` holds a complete policy projection for every committed revision. Construction fails closed if genesis is absent, any committed revision is missing, the history does not reach its declared commit, or an epoch transition is invalid. Policy epochs remain stable on ordinary commits and advance by exactly one on a policy-changing commit.

`AuthorizationAtRevision(r)` is selected explicitly. The current policy must first grant `SecurityPermissionHistoryRead` at project scope. The selected policy is then exactly the projection at `r`; future/uncommitted revisions, incomplete history, and missing permission return typed errors without fallback. `SecurityPolicyView` exposes both the current and evaluated epochs for snapshot/cursor binding.

The authorized raw-history query now resolves policy from the same `QueryContext` used for data revision and schema binding, before constructing any returned rows. Candidate and projection authorization can consume the `SecurityPolicyView::snapshot()` selected by the same history resolver.

## Verification

- `security::tests::historical_policy_is_explicit_admin_gated_and_now_never_revives_rights`: current `AuthorizationNow` denies a right that the old policy allowed; explicit, currently authorized `AtRevision` evaluates the old projection and exposes both epochs.
- `security::tests::historical_policy_requires_current_permission_and_complete_genesis_history`: historical permission denial, missing genesis, and a missing committed revision all fail closed.
- `reference_query::tests::historical_binding_repeats_exactly_for_the_same_data_and_schema_snapshots`: unchanged M2-16 regression for identical data revision, `SchemaMode`, schema revision, and fingerprint.
- `reference_query::tests::raw_full_scan_pins_recorded_revision_and_returns_owned_canonical_rows`: unchanged canonical ordering and data-revision regression.

## Scope

This is an in-memory reference history model. Durable policy-record reconstruction, crash recovery, and cursor invalidation are follow-ups M5-06a, M5-12, and M5-22. A storage host must create `SecurityPolicyHistory` only from its validated complete committed policy projection.
