# M3-15 – API- und Security-Vertragstests

**Status:** lokal verifiziert am 2026-09-30.

## Kombinierte Resolution-Matrix

`single_value_resolution::tests::combined_resolution_security_truth_table_has_no_hidden_contributors` führt 96 deterministische Fälle auf den indexfreien Referenzfunktionen aus:

- alle drei `ResolutionPolicy`-Werte;
- Maske aktiv/inaktiv;
- gleiche Priorität gegen lokale höhere Priorität;
- gleiche gegen widersprüchliche Polarität;
- ReplacementBoundary aktiv/inaktiv;
- beide Kandidaten sichtbar gegen einen sicherheitsgefilterten Kandidaten.

Jeder Fall vergleicht die ausgegebenen Contributor-IDs mit der unabhängig erwarteten Menge und prüft, ob ein Konflikt genau dann sichtbar ist, wenn die sichtbaren Kandidaten ihn erzeugen. Verborgene, maskierte und unter einer Boundary liegende Records dürfen nicht in Result oder Conflict Contributors erscheinen.

## API-, Fehler- und Security-Verträge

- `errors::tests::error_codes_are_stable_and_domain_specific` fixiert öffentliche Fehlercodes.
- `errors::tests::public_error_formatting_is_deterministic_and_has_no_side_effects` und `errors::tests::public_error_views_hide_secret_causes_and_preserve_typed_sources` prüfen deterministische Formatierung, typed interne Ursachen und Secret-Redaction.
- Cursor-Tests prüfen opake Handles, Manipulation, gleichförmige Invalidierung, Ablauf/Neustart, Bounded State sowie Re-Autorisierung anhand Principal, Capabilities und SecurityEpoch.
- Search, Graph und Aggregate prüfen Budget- und Cancellationfehler ohne Teilergebnisse sowie Filterung verborgener Kandidaten vor Zählungen und Workbudgets.
- `non_interference::tests::paired_world_compares_values_shapes_public_errors_and_cursor_behavior` vergleicht Paarwelten auf Werte, Shapes, öffentliche Fehler und Cursorverhalten.

## Validierung

- `cargo test --locked --workspace`: 300 Core-Tests und 75 Rustdoc-Tests PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-SKIP, 0 FAIL.
- `python -B WorldDB_1.0_Plancheck.py`: PASS.
- `git diff --check HEAD`: PASS (mit vorhandenen LF/CRLF-Konvertierungshinweisen).

## Abgrenzung

M3-Nachweise gelten für die indexfreien Core-Referenzpfade. Optimierte Indizes und produktive Streampfade werden gemäß Plan in M6 erneut gegen dieselben Verträge geprüft.
