# M0-13 – Deterministisches Testkit und Evidenz

## Seed- und Fixtureformat

`worlddb-testkit` stellt ohne externe Laufzeitdependencies `Seed` und einen stabilen SplitMix64-Strom bereit. `WORLDDB_TEST_SEED` akzeptiert Dezimalwerte und `0x`-Hexwerte im `u64`-Bereich; die Ausgabe normalisiert Seeds auf 16 Hexziffern. Ohne Umgebungswert gilt `0x574f524c44444231`. Der Generator ist Testinfrastruktur und keine kryptografische oder Produkt-ID-Zufallsquelle.

Die Corpusversion `m0-13-v1` enthält acht reproduzierbare Fuzz-Seeds, vier versionierte SplitMix64-Goldenvektoren und 16 synthetische Datensätze für einen vollständigen Scan. Die Datensätze bestehen nur aus Sequenznummer und opakem Payload. Sie definieren kein WorldDB-Domänenmodell. `manifest.json` bindet die drei Dateien an SHA-256; der Corpuscheck prüft Pfade, Version, IDs, Werte und Hashes vor der Ausführung. Neue oder veränderte Testdaten erhalten eine neue Version oder einen gezielten Manifestwechsel mit Review.

## Fault-Hooks und Replay

Fault-Hooks existieren ausschließlich hinter dem nicht standardmäßigen Cargo-Feature `fault-injection`. Das Feature ist in Releaseprofilen durch `compile_error!` gesperrt. Das reine Releaseartefakt wird zusätzlich auf den Fault-Marker und den Probe-Binärdateinamen untersucht. Der M0-13-Replay-Probe löst mit demselben expliziten Seed zweimal denselben absichtlichen Exitcode 73 und dieselbe Ausgabe aus. Er ist ein Test des Reproduktionspfads, keine Produktfehlerbehandlung.

## Laufbelege und Ablage

`docs/schemas/test-evidence.schema.json` versioniert das JSON-Format. `tools/run_m013_evidence.py` führt Corpus-, Python- und Rusttests, einen Releasebuild, die Feature-/Artefaktprüfung, die erwartete Releaseablehnung und den Seed-Replay aus. Es speichert stdout/stderr je Prüfschritt und `evidence.json` in einem eindeutigen Verzeichnis unter `%LOCALAPPDATA%\WorldDB\test-runs\M0-13` (mit benutzerspezifischem Fallback außerhalb des Repositories). Das Manifest erfasst Run-ID, PASS/FAIL, auflösbare Artefakte und SHA-256, Commit/Dirty-Status, OS-Version, Dateisystem, Architektur/Hardware, Rust/Cargo/Hosttarget, Seed und Replaybefehl. Zusätzlich enthält jeder Lauf ein ZIP mit allen getrackten und nicht ignorierten Quelltextdateien der exakten Arbeitskopie; sein SHA-256 bindet auch bei einem Dirty-Lauf den Reproduktionsstand. Die Manifestvalidierung prüft Dateiexistenz und Prüfsummen.

Große generierte Fuzz-Läufe und Crash-/Core-Dumps gehören nicht in das synchronisierte Repository. Für einen späteren Produkt-Fuzzer müssen Seed, minimierter Eingabefall, Reproduktionskommando, erwartete Invariante und Build-/Featureprofil im selben Beleg erhalten bleiben; Produktinvarianten werden nicht durch dieses generische Testkit vorweggenommen.
