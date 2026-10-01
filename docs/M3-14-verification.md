# M3-14 – Panic-, FFI- und IPC-Grenzen

**Status:** lokal umgesetzt und am 2026-09-30 verifiziert.

## Fehlerabbildung

- `observe_task_join` wandelt einen beobachteten Worker-Unwind in `TaskFailure::Panicked` um. Der Panic-Payload wird verworfen und weder angezeigt noch als Recovery-Entscheidung verwendet.
- `TaskFailure::terminal_state(TaskRole::Background)` ergibt `Failed`; `TaskRole::Writer` ergibt `NeedsRestart`. Der Writer darf nach einer unterbrochenen Mutation nicht im selben Enginezustand fortfahren. Recovery und `OperationId`-Abgleich entscheiden anschließend den tatsächlichen Commitstatus.
- IPC-/Wire- und Rendererfehler bleiben Transport-/Requestfehler. Renderer- oder App-Abbruch behauptet weder Erfolg noch Fehlschlag eines nicht beantworteten Commits; nach Wiederverbindung wird der Status über die vorhandene `OperationId` aufgelöst. Öffentliche Fehler DTOs enthalten keine Panic-Payloads, Ursachen oder Rust-Typnamen.

## Boundary-Enforcement

- Workspace-Lints verbieten `panic`, `unwrap`, `expect`, ungeprüfte Indexierung und `unsafe` in allen Crates. Der Ausnahmeprüfer verlangt für jedes lokale `allow` eine registrierte Begründung.
- Der Core besitzt derzeit keine produktive FFI-Exportfunktion. `unsafe_code = deny` blockiert unfreigegebene ABI-Aufrufe; sobald eine FFI-Grenze hinzukommt, muss sie separat mit einer `catch_unwind`-Grenze getestet werden, die Panics nur vom ABI fernhält und sie nicht als normale Errors behandelt.
- Die vorhandenen Rust- und TypeScript-Decoder begrenzen Eingaben und liefern typisierte Fehler für malformed/noncanonical Daten. Es wird keine FFI- oder Renderer-Runtime vorgetäuscht, die im Workspace noch nicht existiert.

## Nachweise

- `jobs::tests::joined_panics_are_payload_free_and_writer_panics_require_restart`: kontrolliert Task-Panic-Mapping, sichere feste Darstellung und Writer-`NeedsRestart`.
- `wire::tests::decoder_limits_reject_large_frames_and_owned_values_before_copy` sowie die Record-/Audit-Decoder-Malformed-Tests: kontrollierte Decoderfehler für ungültige oder übergroße Bytes.
- `bindings/typescript/test/transport.test.ts`: kontrollierte Errors bei ungültigem JSON, doppelten Schlüsseln, falschen Versionen, nichtkanonischen Skalaren und überschrittenen Envelopebudgets.
- Statische Prüfer: Workspace-Lints, Exception-Policy, Dependency-Policy, Formatcheck und `cargo clippy`.

## Grenzen

Die konkrete Tauri-/Renderer-/IPC-Prozessboundary und eine native FFI-ABI existieren noch nicht in diesem Workspace. Der Vertrag für Reconnect/OperationId und die typisierte Task-Grenze sind definiert; deren reale Prozessintegration wird mit der Desktop-/Koordinatorimplementierung nachgewiesen.

## Validierung

- `cargo test --locked --workspace`: 299 Core-Tests und 75 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- Statische Workspace-, Ausnahme- und Dependency-Policy-Prüfer: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS (mit den vorhandenen LF/CRLF-Hinweisen von Git).
