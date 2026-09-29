# M0-04a – Archive und HistorySpace-Transfer

**Prüfdatum:** 2026-09-29 02:46:48 +02:00  
**Ergebnis:** PASS für den spezifizierten Vertrag und seine Dokumentationsprüfungen

## Ergebnis

- `docs/contract/archive_transfer_contract.md` definiert Archive als reversible, projektweite operative Sichtbarkeit mit typisiertem `ArchiveTransition`. `Archive`, `Unarchive`, Closure, Retraction, Raw History und Offline-Purge haben getrennte Wirkungen.
- Normale Abfragen schließen archivierte Records vor Resolution/Count/Search aus; ausdrücklich autorisierte Abfragen können sie einbeziehen. Raw History enthält Archiviertes weiterhin.
- Der explizite Transfer ist eine normale, atomare Ein-Revision-Transaktion zwischen zwei verschiedenen bestehenden HistorySpaces derselben Datenbank. Der Plan pinnt die Quellrevision und das Ziel-Head, führt typed ID-Maps, Referenzentscheidungen, Lifecycle- und Archivstatusregeln, prüft Kollisionen und erzeugt `DerivedFrom`-Linienage für die kopierten HistorySpace-Records.
- EventRelationen sind projektweite Kanten ohne HistorySpace-Feld. Sie haben deshalb eine separate getypte ID-Abbildung und werden gegen gemappte oder ausdrücklich beibehaltene, im Ziel sichtbare Event-IDs geprüft. Sie werden nicht in generische Provenance-Endpoints eingeschleust.
- Inhalt wird kopiert, nicht verschoben. Es gibt keine zusätzliche Parentkante, keine Änderung an `base_revision`, Quelle, Eltern oder Geschwistern und keine inhaltsbasierte Deduplizierung. ID-Kollisionen, veraltetes Ziel-Head, ungültige Referenzen oder Graphfehler brechen die ganze Transaktion ab; fachliche Resolution-Konflikte bleiben sichtbar.
- Der Vertrag prüft die Beziehungen zu Master §§2.1.1, 2.3.2, 15.2–15.3, 31.2 und 31.4 sowie WDB-BRA, WDB-LFC, WDB-TX, WDB-PRV, WDB-REF und WDB-PRG. M0-02a bleibt unverändert offen.

## Reproduzierbare Prüfungen

Alle folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
```

Docs Verify meldete `22 First-Class types; M0-04 and M0-04a contracts match Master §33`. Plancheck meldete `236 tasks, 11 milestones, 253 invariants, 153 follow-up pairs`; Sourcecheck bestätigte alle Quellspiegel und das ZIP. Der Vertrag fügt keine neue WDB-ID hinzu und verändert keine der 54 klassifizierten Quellenlücken.

Drei isolierte temporäre Fixtures wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. Fehlende `RecordRef::ArchiveTransition(ArchiveTransitionId)`-Variante.
2. Fehlende getypte `selected_event_relations`-Abbildung.
3. Fehlende `TransactionConflict`-Regel für einen geänderten Zielstand.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach den Prüfungen entfernt.

## Prüfgrenze

Es gibt noch keine Engine oder UI. M0-04a belegt die eindeutige, strukturell geprüfte Semantik. Laufzeitprüfungen für Revisionen, Archive-Projektion, atomare Transfers, ID-Remapping, Konflikte, Graphvalidierung und Faults folgen den im Vertrag festgehaltenen Implementierungstasks.
