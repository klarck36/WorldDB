# M0-06 – Repository-, Plattform- und Toolchain-Iststand

**Prüfstand:** 2026-09-29 04:27:02 +02:00
**Repository:** `C:\Users\wedde\OneDrive\Documents\ChatGPT\Projekt Helper`
**Zweck:** versionierter Iststand und verbindliche Grenzen für spätere Builds, Datenbankläufe und Releaseverantwortung; kein Infrastruktur- oder Release-Gate

## Repository und reproduzierbare Basis

- Lokales Git-Repository, Branch `main`; der Arbeitsplan verlangt Windows, macOS und Linux als Zielplattformen. Die aktuelle Arbeitskopie liegt unter OneDrive und ist ein Synchronisationspfad.
- Ausgangspunkt des aktuellen Arbeitsabschnitts ist M0-05-Abschlusscommit `2103b88`. Die erste reproduzierbare Repository-Baseline ist `0bb3c85` (`chore: establish WorldDB source baseline`): sie enthält Plan-/Quellenregister, Hash-/Bytevergleich, den vollständigen Prüfplan und die geprüfte Audit-ZIP. Die zwei Python-Prüfskripte in diesem Commit sind Repository-Werkzeuge, keine WorldDB-Runtime.
- Es gibt noch kein `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, Produkt-README oder WorldDB-Rust-Quellcode. Der erste Rust-Workspace ist in M0-09 nach Toolchainentscheidung M0-07 und UUIDv7-Entscheidung M0-08 geplant. Er muss `--locked` und aus dem versionierten Stand reproduzierbar sein.
- `.gitattributes` behandelt `*.zip` als binär, hält `docs/source/*` und `docs/contract/*` bytegetreu (`-text`) und verwendet keine allgemeine EOL-Normalisierung. Vor jeder künftigen Code-/Toolchaininitialisierung sind Generatoren und Zeilenenden explizit festzulegen.

## Lizenz, CI und lokale Buildvoraussetzungen

| Bereich | Beobachteter Stand | Folgerung / zuständige Folgearbeit |
|---|---|---|
| Projektlizenz | Kein `LICENSE*`, `COPYING*`, SPDX-Header oder Lizenzentscheid im Repository gefunden. | Lizenz **UNENTSCHIEDEN**; weder Lizenz noch Weitergaberecht aus der Audit-ZIP ableiten. Product Owner unbesetzt; Entscheidung vor erstem Produktcode-/Dependency-Commit in M0-09/M0-12 festhalten und vor Veröffentlichung erneut prüfen. |
| CI | Keine Workflowdatei und kein CI-System konfiguriert. | Keine Build-/Plattformmatrix behaupten. CI-Anbieter und Credentials bleiben nicht eingerichtet; M0-14 verantwortet die Matrix für Linux, Windows, macOS, MSRV und Featureprofile. |
| Git | Git `2.54.0.windows.1`; aktueller branch `main`. | Versionsnummer ist beobachtet, noch keine portable Git-Mindestversion beschlossen. |
| Dokument-/Registerchecks | Python `3.13.3`; die versionierten Source-, Plan- und Vertragsprüfer laufen lokal. | Diese Umgebung reicht für die aktuelle Dokumentationsarbeit, nicht als Rust-Buildnachweis. |
| Rust | `rustc`, `cargo` und `rustup` fehlen; es existiert kein Rust-Projektmanifest. | Kein Build-/Testresultat für WorldDB-Code. M0-07 wählt und pinnt MSRV; M0-09 legt den minimalen Workspace an; danach installiert die lokale Buildumgebung genau den gepinnten Toolchainpfad. |
| Shell/OS | PowerShell `7.6.5`; Windows NT `10.0.26200.0`; Systemlaufwerk `C:` ist `NTFS`. | Dies ist der einzige aktuell nachweisbare lokale OS-/Dateisystem-Profilstand. Die Versionsangabe ist die OS-API-Version, keine separate Freigabe einer Windows-Produktedition. |

Der Repository-Baselinecommit `0bb3c85` ist bereits reproduzierbar und belegt den ersten versionierten ausführbaren Prüfcode. **Produktcode existiert noch nicht**; M0-09 erstellt ihn erst nach den festgelegten ODE-Voraussetzungen. M0-06 gibt weder einen Rust-Build noch ein Durability-Ergebnis vor.

## Plattform- und Dateisystem-Testhosts

Die Plattformziele kommen aus Abschnitt 1 des Arbeitsplans. Die späteren nativen E2E-Runs gehören zu M8-26d. Eine Plattformzelle ist erst verfügbar, wenn ein konkreter Host, Betreiber und Laufprotokoll eingetragen sind.

| Zielprofil | Hoststatus | Dateisystem | Zuständigkeit / Termin | Nachweisstatus |
|---|---|---|---|---|
| Windows | Aktuelle Workspace-Umgebung vorhanden: OS-API `10.0.26200.0` | NTFS auf `C:` | Betreiber für spätere Produkt-Runs nicht separat benannt; M8-26d | Umgebung erkannt; kein WorldDB-Datenbanklauf ausgeführt |
| macOS | Host nicht zugeordnet | APFS | Host/Betreiber unbesetzt; vor M9-13a bereitstellen, nativer Lauf M8-26d | Kein Host oder Nachweis vorhanden |
| Linux | Host nicht zugeordnet | ext4 | Host/Betreiber unbesetzt; vor M9-13a bereitstellen, nativer Lauf M8-26d | Kein Host oder Nachweis vorhanden |

Diese Tabelle weist die drei erforderlichen Profile samt Dateisystemen aus; sie behauptet nicht, dass macOS-/Linux-Hosts bereits verfügbar sind. Keine Durability-, Crash- oder Performancefreigabe erfolgt durch diese Inventur.

## Nicht synchronisierter Laufzeit- und Datenpfad

**Vorgesehener lokaler Pfad:** `C:\Users\wedde\AppData\Local\WorldDB\test-runs` (`%LOCALAPPDATA%\WorldDB\test-runs` auf diesem Host).

Die Basis `C:\Users\wedde\AppData\Local` ist vorhanden und kein Reparse-Point. Der vollständige Zielpfad liegt außerhalb des festgestellten OneDrive-Synchronisationsroots `C:\Users\wedde\OneDrive`; das Zielverzeichnis selbst existiert noch nicht. Es wird erst durch den jeweiligen Test-Harness/Run angelegt.

Verbindliche Laufregel für M0-09 und spätere Storage-/Crash-/Messaufgaben:

1. Quelldateien, Verträge und normale Dokumentation bleiben im vorhandenen Projektpfad. Datenbankdateien, WAL/Segmente, temporäre Restoreziele, Crashkopien, Messkorpora und rohe Run-Artefakte liegen ausschließlich unter `%LOCALAPPDATA%\WorldDB\test-runs\<task-id>\<run-id>` oder einem für einen anderen konkreten Host analog lokal validierten, nicht synchronisierten Pfad.
2. Vor einem Lauf werden auf dem wirklichen Zielhost Pfad, Volume-Dateisystem und Synchronisations-/Reparse-Eigenschaften geprüft und im Manifest festgehalten. Wenn der Zielpfad synchronisiert ist, die Prüfungen unbekannt sind oder der Run auf OneDrive landet, wird der Durability-/Crash-/Performance-Run **nicht gestartet**.
3. Ergebnisse in Git bestehen aus redigiertem Bericht, Manifest, Hashes und erforderlichen kleinen Fixtures. Datenbankkopien, Secrets, Nutzdaten und große Laufkorpora werden nicht in dieses synchronisierte Quellenverzeichnis übernommen.
4. Diese Task hat den vorgesehenen Windows-Pfad geprüft, aber keine WorldDB-Datenbankdatei angelegt oder getestet. Für macOS/APFS und Linux/ext4 ist erst vor dem jeweiligen Run ein lokaler Pfad auf dem konkreten Host zu validieren.

## Zuständigkeiten und Fälligkeiten

| Rolle | Besetzung | Fälligkeit | Erwarteter Beleg |
|---|---|---|---|
| Signierungsverantwortung | UNBESETZT | vor M9-13a | benannte Person/Key-Owner und versionierter Signatur-/Key-Lifecycle-Prozess |
| macOS-Notarisierung | UNBESETZT | vor M9-13a | benannte Person, Apple-Konto/Secret-Verwaltung und erfolgreicher Notarisierungsnachweis |
| Pilotkoordination und reale Pilotnutzer | UNBESETZT | vor M9-13a | namentlich zugeordnete Rolle und vereinbarter Pilotkorpus/-ablauf |
| Produktverantwortung | UNBESETZT | vor M9-13a | benannte Entscheidungsperson für Produktumfang und Pilot-Abnahme |
| Releasefreigabe | UNBESETZT | vor M10-10 | versionierte Freigabe durch eine konkrete Person für genau das geprüfte Bundle |
| macOS-/Linux-Testhost-Betreiber | UNBESETZT | vor M9-13a | konkrete Host-/OS-/Dateisystemangabe, Betreiber und lokaler Runpfad je Profil |

Die Rollen bleiben offen und werden nicht von Luna oder einem Rollenlabel implizit ausgefüllt. Die Fälligkeit liegt vor den jeweils vorgesehenen Abnahmen; bis dahin ist kein Pilot-/Release-Erfolg behauptet.

## Abnahmegrenze und Folgearbeiten

M0-06 erfasst den vorhandenen lokalen Zustand, lässt Lizenz-, CI-, Host- und Freigaberollen sichtbar offen und fixiert einen nachweislich nicht synchronisierten Pfad für zukünftige Datenläufe. Der reproduzierbare Repository-Ausgangspunkt ist `0bb3c85`; es gibt keinen WorldDB-Rust-Build oder Durability-Run. Folgearbeiten sind M0-07 (MSRV), M0-08 (UUIDv7), M0-09 (Rust-Workspace), M0-10–12 (Verify-/Policy-Tooling) und M0-14 (CI-Matrix). Keine Aufgabe darf Laufzeitdaten auf dem OneDrive-Projektpfad ablegen oder einen Crash-/Durability-Check dort als bestanden werten.
