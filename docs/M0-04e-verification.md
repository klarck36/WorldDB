# M0-04e – Assertion-Correction-Vertrag

**Prüfdatum:** 2026-09-29 04:11:37 +02:00

**Ergebnis:** PASS für die spezifizierten Korrekturaktionen und ihre strukturelle Dokumentationsprüfung

**Vertragscommit:** `97d3894` (`docs: specify WorldDB correction actions`)

## Ergebnis

- [Korrekturvertrag](contract/correction_contract.md) und [ADR-035](contract/ADR-035-correction-actions.md) definieren getrennte, typisierte `CorrectAssertion`- und `CorrectEvent`-Aktionen an der normalen Transaktionsgrenze.
- `CorrectAssertion` schreibt atomar eine neue Assertion, eine ausdrückliche `AssertionRetraction` gegen das Ziel und eine `Corrects`-Provenance-Kante. Die Kante selbst ändert niemals den Lifecycle. Slot, Historienraum, Layer, Rechte, Validierung, Konflikt und Commit-Outcome sind explizit.
- `CorrectEvent` schreibt einen neuen Event und eine `Corrects`-Kante, erzeugt aber keine implizite EventRetraction, Span-Schließung, Mask oder Eventrelation. Das Original bleibt aktiv, bis eine separate ausdrückliche Aktion es zurücknimmt.
- Der Vertrag beschreibt zusätzlich aktuelle Commit-Autorisierung, die erforderlichen Action- und Datenrechte, sichere Behandlung unsichtbarer Ziele, vollständige Audit-/Atomizitätsgrenzen, Retry-/Unknown-Commit-Regeln und die UI-Erfolgsquittung.
- Der generierte Master enthält den M0-04e-Supplement; `source-errata.json` registriert `ERR-M0-04E-CORRECTION-ACTIONS`. Es kamen keine Invariant-IDs oder First-Class-Persistenztypen hinzu.

## Reproduzierbare Prüfungen

Die folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
git diff --check
```

Die Prüfer bestätigten 253 Invarianten, 149 MAIN-L-Bindungen, 23 First-Class-Typen, 54 Quellenlücken (52 HARD, 2 GUARDED), den Master-Supplement-Suffix M0-04 bis M0-04e, die Bytegleichheit der Quellspiegel samt Audit-ZIP sowie 236 Tasks, 11 Milestones und 153 Folgebeleg-Paare.

Fünf isolierte temporäre Fixtures wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. Fehlendes `AssertionRetraction`-Ziel in der Assertion-Korrektur.
2. Geschwächte Regel gegen implizite Event-Retraction.
3. Fehlendes `AssertionRetract`-Recht.
4. Unvollständige atomare Drei-Datensatz-Schreibmenge.
5. Fehlender UI-Hinweis, dass das ursprüngliche Event aktiv bleibt.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach der Prüfung entfernt.

## Prüfgrenze

Es gibt noch keine Korrektur-Engine, CLI oder Desktop-Implementierung. M0-04e belegt den 1.0-Arbeitsvertrag und seine strukturelle Verankerung; Transaktionsatomizität, Rechteauswertung, Retry, Cancellation und UI-Verhalten müssen später gegen die Laufzeitimplementierung geprüft werden.
