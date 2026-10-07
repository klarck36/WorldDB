# M9-06 – Security-Härtung (Zwischenstand)

**Status:** RUNNING. Windows und WSL2/Ubuntu 26.04 sind lokal geprüft. Der neueste native macOS-Lauf `37658878947` auf PR `#1` endete mit 41 PASS, einem erwarteten `ci-matrix`-SKIP und 4 FAIL. Die lokale Ursachenbehebung besteht nun unter Windows; eine neue native macOS-Wiederholung steht aus. Der WSL-Workspace lag unter `/mnt/c`; Testdaten lagen unter `/tmp` (tmpfs), daher ist dies kein nativer ext4-Nachweis.

## Geprüfte Non-Interference-Pfade

| Bereich | Nachweis |
|---|---|
| Result und Explain, Index und Full Scan | `paired_hidden_assertion_is_inert_for_resolved_and_explain_index_and_scan` |
| Budgetfehler | `query_budget_errors_share_coarse_work_and_memory_buckets` ordnet semantische Queryarbeit einer groben `QueryWork`-Klasse und Speicherfehler separat `ProcessMemory` zu. |
| Token Search | `token_search_is_exact_typed_and_excludes_hidden_candidates_before_budgets`; `hidden_search_fields_are_removed_before_candidate_budgets` |
| Storage-Adapter für Token Search | `token_search_denies_field_before_inspecting_assertion_value` verwendet einen absichtlich inkompatibel typisierten Wert. Bei verweigertem `FieldRead` wird der Inhalt verworfen, bevor Slotfelder gelesen, Werte in den Suchverlauf kopiert oder ein Suchdokument erzeugt werden. `storage_token_search_omits_assertion_when_field_read_is_denied` prüft zusätzlich den echten Storage-Endpunkt. |
| Graph | `hidden_nodes_and_edges_are_filtered_before_budgets_and_traversal` |
| COUNT, EXISTS und GroupedCount | `count_exists_and_grouped_count_consume_only_visible_resolved_rows` |
| ConflictReport | `conflict_report_filter_keeps_only_authorized_facts` |
| Cursor | `paired_unknown_expired_and_restarted_cursor_errors_share_public_observation` |
| Public Errors | `paired_internal_error_id_is_hidden_by_the_public_error_projection`; `paired_world_compares_values_shapes_public_errors_and_cursor_behavior` |
| Logical Export | `paired_export_omits_unselected_history_spaces_from_records_and_manifest` |
| Required Audit | `paired_required_audit_fault_hides_the_attempted_policy_fingerprint`; `required_audit_failure_blocks_policy_change_without_partial_effect` |

Im Storage-Adapter werden Assertions jetzt bereits vor dem Klonen in den TokenSearch-Verlauf gegen `AssertionRead` und das ausgewählte `FieldRead` geprüft. Der Adapter prüft dieselben Rechte vor dem Zugriff auf Subject, Predicate, Kontextpartition, Gültigkeit und Wert erneut. Die Core-Suche filtert weiterhin vor Tokenisierung, sichtbaren Kandidatenbudgets und Ergebnisaufbau. Der Regressionstest bestätigt insbesondere, dass ein verborgener ungültiger Wert keinen inhaltsabhängigen Fehler erzeugt.

## Lokale Messung

Der Release-Probe unter Windows mischt 101 Messpaare für sichtbare Suche und dieselbe Suche mit 4.096 synthetisch wiederholten, verweigerten SearchDocuments:

| Plattform und Eingabe | p50 | p95 | p99 |
|---|---:|---:|---:|
| Windows, nur sichtbares Dokument | 700 ns | 1.400 ns | 2.300 ns |
| Windows, zusätzlich 4.096 verborgene Kandidaten | 57.300 ns | 97.500 ns | 123.800 ns |
| WSL2/Ubuntu 26.04, nur sichtbares Dokument | 216 ns | 409 ns | 490 ns |
| WSL2/Ubuntu 26.04, zusätzlich 4.096 verborgene Kandidaten | 53.555 ns | 58.819 ns | 74.338 ns |

Das Ergebnis und die sichtbare Budgetklasse bleiben gleich. Die Messung zeigt aber eine messbare lineare Scan-Kostenkomponente. Die 4.096 Zeilen verwenden absichtlich wiederholt dieselbe verweigerte Record-ID; dies ist ein adversariales synthetisches Lastprofil und kein repräsentativer Indexkorpus. Die absolute p95 liegt in diesem Profil unter 100 µs. Daraus folgt keine Constant-Time-Garantie. Ein repräsentativer, versionierter Korpus mit p50/p95/p99 und Peak RSS bleibt für M9-07 erforderlich.

Der Debug-Probe wurde ebenfalls ausgeführt, ist aber nicht als Performancewert zu verwenden: p50 5,5 µs ohne und 1,0925 ms mit 4.096 verborgenen Zeilen. Maßgeblich für die obige Einordnung ist die optimierte Release-Messung.

## Lokale Verifikation

- Windows `cargo test --locked -p worlddb-core`: 527 Unit- und 87 Rustdoc-Tests bestanden.
- Windows `cargo test --locked -p worlddb-storage-file --lib`: 117 Unit-Tests bestanden, 0 fehlgeschlagen, 2 ignoriert (29,43 s).
- `cargo clippy --locked -p worlddb-core -p worlddb-storage-file --all-targets -- -D warnings`: bestanden.
- Vollständiges Windows-`cargo xtask verify`: 45 PASS, ein vorgesehener `ci-matrix`-SKIP, 0 FAIL.
- `cargo fmt --all`: ausgeführt.
- Die gezielten Paarwelt-, Timing-, Export-, Cursor-, Conflict-, Required-Audit-, Aggregat- und Adaptertests bestanden.
- WSL2/Ubuntu: die Core-Suite bestand mit 527 Unit- und 87 Rustdoc-Tests. Die vollständige Storage-Suite bestand nach dem Fix dreimal mit der Standardparallelität (je 105 bestanden, 0 fehlgeschlagen, 1 ignoriert; letzter Lauf 1,78 s). Striktes Linux-Clippy für Core und Storage bestand.
- `.github/workflows/m9-06-security-hardening.yml` bindet den PR-Lauf an den vorhandenen wiederverwendbaren `macos-msrv`-Job. Run `37654031855` lief auf macOS 26.6.2/arm64 mit Rust 1.85 und endete mit 40 PASS, einem erwarteten `ci-matrix`-SKIP und 5 FAIL.
- Die lokale Wiederholung nach den macOS-Korrekturen bestand unter Windows mit `cargo xtask verify`: 45 PASS, ein vorgesehener `ci-matrix`-SKIP, 0 FAIL. Die CLI- und Storage-Fuzz-Dispatcher bestehen jeweils gezielt; der öffentliche Vertragsprüfer und 13 Klassifikationstests bestehen.

## macOS-Fehler und Vertragsbaseline

Die fünf CI-Fehler waren `cli-contract` (ein CLI-Fuzzziel gab einen Fehler
zurück), `clippy-unsafe-policy` (plattformabhängig ungenutzter Import und
Variable), `public-contracts`, `public-contract-tests` sowie
`m5-22-windows-crash-contract` (ein Storage-Fuzzziel gab einen Fehler zurück).
Die Dispatcher-Assertions geben jetzt Ziel und Fehler aus, damit ein erneuter
macOS-Lauf die zwei plattformabhängigen Fuzzfehler eindeutig benennt. Der
Clippy-Befund wurde durch Windows-spezifische Imports und Bindings behoben;
striktes lokales Clippy für Core, Storage und CLI besteht.

Der Zwischenlauf `37657249743` auf Commit `8113392` endete mit 41 PASS, einem
erwarteten SKIP und 4 FAIL. Der nächste Lauf `37658222881` endete ebenfalls
mit 41 PASS, einem erwarteten SKIP und 4 FAIL. Der neueste Lauf
`37658878947`, Job `112920970410`, prüfte den PR-Merge-Commit
`353050e580dd76474b2e924a8242193d488f7d7d` (Branch-Commit `6eef67f`) und
endete erneut mit 41 PASS, einem erwarteten SKIP und 4 FAIL.

`public-contracts` und der passende Test scheiterten, weil M9-06 ausschließlich
Testmodule und test-only Crashprozess-Koordination in Dateien geändert hat,
deren ganze Quelldateien rc.1 per Fingerprint bindet. Die strukturiert
extrahierten API-, Wire-, Fehler-, Format- und Exportwerte blieben unverändert.
Der rc.1-Snapshot blieb unverändert (SHA-256
`CC7DCFE1F0C645CF08B72123E00D99EC3A64986D156BED85F51E9715417E8282`). Gemäß
der dokumentierten Regel `contract_version_changed` wurde ein eigener
unveränderlicher rc.2-Snapshot erstellt; der aktuelle Vertragscheck und alle
12 Klassifikationstests bestanden zunächst. Eine plattformübergreifende
Wiederholung ergab, dass die Rohbyte-Fingerprints zweier Policy-TSVs unter
Windows wegen CRLF von den Git-Blobs abweichen konnten. Der Prüfer normalisiert
nun CRLF zu LF; die noch nicht veröffentlichte rc.2-Arbeitsbaseline wurde
entsprechend korrigiert. Der unveränderte rc.1-Hash wurde erneut bestätigt.
Vertragscheck und 13 Klassifikationstests bestehen lokal.

Der wiederholte Fuzzfehler kam aus leeren, nicht in Git abgebildeten
Storage-Verzeichnissen im M7-16h-Fixture. Beide test-only Fuzz-Helfer erzeugen
vor dem Überlagern der Fixture-Dateien jetzt das kanonische Layout über
`DatabaseLayout::create`. Damit hängt der Test nicht von leeren lokalen
Verzeichnissen ab. Die Dispatcher-Tests und der vollständige Windows-Verify
bestehen mit dieser Korrektur; die native macOS-Abnahme bleibt bis zum neuen
CI-Lauf offen.

Die vorherigen parallelen WSL-Läufe hatten wechselnde Fehler mit `database writer lock is already held`. Die Ursache war der Fork/Exec-Übergang in den Prozessabbruchtests: Der kurzlebige Kindprozess erbte offene `WriterLock`-Deskriptoren aus parallelen Tests. Fiel das Schließen des Elternhandles in dieses Zeitfenster, blieb der `flock` bis zum Exec vorübergehend aktiv. Die Test-Builds registrieren nun aktive `WriterLock`-Handles; ein Kindprozessstart wartet, bis diese Handles geschlossen sind. Die Synchronisierung betrifft ausschließlich Tests und ändert das Produktionsverhalten nicht. Drei vollständige parallele WSL-Läufe und der finale vollständige Windows-Lauf bestanden danach.

## Noch offen

- Den korrigierten macOS-Job vollständig bestehen lassen. M8-26b APFS-Desktop-E2E bleibt gemäß Reihenfolge bis M9-07 zurückgestellt.
- Die beobachtbare Laufzeitabhängigkeit von der Zahl verborgener Quellzeilen bleibt eine dokumentierte Grenze. Es gibt keine Constant-Time-Behauptung; ein realistischer Korpus und eine Produktentscheidung über die gemessene Restabweichung gehören zur M9-07-Abnahme.
- M9-04a/b/c-Fuzzkampagnen und ihre noch ausstehende plattformübergreifende Triage bleiben unabhängige offene Tasks.
