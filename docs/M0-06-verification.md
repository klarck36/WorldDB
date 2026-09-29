# M0-06 – Repository-, Plattform- und Toolchain-Inventur

**Prüfdatum:** 2026-09-29 04:29:44 +02:00

**Ergebnis:** PASS für den versionierten Iststand, die offenen Zuständigkeiten und den lokal validierten, nicht synchronisierten Laufzeitpfad

**Inventurcommit:** `bf3ab10` (`docs: inventory WorldDB platform and toolchain`)

## Ergebnis

- [Inventur](M0-06-environment-inventory.md) verankert die reproduzierbare Repo-Baseline `0bb3c85`. Dieser erste ausführbare Repo-Code enthält Plan-/Quellenprüfer; WorldDB-Runtime und Rust-Workspace existieren noch nicht und folgen M0-09 nach M0-07/08.
- Lizenzentscheidung und CI sind offen und explizit als solche registriert. `LICENSE*`, SPDX-Angaben, CI-Konfiguration, `Cargo.toml`, `rust-toolchain.toml` und `.gitignore` fehlen. Lokal verfügbar sind Git 2.54.0, Python 3.13.3 und PowerShell 7.6.5; `rustc`, `cargo` und `rustup` fehlen.
- Das aktuelle Profil ist Windows NT `10.0.26200.0` auf NTFS. Drei Zielprofile sind dokumentiert: Windows/NTFS vorhanden, macOS/APFS und Linux/ext4 ohne zugeordneten Host. Signierung, Notarisierung, Pilot-, Produkt-/Release- und fehlende Hostrollen sind unbesetzt und mit Fälligkeit versehen.
- `%LOCALAPPDATA%\WorldDB\test-runs` löst auf diesem Host zu `C:\Users\wedde\AppData\Local\WorldDB\test-runs` auf. Der Basisordner existiert, ist kein Reparse-Point, und der vollständige Zielpfad liegt außerhalb des OneDrive-Syncroots. Das Zielverzeichnis wurde nicht angelegt; es gab keinen Datenbank-, Crash- oder Performance-Lauf.
- Die verbindliche Laufregel verbietet DB-Dateien, WAL/Segmente, Crashkopien und Messkorpora im OneDrive-Projektpfad. Der erste Produktcodecommit ist noch offen; M0-06 verlangt keine nicht verfügbare Rust-Runtime und claimt kein Durability-Ergebnis.

## Reproduzierbare Prüfungen

Die folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
git diff --check
```

Die lokale Inventur hat außerdem Commitinhalt `0bb3c85`, ausführbare Versionsausgaben, vorhandene Projektmanifest-/Lizenz-/CI-Dateien, `C:`-Dateisystem, `%LOCALAPPDATA%`-Reparse-Eigenschaft und Pfadbeziehung zum OneDrive-Syncroot geprüft. Die Strukturprüfer bestätigten weiterhin 236 Tasks, 11 Milestones, 253 Invarianten und 153 Folgebeleg-Paare; sechs Quellspiegel und Audit-ZIP stimmen bytegenau.

## Prüfgrenze

Die Inventur ist kein Lizenzentscheid, keine CI-Einrichtung, keine macOS-/Linux-Hostzusage und kein Laufzeittest. Die offenen Hosts und Verantwortlichen bleiben bis zu ihren dokumentierten Terminen sichtbar. Ein späterer Durability-/Crash-/Performance-Nachweis muss auf dem jeweiligen nicht synchronisierten Hostpfad entstehen.
