# M0-14 – Anbieteroptionen ohne GitHub

**Geprüft:** 2026-09-29

## Empfehlung: GitLab.com für Repository und CI

GitLab.com passt am besten, wenn WorldDB öffentlich auf GitLab liegen darf: GitLab bietet für qualifizierte Open-Source-Projekte ein Open-Source-Programm mit GitLab-Ultimate-Funktionen und 50.000 Compute-Minuten. Die Anmeldung verlangt eine öffentlich sichtbare Gruppe und Quelltexte; jedes Projekt in der Gruppe muss eine OSI-anerkannte Lizenz tragen. Die Mitgliedschaft muss jährlich erneuert werden. [Programmbedingungen](https://about.gitlab.com/solutions/open-source/join/)

GitLabs macOS-Runner sind direkt in GitLab CI/CD integriert, derzeit aber Beta. Sie stehen dem Open-Source-Programm sowie Premium-/Ultimate-Kunden zur Verfügung. Die dokumentierten Runner nutzen Apple-Silicon-VMs; bekannte Einschränkungen betreffen unter anderem Verfügbarkeit und gelegentlich hängende Jobs. Jobartefakte sind bereits im Free-Tier dokumentiert. [macOS-Runner](https://docs.gitlab.com/ci/runners/hosted_runners/macos/), [Jobartefakte](https://docs.gitlab.com/ci/jobs/job_artifacts/)

Das Projekt hat `MIT OR Apache-2.0` beschlossen. Das erfüllt die Lizenzanforderung, aber noch nicht die öffentliche GitLab-Gruppen-/Quellcode-Anforderung. Es wurde kein GitLab-Projekt erstellt und nichts hochgeladen.

## Alternative: CircleCI mit GitLab als Repository-Host

CircleCI Cloud unterstützt GitLab.com als Repository-Anbieter, einen macOS-Executor und das Speichern von Jobartefakten. Für Open-Source-macOS-Builds nennt CircleCI im Free-Plan 30.000 Credits pro Monat. Das wäre eine Alternative, wenn das Repository bei GitLab liegt, die Pipeline aber bei CircleCI laufen soll. [GitLab-Integration](https://circleci.com/docs/guides/integration/version-control-system-integration-overview/), [macOS-Executor](https://circleci.com/docs/guides/execution-managed/using-macos/), [Open-Source-Credits](https://circleci.com/docs/guides/plans-pricing/credits/), [Artefakte](https://circleci.com/docs/guides/optimize/artifacts/)

## Entscheidung

Für die kürzeste Route zu einem vollständigen M0-14 ist GitLab.com Open Source die Empfehlung, sofern eine öffentliche GitLab-Gruppe und jährliche Programmverlängerung akzeptabel sind. Andernfalls kann CircleCI mit einem unterstützten Repository-Host genutzt werden. Beide Optionen benötigen eine Kontoverknüpfung und ein echtes Anbieterprojekt; die macOS-Zelle muss dort ausgeführt und ihr Step-Manifest archiviert werden.

Bis der Anbieter gewählt und der externe Repository-/Kontozugriff bereitgestellt ist, bleibt M0-14 `BLOCKED`. Die lokale Matrix und der Runner bleiben anbieterneutral; es wurde kein Workflow für einen nicht ausgewählten Anbieter angelegt.
