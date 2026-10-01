# M3 Gate – lokale Abnahme

**Ergebnis:** PASS für M3s ausführbare indexfreie Referenz- und Boundary-Pfade am 2026-09-30.

## Gate-Umfang

Alle M3-Einzelaufgaben M3-01 bis M3-15 sind DONE. Die M3-API- und Security-Semantik ist auf den lokalen Referenzpfaden ausführbar und getestet. Die kombinierten 96 Fälle aus M3-15 variieren ResolutionPolicy, Mask, Precedence, Polarity, ReplacementBoundary und Security und prüfen Ergebnis- sowie Conflict-Contributors gegen die erwartete Menge.

Die Paarwelt-Tests vergleichen sichtbare Werte, Counts, Shapes, öffentliche Errorcodes und Cursorverhalten. Eigene Tests prüfen Resolved-/Explain-Beitragsfilter, Search-/Graph-/Aggregate-Sichtbarkeit vor Budgets, stabile Errorcodes und Debug-Redaction. M3-12 weist nach, dass eine volle oder konkurrierende Telemetriequeue nur Events verwirft und zählt; M3-13 blockiert Policyänderungen bei fehlendem Required Audit; M3-13a verweigert Admin-Raw-Ausgabe bei fehlender Capability oder Audit-Autorisierung.

## Verifikation

- `cargo test --locked --workspace`: 300 Core-Tests und 75 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py --gate-precheck M3`: PASS; Struktur und Zuordnung geprüft.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS; Git gibt bestehende LF/CRLF-Hinweise aus.
- M3-spezifische Paarbelege sind in `WorldDB_1.0_Folgebelege.tsv` für WDB-BRA-005, WDB-LAY-003/004/011, WDB-REF-001, WDB-TYP-001, WDB-SCH-017, WDB-EVI-004 und WDB-SEC-003/004 auf PASS gesetzt.

## Verbleibende Folgeprüfungen

Der Gate-Precheck selbst weist darauf hin, dass er keine Testwirkung bewertet. Die oben aufgeführten Testläufe und konkreten Evidenzen liefern den inhaltlichen Nachweis. M3 bezieht sich auf indexfreie Core-Referenzpfade; produktive Indizes und Streams werden in M6 wiederholt.

Der unabhängige externe Release-Prüfpunkt M0-14 und WDB-ENG-005 bleiben offen; der Precheck hält deshalb M9-13b und M10-10 zurück. Das hängt nicht von einer GitHub-Verbindung ab und blockiert die hier geprüfte lokale M3-Semantik nicht.
