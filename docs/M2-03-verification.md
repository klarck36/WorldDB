# M2-03 verification: SchemaAt and schema snapshots

**Status:** PASS for the index-free reference model. Checked 2026-09-30.

## Implemented

- `SchemaHistoryReferenceModel` stores immutable project-wide schema batches on the common `Revision` axis. It accepts layer definitions and snapshots, EntityType, Predicate, and EventKind definitions.
- `SchemaMode::Historical` is the default; `Current` selects the latest published shared revision; `Explicit` selects the exact requested `SchemaRevision`. Future/unpublished requests fail.
- `SchemaSnapshot` materializes the latest definition for each typed identity, in canonical family/ID order. Its BLAKE3 fingerprint is derived from length-delimited canonical record encodings and is independent of publication revision when effective schema content is unchanged.
- Invalid batches are staged and validated before publication. Revision mismatch, duplicate definition IDs, and duplicate family symbols fail without advancing the head.
- A damaged required historical entry returns `HistoricalSchemaCorrupt`; neither Historical nor Explicit silently substitutes Current or a neighboring snapshot. Fingerprint disagreement has a separate `SchemaFingerprintMismatch` error.

## Verification

- `cargo test --locked --offline --workspace` — PASS; 160 core unit tests, workspace integration/unit tests, wire oracle, and 66 Rustdoc tests passed. One-hour decoder and ten-million-case precision campaigns remain intentionally ignored by the ordinary suite.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS after formatting.
- `python -B WorldDB_1.0_Plancheck.py` — PASS; 236 tasks, 11 milestones, 253 invariants, 161 follow-up pairs; DAG and references valid.

Focused tests cover all three modes on one history, stable/content-sensitive fingerprints, failed publication atomicity, default-Historical behavior, corruption without fallback, and unpublished revision rejection.

## Scope

This is the M2 reference model. Durable schema-history storage, transaction-integrated schema writes, commit-time schema dependency revalidation, external corruption/recovery testing, and engine query integration remain scheduled work in later tasks.
