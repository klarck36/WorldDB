# ADR-038 – WorldDB-Projektlizenz

**Status:** Accepted
**Entscheidungsdatum:** 2026-09-29
**Entscheidung:** Product Owner
**Umsetzung:** M0-09

## Kontex

M0-06 erfasste die Projektlizenz als unentschieden und verlangte eine ausdrückliche Wahl vor dem ersten Produktcode-/Dependency-Commit. Der Product Owner hat am 2026-09-29 festgelegt, dass WorldDB Open Source sein soll. Auf die anschließende Empfehlung für die in der Rust-Community übliche permissive Dual-Lizenz folgte die Bestätigung „opensource“.

## Entscheidung

WorldDB wird unter **MIT OR Apache-2.0** veröffentlicht. Nutzer dürfen die Bedingungen einer der beiden Lizenzen wählen. Cargo übernimmt den SPDX-Ausdruck in allen Workspace-Crates; das Repository enthält `LICENSE-MIT` und `LICENSE-APACHE`. Der MIT-Copyrightvermerk lautet `Copyright 2026 WorldDB contributors`.

Die Auswahl von Open Source erteilt keine Freigabe zur Veröffentlichung oder zum Push eines GitHub-Branches. Sie setzt die Lizenzbedingungen des Quellcodes fest. Dependency-Lizenzen, Advisories, Quellen und Feature-Policy bleiben Aufgabe M0-12.

## Begründung und Folgen

- MIT bietet eine kurze permissive Lizenz mit Erhalt des Copyright- und Lizenzhinweises.
- Apache-2.0 ergänzt eine ausdrückliche Patentlizenz und Regeln für weitergegebene Änderungen und Notices.
- `OR` in der Cargo-SPDX-Angabe erlaubt Empfängern, eine der beiden Lizenzen auszuwählen.
- Die Wahl macht WorldDB-Code weiterverwendbar und ändert nichts an der offenen Dependency-/Supply-Chain-Prüfung in M0-12.
- Vor einer Veröffentlichung ist zu prüfen, ob der generische Vermerk `WorldDB contributors` durch den tatsächlichen Rechtsträger ergänzt werden muss.

## Quellen

- [Cargo Manifest: SPDX-Lizenzausdrücke und `OR`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields)
- [MIT License – Open Source Initiative](https://opensource.org/license/mit)
- [Apache License 2.0 – Apache Software Foundation](https://www.apache.org/licenses/LICENSE-2.0.html)
