# M0-13 – Testkit-Grundlage

**Status:** DONE

**Geprüft:** 2026-09-29

**Implementierungscommit:** `e538756` (`feat: build WorldDB testkit foundation`)

**Implementierungsumfang:** `worlddb-testkit`, versioniertes Testdatenkorpus, Evidenzschema und Verify-Runner

## Ergebnis

`worlddb-testkit` bietet deterministische `u64`-Seeds und einen Dependency-freien SplitMix64-Stream. `WORLDDB_TEST_SEED` akzeptiert Dezimal- und Hexwerte; ohne Vorgabe ist der Seed `0x574f524c44444231`. Corpusversion `m0-13-v1` enthält acht Fuzz-Seeds, vier Goldenvektoren und 16 synthetische Full-Scan-Datensätze. SHA-256, Dateipfade, Seed-Eindeutigkeit und Fixtureformat werden geprüft. Die synthetischen Zeilen enthalten keine vorweggenommenen Produktdomänenregeln.

Das optionale Cargo-Feature `fault-injection` enthält einen einmaligen, seedgebundenen Hook und den `fault-probe`. Beide sind nicht Teil des Standardprofils. Ein Releasebuild mit aktiviertem Feature endet am Compile-Time-Guard; ein Featuretree-, rlib-Marker- und Binärdateicheck untersucht zusätzlich das Standard-Releaseartefakt.

## Verifikation

- `cargo xtask verify` auf Windows x86_64 MSVC, NTFS, Rust/Cargo 1.85.0: **24 Schritte bestanden, 1 sichtbarer M0-14-Skip, 0 Fehler**.
- Der M0-13-Evidenzlauf besteht mit **10 Einzelprüfungen** und schreibt Run-ID, PASS/FAIL, Umgebung, Seed, Commit, Fixture-/Loghashes und Quell-Snapshot in das versionierte JSON-Format.
- Die Rust-Tests des Testkits bestehen mit 7 Tests im Standardprofil; der explizite `fault-injection`-Profiltest besteht zusätzlich.
- Vier Python-Corpustests bestehen. Veränderte Hashes, nicht registrierte Pfade und doppelte Seeds werden abgewiesen.
- Der Releasebuild besteht ohne Fault-Hooks. `fault-probe` fehlt im Releaseartefakt und der Hook-Marker ist nicht enthalten. `--release --features fault-injection` scheitert wie vorgesehen am Compile-Time-Guard.
- Der kontrollierte Seed-Replay endet zweimal mit Exitcode 73, protokolliert denselben Seed und liefert byteidentische Ausgaben.
- Das Evidenzmanifest löst die Fixture-, Log-, Quell-Snapshot- und Manifestpfade auf und prüft ihre SHA-256-Werte. Es enthält Commit, Arbeitsbaumzustand, Betriebssystem, Dateisystem, Hardware, Toolchain und Seed. Der ZIP-Quellsnapshot ermöglicht die Replay-Prüfung auch bei einem Dirty-Arbeitsbaum.
- Sourcecheck, Plancheck, Workspaceformatierung, gelockter Workspacecheck, Clippy, cargo-deny, die drei Featureprofile und `git diff --check` bestanden.

## Laufbeleg

Der finale, reproduzierbare Beleg wird beim abgeschlossenen Lauf mit fester Run-ID `M0-13-20260929-final` unter `C:\Users\wedde\AppData\Local\WorldDB\test-runs\M0-13-final\M0-13-20260929-final\evidence.json` abgelegt. stdout/stderr, Fixturehashes, Toolchaindaten und ein Quell-Snapshot liegen im selben externen Runverzeichnis. Der konkrete Hostpfad ist zusätzlich im Taskregister verzeichnet.

## Grenzen

Der Lauf wurde nativ nur auf Windows/NTFS geprüft. Native Linux-/macOS-Jobs gehören zu M0-14. Die Fuzz-Seeds sind ein kleines versioniertes Startkorpus; lange Fuzzläufe und Crash-/Core-Dumps verbleiben außerhalb des synchronisierten Repositorys. Der M0-02a-Blocker mit 52 offenen HARD-Quellenlücken und zwei GUARDED-Lücken bleibt unverändert und verhindert weiterhin das M0-15-Gate.

## Reproduktion

```text
cargo xtask verify
```

Für den absichtlichen Replayfehler steht das im JSON-Beleg protokollierte Cargo-Kommando bereit. Zum Reproduzieren eines konkreten Laufs zuerst `source-snapshot.zip` aus dessen Runverzeichnis entpacken, dann den protokollierten Befehl mit demselben `WORLDDB_TEST_SEED` ausführen.
