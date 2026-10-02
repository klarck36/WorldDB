# M6-01 – Indexformat und Fallback

**Ergebnis:** PASS für das Format- und Auswahlprotokoll auf Windows. Geprüft am 2. Oktober 2026.

## Vertrag

Eine Indexgeneration ist ein abgeleitetes, versionsgebundenes Artefakt. Die geschlossene 1.0-Familienliste verwendet dauerhafte Wiretags für Record-ID, Operation-ID, Schema-ID/Revision, Lifecycle, Assertion-Point-/History, Assertion-Validity, Mask-/ContextPrecedence, Eventsuche, Eventrelation, Eventmasken und Provenance-Nachbarschaft.

Jede Generation trägt eine nichtnull Generation-ID, getrennte Schema-, Containerformat- und Builderversionen sowie einen inklusiven, lückenlosen Revisionsbereich. Umgekehrte Bereiche und reservierte Nullversionen werden abgewiesen. Eine Generation ist nur für eine Anfrage verwendbar, wenn Familie, alle drei Versionen und der angefragte Revisionspunkt genau passen.

Der Dateicontainer ist ein eigener WorldDB-Frame (`0x5744_4947`) mit streng geordneten TLV-Feldern. Der vorhandene BLAKE3-Framechecksum deckt Metadaten und Indexpayload ab. Erst nach vollständiger Frameprüfung, Feldvalidierung und Ressourcenlimits wird der Payload als verwendbar zurückgegeben. Der Codec begrenzt Frame- und Payloadgröße anhand der übergebenen `DecoderLimits`; unbekannte Felder, Familien, Frameversionen und ungültige Werte gelten als nicht verwendbar.

Fehlt die Datei, ist sie beschädigt, deckt sie die angefragte Revision nicht ab oder weicht eine Version/Familie ab, liefert die Auswahl ausschließlich `FullScan` oder – wenn das Full-Scan-Budget erschöpft ist – `BudgetExceeded` mit Budgetdimension. In keinem dieser Fälle gibt die API den Indexpayload heraus. Die tatsächliche Verknüpfung dieser Entscheidung mit produktiven Querypfaden folgt in M6-08; differential gleiche Queryresultate gegen das Full-Scan-Orakel bleiben M6-13 (`WDB-IDX-001`).

## Umsetzung

- `crates/worlddb-core/src/index_generation.rs`: geschlossene Indexfamilien, Versionstypen, Revisionsabdeckung und reine Kompatibilitäts-/Fallbackentscheidung.
- `crates/worlddb-storage-file/src/index_generation.rs`: versionierter Frame, Integritätsprüfung, endliche Codec-Limits und Payloadfreigabe nach positiver Prüfung.
- `WorldDB_1.0_Invariantenabdeckung.tsv`: Primärbelege für `WDB-IDX-002`; M6-08 ist als Integrationsfolgebeleg offen.

## Nachweise

- Core: 5 M6-01-Unit-Tests bestanden. Sie prüfen kompatible inklusive Grenzen, alle Abweichungsarten, Budgetfehler, reservierte Werte und geschlossene Wiretags.
- Dateiformat: 8 M6-01-Unit-Tests bestanden. Sie prüfen Roundtrip/Checksum, fehlende und beschädigte Dateien, veraltete Revisionen, Schemaabweichung, ungültige Frame-/Feldformen, Payloadgrenzen und leeren Payload.
- `cargo test --locked --workspace`: PASS für die Windows-Workspace-, Integrations- und Rustdoc-Tests. Die ausdrücklich ignorierten Decoder-Fuzz- und 100.000-Punkt-Crashkampagnen wurden nicht als bestanden gezählt.
- `cargo clippy --locked -p worlddb-core -p worlddb-storage-file --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo xtask verify` auf Windows: **34 PASS, 1 erwarteter SKIP, 0 FAIL**. Der SKIP ist der von M0-14 zurückgestellte externe `ci-matrix`-Lauf. Der Decoder-Langfuzzlauf und die separate 100.000-Punkt-Windows-Crashkampagne sind als ignorierte Langläufe ausgewiesen und wurden in diesem Durchgang nicht als bestanden gezählt.
- Workspace-Plancheck und Sourcecheck: PASS; die unveränderten Contract-Spiegel stimmen bytegenau.

## Grenze dieser Abnahme

Diese Task führt noch keine produktiven Sekundärindizes, atomaren Rebuilds oder Queryausführung ein; sie liefert das dauerhafte Format und die fail-safe Auswahlgrenze. Die queryseitige Ausführung von `FullScan` beziehungsweise des expliziten Budgetfehlers wird in M6-08 belegt. Das Windows-Ergebnis schließt die zurückgestellten Linux-/macOS- und M5-23-Nachweise nicht.
