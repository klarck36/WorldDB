# M8-23a – Backup-/Restore-Oberfläche

**Status:** DONE für den lokalen Windows-Arbeitsumfang. Linux/macOS bleiben wie vereinbart bis M9-07 ausstehend.

## Ergebnis

Die Desktopoberfläche unterstützt beide im CLI-Vertrag definierten Sicherungsprofile:

- **ExactDatabaseBackup** bindet den Auditumfang `Excluded`.
- **AuditCompleteBackup** bindet den Auditumfang `Included`.

Die Profilwahl ist im geschlossenen Projektzustand sichtbar und steuert sowohl Erstellung als auch Verify. Die Anwendung wählt Quellprojekt und Zielordner über native Windows-Dialoge. Die IPC-Eingaben enthalten keine Rendererpfade; zusätzliche Felder werden wegen des geschlossenen Request-Vertrags abgewiesen. Ziele werden in einem eindeutig neu erzeugten Unterordner angelegt. Nach der Erstellung prüft ein separater CLI-Aufruf das fertige Backup mit dem gewählten Profil und Auditumfang. Ein Backup kann auch eigenständig für ein gewähltes Profil verifiziert werden.

Restore ist ausschließlich ein **Clone-Restore in ein neues Ziel**. Der Nutzer wählt zuerst ein sauberes Autorisierungsprojekt, dessen DatabaseId zur Quelle des Backups passt, dann das Backup und anschließend den Zielordner. Die Host-Policy und die erforderlichen Rechte werden im CLI geprüft. Die Wiederherstellung muss eine neue DatabaseId erzeugen und darf die Quellbank nicht verändern. Danach führt die Anwendung `v1 verify` auf dem Klon aus und verlangt ein sauberes Ergebnis ohne Findings sowie übereinstimmende Zielrevision. Es gibt keinen In-Place-Restore.

Die Oberfläche gibt nur zusammengefasste Statusdaten und Ordnernamen an den Renderer zurück. Vollständige Dateisystempfade laufen nicht über Renderer-IPC. Backup-/Restore-Aktionen belegen denselben exklusiven Desktop-Operationsschutz wie konkurrierende Projekt- und Migrationsaktionen.

## Verifikation auf Windows

- ODE Desktop-Tests: **21 In-Process bestanden**, **22 Sidecar bestanden**.
- CLI-Backup-Profiltests: **4 bestanden**; Restore-Policy-Test: **1 bestanden**.
- Striktes Clippy für ODE In-Process und Sidecar bestanden.
- `cargo fmt --all -- --check`, JavaScript-Syntaxprüfung und `git diff --check` bestanden.
- `cargo xtask verify`: **39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**.
- Nativer Zwei-Fenster-IPC-Smoke **In-Process bestanden**. Alle Smoke-Schritte bestanden, einschließlich Ablehnung eingeschleuster Backup-/Restore-Rendererpfade vor nativen Dialogen.
- Nativer Zwei-Fenster-IPC-Smoke **Sidecar bestanden**. Alle Smoke-Schritte bestanden, einschließlich derselben Rendererpfad-Prüfung.
- Plancheck und Sourcecheck bestanden: 257 Tasks, 11 Milestones, 253 Invarianten und 230 Follow-up-Paare.
- Linux/macOS wurden nicht ausgeführt; wie vereinbart sind diese Plattformprüfungen M9-07 vorbehalten.

## Grenzen

Die Oberfläche bietet bewusst nur den vertraglich definierten Clone-Restore an. Ein fehlgeschlagener Verify oder eine Profil-/Umfangsabweichung wird nicht als erfolgreiches Backup bzw. Restore angezeigt. Plattformabnahmen außerhalb Windows stehen weiterhin aus.
