# M6-14a – Versionierter Messkorpus

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Korpus

Der Generator `worlddb-testkit m6-14a-corpus` erzeugt das versionierte Schema `m6-14a-v1` mit Little-Endian-Datensätzen und dem reproduzierbaren SplitMix64-v1-Stream. Seed: `0x574f524c44444232`. Der Full-Korpus enthält 100.000 Entities, 1.000.000 Assertions, 10.000 Events, 100 HistorySpaces und separat 10.000.000 Provenance-Kanten. Zusätzlich enthält er 200.324 Masken (20,0324 % der Assertions). Die im Manifest festgehaltenen Schieflagen decken häufige und lange Verteilungsschwänze für Entity-, Predicate-, Value-, Event- und Provenance-Typen ab.

Die sechs Binärdateien umfassen 413.206.784 Byte. Ihr Manifest mit Schema, Dateigrößen, Einzelhashes, aggregiertem Dataset-Hash, Seed, Compiler- und Hardwareprofil liegt unter `crates/worlddb-testkit/testdata/m6-14a/manifest.json`. Die Rohdaten wurden außerhalb des Repositories und OneDrive in `LOCALAPPDATA\WorldDB\benchmarks\m6-14a-v1` abgelegt. Ins Repository kommt nur das Manifest.

## Reproduzierbarkeit und Host

Zwei unabhängige Full-Läufe mit demselben Seed erzeugten für alle sechs Dateien identische SHA-256-Werte und denselben aggregierten Hash:

`a8f623e9f82329e5b626ca21a6ada1b1af1e765184e278a9a711549066d962e5`

Der Manifestprüfer verifizierte beide erzeugten Rohkorpora gegen das versionierte Manifest. Das Hostprofil erfasst Windows 11 Home Build 26200, Rust/Cargo 1.85.0, Intel Core i5-14600K mit 14 physischen und 20 logischen Kernen, 68.448.346.112 Byte RAM sowie ein festes NTFS-Volume auf Samsung SSD 970 EVO Plus 1TB NVMe. Die Ausgabe lag auf lokalem, nicht synchronisiertem Speicher. Das Profil dient der Reproduzierbarkeit und ist kein allgemeiner Leistungs- oder Haltbarkeitsnachweis.

## Prüfung

- `tools/m6-14a/build_corpus.py` erzeugte den Full-Korpus zweimal mit dem festgehaltenen Seed.
- `tools/m6-14a/verify_corpus.py` prüfte Manifest und sämtliche Rohdateien beider Läufe.
- `python -B -X utf8 tools/m6-14a/verify_corpus.py --metadata-only`: PASS.
- `python -B -X utf8 -m unittest discover -s tools/m6-14a -p test_*.py -v`: 3 PASS; manipulierte Hash- und Zählwerte werden abgewiesen.
- `cargo test --locked --workspace --quiet`: 438 Core- und 82 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 36 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- Linux- und macOS-Läufe bleiben wie angewiesen zurückgestellt.

## Abgrenzung

M6-14a erstellt und identifiziert den Messkorpus; es beansprucht keine Performance-Ergebnisse. M6-14b führt die Plattformmessungen aus und muss Rohdaten, Cache-/Durability-Modus und Quantile festhalten. Die Linux- und macOS-Anteile werden später ergänzt.
