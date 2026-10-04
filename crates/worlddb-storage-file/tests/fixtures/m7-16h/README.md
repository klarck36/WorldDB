# M7-16h pre-Alpha fixture baseline

This versioned synthetic corpus exercises the current supported storage profile. Version 2 updates the logical-export envelope for the expanded closed RecordKind manifest, including TimelineDefinition and TimeUnitDefinition. It supersedes the pre-Alpha version 1 fixture before any Alpha release. `manifest.tsv` records each artifact's byte length and BLAKE3 digest; the integration test pins the manifest digest itself and rejects extra or missing files.

| Fixture | Expected result |
| --- | --- |
| `storage/` | A complete two-revision file-store database opens and passes Storage Verify. Its pinned DatabaseId is `id(1)`. |
| `exact-backup/` | A full ExactDatabaseBackup verifies at source revision 2 and restores as a new DatabaseId with a matching RestorePublication Required Audit. |
| `exports/logical-export.wdbx` | Decodes and exactly matches a fresh scoped Logical Export from the storage fixture. |
| `exports/sharing-export.wdbs` | Decodes and exactly matches a fresh authorized Sharing Export from a copy of the storage fixture. |
| `migration/plan.record` and `migration/step-*.record` | Decode as a two-step Restrictive migration plan and its canonical input records. Applying it to the storage fixture commits revisions 3 and 4, leaves the `FORMAT` probe byte-identical, reopens a Completed run journal, and writes two matching Required Audit records. |
| `migration/expected-logical-export.wdbx` | Canonical logical export of the migrated revision-4 database; the executed migration must reproduce these exact bytes. |

The corpus uses only synthetic records. Its physical storage snapshot is frozen once; later refreshes are explicit fixture-version changes and update the manifest and pinned test digest together. To capture a refreshed set intentionally, set `WORLDDB_M7_16H_CAPTURE_FIXTURES=1` and run `cargo test --locked -p worlddb-storage-file --test m7_16h_fixture_baseline -- --nocapture`; then review every changed binary and update the pinned digest. If only derived exports change, update those specific files and use `WORLDDB_M7_16H_REFRESH_MANIFEST=1` with the same test command to regenerate the manifest without replacing the physical snapshot. The normal test never writes fixtures.

## N-1 applicability

As checked on 3 October 2026, the public repository had no published releases and no tags. Therefore this pre-Alpha baseline is **not applicable as N-1**, not an N-1 pass. M9-02 must be revisited after the first Alpha release is actually published.
