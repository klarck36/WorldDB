# M5-06 – Immutable Historysegmente

**Ergebnis:** DONE – lokal auf Windows geprüft am 1. Oktober 2026. Linux- und macOS-Läufe sind auf Wunsch des Product Owners zurückgestellt und nicht Teil dieses Ergebnisses.

## Umsetzung

`worlddb-storage-file::HistorySegmentStore` schreibt jede Variante des geschlossenen Core-`Record`-Typs als kanonisches, unveränderliches Segment. Die Dateierstellung verwendet `create_new`, bindet Schreibzugriffe an den Writerlock desselben Datenbankroots und synchronisiert den Segmentinhalt mit `File::sync_all`. Ein vorhandener Segmentpfad wird niemals überschrieben. `write_decoded_segment` bewahrt zusätzlich unbekannte optionale Framebits.

Die 56-Byte-Dateihülle enthält Formatkennung, registrierte zufällige `SegmentId` und getrennten `ContentDigest`. Die Segment-ID wird mit dem falliblen Betriebssystem-CSPRNG erzeugt und auf UUIDv4 gesetzt. Der Digest ist BLAKE3 über den kanonischen Inhalt und schließt die Segment-ID sowie veränderliche Dateisystemmetadaten aus.

Der kanonische Inhalt enthält Major-/Minor-Formatversion, Recordzahl, ein festes Inhaltsverzeichnis aus Little-Endian-Offset-/Längenpaaren und die kanonischen Core-Recordframes. Die Frames werden nach ihren Bytes sortiert; damit ergeben dieselben Recordinhalte unabhängig von ihrer Eingabereihenfolge denselben Digest. Der Reader prüft separat die angeforderte Segment-ID, den Digest, Version, Grenzen und Kontiguität des Inhaltsverzeichnisses, die kanonische Frame-Reihenfolge sowie jedes Recordframe gegen den Core-Decoder. Die Ressourcenlimits entsprechen höchstens 1.000.000 Frames, 64 MiB pro Frame und 256 MiB Framebytes.

## Nachweise

- `test:random_segment_identity_and_content_digest_are_independent` – zwei identische Recordmengen erhalten unterschiedliche zufällige IDs und denselben Digest; HistorySpaceDefinition, Entity, EntityRetirement, PerspectiveDefinitionRevision, LayerDefinition und ArchiveTransition roundtrippen bytegleich. Die erste Datei bleibt nach einem zweiten Write bytegleich.
- `test:decoded_segment_preserves_optional_record_flags` – optionale Recordflags bleiben beim Segment-Roundtrip erhalten.
- `test:readback_rejects_changed_identity_and_content_digest` – veränderte Hüllen-ID und veränderter Digest werden unabhängig abgewiesen.
- `test:segment_writes_require_a_matching_lock_and_nonempty_records` – fremder Writerlock und leeres Segment werden abgewiesen.
- `test:segment_id_validates_the_registered_uuid_shape` – die registrierte ID-Grammatik bleibt erzwungen.
- `test:segment::tests::create_new_file_refuses_to_overwrite_existing_segment_bytes` – `create_new` lehnt eine bestehende Segmentdatei ab und lässt ihre Bytes unverändert.

## Grenzen

Manifestbindung, CURRENT-Publikation, Directory-Sync, unabhängiger Verify, Crash-Recovery und Restart-Rekonstruktion sind nachgelagerte Aufgaben M5-07, M5-12 und M5-16. Diese Implementierung behauptet für sich allein keine Machine-Durability. Linux/ext4 und macOS/APFS werden in den späteren Plattformaufgaben M5-08 und M5-10 geprüft.

## Windows-Prüflauf

`cargo test --locked --workspace`: 396 Core-Unit-Tests, 23 Storage-File-Tests, 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; die zwei CPU-Langläufe bleiben absichtlich ignoriert. `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, Plancheck, Sourcecheck, Dependency-Policy, Crategraph und `cargo-deny check all` bestanden. `cargo xtask verify`: 32 PASS, ein erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. M0-13-Evidenzlauf `M0-13-20261001T125839Z-a62c6a7b44` ist PASS. Diese Prüfläufe belegen Windows; Linux/macOS-Prüfungen wurden nicht ausgeführt.
