# M0-05 – Produktabläufe

**Prüfdatum:** 2026-09-29 04:21:24 +02:00

**Ergebnis:** PASS für die dokumentierten Desktop- und CLI/API-Abläufe sowie die strukturelle Plan-/Quellenprüfung

**Vertragscommit:** `ab6355b` (`docs: define WorldDB product workflows`)

## Ergebnis

- [Produktabläufe](product/product_workflows.md) definieren zwölf Kernflüsse: Projekt anlegen, öffnen, Schema bearbeiten, Entity anlegen/verwalten, Assertion erfassen/korrigieren, HistorySpace/Layer verwenden, Perspective definieren, Event erfassen/korrigieren, Source/Evidence/Provenance, Query, Rechte sowie Backup/Restore.
- Jeder Fluss hat einen benannten Desktop-Einstieg, einen typed CLI/API-Einstieg und einen Abnahmeschritt mit erfolgreichem sowie abzulehnendem bzw. fehlerhaftem Fall. Die gemeinsamen UX-Regeln binden Writes an den Commitbeleg, behandeln Konflikt und unbekannten Commit getrennt und erhalten geschützte Fehlerformen.
- Der erste Projektstart ist als vollständiger Policy-Bootstrap beschrieben: nur der host-authentifizierte Ersteller erhält das explizite GM-Bundle; `Player` startet ohne Grants oder Zuweisung. Hochprivilegierte Vorgänge wie `AdminRawRead`, Purge, Import/Export, Migration und Audit-Export sind keine Standardgrants.
- Known/Unknown/Conflict, Schemahistorie, unveränderliche Records, HistorySpace-Cutoff, Layer, Perspective/Principal-Trennung, explizite Evidence/Provenance, aktuelle Rechteprüfung sowie getrennte Backup-/Audit-Wasserstände bleiben in der Bedienung sichtbar.
- Die Bestätigung erfasst Dokumentation, Contract-Referenzen und Register-/Quellenkonsistenz. Es gibt noch keine Desktop-App, CLI oder Laufzeitimplementierung; die beschriebenen Akzeptanzschritte wurden nicht gegen Software ausgeführt.

## Reproduzierbare Prüfungen

Die folgenden Befehle endeten mit Exitcode 0:

```powershell
python -X utf8 docs/contract/build_contract_sources.py --verify-only
python -X utf8 docs/contract/verify_contract_docs.py
python -X utf8 WorldDB_1.0_Sourcecheck.py
python -X utf8 WorldDB_1.0_Plancheck.py
git diff --check
```

Eine strukturelle Sichtprüfung bestätigte für alle zwölf Ablaufabschnitte jeweils Desktop-Einstieg, CLI/API-Einstieg und Abnahmeschritt. Der Plancheck bestätigte 236 Tasks, 11 Milestones, 253 Invarianten, 153 Folgebeleg-Paare sowie einen gültigen Abhängigkeitsgraphen. Der Quellcheck bestätigte Bytegleichheit der sechs Spiegel und des Audit-ZIP.

## Prüfgrenze

M0-05 ist ein UX-Vertrag, keine Implementierung oder Usability-Abnahme. Native Tastatur-/Accessibility-, Recovery-, Security- und Laufzeit-E2E-Prüfungen bleiben für die zugehörigen späteren Tasks vorgesehen. Die GM-/Player-Voreinstellung ist ein expliziter Produktdefault für den Standardstart; spätere Grants bleiben normale policyhistorisierte und auditpflichtige Aktionen.
