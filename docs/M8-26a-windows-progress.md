# M8-26a – Windows-E2E-Ergebnis

## Vorläufe

Der erste vollständige Windows/NTFS-Lauf vom 2026-10-05 (`m8-26a-20261005T012844Z-823933db`) endete mit `FAIL`. Builds, IPC-Smokes (56/56 je Profil) und konkurrierende Schreiber (4 In-Process-, 5 Sidecar-Prüfungen) bestanden. Die Tastatur- und Commit-Crashfälle scheiterten damals beim UI-Automation-Aufruf `SetFocus`, bevor Tastatureingabe oder Commit-Failpoint erreicht wurden.

Ein weiterer Lauf (`m8-26a-20261005T134338Z-c303fb1a`) bestätigte Builds, beide IPC-Profile, beide Writer-Lock-Fälle und die Commit-Recovery. Er endete mit drei Fehlern beim Aufräumen temporärer WebView2-Profile. Die Keyboard-Resultatdateien meldeten Eingabe, Tab/Enter und den Commit-Crash samt read-only Recovery bereits als `PASS`; die Profile waren beim Löschen noch kurz gesperrt. Das Manifest dieses Laufs bleibt als Fehlerbeleg erhalten.

## Korrekturen

- Der Keyboard-Smoke setzt `WORLDDB_ODE_PROJECT_SMOKE_ROOT` auf den erwarteten Projektpfad und deaktiviert den automatischen Startup-Smoke. `WORLDDB_ODE_DATABASE` wird entfernt, weil es das Projekt bereits beim App-Start öffnet und damit die Erstellungsprüfung stört.
- Der Sidecar-Modus legt das Projekt nun in einem host-authentifizierten Sidecar-Prozess an. Der WAL-Failpoint beendet damit den tatsächlichen Engine-Prozess.
- Die Bereinigung des isolierten Testverzeichnisses wiederholt Löschversuche bis zu 15 Sekunden. Bei Fehlern wird das flüchtige `webview-profile` nicht ins Fehlerarchiv kopiert.

## Vollständiger Windows-Lauf

Das Manifest `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T140119Z-29c16683/manifest.json` meldet auf Windows 11/NTFS `PASS`: 11 von 11 Fällen, 138 Prüfschritte, keine Fehler und kein übersprungener Fall. Es wurde auf Commit `73b65dbfae2f27761471f7d012761e11d6fcb07b` mit sauberem Arbeitsbaum ausgeführt.

- IPC: je 56 Prüfungen für In-Process und Sidecar bestanden.
- Konkurrierende Schreiber: 4 In-Process- und 5 Sidecar-Prüfungen bestanden.
- Tastatur: Texteingabe, Tab-Navigation und Enter-Erstellen in beiden Modi bestanden.
- Commit-Recovery: In-Process-App und Sidecar-Engine wurden nach dem WAL-Sync mit Exitcode 86 beendet; die anschließende read-only Recovery-Prüfung bestand in beiden Modi.

## Wiederholung auf dem aktuellen Branch

Der Lauf `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T154837Z-2b012994/manifest.json` wurde auf Commit `01ab573a620ade99f96ced7e46249734a2283c36` mit sauberem Arbeitsbaum ausgeführt und meldet `FAIL`: 7/11 Fälle bestanden, 0 nicht ausgeführt. Builds, beide IPC-Fälle (56/56 je Profil) und beide Writer-Lock-Fälle (4 bzw. 5 Prüfschritte) bestanden. Die vier Fälle `keyboard_navigation` und `commit_crash_recovery` brachen in beiden Modi vor jeder Eingabe ab.

Die gemeinsame Ursache war `SetCursorPos`: Windows meldete `false`, obwohl der geprüfte Titelleistenpunkt innerhalb des WorldDB-Fensters lag. Eine isolierte Wiederholung meldete Fehlercode 0 bei Ziel `(268,218)`, Fenstergrenzen `(208,208,704,567)` und Cursorposition `(355,480)`. Zusätzlich lieferte ein Setzen auf die bereits aktuelle Cursorposition `false`; der Eingabe-Desktop war `Default`, und weder `LockApp` noch ein Speed-/Auto-Klickerprozess lief. Die native Computer-Use-Schnittstelle fand beide WorldDB-Fenster, konnte das Primärfenster nach einer frischen Erfassung aber nicht aktivieren. In keinem der vier fehlgeschlagenen Fälle wurden Tastatureingaben gesendet.

Der frühere vollständige PASS bleibt ein gültiger Laufbeleg für den getesteten Build auf Commit `73b65db`; der aktuelle FAIL ist als abweichender Lauf auf HEAD archiviert und ersetzt ihn nicht. Die Windows-Tastaturfälle sind mit dem aktuellen Desktop-/Eingabezustand nicht erneut nachgewiesen. Das Manifest und alle erzeugten Rohprotokolle bleiben erhalten.

## Status und Plattformgrenze

Der vollständige Windows-Lauf vom Commit `73b65db` ist belegt; der aktuelle HEAD-Wiederholungslauf zeigt die oben dokumentierte Eingabegrenze. `M8-26a` bleibt im Aufgabenregister bis zum Abschluss seiner Abhängigkeit `M8-26` offen. Die nativen APFS- und ext4-Läufe sowie ihre Startbarkeit bleiben gemäß Nutzervorgabe bis M9-07 zurückgestellt; M8-26d und das M8-Gate bleiben daher offen.
