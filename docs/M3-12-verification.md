# M3-12 – Diagnostik-Port

**Status:** DONE, lokale Core-Port-Nachweise verifiziert 2026-09-30.

## Implementierung

- `diagnostics.rs` stellt unveränderliche, reine `DiagnosticEvent`s mit geschlossenen Eventtypen und stabilen Span-Namen bereit.
- Diagnosefelder sind auf ein geschlossenes Feld- und Wertvokabular begrenzt. Unklassifizierte Felder sind `Omitted`; `Shown` akzeptiert nur sichere skalare Typen; `Hashed` nur einen festen 16-Byte-Digest. Beliebige Strings, Querytexte, Pfade und Principalnamen können nicht als Diagnosewert übergeben werden.
- Events sind auf 16 eindeutige Felder begrenzt; die Queue ist auf 4096 Events begrenzt.
- `DiagnosticPort::report` liefert keinen Fehler an den Domainaufrufer zurück, nutzt `try_lock` und verwirft Queue-volle, gesperrte oder vergiftete Zustände fail-open. Jeder Drop erhöht saturierend `dropped_telemetry`.
- Das geschlossene Counter-Set enthält `conflicts`, `unknown_commit_outcomes`, `recovery_actions`, `corrupt_frames`, `snapshot_pins`, `dropped_telemetry` und `security_denials`; alle Zähler sättigen statt zu überlaufen.
- Der Port ist synchron und erzeugt keine Span-Guards. Er implementiert keine Auditpersistenz; Audit bleibt als eigener Port/Commitvertrag getrennt.

## Nachweise

- `diagnostics::tests::event_names_are_stable_and_unknown_fields_default_to_omitted`: stabiler Spanname und Omitted-Default.
- `diagnostics::tests::shown_and_hashed_values_are_explicit_and_events_are_bounded`: Opt-in-Redaktion, Feldlimit und Duplikatabwehr.
- `diagnostics::tests::queue_is_bounded_nonblocking_and_counts_drops_without_vetoing_work`: Null-/Überkapazität wird abgewiesen; volle Queue zählt den Drop, während die simulierte Domainoperation erfolgreich bleibt; die übrigen sicheren Counter funktionieren.
- `diagnostics::tests::lock_contention_drops_immediately_and_saturates_counters`: Producer warten nicht auf die Queue und Drop-Counter laufen nicht über.
- Rustdoc compile-fail auf `DiagnosticField`: beliebiger String kann nicht als `Shown`-Diagnosewert gesetzt werden.
- Rustdoc compile-fail auf `DiagnosticField`: ein nicht vorhandener RAII-Span-Guard kann nicht importiert oder über den Core-Diagnoseport gehalten werden.

## Validierung

- `cargo test --locked --workspace`: 290 Core-Tests und 75 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS (mit LF/CRLF-Konvertierungswarnungen von Git).

## Grenzen

Dies ist ein bounded In-Memory-Core-Port, kein Exporter. Dateirotation, Flush-/Shutdown-Deadline und exporter retry policy bleiben Adapter-/Infrastrukturaufgaben. Es gibt keinen Constant-Time-Anspruch.
