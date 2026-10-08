# ADR-043 – Hostgebundener Projektstart und Multiwindow

**Status:** Windows-Bindung in M8-10 angenommen; hostlokale Unix-Bindung für den nativen Desktop in M8-26b/c ergänzt
**Entscheidungsdatum:** 2026-10-03
**Task:** M8-10
**Evidenz:** `docs/M8-10-windows-progress.md`

## Kontext

M8-09 authentisiert native Fenster und M8-10 bindet dauerhafte Projekte an das angemeldete Windows-Konto. Der Renderer darf weder den Projektpfad noch den WorldDB-Principal festlegen. Ein Projektstart darf außerdem keine stille Formatänderung, Recovery oder zweite Engine mit Writerlock auslösen. Die native Desktopabnahme auf APFS/ext4 erfordert dieselbe fail-closed Grenze unter Unix.

## Entscheidungen

1. **Konto-Principal:** Windows liest die SID aus dem Prozesstoken. macOS/Linux binden die effektive UID an die Plattform-UUID aus der IOKit-`IOResources`-Property bzw. `/etc/machine-id`; diese maschinen- und plattformgetaggten Bytes werden mit der gemeinsamen BLAKE3-Domainseparation `worlddb.host-account-principal.v1` in eine stabile UUID-v8-Principal-ID überführt. Die Unix-Principal-ID ist über Neustarts auf demselben Host stabil. Roh-SID, UID und Host-UUID werden nicht persistiert und nicht als Rendererargument übertragen.
2. **Sidecar-Identität:** Der Desktophost übergibt dem Sidecar nur den Startmodus `--host-account`, keine Principal-ID. Das Sidecar liest und hasht unter Windows seine eigene Prozesstoken-SID und unter macOS/Linux seine eigene effektive UID plus Plattform-UUID bzw. `/etc/machine-id`, bevor es ein Projekt öffnet. Eine gleichnamige Umgebungsvariable wird entfernt und nicht als Identitätsquelle akzeptiert. Ungebundener Enginestart bleibt auf Debug-Builds für lokale Benchmarks begrenzt; Release-Builds weisen ihn ab.
3. **Projektanlage:** Der Host nimmt den über native Ordnerauswahl gewählten Elternordner entgegen; der Renderer liefert nur einen validierten Projektnamen. Der Host löst den Elternordner kanonisch auf, legt eine neue Datenbank an und veröffentlicht Genesis, die Anfangspolicy und den erforderlichen SecurityPolicy-Audit im ersten Commit. Die Policy registriert `gm` und `player`, weist dem angemeldeten Ersteller `gm` zu und gewährt kein `AdminRawRead`.
4. **Projektöffnung:** Der Host wählt den Ordner nativ aus. Die Engine lehnt einen Reparse-Point als Projektwurzel ab, kanonisiert den Pfad, prüft Format, Manifest, Security-History, Storagezustand und exklusiven Writerlock. Öffnen führt weder Recovery noch Migration aus; ein nicht sauberer Storagezustand bleibt unverändert und wird mit `recovery_required` abgelehnt. Der aktuelle Konto-Principal muss `ProjectRead` besitzen.
5. **Multiwindow:** Eine Anwendung hält höchstens ein geöffnetes Projekt und genau ein Enginehandle. Das zweite Fenster darf sich an dasselbe Projekt und dieselbe Engine anhängen. Es erhält eine eigene zufällige Snapshot-ID mit der beim Öffnen gepinnten Revision. Ein anderes Projekt kann erst nach `close_project` geöffnet werden.
6. **Renderervertrag:** Create/Open/Close besitzen geschlossene, versionierte DTOs. Unbekannte Felder wie `project_path` oder `principal` werden abgewiesen. Nur der Hostcommand gibt Pfad und Principal an die Engine weiter.

## Folgen

- Alle Projekte desselben Windows-Kontos verwenden denselben Principal. Unter Unix gilt dieselbe Stabilität nur für dieselbe effektive UID auf demselben Host. Ein anderer Account oder Host erhält nur Zugriff, wenn eine gültige Projektpolicy ihn ausdrücklich berechtigt; Import-/Kontomigration bleibt ein separater Vorgang.
- Recovery, Formatupgrade und ACL-Änderungen werden nicht implizit beim Öffnen ausgeführt.
- Ein offenes Projekt wird anwendungsweit geschlossen, nicht nur im aufrufenden Fenster. Beide Fenster aktualisieren ihren angezeigten Openstatus über die Hostabfrage.
- Windows ist in M8-10 geprüft. Die Unix-Identitätsquelle ist in M8-26b/c implementiert und wird dort in echten Desktopläufen geprüft; umfassende IPC-, CSP/WebView- und Paketierungsabnahme bleibt M9-07.

## Implementierungs- und Testreferenzen

- `experiments/ode-002/crates/ode-engine/src/project.rs`
- `experiments/ode-002/crates/ode-engine/src/main.rs`
- `experiments/ode-002/crates/desktop-shell/src/host_session.rs`
- `experiments/ode-002/crates/desktop-shell/src/main.rs`
- `experiments/ode-002/scripts/run-ipc-security-smoke.ps1`
- `project::tests::opening_never_repairs_a_project_with_an_incomplete_wal_tail`
- `project::tests::opening_a_directory_junction_is_rejected` (Windows)
- `ipc_security_tests::project_dtos_reject_renderer_selected_paths_and_principals`
