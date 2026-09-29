# M0-14 – CI-Anbieter und Optionen

**Geprüft:** 2026-09-29

## Entscheidung

Ein zweiter CI-Anbieter ist nicht erforderlich. GitHub Actions kann öffentliche Repositories auf Linux, Windows und macOS ausführen. Die Standard-Runner für öffentliche Repositories sind laut GitHub kostenlos und unbegrenzt verfügbar. GitHub Actions ist damit für die geplante Plattformmatrix ausreichend. [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)

GitHub wurde auf Nutzervorgabe auf später verschoben. Es wurde kein GitHub-Workflow angelegt, kein Repository erstellt und keine Kontoverknüpfung vorgenommen. Die anbieterneutrale Matrix und die lokalen Runnerläufe bleiben vorbereitet. M0-14 bleibt offen, bis GitHub später eingerichtet und die externe Matrix einschließlich macOS ausgeführt wurde.

## Optionale Alternativen, falls GitHub nicht genutzt wird

- **GitLab.com mit GitLab CI/CD:** Das Open-Source-Programm bietet bei erfüllten Voraussetzungen GitLab-Ultimate-Funktionen und 50.000 Compute-Minuten. Gruppe und Quellcode müssen öffentlich sein, jedes Projekt im Namespace eine OSI-anerkannte Lizenz haben und die Mitgliedschaft jährlich erneuert werden. Die macOS-Runner sind Beta; GitLab dokumentiert bekannte Warte- und Hängeprobleme. Das wäre eine mögliche Alternative, erfordert aber einen GitLab-Host und erfüllt denselben M0-14-Abnahmepunkt noch nicht ohne echte macOS-Ausführung. [Open-Source-Programm](https://about.gitlab.com/solutions/open-source/join/), [macOS-Runner](https://docs.gitlab.com/ci/runners/hosted_runners/macos/)
- **CircleCI mit einem unterstützten Repository-Host:** CircleCI Cloud integriert unter anderem GitLab und bietet macOS-Ausführung. Für Open-Source-macOS-Builds nennt CircleCI derzeit 25.000 kostenlose Credits pro Monat mit höchstens zwei parallelen Jobs. Das wäre ein zusätzlicher Dienst samt eigener Integration und Konfiguration; für die geplante spätere GitHub-Actions-Nutzung ist es nicht nötig. [VCS-Integrationen](https://circleci.com/docs/guides/integration/version-control-system-integration-overview/), [Credits für Open Source](https://circleci.com/docs/guides/plans-pricing/credits/)

## Stand der Matrix

Saubere lokale Windows- und WSL2/Linux-Läufe sind dokumentiert. Ein externer Anbieterjob und ein macOS-Lauf fehlen weiterhin. Sobald GitHub später eingerichtet wird, kann die vorhandene Matrix dort ausgeführt und das Step-Manifest archiviert werden. Bis dahin bleibt M0-14 `BLOCKED`; ein weiterer Anbieter muss nicht ausgewählt werden.
