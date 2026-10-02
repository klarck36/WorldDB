# M6-07 – Atomarer Index-Rebuild

**Ergebnis:** PASS auf Windows. Geprüft am 2. Oktober 2026.

## Umsetzung

`crates/worlddb-storage-file/src/index_rebuild.rs` ergänzt den generischen Rebuild- und Speicherpfad für abgeleitete Indexfamilien:

- `IndexRebuildManager` hält einen unveränderlichen Snapshot-Pin vom vollständigen Snapshot-Aufbau bis zur Veröffentlichung. Während des Online-Aufbaus holt er geordnete Deltas auf. Ein fehlendes, doppeltes, falsch geordnetes oder zu spätes Revisionsdelta sowie ein überschrittenes Catch-up-Limit brechen den Vorgang ab, ohne den aktuellen Pointer anzutasten.
- Vor der Veröffentlichung wird die Quelle auf dieselbe Datenbankwurzel geprüft. Der Rebuilder erwirbt dann die Writer-Sperre, gleicht einen seit dem stabilen Online-Stand eingetroffenen Rest auf und prüft, dass sich der Head bis zur Veröffentlichung nicht mehr bewegt.
- `IndexGenerationStore` speichert jede Generation als unveränderliche, versionsgebundene Datei. Der separate Familienpointer bindet Familie, Generationsnummer und Dateidigest mit einer BLAKE3-Prüfsumme. Beim Lesen werden Pointer, Datei, Frame, Metadaten und Digest vollständig geprüft; unvollständige oder korrupte Payloads werden nicht zurückgegeben.
- Veröffentlichung folgt der Reihenfolge: Stagingdatei schreiben und synchronisieren, immutable Generation umbenennen, Indexverzeichnis synchronisieren, neuen Pointer schreiben und synchronisieren, Pointer atomar ersetzen, Verzeichnis erneut synchronisieren und die publizierte Generation zurücklesen. Auf Windows wird dafür der vorhandene NTFS-Veröffentlichungsadapter mit Write-through eingesetzt.
- Das `indexes`-Verzeichnis wird erst bei der ersten Indexveröffentlichung angelegt. Bestehende 1.0-Datenbanklayouts müssen dadurch nicht nachträglich ein zusätzliches Pflichtverzeichnis enthalten.

## Nachweise

- Fünf gezielte Rebuild-/Speichertests bestanden.
- Deltas, die während des Snapshot-Aufbaus und während der Delta-Anwendung eintreffen, werden lückenlos bis Revision 3 eingearbeitet; die Snapshot-Pin-Lebensdauer reicht bis nach dem Publizieren.
- Ein fehlendes Revisionsdelta lässt eine vorherige vollständige Generation unverändert aktiv. Beschädigte Pointer-Prüfsummen und beschädigte Generation-Frames werden beim Lesen abgewiesen.
- Der Windows-Prozesscrashtest beendet einen Kindprozess nach jeder der sechs Dateisystemgrenzen: Generationsdatei synchronisiert, Generation umbenannt, Generationsverzeichnis synchronisiert, Pointerdatei synchronisiert, Pointer ersetzt und Pointerverzeichnis synchronisiert. Nach jedem Neustart ist ausschließlich die komplette alte oder die komplette neue Generation lesbar.
- `cargo test --locked --workspace --quiet`: PASS; sämtliche aktiven Workspace- und Rustdoc-Suites bestanden. Drei separat markierte Langkampagnen blieben ignoriert und zählen nicht als bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python -X utf8 WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks und Abhängigkeiten gültig.
- `python -X utf8 WorldDB_1.0_Sourcecheck.py`: PASS; alle sechs Contract-Spiegel bytegleich.
- `git diff --check`: PASS.
- `cargo xtask verify`: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der `ci-matrix`-SKIP ist der von M0-14 zurückgestellte externe Lauf.

## Abgrenzung

Der Crashnachweis deckt Prozessabbrüche an den benannten Windows-Dateigrenzen ab und bestätigt die atomare Pointer-Sicht. Er ist keine Aussage über reale Stromausfälle oder Hardwarefehler. Linux-/macOS-Prüfungen bleiben wie vom Product Owner zurückgestellt offen; produktive Queryintegration folgt mit M6-08.
