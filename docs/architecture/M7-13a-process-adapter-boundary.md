# M7-13a – Prozessadapter-Boundary-Gate

## Anlass und Crate-Schnitt

M7-13a muss Import-/Exportadapter als nicht vertrauenswürdige Prozesse ausführen und auf Windows hart nach Speicher, Laufzeit und Prozessbaum begrenzen. Rusts `std::process::Command` stellt keine Job-Objekt-Memory-Limits und keinen suspendierten Start mit nachfolgender Jobzuweisung bereit. Diese OS-FFI-Aufrufe gehören nicht in Core, Storage oder die CLI-Präsentation.

`worlddb-process-adapter` kapselt den OS-spezifischen Start und bleibt unabhängig von Core und Storage. `worlddb-cli` konsumiert diese Grenze. Damit erhält der Workspace eine eigene Plattform-Crate für Prozessressourcen, weil isoliertes Plattform-FFI ein explizites M0-09-Extraktionskriterium erfüllt. `tools/check_crate_graph.py` lässt nur die dokumentierte Richtung zu.

## Windows-Vertrag

Der Prozessadapter erzeugt einen Windows Job mit `JOB_OBJECT_LIMIT_PROCESS_MEMORY`, `JOB_OBJECT_LIMIT_JOB_MEMORY`, `JOB_OBJECT_LIMIT_ACTIVE_PROCESS` und `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Er setzt das Committed-Memory-Limit auf den im Manifest gebundenen Wert; maximal 64 Prozesse gehören zu einem Adapterlauf. Der Kindprozess startet mit `CREATE_SUSPENDED`, wird dem Job zugewiesen und wird erst danach fortgesetzt. Scheitert Erzeugung, Zuweisung oder Resume, wird kein unbegrenzter Fallback gestartet.

Die Jobgrenze gilt für den Adapter und seine Nachkommen. Beim erfolgreichen Ende beendet der Host verbliebene Nachkommen; beim Abbruch schließt der Job-Guard und beendet den Prozessbaum. Der CLI-Zeitwächter deckelt Verhandlung und Ausführung zusammen. Die OS-Grenze deckelt committed memory; außerdem deckeln die Protokoll-Decoder sämtliche Hostpuffer und Eingabe-/Ausgabebytes.

`worlddb-process-adapter/src/windows.rs` ist die einzige lokale Unsafe-Grenze. Job- und Prozesskontrollaufrufe verweisen auf WDB-EXC-0005. M8-04 ergänzt die gebundene Leseroutine für die primäre SID des aktuellen Prozess-Tokens unter WDB-EXC-0006; Desktophost, Sidecar und CLI erhalten dadurch dieselben Identitätsbytes. Alle Aufrufe tragen SAFETY-, TEST- und REVIEW-Belege. Die Crate verschärft das Workspace-Lint weiterhin mit `#![deny(unsafe_code)]`.

## Plattformen

Windows ist der verifizierte M7-13a-Zielhost. Der Adapter verweigert auf anderen Plattformen derzeit den Start, weil dort noch keine harte Prozessmemory-Grenze installiert ist. Linux/macOS-Implementierung und Nachweis bleiben gemäß Projektvorgabe in M9-07; dieser Status erlaubt keine unbegrenzte Ausführung als Ausweichpfad.

## Primärreferenzen

- [Microsoft: Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
- [Microsoft: JOBOBJECT_EXTENDED_LIMIT_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
- [Microsoft: AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject)
