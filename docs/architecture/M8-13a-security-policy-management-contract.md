# M8-13a – Vertrag zur Rechteverwaltung

**Status:** Windows-Implementierung angenommen; Linux/macOS-Nachweise bleiben M9-07 zugeordnet
**Voraussetzungen:** ADR-033, ADR-043 und die bestehenden Security-, Audit- und Cursorverträge

## Zweck und Trennung

Die Desktopansicht administriert authentisierte WorldDB-Principals, Role-Zuweisungen und explizite Capability-Regeln. Sie bleibt von der Perspektivenansicht getrennt: Eine Perspective ist weder ein Benutzerkonto noch eine Rolle oder ein Principal. Der Renderer liefert weder den handelnden Principal noch eine Capability-Menge.

Die Host-Sitzung bestimmt den Akteur nach ADR-043. Lesen der Policy erfordert `SecurityPolicyRead`; jede Änderung erfordert die aktuelle `SecurityPolicyManage`-Berechtigung. Der Storage-Manager prüft die Berechtigung erneut auf dem aktuellen Policystand.

## Unterstützte Änderungen

- Den Zustand eines bereits registrierten Principals ändern. Ein stillgelegter Principal kann nicht reaktiviert werden.
- Eine Rolle mit leerem eigenständigem Capability-Bundle registrieren.
- Einer aktiven Person eine bekannte Rolle auf Projektebene zuweisen oder eine konkrete Zuweisung widerrufen.
- Eine explizite Projektregel `Allow` oder `Deny` für einen bekannten Principal oder eine bekannte Rolle hinzufügen oder widerrufen.
- Rollen-Bundle und zusätzliche explizite Regeln als getrennte Abschnitte lesen und anzeigen.

Enrollment neuer Principals bleibt dem vertrauenswürdigen Host-/Konto-Provisionierungsfluss vorbehalten. `PrincipalRegistered` und `RoleRetired` werden von dieser Desktopoberfläche nicht angeboten; die Storagegrenze weist diese Übergänge ab. Eine Rollenhierarchie, Wildcards oder implizite Grants entstehen nicht.

## Commit- und Cursorsemantik

Jede erfolgreiche Änderung ist append-only und verwendet eine erwartete Basisrevision. Der Policyrecord, die neue Policyprojektion, der SecurityEpoch, der Shared-Revision-Eintrag und der erforderliche Auditrecord werden gemeinsam über den WAL-Commit publiziert. Ein Konflikt oder fehlende aktuelle Berechtigung veröffentlicht nichts. Jede Policyänderung erhöht den SecurityEpoch genau einmal. Selbstentzug ist zulässig und wirkt ab demselben Commit.

Cursor binden Principal, effektive Rechte und SecurityEpoch. Der produktive Fortsetzungspfad autorisiert jede Seite erneut; ein geänderter Epoch invalidiert den aktiven Cursor mit `CursorInvalidated` und gibt dessen Seite frei. Die Rechteansicht ändert keine Cursor direkt.

## GM-/Player-Bundle

Die Ansicht zeigt das etablierte GM- und Player-Bundle getrennt von zusätzlichen Regeln. `RawHistoryRead` kann Teil des GM-Bundles sein. `AdminRawRead` bleibt ein unabhängiges Recht und ist weder aus dem GM-Namen noch aus dem gewöhnlichen Raw-History-Recht ableitbar. Eine explizite Regel wird als eigene Policyänderung angezeigt und persistiert.

## Prüfumfang

Der Windows-Nachweis deckt dauerhafte Policyänderung samt Required Audit, Epoch-Inkrement, Selbstentzug, GM-Rohrechtetrennung sowie authentisierte Zwei-Fenster-IPC in-process und sidecar ab. Linux/macOS folgen wie vereinbart in M9-07.
