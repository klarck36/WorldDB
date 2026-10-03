# M7-13a – Isolierter Import-/Exportadaptervertrag

## Ablauf

`worlddb-cli adapter run` lädt ein kanonisches Manifest und eine Eingabedatei innerhalb ihrer Bytebudgets. Der Host startet genau den angegebenen Adapter in einem eigenen OS-Prozess und löscht vor dem Start dessen Umgebung. Ein Adapter erhält keine Datenbankdatei, keinen Core-Handle, keine Storage-Session und keine prozess-internen Rust-Objekte.

Der Host sendet zuerst das versionierte, digestsicher gerahmte Manifest. Der Adapter antwortet mit ProtocolMajor/Minor und sortierten ASCII-Capabilities. Der Host prüft Major, Mindestminor und jede Required-Capability, bevor er Eingabebytes sendet. Unbekannte oder fehlende Required-Capabilities brechen geschlossen ab. Nach positiver Verhandlung erhält der Adapter eine einzige begrenzte Bytefolge.

Der Host akzeptiert genau einen digestsicheren Outputframe und verlangt danach EOF sowie einen erfolgreichen Adapterexit. Fehler, ungültige Frames, zusätzliche Antwortbytes, Timeout oder Prozessabbruch liefern kein Ergebnis an den Aufrufer. Die CLI schreibt die Ausgabedatei erst nach diesem erfolgreichen Ende. Der Aufrufer prüft das Ergebnis anschließend mit den kanonischen WorldDB-Decodern und dem validierten Importvertrag.

## Determinismus und Grenzen

Das Manifest bindet Operation, ProtocolMajor/Minor, 256-Bit-Seed, die exakten kanonischen Mappingplan-Bytes, Zeit-/Speicherbudget und sortierte Required-Capabilities. Der Host inventiert keine IDs im Adapterprozess. Gleiche Eingabebytes und dieselbe Manifestkonfiguration werden als derselbe logische Eingabestrom übergeben; der nachgelagerte kanonische Importplan prüft die resultierenden Records erneut.

- Wall-clock-Budget: 1 ms bis höchstens eine Stunde; Verhandlung und Ausführung zählen gemeinsam.
- Adapter-Committed-Memory: höchstens 1 GiB; Windows setzt Prozess- und aggregiertes Joblimit am gesamten Prozessbaum.
- Adapter-Ein- plus -Ausgabe: zusammen höchstens 512 MiB; jedes einzelne Frame hat zusätzliche feste Header-/Digestgrenzen.
- Mappingplan: höchstens 64 MiB; Capability-Satz: höchstens 256 kanonische Tokens.
- Windows-Job: höchstens 64 Prozesse; Jobschließung beendet verbliebene Nachkommen.
- stderr wird fortlaufend geleert und Diagnoseinhalt verworfen; der Host meldet, ob der Adapter mehr als 64 KiB ausgegeben hat.

Unbekannte Plattformen starten keinen Adapter, solange ihre Prozessmemory-Grenze nicht implementiert ist. Linux/macOS folgen im Plattformgate M9-07. Entfernen eines Adapters hat keine Wirkung auf bereits importierte Datensätze, weil Adapterausführung keine Rückreferenz auf gespeicherte Daten erhält.

## CLI-Form

```text
worlddb-cli adapter run --manifest <manifest.bin> --input <source.bin> --output <logical-output.bin> -- <adapter-executable> [arguments...]
```

Die CLI speichert nur den erfolgreich zurückgegebenen Output. Sie führt keine Datenbankmutation aus; die nachgelagerte Importausführung bleibt an den kanonischen Importmanager und den persistenten Commitadapter gebunden.
