# M6-09 – Suche und FullText-Grenze

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`ProductiveQueryEngine::token_search` verbindet die M3-Token-Suche mit der produktiven Query-Engine und dem gebundenen `OwnedQueryResult`-Port:

- TokenSearch verwendet den vollständigen Scan, weil das geschlossene M6-Indexregister noch keine Tokenposting-Familie enthält. Der Ausführungspfad wird als `FullScan` ausgewiesen.
- Die bestehende 1.0-Semantik bleibt bytegenau und case-sensitiv: gespeicherter Text wird nur an ASCII-Whitespace getrennt. Es gibt keine implizite Normalisierung, Stemming-, Phrase- oder Rankingsemantik.
- `QuerySearch`, passende Record-Read-Rechte und `FieldRead` werden geprüft, bevor ein Suchdokument tokenisiert oder Kandidatenbudget verbraucht. Ein nicht lesbares angefragtes Feld entfernt das Dokument vollständig aus dem Suchpfad.
- Treffer enthalten ausschließlich den typisierten RecordRef und die autorisierten passenden Feldselektoren. Suchtext und Snippets werden nicht ausgegeben; das folgt dem 1.0-Query-Vertrag.
- FullText bleibt die optionale, in dieser 1.0-Engine nicht implementierte Operation. `QueryFullText` wird nicht als `QuerySearch` behandelt; der Vertrag erlaubt für ein ausgelassenes FullText eine Unsupported-Antwort. Es gibt keinen stillen Rückfall auf TokenSearch. Eine spätere FullText-Implementierung benötigt weiterhin ausdrücklich registrierte indexierte Textfelder und die separate Capability.

## Nachweise

- Sechs gezielte Search-Tests bestanden: produktiver Engine-Port und Ergebnisbindung, bytegenaue Tokenregeln, versteckte Records und Felder vor Budget/Tokenisierung, getrennte QuerySearch-/QueryFullText-Rechte, Schemafeldvalidierung sowie Budget-/Cancellation-Abbruch ohne Teilergebnis.
- `cargo test --locked --workspace --quiet`: PASS; 428 Core-Tests, 43 Storage-Tests, 5 Wire-Oracle-Tests, 82 Rustdoc-Tests und die übrigen Workspace-Suites bestanden. Die drei separat markierten Langkampagnen blieben ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.
- Windows-Plattformprofil; Linux- und macOS-Prüfungen bleiben wie vom Product Owner zurückgestellt offen.

## Abgrenzung

TokenSearch ist produktiv über den sicheren Full-Scan-Pfad verfügbar, nicht über persistente Tokenpostings. FullText ist ausdrücklich nicht implementiert, weil 1.0 dafür keine registrierte Textindexfamilie und kein dauerhaftes Feldopt-in enthält. Das ist vom Query-Vertrag gedeckt und verhindert, dass ein bloßes Capability-Grant eine unindexierte Textsuche freischaltet.
