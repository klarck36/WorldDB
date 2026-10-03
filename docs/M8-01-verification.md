# M8-01 – Öffentliche Rust-Facade

**Ergebnis:** PASS auf Windows am 2026-10-03. Die stabile Anwendungsgrenze liegt unter `worlddb_core::api::v1`; für sie wurde kein neues Crate angelegt. Das folgt dem M0-09-Boundary-Gate, das `worlddb-api` zunächst als Core-Modul festlegt.

## Gelieferte Grenze

- `QueryContextDtoInput` verlangt alle semantischen Achsen. `QueryContextDto::new` prüft das Master-definierte Perspective-/EpistemicMode-Paar. Principal, Capabilities, SecurityContext, Dateipfade und CancellationToken sind keine Requestfelder; die Engine bindet Authentifizierung und Cancellation im vertrauenswürdigen Host.
- `QueryRequest` verbindet Operation, bounded Filter-AST, Projection, Sortierung und Paging. Filter- und Fulltextgruppen sind nichtleer, auf 32 Ebenen und 256 Blätter/Tokens begrenzt und kanonisieren ihre Kinder. Sortierung ist auf acht Terme begrenzt; Aggregat- und Gruppensortfelder werden gegen die Operation geprüft.
- Requests und Responses verwenden eigene `ProtocolVersion`, `RequestId` und geschlossene Envelopes. Handshake und Featureauswahl sind typisiert; Cancel nimmt nur eine Request-ID entgegen und meldet `CancellationSignalled` oder `AlreadyTerminal`.
- `QueryPageDto`, Raw-Record-, Explain- und Query-Ergebnisvarianten besitzen ihre Daten. Der Cursor bleibt opaque und seine `Debug`-Ausgabe redigiert Bytes; auch eine fehlerhafte Form wird zum Engine-Endpunkt durchgereicht, damit alle Cursorfehler gleich als `CursorInvalidated` enden.
- `PublicCode` schließt die 14 Codes aus `query_transport_contract.md` ein. Die Abbildung der bestehenden Core-Codes ist explizit und cause-frei; unbekannte Core-Codes werden als Mappingfehler gemeldet.
- Root-Exports bleiben für bestehende interne Workspaceadapter als Kompatibilitätsexporte erhalten. Sie sind nicht die unterstützte Anwendungs-API. `api::v1` exportiert weder Storage-Traits noch Cursorstore, Locks oder Sicherheitsidentitäten.

Die JSON- und IPC-Codierung sowie die konkrete Datei-Engine-Session werden in M8-09 und den späteren Query-/UI-Aufgaben an diese DTOs gebunden. Diese Abgrenzung hält die M8-01-Schnittstelle stabil, ohne eine weitere Crate-Grenze oder eine zweite Querysemantik einzuführen.

## Nachweise

- `crates/worlddb-core/src/api.rs`: versionierte Requests/Responses, EngineOperations, Owned DTOs, Filter-/Fulltext-AST, Cursor und Public-Code-Mapping.
- `crates/worlddb-core/src/lib.rs`: `api::v1` ist die dokumentierte stabile Facade. Rustdoc-`compile_fail`-Beispiele weisen die Verwendung interner `RevisionBackend`-/`CursorStateStore`-/`SecurityContext`-Typen und den Zugriff auf eine Principal-Identität über den QueryContext zurück.
- `api::v1::tests`: **7 PASS** für Protokoll-/Featureverhandlung, öffentliche Code-Tabelle und Projektion, AST-Kanonisierung und -Grenzen, ungültige Perspektivenpartition, typed IDs sowie redigierte Cursor.
- `cargo test --doc -p worlddb-core`: **87 PASS**, einschließlich aller drei neuen Facade-Grenznachweise.
- `cargo clippy -p worlddb-core --all-targets -- -D warnings`: **PASS**.
- `cargo xtask verify`: **38 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL**. Windows-Storage-/Crashvertrag: **86 PASS, 0 FAIL, 1 ignorierter manueller Langlauf**. Plancheck und Sourcecheck: **PASS**.

Linux-/macOS-Abnahmen bleiben wie vereinbart M9-07 zugeordnet. Öffentliche Rust-Datei: 1.991 Zeilen einschließlich Modul-Dokumentation und Tests.
