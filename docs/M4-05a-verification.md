# M4-05a – Autorisierte Projektmetadaten

**Status:** DONE, lokal gegen den In-Memory-Revisionbackend verifiziert am 30. September 2026.

## Ergebnis

`ProjectMetadataSnapshot` bindet HistorySpace-Katalog, Layerzustand, Entity-Katalog, Perspective-Historie und Schema an dieselbe veröffentlichte Revision. Der Snapshot lehnt inkonsistente Layerprojektionen, Entities mit unbekanntem EntityType und neue Entities mit bereits retired EntityType ab.

`validate_project_metadata_transaction` revalidiert am Commitrand die komplette Kandidatenhistorie. HistorySpaces bleiben unveränderlich und azyklisch; neue Child-Cutoffs dürfen weder vor der Parent-Basis liegen noch über den bereits veröffentlichten Head hinausreichen. Entity-, Perspective-, Layer- und Schemahistorien müssen append-only sein. Geänderte Schemawerte müssen die Commitrevision tragen. Der vollständige Satz der gestagten Metadatenrecords muss exakt dem Kandidatendelta entsprechen.

Aktuelle Policyrechte werden für jede gestagte Metadatenoperation erneut ausgewertet. Der Layerwechsel wird mit einer vollständigen `LayerSchemaSnapshot` geprüft: die neue Basis muss im selben Postzustand eindeutig niedrigster aktiver Layer sein; eine Änderung am ehemaligen Layer wird mitpubliziert, ohne bestehende Datenrecords umzuschreiben. Ein reiner Datencommit kann die Metadatensichten auf die neue gemeinsame Revision heben, ohne künstliche Metadatenrecords oder `LayerManage`-Recht zu verlangen.

## Nachweise

- `crates/worlddb-core/src/project_metadata_transaction.rs`: gemeinsame Metadatensicht, Postzustandsprüfung, aktuelle Operationsrechte und exakter Recorddelta-Abgleich.
- `crates/worlddb-core/src/transaction_flow.rs`: Validierung kann als Callback von `commit_mixed_record_batch` vor dem einzigen Publish aufgerufen werden.
- Fünf Tests in `project_metadata_transaction::tests`:
  - `one_authorized_transaction_publishes_metadata_and_atomic_base_switch`
  - `metadata_validator_runs_inside_one_publish_and_denial_leaves_head_unchanged`
  - `unpublished_child_cutoff_and_denied_metadata_rights_reject_candidate`
  - `data_only_revision_keeps_catalog_views_aligned_without_metadata_records`
  - `history_space_children_publish_without_mutating_schema_or_siblings`
- `cargo test --locked --workspace`: 329 Core-, 7 Testkit-, 2 Backend-Contract- und 79 Rustdoc-Tests bestanden.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `WorldDB_1.0_Plancheck.py`: 236 Tasks, 253 Invarianten und 169 Folgebelegpaare valide.
- `cargo xtask verify`: 30 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL.

## Abgrenzung

Die Tests belegen den Engine-Validierungs- und In-Memory-Publishpfad. Dauerhafte/crashsichere Publication ist Sache eines konkreten Backends. Der aufrufende Writer muss diesen Metadatenvalidator im vollständigen Transaction-Callback mit M4-04c und weiteren betroffenen Domainvalidatoren zusammensetzen. OCC-Revalidierung gegen einen inzwischen veränderten Head folgt M4-06/M4-07.
