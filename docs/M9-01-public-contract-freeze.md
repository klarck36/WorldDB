# M9-01 – Öffentliche Verträge für 1.0.0-rc.1 eingefroren

**Status:** DONE
**Abnahme:** 6. Oktober 2026, Windows
**RC-Version:** `1.0.0-rc.1`

## Eingefrorene Oberflächen

Der unveränderliche Snapshot steht in
`contracts/public/v1.0.0-rc.1/manifest.json`. Er versioniert die nummerierte
Rust-Fassade `worlddb_core::api::v1` mit Protokoll `1.0`, geschlossene
Wire-Registries, öffentliche Fehlercodes und Zuordnungen, persistente
Formatkennungen sowie Logical Export v2. Die API- und Implementierungsquellen
sind zusätzlich per SHA-256-Fingerprint gebunden; nicht anderweitig erfasste
Quelländerungen gelten vorsorglich als brechend.

Der Snapshot enthält 10 `ValueTag`-Werte, 38 Recordtypen, 25
Record-Referenztypen, zwei Audittypen und 31 Auditwerte. Er friert 14
Transportcodes sowie 71 Core-Fehlercodes mit 71 vollständigen Zuordnungen zu
den Transportcodes ein. Der Prüfer lehnt unvollständige, doppelte oder auf
nicht definierte Transportcodes zeigende Zuordnungen ab.

Die persistierten Formate stehen auf Frame, Segment und Security-Segment
`1.0`; Manifest- und CURRENT-Magics sind ebenfalls im Snapshot enthalten.
Logical Export ist Version 2 mit Magic `WDBLEX\0\x02`, dem gebundenen
Digest-Kontext, einem Limit von 512 MiB, 1.000.000 Records und 65.536
HistorySpaces. Die normativen Semantiken und fünf ausgelassenen
Storageklassen stehen in `docs/contracts/logical-export-v2.md`.

## Maschinenprüfung und Klassifikation

`tools/check_public_contracts.py` vergleicht den Arbeitsbaum mit dem
versionierten Snapshot. Ohne Änderungen meldet der Prüfer null Abweichungen;
`--json` liefert einen maschinenlesbaren Bericht. `--write-baseline` legt den
Snapshot nur an, wenn er noch nicht existiert, und überschreibt eine bestehende
RC-Baseline nicht.

`contracts/public/classification-rules.tsv` ordnet jede erkannte Änderung als
`BREAKING` oder `ADDITIVE` ein und nennt die Versionsregel samt Begründung.
Entfernte oder geänderte Fehlerzuordnungen, Änderungen geschlossener Wiretags,
Formatmajor-/Magic-Änderungen und Änderungen der bestehenden Logical-Export-v2
Semantik sind brechend. Ein API-Protokoll-Minor- oder kompatibler
Format-Minor-Anstieg sowie ein neuer stabiler Fehlercode sind additiv.
Unstrukturierte Änderungen an gebundenen Quellen werden konservativ als
brechend ausgewiesen.

## Verifikation

- `python -B -X utf8 tools/check_public_contracts.py`: keine Abweichungen zur RC-Baseline.
- `python -B -X utf8 -m unittest discover -s tools -p test_public_contracts.py -v`: 12 Tests bestanden; darunter Mutationen für API-Version, Wiretags, Fehlercodes und Zuordnungen, Formatversionen, Logical-Export-Semantik, Quellfingerprints und Ressourcenlimits.
- `WorldDB_1.0_Plancheck.py`, `WorldDB_1.0_Sourcecheck.py`, Vertragsdokument-Prüfung und `git diff --check`: bestanden.
- `cargo xtask verify`: 43 Schritte bestanden, ein vorgesehener `ci-matrix`-Skip aus M0-14, null Fehler.

Der Snapshot beschreibt den 1.0 RC-Vertrag; er ist keine plattformübergreifende
Release-Abnahme. Native macOS-/Linux-Belege bleiben gemäß Projektplan bei
M9-07.
