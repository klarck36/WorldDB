# M1-09a – Archive-Typen

Status: **DONE**\
Prüfdatum: 2026-09-29

## Ergebnis

`crates/worlddb-core/src/archive.rs` enthält einen geschlossenen `ArchiveTargetRef` mit allen aktuell vorgesehenen persistierten Domain- und Lifecycle-Zieltypen. `ArchiveTransition` ist absichtlich kein zulässiges Ziel. Die spätere Konvertierung aus dem zentralen `RecordRef` folgt M1-14.

`ArchiveAction::{Archive, Unarchive}` und `ArchiveState::{Unarchived, Archived}` sind getrennte Typen. `ArchiveState::transition` erlaubt nur `Unarchived → Archived` und `Archived → Unarchived`; wiederholtes Archivieren liefert `AlreadyArchived`, Unarchive eines nicht archivierten Ziels liefert `NotArchived`. `ArchiveTransition` trägt eine eigene `ArchiveTransitionId`, ein getyptes Ziel, eine Action und die `created_revision`.

Archive ist damit eine reversible operative Sichtbarkeit. Es verändert weder Weltzeit-Validity noch Assertion-Retraction und löscht keine Bytes. Purge bleibt die separate administrative Offline-Neuschreibung und besitzt keine Variante in `ArchiveAction`.

## Grenzen und Folgebelege

Der Konstruktor nimmt den zuvor ermittelten Zielzustand entgegen, liest aber selbst keine Historie. As-of-Projektion, Autorisierung, Raw-History-Verhalten, konkurrierende Transaktionen und Recovery folgen M2-05a/M4-05b. Die `RecordRef`-Konvertierung und Wire-/Codec-Regeln folgen M1-14/M1-17b.

## Belege

- `crates/worlddb-core/src/archive.rs`: geschlossene Zielmenge, getrennte Actions/Zustände und immutable Transition-Recordform.
- `crates/worlddb-core/src/lib.rs`: öffentliche Exporte und Compile-Fail-Fälle für Purge, Closure, Retraction und Selbstziel.
- `archive::tests::archive_and_unarchive_are_reversible_and_reject_repeated_actions`: erlaubte und abgewiesene Zustandsübergänge.
- `archive::tests::transition_record_has_a_concrete_id_typed_target_and_revision`: positiver typed-record-Nachweis.
- `cargo test --locked --offline --workspace --all-targets`: 53 Core-, 7 Testkit- und 4 xtask-Unit-Tests bestanden.
- `cargo test --locked --offline --doc --package worlddb-core`: 33 Dokumentationstests bestanden (32 Compile-Fail-Beispiele und ein positives Beispiel).
- `cargo xtask verify`: 27 PASS, 1 sichtbarer SKIP (`ci-matrix`, M0-14), 0 FAIL.
