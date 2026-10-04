# M8-14c – Assertion-, Mask- und Boundary-Erfassung

Stand: 4. Oktober 2026

## Ergebnis

`FileFactManager` stellt typisierte Schreibmethoden für Assertions, Masks und ReplacementBoundaries bereit. Jeder Aufruf bindet die Eingabe an den aktuellen gemeinsamen Datenstand und prüft vor dem Publizieren die aktuelle Policy, die Schemafamilie, die Entity- und Predicate-Referenzen, den konkreten HistorySpace-/Layer-/Perspective-Kontext sowie erforderliche Feld- und Referenzberechtigungen.

Assertions durchlaufen die vorhandene Assertion-Schema- und Referenzvalidierung. Proposition-Masks prüfen Predicate-Werttyp und Constraints ohne eine Assertion-Erstellung vorzutäuschen; Slot-Masks prüfen Subject, Predicate und die passende epistemische Partition. ExactAssertion-Masks verlangen ein vorhandenes und für den Principal lesbares Ziel. ReplacementBoundaries akzeptieren nur aktive Predicates mit `MultiValueReplace`. Optionale Weltzeit-Gültigkeiten erfordern eine registrierte Timeline, deren Lifecycle für den jeweiligen Schreibpfad zulässig ist.

Jeder angenommene Record ist append-only und erhält die nächste gemeinsame Revision. Historysegment, unveränderte Policy-Version, Manifest, WAL-Commitmarker und Required Audit werden mit derselben `OperationId` atomar publiziert. Neue Auditcodes sind in Core-Codec und `policy/audit-wire-values.tsv` geschlossen registriert: `FactualRecordWrite` = 14 und `FactualRecord` = 11. Abgelehnte Eingaben erzeugen weder Revision noch Audit.

`snapshot_at` rekonstruiert Assertions, Masks und ReplacementBoundaries für eine veröffentlichte Revision und gibt Records nur dann zurück, wenn der Principal die jeweilige Familien-Leseberechtigung im Record-Kontext besitzt.

## Nachweise

- `schema_management::facts::tests::assertion_mask_and_boundary_commit_with_required_audit_and_survive_reopen`: alle drei Familien werden geschrieben, jedem Commit wird sein Required Audit zugeordnet, und alle drei Records überstehen Manager-Reopen und StorageVerify.
- `schema_management::facts::tests::invalid_and_unauthorized_assertions_leave_revision_and_audit_unchanged`: falscher Predicate-Werttyp und fehlendes `AssertionCreate` werden fail-closed abgewiesen; Revision und Auditanzahl bleiben unverändert.
- `audit_wire::tests::audit_enums_have_closed_stable_codes`: neue Auditcodes lassen sich eindeutig encodieren und decodieren.
- `audit_wire::tests::audit_enum_codes_match_the_separate_policy_registry`: Policy-Registry und Rust-Codec stimmen überein.
- Striktes Clippy für `worlddb-core` und `worlddb-storage-file` mit `--all-targets -- -D warnings` bestanden.
- `cargo xtask verify`: 39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.
- Storage-Datei-Tests einschließlich beider neuer `facts`-Tests: PASS; striktes Clippy für `worlddb-core` und `worlddb-storage-file`: PASS.
- Plancheck, Sourcecheck und `git diff --check HEAD`: PASS.

Linux- und macOS-Nachweise bleiben vereinbarungsgemäß bis M9-07 zurückgestellt.
