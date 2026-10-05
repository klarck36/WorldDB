# M8-27 – Windows-/Local-Gate-Abnahme

**Datum:** 2026-10-06
**Ergebnis:** BESTANDEN im vereinbarten Windows-/Local-Umfang
**Geltungsbereich:** Windows 11 / NTFS

## Prüfumfang und Ergebnis

| Kriterium | Ergebnis und Nachweis |
| --- | --- |
| Gemeinsame API-, Security- und Storage-Semantik für CLI und Desktop | Die Windows-Verifikationen für Projektoperationen, Backup/Restore, Migration, Export/Import und Purge sind in `docs/M8-04-verification.md` bis `docs/M8-07-verification.md` dokumentiert. CLI- und Desktopaufträge verwenden die typisierten Engine-/Storagegrenzen; Policy, Audit, Konflikte und Fehler bleiben an denselben Verträgen gebunden. |
| Native Windows-Bedienung der lokalen Workflows | Die Tasknachweise M8-01 bis M8-25 sind im Taskregister DONE. Sie decken Projektanlage/-öffnung, Schema und Domainrecords, Abfragen, Sicherheit, Jobs, Recovery, Migration, Backup/Restore, Export/Import und Diagnose ab. Die produktspezifischen Windows-Nachweise stehen in den jeweiligen `docs/M8-*-verification.md`- und `docs/M8-*-windows-progress.md`-Dateien. |
| Keine Storageautorität im Renderer | `experiments/ode-002/crates/desktop-shell/frontend/main.js` ruft typisierte Hostkommandos über `invoke` auf. Der direkte Dateisystem-Pluginaufruf steht ausschließlich in der Negativprobe und wird abgewiesen. `docs/M8-09-windows-progress.md` dokumentiert die Ablehnung von Rendererpfad, ungültiger Sitzung und Dateisystemplugin in In-Process und Sidecar sowie das Fehlen eines Netzwerklisteners. |
| Native Ende-zu-Ende-Abdeckung | Der kataloggesteuerte Lauf auf sauberem Commit `f065583` bestand mit **11/11 Fällen und 140/140 Prüfpunkten**, ohne Fehler oder ausgelassene Fälle. Manifest: `experiments/ode-002/evidence/native-e2e/m8-26a-20261005T214149Z-8fe0f706/manifest.json`; JSON-Schema, Hashes und Byteumfänge aller 11 Artefakte wurden geprüft. |
| Windows-Verifikationsprofil | `cargo xtask verify` bestand mit **41 PASS, 1 erwartetem `ci-matrix`-SKIP, 0 FAIL**. Plancheck, Sourcecheck, Contract-Source-Verifikation, Contract-Docs-Verifikation und Arbeitsbaum-Whitespaceprüfung bestanden ebenfalls. |
| Invariant-Folgebelege | Die 20 bereits vorhandenen Windows-Folgebelege für M8-09, M8-11a und M8-14b bis M8-14f sind paarweise in `WorldDB_1.0_Folgebelege.tsv` mit ihren Test-/Run-Artefakten erfasst. Die zusätzlichen 18 plattformübergreifenden Folgepaare sind M9-07 zugeordnet. |

## Zurückgestellte Plattformabnahme

Gemäß Nutzervorgabe wurden keine macOS/APFS- oder Linux/ext4-Läufe ausgeführt. M8-26b und M8-26c sowie die 18 plattformübergreifenden Folgepaare bleiben offen und sind als Eingaben für M9-07 eingetragen. M9-07 hängt von M9-06 und beiden Plattformläufen ab; das M8-Ergebnis erteilt daher keine plattformübergreifende RC-Freigabe.

Der M8-Vorabcheck bestand mit dem ausdrücklich begrenzten Windows-/Local-Scope. Die weiterhin offenen M0-CI- und M5-Plattformvoraussetzungen bleiben gemäß ihrem bestehenden Plan für spätere Release-Gates sichtbar.
