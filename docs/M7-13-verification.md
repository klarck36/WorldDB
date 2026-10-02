# M7-13 – Verifikationsnachweis

## Umfang

M7-13 ergänzt einen kanonisch codierten `LogicalImportPlan`, eine typisierte Remaptabelle, einen Identitäts-Snapshot des Ziels und `LogicalImportManager::prepare`. Der Plan bindet die exakten Exportbytes und beide DatabaseIds. Vorbereitung prüft Remap-Familien, Kollisionen, Ziel-IDs, exportinterne Abhängigkeiten sowie Record-, Schema-, HistorySpace- und Timeline-Referenzen gegen den geplanten Importbestand und das Zielinventar.

Der Import bleibt bis zum nachgelagerten Adapter-/Storage-Vertrag nicht persistent. Die zentrale Abbildung gilt für Recordidentitäten und deren Referenzen; der deterministische `stream_fingerprint` bindet Quellbytes und den kanonischen Plan.

## Gezielte Fälle

- `logical_export::tests::logical_import_requires_and_replays_explicit_collision_remaps`: Kollision ohne Plan wird abgewiesen; gleicher Export und Plan ergeben wiederholt denselben Fingerprint; Record-Remap ist typisiert.
- `logical_export::tests::logical_import_rejects_unlisted_or_occupied_remap_targets`: Quelle außerhalb des Exports und belegtes Remap-Ziel werden abgewiesen.
- `logical_export::tests::logical_import_rejects_missing_schema_references`: ein importiertes Entity ohne vorhandene oder mitimportierte EntityType-Definition wird abgewiesen.

## Windows-Prüfergebnisse

- `cargo test --workspace --locked --quiet`: PASS; 501 Core-Tests, 67 Storage-File-Tests und 84 Rustdoc-Tests bestanden. Vorgesehene manuelle/lang laufende `#[ignore]`-Fälle wurden nicht gestartet.
- `cargo clippy --workspace --locked --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo check --workspace --locked --all-targets`: PASS.
- `WorldDB_1.0_Plancheck.py`: PASS; 243 Tasks, 11 Milestones, 253 Invarianten und 177 Folgebeleg-Paare.
- `WorldDB_1.0_Sourcecheck.py`: PASS; geprüfte Quelldateien und Audit-ZIP stimmen bytegenau.
- `cargo xtask verify`: 38 PASS, 1 erwarteter `M0-14 ci-matrix`-SKIP, 0 FAIL.
- `git diff --check HEAD`: PASS.

Linux/macOS sind auftragsgemäß nicht Teil dieses Laufs und bleiben für M9-07 zurückgestellt.
