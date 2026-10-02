# M7-02 – Deterministischer Transformer

**Status:** lokal abgeschlossen 2026-10-02; Windows-Prüfung. Linux/macOS-Nachweise bleiben gemäß Arbeitsplan bei M9-07.

## Implementierter Vertrag

- `MigrationTransformer` ist eine geschlossene, versionsgebundene Implementierung. Version 1 validiert kanonische Record-Frames und gibt sie in derselben Reihenfolge unverändert zurück. Unbekannte oder nichtkanonische Eingaben werden zurückgewiesen; es gibt keine stille Coercion.
- Das Transformationsresultat bindet geordnete Ergebnisbytes, Planfingerprint und Transformer-Version in einem BLAKE3-Fingerprint. Arbeits- und Speicheradmission sind endlich; ein Fehler liefert kein Teilergebnis.
- Start prüft Source-Schema-Revision, Source-Fingerprint und verfügbare Transformer-Version. Resume prüft zusätzlich die MigrationId des Runs. Nicht unterstützte oder abweichende Versionen sperren den Transformationsstart.
- Ein Plan darf optional einen expliziten Kalender-Shift enthalten: TimelineId, UTC-Epochenversatz in i128-Nanosekunden, Gregorianisches Profil, `CalendarPeriod` und Richtung. Alle Werte sind im Planfingerprint gebunden. Die checked Transformation entspricht `constraint_time_contract.md`: Jahre, Monate, dann zivile Tage; ungültige Monatstage werden nach Jahr-/Monatsschritten auf den letzten gültigen Tag geklemmt. Floor-Division erhält vor-epochale Uhrzeiten; Zeitzone, Sommerzeit, Schaltsekunden, Host-Uhr und Rundung werden nicht eingeführt.
- Der Commit-Transformer hat keine Callback-Schnittstelle und keine Uhr-, Zufalls-, Locale-, Netzwerk-, Datei-, Prozess- oder AI-Abhängigkeit. `DecoderLimits::DEFAULT` hält die Wire-Prüfung von variabler Prozesskonfiguration unabhängig.

## Persistenz und Kompatibilität

MigrationPlan-Feld 13 enthält den optionalen Kalender-Shift nur, wenn er gesetzt ist. Bestehende M7-01-Pläne ohne Shift behalten ihre v1-Fingerprints und exakt dieselben Wire-Bytes; der bereits festgelegte Golden-Vektor bleibt unverändert. Pläne mit Shift verwenden die v2-Fingerprintdomäne und einen eigenen Golden-Vektor. Das geschlossene Decoderformat weist unbekannte Profile, ungültige Perioden und unvollständige Nested-Felder als Fehler auf Feld 13 zurück.

Der Dry-Run-Prüfsink und die persistente Step-Ausführung rufen denselben Transformer noch nicht auf; ihre Integration und Paritätsprüfung erfolgen in M7-03/M7-05. Die administrative Freigabe eines überprüften Plans wird in M7-04/M7-05 integriert. Daher bleiben WDB-MIG-010 und WDB-MIG-012 in der Invariantenabdeckung bis zu diesen Nachweisen offen; WDB-MIG-011 ist mit diesem Task belegt.

## Nachweise

- 12 Transformer-Unit-Tests, darunter identische Wiederholung, Reihenfolgetreue, Arbeits-/Speichergrenzen, nichtkanonische Frames, Start-/Resume-Versionssperre, Planfingerprintbindung und Persistenz-zu-Transformationspfad.
- Ein unabhängiges Gregorianisches Referenzmodell stimmt in 1.296 Fällen überein. Separate Fälle prüfen Monatsende, Schaltjahr, negative Richtung, Timeline-Abweichung und i128-Überlauf.
- Golden-Frame-Roundtrip für M7-01-Plan ohne Kalenderparameter und M7-02-Plan mit Kalenderparameter; fehlerhafte Feld-13-Varianten werden gezielt zurückgewiesen.
- Statischer Capability-Check `tools/check_migration_transform.py` verbietet Uhr, Zufall, Locale, Netzwerk, AI, Datei-/Prozesszugriff und Callback-Injektion. Drei Negativtests belegen, dass verbotene Pfade erkannt werden.

## Abschlussprüfung auf Windows

- `cargo test --locked --workspace`: **PASS**; 470 Core-Unit-Tests und 84 Rustdoc-Tests bestanden. Decoder-Fuzz-Langlauf, M6-14b-Performanceprobe, Präzisionskampagne und 100.000-Punkte-Recoverykampagne blieben als ausdrücklich manuelle Langläufe ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- `cargo check --locked --workspace --all-targets`: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.
- Plancheck, Sourcecheck, Contract-Docs, statische Transformer-Policy, Python-Policytests und `git diff --check`: **PASS**.

Nicht-Windows-CI- und Plattformnachweise werden nicht als durch diesen lokalen Lauf erbracht ausgegeben; sie bleiben bei M9-07.
