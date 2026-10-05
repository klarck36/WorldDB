# M8-26a – Windows-E2E-Zwischenstand

## Vollständiger Lauf

Der vollständige Windows/NTFS-Lauf vom 2026-10-05 (`m8-26a-20261005T012844Z-823933db`) endete mit `FAIL`. Die In-Process- und Sidecar-Builds sowie beide IPC-Fälle bestanden; die IPC-Smokes meldeten jeweils 56 bestandene Prüfungen. Die Writer-Lock-Fälle bestanden mit 4 In-Process- und 5 Sidecar-Prüfungen.

Beide Keyboard-Fälle und beide Commit-Crash-Fälle scheiterten beim UI-Automation-Aufruf `SetFocus` auf dem Fensterwurzelelement. Der Voll-Lauf erreichte deshalb noch keine Tastatureingabe und keinen injizierten Desktop-Prozessabbruch.

## Folgeprüfungen

- Das sichtbare WorldDB-Primärfenster gehört dem gestarteten Desktopprozess. Das Texteingabefeld wird vom gestarteten `msedgewebview2.exe`-Kindprozess bereitgestellt; die Prozessbaumprüfung akzeptiert diesen Kindprozess, während Tastaturaktionen zusätzlich den WorldDB-Vordergrundprozess und das konkrete fokussierte Steuerelement verlangen.
- Das Erstellen-Steuerelement liegt anfänglich außerhalb des sichtbaren Fensters (`IsOffscreen = true`). `ScrollItemPattern.ScrollIntoView()` macht es sichtbar; der native Klickpunkt bleibt auf das WorldDB-Fenster begrenzt.
- Jeder Lauf erhält ein eigenes WebView2-Datenverzeichnis; die Testläufe erzeugen darin jeweils einen eigenen `EBWebView`-Profilordner.
- Auf dem Host setzte die automatisierte Zeichenfolge den Accessibility-Wert nicht korrekt zusammen; zuletzt wurde nur das eingegebene Zeichen gemeldet. Die Tab-Prüfung blieb auf `Neuer Projektname`.
- Ein Folgeversuch mit dem nativen Erstellen-Steuerelement erreichte den WAL-Crash-Failpoint nicht. Die read-only CLI-Prüfung des temporären Datenbankverzeichnisses meldete `safe_revision = 0`, `disposition = Clean`, `source_modified = false`. Der Storage-Crash-Recovery-Einzeltest besteht, ersetzt aber den fehlenden Desktopnachweis nicht.

## Status

`M8-26a` ist noch nicht abgeschlossen. Erforderlich sind erfolgreiche Keyboard- und Commit-Crash-Fälle jeweils in In-Process und Sidecar sowie ein vollständiges PASS-Manifest. macOS/APFS und Linux/ext4 bleiben wie vereinbart bis M9-07 zurückgestellt.
