# ADR-037 – Fallible UUIDv7 generation

**Status:** Accepted for the WorldDB 1.0 implementation baseline
**Task:** M0-08
**Decision date:** 2026-09-29
**Decision owner:** Domain Lead / Toolchain Maintainer

## Context

The Master requires opaque 128-bit ID newtypes and sets UUIDv7 as the default generation policy. UUID time bits are operational only; they do not establish domain order, `RecordedAsOf`, or authenticity. ODE-005 asks for a maintained UUID library versus a small OS-CSPRNG generator, with license, MSRV, unsafe/transitive review, RFC vectors, collision/monotonicity properties, and fuzzing. The Rust engineering contract also forbids production panics on clock and randomness inputs.

## Decision

1. Generate WorldDB UUIDv7 values with a small, fallible internal wrapper around `SystemTime`, `getrandom::fill`, and `uuid::Builder::from_unix_timestamp_millis`. Keep `uuid` for the UUID value, parsing/formatting, and its RFC bit-layout builder. Keep the UUID type behind WorldDB ID newtypes; do not enable the `uuid/serde` feature or expose `uuid::Uuid` in public domain APIs.
2. Do not use `Uuid::now_v7` or `Uuid::new_v7` on the product generation path for `uuid 1.26.1`. Its built-in RNG maps `getrandom` failure to `panic!`, which violates Master §4.2/WDB-ENG-003 for randomness sources. The selected wrapper maps clock-before-epoch, out-of-range 48-bit milliseconds, and entropy failure to typed `Result` errors and never substitutes a weaker entropy source.
3. Use all 74 random payload bits allowed by RFC 9562. The generator does not promise order among IDs created in the same millisecond. This follows the Master, which makes UUID sortability an operational storage property and requires no temporal ID semantics. The timestamp prefix still sorts values with different millisecond timestamps; clock rollback is not reinterpreted as domain order.
4. Keep the generator behind a private product constructor in M0-09. If a future fallible UUID library or a justified monotonic-counter requirement changes the implementation, the constructor may be replaced without changing UUIDv7 wire bytes or migrating stored IDs. Same-millisecond ordering would require a separate decision and counter-overflow/clock-rollback proof.

## Candidate and dependency review

| Package/path | License | Declared MSRV | Findings |
|---|---|---:|---|
| `uuid 1.26.1` with `std` + `v7`, defaults off | Apache-2.0 OR MIT | 1.85.0 | `Uuid::now_v7` documents same-process creation ordering. Its `v7` module uses a shared `ContextV7` counter and getrandom. Its generator source panics if entropy retrieval fails. No UUID `serde` or `rand` feature is enabled. |
| `getrandom 0.4.3` direct | MIT OR Apache-2.0 | 1.85 | Safe `fill` API returns an error; platform-specific unsafe code stays inside the crate. The documented backends cover Windows 10+, Linux, and macOS. |
| UUID-generation target graph | — | — | Windows: `uuid -> getrandom 0.4.3 -> cfg-if 1.0.5`; Linux/macOS add `libc 0.2.189`. These transitive crates declare Rust 1.32/1.65 and dual MIT/Apache licensing. `proptest` separately brings dev-only `getrandom 0.3.4`; it is not on the product ID-generation path. |

The UUID crate contains unsafe code in formatting and helper APIs outside the UUIDv7 timestamp/RNG code path. The internal wrapper adds no unsafe code. `getrandom` necessarily encapsulates platform FFI in its safe API. This is a source/feature-graph review, not a formal whole-repository unsafe audit; M0-12 owns the later machine-enforced dependency policy. The project license remains undecided from M0-06, so the dual-license package options are recorded without selecting a project license.

## Verification evidence

The spike under `experiments/msrv-spike` exercises both candidates:

- RFC 9562 Appendix A.6 vector reproduced exactly as `017f22e2-79b0-7cc3-98c4-dc0c0c07398f` using the internal wrapper and crate builder.
- `ContextV7` candidate generated 4,096 collision-free UUIDs at one fixed millisecond in strictly increasing order.
- The selected CSPRNG candidate generated 4,096 IDs at one fixed millisecond with no observed collisions. For independent uniform 74-bit tails, the birthday approximation is `n(n−1)/2^75` per timestamp bucket; one million IDs in one millisecond gives about `2.65e-11`, not a uniqueness guarantee.
- Property-based fuzzing ran 4,096 fixed-seed cases over 48-bit timestamps and 80 input random bits, checking timestamp placement, version/variant bits, lowercase formatting, and parse round-trip. Error-path tests cover clock failure, entropy failure, and timestamp range boundaries.
- `cargo test --locked` passed 10 tests; `cargo check --locked --all-targets` passed under Rust/Cargo 1.85.0. `cargo tree --locked --target all` and metadata inspected the locked graph. The build ran on Windows x86_64 MSVC; Linux/macOS dependency trees were resolved but not compiled.

## Consequences

- OS CSPRNG failures are reported through a normal error path; generation cannot silently fall back to a weaker source.
- Same-millisecond UUID order is random. Any caller requiring causal/domain order must use revision or transaction semantics, never UUID sort order.
- The product will keep both `uuid` and `getrandom` in the candidate dependency graph. This ADR selects the generator path, not the unresolved repository-wide license or M0-12 supply-chain policy.

## References

- [RFC 9562 §5.7, §6.2, and Appendix A.6](https://www.rfc-editor.org/rfc/rfc9562.html#section-5.7)
- [`uuid 1.26.1` `Uuid::now_v7`](https://docs.rs/uuid/1.26.1/uuid/struct.Uuid.html#method.now_v7)
- [`uuid 1.26.1` UUIDv7 source](https://docs.rs/crate/uuid/1.26.1/source/src/v7.rs)
- [`uuid::Builder::from_unix_timestamp_millis`](https://docs.rs/uuid/1.26.1/uuid/struct.Builder.html#method.from_unix_timestamp_millis)
- [`getrandom 0.4.3` `fill`](https://docs.rs/getrandom/0.4.3/getrandom/fn.fill.html)
- [`getrandom 0.4.3` supported RNG backends](https://docs.rs/crate/getrandom/0.4.3/source/README.md)
- [WorldDB Master working copy §3.1 IDs and §4.2 panic/fallibility rules](WorldDB_Finaler_Vollstaendiger_Plan_vNext.md)
