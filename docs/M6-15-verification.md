# M6-15 – Ressourcenbudgets und M6-Gate

**Profil:** Windows 11, x86_64-pc-windows-msvc, Rust 1.85.0
**Ergebnis:** lokale M6-Prüfung; Plattformnachweise bleiben gemäß Product-Owner-Anweisung für M9-07 zurückgestellt.
**Stand:** 2026-10-02

## Prozessprofil und harte Grenzen

`ProcessResourceProfile` Version 1 setzt standardmäßig 512 MiB Softlimit und 1 GiB Hardlimit. Das Softlimit meldet Speicherdruck; das Hardlimit wird über gemeinsam genutzte Admission-Reservierungen für Query- und Indexpfade durchgesetzt. Hosts können vor dem ersten Query-, Decoder- oder Indexzugriff genau ein validiertes Profil installieren. Danach ist ein Profilwechsel abgewiesen, damit aktive Reservierungen nicht unter neue Grenzwerte umgedeutet werden.

Der Prozessledger ist kooperativ: Er zählt ausdrücklich reservierte Bytes und ist kein vom Betriebssystem erzwungenes RSS-Limit. Reservierungsgrößen sind begrenzte Schätzwerte; Prozess-, Runtime-, Allocator- und nicht integrierte Bibliotheksallokationen werden nicht als RSS-Garantie ausgegeben.

Querybudgets haben zusätzlich unveränderliche absolute Obergrenzen: 1.000.000 autorisierte Kandidaten, 10.000.000 Arbeitseinheiten und 100.000 Ergebniszeilen. Höhere Limits werden bei der Querykonfiguration abgewiesen. QueryContext hält das gemeinsame Prozessledger; Kandidaten, Seitenergebnisse, Suche, Graphtraversierung und Aggregation reservieren vor wachsendem Ergebnisbuffer. Fehler liefern kein als vollständig markiertes Teilergebnis.

Die Indexfamilien für Record-ID, Operation-ID, Schema-ID/Revision, Lifecycle, Assertion-Point, Validity, Mask-Selector, ContextPrecedence, ReplacementBoundary und Event-Suche/-Zeit/-Mask/-Relation halten eine Indexreservierung über ihre Lebensdauer. Querygebundene Source-/Evidence-/Provenance-Nachbarschaften halten stattdessen eine Queryklassen-Reservierung. Die Größenberechnung prüft Überläufe; Ablehnung erfolgt vor dem Aufbau der Indexpostings. Für Tests stehen explizite gemeinsame Testledgers bereit.

## Parserlimits

Die Standard-Wire- und Auditdecoder leiten ihre Limits aus dem Parserbudget des aktiven Prozessprofils ab. Größere explizit gesetzte Limits bleiben eine bewusste Host-Konfiguration. Das Profil kann die Standardhöchstwerte weiter absenken.

Die v1-Standardwerte begrenzen einen Frame auf 64 MiB, einen String-/Byteswert auf 16 MiB, ein einzelnes Collection-Reservationbudget auf 64 MiB, einen Batch auf 256 MiB und einen Batch auf höchstens 1.000.000 Datensätze; die Struktur ist auf Tiefe 8 begrenzt. Felder pro Record sind auf 256 begrenzt. Länge, Zähler, Überlauf und Allokationsfehler werden vor Kopie oder Bufferwachstum geprüft. Parserlimits sind pro Decodieroperation; diese API behauptet keine OS-weite RSS-Isolation paralleler Hosts.

## Sicherheits- und Fehlerpfade

- Assertion-Point- und Full-Scan-Kandidaten prüfen Record- und FieldRead-Rechte vor Kandidatenaufnahme; verborgene Einträge zählen nicht gegen das sichtbare Kandidatenbudget.
- Suche autorisiert Records und Felder vor dem Ergebnisindex. Graphkanten erscheinen nur, wenn Knoten, Beziehung und erforderliche Felder sichtbar sind. Aggregation prüft Sichtbarkeit vor Gruppenschlüsseln und Fehlern, sodass verborgene Felder keine Duplikat- oder Gruppenfehler offenlegen.
- Page-, Search-, Graph-, Aggregate-, Query- und Indexbudgetfehler liefern einen typisierten terminalen Fehler ohne Teilergebnis als komplett zu kennzeichnen.
- Gezielte Non-Interference-Nachweise liegen in `candidate_scan`, `query_engine`, `query_search`, `query_graph`, `query_aggregate`, `provenance_graph`, `event_relations` und `errors`. Der deterministische M6-13-Test vergleicht 96 indizierte Queryausführungen über drei Welten mit dem Full-Scan-Orakel.

## Prüfergebnisse

- `cargo test --locked --workspace`: **PASS**; Core 450 Unit-Tests, Wire-Orakel 5, Decoderinventar 1 (Langfuzzkampagne separat), 82 Rustdoc-Tests; alle übrigen Workspace-Crates bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS**; `cargo fmt --all -- --check`: **PASS**.
- `python -B -X utf8 WorldDB_1.0_Sourcecheck.py`, `python -B -X utf8 docs/contract/build_contract_sources.py --verify-only`, `python -B -X utf8 tools/check_exceptions.py` und M6-Gate-Precheck: **PASS**.
- `cargo xtask verify`: **36 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**. Die verbliebene M0-14-CI-Ausführung ist nicht Teil dieses lokalen Gates.
- `git diff --check`: **PASS**.

## Plattformgrenze

Linux/macOS, echte Datei-/OS-Cold-Cache-Messungen sowie plattformübergreifendes Recovery-Replay werden nicht als M6-Ergebnis behauptet. Sie bleiben für M9-07 vorgemerkt. Die Windows-Messwerte und die vorläufige ODE-003-Entscheidung stehen in `docs/M6-14b-windows-progress.md` und `docs/contract/ADR-040-performance-resource-budgets.md`.
