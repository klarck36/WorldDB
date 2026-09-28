# M0-01 – Quelleninventar und Verifikation

**Prüfdatum:** 2026-09-29  
**Ergebnis:** PASS

Die ZIP-Datei `WorldDB_vNext_Lossless_Consolidation_Audit.zip` ist 127,472 Bytes groß. Ihr SHA-256 lautet:

`f53fb969515171030bca3db9b9d4526a9ca51dfa6b68872da8e6e9029af8edbe`

Der ZIP-Integritätstest meldet `Done testing`. Alle sechs Einträge wurden als UTF-8 gelesen und unverändert nach `docs/source/` gespiegelt. Die Größen und individuellen SHA-256-Werte stehen in [`source_manifest.json`](source/source_manifest.json). `.gitattributes` verhindert Zeilenenden-Konvertierung der Spiegeldateien und behandelt die ZIP als Binärdatei.

## Reproduzierbare Prüfung

```powershell
python -X utf8 -m zipfile -t WorldDB_vNext_Lossless_Consolidation_Audit.zip
python -X utf8 WorldDB_1.0_Sourcecheck.py
```

Beide Befehle beendeten sich mit Exitcode 0. Der Sourcecheck bestätigte den ZIP-Hash, den vollständigen Eintragskatalog und alle sechs Bytevergleiche. Der eingebundene Plancheck meldete:

`STRUCTURE OK: 236 tasks, 11 milestones, 253 invariants, 153 follow-up pairs; DAG and references valid`

Das Manifest, der Sourcecheck und dieser Bericht sind die prüfbaren M0-01-Artefakte. Der strukturelle Plancheck prüft keine Existenz oder Wirksamkeit späterer Produkttests.

## Negativnachweis

In einer temporären Kopie wurde `WorldDB_ADRs_vNext.md` um Bytes ergänzt. Der Sourcecheck beendete sich erwartungsgemäß mit Exitcode 1 und meldete `Source mirror is not byte-identical`. Die Arbeitskopie blieb unverändert.
