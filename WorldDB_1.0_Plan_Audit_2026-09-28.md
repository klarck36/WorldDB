# WorldDB 1.0 – Prüfung und Korrekturen des Luna-Arbeitsplans

**Erstellt:** 28. September 2026; **nachgeprüft:** 29. September 2026  
**Prüfumfang:** Plan, Taskregister, Abdeckungsmatrix und alle sechs Dateien der gelieferten ZIP. Dies ist eine Prüfung der Planung; WorldDB-Code oder ausgeführte Produkttests existieren noch nicht.

## Behobene Planfehler

| Befund | Auswirkung | Korrektur |
|---|---|---|
| M1-03 setzte Int, UInt und Decimal faktisch gleich. | Falsche Value-Semantik und Wire-Identität. | Decimal-Skalierungsäquivalenz präzisiert; typübergreifende Ungleichheit als Negativtest ergänzt. |
| M1-/M2-Gates verlangten auch Invarianten, deren produktive Nachweise erst M3–M7 entstehen. | Gates waren wörtlich unerfüllbar. | Gates prüfen jetzt nur fällige Primär- und Folgebelege anhand der Matrix. `WorldDB_1.0_Plancheck.py --gate-precheck Mx` prüft ihre Struktur und Fälligkeit. |
| M3 verschob Search/Graph/Count-Non-Interference auf M6. | Das vom Master geforderte M3-Security-Gate war zu schwach. | Indexfreie Referenzports und vollständige Paarwelt-Tests in M3; Wiederholung auf optimierten Pfaden in M6. QueryContext steht vor Query-Autorisierung. |
| Deprecated-Writes und Retired-Writes hatten keinen vollständigen Schreibvertrag. | Unerlaubte Schema-Lifecycle-Writes könnten gültig erscheinen. | Opt-in, Capability, typisierte Warning und Retired-Verbot in M4; UI-Folgeprüfung in M8; `WDB-SCH-010` primär M4 zugeordnet. |
| M5 testete Crashprofile vor ihrer Supportentscheidung. | Die 100.000-Crashpunkt-Matrix hatte keinen definierten Zielumfang. | M5-21 legt die zu prüfende Schreibbasis fest; M5-22 testet sie; M5-23 bestätigt Claims. Hardware-/Power-Loss-Lücken werden ausdrücklich ausgewiesen. |
| M7-05 und M7-07 verlangten Restorepoints vor implementiertem Backup/Restore. | Breaking- und Dateiformat-Migration waren in der geplanten Reihenfolge nicht abnehmbar. | M7-05/M7-07 auf nichtbrechende Ausführung bzw. Upgradeplan begrenzt; M7-10a/M7-10b führen die riskanten Pfade erst nach verifiziertem Backup/Restore aus. |
| Frühe WAL-/Segment-Tasks und M7-08 verlangten Restart, unabhängiges Verify oder Restore vor den entsprechenden Implementierungen. | Lokale Tasks wären nur mit vorgezogener Arbeit als `DONE` markierbar gewesen. | Frühe Abnahmen auf Readback, Writer-/Adaptervertrag und Ziel-Verify begrenzt; Restart/Recovery in M5-12/M5-22, Manifest-Verify in M5-16 und realer Restore in M7-10 mit Matrix-Folgebelegen. |
| Provenance M2-15a verlangte Transaction-Time-Reihenfolge pauschal. | DerivedFrom/ResultedFrom hätten eine nicht spezifizierte Einschränkung erhalten. | Reihenfolge auf `Corrects` beschränkt. |
| Abdeckungsmatrix ordnete einige Regeln nur Modell-/Gate-Tasks statt den Write-, Security- und Storage-Pfaden zu. | Produktive Verletzungen wären trotz grünem Modelltest möglich. | Primär-/Folgeaufgaben für BranchId, Base-Layer, Event/Evidence/Provenance-Batchvalidierung, Graph-Security, Ressourcen und Migration angepasst. Gate-zu-Gate-Evidenzverweise entfernt. |
| Ein allgemeines Task-Evidenzfeld konnte beliebige Invarianten-Folgebelege erfüllen. | Der Gate-Check hätte einen falschen Nachweis akzeptiert. | `WorldDB_1.0_Folgebelege.tsv` enthält jetzt je Invariant-Task-Paar Status, konkrete Test-/Artefakt-/Run-Referenz und Ergebnis; Plancheck verlangt genau dieses Paar. |
| Selbst erfundene `test:`-/`run:`-Referenzen konnten den syntaktischen Check bestehen. | Ein grüner Checker wäre als echte Gate-Abnahme missverständlich. | CLI und Plan nennen ihn ausdrücklich **Struktur-Precheck**; jedes Gate verlangt zusätzlich ein Protokoll mit geöffneten realen Test-/Run-Manifesten, PASS/FAIL, Hashes und Reviewentscheidung. |
| Die mechanische Quellenreparatur und mögliche Produktentscheidungen waren in M0-02 verkettet. | Eine unklare HARD-Regel hätte auch unabhängige Toolchain-/CI-Arbeit gestoppt. | M0-02 klassifiziert und repariert mechanisch; M0-02a entscheidet normative Lücken. M0-03 und M0-06 können nach ihren echten Voraussetzungen weiterlaufen; M0-Gate wartet auf M0-02a. |
| Einige Primär- und Folgebelege lagen vor der zu prüfenden Implementierung oder auf sachfremden Tasks. | Frühe Gates konnten Vollständigkeit nur behaupten. | Unter anderem PHIL/ENG/DEP, LayerId, RecordRef, Konstruktoren, Security-Non-Interference und Migration auf ausführbare Prüfaufgaben verschoben bzw. mit Folgebelegen versehen. |
| Quellenrang bei beschädigtem TOML war unklar. | Luna hätte die fehlerhafte Fassung implementieren können. | ZIP als unveränderlicher Beleg, geprüfte `docs/contract/`-Arbeitskopien als spätere Implementierungsbasis, Errata-/ADR-Pfad festgelegt. |

Weitere präzise Abnahmen betreffen Terminologie-Lint, konkrete Ressourcenprofile, Loom-/Compile-Fail-Schedules, fünf Korruptionsklassen, echte Hardwaretests soweit möglich und Timing-Leak-Minderung. Die ID-Taxonomie umfasst jetzt auch sessionlokale IDs; Decimal-Präzision liegt explizit in Feld-/Schemametadaten. N-1 prüft unterstützte AuditComplete-Backups, der RC erhält eine reale Pilotnutzung, und der finale Restore-Drill verlangt nur die tatsächlich gewählte Backup-Authentizität. Langläufe können `WAITING_EXTERNAL` sein; M1-19 ist trotz laufendem M1-18-Fuzz unabhängig ausführbar. Native E2E-Runs und 24-Stunden-Fuzzing sind in getrennte OS-/Target-Tasks mit gemeinsamer Auswertung zerlegt. Die zwei M10-Audits haben getrennte Reviewer als Owner.

## Nachprüfung am 29. September 2026

| Befund | Korrektur |
|---|---|
| Einige frühen M1-Abnahmen verlangten Decoder-, Budget-, Import- oder Querynachweise, bevor diese Pfade gebaut werden. | M1-09a, M1-13, M1-14 und M1-17b/d auf lokal verfügbare Artefakte begrenzt; vollständige Decoderprüfung in M1-17d und Budget-/Fuzzprüfung in M1-18. Das Decoderinventar muss im einstündigen Lauf vollständig erreicht werden. |
| `Correct` bei Assertions war nur als lose `Corrects`-Relation behandelt. | M0-04e spezifiziert die atomare Aktion aus neuer Assertion, explizitem Retractionrecord und `Corrects`-Provenance; M2-15c modelliert sie nach Aufbau des Event-/Provenance-Modells, M4-05d committet sie, M8-14a zeigt die Wirkung. Event-Correct bleibt neuer Event plus Kante ohne implizite Retraction. |
| Die Engine-Tasks nannten nicht alle schreibbaren First-Class-Domain-/Lifecycleformen. | M4-05d ergänzt einen maschinellen 22-Typen-Abgleich samt validierten Schreibpfaden für Mask, Boundary, EventMask, EventRelation, Source und Lifecycleformen. |
| Veröffentlichten Revisionen fehlte die lückenlose Crash-/Overflow-Abnahme. | M2-01 verlangt Genesis 0, ersten Commit 1, lückenlose publizierte Folge und reserviertes `Revision::MAX`; M5-22 prüft den sicheren, lückenlosen Crash-Präfix. Die schwächere Formulierung in `WDB-HIS-001` bleibt unverändert aus dem Quellregister erhalten und ist als M0-02-Abweichung notiert. |
| Typisierte Deprecated-Warnung und Required Audit waren an frühen oder zu breiten Tasks platziert. | M4-04a gibt die Warning als Validierungsergebnis aus, M4-09 trägt sie in den Receipt; M5-17 prüft den generischen Auditmechanismus mit echter Policyaktion, M7-10a die Breaking-Migration. Restrictive benötigt explizite Unresolved-Entscheidungen, aber Restorepoint und Required Audit nur gemäß ihrem eigenen Vertrag/Policy; Breaking verlangt zusätzlich Admin-Aktion, verifizierten Restorepoint und atomaren Required Audit Record. |
| Die Performanceabnahme nannte nur „Historyseite“. | M6-14b misst p95 ≤ 250 ms **pro Seite** über eine paginierte History mit 1000 Seiten gemäß Master §20.2. |
| Primär- und Folgebelege übersprangen produktive Write-, Recovery-, Query- und Compaction-Pfade. | `WDB-HIS-003/004`, `WDB-SCH-012` und `WDB-IDX-001` auf die prüfbaren Primärpfade gelegt; für HIS-004 zusätzlich Security-Zeitbasis in M3-05, für BRA-001–004, LAY-005/008 sowie AST-002/EVT-012/PRV-002 spätere, jeweils eigene Folgebelege ergänzt. |
| Wartende Langläufe hatten keinen verbindlichen nächsten Prüftermin; gestartete oder erledigte Tasks konnten offene Abhängigkeiten haben. | `next_check` als Zeitzonenzeitpunkt im Taskregister ergänzt; Plancheck weist fehlende/falsch geordnete Termine und `RUNNING`, `WAITING_EXTERNAL` oder `DONE` bei offenen Dependencies ab. Ohne `READY`-Task wird der nächste fällige Run geprüft oder der Durchgang ohne Erfolgsmeldung beendet. |
| Ein unterbrochener `RUNNING`-Status konnte von einer neu gewählten `READY`-Task übergangen werden. | Luna prüft fällige externe Runs und setzt danach die einzige `RUNNING`-Task fort; Plancheck weist mehrere gleichzeitige `RUNNING`-Tasks ab. |
| Ein Matrix- oder Folgebeleg konnte `DONE` melden, obwohl die zugehörige Task noch offen war. | Plancheck koppelt `DONE` an die abgeschlossene Primär- bzw. Folgetask und verlangt für Folgetests einen echten ISO-8601-Prüfzeitpunkt mit Offset. |
| Eine Task-ID konnte wie eine numerische Sortierfolge gelesen werden. | Der Plan erklärt die physische Zeilenfolge des Taskregisters als maßgeblich; M2-15c steht folgerichtig nach den Event-/Provenance-Grundlagen. |

## Bestätigte Quellenmängel für M0-02 und M0-02a

- `invariants_vNext.toml` enthält 149 `main_rule_binding`-Einträge. In 33 Bindungstexten werden per Slash-Schreibweise 44 zusätzliche WDB-ID-Verweise genannt, die in den jeweiligen `invariant_ids`-Arrays fehlen (43 verschiedene IDs).
- 68 der 253 WDB-IDs stehen derzeit in keinem Binding-Array. Eine Array-Ergänzung ist nur zulässig, wenn die echte normative Haupttextstelle vorhanden ist; sonst braucht die Lücke einen dokumentierten Entscheid.
- `MAIN-L1162` nennt noch PRV-001–010 und `MAIN-L1173` AUD-001–011, während der Master inzwischen jeweils bis 013 reicht.
- `WDB-LAY-011` ist im TOML-Feld durch ein nicht maskiertes Tabellen-`|` beschädigt; Markdownregister und Master enthalten den vollständigen Wortlaut.
- `WDB-HIS-001` ist im separaten Register gegenüber Master §§2.1/3.1 verkürzt: Dort fehlen Lückenlosigkeit, Genesis 0, erster Commit 1 und die Overflowreserve `Revision::MAX`. Die Matrix bewahrt die Registerformulierung für den exakten Quellvergleich; M0-02 muss die Quellabweichung explizit erfassen, M2-01/M5-22 prüfen die stärkere Masterregel.
- Die originale v3.1-Gesamtspezifikation liegt in der ZIP nicht vor. Der spätere Lossless-Nachweis darf nur die verfügbaren Quellen als geprüft ausgeben.

## Prüfergebnis für die überarbeiteten Arbeitsartefakte

- `python -B -X utf8 WorldDB_1.0_Plancheck.py`: **Struktur OK**, 236 eindeutige Tasks in 11 Meilensteinen, 253 eindeutige Invarianten, 153 Invariant-Task-Folgepaare, gültige Taskreferenzen und azyklische Abhängigkeiten.
- `python -X utf8 WorldDB_1.0_Plancheck.py --gate-precheck M0` vor Aufgabenabschluss: erwartungsgemäß abgewiesen; ein offenes Gate wird nicht als strukturell bereit gemeldet.
- Direkter Vergleich der 253 `source_statement`-/Klassenpaare der Matrix mit `WorldDB_Invariantenregister_vNext.md` aus der ZIP: **0 Abweichungen**.
- Simulierter Dependency-Durchlauf erreicht 236/236 Tasks ohne Zyklus oder Sackgasse; mit offenem M0-02a bleiben M0-03 und M0-06 ausführbar. Isolierte Prüfroutinen-Proben weisen fehlenden bzw. falsch geordneten `next_check`, mehrere `RUNNING`-Tasks, aktive/erledigte Tasks mit offenen Abhängigkeiten sowie vorzeitig fertige oder falsch datierte Nachweise ab; ein korrekt terminierter wartender Run wird akzeptiert.
- Der Plancheck ist **kein** Beweis für Existenz oder Wirksamkeit der referenzierten Tests. Die in M0-02/M0-02a zu erstellende Dokumentenprüfung muss zusätzlich den bidirektionalen Normtext-Abgleich zwischen Master, korrigiertem TOML und Arbeitskopien leisten; jedes spätere Gate prüft reale Evidenz separat.

## Offene Voraussetzungen

Unabhängige Aufgaben sind abarbeitbar, aber die Software ist noch nicht implementiert. M0-02a und spätere Produkt-/Releasefreigaben können echte externe Entscheidungen erfordern; ein automatischer Abschluss ist damit nicht zugesichert. Lizenz, Toolchain, CI-Hosts, Signierung/Notarisierung, reale Dateisystemprofile sowie die fachlichen Lücken zu Entity/Perspective werden in M0 erhoben oder entschieden. Bis dahin sind weder Plattform- noch Durability- oder Release-Claims bewiesen.

## Ausführungsreihenfolge am 29. September 2026

Der Product Owner bestätigte, dass Git/GitHub wegen eigener Probleme, die während dieses Arbeitslaufs nicht behoben werden können, nachträglich integriert wird. Das ändert keine Norm, Plattformanforderung oder Release-Abnahme.

- M0-15 wird als lokaler Entwicklungs-Vorfreigabepunkt ausgeführt; seine Dependencies umfassen alle lokalen M0-Tasks außer M0-14. Der Punkt prüft weiterhin den realen Offline-Clean-Checkout-Build, MSRV, Policies, Quellen, ODEs und die vorhandenen lokalen Windows-/WSL2-Nachweise. Er schließt M0 nicht vollständig.
- M0-14 bleibt `BLOCKED`; WDB-ENG-005 bleibt bis zu realer CI-Evidenz offen. Die geplanten Anbieterjobs einschließlich macOS müssen nach der Git-Integration auf dem dann aktuellen RC-Stand erfolgreich laufen.
- M9-13b und M10-10 hängen direkt von M0-14 ab. Dadurch können M1–M8 lokal vorbereitet und geprüft werden, während RC-Gate und Veröffentlichung bis zum externen Matrixnachweis gesperrt bleiben.
- Das M0-Vorfreigabe-Kriterium verlangt keine offene HARD-Quellenlücke oder unregistrierte Ausnahme. Die zwei ausdrücklich in ADR-039 registrierten GUARDED-Lücken WDB-ENG-006 und WDB-PER-001 bleiben offen und werden nicht durch diese Reihenfolgeänderung als erledigt behauptet.
- `WorldDB_1.0_Plancheck.py` prüft die Ausnahme exakt: M0-15 lässt nur M0-14 offen, die CI-Invariante WDB-ENG-005 wird nur vor M9 zurückgestellt, und M9-13b/M10-10 müssen M0-14 als direkte Dependency führen.

## Abschlussreview M0-15 am 29. September 2026

Der lokale Vorfreigabepunkt M0-15 bestand auf Basis des Implementierungsstands `493506c7e144c5c1763ff560ac8aba35c5fc3118`. Saubere Windows- und WSL2/Linux-Checkouts bestanden jeweils den gelockten Offline-Build und `cargo xtask verify` mit 27 PASS, einem erwarteten `ci-matrix`-SKIP und 0 FAIL. Die 22 Artefakte des M0-13-Evidenzmanifests sowie die Artefakte der beiden Runner-Manifeste stimmten mit ihren SHA-256-Werten überein. Das Gateprotokoll steht in `docs/gates/M0.md`.

Der abschließende Verify-Lauf nach dem Eintragen von Gateprotokoll, Taskregister und Status bestand ebenfalls mit 27 PASS, einem erwarteten `ci-matrix`-SKIP und 0 FAIL. M0-15 ist DONE; M1-01 ist READY. M0-14 und WDB-ENG-005 bleiben offen, M0 ist nicht abgeschlossen. GitHub-/Anbieter-CI einschließlich macOS wird nach der vom Product Owner angekündigten späteren Git-Integration vor M9-13b, RC und M10-10 nachgeholt.
