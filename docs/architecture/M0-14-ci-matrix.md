# M0-14 – Anbieterneutrale CI-Matrix

## Jobvertrag

`policy/ci-matrix.tsv` deklariert genau drei Pflichtjobs: Linux, Windows und macOS. Jeder Job verlangt einen sauberen Checkout, Rust/Cargo gemäß `rust-toolchain.toml` (1.85.0), den Verify-Profileintrag `dev`, `cargo xtask verify` und den Step-Manifestpfad `tools/verify/steps.tsv`. Der Dev-Verify führt seinerseits no-default-, default- und all-features-Builds aus.

`tools/check_ci_matrix.py` prüft die Matrix gegen den gepinnten Toolchainchannel, die tatsächliche Featurematrix und das aktive Verify-Manifest. Mutationstests weisen fehlende Plattformen, doppelte Jobs, falschen MSRV, unvollständige Featureprofile, einen unsauberen Checkoutvertrag, falschen Verifybefehl und falschen Artefaktpfad ab.

## Job-Einstieg und Artefakte

Jeder CI-Job kann `python -B -X utf8 tools/run_ci_job.py --job-id <linux-msrv|windows-msrv|macos-msrv>` aufrufen. Der Einstieg lehnt Dirty-Checkouts, die falsche Plattform, einen Toolchainfehler, abweichende Verify-Skipzeilen und fehlgeschlagene Schritte ab. Er führt `cargo xtask verify` aus und legt `steps.tsv`, stdout, stderr sowie `ci-job.json` unter `WORLDDB_CI_ARTIFACT_ROOT/<run-id>` ab. Das JSON bindet Run-ID, Betriebssystem, Architektur, Rust-/Cargo-Version, Target, Commit, Featureprofile, Exitcode und Artefakte mit SHA-256. Der CI-Anbieter muss dieses Verzeichnis anschließend als Jobartefakt archivieren.

## Abnahmegrenze

Matrixvertrag und Job-Einstieg sind lokal prüfbar und verwenden keine CI-Anbieter-Labels. Saubere lokale Windows- und WSL2/Linux-Checkouts auf Commit `8e05029` bestanden jeweils mit 27 Verify-Schritten, einem sichtbaren `ci-matrix`-Skip und 0 Fehlern. Beide Läufe archivierten und hashten das Step-Manifest und die Logs. Sie ersetzen weder echte Anbieterjobs noch den macOS-Lauf. Die lokale Arbeit nutzt lokale Git-Commits ohne Remote-Host; ein GitHub-Zugang ist für M1–M8 nicht erforderlich. `M0-14` bleibt bis zu einem externen Anbieter- und macOS-Lauf `BLOCKED`; der lokale M0-15-Vorfreigabepunkt gibt die Entwicklung M1–M8 frei, ohne den M0-Meilenstein als abgeschlossen auszugeben. M0-14 und WDB-ENG-005 sind zwingende Abhängigkeiten von M9-13b und M10-10.
