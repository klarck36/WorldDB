# M5-02 – Dateilayout, Formatprobe und Writerlock

**Ergebnis:** DONE auf Windows; lokale Vertragsabnahme am 2026-10-01.

## Umsetzung

`worlddb-storage-file` stellt ein festes Datenbanklayout mit `FORMAT`, `LOCK`, `CURRENT`, `wal/`, `segments/`, `manifests/`, `audit/wal/`, `audit/segments/`, `staging/` und `quarantine/` bereit. Beim Anlegen wird `FORMAT` zuletzt geschrieben. Beim Öffnen werden Pflichtverzeichnisse, Dateiart, kanonische Pfadgrenzen und die Prüfsumme der Formatprobe geprüft.

Die Version 1.0 unterstützt keine Required-Capability-Bits. Unbekannte Required-Bits werden fail-closed abgewiesen. Optional-Bits bleiben als undurchsichtiger Rohwert erhalten; die aktuelle Implementierung weist ihnen keine Semantik zu. `resave_format` schreibt zunächst in `staging/` unter demselben Datenbankroot, synchronisiert die Datei und veröffentlicht sie per Rename. Ein exklusiver, nicht blockierender Prozesslock schützt Format-Resaves und wird beim Freigeben des Datei-Handles vom Betriebssystem gelöst.

Die Lock-Implementierung verwendet die sichere `fs4`-Datei-Lock-API. Diese Arbeit behauptet noch keine Machine-Durability und ersetzt nicht die NTFS-Sync-, Replace-, Write-through-, Antivirus- und Crashabnahme aus M5-09/M5-22.

## Windows-Nachweise

- `cargo test --locked --workspace`: 396 Core-, 8 Storage-File- (3 Unit-, 5 Integrationstests), 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; zwei CPU-Langläufe bleiben absichtlich ignoriert.
- `m5_02_contract.rs`: festes Layout und root-lokales Staging; unbekannte optionale Bits bleiben nach Open/Resave bitgenau erhalten; unbekannte erforderliche Bits werden mit gültiger Prüfsumme abgewiesen; ein zweiter Windows-Prozess kann den gehaltenen Writerlock weder erwerben noch den Format-Resave ausführen und kann nach Freigabe den Lock erwerben.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `python -B WorldDB_1.0_Plancheck.py` und `python -B WorldDB_1.0_Sourcecheck.py`: bestanden.
- Dependency-Policy, `cargo-deny` und Crategraph-Prüfung: bestanden; direkte Storage-Crate-Abhängigkeiten `fs4` und der nur in Testfixtures verwendete `blake3`-Hasher sind registriert.
- `cargo xtask verify`: 32 PASS, 1 erwarteter `ci-matrix`-SKIP für M0-14, 0 FAIL. M0-13-Evidenzlauf `M0-13-20261001T113024Z-e232e8b0df` ist PASS.
- `git diff --check HEAD`: bestanden; GitHub/Remote wurde nicht verwendet.

## Abgrenzung

Nur Windows wurde in dieser Task ausgeführt. Linux/ext4 und macOS/APFS sind nicht durch diese Ergebnisse abgedeckt und bleiben für die späteren Plattformaufgaben zurückgestellt. Der separate Windows-Adapter- und Crashnachweis M5-09/M5-22 ist ebenfalls offen.
