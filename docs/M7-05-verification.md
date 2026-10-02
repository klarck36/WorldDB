# M7-05 – Nichtbrechende Migration ausführen

**Status:** lokal abgeschlossen und unter Windows geprüft. Linux-/macOS-Nachweise bleiben gemäß Arbeitsplan bei M9-07.

## Gelieferter Vertrag

- Ein `MigrationStepTargetSchema` bindet jeden geordneten Plan-Step an genau eine Schemarevision und einen Fingerprint. Die Revisionen steigen ohne Lücken pro Commit; der letzte Step muss exakt dem Plan-Target entsprechen. Die optionalen Targets sind im Planfingerprint (Version 3) enthalten und werden über Wire-Feld 14 gespeichert. Alte Pläne ohne dieses optionale Feld behalten ihre bisherigen Fingerprints und Wire-Bytes.
- `execute_compatible_migration` akzeptiert ausschließlich MetadataOnly-, Additive- und CompatibleConstraintChange-Pläne. Vor dem ersten Commit prüft es Source-Head und Source-Fingerprint, Transformer-Version, Step-Reihenfolge, eindeutige OperationIds, alle Eingabeframes sowie gemeinsame Work- und Speichergrenzen. Die Speichergrenze umfasst Eingabeframes, temporäre Ergebnis- und Record-Slots, Receipts und die OperationId-Sammelstruktur.
- Jeder Step läuft durch denselben deterministischen Transformer wie Dry Run. Kanonische Ergebnisframes werden dekodiert und zusammen mit einer `MigrationStepCommitIdentity` als genau eine normale `OpenTransaction`-OCC-Transaktion veröffentlicht.
- Vor dem Commit erhält der erforderliche Validator Zugriff auf die vollständige Historie am gepinnten Basisstand, den gesamten gestagten Step und dessen fingerprintgebundenes Zielschema. Eine spätere Step-Prüfung sieht damit die bereits committed Vorsteps. Jeder positive Commit liefert Revision, Step-/Operationsidentität, Zielschema und Transformfingerprint als Receipt zurück.
- Scheitert ein späterer Step, enthält der Fehler exakt den bereits committed Prefix. Frühere Commits bleiben bestehen; der Executor führt keinen Rollback committed History aus.
- Transformer-Version 1 erhält kanonische Recordframes bytegenau. Die separate WorldTime-Kalenderoperation bleibt an Timeline, Epoch, Periode, Richtung und Source-Precondition eines expliziten Plans gebunden. Die Execution-API liest oder überschreibt bestehende Records nicht, sondern committed nur die explizit für den Step übergebenen Records.

Der domain-spezifische Validator bleibt ein erforderlicher Aufrufer-Callback: Er muss die komplette Engine- und Zwischen-Schema-Semantik gegen die bereitgestellte Historie und das Ziel prüfen. Der Executor erzwingt Commitreihenfolge, Planbindung, OCC und atomare Veröffentlichung, ersetzt aber keine projektspezifische Schema-Validierung.

## Nachweise

- Acht `migration_execution`-Tests decken getrennte OCC-Commits, historische Lesbarkeit, konkrete Zwischen-Schema-Fingerprints, Ablehnung eines später ungültigen Zwischenstands ohne Rollback, Restrictive-Gate, vorgezogene Gesamt-Work-/Speicheradmission, ungültige spätere Frames und doppelte OperationIds ab.
- `migration::tests::migration_step_targets_bind_every_contiguous_intermediate_schema` prüft Schrittzuordnung, lückenlose Revisionen, finales Target und Fingerprintbindung.
- Die neuen Golden-/Decoderprüfungen validieren Wire-Feld 14; die unabhängige Wire-Orakelprüfung umfasst jetzt 50 feste Frames.
- `cargo test --locked --workspace`: **PASS** (489 Core-Tests und 84 Rustdoc-Tests; übrige Workspace-Suites PASS; vorhandene manuelle Langläufe bleiben ignoriert).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo fmt --all -- --check`, Workspace-Check, Plancheck, Sourcecheck und `git diff --check HEAD`: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**.

## Bewusst spätere Arbeit

Restrictive-/Breaking-Ausführung und deren validierte Adminentscheidungen bleiben M7-10a. Recoverbares Run-Journal, Resume, Unknown-Outcome-Auflösung und fachliche Compensating Migration bleiben M7-06. Diese Implementierung bietet den generischen `RevisionBackend<Record>`-Ausführungspfad und beansprucht keinen Crash-Resume- oder Run-Journal-Nachweis. Linux-/macOS-Prüfungen folgen bei M9-07.
