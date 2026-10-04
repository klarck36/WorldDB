# M8-14f – Korrektur, Archive und Retraction bedienen

Stand: 4. Oktober 2026

## Ergebnis

Der Faktenkatalog unterstützt append-only Archive/Unarchive und explizite Retractionen sowie getrennte Assertion- und Event-Korrekturen. Eine Assertion-Korrektur publiziert in einem Commit genau drei Records: die Ersatz-Assertion, die explizite Retraction des Originals und `Corrects(ersatz, original)`. Eine Event-Korrektur publiziert in einem Commit genau zwei Records: den Ersatz-Event und `Corrects(ersatz, original)`; der Original-Event bleibt aktiv. Retraction und Archivierung sind eigene spätere Lifecycle-Commits. Keine Aktion ändert einen historischen Record in-place.

Die Desktopformulare binden Vorschau und Commit an den ausgewählten Originalrecord und dessen Erstellungsrevision. Commit-Antworten nennen Commitrevision, neue Record-ID, Provenance-ID und bei Assertion-Correct die explizite Retraction. Die Event-Correct-Vorschau zeigt den Ersatz-Event und erklärt, dass keine implizite Retraction erfolgt. Lifecycle-Belege unterscheiden Archivieren, Unarchive und Rücknahme.

Korrekturbefehle verlangen eine stabile `OperationId` und vorab erzeugte Output-IDs. Bei Wiederholung derselben committed Operation werden die kanonischen Records der Ursprungsrevision mit der Eingabe verglichen und derselbe typisierte Beleg zurückgegeben; geänderte Eingaben ergeben `IdempotencyMismatch`. `CommitStatus` macht einen unklaren Ausgang abfragbar. Die UI hält bei unklarem IPC-Ausgang dieselbe Operation zur Reconciliation fest, statt einen zweiten Satz IDs zu erzeugen.

## Nachweise

- Storage-Korrekturtests belegen die atomare Drei-Record-Assertion-Korrektur, die atomare Zwei-Record-Event-Korrektur, getrennte Retraction/Archive-Commits, Reopen-Persistenz, Required Audit und Idempotenz; die vollständige Storage-Suite besteht mit 104 Tests, ein Langzeittest ist erwartungsgemäß ignoriert. Alle Storage-Integrationstestziele bestehen.
- `worlddb-ode-engine`: 23 Unit-Tests und ein Sidecar-Transporttest bestanden.
- Native Windows-IPC-Smokes bestanden in-process und sidecar. Sie prüfen Assertion-Korrektur mit expliziter Original-Retraction/`Corrects`, getrennte Archivierung/Unarchive und die bestehenden Mask-/Boundary- und Auflösungspfade.
- Striktes Clippy besteht für Root-Workspace, ODE-Standardprofil und ODE-Sidecar-Profil. Standard- und Sidecar-Build bestehen.
- Der Request transportiert den größeren Facts-Befehl heap-gebunden, ohne seine serialisierte JSON-Form zu ändern.
- `cargo xtask verify` bestand auf Windows: 39 PASS, ein erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. Plancheck, Sourcecheck, Ausnahme-Policy, `git diff --check HEAD`, Node-Syntax und PowerShell-Parser bestanden.
- Die vier eng gebundenen Korrekturmethoden verwenden den versionierten Ausnahmegrund `WDB-EXC-0008`; der Registereintrag läuft am 31. Oktober 2026 ab und verlangt den Ersatz durch typisierte Korrekturanfragen.
- Der native Smoke führt die Event-Korrektur wegen der noch ausstehenden Event-Erfassung aus M8-15 nicht durch. Ihre Zwei-Record-Wirkung, aktive Originalhistorie, explizite spätere Retraction und Idempotenz sind durch den persistenten Storage-API-Test belegt; UI-Vorschau und UI-Beleg sind implementiert.
- Linux- und macOS-Prüfungen bleiben vereinbarungsgemäß bis M9-07 zurückgestellt.
