# M0-09 – Nachweis zum initialen Rust-Workspace

**Status:** DONE
**Geprüft:** 2026-09-29
**Artefaktcommit:** `66bfb37` (`feat: establish WorldDB M0-09 workspace`)

## Ergebnis

Der gesperrte Rust-Workspace enthält die fünf geplanten Start-Crates `worlddb-core`, `worlddb-storage-file`, `worlddb-cli`, `worlddb-testkit` und `xtask`. Alle fünf übernehmen Edition 2024, MSRV 1.85.0 und die vom Product Owner gewählte Lizenz `MIT OR Apache-2.0`. Der Repositorybaum bleibt eine Start-Hypothese; jede spätere Crate-Extraktion benötigt ein eigenes Boundary-Gate.

Der tatsächliche Cargo-Metadatengraph hält die erlaubte Richtung ein. `worlddb-core` hat keine Crate-Abhängigkeiten; der Dateiadapter hängt nur vom Core ab; CLI und Testkit hängen nur in den dokumentierten Richtungen; `xtask` ist isoliert. In M0-09 gibt es keine externen Dependencies.

## Nachweise

- `cargo fmt --all -- --check` – bestanden.
- Unter Rust/Cargo 1.85.0 auf Windows x86_64 MSVC: `cargo check --locked --workspace --all-targets` – bestanden.
- `cargo metadata --locked --format-version 1 --no-deps` – genau fünf Workspace-Mitglieder, alle mit `MIT OR Apache-2.0`.
- `tools/check_crate_graph.py` – tatsächlicher Cargo-Graph bestanden; `cargo tree --locked` enthält ausschließlich lokale Workspace-Abhängigkeiten.
- `tools/test_crate_graph.py` – vier Tests bestanden: erlaubter Startgraph sowie Ablehnung einer Core-Rückkante, einer externen Dependency und einer nicht genehmigten Crate-Extraktion.
- `build_contract_sources.py --verify-only`, `verify_contract_docs.py`, `WorldDB_1.0_Sourcecheck.py` und `WorldDB_1.0_Plancheck.py` – bestanden.
- `git diff --check` und `git diff --cached --check` – bestanden.

## Reproduktion

Aus dem Repository-Stamm in PowerShell. Die Toolchainpfade werden hier explizit ergänzt, weil sie in der Codex-Shell nicht im initialen `PATH` lagen. Der Build-Ordner bleibt außerhalb des synchronisierten OneDrive-Projektpfads.

```powershell
$toolchainBin = Join-Path $env:USERPROFILE '.rustup\toolchains\1.85.0-x86_64-pc-windows-msvc\bin'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH = "$toolchainBin;$cargoBin;$env:PATH"
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'WorldDB\test-runs\M0-09\target'
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo metadata --locked --format-version 1 --no-deps
python -B -X utf8 tools/check_crate_graph.py
python -B -X utf8 -m unittest discover -s tools -p 'test_crate_graph.py' -v
cargo tree --locked
python -B -X utf8 docs/contract/build_contract_sources.py --verify-only
python -B -X utf8 docs/contract/verify_contract_docs.py
python -B -X utf8 WorldDB_1.0_Sourcecheck.py
python -B -X utf8 WorldDB_1.0_Plancheck.py
git diff --check HEAD
```

## Grenzen und Folgearbeit

Gebaut wurde nur auf Windows x86_64 MSVC. CI folgt M0-14. `cargo xtask verify`, seine Unix-/PowerShell-Wrapper und das Schrittmanifest gehören zu M0-10. Dependency-Lizenzen, Advisories und Featureregeln folgen M0-12. Die 52 offenen HARD-Quellenlücken und zwei GUARDED-Lücken aus M0-02a bleiben unverändert; sie blockieren weiterhin das M0-15-Gate, nicht den abgeschlossenen M0-09-Workspace.
