# M0-14 – CI-Anbieter und Optionen

**Geprüft:** 2026-09-29

## Entscheidung

Die lokale Entwicklungsarbeit nutzt lokales Git und benötigt keinen Remote-Host. Ein GitHub-Zugang oder eine Verbindung zu GitHub ist keine Voraussetzung für M1–M8. M0-14 bleibt ausschließlich wegen des fehlenden externen macOS-Matrixlaufs und des externen Artefaktnachweises offen.

Für den späteren Plattformnachweis bleibt die Matrix anbieterneutral. Es wird jetzt kein zusätzlicher CI-Dienst eingerichtet oder ausgewählt; der Plan kann lokal weiterlaufen. Vor den Release-Gates muss ein CI-Anbieter mit macOS-Runner die bestehende Matrix ausführen und die Manifeste archivieren.

## Mögliche Anbieter für den späteren Plattformnachweis

- **GitLab.com mit GitLab CI/CD:** Das Open-Source-Programm bietet bei erfüllten Voraussetzungen GitLab-Ultimate-Funktionen und 50.000 Compute-Minuten. Gruppe und Quellcode müssen öffentlich sein, jedes Projekt im Namespace eine OSI-anerkannte Lizenz haben und die Mitgliedschaft jährlich erneuert werden. Die macOS-Runner sind Beta; GitLab dokumentiert bekannte Warte- und Hängeprobleme. Das wäre eine mögliche Alternative, erfordert aber einen GitLab-Host und erfüllt denselben M0-14-Abnahmepunkt noch nicht ohne echte macOS-Ausführung. [Open-Source-Programm](https://about.gitlab.com/solutions/open-source/join/), [macOS-Runner](https://docs.gitlab.com/ci/runners/hosted_runners/macos/)
- **CircleCI mit einem unterstützten Repository-Host:** CircleCI Cloud integriert unter anderem GitLab und bietet macOS-Ausführung. Das wäre ein zusätzlicher Dienst samt eigener Integration und Konfiguration. [VCS-Integrationen](https://circleci.com/docs/guides/integration/version-control-system-integration-overview/)

## Stand der Matrix

Saubere lokale Windows- und WSL2/Linux-Läufe sind dokumentiert. Ein externer Anbieterjob und ein macOS-Lauf fehlen weiterhin. Diese Lücke hält M1–M8 nicht an; M0-14 bleibt bis zum Plattformnachweis `BLOCKED` und muss vor M9-13b und M10-10 abgeschlossen werden.
