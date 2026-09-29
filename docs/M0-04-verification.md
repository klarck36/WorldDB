# M0-04 – Entity-/Perspective-Vertrag

**Prüfdatum:** 2026-09-29 02:27:55 +02:00  
**Ergebnis:** PASS für den spezifizierten Vertrag und seine Dokumentationsprüfungen

## Ergebnis

- `docs/contract/entity_perspective_contract.md` legt für Entity und Perspective Identität, projectweite Revisionierung, typed references, EntityType-Zuweisung, Metadaten, Retirement, historische Sicht und Berechtigungsgrenzen fest.
- `docs/contract/ADR-030-entity-perspective.md` hält die Entscheidungen und Folgen fest. Die Retirement-IDs ergänzen die bisherige Lifecycle-ID-Liste und `RecordRef`-Enum ausdrücklich; die bestehenden Lifecycle-Endpointregeln bleiben maßgeblich.
- Der Ergänzungstext prüft die Beziehungen zu Master §§2.1, 2.3, 3.1–3.2, 17, 31.2.1, 31.3 und 33. Er erhält die Trennung von HistorySpace, Layer, Perspektive, Schema und Security. Die 52 HARD-Quellenlücken und WDB-HIS-001 bleiben für M0-02a offen.
- Der source-derived Master enthält die spezifizierten Entity-/Perspective-Zeilen und den vollständigen Vertrag. Das maschinenlesbare Register stimmt weiterhin feldgenau mit den 22 First-Class-Zeilen in §33 überein.
- Das Register der unveränderlichen Quellen wurde nicht verändert. Die Quellenprüfung zählt weiterhin 253 Invarianten, 149 Haupttextbindungen und 54 Quellenlücken (52 HARD, 2 GUARDED).

## Reproduzierbare Prüfungen

Alle folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
```

Docs Verify meldete `22 First-Class types; required fields and Master §33 match`. Plancheck meldete `236 tasks, 11 milestones, 253 invariants, 153 follow-up pairs`; Sourcecheck bestätigte alle Quellspiegel und das ZIP.

Zwei isolierte temporäre Fixtures wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. `RecordRef::EntityRetirement(EntityRetirementId)` aus der Spezifikation entfernt: erforderliche Retirement-Variante erkannt.
2. `perspective.use` aus der Spezifikation entfernt: erforderlicher Berechtigungsbereich erkannt.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach den Prüfungen entfernt.

## Prüfgrenze

Das Repository enthält noch keine Engine oder UI. Daher belegt M0-04, dass die Anforderungen und Fehlerfälle ohne still erfundene Recordsemantik spezifiziert und strukturell prüfbar sind. Laufzeit-, Codec-, Berechtigungs- und UI-Tests gehören zu den späteren Implementierungstasks; die Spezifikation benennt diese Prüfpflichten.
