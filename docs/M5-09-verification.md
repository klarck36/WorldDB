# M5-09 – Windows/NTFS-Adapter

**Ergebnis:** DONE für die lokale Windows-Abnahme. Die plattformspezifische Prüfung wurde auf dem vorhandenen Windows-/NTFS-Host ausgeführt. Linux/ext4 und macOS/APFS bleiben gemäß Anweisung zurückgestellt. Die Tests belegen den Adaptervertrag, aber keine vollständige Crash- oder Hardwaredurabilität.

## Implementierung

- Manifestgenerationen und `CURRENT` werden unter Windows über einen eng begrenzten Adapter mit `MoveFileExW` veröffentlicht. Alle Pfade werden zu absoluten UTF-16-Pfaden mit Extended-Length-Präfix normalisiert; UNC- und Extended-UNC-Pfade werden vor jedem Schreibzugriff abgewiesen.
- Jede Veröffentlichung fordert `MOVEFILE_WRITE_THROUGH` an. `MOVEFILE_REPLACE_EXISTING` wird ausschließlich beim Wechsel des veränderlichen `CURRENT`-Pointers verwendet; unveränderliche Manifestgenerationen werden ohne Ersetzung veröffentlicht. Beide Pfade stammen aus demselben Datenbank-Volume.
- Die einzelne FFI-Stelle ist durch `WDB-EXC-0002` freigegeben und mit SAFETY-, TEST- und REVIEW-Begründung versehen. Die verwendeten Flagkonstanten und die API-Signatur wurden mit der [Microsoft-Dokumentation zu MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw) abgeglichen.
- Das stabile Writer-Lockfile erhält explizite Windows-Sharingflags für Lesen, Schreiben und Löschen; die Sperre selbst bleibt exklusiv und wird über den Dateihandle-Lebenszyklus gehalten.
- Die crates.io-Abhängigkeit `windows-sys` ist exakt gepinnt, Windows-only eingebunden, in Dependency-/Feature-/Unsafe-Policies dokumentiert und im Crategraph-Allowlist geprüft.

## Windows-Vertragsprüfungen

- `windows_publication::tests::absolute_local_paths_use_the_extended_length_prefix` prüft Präfix und NUL-Terminierung lokaler Pfade.
- `windows_publication::tests::network_paths_fail_closed_before_publication` weist gewöhnliche UNC- und Extended-UNC-Pfade zurück.
- `a_nondeletable_current_handle_preserves_the_old_pointer_and_allows_retry` hält `CURRENT` mit einem Handle ohne Delete-Sharing offen. Der Replace-Aufruf muss scheitern, Generation 1 bleibt sichtbar, die nicht referenzierte Generation 2 bleibt als sichere Orphan-Datei erhalten und ein erneuter Publish nach Freigabe führt zu Generation 3.
- `a_current_directory_entry_is_rejected_without_following_it` weist einen Verzeichniseintrag anstelle des Pointerfiles fail-closed ab.
- Der Sharing-Konflikt ist ein deterministischer Windows-Testfall für eine mögliche Scanner-/Antivirus-Blockade; es wurde kein bestimmtes Antivirusprodukt ausgeführt oder zertifiziert.
- Die vorhandenen Writer-Lock-, Synchronisationsreihenfolge- und Faulttests bleiben aktiv. Insbesondere wird `CURRENT` bei Fehlern vor der Pointerersetzung nicht weitergeschaltet.

## Prüfergebnisse

- `cargo test --locked -p worlddb-storage-file` – **PASS**, 43 Unit-/Integrationstests, einschließlich der Windows-Pfad- und NTFS-Vertragsfälle; 0 fehlgeschlagen.
- `cargo clippy --locked -p worlddb-storage-file --all-targets -- -D warnings` – **PASS**.
- `cargo check --offline --workspace --all-targets` – **PASS**.
- `cargo test --locked --workspace` – **PASS**: 396 Core-, 1 Decoder-Inventar-, 5 Wire-Oracle-, 43 Storage-File-, 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests; insgesamt 543 bestanden, 0 fehlgeschlagen und 2 lang laufende Kampagnen ignoriert.
- `cargo xtask verify` – **PASS**, 32 Prüfschritte bestanden, 1 vorgesehener M0-14-`SKIP`, 0 Fehler; M0-13-Evidenzlauf `M0-13-20261001T143127Z-0130a2f9c6` mit Status PASS und festem Seed.

## Grenzen

Der Fehlerfalltest simuliert eine Sharing-Blockade mit einem offenen Windows-Dateihandle; er ersetzt keine Prüfung mit konkreter Antivirussoftware. Netzwerkpfade sind für Machine-durable Schreibzugriffe ausdrücklich nicht freigegeben. Die Ausführung belegt weder reale Stromausfall-/Datenträgercache-Durabilität noch die M5-22-Crashmatrix. Linux/ext4- und macOS/APFS-Tests wurden nicht ausgeführt.
