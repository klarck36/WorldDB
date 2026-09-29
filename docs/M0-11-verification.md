# M0-11 – Nachweis zur Format-, Lint- und Unsafe-Policy

**Status:** DONE
**Geprüft:** 2026-09-29
**Artefaktcommit:** `3b33821` (`feat: activate WorldDB M0-11 lint policies`)

## Ergebnis

`rustfmt.toml` bindet die Formatierung an Edition 2024. Alle fünf Workspacecrates erben ihre Lints explizit. Workspace-Rust verbietet `unsafe_code`; `worlddb-core` verschärft die Grenze auf `forbid`, der einzige Plattformadapter `worlddb-storage-file` bleibt bei `deny`. Clippy verweigert `correctness`, `suspicious`, `panic`, `unwrap_used`, `expect_used` und `indexing_slicing`; der Verify-Aufruf behandelt zusätzlich alle Warnungen als Fehler.

Das versionierte `policy/exceptions.tsv` enthält derzeit keine lokalen Ausnahmen. Jeder spätere `#[allow]`-Eintrag braucht eine eindeutige `WDB-EXC-NNNN`-ID, Owner, Begründung und gültiges Ablaufdatum. Adapter-Unsafe braucht zusätzlich lokale `SAFETY:`, `TEST:` und `REVIEW:`-Belege. Der Terminologie-Check klassifiziert 72 bestehende Treffer aus ADRs, historischen Erklärungen und expliziten Negativregeln; unklassifizierte Treffer scheitern.

## Nachweise

- `cargo xtask verify` – 18 erforderliche Schritte bestanden, zwei für M0-12 und M0-14 sichtbare Schritte übersprungen, null Fehler.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` und `cargo check --locked --workspace --all-targets` – bestanden unter Rust/Cargo 1.85.0 auf Windows x86_64 MSVC.
- Workspace-Lint-, Unsafe-, Ausnahme- und Terminologiechecks bestanden; 72 Treffer vollständig klassifiziert (8 ADR, 28 explizite Negativregel, 36 historische Erklärung); keine registrierten lokalen Ausnahmen.
- 25 automatisierte Tests bestanden: vier Crategraph-, 16 Policy-, ein Rust-Compile-Fail- und vier xtask-Tests.
- Die negativen Fixtures lassen den jeweiligen Verify-Prüfschritt fehlschlagen: Unsafe außerhalb des Adapters, ein normativer `BranchId`, eine abgelaufene Ausnahme, fehlende Unsafe-Belege sowie `unsafe` unter `forbid(unsafe_code)`.
- `scripts/verify.ps1` lief nativ; `scripts/verify.sh` lief samt `bash -n` unter Git Bash. Beide meldeten 18 PASS, zwei begründete SKIPs und null Fehler.
- Vertragsquellen-, Vertragsdokumentations-, Quellen-, Plan- und Whitespacechecks bestanden.

## Reproduktion

Vom Repository-Stamm:

```text
cargo xtask verify
```

```powershell
.\scripts\verify.ps1
```

```sh
./scripts/verify.sh
```

Die Wrapper setzen das Profil `dev` ausdrücklich und legen Cargo-Buildartefakte standardmäßig außerhalb des synchronisierten Repositorys ab. Lokale Plattformprüfungen liefen unter Windows x86_64 MSVC; Git Bash bestätigt die Unix-Shellsyntax und Wrapperfunktion, native Linux-/macOS-Runs folgen M0-14.

## Grenzen und Folgearbeit

Die zwei im Manifest sichtbaren SKIPs bleiben bis M0-12 (Dependency- und Feature-Policy) und M0-14 (CI-Matrix) bestehen. M0-02a hat weiterhin 52 offene HARD- und zwei GUARDED-Quellenlücken; das bleibt ein Blocker für das M0-15-Gate, nicht für diese Policy-Task.
