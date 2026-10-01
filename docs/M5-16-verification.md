# M5-16 – Storage Verify (Windows)

## Ergebnis

`StorageVerifier` ergänzt den Recovery-Scan um einen unabhängigen, nur lesenden Prüflauf für das aktuelle Manifestinventar, die referenzierten History- und SecurityPolicy-Segmente, den WAL-OperationId-Index sowie Schema- und Capability-Historien. Der Bericht enthält immer die letzte vollständig verifizierte `safe_revision`, die effektive Read-only-/Recovery-Einstufung, Inventarzähler und je Befund eine Schadensklasse mit sicheren nächsten Aktionen.

Der Verifier schreibt, kürzt oder repariert keine Datenbankdateien. Er hält während des Laufs den vorhandenen exklusiven Writer-Lock, damit kein WorldDB-Writer parallel ein anderes Inventar publiziert. Erkennt der zweite Prüflauf einen zusätzlichen Integritätsfehler, bleibt der Lock read-only.

## Unabhängige Prüfschritte

- Manifest und `CURRENT`: Pointer, Manifestdigest, Generation, Revision und Commit-Hashbindung werden durch den read-only Recovery-Scan geprüft.
- WAL und Commitpräfix: Der Scan validiert Framegrenzen, Prüfsummen, OperationIds, Commitmarker, Hashkette und Revisionsfolge. Nur vollständig verifizierte Commits erhöhen `safe_revision`.
- OperationId-Index: Bei vollständig sauberem WAL wird der Index separat aus den WAL-Segmenten rekonstruiert. Anzahl und jeder `Committed(receipt)`-Eintrag werden mit den unabhängig gescannten Commitframes abgeglichen. Bei beschädigtem oder unvollständigem WAL weist der Bericht den Index als nicht vollständig verifiziert aus.
- History-Segmente: Referenzidentität und `ContentDigest` werden erneut geprüft; danach werden kanonische Recordframes typisiert gelesen. Schema-Records werden nach ihrer Schema-Revision gesammelt und mit dem Schema-Referenzmodell auf Revisionstreue, doppelte Definitionen und doppelte Symbole geprüft. Genesis-Definitionen werden als Anfangsschema validiert.
- SecurityPolicy-Segmente: Referenzidentität, Digest und gebundene Revision werden geprüft. Die gespeicherten Policy-Snapshots werden zusätzlich als vollständige Capability-Historie materialisiert; fehlende oder doppelte Versionen, ungültige Epochenfolgen und ungültige Subjects/Assignments schlagen fehl.

## Korruptionsklassen und sichere nächste Schritte

| Gezielter Fall | Erkennung im Bericht | Sichere nächste Schritte |
| --- | --- | --- |
| Bitflip in einem WAL-Commitframe | `Bitflip`; `safe_revision` bleibt bei Genesis | Original bewahren, read-only halten; getrennt verifiziertes Backup in ein neues Ziel zurückspielen oder in eine neue Datenbank salvagen |
| Trunkierter WAL-Tail | `Truncation`; `safe_revision` bleibt beim letzten vollständigen Commit | Original bewahren, read-only halten; anschließend journalisierte Tail-Recovery ausführen |
| Prepare-/Commitframe-Reihenfolge vertauscht | `Reorder`; kein Commit hinter der verletzten Reihenfolge gilt als sicher | Original bewahren und read-only halten; getrenntes Restore oder Salvage in ein neues Ziel |
| Doppelte OperationId in einem erneut auftretenden Prepareframe | `DuplicateFrame`; die Revision des vorherigen vollständigen Commits bleibt erhalten | Original bewahren und read-only halten; getrenntes Restore oder Salvage in ein neues Ziel |
| WAL-Prüfsummen gültig, Replay-Snapshot aber revisionssemantisch ungültig | `SemanticInvalidity`; der Commit bleibt Teil des WAL-Präfixes, das Materialisieren des Inventars wird abgewiesen | Original bewahren und read-only halten; getrenntes Restore oder Salvage in ein neues Ziel |

Die Schadensklasse benennt das beobachtete Integritätsmuster und behauptet keine physische Ursache. So bezeichnet `Bitflip` einen fehlgeschlagenen Digest-/Prüfsummenbezug; der Bericht kann daraus nicht beweisen, ob tatsächlich ein einzelnes Bit umgekippt ist.

Zusätzliche checksum-gültige Negativfälle prüfen doppelte Schema-Symbole und eine Lücke in der Capability-Historie. Sie werden als `SemanticInvalidity` gemeldet.

Zwei weitere Segment-Negativfälle ändern getrennt die gespeicherte `SegmentId` oder den `ContentDigest`. Beide werden gegen die Manifestreferenz zurückgewiesen; die Tests bestätigen, dass Verify die beschädigten Quellbytes nicht verändert.

## Windows-Nachweise

- `cargo test --locked -p worlddb-storage-file`: 80 Tests bestanden, 0 fehlgeschlagen.
- `cargo test --locked --workspace`: bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: bestanden.
- `cargo fmt --all -- --check`: bestanden.
- `cargo xtask verify`: 32 bestanden, 1 erwarteter M0-14-Skip, 0 fehlgeschlagen. Der M0-13-Testkit-Lauf `M0-13-20261001T175124Z-2d0896dfe9` ist PASS.
- Im kanonischen Verify war `working-tree-whitespace` (`git diff --check HEAD`) erfolgreich; GitHub wurde nicht verwendet.

Linux/macOS-Läufe und deren Plattformnachweise wurden gemäß Arbeitsvorgabe zurückgestellt. Backup-Readback verbleibt in den geplanten Folgeaufgaben M7-08/M7-10/M8-26d; M5-16 beansprucht dafür keinen Nachweis.
