# M0-14 – CI-Anbieter und Optionen

**Geprüft:** 2026-10-05

## Entscheidung

GitHub Actions ist als Ausführungsanbieter für das bestehende Repository eingerichtet. Die anbieterneutrale Matrix bleibt in `policy/ci-matrix.tsv` und wird über `tools/run_ci_job.py` ausgeführt. Die Workflows in `.github/workflows/` sind manuell startbar; `windows-only` ist vorausgewählt. Linux und macOS laufen nur bei einer ausdrücklichen Auswahl von `full-matrix` und bleiben bis zum späteren Plattformnachweis zurückgestellt. Es gibt keinen Push-Trigger.

Die verwendeten Actions sind auf vollständige Commit-SHAs festgelegt. Der wiederverwendbare Job installiert Rust 1.85.0 und 1.88.0, dazu cargo-deny 0.20.2, und archiviert `ci-job.json`, `steps.tsv` sowie die Verify-Logs als GitHub-Artefakt.

Die Konfiguration selbst ist noch kein CI-Lauf. `M0-14` bleibt `BLOCKED`, bis die vollständige Matrix einschließlich eines echten macOS-Laufs auf dem aktuellen Produktstand erfolgreich war und die Anbieterartefakte vorliegen. Das Windows-Profil kann vorher allein ausgeführt werden. M0-14 hält M1–M8 nicht an, bleibt aber Voraussetzung für M9-13b und M10-10.

## Mögliche Anbieter für den späteren Plattformnachweis

- **GitLab.com mit GitLab CI/CD:** Das Open-Source-Programm bietet bei erfüllten Voraussetzungen GitLab-Ultimate-Funktionen und 50.000 Compute-Minuten. Gruppe und Quellcode müssen öffentlich sein, jedes Projekt im Namespace eine OSI-anerkannte Lizenz haben und die Mitgliedschaft jährlich erneuert werden. Die macOS-Runner sind Beta; GitLab dokumentiert bekannte Warte- und Hängeprobleme. Das wäre eine mögliche Alternative, erfordert aber einen GitLab-Host und erfüllt denselben M0-14-Abnahmepunkt noch nicht ohne echte macOS-Ausführung. [Open-Source-Programm](https://about.gitlab.com/solutions/open-source/join/), [macOS-Runner](https://docs.gitlab.com/ci/runners/hosted_runners/macos/)
- **CircleCI mit einem unterstützten Repository-Host:** CircleCI Cloud integriert unter anderem GitLab und bietet macOS-Ausführung. Das wäre ein zusätzlicher Dienst samt eigener Integration und Konfiguration. [VCS-Integrationen](https://circleci.com/docs/guides/integration/version-control-system-integration-overview/)

## Stand der Matrix

Saubere lokale Windows- und WSL2/Linux-Läufe sind dokumentiert. Es gibt noch keinen externen Anbieterjob und keinen macOS-Lauf. Diese Lücke hält M1–M8 nicht an; M0-14 bleibt bis zum Plattformnachweis `BLOCKED` und muss vor M9-13b und M10-10 abgeschlossen werden.
