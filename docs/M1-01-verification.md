# M1-01 – Getypte IDs und Revisionen

**Stand:** 2026-09-29\
**Umfang:** Master §3.1, ADR-030/031 und ADR-037; WDB-ID-001–004, WDB-SEN-001, WDB-TYP-001.

## Umsetzung

- `worlddb-core::ids` registriert 46 getrennte UUID-Newtypes: die 43 Typen der §3.1-Taxonomie sowie `EntityRetirementId`, `PerspectiveRetirementId` und `ArchiveTransitionId` aus den beschlossenen Lifecycle-Ergänzungen. `BranchId`, `SchemaVersionId` und `LifecycleRecordId` sind nicht definiert.
- Jeder ID-Typ hat einen expliziten Namespace-, Persistenz-, Wire- und Public-API-Scope. `SnapshotId` ist sessionlokal und implementiert `PersistentId` nicht. `SegmentId` bleibt eine interne Backend-Identität und wird nicht aus dem Crate exportiert.
- UUID-Parser akzeptieren ausschließlich kanonischen kleingeschriebenen UUID-Text und RFC-UUID-Versionen 1–8 mit RFC-Variante. Nil und All-FF werden abgewiesen. UUID-Zeitbits werden weder als WorldDB-Zeit noch als fachliche Ordnung oder Authentizität exponiert.
- Der private UUIDv7-Generator verwendet `SystemTime`, `getrandom::fill` und den UUID-Builder. Uhr-, Entropie- und 48-Bit-Zeitstempelfehler liefern typisierte Fehler; es gibt keinen Panic- oder Zufallsfallback. Für IDs derselben Millisekunde ist keine Sortierreihenfolge zugesichert.
- `Revision` und `SchemaRevision` sind getrennte `u64`-Newtypes auf derselben Achse. Genesis ist 0, der erste Commit ist 1, und `u64::MAX` bleibt reserviert. `SchemaRevision` besitzt keinen eigenen Zähler. Die Zugehörigkeit einer Revision zur tatsächlich veröffentlichten Schemahistorie muss der spätere History-Reader/Writer prüfen; M1-01 implementiert noch keine History.
- `uuid 1.26.1` und `getrandom 0.4.3` sind gepinnt und im Dependency-Register, in ADR-037 sowie im Crate-Graph-Check eingetragen. Der cargo-deny-Interpreter-Scan bleibt aktiv; ausschließlich `libc/etc/libc-util.py` ist als mitgelieferte Entwicklerhilfe pfadgenau ausgenommen.

Der UUIDv7-Generator erhält in M5-14 erstmals einen Produktaufruf über die interne Core-Funktion für Salvage-Fork-Identitäten. Damit sind die drei lokalen Dead-Code-Ausnahmen aus `WDB-EXC-0001` entfallen.

## Verifikation

- `cargo test --locked --offline --workspace --all-targets`: 21 Tests bestanden (10 core, 7 testkit, 4 xtask).
- `cargo test --locked --offline --doc --package worlddb-core`: 5 Compile-Fail-Beispiele bestanden, darunter Typvertauschung, Persistenz eines `SnapshotId`, die drei verworfenen Alttypen, internes `SegmentId` und UUID-Zeitbits als Domain-Zeit.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `cargo-deny 0.20.2 --config .cargo/deny.toml --workspace --locked check all`: Advisories, Bans, Lizenzen und Sources bestanden.
- `tools/check_dependency_policy.py`: 22 gelockte externe Pakete, keine verbotenen Featurepaare; `tools/check_exceptions.py` und `tools/check_terminology.py` bestanden.
- `tools/check_crate_graph.py` und `test_crate_graph.py`: erlaubte Workspace-Richtung und exakt registrierte externe Core-Abhängigkeiten bestanden; 5 Policy-Tests bestanden.
- Kanonischer Abschlusslauf: `cargo xtask verify` – 27 PASS, ein sichtbarer `ci-matrix`-SKIP, 0 FAIL.

Der sichtbare `ci-matrix`-Skip bleibt an M0-14 gebunden. Externe Provider-CI und macOS sind nicht Teil dieses lokalen M1-01-Nachweises.
