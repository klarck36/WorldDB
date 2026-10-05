# M0-14 – Anbieterneutrale CI-Matrix

## Jobvertrag

`policy/ci-matrix.tsv` deklariert genau drei Pflichtjobs: Linux, Windows und macOS. Jeder Job verlangt einen sauberen Checkout, Rust/Cargo gemäß `rust-toolchain.toml` (1.85.0), den Verify-Profileintrag `dev`, `cargo xtask verify` und den Step-Manifestpfad `tools/verify/steps.tsv`. Der Dev-Verify führt seinerseits no-default-, default- und all-features-Builds aus.

`tools/check_ci_matrix.py` prüft die Matrix gegen den gepinnten Toolchainchannel, die tatsächliche Featurematrix und das aktive Verify-Manifest. Mutationstests weisen fehlende Plattformen, doppelte Jobs, falschen MSRV, unvollständige Featureprofile, einen unsauberen Checkoutvertrag, falschen Verifybefehl und falschen Artefaktpfad ab.

## Job-Einstieg und Artefakte

Jeder CI-Job kann `python -B -X utf8 tools/run_ci_job.py --job-id <linux-msrv|windows-msrv|macos-msrv>` aufrufen. Der Einstieg lehnt Dirty-Checkouts, die falsche Plattform, einen Toolchainfehler, abweichende Verify-Skipzeilen und fehlgeschlagene Schritte ab. Er führt `cargo xtask verify` aus und legt `steps.tsv`, stdout, stderr sowie `ci-job.json` unter `WORLDDB_CI_ARTIFACT_ROOT/<run-id>` ab. Das JSON bindet Run-ID, Betriebssystem, Architektur, Rust-/Cargo-Version, Target, Commit, Featureprofile, Exitcode und Artefakte mit SHA-256. Der CI-Anbieter muss dieses Verzeichnis anschließend als Jobartefakt archivieren.

## Abnahmegrenze

Matrixvertrag und Job-Einstieg sind lokal prüfbar und verwenden keine CI-Anbieter-Labels. Saubere lokale Windows- und WSL2/Linux-Checkouts auf Commit `8e05029` bestanden jeweils mit 27 Verify-Schritten, einem sichtbaren `ci-matrix`-Skip und 0 Fehlern. Beide Läufe archivierten und hashten das Step-Manifest und die Logs. Der externe Windows-only-Lauf [#37330953047](https://github.com/klarck36/WorldDB/actions/runs/37330953047) besteht auf Commit `6cda119` mit 39 PASS, einem erwarteten `ci-matrix`-SKIP und 0 FAIL; seine Artefakte liegen beim GitHub-Lauf. Linux und macOS wurden in diesem Lauf übersprungen. Der Windows-Beleg ersetzt weder einen vollständigen externen Matrixlauf noch den macOS-Lauf. `M0-14` bleibt bis zu diesen Plattformnachweisen `BLOCKED`; der lokale M0-15-Vorfreigabepunkt gibt die Entwicklung M1–M8 frei, ohne den M0-Meilenstein als abgeschlossen auszugeben. M0-14 und WDB-ENG-005 sind zwingende Abhängigkeiten von M9-13b und M10-10.

## GitHub-Actions-Anbindung

`.github/workflows/m0-14-ci.yml` ist ein manueller Einstieg: `windows-only` ist die Voreinstellung; `full-matrix` ergänzt Linux und macOS. Es gibt keinen Push-Trigger. `.github/workflows/m0-14-ci-job.yml` installiert die festgelegten Toolchains und ruft für jeden Job denselben Runner auf. Actions sind auf vollständige Commit-SHAs gepinnt. Die vier vom Runner erzeugten Belegdateien werden bis zu 30 Tage als GitHub-Artefakt archiviert. Der erfolgreiche Windows-only-Lauf bestätigt den Windows-Job; die vollständige Anbietermatrix einschließlich macOS bleibt erforderlich, um M0-14 abzuschließen.
