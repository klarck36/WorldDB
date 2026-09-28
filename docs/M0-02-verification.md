# M0-02 – Quellenregister repariert und Lücken klassifiziert

**Prüfdatum:** 2026-09-29  
**Ergebnis:** PASS – Arbeitskopien bleiben ausdrücklich vorläufig.

## Ergebnis

- Sechs Arbeitskopien liegen unter `docs/contract/` und verweisen im [`source-errata.json`](contract/source-errata.json) auf die unveränderten ZIP-Quellen.
- `WDB-LAY-011` wurde aus dem Markdownregister wieder zusammengesetzt; der Testfeldwert ist wieder getrennt. `MAIN-L1162` und `MAIN-L1173` stimmen jetzt mit den Masterzeilen 1162 und 1173 überein.
- Alle 149 `MAIN-L`-Zeilen wurden nach Tabellen-Pipe-Normalisierung exakt verglichen. Die 33 unvollständigen Arrays des ursprünglichen TOML sowie die zwei veralteten Bindungszeilen wurden repariert. 51 ID-Referenzen sind ergänzt; jedes Array entspricht jetzt exakt den expandierten WDB-Referenzen seiner Masterzeile.
- Der 253-ID-Abgleich zwischen TOML, Invarianten-TSV und Markdownregister besteht ohne fehlende oder zusätzliche IDs und ohne Klassen-/Statementabweichungen.
- [`source_gaps.tsv`](contract/source_gaps.tsv) erfasst alle 253 IDs: 199 besitzen einen direkten `MAIN-L`-Anker; 54 haben im verfügbaren Master keinen direkten Anker (52 `HARD`, 2 `GUARDED`). Für diese IDs wurde kein Normtext erfunden. Sie sind Eingabe für M0-02a.
- Die stärkere Masterregel zu `WDB-HIS-001` ist dokumentiert; der Registerwortlaut blieb für den exakten Quellenvergleich unverändert. Die nötige normative Entscheidung bleibt M0-02a.

## Reproduzierbare Prüfungen

```powershell
python -X utf8 docs/contract/build_contract_sources.py
python -X utf8 docs/contract/build_contract_sources.py --verify-only
```

Beide Aufrufe beendeten sich mit Exitcode 0. Die Ausgabe bestätigte 253 IDs, 149 Bindungen, 2 korrigierte Bindungstexte, 35 aktualisierte Arrays, 51 ergänzte Referenzen sowie die obige Anker-/Lückenverteilung.

Für den Negativnachweis wurde eine temporäre Kopie von `source_gaps.tsv` manipuliert. `--verify-only` wies die Kopie mit Exitcode 1 ab; das Arbeitsverzeichnis blieb unverändert.

Der Plan-Audit vom 28.09. hatte 44 fehlende Referenzen und 43 verschiedene IDs notiert. Die erneute Expansion des unveränderten TOML reproduziert 46 fehlende Referenzen in 33 Bindungszeilen und 45 verschiedene IDs. Das Manifest enthält die vollständige zeilenweise Liste; die frühere, datierte Auditnotiz bleibt als Historie erhalten.

Der Prüfer belegt Quellenstruktur und Verknüpfungen, nicht die Wirksamkeit späterer Produkt- oder Laufzeittests. Die fehlende eigenständige v3.1-Gesamtspezifikation bleibt eine bekannte Quellenbegrenzung.
