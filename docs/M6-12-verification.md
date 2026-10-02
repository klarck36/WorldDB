# M6-12 – Pagination und Streaming

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`ProductiveQueryEngine::stream_page` verbindet `PageRequest` und den M3-Cursorstore mit einem lazily ausgewerteten Kandidatenstrom. Die Ausführung zieht nur bis zur Seitengrenze und höchstens bis zum ersten zusätzlichen sichtbaren Ergebnis. Dieser Lookahead erzeugt einen Cursor nur, wenn eine weitere sichtbare Zeile vorhanden ist. Der vertrauenswürdige Quelladapter startet Fortsetzungen strikt hinter dem gespeicherten SortKey; der Engine-Port erzwingt auf sichtbaren Ergebnissen eine streng steigende, nicht leere Sortierung.

Vor jeder Seite werden Snapshot-Lebensdauer, Operation-Capability und Cursorbindung geprüft. Operation und zeilenbezogene Record-/Field-Rechte werden sowohl gegen die aktuelle als auch die für die Abfrage gewählte Policy ausgewertet. Administrative Rohdaten verlangen zusätzlich `AdminRawRead`. Nur vollständig autorisierte Zeilen beeinflussen Ergebnisse und Budgets.

Kandidaten-, Arbeits- und Ergebnisbudgets laufen über die Fortsetzungen weiter. Cancellation wird vor jedem Pull und vor Rückgabe geprüft. Itemfehler, Cancellation, Budgetende, ungültige Sortierung oder eine invalide Fortsetzung liefern keinen Teil der aktuellen Seite zurück. Der page DTO enthält ausschließlich eigene Ergebniswerte mit der vollständigen Query-/Schema-/Security-Bindung.

Der serverseitige Cursorzustand speichert SortKey, Seitenlimit, Ablaufzeit und bisherige Budgets; QueryHash, Snapshot, Principal, Capability-Fingerprint und SecurityEpoch sind zusätzlich durch den M3-Sessionstore gebunden. Die Ablaufzeit bleibt über Fortsetzungen unverändert. Ein Cursor erhält einen unabhängigen Snapshot-Pin. Die letzte Seite, terminale Fehler, Sicherheitsinvalidierung sowie `reap_expired` geben Zustand und Pin frei; der Engine-Port räumt abgelaufene Zustände bei jeder Seitenausführung auf.

## Nachweise

- Fünf Seitentests decken begrenztes Pulling mit sichtbarem Lookahead, vollständige Fortsetzung, terminale Itemfehler, Cancellation, Ergebnisbudget ohne Teilseite und SecurityEpoch-Wechsel mit Cursorfreigabe ab.
- Ein Snapshot-Leasetest belegt, dass Cursorzustand seinen unabhängigen Pin bis zum Konsum oder Ablauf hält und ihn danach freigibt.
- `cargo test --locked --workspace --quiet`: PASS; 437 Core-Tests sowie die übrigen Workspace- und Rustdoc-Suiten bestanden. Separat markierte Lang-/Plattformläufe bleiben ausgelassen.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- Der Gesamtverifier meldet 34 PASS, 1 erwarteten M0-14-`ci-matrix`-SKIP und 0 FAIL.
- Linux- und macOS-Prüfungen bleiben wie vom Product Owner zurückgestellt offen.

## Abgrenzung

Der vertrauenswürdige Adapter liefert den kanonischen `QueryHash`, den deterministisch sortierten Snapshot-Quellstrom und die konkrete zeilenbezogene Rechteprüfung. Die Engine führt diese Rechteprüfung für aktuelle und ausgewählte Policies erneut aus, berechnet aber weder Query-DTOs noch den kanonischen Hash. Der vollständige Differentialabgleich produktiver Indexpfade bleibt M6-13.
