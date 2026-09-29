# M0-07 – ODE-001 Rust-MSRV

**Prüfdatum:** 2026-09-29
**Ergebnis:** PASS für den dokumentierten Edition-2024-Kandidatenspike und die Entscheidung ODE-001
**Artefaktcommit:** `42f7ec5` (`docs: decide WorldDB Rust MSRV`)

## Ergebnis

- [ADR-036](contract/ADR-036-rust-msrv.md) setzt Rust 1.85.0 als initiale MSRV fest, pinnt die exakte Toolchain und dokumentiert die sechsmonatige Änderungsregel; damit ist ODE-001 entschieden. Die offene Entscheidungsliste unter `docs/contract/` bleibt bytegetreu zum unveränderlichen Quellspiegel und enthält den ursprünglichen ODE-001-Eintrag weiter. Das Statusregister führt ODE-001 als `DECIDED`.
- [Der Spike](../experiments/msrv-spike/README.md) enthält die Edition-2024-Manifestkonfiguration, repräsentative Dependency-Features, Quellcode und `Cargo.lock`. Der tatsächliche UUIDv7-Generator bleibt eine eigene Entscheidung in M0-08.
- Lokale Ausgabe: `rustc 1.85.0 (4d91de4e4 2025-02-17)`, `cargo 1.85.0 (d73d2caf9 2024-12-31)`, Windows x86_64 MSVC mit Visual Studio 18 C++-Buildumgebung. Rust-Zielartefakte lagen unter `C:\Users\wedde\AppData\Local\WorldDB\test-runs\M0-07\target`, außerhalb des OneDrive-Projektpfads.
- Die endgültige Lockdatei enthält 62 Registry-Abhängigkeiten. Der Graph wurde für alle Targets inspiziert; keine deklarierte `rust_version` liegt über 1.85.0. `blake3`, `unarray` und `zerocopy-derive` deklarieren keine MSRV, wurden aber mit dem gepinnten Compiler gebaut. Cargo wählte `wasip2 1.0.1+wasi-0.2.4`, weil die neuere angebotene Version Rust 1.87.0 erfordert.

## Reproduzierbare Prüfungen

Die folgenden Befehle liefen am Prüfdatum mit Exitcode 0:

```powershell
cargo test --locked
cargo check --locked --all-targets
cargo tree --locked --target all
cargo metadata --locked --format-version 1 --all-features
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
git diff --check HEAD
```

`cargo test --locked` meldete drei bestandene Unit-/Property-Tests und keine Doc-Tests. `cargo check --locked --all-targets` kompilierte alle für den Host aktivierten Targets. Cargo metadata umfasste 62 Registry-Pakete plus das lokale Spikepaket; der MSRV-Scan fand null deklarierte Versionsanforderungen über 1.85.0 und drei fehlende MSRV-Angaben. `cargo tree --locked --target all` gab 76 Zeilen mit den direkten, transitiven und targetgebundenen Zweigen aus.

## Prüfgrenze

Dies ist ein Dependency-/Toolchain-Spike, kein WorldDB-Produktworkspace und kein Laufzeittest. Der Build lief nur auf Windows x86_64 MSVC; macOS/APFS und Linux/ext4 wurden nicht geprüft. Der echte Workspace und seine endgültige Dependency-Featurematrix müssen M0-09/M0-12 und am M0-15-Gate erneut mit `--locked` auf der MSRV bestätigt werden. Die CI-MSRV-Matrix folgt M0-14.
