# M1-04 – Geschlossener Value-Katalog

Status: **DONE**\
Prüfdatum: 2026-09-29 15:50 CEST

## Ergebnis

`worlddb_core::Value` enthält genau zehn Varianten: `Bool`, `Int`, `UInt`, `Decimal`, `String`, `Symbol`, `Entity`, `Time`, `Duration` und `Bytes`. Die Varianten verwenden die bereits validierten Integer-, Decimal-, ID- und Duration-Typen. `String` hält gültiges UTF-8 bytegenau; `Bytes` speichert eine unveränderte Bytefolge.

Die Symbolgrammatik wurde nach Rückfrage beim Product Owner festgelegt: `[a-z][a-z0-9_]*`. Das erste ASCII-Byte ist ein Kleinbuchstabe; weitere Bytes sind ASCII-Kleinbuchstaben, Ziffern oder Unterstriche. Großbuchstaben, Punkte, Bindestriche, leere Symbole und Nicht-ASCII-Bytes werden abgewiesen. Es gibt weder Unicode-Normalisierung noch eine implizite Umschreibung.

`Time` bewahrt `TimelineId`, signed Ticks und ein validiertes Unit-`Symbol`. Ein ausgewählter Schema-Snapshot muss Timeline und Einheit auflösen und die Nanosekunden-Normalisierung auf Überlauf prüfen, bevor Zeitwerte verglichen oder akzeptiert werden. `Time` sowie `Value` implementieren deshalb kein direktes `Eq`/`Ord`; insbesondere existiert keine globale fachliche Value-Ordnung. Die zeitbezogene Gleichheit benötigt die schemaaufgelöste Nanosekundenkoordinate. `WorldTime` aus M1-02 stellt diese Koordinate bereits innerhalb einer Timeline dar.

Null, Float, generisches JSON, Array, Map und `CalendarPeriod` sind keine `Value`-Varianten. Die Compile-Fail-Beispiele belegen die ausgeschlossenen Formen sowie das Fehlen globaler Ordnung. `CalendarPeriod` bleibt ein eigenständiger Schema-/Query-/Migrationstyp gemäß ADR-032; diese M1-Aufgabe legt lediglich die Grenze zum Assertion-Value fest. Die Wire-Scalarkodierung und Decoderbudgets folgen in M1-16 und M1-18.

## Belege

- `crates/worlddb-core/src/values.rs`: geschlossene Enum, Symbol- und Bytes-Typ, Time-Eingabeform sowie vier gezielte Unit-Tests.
- `crates/worlddb-core/src/lib.rs`: öffentliche Reexports und Compile-Fail-Beispiele für Null, Float, JSON, Array, Map, CalendarPeriod sowie globale `Ord`-/Vergleichsversuche.
- `cargo test --locked --offline --workspace --all-targets`: 39 Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 22 Dokumentationstests bestanden (21 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.

WDB-VAL-005 bleibt M1-05 zugeordnet: Die Trennung von numerischer Decimal-Identität und Darstellungs-/Messpräzisionsmetadaten wird dort belegt.
