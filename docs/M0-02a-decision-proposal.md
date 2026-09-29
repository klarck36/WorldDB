# M0-02a – Entscheidungsantrag an Product Owner

**Status:** Vorschlag; keine Produktentscheidung ist vorweggenommen.

## Entscheidung 1: 52 HARD-Quellenlücken

`docs/contract/source_gaps.tsv` enthält für jede Lücke die ID und den aktuell erfassten `source_statement`. Der Vorschlag ist, diese 52 Aussagen in ihrem bestehenden Sinn als verbindliche Normintention zu bestätigen und sie in beschlossenen Arbeitskopien des Masters mit expliziten ADR-Ergänzungen zu verankern. Jede Ergänzung erhält Verantwortlichen, exakte Quellgrenze und konkrete Testpflicht. Detailtext darf die bestätigte Aussage nicht abschwächen oder erweitern. Bestehende Masterregeln werden als Quellenanker verwendet, wo sie die Aussage tatsächlich tragen; andernfalls wird die Aussage als Ergänzung gekennzeichnet.

Die Entscheidung betrifft alle folgenden 52 IDs. Eine einzelne Ausnahme kann durch Nennung der ID und einer Ersatzentscheidung zurückgegeben werden; ohne Ersatz bleiben HARD-Lücken offen und das M0-Gate gesperrt. Der [Kandidaten-Crosswalk](M0-02a-candidate-crosswalk.tsv) führt pro ID die derzeit deklarierte Masterstelle, konkrete Zeilenkandidaten in der Quelldatei sowie vorhandene primäre Task- und Evidenzklassen aus `WorldDB_1.0_Invariantenabdeckung.tsv` auf. Er ist ausdrücklich keine bestätigte Bindung.

### Mechanische Crosswalk-Prüfung

Am 29. September 2026 wurde der Crosswalk mechanisch gegen `source_gaps.tsv`, `WorldDB_1.0_Invariantenabdeckung.tsv` und die Quelldatei geprüft. Die 52 eindeutigen Crosswalk-IDs entsprechen den 52 HARD-Lücken unter insgesamt 54 normativen Lücken; Aussagen, primäre Tasks und Evidenzklassen stimmen mit den Registern überein. Alle 61 physischen Kandidatenzeilen liegen innerhalb der Quelldatei mit 2.171 Zeilen. Alle Einträge bleiben als unbestätigte Kandidaten markiert. Dieser Check belegt Registergleichheit und gültige Zeilengrenzen, keine fachliche Freigabe oder semantische Richtigkeit der Kandidaten.

| Bereich | Anzahl | IDs |
|---|---:|---|
| API | 5 | WDB-API-001–005 |
| Concurrency | 1 | WDB-CON-001 |
| Desktop | 2 | WDB-DES-001–002 |
| Engineering | 3 | WDB-ENG-004, WDB-ENG-005, WDB-ENG-007 |
| Errors | 1 | WDB-ERR-006 |
| Export/Import | 2 | WDB-EXP-001–002 |
| Extensions | 1 | WDB-EXT-001 |
| IDs | 1 | WDB-ID-004 |
| Indexes | 3 | WDB-IDX-001–003 |
| Observability | 3 | WDB-OBS-001, WDB-OBS-002, WDB-OBS-004 |
| Optimistic concurrency | 3 | WDB-OCC-001–003 |
| Purge | 2 | WDB-PRG-001–002 |
| Recovery | 5 | WDB-REC-001–005 |
| Record references | 2 | WDB-REF-001–002 |
| Resolution | 1 | WDB-RES-006 |
| Semantic types | 1 | WDB-SEN-002 |
| Storage | 3 | WDB-STO-001–003 |
| Type semantics | 1 | WDB-TYP-002 |
| Values | 3 | WDB-VAL-004–006 |
| WAL | 5 | WDB-WAL-001–005 |
| Wire format | 4 | WDB-WIR-001, WDB-WIR-002, WDB-WIR-004, WDB-WIR-005 |

### Ergebnis der Fundstellenprüfung

Für den Großteil der Aussagen gibt es bereits passende normative Sätze im Master; die Lücke besteht dort vor allem darin, dass die konkrete WDB-ID nicht an die Satzstelle gebunden ist. Diese Verknüpfung ändert die vorhandene Norm nicht. Der Crosswalk markiert jede Stelle als ungeprüften Kandidaten, bis die beschlossene Arbeitskopie erstellt und der bidirektionale Linkcheck bestanden ist.

Vier Problemgruppen mit fünf betroffenen IDs brauchen besondere Prüfung:

- `WDB-DES-001/002`: Der Registerverweis zeigt auf §34.2, während die passenden Beschlüsse in ADR-018 auf Quellzeilen 2010–2011 stehen. Zu bestätigen ist, ADR-018 als normative Stelle zu binden und den Registerverweis zu berichtigen.
- `WDB-VAL-004`: Die Regel „kein globales fachliches `Ord` für `Value`“ steht in §3.2, Quellzeile 239; das Register nennt derzeit §§2.3 und 12.
- `WDB-WIR-005`: Die Trennung von Semantic HARD Contract, Default Implementation Profile und Provisional Performance Gate steht in §20.2, Quellzeilen 636–640; das Register nennt derzeit §12.
- `WDB-RES-006`: Die nächste passende Stelle verspricht deterministisch sortierte Contributors (§16, Quellzeile 555). Sie deckt die breitere Registerformulierung „Resolutionausgabe“ nicht eindeutig ab. Der Vorschlag ist, die bestehende Aussage beizubehalten und die vollständige kanonische Ordnung in einer eng begrenzten Ergänzung festzuschreiben.

Für Tests liegen bereits primäre Tasks und Evidenzklassen im Invariantenabdeckungsregister vor. Der Crosswalk übernimmt diese Angaben als Prüfpflicht-Vorschläge; nach Freigabe werden daraus genaue positive und negative Akzeptanzfälle abgeleitet. Keine der Kandidatenstellen oder Ergänzungen wird vor der Freigabe als beschlossen markiert.

## Entscheidung 2: WDB-HIS-001

Der Master enthält bereits die stärkere Regel als das gekürzte Register: veröffentlichte Revisionen sind lückenlos; der leere Genesis-Stand ist Revision 0; der erste Commit veröffentlicht Revision 1; `Revision::MAX` wird wegen Overflowreserve nicht vergeben. Der Vorschlag ist, diese Masterregel unverändert als maßgeblich zu bestätigen und WDB-HIS-001 daran zu binden.

## Nicht Teil dieser Entscheidung

Die zwei `GUARDED`-Lücken WDB-ENG-006 und WDB-PER-001 sind nicht Teil der 52 HARD-Gates. Die Open-Source-Wahl ist bereits umgesetzt: ADR-038 legt `MIT OR Apache-2.0` fest. M0-14 ist davon getrennt. GitHub Actions reicht für die geplante Matrix aus und wird später eingerichtet; M0-14 bleibt bis zum externen Lauf einschließlich macOS offen. Ein zweiter CI-Anbieter ist nicht erforderlich.

## Antwortformat

Zum Freigeben reicht:

> M0-02a: alle 52 Aussagen in `source_gaps.tsv` als Normintention bestätigen. WDB-HIS-001: stärkere Masterregel bestätigen.

Damit bestätigst du auch die im Crosswalk genannten bestehenden Textstellen, soweit sie den jeweiligen Wortlaut tragen. Für `WDB-RES-006` umfasst die Freigabe die eng begrenzte Ergänzung zur vollständigen kanonischen Resolutionausgabe. Die drei falschen bzw. zu groben Registerstellen (`WDB-DES-001/002`, `WDB-VAL-004`, `WDB-WIR-005`) werden auf die genannten Fundstellen berichtigt.

Für Abweichungen bitte die betroffenen IDs und den gewünschten Ersatz nennen. Bis zur Antwort bleiben die Arbeitskopien vorläufig und M0-02a `BLOCKED`.
