# M0-11 – Workspace-Lint-, Unsafe- und Ausnahme-Policy

## Formatierung und Lints

`rustfmt.toml` bindet rustfmt an Edition 2024 und die festgelegte Zeilenbreite. Jedes Workspacemitglied erbt seine Regeln explizit über `[lints] workspace = true`. Rust `unsafe_code` und die Clippy-Gruppen `correctness`, `suspicious`, `panic`, `unwrap_used`, `expect_used` und `indexing_slicing` stehen auf `deny`. Der kanonische Dev-Verify führt Clippy mit `-D warnings` aus; diese Warnungsbehandlung gilt dadurch für Verify/CI und wird nicht als veröffentlichte Library-Eigenschaft festgeschrieben.

## Unsafe-Grenze

Alle Workspacecrates erben das Rust-Lint `unsafe_code = deny`. `worlddb-core` verschärft das auf Crate-Ebene zu `#![forbid(unsafe_code)]`. `worlddb-storage-file` und `worlddb-process-adapter` sind die genehmigten Plattformadapter und setzen selbst `#![deny(unsafe_code)]`; ihre eng begrenzten, geprüften Ausnahmen müssen lokal registriert sein.

`tools/check_unsafe_policy.py` weist Unsafe außerhalb dieser beiden Adapter ab. Jeder Unsafe-Block braucht im lokalen Kontext ein `#[allow(unsafe_code, reason = "WDB-EXC-NNNN")]`-Attribut sowie `SAFETY:`, `TEST:` und `REVIEW:`-Belege. Rustc/Clippy erzwingen die Codegrenze; der Check prüft die zusätzlichen Reviewangaben. WDB-EXC-0002 bindet die Windows-Dateiveröffentlichung. WDB-EXC-0005 bindet Job-, Prozesszuweisungs- und Resume-Aufrufe des Prozessadapters. WDB-EXC-0006 bindet das begrenzte Lesen der Benutzer-SID aus dem Primärtoken des aktuellen Prozesses; nicht unterstützte Plattformen bleiben fail-closed.

## Ausnahme-Register

`policy/exceptions.tsv` verwendet die Spalten `exception_id`, `owner`, `reason` und `expires_on`. IDs haben das Format `WDB-EXC-NNNN`, Ablaufdaten `YYYY-MM-DD`. Jede lokale `#[allow]`-Annotation muss genau eine registrierte ID nennen; abgelaufene, ungenutzte oder unvollständige Einträge lassen Verify fehlschlagen. Das Register führt alle aktiven Ausnahmen einschließlich WDB-EXC-0005.

## Fachterminologie

Der Terminologie-Check folgt Master §34.3. Er klassifiziert jeden Treffer auf `BranchId`, `SchemaVersionId` und typgelöschtes `LifecycleRecordId` als ADR-Entscheidung, historische Erklärung, explizite Negativregel oder Compile-Fail-Test. Nicht klassifizierte Treffer sind ein Fehler. Die aktuellen 72 Treffer in Code- und Vertragsquellen sind vollständig als ADR (8), Negativregel (28) oder historische Erklärung (36) klassifiziert.

## Durchsetzung

Das Dev-Profil in `tools/verify/steps.tsv` führt Format-, Workspace-, Clippy-, Unsafe-, Ausnahme- und Terminologiechecks aus. Negative Prüffälle belegen, dass unzulässiger Unsafe-Code, eine normative Alt-ID, eine abgelaufene Ausnahme und fehlende Unsafe-Nachweise scheitern. Der Prozessadapter bleibt unter `deny(unsafe_code)`; nur die einzeln markierten Job-API-Aufrufe verwenden WDB-EXC-0005. Future dependency- und CI-Schritte bleiben sichtbar als M0-12- bzw. M0-14-SKIPs registriert.
