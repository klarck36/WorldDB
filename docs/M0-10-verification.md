# M0-10 – Nachweis zum kanonischen Verify-Pfad

**Status:** DONE
**Geprüft:** 2026-09-29
**Artefaktcommit:** `ab987c5` (`feat: add WorldDB M0-10 verify path`)

## Ergebnis

`cargo xtask verify` startet den Runner über einen Cargo-Alias, explizit mit dem Cargo-Profil `dev`. Der Runner liest `tools/verify/steps.tsv`, prüft dessen Datensätze und führt die Schritte des ausgewählten Verify-Profils aus. Fehler werden gesammelt und führen am Ende zu Exitcode 1. Jeder Schritt erscheint als RUN/PASS oder SKIP; übersprungene Manifest-Schritte brauchen einen sichtbaren Grund. `--skip STEP_ID` macht auch einen benutzerseitig ausgelassenen Pflichtschritt im Protokoll und Summenzähler sichtbar.

Das Dev-Profil führt Format- und Workspacechecks, Cargo-Abhängigkeitsbaum, Crategraphprüfung samt Negativtests, Vertragsquellen- und Dokumentationsprüfung, Quell- und Planchecks, Runner-Unit-Tests und Whitespace-Prüfung aus. M0-11-, M0-12- und M0-14-Schritte sind mit jeweiliger Zuständigkeit als SKIP im Manifest verzeichnet.

## Nachweise

- `cargo xtask verify` – 11 erforderliche Schritte bestanden, drei vorgesehene Schritte sichtbar übersprungen, null Fehler.
- `cargo xtask verify --skip crate-graph-tests` – 10 Schritte bestanden; `crate-graph-tests` wurde mit `[SKIP] ... requested via --skip` ausgegeben und in der Zusammenfassung gezählt (vier Skips insgesamt).
- `cargo test --locked --package xtask` – vier Parser-/Profil-/Skip-Tests bestanden.
- `scripts/verify.ps1` – vollständiges Dev-Profil bestanden; der Wrapper ergänzt Cargo bei Bedarf zum PATH und legt Buildartefakte standardmäßig außerhalb des OneDrive-Projektpfads ab.
- `scripts/verify.sh` – vollständiges Dev-Profil unter Git Bash auf Windows bestanden; `bash -n scripts/verify.sh` ebenfalls bestanden.
- Der vollständige Verify-Aufruf bestätigt zusätzlich die Cargo-Formatierung, den gelockten Workspacebuild, den Dependency-Baum, Quellen- und Vertragschecks, Plancheck sowie Whitespace-Prüfung.

## Reproduktion

Vom Repository-Stamm:

```text
cargo xtask verify
cargo xtask verify --skip crate-graph-tests
```

Die Wrapper wählen `dev` ausdrücklich und setzen standardmäßig ein Buildverzeichnis außerhalb des synchronisierten Repositorys: PowerShell unter `%LOCALAPPDATA%\WorldDB\verify-target`, Unix unter `$XDG_CACHE_HOME/worlddb/verify-target` oder `$HOME/.cache/worlddb/verify-target`.

```powershell
.\scripts\verify.ps1
```

```sh
./scripts/verify.sh
```

## Grenzen und Folgearbeit

Die Rust-Builds liefen auf Windows x86_64 MSVC mit Rust/Cargo 1.85.0. Der PowerShell-Wrapper wurde nativ, der Unix-Wrapper mit Git Bash auf demselben Windows-Rechner ausgeführt; native Linux-/macOS-Läufe folgen der Plattformmatrix in M0-14. Die sichtbaren SKIP-Einträge werden in M0-11, M0-12 und M0-14 ersetzt, sobald deren jeweilige Policy bzw. CI eingerichtet ist. Die 52 HARD- und zwei GUARDED-Quellenlücken aus M0-02a bleiben offen und blockieren weiterhin das M0-15-Gate.
