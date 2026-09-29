# ADR-036 – Rust MSRV and toolchain baseline

**Status:** Accepted for the WorldDB 1.0 implementation baseline
**Task:** M0-07
**Decision date:** 2026-09-29
**Decision owner:** Toolchain Maintainer

## Context

The repository had no Rust toolchain or product workspace at M0-06. ODE-001 therefore required a reproducible Edition-2024 dependency spike before selecting a concrete minimum supported Rust version (MSRV). Rust 1.85.0 is the first stable compiler with Edition 2024; Rust 1.98.1 was the latest stable release on the decision date. The project recommendation was to select the oldest stable compiler that supports the chosen edition and the candidate dependency surface.

## Decision

1. Set the initial MSRV to Rust **1.85.0**. The repository-root `rust-toolchain.toml` pins contributors to the exact `1.85.0` toolchain. The spike manifest sets `rust-version = "1.85"`.
2. Use Edition 2024 and Cargo resolver 3. Commit `Cargo.lock` for application workspaces and use `--locked` in reproducible verification.
3. Keep the MSRV at 1.85.0 for at least six months from this decision (through 2027-03-29). An increase before that date is permitted only when announced in advance for a minor release. Every increase is introduced in a minor release, never a patch release.
4. Every new or updated dependency must be checked against this exact compiler with the actual feature set. A missing crate `rust_version` declaration is not evidence of compatibility; the locked dependency graph must build and test on the MSRV. Cargo's Edition-2024 resolver uses declared Rust-version information when resolving dependencies.
5. Recheck the decision against the real product workspace in M0-09 and M0-15. The spike's `uuid` dependency is only a compile candidate; ODE-005's UUIDv7 generator decision remains open for M0-08.

## Evidence

`experiments/msrv-spike` compiles a small surface for hashing, CLI parsing, JSON adapter DTOs, diagnostics, UUIDv7 API availability, and property testing. Under Rust/Cargo 1.85.0 on Windows x86_64 MSVC, `cargo test --locked` passed all three tests and `cargo check --locked --all-targets` passed. The lockfile contains 62 registry packages. `cargo tree --locked --target all` resolved all target-specific dependency branches; Cargo metadata reports no declared dependency MSRV above 1.85. Three packages omit a declared MSRV (`blake3`, `unarray`, and `zerocopy-derive`) and were compiled successfully in the spike. Cargo selected `wasip2 1.0.1+wasi-0.2.4` because the newer available release requires Rust 1.87.0.

The evidence establishes the candidate graph on this Windows host, not on macOS/Linux and not for product runtime code. The verification report records commands, environment, counts, and limits.

## Consequences

- Contributors install one exact compiler while the project supports the 1.85 language/library floor.
- Dependency updates are constrained by the locked graph and tested at the declared MSRV; packages with missing MSRV metadata require the same build evidence.
- The actual application workspace, platform matrix, and dependency policy still require their planned M0-09, M0-12, M0-14, and M0-15 work.

## References

- [Announcing Rust 1.85.0 and Rust 2024](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)
- [Announcing Rust 1.98.1](https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/)
- [Rust 2024 Cargo resolver guide](https://doc.rust-lang.org/edition-guide/rust-2024/cargo-resolver.html)
- [Cargo `rust-version` reference](https://doc.rust-lang.org/cargo/reference/rust-version.html)
