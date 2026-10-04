# M8-23b – Export-/Import-Oberfläche

**Status:** DONE für den lokalen Windows-Arbeitsumfang. Linux/macOS bleiben wie vereinbart bis M9-07 ausstehend.

## Ergebnis

Der Exportbereich bindet einen inklusiven Revisionsbereich, explizite HistorySpace-UUIDs und Recordklassen. Nicht exportierbare Migrationsrecords sind im Auswahlkatalog nicht enthalten. Auswahl und Zielpfad kommen aus nativen Ordner-/Dateidialogen; Renderer-IPC nimmt keine Pfade an.

**Logical Export** und **Teilen-Export** werden als eigene Formate ausgeführt und dargestellt:

- Logical Export verlangt `HistorySpaceDefinition` im Recordklassenscope. Das Ergebnis zeigt Quell-DatabaseId, Snapshotrevision, Recordzahl und das vollständige Auslassmanifest mit den ausgelassenen Record- und Storageklassen. Das Quellprojekt wird nicht geändert.
- Teilen-Export wendet die aktuelle Feld-, Beziehungs- und Scope-Policy an. Das Ergebnis zeigt keine Quellidentität und keine ausgelassenen Mengen. Es wird ausdrücklich als Teilen-Export und nicht als Sicherung bezeichnet. Die erforderlichen Autorisierungs- und Abschlussnachweise werden dauerhaft im Quellprojekt protokolliert; die Oberfläche weist auf diese Änderung hin.

Der Importablauf erstellt zunächst einen kanonischen Plan mit expliziten typisierten ID-Remaps. Der Nutzer wählt das Zielprojekt, Logical-Export-Artefakt und den neuen Plan-Dateipfad über native Dialoge. Die anschließende Prepare-Aktion bindet denselben Export an den Plan und den aktuellen Identitätsbestand des Ziels; sie prüft Kollisionen und Referenzabschluss. Prepare ist eine Validierungsgrenze und veröffentlicht keine Records in die Zieldatenbank. Sharing-Artefakte werden als Importquelle nicht akzeptiert.

Renderer-Requests sind geschlossene, versionierte DTOs mit begrenzten Scope- und Mappinggrößen. Vollständige Pfade werden weder vom Renderer angenommen noch in Ergebnissen zurückgegeben; die Oberfläche zeigt nur die gewählten Dateinamen an. Export, Import, Backup, Migration und Recovery verwenden den gemeinsamen exklusiven Host-Operationsschutz.

## Verifikation auf Windows

- ODE Desktop-Tests: **26 In-Process bestanden**, **27 Sidecar bestanden**.
- CLI-Export-/Import-Roundtrip mit Scopemanifest, Teilen-Rechten und getrennten Formaten: **1 bestanden**.
- Striktes Clippy für ODE In-Process und Sidecar bestanden.
- `cargo fmt --all -- --check`, JavaScript-Syntaxprüfung, PowerShell-Parserprüfung und `git diff --check` bestanden.
- `cargo xtask verify`: **39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL**.
- Nativer Zwei-Fenster-IPC-Smoke **In-Process bestanden**, einschließlich Ablehnung eingeschleuster Backup- sowie Export-/Import-Rendererpfade vor nativen Dialogen.
- Nativer Zwei-Fenster-IPC-Smoke **Sidecar bestanden**, einschließlich derselben Pfadprüfungen.
- Plancheck und Sourcecheck bestanden: 257 Tasks, 11 Milestones, 253 Invarianten und 230 Follow-up-Paare.
- Linux/macOS wurden nicht ausgeführt; wie vereinbart sind diese Plattformprüfungen M9-07 vorbehalten.

## Abgrenzung

Die Importoberfläche erstellt und validiert den Remap-Plan. Sie führt keine Datensätze in das Zielprojekt ein. Der spätere persistente Importvollzug bleibt außerhalb des in M8-06 definierten CLI-Vertrags.
