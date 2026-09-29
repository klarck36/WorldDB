# M0-04b – Constraints und Zeitrepräsentation

**Prüfdatum:** 2026-09-29 03:05:14 +02:00

**Ergebnis:** PASS für den spezifizierten Vertrag und seine strukturellen Dokumentationsprüfungen

## Ergebnis

- `docs/contract/constraint_time_contract.md` definiert `ConstraintSet` als geschlossene, typisierte Konjunktion mit Parser-, Kanonisierungs-, Wertebereichs- und `ValueKind`-Regeln. `EntityTypeConstraint` bleibt separat; Decimal-Skala ist keine numerische Identität.
- Timeline- und TimeUnit-Registrierung liegt auf der Revision-Achse. Symbole, Zeitskalen und Kalenderabbildungen sind stabil; Zeiten werden nur innerhalb derselben Timeline exakt normalisiert und verglichen. Lifecycle, historische Schema-Snapshots und inkompatible Timelines sind geregelt.
- `CalendarPeriod` ist ein kanonischer, nichtnegativer Schema-/Query-/Migrationsparameter und kein Assertion-Value. Kalenderprofil, UTC-Epoch, gregorianische Schaltjahre, negative Zeitkoordinaten, Reihenfolge der Periodenkomponenten, Monatsend-Clamping und Overflow sind festgelegt.
- Zulässige Schemaverwendung ist die optionale maximale Kalender-Spanne eines spanfähigen EventKind. Die bestehende `start < end`-Regel bleibt bestehen; offene Spans werden bei `EventSpanClosure` geprüft. Relative Queryfenster haben explizite Anker und Halb-offen-Grenzen. Migration Dry Run und Ausführung teilen Validator und Transformer.
- ADR-032 und der M0-04b-Supplement sind an die reproduzierbare Master-Arbeitskopie angehängt. `source-errata.json` bindet ihre Hashes. Es wurden keine WDB-Invariant-IDs oder klassifizierten Quellenlücken geändert.

## Reproduzierbare Prüfungen

Alle folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
git diff --check
```

Die Prüfer meldeten 22 First-Class-Typen, 253 Invarianten, 149 MAIN-L-Bindungen und 54 offene Quelllücken (52 HARD, 2 GUARDED). Sourcecheck bestätigte die Bytegleichheit der sechs Spiegel und das unveränderte ZIP. Plancheck bestätigte 236 Tasks, 11 Milestones und 153 Folgebeleg-Paare.

Drei isolierte temporäre Fixtures wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. Fehlender Parser-/Typprüfvertrag für Constraint-Werte.
2. Fehlende Eindeutigkeit der Timeline-/TimeUnit-Registrierung.
3. Fehlende Gleichheit der Validator-/Transformer-Regeln zwischen Migration Dry Run und Ausführung.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach den Prüfungen entfernt.

## Prüfgrenze

Es gibt noch keine Engine, Parser-Implementierung oder UI. M0-04b belegt die konkrete, reproduzierbar eingebundene Arbeitsvertragssemantik. Laufzeitprüfungen zu Constraint-Auswertung, Gregorianischer Zeitverschiebung, Queryfenstern, Schemaentwicklung, Migration und Fault-Atomicity gehören zu den nachgelagerten Implementierungstasks.
