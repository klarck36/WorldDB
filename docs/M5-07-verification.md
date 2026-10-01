# M5-07 – Manifestgenerationen und CURRENT

**Ergebnis:** DONE auf Windows/NTFS für die generische Manifestpublication. Linux/ext4 und macOS/APFS wurden auf Anweisung des Product Owners nicht geprüft und bleiben ihren Plattformtasks M5-08 und M5-10 vorbehalten.

## Implementierung

- `ManifestStore` schreibt unveränderliche, durchnummerierte Generationen unter `manifests/` und liest ausschließlich die von `CURRENT` benannte Generation.
- Eine Generation bindet ihre Revision an den Commit-Hash genau dieser Revision. `WalPrepareLog::verified_head_at` prüft dafür die vollständige vorhandene WAL-Hashkette; ein älterer, tatsächlich committed Stand darf materialisiert werden.
- Jede Segmentreferenz bindet Namespace, `SegmentId`, `ContentDigest` und höchste deklarierte Datenrevision. Sortierung, eindeutige IDs, Revisionen und Eingabelimits werden beim Erstellen und Lesen validiert.
- Generation und `CURRENT` besitzen versionierte kanonische Binärformen und BLAKE3-Prüfsummen. `CURRENT` bindet zusätzlich den Digest der vollständigen Manifestdatei.
- Veröffentlichung schreibt und synchronisiert zunächst die Stagingdatei der Generation, benennt sie in `manifests/` um und synchronisiert das Manifestverzeichnis. Danach wird `CURRENT` gestaged, synchronisiert und ersetzt; zuletzt wird das Datenbankverzeichnis synchronisiert.
- Unter Windows wird ein Verzeichnishandle mit `FILE_FLAG_BACKUP_SEMANTICS` geöffnet und mit `sync_all` synchronisiert. Schlägt der Manifest-Verzeichnissync fehl, wird `CURRENT` nicht ersetzt. Ein Fehler beim letzten Verzeichnissync wird als Fehler gemeldet; die Pointerdatei kann dann bereits sichtbar sein und muss beim erneuten Öffnen gelesen werden.
- Ein neuer Manifeststand oberhalb der verifizierten WAL-Revision, ein Rückschritt gegenüber `CURRENT`, ein nicht passender Commit-Hash und eine Segmentrevision oberhalb des Manifests werden abgewiesen.

## Nachweise

- `test:manifest_publication_advances_current_only_after_a_verified_generation` – Generationen werden angelegt und `CURRENT` wechselt auf die neueste verifizierte Generation.
- `test:a_lagging_manifest_is_allowed_but_ahead_of_wal_manifest_is_rejected` – Revision 1 darf bei WAL-Stand 2 veröffentlicht werden; Revision 3 wird bei WAL-Stand 2 abgewiesen und lässt `CURRENT` unverändert.
- `test:manifest_revision_cannot_regress_and_segment_revisions_are_bounded` – Rückschritt und über die Snapshotrevision hinausreichende Segmentreferenz werden abgewiesen.
- `test:current_pointer_digest_detects_manifest_tampering` – Änderung am Manifest wird beim Laden über den `CURRENT`-Digest erkannt.
- `test:manifest::tests::publication_syncs_generation_before_current_in_platform_order` – tatsächliche Windows-Veröffentlichung beobachtet die Sequenz Manifestdatei-Sync, Generationpublish, Manifestverzeichnis-Sync, `CURRENT`-Datei-Sync, Pointerersetzung, Datenbankverzeichnis-Sync.
- `test:manifest::tests::manifest_directory_sync_failure_does_not_publish_current` – injizierter Fehler vor `CURRENT`-Publikation lässt den Pointer unangetastet.
- `test:manifest::tests::current_pointer_is_exact_and_checksummed`, `test:manifest::tests::manifest_encoding_is_canonical_and_rejects_future_segments` und `test:manifest::tests::manifest_decoder_rejects_checksum_and_trailing_data` – exakte Längen, kanonische Reihenfolge, Versions-/Revisions- und Prüfsummenvalidierung.

## Lokale Windows-Prüfung

- `cargo test --locked -p worlddb-storage-file` – **PASS**, 39 Unit-/Integrationstests, 0 fehlgeschlagen; Rustdoc: 0 Tests.
- `cargo clippy --locked -p worlddb-storage-file --all-targets -- -D warnings` – **PASS**.
- `cargo fmt --all -- --check` – **PASS**.
- `git diff --check HEAD` – **PASS**; Git meldet nur die vorhandenen Windows-Zeilenendenhinweise.

Dieses Ergebnis belegt keine Stromausfall-/Hardwaredurabilität. Die Windows-Replace-/Write-through- und NTFS-Fehlerfallabnahme bleibt M5-09; Restart, Replay und Crashmatrix bleiben M5-11/M5-12/M5-22. Linux/ext4- und macOS/APFS-Tests wurden nicht ausgeführt.
