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

## Status und Plattformgrenze

Der Windows-Lauf ist vollständig bestanden. `M8-26a` bleibt im Aufgabenregister bis zum Abschluss seiner Abhängigkeit `M8-26` offen. Die nativen APFS- und ext4-Läufe sowie ihre Startbarkeit bleiben gemäß Nutzervorgabe bis M9-07 zurückgestellt; M8-26d und das M8-Gate bleiben daher offen.
