# M0-14 – Anbieterneutrale CI-Matrix

## Jobvertrag

`policy/ci-matrix.tsv` deklariert genau drei Pflichtjobs: Linux, Windows und macOS. Jeder Job verlangt einen sauberen Checkout, Rust/Cargo gemäß `rust-toolchain.toml` (1.85.0), den Verify-Profileintrag `dev`, `cargo xtask verify` und den Step-Manifestpfad `tools/verify/steps.tsv`. Der Dev-Verify führt seinerseits no-default-, default- und all-features-Builds aus.

`tools/check_ci_matrix.py` prüft die Matrix gegen den gepinnten Toolchainchannel, die tatsächliche Featurematrix und das aktive Verify-Manifest. Mutationstests weisen fehlende Plattformen, doppelte Jobs, falschen MSRV, unvollständige Featureprofile, einen unsauberen Checkoutvertrag, falschen Verifybefehl und falschen Artefaktpfad ab.

## Job-Einstieg und Artefakte

Jeder CI-Job kann `python -B -X utf8 tools/run_ci_job.py --job-id <linux-msrv|windows-msrv|macos-msrv>` aufrufen. Der Einstieg lehnt Dirty-Checkouts, die falsche Plattform, einen Toolchainfehler, abweichende Verify-Skipzeilen und fehlgeschlagene Schritte ab. Er führt `cargo xtask verify` aus und legt `steps.tsv`, stdout, stderr sowie `ci-job.json` unter `WORLDDB_CI_ARTIFACT_ROOT/<run-id>` ab. Das JSON bindet Run-ID, Betriebssystem, Architektur, Rust-/Cargo-Version, Target, Commit, Featureprofile, Exitcode und Artefakte mit SHA-256. Der CI-Anbieter muss dieses Verzeichnis anschließend als Jobartefakt archivieren.

## Abnahmegrenze

Matrixvertrag und Job-Einstieg sind lokal prüfbar und verwenden keine CI-Anbieter-Labels. Sie ersetzen weder eine aktive Pipelinekonfiguration noch echte saubere Anbieterjobs. Linux/WSL2 und Windows wurden lokal geprüft; die macOS-Zelle und der Upload auf einem ausgewählten CI-Dienst bleiben offen. Der sichtbare `ci-matrix`-Skip bleibt bis zu diesen Läufen bestehen, daher ist M0-14 weiterhin `BLOCKED`.
