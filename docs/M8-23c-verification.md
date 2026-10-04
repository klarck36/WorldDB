# M8-23c – Purge-Oberfläche

**Status:** DONE für den lokalen Windows-Arbeitsumfang. Linux/macOS bleiben wie vereinbart bis M9-07 ausstehend.

## Ergebnis

Die Hostoberfläche erstellt vor jedem Lauf einen Offline-Purgeplan. Die Vorschau bindet Quelle, Targets, Referenzmodus, vollständigen beziehungsweise begrenzten externen Bestand und bekannte Kopien an einen Fingerprint. Sie zeigt transitive Referenzen, betroffene Records und Indexgenerationen mit begrenzten Listen und Auslasszählern.

Die Ausführung erzeugt das Ziel als separate Datenbank mit einer neuen DatabaseId und speichert einen begrenzten, gehashten Report. Die Oberfläche hält Quelldatenbank, Ziel und Report getrennt sichtbar. Nach dem Lauf werden Ziel und Report geprüft und die unveränderte Quelle bestätigt. Das Verfahren verspricht kein Secure Erase.

Pfade werden ausschließlich im Host über native Datei-/Ordnerdialoge bestimmt. Geschlossene versionierte Renderer-DTOs enthalten keine Pfadfelder; manipulierte Rendererpfade werden vor dem Öffnen eines Hostdialogs abgewiesen. Vorschau, Ausführung und Verwerfen des Planentwurfs verwenden den exklusiven Host-Operationsschutz. Ein Bestätigungsfingerprint bindet die Ausführung an den angezeigten Plan.

## Verifikation auf Windows

- ODE Desktop-Tests: **31 In-Process bestanden**, **32 Sidecar bestanden**.
- CLI-Purge-Integrationstest für explizite Kaskadenbestätigung und Quellbewahrung: **1 bestanden**.
- Striktes Clippy für ODE In-Process und Sidecar bestanden.
- Nativer Zwei-Fenster-IPC-Smoke **In-Process bestanden**, einschließlich Ablehnung eingeschleuster Purge-Rendererpfade vor nativen Dialogen und geordnetem Shutdown.
- Nativer Zwei-Fenster-IPC-Smoke **Sidecar bestanden**, einschließlich derselben Purge-Pfad- und Shutdown-Prüfungen.
- `cargo fmt --all -- --check`, JavaScript-Syntaxprüfung, PowerShell-Parserprüfung, Plancheck, Sourcecheck und `git diff --check` bestanden.
- `cargo xtask verify`: **39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**.
- Linux/macOS wurden nicht ausgeführt; wie vereinbart sind diese Plattformprüfungen M9-07 vorbehalten.

## Abgrenzung

Der Purge-Lauf erstellt ein neues Ziel und einen prüfbaren Bericht. Der Quellbestand bleibt unangetastet; physische sichere Löschung wird nicht zugesichert.
