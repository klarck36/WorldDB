# M0-14 – CI-Matrix: Umgebungsprüfung und Blocker

**Status:** BLOCKED

**Geprüft:** 2026-09-29T10:21:01+02:00

**Basis:** sauberer Commit `c2f9cec54d95aa6e88dd97ae3f41c5e01f57e50c`

## Verfügbare lokale Läufe

- Windows x86_64 MSVC, NTFS, Rust/Cargo 1.85.0: `cargo xtask verify` auf dem sauberen Commit bestand mit **24 PASS, 1 sichtbarem `ci-matrix`-SKIP und 0 FAIL**. Der finale M0-13-Nachweis liegt unter `C:\Users\wedde\AppData\Local\WorldDB\test-runs\M0-13-final\M0-13-20260929-final\evidence.json`.
- Ubuntu unter WSL2, Linux x86_64 GNU, ext4, Rust/Cargo 1.85.0: sauberer lokaler Klon von `c2f9cec`; `sh scripts/verify.sh` bestand ebenfalls mit **24 PASS, 1 sichtbarem `ci-matrix`-SKIP und 0 FAIL**. Die drei Featureprofile bestanden. Der M0-13-Evidenzlauf enthält OS-/Dateisystem-/Toolchainangaben und Quell-Snapshot unter `/home/wedde/WorldDB/test-runs/M0-14-linux/M0-14-linux-wsl2-20260929/evidence.json`.

Der WSL-Lauf ist ein lokaler Linux-Zusatznachweis, kein externer CI-Job. Beide Läufe behalten den M0-14-SKIP bei und ersetzen ihn nicht durch eine behauptete Pipeline.

## Anbieterneutrale Vorbereitung

`policy/ci-matrix.tsv` deklariert inzwischen die drei OS-Jobs, Rust 1.85.0, `cargo xtask verify`, die Featureprofile und das zu archivierende Step-Manifest. `tools/run_ci_job.py` prüft sauberen Checkout/OS/MSRV und erzeugt `steps.tsv`, Logs und ein JSON-Jobmanifest unter einem extern gesetzten Artefakt-Root. Der Matrixcheck und die Mutationstests sind Teil des lokalen Verify-Pfads. Diese Dateien sind ein direkt verwendbarer Anbieter-Einstieg, aber noch keine aktive CI-Pipeline.

## Konkreter Blocker

Der Arbeitsplan verlangt Linux-, Windows- und macOS-Jobs aus einem sauberen Checkout, die jeweils ihr Step-Manifest archivieren. Vorhanden ist nur dieser Windows-Rechner und Ubuntu/WSL2 auf demselben Rechner. Es gibt keinen macOS-Host, kein konfiguriertes CI-System und kein Git-Remote. Der Nutzer hat GitHub vorerst zurückgestellt; ein anderer Anbieter wurde nicht ausgewählt. Ohne Anbieter und verfügbaren macOS-Runner lassen sich die Pflichtjobs nicht als CI einrichten oder real ausführen. `M0-14` bleibt daher `BLOCKED`.

## Zum Fortsetzen erforderlich

1. Einen CI-Anbieter benennen, den lokal versionierten Pipeline-Einstieg und Artefakt-Upload verwenden dürfen.
2. Einen macOS-Runner/Host auf diesem Anbieter verfügbar machen.

Nach Bereitstellung setzt M0-14 mit clean-checkout Jobs, MSRV 1.85, den Profilen no-default/default/all-features und archivierten Step-Manifesten fort. Der neutrale Runner muss dann auf dem macOS-Job verifiziert und vom Anbieter gestartet werden; es wurde kein Anbieter-Workflow auf Verdacht angelegt.
