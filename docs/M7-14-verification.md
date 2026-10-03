# M7-14 – Windows-Verifikation

**Stand:** 2026-10-03  
**Host:** Windows  
**Ergebnis:** bestanden; Linux/macOS-Nachweise folgen gemäß Projektentscheidung in M9-07.

## Umsetzung

- `PurgePlanManager::preview` decodiert den kanonischen Quell-Export, verlangt den vollständigen
  Revisionsbereich, alle revisionstragenden Recordklassen und alle im Export manifestierten
  HistorySpaces und bindet den Plan an DatabaseId, Snapshotrevision und Exportdigest.
- Der Purge-Graph verwendet dieselbe typisierte Referenzauswertung wie der Import. Er inventarisiert
  Datensätze und berechnet eine transitive, zyklussichere Abhängigkeitsmenge für Schema, History,
  Assertions, Masken, Boundaries, Events, Lifecycle, Evidence, Provenance und TransferLineage.
- `RejectIfReferenced` scheitert bei Abhängigkeiten. Eine `PurgeCascadePlan` wird nur angenommen,
  wenn sie exakt der vollständigen berechneten Menge entspricht. Der Fingerprint bindet Quelle,
  Ziele, betroffene Datensätze, Index-/Extrakopieninventar und Freigabemodus.
- `IndexGenerationStore::inventory_all` scannt unter dem Datenbank-Lock alle Indexdateien,
  validiert Generationen und aktuelle Zeiger und scheitert bei unbekannten Dateien, fehlenden
  Indexverzeichnissen, fehlerhaften Generationen oder dangling pointers. Ein vollständiges
  Inventar lässt sich nicht durch eine frei gesetzte Vollständigkeitsmarkierung erzeugen.
- Bekannte Backups und Exporte erscheinen im Plan als externe, weiterbestehende Kopien. Ein
  unvollständiges externes Suchinventar bleibt sichtbar. Der Plan verspricht kein Secure Erase.

## Automatisierte Nachweise

- `purge::tests::dependant_closure_is_transitive_and_cycle_safe` prüft Transitivenabschluss und
  Zyklusende.
- `logical_export::tests::purge_plan_rejects_implicit_cascade_and_binds_the_exact_approved_set`
  prüft unvollständige Exporte, RejectIfReferenced, ausgelassene Cascade-Einträge,
  Indexinventar-Vollständigkeit, deterministischen Fingerprint und gemeldete externe Backups.
- `index_rebuild::tests::purge_inventory_includes_stale_and_current_generations_and_rejects_unknown_files`
  prüft aktuelle und veraltete Generationen sowie fail-closed Behandlung unbekannter Dateien.
- `index_rebuild::tests::purge_inventory_treats_an_uncreated_optional_index_directory_as_empty`
  prüft, dass ein noch nicht angelegtes optionales Indexverzeichnis als leeres Inventar gilt.
- `m7_14_purge_plan_contract::purge_plan_reports_references_and_requires_the_exact_cascade_without_writing`
  erstellt eine echte Dateidatenbank, exportiert einen vollständigen Snapshot und belegt, dass
  die Planvorschau Quell-DatabaseId und Commitrevision unverändert lässt.

## Prüfläufe

- `cargo test --locked --workspace` – bestanden; ein bereits registrierter langer Windows-NTFS-
  Crashlauf bleibt standardmäßig ignoriert.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` – bestanden.
- `cargo fmt --all -- --check` – bestanden.
- `cargo check --locked --workspace --all-targets` – bestanden.
- `WorldDB_1.0_Plancheck.py` und `WorldDB_1.0_Sourcecheck.py` – bestanden.
- `cargo xtask verify` – 38 PASS, 1 erwarteter `M0-14 ci-matrix`-SKIP, 0 FAIL.
- `git diff --check HEAD` – bestanden.

Die Vorschau führt noch keinen Offline-Rewrite aus. M7-15 muss den aktuellen vollständigen
Quellsnapshot erneut verifizieren, eine neue DatabaseId vergeben, das Ziel prüfen und die
erforderliche Audit-Publikation als All-or-Nothing-Grenze behandeln.
