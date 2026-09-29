# WorldDB MSRV spike

This minimal Edition-2024 package is the reproducible evidence for ODE-001. It checks whether the candidate dependency surface builds and tests on the proposed Rust 1.85.0 floor. It is not product runtime code, and passing these tests does not close the later workspace, platform, dependency-policy, or M0-gate tasks.

## Candidate surface

The spike deliberately enables small feature sets and exercises each direct dependency:

| Crate | Resolved version | Use in the spike |
|---|---:|---|
| `blake3` | 1.8.7 | Hashing candidate |
| `clap` | 4.6.7 | CLI derive and parser candidate |
| `serde` | 1.0.229 | Adapter DTO derive |
| `serde_json` | 1.0.151 | Adapter DTO encoding/decoding |
| `tracing` | 0.1.44 | Diagnostic event candidate |
| `uuid` | 1.26.1 | UUID value/format and UUIDv7 builder; its `now_v7` API is retained only as a comparison candidate |
| `getrandom` | 0.4.3 | Direct, fallible OS CSPRNG call for the internal UUIDv7 generator candidate |
| `proptest` | 1.11.0 | Dev-only DTO round-trip property test |

`Cargo.lock` locks 62 third-party package versions. The full resolved graph, including target-specific branches, is reproducible with `cargo tree --locked --target all`. Three packages in Cargo metadata do not declare `rust_version`: `blake3`, `unarray`, and `zerocopy-derive`; the spike compiles them under Rust 1.85.0. No package with a declared MSRV above 1.85.0 was present in the locked graph.

## M0-08 UUIDv7 generator comparison

ADR-037 selects a small fallible generator wrapper built from `SystemTime`, `getrandom::fill`, and `uuid::Builder::from_unix_timestamp_millis`. It keeps `uuid` for the UUID value and RFC bit layout but does not call `Uuid::now_v7` or `Uuid::new_v7` in the product generator path. In `uuid 1.26.1`, those convenience generation methods panic if their internal getrandom call fails. The WorldDB Master forbids panics on clock and randomness inputs.

The internal candidate returns typed errors for a pre-epoch clock, an out-of-range timestamp, and unavailable entropy. It uses 74 random bits as permitted by RFC 9562. That provides no creation order among UUIDs generated in the same millisecond. The Master states that UUID timestamp/order bits are operational only, so no same-millisecond ordering is required. The crate candidate's `ContextV7` was separately tested for strict same-process order and collision-free generation at a fixed millisecond.

| Review item | `uuid` crate generator | Fallible internal generator selected |
|---|---|---|
| License/MSRV | `uuid 1.26.1`, Apache-2.0 OR MIT, Rust 1.85.0 | `getrandom 0.4.3`, MIT OR Apache-2.0, Rust 1.85 |
| Entropy/targets | `uuid` v7 uses `getrandom`; supported platform RNG is delegated | Calls the same `getrandom::fill` API directly |
| Unsafe review | No explicit unsafe in UUIDv7's `v7.rs` or timestamp context; the crate has unsafe code in other formatting/helper routines | Candidate adapter has no unsafe; `getrandom` contains platform FFI behind its safe API |
| Failure handling | `now_v7`/`new_v7` panic if entropy retrieval fails | Clock and entropy failures return typed `Result` errors |
| Same-millisecond order | `ContextV7` is monotonic within a process | Random payload does not promise same-millisecond order |
| Decision | Rejected as the production generation entry point because of the panic path | Selected; the library builder still constructs the RFC fields |

The active target graph for `uuid` v7 is `uuid -> getrandom 0.4.3 -> cfg-if 1.0.5` on Windows and adds `libc 0.2.189` on Linux/macOS. The target graph was resolved for all three targets, but only the Windows host was built here. The `serde` and `rand` UUID features are disabled. The spike's separate `proptest` dev graph also contains `getrandom 0.3.4`.

The RFC 9562 Appendix A.6 vector is tested byte-for-byte: `017f22e2-79b0-7cc3-98c4-dc0c0c07398f`. Property fuzzing checks 4,096 fixed-seed cases for timestamp prefix, version, RFC variant, lowercase formatting, and parse round-trip. A separate 4,096-ID CSPRNG sample at one fixed millisecond found no collisions. These finite samples are evidence of the implementation path, not a uniqueness proof.

## Reproduction

Run from the repository root in PowerShell with the pinned toolchain installed and the Visual Studio C++ build environment available on Windows:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'WorldDB\test-runs\M0-08\target'
$env:PROPTEST_CASES = '4096'
$env:PROPTEST_RNG_SEED = '8675309'
Set-Location experiments\msrv-spike
cargo test --locked
cargo check --locked --all-targets
cargo tree --locked --target all
cargo metadata --locked --format-version 1 --all-features
```

The target directory stays outside the OneDrive project tree, following the M0-06 test-path rule. `rust-toolchain.toml` at the repository root selects Rust 1.85.0; the spike manifest declares `rust-version = "1.85"`, Edition 2024, and resolver 3.

## Scope limits

- The property test covers only this spike's adapter DTO round-trip. It is not a WorldDB persistence, identifier, security, or durability test.
- The generator property tests and dependency tree ran under the pinned MSRV on Windows x86_64 MSVC. macOS/APFS and Linux/ext4 were not compiled in this environment.
- The final product dependency graph does not yet exist. M0-09/M0-12/M0-15 must apply this baseline to that graph and re-run locked MSRV checks.
