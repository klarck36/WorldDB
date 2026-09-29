# M0-14 – CI-Matrix: Umgebungsprüfung und Blocker

**Status:** BLOCKED

**Geprüft:** 2026-09-29T11:21:16+02:00

**Basis:** aktuelle saubere Läufe auf Commit `9e2d94894806112099efc560c4d524cb22e51470`; frühere Baseline-Läufe auf `8e0502961b46a26733c5342008df1227bec2874e`

## Saubere lokale Jobläufe auf dem aktuellen Commit

- Windows x86_64 MSVC, Rust/Cargo 1.85.0: Der `windows-msrv`-Job lief mit `checkout_clean_before=true` und `checkout_clean_after=true`. `cargo xtask verify` meldete **27 PASS, 1 sichtbaren `ci-matrix`-SKIP und 0 FAIL**. Das Manifest und die gehashten Artefakte liegen unter `C:\Users\wedde\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\WorldDB\ci-jobs\windows-fixed\M0-14-windows-msrv-20260929T092026Z-c87c8d0f\ci-job.json`.
- Linux x86_64 GNU unter WSL2, Rust/Cargo 1.85.0: Der `linux-msrv`-Job lief aus einem sauberen Linux-Checkout mit `checkout_clean_before=true` und `checkout_clean_after=true`. Ergebnis ebenfalls **27 PASS, 1 sichtbarer `ci-matrix`-SKIP und 0 FAIL**. Manifest und gehashte Artefakte: `/home/wedde/WorldDB/ci-jobs/linux-fixed/M0-14-linux-msrv-20260929T092111Z-2ad8af7b/ci-job.json`.

Beide Manifeste beziehen sich auf Commit `9e2d94894806112099efc560c4d524cb22e51470`, bestätigen die Profile `no-default`, `default` und `all-features`, archivieren das zugehörige `steps.tsv`, stdout und stderr und prüfen die SHA-256-Werte. Das sind erfolgreiche lokale Runnerläufe auf Windows und WSL2, keine externen CI-Ausführungen. Der sichtbare `ci-matrix`-Skip bleibt deshalb bestehen.

Der erste saubere Windows-Klon deckte auf, dass `core.autocrlf=true` die hashgebundenen TSV-Testfixtures in CRLF umschrieb. `.gitattributes` setzt `crates/worlddb-testkit/testdata/m0-13/**` nun auf bytegetreue Auschecke (`-text`). Der danach erneut angelegte Windows-Klon bestand vollständig.

## Anbieterneutrale Vorbereitung

`policy/ci-matrix.tsv` deklariert Linux-, Windows- und macOS-Jobs, Rust 1.85.0, `cargo xtask verify`, die Featureprofile und das zu archivierende Step-Manifest. `tools/check_ci_matrix.py` prüft den Vertrag; sieben Mutationstests prüfen unter anderem Plattformabdeckung, Toolchain, Featureprofile und Artefaktmanifest. `tools/run_ci_job.py` prüft sauberen Checkout, Betriebssystem, MSRV, Skip-Zeilen und Verify-Ergebnis und erzeugt das SHA-256-gebundene JSON-Manifest samt Logs und Step-Manifest. Sechs Jobvalidierungstests prüfen Erfolg und Fehlerfälle. Die Vorbereitung ist im Commit `a7dec86` enthalten; die bytegetreue Fixturekorrektur in `8e05029`.

## Konkreter Blocker

Der Arbeitsplan verlangt Linux-, Windows- und macOS-Jobs aus sauberen Checkouts. Lokal bestehen Windows und WSL2/Linux. GitHub Actions ist für später vorgesehen; GitHub wurde auf Nutzervorgabe noch nicht eingerichtet. Ein zweiter Anbieter ist nicht erforderlich. Damit fehlen bis zur späteren GitHub-Einrichtung weiterhin der externe Anbieterjob samt Artefaktupload und die macOS-Ausführung. `M0-14` bleibt `BLOCKED`. Optionale Alternativen, falls GitHub doch nicht genutzt wird, stehen in [M0-14-provider-options.md](M0-14-provider-options.md).

## Zum Fortsetzen erforderlich

1. GitHub später für CI einrichten.
2. Die Matrix einschließlich `macos-msrv` aus sauberen Checkouts laufen lassen und die Step-Manifeste archivieren.

Danach muss `macos-msrv` aus einem sauberen Checkout laufen und der Anbieter die Step-Manifeste archivieren. Es wurde kein Anbieter-Workflow auf Verdacht angelegt.
