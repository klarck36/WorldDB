# M0-04d – Query-/Transportvertrag

**Prüfdatum:** 2026-09-29 03:53:12 +02:00

**Ergebnis:** PASS für den versionierten Query-/Transportvertrag und seine strukturellen Dokumentationsprüfungen

**Vertragscommit:** `3269d04` (`docs: specify WorldDB query and transport contract`)

## Ergebnis

- [Query-/Transportvertrag](contract/query_transport_contract.md) und [ADR-034](contract/ADR-034-query-transport.md) schließen einen gemeinsamen owned `QueryRequest` für Rust-Fassade, CLI und Desktop-IPC. Der vertrauenswürdige Host bindet Principal und Cancellation; DTOs können keine Identität, Capabilities oder Dateipfade setzen.
- Der Vertrag legt den vollständigen Query-Kontext, endliche Budgets, einen geschlossenen Filter-AST, typed Fieldvergleiche, Zeitfenster, Projektionsregeln und die Operation-Capabilities aus M0-04c fest. FieldRead und Record Read liegen vor Candidate-Erzeugung; unsichtbare Daten dürfen weder Resultate noch Counts, Sortierung oder Fehler beeinflussen.
- TokenSearch verwendet eine feste ASCII-Whitespace-Tokenisierung und bytegenauen, case-sensitiven Vergleich ohne Locale-, Normalisierungs-, Stemming-, Phrasen- oder Snippetregeln. FullText ist ein eigenes optionales Capability-Feature und wird nicht still auf TokenSearch abgebildet.
- Sortierung definiert typed Comparatoren, fehlende Werte, EventTime-Spans und einen stabilen caller-visible `ResultKey`-Tie-Breaker. Seiten, QueryHash und Cursor binden die konkrete Snapshot- und Security-Semantik; jeder Cursorfortschritt prüft aktuelle Autorisierung.
- Versionierte CLI-/IPC-Envelopes, IPC-Handshake und Feature-Aushandlung, stabile Request-IDs, Cancellation, geschlossene JSON-Tags, präzise skalare Encodings und sichere Fehlercodes sind festgelegt. Der generierte Arbeits-Master hängt den M0-04d-Supplement hinter M0-04c an; `source-errata.json` enthält `ERR-M0-04D-QUERY-TRANSPORT`.
- Es kamen weder WDB-Invariant-IDs noch First-Class-Persistenztypen hinzu. Die 253 Invarianten, 149 MAIN-L-Bindungen, 23 First-Class-Typen und 54 Quellenlücken (52 HARD, 2 GUARDED) bleiben unverändert.

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

Die Prüfer bestätigten die Reproduktion aller Quellarbeitskopien und Errata, 23 First-Class-Typen, den exakten Master-Supplement-Suffix M0-04 bis M0-04d, die Bytegleichheit der sechs unveränderten Quellspiegel samt Audit-ZIP sowie 236 Tasks, 11 Milestones, 253 Invarianten und 153 Folgebeleg-Paare.

Vier isolierte temporäre Fixtures wurden erwartungsgemäß mit Exitcode 1 abgewiesen:

1. Entfernte `MatchAll`-Filtervariante.
2. Entfernte FieldRead-vor-Candidate-Regel.
3. Nicht-strikte IPC-Versionsliste im Handshake.
4. Entfernte EventTime-Sortierordnung.

Die Fixtures lagen außerhalb des Arbeitsverzeichnisses und wurden nach der Prüfung entfernt.

## Prüfgrenze

Es gibt noch keine Query-Engine, CLI-Implementierung oder Desktop-IPC. M0-04d belegt den vollständigen 1.0-Arbeitsvertrag und die strukturelle Verankerung; Adapterparität, Comparatoren, Cursorfortsetzung, Precision, Search, Security-Non-Interference, Budget und Cancellation müssen später gegen die Laufzeitimplementierung geprüft werden.
