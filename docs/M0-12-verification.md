# M0-12 – Dependency- und Feature-Policy

**Status:** DONE
**Geprüft:** 2026-09-29
**Implementierungscommit:** `107f822` (`feat: enforce WorldDB dependency policy`)

## Ergebnis

`.cargo/deny.toml` aktiviert `cargo-deny 0.20.2` für Advisory-, Ban-, Lizenz- und Sourceprüfungen. Erlaubt sind nur `MIT` und `Apache-2.0`; Git- und unbekannte Registryquellen, yanked oder advisory-betroffene Packages, Mehrfachversionen und Wildcardversionen stoppen den Check. Externe Default-Features sowie `anyhow`, `thiserror` und `failure` sind standardmäßig gesperrt.

`Cargo.lock` bleibt verpflichtend und jeder Workspacebuild im Verify-Pfad nutzt `--locked`. Das Dependencyregister bindet Reviewdaten an Name, Version und Source jeder externen Lockfileversion, zählt transitive externe Packages und prüft die deklarierten MSRVs gegen den Projekt-MSRV 1.85. Die Registerfelder decken Boundarynutzen, Maintainer-/Releasezustand, Lizenz, Advisories, Unsafe-Anteil, Build-/Proc-Macro-Risiko, Features, Exit-Plan und Owner ab. Der aktuelle Workspace enthält keine externen Dependencies; die Reviewdatei ist daher nur mit ihrer Kopfzeile befüllt.

Die ausführbare Featurematrix baut alle Workspacetargets ohne Default-Features, mit Default-Features und mit allen Features. Der all-features-Metadatencheck prüft zusätzlich gegen die Einträge in `policy/feature-conflicts.tsv`. Dieses Register ist aktuell leer, weil die Produktcrates noch keine Produktfeatures definieren. Im Core-Quellbaum ist `Box<dyn Error>` verboten; die drei universellen Error-Crates sind workspaceweit gesperrt.

Die Updatefolge ist in `docs/architecture/M0-12-dependency-policy.md` versioniert: monatliche Dependencyprüfung und zusätzliche Sicherheits-/Lizenzupdates, enge Updategruppen, Register-/Lockfileabgleich, Prüfung auf MSRV 1.85, Lizenz-/Advisory-/Featureabgleich sowie manuelle Sichtung von Build-Scripts und Proc-Macros. Die Prüfungstoolversion ist separat auf `cargo-deny 0.20.2` festgelegt; der Projekt-MSRV bleibt Rust 1.85.0.

## Nachweise

- `cargo xtask verify` auf Windows x86_64 MSVC mit Rust/Cargo 1.85.0: **23 Schritte bestanden, 1 sichtbarer Skip für M0-14, 0 Fehler**.
- Workspaceformatierung, gelockter Workspacecheck, Clippy und alle drei Featurematrix-Builds bestanden; `cargo tree --locked --workspace --all-features -e features` ausgegeben.
- `cargo-deny 0.20.2 --workspace --locked check all`: Advisories, Bans, Lizenzen und Sources bestanden. Das Lockfile enthält derzeit null externe Packages.
- 38 automatisierte Tests bestanden: 16 M0-11-Policytests, ein Unsafe-Compile-Fail-Test, 13 M0-12-Dependencytests, vier Crategraphtests und vier `xtask`-Tests.
- Die Negativtests zeigen, dass `anyhow`, externe Default-Features ohne Freigabe, eine inkompatible Featurekombination, ein unreviewter/versionsveränderter Dependencyeintrag, ein Dependency-MSRV oberhalb 1.85 sowie `Box<dyn Error>` im Core scheitern.
- `scripts/verify.ps1` nativ unter PowerShell sowie `scripts/verify.sh` unter Git Bash auf Windows: jeweils **23 PASS, 1 M0-14-SKIP, 0 FAIL**. `bash -n scripts/verify.sh` bestanden; der Unix-Wrapper verwendet auf Windows den vorhandenen `python`-Launcher, wenn `python3` nur als nicht startbarer Store-Alias vorhanden ist.
- Quellen-, Vertragsdokumentations-, Plan- und Whitespacechecks bestanden.
- Alle Buildartefakte liegen unter `%LOCALAPPDATA%\WorldDB\test-runs\M0-12`, außerhalb des synchronisierten Repositorypfads.

## Grenzen und Folgearbeit

Die Dependency- und Featureregeln sind wirksam; mangels externer Produktdependencies wurde kein reales Fremdpaket ausgewählt oder lizenziert. M0-14 ergänzt native Linux-, Windows- und macOS-CI. M0-02a hat weiterhin 52 offene HARD- und zwei GUARDED-Quellenlücken und bleibt Blocker für M0-15. Die Release-SBOM und der Advisory-Snapshot bleiben M9-09; der stabile Public-API-Diff bleibt M8-01.

## Reproduktion

```text
cargo xtask verify
```

Falls der gepinnte Prüfer fehlt, installieren:

```powershell
rustup toolchain install 1.88.0 --profile minimal
cargo +1.88.0 install cargo-deny --version 0.20.2 --locked
```
