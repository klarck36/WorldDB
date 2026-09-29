# M0-14 – CI-Matrix: Umgebungsprüfung und Blocker

**Status:** BLOCKED

**Geprüft:** 2026-09-29T11:03:00+02:00

**Basis:** sauberer Commit `8e0502961b46a26733c5342008df1227bec2874e`

## Saubere lokale Jobläufe

- Windows x86_64 MSVC, NTFS, Rust/Cargo 1.85.0: Der providerneutrale `windows-msrv`-Job startete und endete mit sauberem Checkout. `cargo xtask verify` meldete **27 PASS, 1 sichtbaren `ci-matrix`-SKIP und 0 FAIL**. Das Manifest und die gehashten Artefakte liegen unter `C:\Users\wedde\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\WorldDB\ci-jobs\windows-fixed\M0-14-windows-msrv-20260929T083922Z-112657e9\ci-job.json`.
- Ubuntu unter WSL2, Linux x86_64 GNU, ext4, Rust/Cargo 1.85.0: Der providerneutrale `linux-msrv`-Job lief mit demselben Commit und sauberem Checkout. Ergebnis ebenfalls **27 PASS, 1 sichtbarer `ci-matrix`-SKIP und 0 FAIL**. Manifest und gehashte Artefakte: `/home/wedde/WorldDB/ci-jobs/linux-fixed/M0-14-linux-msrv-20260929T084039Z-06961c53/ci-job.json`.

Beide Manifeste bestätigen die Profile `no-default`, `default` und `all-features`; sie archivieren jeweils das zum Commit gehörende `steps.tsv`, stdout und stderr und prüfen die SHA-256-Werte. Das sind erfolgreiche lokale Runnerläufe auf Windows und WSL2, keine externen CI-Ausführungen. Der sichtbare `ci-matrix`-Skip bleibt deshalb bestehen.

Der erste saubere Windows-Klon deckte auf, dass `core.autocrlf=true` die hashgebundenen TSV-Testfixtures in CRLF umschrieb. `.gitattributes` setzt `crates/worlddb-testkit/testdata/m0-13/**` nun auf bytegetreue Auschecke (`-text`). Der danach erneut angelegte Windows-Klon bestand vollständig.

## Anbieterneutrale Vorbereitung

`policy/ci-matrix.tsv` deklariert Linux-, Windows- und macOS-Jobs, Rust 1.85.0, `cargo xtask verify`, die Featureprofile und das zu archivierende Step-Manifest. `tools/check_ci_matrix.py` prüft den Vertrag; sieben Mutationstests prüfen unter anderem Plattformabdeckung, Toolchain, Featureprofile und Artefaktmanifest. `tools/run_ci_job.py` prüft sauberen Checkout, Betriebssystem, MSRV, Skip-Zeilen und Verify-Ergebnis und erzeugt das SHA-256-gebundene JSON-Manifest samt Logs und Step-Manifest. Sechs Jobvalidierungstests prüfen Erfolg und Fehlerfälle. Die Vorbereitung ist im Commit `a7dec86` enthalten; die bytegetreue Fixturekorrektur in `8e05029`.

## Konkreter Blocker

Der Arbeitsplan verlangt Linux-, Windows- und macOS-Jobs aus sauberen Checkouts. Lokal bestehen Windows und WSL2/Linux. Es gibt weiterhin keinen macOS-Host, keinen ausgewählten CI-Anbieter und kein Git-Remote. GitHub wurde auf Nutzervorgabe zurückgestellt; ein anderer Anbieter wurde noch nicht ausgewählt. Die geprüften Alternativen stehen in [M0-14-provider-options.md](M0-14-provider-options.md): GitLab.com Open Source wird empfohlen, falls ein öffentliches GitLab-Projekt und die jährliche Programmverlängerung akzeptabel sind; CircleCI kann mit einem unterstützten Repository-Host betrieben werden. Damit fehlen weiter der echte Anbieterjob samt Artefaktupload und die macOS-Ausführung. `M0-14` bleibt `BLOCKED`.

## Zum Fortsetzen erforderlich

1. Einen CI-Anbieter benennen, über den die providerneutralen Jobs gestartet und Jobartefakte archiviert werden.
2. Einen macOS-Runner auf diesem Anbieter verfügbar machen.

Danach muss `macos-msrv` aus einem sauberen Checkout laufen und der Anbieter die Step-Manifeste archivieren. Es wurde kein Anbieter-Workflow auf Verdacht angelegt.
