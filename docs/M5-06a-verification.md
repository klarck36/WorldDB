# M5-06a – Persistente Security- und Audit-Policyhistorie

**Ergebnis:** DONE – Windows-lokal geprüft am 1. Oktober 2026. Linux- und macOS-Prüfungen bleiben auf Wunsch des Product Owners zurückgestellt.

## Umsetzung

`worlddb-storage-file::SecurityPolicyHistoryStore` speichert pro gemeinsamer `Revision` ein unveränderliches Segment im Verzeichnis `security/segments`. Das Segment verwendet dieselbe Dateihülle, zufällige `SegmentId`, BLAKE3-`ContentDigest` und kanonische Offset-/Längenindexstruktur wie M5-06. Ein Segment enthält genau einen begrenzten Security-Frame.

Der Frame persistiert eine vollständige `SecurityPolicySnapshot` mit Principals und ihren Zuständen, Roles samt Capability-Bundles, RoleAssignments mit exaktem Scope sowie direkten und rollenbasierten CapabilityRules. Er enthält außerdem `SecurityEpoch`, die optionale `SecurityPolicyRecord`-Änderungsmenge und die an dieser Revision geltende optionale `AuditRetentionPolicy`. Jede Identität bleibt ihr registrierter typisierter 128-Bit-ID-Typ; `SecurityPolicyRecordId` ist dafür im Core-API verfügbar. Domain-`RecordRef` wird nicht als Identität für Security-Objekte verwendet. Nur wenn eine Policy-Scope-Dimension ausdrücklich auf einen Domainrecord zeigt, kommt der separate Core-Codec für diesen Scopewert zum Einsatz.

Enum- und Änderungsvarianten besitzen geschlossene Tags. Capability-Tags folgen der kanonischen, geschlossenen `Capability::ALL`-Tabelle; der Roundtrip deckt alle 77 Einträge ab. Decoding prüft UUID-Formen, Versions- und Epochwerte, RecordRef-Scopes, Symbolgrammatik, Audit-Aufbewahrungsgrenzen, Sammlungs-/Framebudgets und die kanonische ID-Reihenfolge. Die Snapshot- und Änderungsrecord-Konstruktoren im Core prüfen zusätzlich doppelte IDs und referenzielle Konsistenz.

Der Loader ordnet die angegebene Segmentmenge nach Revision und verlangt Genesis sowie eine lückenlose Abdeckung bis zur angegebenen committed Revision. Bei unveränderter Epoch muss der Snapshot gleich bleiben und es darf kein SecurityPolicyRecord vorliegen. Eine fortgeschriebene Epoch braucht genau im entsprechenden Segment einen PolicyRecord, dessen `recorded_revision` und `security_epoch_after` zur Version passen. Fehlende, doppelte oder unpassende Historie wird abgewiesen. Core `SecurityPolicyHistory` lehnt zusätzlich Policy-Snapshotwechsel ohne Epoch-Fortschritt ab.

## Nachweise

- `test:complete_security_snapshot_epoch_and_audit_configuration_roundtrip` – Principalzustände, Role, Capability-Bundle, Assignment-Scope, Rule, Epoch und Audit-Aufbewahrung lokal roundtrippen.
- `test:complete_closed_capability_catalog_roundtrips_with_stable_tags` – alle 77 Capability-Tags roundtrippen.
- `test:all_typed_policy_change_variants_roundtrip_without_domain_record_encoding` – alle acht SecurityPolicyChange-Varianten bleiben als typisierte Records erhalten.
- `test:history_reconstruction_fails_closed_when_any_revision_snapshot_is_missing` – fehlende Zwischenrevision wird geschlossen abgewiesen; vollständige Historie liefert die versionsgebundene Audit-Konfiguration und PolicyRecord.
- `test:an_epoch_advance_requires_a_policy_record_bound_to_the_same_revision` – fehlender Änderungsrecord bei Epoch-Fortschritt und ein Record mit falscher Revision werden abgewiesen.
- `test:an_empty_or_missing_genesis_snapshot_does_not_open_policy_history` – leere Historie kann nicht geöffnet werden.

## Grenzen

Der Aufrufer schreibt je gemeinsamer Revision den vollständigen Policy-Snapshot und referenziert das Segment später gemeinsam mit Domainsegmenten aus demselben Manifest. Manifestpublikation und CURRENT gehören zu M5-07. Neustart-Replay und Cursorinvalidierung folgen M5-12/M5-22. Diese Task behauptet keine atomare Required-Audit-Commitgarantie; der gemeinsame WAL-Commitpoint für Pflicht-Audit bleibt Gegenstand von M5-17. Directory-Sync und Machine-Durability sind nicht durch den Segment-Write allein belegt. Linux/ext4- und macOS/APFS-Läufe wurden nicht ausgeführt.

## Windows-Prüflauf

`cargo test --locked --workspace`: 396 Core-Unit-Tests, 30 Storage-File-Tests, 7 Testkit-, 5 Backend-Contract-, 4 xtask- und 82 Rustdoc-Tests bestanden; zwei CPU-Langläufe bleiben absichtlich ignoriert. `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, Plancheck, Sourcecheck, Crategraph-/Dependency-Policy, `cargo-deny check all` und `git diff --check HEAD` bestanden. `cargo xtask verify`: 32 PASS, 1 erwarteter M0-14-`ci-matrix`-SKIP, 0 FAIL. M0-13-Evidenzlauf `M0-13-20261001T134615Z-5f70dea40f` ist PASS. Diese Ergebnisse belegen Windows; Linux- und macOS-Prüfungen wurden nicht ausgeführt.
