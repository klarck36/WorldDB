# M0-04c – Security-Policy-Vertrag

**Prüfdatum:** 2026-09-29 03:31:07 +02:00

**Ergebnis:** PASS für den versionierten Sicherheitsvertrag und seine strukturellen Dokumentationsprüfungen

## Ergebnis

- `docs/contract/security_policy_contract.md` definiert append-only `SecurityPolicyRecord`s auf der gemeinsamen Revision-Achse, typed Principal-/Role-/Assignment- und PolicyRule-Änderungen sowie getrennte `SecurityEpoch`-Semantik.
- Die Capability-Liste ist geschlossen und typisiert. Scope-Matching ist exakt und dimensionsweise konjunktiv; fehlende Grants verweigern, passende Denies überwiegen. Rollenamen wie GM/Player sind keine Bypass-Regeln, und Admin-Raw-Rechte bleiben explizit.
- HistorySpace-, Layer-, Record-, Feld-, Relationship- und Operationsrechte sind aufgeschlüsselt. Autorisierung und FieldRead laufen vor Candidate-Erzeugung, Masking, Resolution, Aggregation, Explain, Search, Graphausgabe und Redaction. Hidden-Data- und Endpoint-Schutz sowie das feste Redacted-Feldverhalten sind festgelegt.
- `AuthorizationNow` bleibt Default auch bei historischen Daten. `AuthorizationAtRevision` braucht die aktuelle Capability `SecurityPermissionHistoryRead` und vollständige Policyhistorie. Jeder Policy-Commit erhöht die `SecurityEpoch` genau einmal; Policyrecords, Epoch, Revision und erforderliches Audit committen atomar und invalidieren Cursor.
- `SecurityPolicyRecord` ist als 23. First-Class-Typ in Master §33 und `invariants_vNext.toml` registriert, mit eigener ID und expliziter Abgrenzung von Domain-`RecordRef`/Provenance. M0-05 übernimmt den Bootstrap-Ablauf und die initialen Role-Grants.
- Es wurden keine neuen WDB-Invariant-IDs eingeführt. Die 54 Quellenlücken (52 HARD, 2 GUARDED) und die M0-02a-Entscheidung zu WDB-HIS-001 bleiben unverändert offen.

## Reproduzierbare Prüfungen

Die folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
```

Die Prüfer bestätigten 23 First-Class-Typen, 253 Invarianten, 149 MAIN-L-Bindungen und die unveränderte Quellklassifikation. Sourcecheck bestätigte die Bytegleichheit der sechs Quellspiegel und das unveränderte Audit-ZIP. Plancheck bestätigte 236 Tasks, 11 Milestones und 153 Folgebeleg-Paare.

Vier isolierte temporäre Fixtures wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. Fehlende Deny-überwiegt-Regel.
2. Fehlende FieldRead-Prüfung vor Candidate-Erzeugung.
3. Fehlende `SecurityEpoch`-Overflow-Regel.
4. Fehlender `SecurityPolicyRecord`-Eintrag im First-Class-Register.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach der Prüfung entfernt.

## Prüfgrenze

Es gibt noch keine Security-Engine, Policy-Parser, API-/IPC-Implementierung oder UI. M0-04c belegt die vollständige Arbeitsvertragssemantik und ihre strukturelle Verankerung; Laufzeittests für Policy-Auswertung, Non-Interference, historische Autorisierung, Cursorinvalidierung und Audit-Faults bleiben in M3/M4/M5/M6/M8.
