# ADR-034 – One query contract across Rust, CLI, and desktop IPC

**Status:** Accepted for the WorldDB 1.0 working contract

**Task:** M0-04d

**Decision owner:** Luna (contract author)

**Normative detail:** [Query and transport supplement](query_transport_contract.md)

## Context

Master §16 defined query operations, cursor binding, security order, and terminal budget/cancellation behavior, while §§12 and the desktop boundary defined JSON precision and versioned IPC. It did not define a common owned request DTO, filter grammar, token-search behavior, stable query sort keys, or how CLI and IPC versions map to the Rust facade. Adapter-specific defaults or query semantics could otherwise produce different logical results.

## Decision

1. The stable Rust facade, CLI, and desktop IPC map to one owned typed `QueryRequest`, `QueryPage`, closed result union, and stable error enum. The authenticated host supplies the Principal and cancellation state; caller DTOs cannot choose identity, Capabilities, or filesystem paths.
2. Query context is explicit and pins the selected snapshot, RecordedAsOf, HistorySpace, layers, world-time selector, Perspective/EpistemicMode, SchemaMode, authorization mode, and finite work budgets. The shared constructor alone supplies the Master-defined Historical schema default.
3. Filters are a bounded closed typed AST. Security and FieldRead run before candidate creation; typed comparisons use the Master scalar rules. TokenSearch is byte-exact and case-sensitive over ASCII-whitespace tokens, with no implicit locale, normalization, stemming, phrase, ranking, or snippets. FullText remains a separate optional capability-gated operation.
4. Sort terms use closed typed selectors, explicit direction and missing placement, and a caller-visible stable result-key tie-breaker. Pages are owned and bounded. Cursor continuation binds the canonical query and pinned snapshot and repeats current authorization checks.
5. CLI machine mode and desktop IPC use a closed 1.0 versioned envelope with precision-safe JSON encodings, stable public errors, request correlation, and explicit IPC version/feature negotiation. Storage format versions are independent.

## Consequences

- Adapter parity can be checked against canonical DTOs without reproducing query semantics in UI or CLI code.
- M3/M4/M5/M6/M8 must implement the DTO conversions, comparators, cursor continuation, cancellation, authorization mapping, precision rules, and parity/property/fuzz checks.
- FullText may be absent in a conforming 1.0 engine; it must fail as unsupported and cannot silently become token search.
- No persisted First-Class type or WDB invariant ID is added. ADR-039 later closes the 52 HARD source gaps; the two GUARDED gaps remain open.

## Verification obligations

Verify canonical request parity across the Rust facade, CLI, and IPC; strict decode and precision behavior; exact typed filter and sort semantics; stable result-key ordering; explicit calendar-window fingerprints; search token rules; same-snapshot cursor continuation and security invalidation; public error non-interference; and bounded cancellation/budget outcomes. Structural contract checks do not claim that any runtime implementation exists.
