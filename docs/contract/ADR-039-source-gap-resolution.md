# ADR-039 – Normative Auflösung der M0-02a-Quellenlücken

**Status:** Accepted for the WorldDB 1.0 working contract
**Entscheidungsdatum:** 2026-09-29
**Entscheidung:** Product Owner
**Umsetzung:** M0-02a

## Kontext

Die M0-02-Quellenprüfung fand 52 HARD- und zwei GUARDED-Invarianten ohne direkte ID-Bindung in den 149 unveränderten MAIN-L-Zeilen. Der Kandidaten-Crosswalk stellte Fundstellen, Tasks und Evidenzklassen zusammen, ohne Normintention zu behaupten. Für WDB-HIS-001 enthielt das Register eine kürzere Formulierung als Master §§2.1/3.1.

## Entscheidung

Der Product Owner bestätigt am 2026-09-29 alle 52 source_statement-Aussagen in source_gaps.tsv unverändert als normative Intention. Der Wortlaut jedes Statements wird als einzelne HARD-Regel im beschlossenen Arbeitsmaster wiedergegeben. Jede Bindung besitzt Verantwortlichen, exakte Quellgrenze und Testpflicht in source_gap_bindings.tsv und [source_gap_resolution.md](source_gap_resolution.md).

Der Product Owner bestätigt außerdem die stärkere, bereits vorhandene WDB-HIS-001-Regel in Master §§2.1/3.1 unverändert: veröffentlichte Revisionen sind monoton und lückenlos; unveröffentlichte Reservierungen sind unsichtbar; Genesis ist Revision 0; der erste Commit ist Revision 1; Revision::MAX wird als Overflowreserve nicht vergeben. Das separate Quellenregister behält seinen kürzeren Wortlaut, damit der Quellenvergleich unverfälscht bleibt.

Die bestätigten Fundstellenkorrekturen lauten WDB-DES-001/002 → ADR-018, WDB-VAL-004 → §3.2 und WDB-WIR-005 → §20.2. Für die weiter gefasste WDB-RES-006-Formulierung ergänzt der Anhang die vollständige kanonische Ordnung; Master §16 bleibt der Kontextanker. Die Statements erhalten dadurch keine weitere Semantik über ihren jeweils bestätigten Wortlaut hinaus.

## Verantwortlichkeit, Quellgrenze und Testpflicht

Luna, als Taskverantwortliche der in source_gap_bindings.tsv je Regel genannten Primäraufgabe, ist für Implementierung und Evidenz zuständig; der Product Owner bleibt für Änderungen an der Semantik verantwortlich. Tests müssen das genaue Statement im aufgeführten Evidenztyp belegen und, soweit darstellbar, einen regelwidrigen Ausgang abweisen. Diese Entscheidung definiert Testpflichten; sie behauptet nicht, dass die noch nicht fälligen Produkttests bereits bestanden sind.

Die einzige normative Semantik der 52 Ergänzungen ist jeweils der wortgleiche source_statement. Verlinkte Zeilen im unveränderten Quellenbestand liefern Kontext, keine stillschweigende Erweiterung. WDB-HIS-001 referenziert ausschließlich die bestehenden Mastertexte in §§2.1/3.1. Die zwei GUARDED-Lücken WDB-ENG-006 und WDB-PER-001 bleiben offen.

## Folgen

- M0-02a ist für die 52 HARD-Quellenlücken und den WDB-HIS-001-Abgleich abgeschlossen.
- Die unveränderte Herkunftsklassifikation bleibt in source_gaps.tsv nachvollziehbar; der M0-02a-Wirksamkeitsstatus wird separat ausgewiesen.
- M0-02a schließt nicht M0-14 oder M0-15. Der spätere GitHub-Actions-Lauf und macOS-Nachweis bleiben unabhängig offen.
- [source_gap_resolution.md](source_gap_resolution.md) wird als letzter Normergänzungsanhang an die Master-Arbeitskopie angefügt.
