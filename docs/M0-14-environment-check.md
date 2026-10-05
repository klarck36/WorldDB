# M0-14 – CI-Matrix: Umgebungsprüfung und Blocker

**Status:** BLOCKED

**Geprüft:** 2026-10-05T17:24:09+02:00

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

## Aktueller Anbieterstand

GitHub Actions ist für das öffentliche Repository konfiguriert. `.github/workflows/m0-14-ci.yml` ist manuell startbar und wählt standardmäßig ausschließlich den Windows-Job. Die Auswahl `full-matrix` schaltet zusätzlich Linux und macOS ein; diese beiden Jobs bleiben bis zum späteren Plattformnachweis zurückgestellt. Es gibt keinen Push-Trigger. `.github/workflows/m0-14-ci-job.yml` ruft den provider-neutralen Runner auf und archiviert dessen Manifest, Step-Manifest und Logs.

Die Actions sind SHA-gepinnt. Rust 1.85.0 erfüllt den Projekt-MSRV; Rust 1.88.0 wird nur für die Installation des auf 0.20.2 gepinnten cargo-deny verwendet. Die Konfiguration allein ist kein externer CI-Nachweis; der erfolgreiche Windows-only-Lauf ist im folgenden Abschnitt dokumentiert.

## Externe Windows-only-Läufe

Der dritte manuelle Lauf [#37330953047](https://github.com/klarck36/WorldDB/actions/runs/37330953047) besteht auf Commit `6cda1190d27b14d38c2347a39883791e06b96377`. Der saubere Windows-x86_64-MSVC-Checkout lief mit Rust/Cargo 1.85.0; `cargo xtask verify` meldete **39 PASS, 1 erwarteten `ci-matrix`-SKIP und 0 FAIL**. Alle drei Featureprofile (`no-default`, `default`, `all-features`) liefen; `checkout_clean_before` und `checkout_clean_after` sind `true`. Das GitHub-Artefakt [m0-14-windows-msrv-37330953047-1](https://github.com/klarck36/WorldDB/actions/runs/37330953047/artifacts/11354234118) enthält `ci-job.json`, `steps.tsv` und beide Logs. Der heruntergeladene ZIP-Hash stimmt mit GitHub überein: `SHA-256 54dd24b227a88fb01b81822e2f0711c1f6713192d0262ac941706bb9fc00e442`.

Die ersten zwei Dispatches sind Fehlerbelege und wurden behoben: [Run #37328115435](https://github.com/klarck36/WorldDB/actions/runs/37328115435) scheiterte beim Workflowstart, weil `runner.temp` auf Job-Ebene verwendet wurde; Commit `d2535c1` verschob die Runner-Variable in den Jobschritt. [Run #37328625559](https://github.com/klarck36/WorldDB/actions/runs/37328625559) erreichte den Windows-Job, aber der bytegehashte M7-16h-Fixture-Hash änderte sich durch Windows-Zeilenenden. Commit `6cda119` ergänzt dafür den `-text`-Schutz in `.gitattributes`; der erneute saubere Windows-Checkout besteht.

Im erfolgreichen Lauf wurden die GitHub-Jobs Linux und macOS explizit als übersprungen angezeigt ([Linux](https://github.com/klarck36/WorldDB/actions/runs/37330953047/job/111833641186), [macOS](https://github.com/klarck36/WorldDB/actions/runs/37330953047/job/111833642617)). Es wurden dafür keine Plattformprüfungen gestartet.

## Konkreter Blocker

Der externe Windows-only-Job besteht nun; es fehlen weiterhin die vollständige externe Matrix samt Linux-/macOS-Läufen und deren archivierte Manifeste. `M0-14` bleibt deshalb `BLOCKED`, hält M1–M8 nach M0-15 aber nicht an. Vor M9-13b und M10-10 müssen die vollständige Plattformmatrix und WDB-ENG-005 durch tatsächliche CI-Läufe belegt werden. Linux und macOS bleiben bis zur späteren Prüfung zurückgestellt. Mögliche zusätzliche Anbieter stehen in [M0-14-provider-options.md](M0-14-provider-options.md).

## Zum Fortsetzen erforderlich

1. Nach den zurückgestellten Plattformprüfungen und vor M9-13b die vollständige manuelle GitHub-Actions-Matrix auf dem dann aktuellen Produktstand aus sauberen Checkouts starten.
2. Das bereits gesicherte Windows-Manifest sowie die Linux- und macOS-Manifeste, Step-Manifeste und Verify-Logs als Anbieterartefakte prüfen und archivieren.

Danach muss `macos-msrv` aus einem sauberen Checkout laufen und GitHub Actions die Step-Manifeste archivieren. Die Linux/macOS-Jobs sind vorbereitet, werden aktuell aber nicht gestartet.
