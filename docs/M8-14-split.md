# M8-14 – Aufgabenzerlegung

Stand: 4. Oktober 2026

Die ursprüngliche Abnahme M8-14 erfordert drei noch nicht vorhandene Bausteine: dauerhafte, autorisierte Erfassung; eine persistenzgebundene Auflösungsvorschau; und eine Desktop-Oberfläche. Nach §2.1 des Arbeitsplans wurde vor jeder Produktcode-Implementierung zerlegt. Die fachliche Abnahme bleibt unverändert und liegt in M8-14c.

| Task | Inhalt | Voraussetzung |
|---|---|---|
| M8-14 | Planung/Abdeckung der Zerlegung; genau die erste Untertask freigeben | M8-13a |
| M8-14a | WAL-/Manifest-gebundene Assertion-, Mask- und Boundary-Erfassung mit Validierung; policy-geforderter AuditRecord im selben Commit | M8-14 |
| M8-14b | Auflösungsvorschau mit Known/Unknown/Conflict, technischen Fehlern und explizit leerer Menge | M8-14a |
| M8-14c | Windows-Desktop-Formulare und getrennte Darstellung der Auflösungsergebnisse; vollständige ursprüngliche Abnahme | M8-14b |
| M8-14d | Die zuvor geplante Korrektur-, Archive- und Retraction-Oberfläche | M8-14c |

Es wurden keine Produktsemantik und keine Invariante geändert. Die bestätigten Entscheidungen bleiben maßgeblich: Symbolgrammatik [a-z][a-z0-9_]*, TransferLineage als eigener Record, WorldTimeSelector für alle Zeiten oder einen Zeitpunkt sowie strikt nachgelagerte ArchiveTransitionen. Linux/macOS-Nachweise folgen wie vereinbart M9-07.

Prüfbelege vom 4. Oktober 2026:

- python -X utf8 WorldDB_1.0_Plancheck.py — PASS: 255 Tasks, 11 Milestones, 253 Invarianten, 224 Folgebelegpaare; DAG und Referenzen gültig.
- python -X utf8 WorldDB_1.0_Sourcecheck.py — PASS: gleiche Planstruktur; alle sechs Quellenkopien und das Audit-ZIP bytegenau bestätigt.
- git diff --check — PASS.

Es wurden keine Produkttests ausgeführt, da diese Planungs-Task keinen Produktcode ändert. M8-14a ist nachweislich die einzige READY-Task.

Veröffentlichung:

- Commit: 65f4bf0b428b4caf0e24352358f79cb889c9b195 (docs: split M8-14 into implementation tasks).
- Push zu origin/codex/worlddb-project-integration erfolgreich; der anschließende Remote-Head-Abgleich lieferte denselben vollständigen Commit-Hash.
