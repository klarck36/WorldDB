# M3-10a – Cursorhandle und State-Store

**Status:** DONE
**Geprüft:** 2026-09-30T19:30:29+02:00

## Ergebnis

`worlddb-core` enthält einen opaken, festen Wirecursor mit einem zufälligen 256-Bit-Handle und keyed BLAKE3-MAC. Das Wireformat ist 73 Byte groß und enthält ausschließlich Formatversion, Ablaufzeit, zufälligen Handle und MAC. Queryhash, Snapshotbindung sowie Cursorpayload werden ausschließlich serverseitig im Session-Store gehalten.

`CursorStateStore` erzeugt beim Start einen neuen 256-Bit-MAC-Schlüssel aus OS-Entropie. Deshalb sind Handles aus einer früheren Session bzw. nach einem Prozessneustart nicht mehr auflösbar. Der Store begrenzt aktive Einträge, aggregierte Statebytes und Cursorlaufzeit; abgelaufene Einträge werden bei Einfügen entfernt. Unbekannte, abgelaufene, manipulierte, sessionfremde sowie hinsichtlich Snapshot oder Queryhash abweichende Tokens ergeben dieselbe `CursorInvalidated`-Antwort.

## Nachweise

- `cursor::tests::wire_handle_hides_payload_and_tampering_has_uniform_invalidation`: Nutzlast bleibt außerhalb des Wireformats; Tokenmanipulation, abgeschnittenes Wireformat und abweichende Snapshot-/Queryhash-Bindung werden abgewiesen.
- `cursor::tests::expiry_unknown_and_restart_share_cursor_invalidated`: frische Session und abgelaufenes Token liefern dieselbe Invalidation.
- `cursor::tests::state_store_enforces_entry_byte_and_lifetime_limits`: Eintrags-, Byte-, Laufzeit- und Nullgrenzen werden durchgesetzt.
- `cargo test --locked --workspace`: 274 Core-Tests und 73 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL; einschließlich Plancheck und `git diff --check HEAD`.

## Umfangsgrenze

Die Reautorisierung von Principal, effektiven Capabilities und SecurityEpoch vor jeder Seite gehört zu M3-10b. Die späteren Paarwelt- und Langzeitnachweise für M8-26d bleiben als Follow-ups registriert.
