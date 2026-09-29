# M0-02a – Entscheidung und Arbeitskopien

**Status:** Abgeschlossen am 29. September 2026
**Entscheidung:** Product Owner

Der Product Owner bestätigte: „M0-02a: alle 52 Aussagen in source_gaps.tsv als Normintention bestätigen. WDB-HIS-001: stärkere Masterregel bestätigen.“

## Umgesetzte Entscheidung

- Alle 52 HARD-Statements sind wortgleich im beschlossenen Arbeitsmaster unter dem M0-02a-Anhang normativ verankert. Jede Aussage hat einen direkten WDB-Anker, einen Quellkontext, eine exakte Quellgrenze, die verantwortliche Person, eine primäre Aufgabe und eine konkrete Testpflicht.
- WDB-HIS-001 bleibt im Quellenregister wortgleich. Seine wirksame Bindung umfasst unverändert die stärkere Masterregel aus §§2.1/3.1: veröffentlichte Revisionen sind monoton und lückenlos; unveröffentlichte Reservierungen sind unsichtbar; Genesis ist Revision 0; der erste Commit ist Revision 1; Revision::MAX wird nicht vergeben.
- WDB-DES-001/002 sind an ADR-018 gebunden, WDB-VAL-004 an §3.2 und WDB-WIR-005 an §20.2. WDB-RES-006 bindet §16 und die eng begrenzte Ergänzung zur vollständigen kanonischen Resolutionausgabe.
- Die beiden GUARDED-Lücken WDB-ENG-006 und WDB-PER-001 bleiben offen und außerhalb dieser Entscheidung.

## Beschlossene und prüfbare Artefakte

- [ADR-039](contract/ADR-039-source-gap-resolution.md) dokumentiert Entscheidung, Verantwortung, Quellgrenze und Testpflicht.
- [Normergänzungsanhang](contract/source_gap_resolution.md) bindet alle 52 Statements in der Master-Arbeitskopie.
- [Maschinenlesbares Bindungsregister](contract/source_gap_bindings.tsv) enthält alle 52 HARD-Bindungen und die gesonderte WDB-HIS-001-Abstimmung.
- [Quellenlückenregister](contract/source_gaps.tsv) bewahrt die ursprüngliche Herkunftsklassifikation und weist den Beschlussstatus je ID separat aus.
- [Kandidaten-Crosswalk](M0-02a-candidate-crosswalk.tsv) bleibt als historischer Kandidatenstand erhalten; der beschlossene Bindungsstand ist das neue Register.

M0-02a ist damit abgeschlossen. M0-15 ist als lokaler Entwicklungs-Vorfreigabepunkt wieder aufgenommen. M0-14 bleibt wegen der nachträglich vorgesehenen Git-/GitHub-Integration und des noch ausstehenden macOS-Laufs offen; es sperrt weiterhin RC- und Releasefreigabe.
