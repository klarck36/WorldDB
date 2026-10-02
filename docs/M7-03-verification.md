# M7-03 – Migration Dry Run

**Status:** lokal abgeschlossen am 2. Oktober 2026; Windows-Prüfung. Linux/macOS-Nachweise bleiben gemäß Arbeitsplan bei M9-07.

## Implementierter Vertrag

- `MigrationDryRun::run` lädt exakt die im unveränderlichen Plan benannte Transformer-Version. Dry Run und `MigrationTransformer::transform_records` verwenden denselben internen Transformationsdurchlauf; die Preview-Variante ergänzt nur begrenzte Fehlerdetails.
- Der Report bindet den Planfingerprint und die Transformer-Version. Er enthält Quell-/Ergebnis-Recordcounts, Quell-/Ergebnisbytes, den exakten Transformfingerprint, Work-/Speicheradmission sowie getrennte Listen für Warnings, Unresolved Items und Fehler.
- Der Prüfsink verwirft Ergebnisframes nach dem gemeinsamen Durchlauf; der Report behält nur Zähler, Budgetwerte und Digest.
- Die Speicheradmission verwendet bewusst dieselbe konservative Obergrenze wie eine Ausführung mit gespeicherten Ergebnisframes. Die Diagnosebytes werden zusätzlich innerhalb des verbleibenden Planbudgets reserviert.
- Die kanonische Wire-Prüfung sammelt alle fehlerhaften Recordpositionen. Bei einem Fehler wird kein Teilergebnis ausgegeben. Abweichende Source-Preconditions und Work-/Speichergrenzen blockieren den Prüflauf.
- Diagnoseeinträge sind durch den verbleibenden Plan-Speicher und eine feste Obergrenze von 256 Einträgen beschränkt. Der Report nennt Gesamtfehler und ausgelassene Einzeldetails; ausgelassene Diagnosen lassen `preflight_complete` falsch.
- Transformer-Version 1 ist für Recordframes ein exakter, geordneter Byte-Passthrough. Sie erzeugt deshalb keine Warnings oder Unresolved Items. Die getrennten Ergebnisarten sind vorhanden; Adminentscheidung und Freigabe sind ausdrücklich M7-04/M7-05. Ein vollständiger Dry Run ist keine Commitautorisierung.

## Nachweise

- Restrictive- und Breaking-Pläne erzeugen denselben Fingerprint und dieselben Bytes wie ein direkter Aufruf des versionierten Transformers.
- Mehrere nichtkanonische Records werden mit ihren stabilen Positionen gemeldet; ein Teilresultat bleibt aus.
- Veraltete Source-Schema-Preconditions und ein überschrittenes Workbudget liefern einen fatalen Fehler ohne Transformresultat.
- Der Speichergrenzfall hält Diagnosebytes innerhalb des verbleibenden Jobbudgets und meldet nicht gespeicherte Fehlerdetails vollständig als Anzahl.
- `tools/check_migration_transform.py` prüft jetzt Transformer und Prüfsink. Vier Policytests bestätigen die geschlossene deterministische Abhängigkeitsgrenze.

## Windows-Abschlussprüfung

- `cargo test --locked --workspace`: **PASS**; 474 Core-Unit-Tests und 84 Rustdoc-Tests bestanden. Vorhandene manuelle Langläufe blieben ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo check --locked --workspace --all-targets`: **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- Migrations-Policychecker und vier Python-Policytests: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.
- Plancheck, Sourcecheck, Contract-Docs und `git diff --check`: **PASS**.

Dieser Nachweis deckt den lokal implementierten Transformer-v1-Vertrag ab. Die Ausführungsparität, administrative Entscheidungen und Plattformnachweise außerhalb Windows folgen M7-04/M7-05 beziehungsweise M9-07.
