# M1-13 – Source, Evidence und Provenance

**Status:** DONE\
**Datum:** 29. September 2026\
**Taskregister:** `WorldDB_1.0_Taskregister.tsv`

## Ergebnis

`crates/worlddb-core/src/source_provenance.rs` implementiert die unveränderlichen Meta-History-Typen; `crates/worlddb-core/src/lib.rs` exportiert sie:

- `Source` besitzt eine validierte `Symbol`-Art, optionale bytegenaue Locator- und Digestwerte, kanonisch sortierte eindeutige Metadaten und eine Erstellungsrevision. Ein Digest bleibt opak, weil der Vertrag für Source keinen Algorithmus und keine feste Länge festlegt.
- `Evidence` bindet eine Source über eine geschlossene `EvidenceTargetRef` und `Supports | Contradicts | Documents`. Die Enum enthält die 20 zulässigen konkreten Varianten aus §31.2.1; Source, Evidence und direkte EventRelation-Referenzen sind nicht konstruierbar.
- `ProvenanceEdge` verwendet die 22 konkreten Varianten der geschlossenen `ProvenanceEndpointRef`. `Corrects` akzeptiert nur dieselbe konkrete Variante, `DerivedFrom.from` sperrt Mask, ReplacementBoundary und EventMask, und `ResultedFrom` akzeptiert Assertion oder Event auf beiden Seiten.
- `ProvenanceEndpointRef` hat weder offene String-/`Other`-Varianten noch direkte EventRelation-, Schema-, Migrations-, Security- oder Audit-Varianten. Dafür bleiben eigene Referenzverträge vorgesehen.
- `EvidenceRetraction` und `ProvenanceRetraction` sind getrennte immutable Lifecycle-Records mit eigenen IDs und Transaction-Time-Revisionen nach dem Zielrecord. Diese Meta-History führt keine World-Time.

## Abgrenzung

Die Konstruktoren prüfen die geschlossenen Typ- und Relationsmatrizen. Ob referenzierte Records im passenden historischen Stand sichtbar sind, sowie Child-/Parent-Cutoffs und öffentliche Endpunktautorisierung, folgt M2-14 und M3-04. Assertion-Proposition-Slot, EventKind-Kompatibilität und die Transaktionsreihenfolge bei `Corrects` folgen M2-15a. Aktive Duplikate und gemischte Provenance-Zyklen im gemeinsamen Graphen folgen M2-15a/b. `RecordRef`-Konversionen folgen M1-14; Decoder- und Fuzz-Negativfälle folgen M1-17d. Die invariantenbezogenen Folgepaare sind in `WorldDB_1.0_Folgebelege.tsv` registriert.

## Prüfung

- `cargo fmt --all` — bestanden.
- `cargo test --locked --offline --workspace --all-targets` — 85 Tests bestanden (74 Core-, 7 Testkit- und 4 xtask-Tests).
- `cargo test --locked --offline --doc --package worlddb-core` — 46 Dokumentationstests bestanden, einschließlich Compile-Fail-Prüfungen für gesperrte Varianten und den fehlenden `Other`-Escape-Hatch.
- `cargo xtask verify` — 27 PASS, 1 sichtbarer `ci-matrix`-SKIP (zurückgestellter M0-14-Nachweis), 0 FAIL.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py` — nach Aktualisierung der Task-, Invarianten- und Folgebelegregister bestanden.

Die Corrects-Prüfung durchläuft alle 484 Paare der 22 konkreten Endpoint-Varianten: 22 gleichartige Variantenpaare werden akzeptiert, alle 462 inkompatiblen Paare abgewiesen. Zusätzliche Fälle prüfen Self-Loops, die drei verbotenen `DerivedFrom.from`-Familien und `ResultedFrom` mit allen verbotenen Varianten auf beiden Seiten.

## Nachtrag aus M1-14

Die Variantenprüfung in M1-14 hat bestätigt, dass ArchiveTransition als Lifecycle Record unter Master §31.2.1 in beide allgemeinen Endpoint-Mengen gehört. EvidenceTargetRef umfasst daher jetzt 21 konkrete Varianten und ProvenanceEndpointRef 23. Die Corrects-Matrix umfasst 23×23 = 529 Paare: 23 kompatible Variantenpaare werden akzeptiert, 506 inkompatible Paare und Self-Loops abgewiesen. ResultedFrom bleibt auf Assertion und Event beschränkt; die 21 übrigen Endpoint-Varianten werden auf beiden Seiten abgewiesen.

## Nächster Schritt

`M1-14 – Geschlossenen RecordRef vervollständigen` ist im Taskregister `READY`.
