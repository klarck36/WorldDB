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
| `uuid` | 1.26.1 | UUIDv7 API/RNG compile candidate; ODE-005 remains open |
| `proptest` | 1.11.0 | Dev-only DTO round-trip property test |

`Cargo.lock` locks 62 third-party package versions. The full resolved graph, including target-specific branches, is reproducible with `cargo tree --locked --target all`. Three packages in Cargo metadata do not declare `rust_version`: `blake3`, `unarray`, and `zerocopy-derive`; the spike compiles them under Rust 1.85.0. No package with a declared MSRV above 1.85.0 was present in the locked graph.

## Reproduction

Run from the repository root in PowerShell with the pinned toolchain installed and the Visual Studio C++ build environment available on Windows:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'WorldDB\test-runs\M0-07\target'
Set-Location experiments\msrv-spike
cargo test --locked
cargo check --locked --all-targets
cargo tree --locked --target all
cargo metadata --locked --format-version 1 --all-features
```

The target directory stays outside the OneDrive project tree, following the M0-06 test-path rule. `rust-toolchain.toml` at the repository root selects Rust 1.85.0; the spike manifest declares `rust-version = "1.85"`, Edition 2024, and resolver 3.

## Scope limits

- The property test covers only this spike's adapter DTO round-trip. It is not a WorldDB persistence, identifier, security, or durability test.
- A UUIDv7 library API is compiled, but generator selection and RFC/property/security review remain in M0-08.
- The current verification ran on Windows x86_64 MSVC. macOS/APFS and Linux/ext4 remain unverified.
- The final product dependency graph does not yet exist. M0-09/M0-12/M0-15 must apply this baseline to that graph and re-run locked MSRV checks.
