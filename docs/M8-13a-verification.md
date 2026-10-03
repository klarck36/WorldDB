# M8-13a – Rechteverwaltung: Windows-Nachweis

**Ergebnis:** PASS auf Windows
**Plattform-Folgeprüfung:** Linux/macOS bleibt bis M9-07 zurückgestellt
**Vertrag:** `docs/architecture/M8-13a-security-policy-management-contract.md`
**Normative Grundlage:** `docs/contract/ADR-033-security-policy.md`, `docs/contract/ADR-043-project-host-binding.md`

## Gelieferte Funktionen

- Authentisierte Policyansicht und Änderungsbefehle für bekannte Principals, Role-Zuweisungen sowie explizite Principal-/Role-Capability-Regeln.
- GM-/Player-Bundles, Zuweisungen und zusätzliche Allow-/Deny-Regeln erscheinen als getrennte Inhalte. Perspectives bleiben davon getrennt.
- Die UI sendet keinen Akteur und keine Capability-Menge. Der Enginehost leitet den Akteur aus der OS-gebundenen Sitzung ab; Storage autorisiert erneut gegen die aktuelle Policy.
- Jede bestätigte Änderung publiziert Policyrecord, neuen SecurityEpoch, Shared Revision und Required Audit atomar. Optimistic revision conflicts und fehlende Berechtigung fail closed.
- Selbstentzug wirkt am gleichen Commitpunkt. Ein aktualisierter SecurityEpoch invalidiert Cursorfortsetzungen über den bestehenden Reautorisierungspfad.
- GM behält das gewöhnliche `RawHistoryRead`, erhält aber kein implizites `AdminRawRead`.

## Gezielte positive und negative Nachweise

- `schema_management::security_policy::tests::policy_rule_publication_advances_epoch_and_required_audit_atomically` – Policyregel, SecurityEpoch und Required Audit werden dauerhaft gemeinsam veröffentlicht.
- `schema_management::security_policy::tests::policy_read_and_management_capabilities_are_independent` – fehlendes `SecurityPolicyManage` verhindert eine Änderung ohne Revision-/Epoch-Fortschritt; fehlendes `SecurityPolicyRead` sperrt den Snapshot separat.
- `schema_management::security_policy::tests::self_revocation_commits_and_takes_effect_at_the_same_revision` – erlaubter Selbstentzug wird im selben Commit wirksam.
- `query_engine::tests::changed_security_epoch_invalidates_and_releases_a_page_cursor` – ein Rechtewechsel entwertet eine bereits begonnene Cursorfortsetzung und gibt die Seite frei.
- `project::tests::creator_policy_is_role_based_and_does_not_grant_raw_admin` – negativer Nachweis: der initiale GM erhält kein `AdminRawRead`; dieses Recht wird nicht aus dem Rollennamen abgeleitet.
- Native Zwei-Fenster-Smokes weisen Zuweisung, Widerruf, explizites GM-`AdminRawRead`-Deny, getrennte Rohrechte, aktualisierte SecurityEpochs und gemeinsame Policyansicht nach.
- Policyzugriffe ohne die erforderliche aktuelle Les-/Verwaltungsberechtigung werden in den Storage-Policytests zurückgewiesen; eine abgewiesene Änderung veröffentlicht keinen Policyzustand.

## Ausgeführte Prüfungen

| Prüfung | Ergebnis |
| --- | --- |
| `cargo test --locked --offline -p worlddb-storage-file schema_management::security_policy::tests` | PASS; 3 gezielte Policy-Verwaltungstests |
| `cargo test --locked --offline -p worlddb-core changed_security_epoch_invalidates_and_releases_a_page_cursor` | PASS; 1 gezielter Cursor-Nachweis |
| `cargo test --locked --offline -p worlddb-ode-engine creator_policy_is_role_based_and_does_not_grant_raw_admin` | PASS; 1 GM-Rohrechtetrennungstest |
| Root `cargo test --locked --offline --workspace --all-targets` | PASS |
| Root `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | PASS |
| ODE-002 `cargo test --locked --offline --workspace --all-targets` | PASS; 15 Desktop-, 20 Engine- und 1 Sidecar-Transporttest |
| ODE-002 `cargo test --locked --offline --workspace --all-targets --no-default-features --features sidecar` | PASS; 16 Desktop-, 20 Engine- und 1 Sidecar-Transporttest |
| ODE-002 strict Clippy, in-process und sidecar | PASS |
| `cargo xtask verify` | 39 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL |
| `run-ipc-security-smoke.ps1 -Mode in-process` | PASS; beide authentisierten Fenster und Rechteänderungen |
| `run-ipc-security-smoke.ps1 -Mode sidecar` | PASS; beide authentisierten Fenster und Rechteänderungen |
| `cargo fmt --all -- --check`, Node-Syntax, PowerShell-Parser, Plan-/Sourcecheck, `git diff --check HEAD` | PASS |

Die UI-Smokes ordnen asynchrone Fensteraktualisierungen nach dem jeweils veröffentlichten SecurityEpoch. Dadurch werden Zwischenstände revisionsgebunden ausgewertet und zusätzliche zulässige Refreshes bleiben sichtbar.

## Nicht-Windows-Prüfungen

Linux-/macOS-Hostintegration und native IPC-Prüfungen werden entsprechend der Projektvorgabe erst in M9-07 durchgeführt.
