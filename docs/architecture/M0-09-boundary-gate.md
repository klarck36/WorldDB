# M0-09 – Initialer Crategraph und Boundary-Gate

**Status:** Boundary-Gate dokumentiert; Projektlizenz für den Produktcode beschlossen.

## Entscheidung

Der Workspace bildet die in Master §25 ausdrücklich genannte Startstruktur ab. `worlddb-core` hält Domain-, Resolution- und Engine-Module zusammen. Es werden vorerst keine eigenen `worlddb-domain`, `worlddb-resolution`, `worlddb-api`, `worlddb-observability` oder Port-Crates extrahiert. Reine Dateigröße oder ästhetische Schichtung rechtfertigen keine weitere Crate.

| Crate | Anfangsrolle | Erlaubte Workspace-Abhängigkeiten |
|---|---|---|
| `worlddb-core` | Domain, Resolution und Engine als Module; keine I/O-Abhängigkeit | keine |
| `worlddb-storage-file` | isolierter Dateisystemadapter; künftige Storage-Port-Schnittstelle bleibt beim Core/Engine-Owner | `worlddb-core` |
| `worlddb-cli` | Präsentationsschicht für Engine und den File-Adapter | `worlddb-core`, `worlddb-storage-file` |
| `worlddb-testkit` | gemeinsame Testhilfe, kein Produkt-Rückweg | `worlddb-core` |
| `xtask` | Repositoryautomation; M0-10 ergänzt den kanonischen Verify-Befehl | keine |

Der File-Adapter ist als eigene Crate vorgesehen, weil die geplante Plattform-/Dateisystemgrenze isoliert gebaut und geprüft werden muss. Diese erste Crateaufteilung ist die ausdrücklich erlaubte M0-09-Startstruktur und bleibt eine Hypothese, keine dauerhafte Architekturvorgabe. Ein konkreter Storage-Port und Laufzeitverhalten werden hier noch nicht erfunden. Jede weitere Extraktion braucht vor ihrer Umsetzung ein eigenes Boundary-Gate mit realem Zyklus-, Adapter-, MSRV/API-, Unsafe- oder Compilezeitnachweis.

`worlddb-core` verbietet `unsafe` per Crateattribut. `worlddb-storage-file` verwendet `deny(unsafe_code)`, damit M0-11 später eine eng begründete und getestete Plattformausnahme zulassen kann, falls der Storagevertrag sie erfordert; derzeit enthält der Adapter kein `unsafe`.

## Durchsetzbarer Graph

`tools/check_crate_graph.py` prüft `cargo metadata` des tatsächlichen Workspaces. Es akzeptiert nur die fünf Startcrates und die obige Abhängigkeitsrichtung; in M0-09 sind zusätzlich alle externen Abhängigkeiten verboten. `tools/test_crate_graph.py` prüft die Policy mit Positiv- und Negativproben: eine Rückkante `worlddb-core -> worlddb-storage-file`, eine externe Dependency und eine unbeschlossene `worlddb-resolution`-Crate werden abgewiesen.

## Lokale Prüfung

- `cargo check --locked --workspace --all-targets` – bestanden mit Rust 1.85.0; alle fünf Workspace-Mitglieder und Targets geprüft.
- `python -X utf8 tools/check_crate_graph.py` – bestanden; der reale Metadaten-Graph entspricht der obigen Tabelle.
- `python -X utf8 -m unittest discover -s tools -p 'test_crate_graph.py' -v` – vier Policytests bestanden, einschließlich drei absichtlich unzulässiger Graphen.
- Die Builds liefen unter Windows x86_64 MSVC. `CARGO_TARGET_DIR` zeigt auf `%LOCALAPPDATA%\WorldDB\test-runs\M0-09\target`, außerhalb des OneDrive-Projektpfads.

## Lizenzgrenze

Die M0-06-Inventur verlangte eine ausdrückliche Produktlizenzentscheidung vor dem ersten Produktcode-/Dependency-Commit. Der Product Owner hat am 2026-09-29 „opensource“ gewählt; umgesetzt wird die zuvor empfohlene Dual-Lizenz `MIT OR Apache-2.0`. Alle fünf Crates erben denselben SPDX-Ausdruck aus dem Workspacemanifest; `LICENSE-MIT` und `LICENSE-APACHE` enthalten die beiden Lizenztexte. Der MIT-Copyrightvermerk lautet `Copyright 2026 WorldDB contributors`.
