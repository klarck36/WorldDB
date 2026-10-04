# M8-14 – Aufgabenzerlegung

Stand: 4. Oktober 2026

Die ursprüngliche Abnahme M8-14 erfordert autorisierte, dauerhafte Assertion-/Mask-/Boundary-Erfassung, eine Auflösungsvorschau und Desktop-Eingaben. Vor Produktcode ergab die Quellenprüfung zusätzlich, dass ADR-032 projektweite, revisionierte Timeline- und TimeUnit-Schemaregister zwingend für die exakte Zeitprüfung verlangt; entsprechende Typen und Records fehlen im aktuellen Code. M8-14 wurde daher vorgezogen um diese bereits normative Voraussetzung erweitert. Die ursprüngliche M8-14-Abnahme liegt in M8-14e.

| Task | Inhalt | Voraussetzung |
|---|---|---|
| M8-14 | Planung/Abdeckung; genau die erste Untertask freigeben | M8-13a |
| M8-14a | Core-Schema, Wireformat, Lifecycle und persistente Timeline-/TimeUnit-Register nach ADR-032 | M8-14 |
| M8-14b | Autorisierte Schema-API und Desktop-Eingabe für Timeline-/TimeUnit-Definitionen | M8-14a |
| M8-14c | WAL-/Manifest-gebundene, autorisierte Assertion-, Mask- und Boundary-Erfassung; policy-required Audit im selben Commit | M8-14b |
| M8-14d | Auflösungsvorschau mit Known/Unknown/Conflict, technischen Fehlern und explizit leerer Menge | M8-14c |
| M8-14e | Windows-Desktop-Formulare und getrennte Darstellung der Ergebnisse; vollständige ursprüngliche M8-14-Abnahme | M8-14d |
| M8-14f | Die zuvor geplante Korrektur-, Archive- und Retraction-Oberfläche | M8-14e |

Es wurden keine Produktsemantik und keine Invariante geändert. Die bestätigten Entscheidungen bleiben maßgeblich: Symbolgrammatik [a-z][a-z0-9_]*, TransferLineage als eigener Record, WorldTimeSelector für alle Zeiten oder einen Zeitpunkt sowie strikt nachgelagerte ArchiveTransitionen. Linux/macOS-Nachweise folgen wie vereinbart M9-07.

Die ADR-032-Voraussetzung steht in docs/contract/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md, Abschnitt 2, Zeilen 2452–2475. Dort sind TimelineDefinition und TimeUnitDefinition als unveränderliche, revisionierte Records festgelegt; der aktuelle SchemaDefinition- und Record-Code kennt diese Familien noch nicht. Der Core-Writevalidator verlangt jedoch registrierte Timeline- und Unit-Definitionen.

Prüfbelege vom 4. Oktober 2026:

- python -X utf8 WorldDB_1.0_Plancheck.py — PASS: 257 Tasks, 11 Milestones, 253 Invarianten, 230 Folgebelegpaare; DAG und Referenzen gültig.
- python -X utf8 WorldDB_1.0_Sourcecheck.py — PASS: gleiche Planstruktur; alle sechs Quellenkopien und das Audit-ZIP bytegenau bestätigt.
- git diff --check — PASS.
- Keine Produkttests ausgeführt, da diese Planungs-Task keinen Produktcode ändert. M8-14a ist die einzige READY-Task.

Die erste Planfassung wurde in 65f4bf0b428b4caf0e24352358f79cb889c9b195 erfasst. Die Dokumentation des Pushs steht in 9cd251b; die Policy-required-Audit-Klarstellung in 9aae209.

Die ADR-032-Erweiterung wurde in Commit d50016d (docs: add ADR-032 time registry prerequisite to M8-14) gespeichert und nach origin/codex/worlddb-project-integration gepusht.

## M8-14a-Abnahme

M8-14a ist am 4. Oktober 2026 auf Windows abgeschlossen. Die beiden revisionierten Register-Records, Schemahistorie, persistente Wiederöffnung, exakte Nanosekunden-Normalisierung und die Schreibreferenzprüfung bestehen ihre gezielten Tests. Weil das Logical-Export-Klassenmanifest geschlossen ist und nun 38 Recordvarianten umfasst, wurde das Vor-Alpha-Envelope auf v2 gehoben; die v2-Fixture aktualisiert nur die abgeleiteten Logical-Export-Dateien und erhält den eingefrorenen physischen Storage-/Backup-Snapshot. Der vollständige Windows-Nachweis steht in `docs/M8-14a-verification.md`.

`cargo xtask verify` bestand beim Abschluss der Zerlegung mit 39 PASS, einem erwarteten M0-14-`ci-matrix`-SKIP und 0 FAIL. Plancheck, Sourcecheck und `git diff --check HEAD` bestanden ebenfalls. M8-14b war danach READY; Linux/macOS bleiben bis M9-07 zurückgestellt.

## M8-14b-Abnahme

M8-14b ist am 4. Oktober 2026 auf Windows abgeschlossen. Autorisierte Schema-API und Desktopformulare veröffentlichen Timelines mit optionalem gregorianischem UTC-Epochprofil sowie TimeUnits mit positiver exakter Nanosekundenskala. Dezimalwerte bleiben über UI und IPC verlustfrei; Deprecated-/Retired-Zustände, Regelhinweise und historische/explizite Ansichten bestehen die In-Process- und Sidecar-Smokes. Der vollständige Nachweis steht in `docs/M8-14b-verification.md`; M8-14c ist READY. Linux/macOS folgen M9-07.
