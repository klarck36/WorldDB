# M1-14 – Geschlossenen RecordRef vervollständigen

**Status:** DONE\
**Datum:** 29. September 2026\
**Taskregister:** `WorldDB_1.0_Taskregister.tsv`

## Ergebnis

`crates/worlddb-core/src/record_refs.rs` enthält die geschlossenen Referenztypen und re-exportiert sie über `crates/worlddb-core/src/lib.rs`.

- `RecordRef` hat 24 konkrete Varianten: die 21 Varianten aus Master §3.3 einschließlich EventRelation sowie EntityRetirement, PerspectiveRetirement und ArchiveTransition aus den beschlossenen Ergänzungen. Jede Variante hat einen eigenen festen `RecordRefWireTag`; die append-only Zuordnung ist in `policy/record-ref-wire-tags.tsv` festgehalten. Unbekannte numerische Tags werden abgewiesen.
- Validierende `TryFrom<RecordRef>`-Konversionen liefern die geschlossenen Teilmengen: EvidenceTargetRef 21 Varianten, ProvenanceEndpointRef 23, LifecycleTargetRef 8, ArchiveTargetRef 23 und EventRelationProvenanceRef 2. Jede Konversion prüft alle 24 Eingangsvarianten. ArchiveTransition ist gemäß der allgemeinen Lifecycle-Endpoint-Regel Evidence-/Provenance-fähig, kann aber keine ArchiveTransition auf sich selbst erzeugen.
- Schema-, Migration-, Security-, Transaction-, Snapshot-, Job- und Audit-Referenzen besitzen eigene geschlossene Typen. SecurityPolicyRecordId, PrincipalId, RoleId, RoleAssignmentId und PolicyRuleId bleiben unterscheidbar; es gibt keine `Other`- oder String-Variante.
- `DatabaseBoundRef<T>` bindet eine Referenz an DatabaseId. Der versiegelte `DatabaseReference`-Trait lässt nur die registrierten geschlossenen Referenztypen zu.
- Rustdoc-Compile-Fail-Belege sperren offene Varianten, operative IDs in RecordRef, freie Strings als Datenbankreferenzen und eine implizite SchemaRef-zu-RecordRef-Konversion.

M1-13s Bestandsbelege wurden nachgetragen: ArchiveTransition ergänzt nun EvidenceTargetRef und ProvenanceEndpointRef. Die Corrects-Matrix umfasst 23×23 = 529 Paare; 23 gleichartige Variantenpaare sind strukturell kompatibel, 506 ungleichartige Paare werden abgewiesen. Assertion-Slot-, EventKind- und Transaction-Time-Kompatibilität benötigen weiter den historischen Kontext aus M2-15a.

## Prüfungen

- `cargo fmt --all` — bestanden.
- `cargo test --locked --offline --workspace --all-targets` — 90 Tests bestanden (79 Core-, 7 Testkit- und 4 xtask-Tests).
- `cargo test --locked --offline --doc --package worlddb-core` — 56 Rustdoc-Tests bestanden, einschließlich der neuen Compile-Fail-Belege für alle getrennten Referenzfamilien.
- `cargo xtask verify` — 27 PASS, 1 sichtbarer erwarteter `ci-matrix`-SKIP für das wegen der zurückgestellten GitHub-/macOS-Integration offene M0-14, 0 FAIL.
- `python -B -X utf8 WorldDB_1.0_Plancheck.py` — bestanden: 236 Tasks, 11 Milestones, 253 Invarianten und 159 Folgebelegpaare.
- `git diff --check HEAD` — bestanden; keine Whitespacefehler.

## Abgrenzung

Codec-, Query- und Import-Negativfälle folgen M1-17d, M3-09 und M7-13. Die historische Corrects-Kompatibilität folgt M2-15a. M0-14 bleibt bis zur späteren Git-/CI-Integration offen. Nächster lokaler Planpunkt ist M1-15.
