# M1-03 – Integer/Decimal-Kanonik

Status: **DONE**\
Prüfdatum: 2026-09-29 15:42 CEST

## Ergebnis

`Int` und `UInt` sind getrennte Werttypen für den vollständigen Wertebereich von `i128` bzw. `u128`. Ihre Textparser akzeptieren nur kanonische Ganzzahlschreibweisen. Die Binärformen verwenden ZigZag- bzw. unsigned LEB128; Decoder weisen abgeschnittene, überlange und nicht minimale Formen zurück.

`Decimal` speichert Vorzeichen, `u128`-Koeffizient und `i32`-Skala. Der Konstruktor entfernt nachgestellte Koeffizientennullen; Null ist stets vorzeichenlos mit Skala 0. Daher haben Schreibweisen wie `1`, `1.0` und `1.00` dieselbe numerische Identität. Parsing und Vergleich arbeiten ohne Float-Coercion. Die numerische Ordnung vergleicht Vorzeichen, Dezimalexponent und Ziffernfolgen ohne potenziell überlaufende Zwischenmultiplikation.

Die kanonische Dezimal-Binärform ist ein Vorzeichenbyte (0 oder 1), ein minimal kodierter unsigned-LEB128-Koeffizient und eine festbreite little-endian `i32`-Skala. Der strikte Decoder weist ungültige Vorzeichen, überlange oder abgeschnittene Eingaben, nicht minimale Koeffizienten sowie negative Null und nicht normalisierte Werte zurück. Der normale Textparser darf Eingaben normalisieren; `from_canonical_string` akzeptiert nur die eindeutige kanonische Schreibweise. `to_canonical_string` begrenzt die Ausgabegröße auch bei extremen Skalen.

Die Typen sind nicht untereinander gleichsetzbar: `Int`, `UInt` und `Decimal` bleiben getrennt. Es gibt keine Float-Konvertierung in `Decimal`. Der geschlossene `Value`-Katalog folgt in M1-04. WDB-VAL-005 bleibt M1-05 zugeordnet, da dort zusätzlich die Trennung von numerischer Identität und Schema-/Messpräzisionsmetadaten belegt wird.

## Belege

- `crates/worlddb-core/src/numbers.rs`: Typen, Parser, kanonische Kodierung/Decoder und Grenzfalltests.
- `crates/worlddb-core/src/lib.rs`: öffentliche Reexports und Compile-Fail-Beispiele für unzulässige Typvergleiche sowie Float-Konvertierung.
- `cargo test --locked --offline --workspace --all-targets`: 35 Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 14 Compile-Fail-Beispiele bestanden.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: bestanden.
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.

Abgedeckte gezielte Fälle umfassen Integergrenzen und kanonische Varints, Decimal-Skalennormalisierung und numerische Ordnung, kanonische Text-/Binärvektoren, Zurückweisung nicht kanonischer Alternativen sowie Compile-Fail-Nachweise gegen typübergreifende Gleichheit und Float-Coercion.
