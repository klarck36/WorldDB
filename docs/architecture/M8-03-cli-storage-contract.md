# M8-03 – CLI Verify, Recovery und Salvage

## Befehle

```text
worlddb-cli [--format human|jsonl] v1 verify <database-directory>
worlddb-cli [--format human|jsonl] v1 recovery inspect <database-directory>
worlddb-cli [--format human|jsonl] v1 recovery run --apply <database-directory>
worlddb-cli [--format human|jsonl] v1 open --read-only <database-directory>
worlddb-cli [--format human|jsonl] v1 salvage <source-directory> --output <new-directory>
```

`verify`, `recovery inspect`, `open --read-only` und `salvage` öffnen die Quelle ausschließlich lesend. Sie fordern einen gemeinsamen Betriebssystem-Lock auf der bereits vorhandenen `LOCK`-Datei an. Eine fehlende `LOCK`-Datei wird nicht angelegt; der Aufruf endet mit `StorageRead`, ohne den Datenbankbaum zu verändern. Während ein gemeinsamer Leselock gehalten wird, kann kein kooperierender WorldDB-Writer den exklusiven Lock erwerben.

`open --read-only` ist im aktuellen CLI eine einmalige Validierungs- und Verify-Operation. Sie startet keine interaktive Sitzung. Der zurückgegebene Bericht enthält den geprüften `safe_revision`-Stand und die Read-only-Einstufung.

`recovery inspect` führt denselben unabhängigen Storage-Verify-Lauf aus und präsentiert ihn als Recovery-Bericht. `recovery run` ist die einzige hier angebotene Reparaturoperation und verlangt die explizite Schreibfreigabe `--apply`. Sie verwendet `RecoveryManager` samt Recovery-Journal. Nach erfolgreichem Lauf verifiziert die CLI den Endstand erneut.

`salvage` schreibt ausschließlich in ein neues Ziel außerhalb der Quelle. Das Ziel darf nicht existieren und sein Elternverzeichnis muss vorhanden sein. Die Quellverzeichnisse werden nur gelesen; der neue Fork wird als markiertes Salvage-Archiv angelegt. Die CLI gibt weder Quell- noch Zielpfade oder gespeicherte Fehlerdetails aus.

## Berichte und Fehlergrenze

Jeder erfolgreich erstellte Bericht enthält `safe_revision`, `disposition`, Befundzahl und Zähler für alle sechs beobachtbaren Schadensklassen: `Bitflip`, `Truncation`, `Reorder`, `DuplicateFrame`, `SemanticInvalidity` und `Other`. `safe_actions` ist die deduplizierte Vereinigung der vom Verifier pro Befund angebotenen sicheren Folgeschritte. Zahlen werden im JSONL-Envelope als Dezimalstrings serialisiert.

`source_modified` sagt aus, ob der jeweilige Befehl die Quelldatenbank verändert hat. Bei den Leseoperationen und bei Salvage ist der Wert `false`; bei expliziter Recovery wird er anhand der abgeschlossenen Tail-, Snapshot- und Manifestreparaturen ausgegeben. Ein erfolgreich erstellter Verify-Bericht hat Exitcode 0, auch wenn seine `disposition` Recovery oder Read-only verlangt. Der Prozessstatus zeigt, ob der Bericht erstellt wurde; `disposition` zeigt den Zustand der Datenbank.

Fehler verlassen die Grenze ausschließlich als dokumentierter Public Code. Pfade, Betriebssystemdetails, Recovery-Finding-Debugtexte, Segment-IDs, Gründe aus Salvage-Einträgen und andere dynamische Diagnosen werden nicht in stdout/stderr kopiert. JSONL bleibt ein einzelner versionierter Envelope pro Aufruf.

Die CLI meldet bei Salvage nur die neue `DatabaseId`, Inventarquelle, sichere Revision, Einstufung, Anzahl kopierter/ausgelassener Segmente und die Befundzahl. Der vollständige Verlust- und Unsicherheitsbericht verbleibt im markierten Zielarchiv.
