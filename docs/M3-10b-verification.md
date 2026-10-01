# M3-10b – Cursorbindung und Re-Autorisierung

**Status:** DONE
**Geprüft:** 2026-09-30T19:45:21+02:00

## Ergebnis

Die autorisierten Cursorpfade speichern Principal, Policyziel, Fingerprint der effektiven Capabilities sowie aktuelle und query-evaluierte `SecurityEpoch`. `CursorSecurityContext` muss exakt mit der für dieselbe Principal selektierten `SecurityPolicyView` übereinstimmen. Der Wirecursor bleibt dabei unverändert opaque.

Vor jeder autorisierten Fortsetzung werden die vollständigen 77 Capabilities erneut gegen `SecurityPolicyView.current_snapshot()` ausgewertet und ihr Fingerprint neu gebildet. Damit wird `AuthorizationNow` auch bei einer Query geprüft, deren Daten-/Policyauswertung auf einer historischen Revision liegt. Eine abweichende Capabilitymenge, Principal, Policyziel, aktuelle oder historische Epoch führt zur gleichen `CursorInvalidated`-Antwort wie unbekannte, abgelaufene oder manipulierte Cursor.

Die unautorisierte Einfüge- und Auflösungsroutine ist nicht Teil der öffentlichen API. `resolve_authorized` ist der einzige öffentliche Fortsetzungspfad; damit kann ein autorisierter Cursor nicht über die M3-10a-Routine ohne Reautorisierung fortgesetzt werden.

## Nachweise

- `cursor::tests::authorized_cursor_rechecks_principal_capabilities_and_epochs`: erfolgreiche Fortsetzung mit aktueller Policy; Ablehnung bei Capabilityänderung (auch ohne vorgetäuschten Epochwechsel), Rechteentzug, Principalabweichung und aktueller Epochabweichung.
- `cursor::tests::historical_query_cursor_pins_evaluated_epoch_and_rechecks_now`: historische Auswertungs-Epoch bleibt gebunden, aktuelle Policy wird dennoch erneut geprüft; Wechsel der Security-Zeitbasis wird abgewiesen.
- `cursor::tests::wire_handle_hides_payload_and_tampering_has_uniform_invalidation` und `cursor::tests::expiry_unknown_and_restart_share_cursor_invalidated`: manipulierte, unbekannte, abgelaufene und sessionfremde Token verwenden die identische Invalidation.
- `cargo test --locked --workspace`: 276 Core-Tests und 73 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL; Plancheck und `git diff --check HEAD` enthalten.

## Umfangsgrenze

Die produktive Pagination-/Streaming-Anbindung mit Backpressure und Pinfreigabe bleibt Aufgabe M6-12. Das Non-Interference-Paarweltkit und die späteren Langzeitnachweise bleiben in M3-06 bzw. M8-26d registriert.
