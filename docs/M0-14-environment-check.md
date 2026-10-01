# M0-14 – CI-Matrix: Umgebungsprüfung und Blocker

**Status:** BLOCKED

**Geprüft:** 2026-09-29T13:58:29+02:00

**Historischer Stand:** ursprüngliche saubere Läufe auf Commit `9e2d94894806112099efc560c4d524cb22e51470`; frühere Baseline-Läufe auf `8e0502961b46a26733c5342008df1227bec2874e`. Der neueste lokale Vorfreigabebeleg steht in `docs/gates/M0.md`.

## Frühere saubere lokale Jobläufe

- Windows x86_64 MSVC, Rust/Cargo 1.85.0: Der `windows-msrv`-Job lief mit `checkout_clean_before=true` und `checkout_clean_after=true`. `cargo xtask verify` meldete **27 PASS, 1 sichtbaren `ci-matrix`-SKIP und 0 FAIL**. Das Manifest und die gehashten Artefakte liegen unter `C:\Users\wedde\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\WorldDB\ci-jobs\windows-fixed\M0-14-windows-msrv-20260929T092026Z-c87c8d0f\ci-job.json`.
- Linux x86_64 GNU unter WSL2, Rust/Cargo 1.85.0: Der `linux-msrv`-Job lief aus einem sauberen Linux-Checkout mit `checkout_clean_before=true` und `checkout_clean_after=true`. Ergebnis ebenfalls **27 PASS, 1 sichtbarer `ci-matrix`-SKIP und 0 FAIL**. Manifest und gehashte Artefakte: `/home/wedde/WorldDB/ci-jobs/linux-fixed/M0-14-linux-msrv-20260929T092111Z-2ad8af7b/ci-job.json`.

Beide Manifeste beziehen sich auf Commit `9e2d94894806112099efc560c4d524cb22e51470`, bestätigen die Profile `no-default`, `default` und `all-features`, archivieren das zugehörige `steps.tsv`, stdout und stderr und prüfen die SHA-256-Werte. Das sind erfolgreiche lokale Runnerläufe auf Windows und WSL2, keine externen CI-Ausführungen. Der sichtbare `ci-matrix`-Skip bleibt deshalb bestehen.

## Aktuelle lokale Läufe für M0-15

Auf dem sauberen Commit `493506c7e144c5c1763ff560ac8aba35c5fc3118` bestanden Windows x86_64 MSVC und WSL2/Linux x86_64 GNU erneut jeweils mit Rust/Cargo 1.85.0. Beide `cargo xtask verify`-Läufe meldeten 27 PASS, 1 sichtbaren `ci-matrix`-SKIP und 0 FAIL; beide Checkouts waren davor und danach sauber. Die anbieterneutralen Runner-Manifeste und sämtliche Artefakthashes sind in [M0-Gateprotokoll](gates/M0.md) eingetragen. Diese Läufe belegen den lokalen M0-15-Vorfreigabepunkt, aber keinen externen CI-Job; M0-14 bleibt `BLOCKED`.

Der erste saubere Windows-Klon deckte auf, dass `core.autocrlf=true` die hashgebundenen TSV-Testfixtures in CRLF umschrieb. `.gitattributes` setzt `crates/worlddb-testkit/testdata/m0-13/**` nun auf bytegetreue Auschecke (`-text`). Der danach erneut angelegte Windows-Klon bestand vollständig.

## Anbieterneutrale Vorbereitung

`policy/ci-matrix.tsv` deklariert Linux-, Windows- und macOS-Jobs, Rust 1.85.0, `cargo xtask verify`, die Featureprofile und das zu archivierende Step-Manifest. `tools/check_ci_matrix.py` prüft den Vertrag; sieben Mutationstests prüfen unter anderem Plattformabdeckung, Toolchain, Featureprofile und Artefaktmanifest. `tools/run_ci_job.py` prüft sauberen Checkout, Betriebssystem, MSRV, Skip-Zeilen und Verify-Ergebnis und erzeugt das SHA-256-gebundene JSON-Manifest samt Logs und Step-Manifest. Sechs Jobvalidierungstests prüfen Erfolg und Fehlerfälle. Die Vorbereitung ist im Commit `a7dec86` enthalten; die bytegetreue Fixturekorrektur in `8e05029`.

## Konkreter Blocker

Die Entwicklung läuft mit lokalen Git-Commits ohne Remote-Host weiter; eine GitHub-Verbindung ist dafür nicht erforderlich. Lokal bestehen Windows und WSL2/Linux. Es fehlen weiterhin ein externer Anbieterjob, eine macOS-Ausführung und die externe Artefaktarchivierung. `M0-14` bleibt deshalb `BLOCKED`, hält M1–M8 nach M0-15 aber nicht an. Vor M9-13b und M10-10 müssen die Plattformmatrix und WDB-ENG-005 mit einem tatsächlichen macOS-CI-Lauf belegt werden. Mögliche Anbieter für diesen späteren Nachweis stehen in [M0-14-provider-options.md](M0-14-provider-options.md).

## Zum Fortsetzen erforderlich

1. Vor M9-13b einen externen CI-Anbieter mit macOS-Runner bereitstellen oder auswählen.
2. Die Matrix auf dem dann aktuellen Produktstand aus sauberen Checkouts laufen lassen und Step-Manifeste samt Job-Artefakten archivieren.

Danach muss `macos-msrv` aus einem sauberen Checkout laufen und der Anbieter die Step-Manifeste archivieren. Es wurde kein Anbieter-Workflow auf Verdacht angelegt.
