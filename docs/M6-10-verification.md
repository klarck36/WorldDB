# M6-10 – sichere Graph-Traversierung

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`ProductiveQueryEngine::graph_traversal` bindet die M3-Graph-Traversierung in die produktive Query-Engine ein. Der Aufruf akzeptiert nur eine `GraphCandidateSet`, deren `QueryContextBinding` exakt zum vollständigen aktuellen Kontext passt. Das bindet unter anderem Snapshot und Revision, `RecordedAsOf`, HistorySpace, Layer, WorldTimeSelector, Perspektive, Epistemik, Schema, Principal/Autorisierungsmodus und Budgets. Ein abweichender Weltzeitpunkt oder ein ungebundener Kandidatensatz wird vor der Ausführung fail-closed abgewiesen.

Der M3-BFS prüft zuerst die Operation `QueryGraphTraverse`, Record-Leserechte für Knoten und RelationshipRead-Rechte für Kanten. Kanten mit einem nicht sichtbaren Endpunkt werden vollständig ausgelassen. Erst sichtbare Kandidaten beeinflussen Traversierung und Budgets. `GraphSpec` erzwingt Relationship-Typen, Richtung, Tiefe, Knoten-/Kantenlimits und die Cycle-Policy. `EventSameTime` bleibt symmetrisch. Knoten-/Kanten-, Arbeitsbudget- oder Cancellation-Abbruch liefern einen terminalen Fehler ohne Teilergebnis; ausgegebene Fehler enthalten keine verborgenen Kandidatenzahlen.

Der produktive Aufruf meldet `FullScan`. Er traversiert den vom vertrauenswürdigen Host bereitgestellten, vollständigen Kandidatensnapshot und wendet die M3-Autorisierung darauf an. Die M6-05-Event- und M6-06-Provenance-Postings werden durch diesen Einstiegspunkt noch nicht selbst ausgewählt; ein späterer Indexpfad muss dieselbe vollständige Contextbindung und einen vollständigen sicheren Fallback belegen.

## Nachweise

- Vier gezielte Graph-Tests bestanden: produktiver Engine-Port mit Zurückweisung ungebundener Kandidaten und abweichender WorldTimeSelector-Bindung; Nichtinterferenz versteckter Knoten/Kanten; Cycle-Policy; Nulltiefe sowie Budgetabbruch ohne Teilergebnis.
- `cargo test --locked --workspace --quiet`: PASS; 429 Core-Tests sowie alle übrigen Workspace- und Rustdoc-Suites bestanden. Drei separat markierte Langkampagnen blieben ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 34 PASS, 1 erwarteter SKIP, 0 FAIL. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.
- Windows-Plattformprofil; Linux- und macOS-Prüfungen bleiben wie vom Product Owner zurückgestellt offen.

## Abgrenzung

Die Graph-Abfrage verwendet den vollständigen `QueryContext` einschließlich des Weltzeitselektors als Kandidaten- und Ergebnisbindung. Die M3-Kantenprojektion selbst liest keine unabhängigen Weltzeitfenster aus `GraphSpec`: Ereignisrelationen sind nach `RecordedAsOf` aktiv, während zeitabhängige Knotenprojektionen vom Host für den gebundenen Kontext bereitgestellt werden. Es wird keine transitive Eventrelation erfunden.
