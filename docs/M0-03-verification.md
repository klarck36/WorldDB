# M0-03 – First-Class-Register maschinenlesbar

**Prüfdatum:** 2026-09-29  
**Ergebnis:** PASS

## Ergebnis

- `docs/contract/invariants_vNext.toml` enthält 22 `[[first_class_type]]`-Einträge aus Master §33. Jeder Eintrag führt Namen, ID-Typ, Main-Location, expandierte Invariant-IDs, Wire, Storage, Lifecycle, Security und Evidence-/Provenance-Eignung.
- `docs/contract/build_contract_sources.py` leitet die Einträge reproduzierbar aus der verifizierten Master-Arbeitskopie ab und vermerkt die Übertragung als `ERR-M0-03-FIRST-CLASS-REGISTER` in `source-errata.json`.
- `docs/contract/verify_contract_docs.py` ist eigenständig ausführbar. Der Prüfer verlangt alle Pflichtfelder, eindeutige Namen, gültige Invariant-IDs und einen feldgenauen Gleichlauf mit §33.

## Reproduzierbare Prüfungen

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
```

Alle Aufrufe beendeten sich mit Exitcode 0. Docs Verify meldete: `22 First-Class types; required fields and Master §33 match`. Sourcecheck bestätigte die ZIP-Hashes und Quellspiegel; Plancheck bestätigte 236 Tasks, 11 Milestones, 253 Invarianten und 153 Folgepaare.

Zwei isolierte, temporäre Fixture-Manipulationen wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. Das Pflichtfeld `name` aus dem ersten TOML-Typeneintrag entfernt: fehlendes Pflichtfeld erkannt.
2. `UnregisteredNewType` in die Master-Arbeitskopie unter §33 eingefügt, ohne TOML-Registereintrag: 23 statt 22 Typen erkannt.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach dem Lauf entfernt. Der Prüfer validiert Dokumentstruktur und Referenzen, keine Laufzeitimplementierung der späteren Recordtypen.
