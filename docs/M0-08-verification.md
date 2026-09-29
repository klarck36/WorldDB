# M0-08 – Nachweis zu ODE-005 UUIDv7

**Status:** DONE
**Geprüft:** 2026-09-29
**Artefaktcommit:** `adfaa57` (`docs: decide WorldDB UUIDv7 generation`)

## Entscheidung

[ADR-037](contract/ADR-037-uuidv7-generator.md) wählt einen kleinen internen UUIDv7-Generator, der Fehler explizit zurückgibt. Er basiert auf `SystemTime`, `getrandom::fill` und `uuid::Builder::from_unix_timestamp_millis`. `uuid 1.26.1` bleibt für UUID-Wert, Parsing/Formatierung und RFC-Bitlayout erhalten. Der Produktionspfad ruft `Uuid::now_v7()`/`Uuid::new_v7()` nicht auf: deren RNG-Fehlerpfad panikt und verletzt den WorldDB-Vertrag zur Fehlerbehandlung bei Uhr- und Zufallsquellen. Die interne Funktion gibt typisierte Fehler zurück und weicht bei fehlender Entropie nicht auf eine schwächere Quelle aus.

Der Generator verwendet die 74 freien UUIDv7-Payloadbits als Zufall und verspricht keine Ordnung innerhalb derselben Millisekunde. Das entspricht dem Master, der Zeitbits als Betriebseigenschaft und nicht als fachliche Chronologie festlegt. Die Entscheidung kapselt UUIDs hinter WorldDB-ID-Newtypes; eine spätere interne Ersetzung migriert keine gespeicherten Bytes.

## Nachweise

- Die RFC-9562-Referenz aus Appendix A.6 wird bytegenau als `017f22e2-79b0-7cc3-98c4-dc0c0c07398f` erzeugt.
- `ContextV7` erzeugt bei festem Zeitstempel 4.096 kollisionsfreie UUIDs in streng steigender Reihenfolge. Der gewählte Zufalls-Payloadgenerator erzeugt weitere 4.096 IDs am selben Zeitstempel ohne beobachtete Kollision; diese Stichprobe ist kein Eindeutigkeitsbeweis.
- Proptest prüft mit `PROPTEST_CASES=4096` und Seed `8675309` Zeitstempelpräfix, Version, RFC-Variante, Kleinschreibung und Parse-Roundtrip über 48-Bit-Zeitstempel und 80 Eingaberandombits. Zusätzliche Fälle prüfen Uhrfehler, Entropiefehler, maximale Zeitstempelgrenze und die fehlende Same-Millisekunden-Ordnung.
- Der echte Systemuhr-/OS-CSPRNG-Pfad erzeugt eine gültige UUIDv7.
- Unter Rust/Cargo 1.85.0 auf Windows x86_64 MSVC: `cargo test --locked` mit 10 bestandenen Tests; `cargo check --locked --all-targets` erfolgreich.
- Der gesperrte Graph hat 62 Registry-Pakete und 77 Zeilen in `cargo tree --locked --target all`. Keine deklarierte MSRV liegt über 1.85.0; `blake3`, `unarray` und `zerocopy-derive` deklarieren keine MSRV und wurden auf der Zieltoolchain gebaut. Die UUID-v7-Zielabhängigkeiten sind dual MIT/Apache lizenziert. Das Projekt selbst hat weiterhin keine gewählte Lizenz.
- `build_contract_sources.py --verify-only`, `verify_contract_docs.py`, `WorldDB_1.0_Sourcecheck.py` und `WorldDB_1.0_Plancheck.py` bestanden. Die Offene-Entscheidungen-Datei blieb bytegetreu zum Quellspiegel; der Status führt ODE-005 mit ADR-037 als `DECIDED`.

## Reproduktion

Aus dem Repository-Stamm in PowerShell; `CARGO_TARGET_DIR` liegt absichtlich außerhalb des OneDrive-Projekts:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'WorldDB\test-runs\M0-08\target'
$env:PROPTEST_CASES = '4096'
$env:PROPTEST_RNG_SEED = '8675309'
Set-Location experiments\msrv-spike
cargo test --locked
cargo check --locked --all-targets
cargo tree --locked --target all
cargo metadata --locked --format-version 1 --all-features
```

Die Laufzeitabhängigkeiten wurden für Windows, Linux und macOS aufgelöst; kompiliert wurde nur Windows. Die endgültige Produktabhängigkeitsmatrix, weitere Plattform-Builds sowie Dependency-/Unsafe-Policy bleiben Folgearbeit in M0-09/M0-12/M0-14/M0-15.

## Primärquellen

- [RFC 9562 – UUIDv7 und Appendix A.6](https://www.rfc-editor.org/rfc/rfc9562.html)
- [`uuid 1.26.1` UUIDv7-API und Quellcode](https://docs.rs/uuid/1.26.1/uuid/struct.Uuid.html#method.now_v7)
- [`getrandom 0.4.3` fallible `fill`-API](https://docs.rs/getrandom/0.4.3/getrandom/fn.fill.html)
