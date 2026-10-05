# M8-25 — Update- und Formatkompatibilität

**Ergebnis:** bestanden auf Windows für In-Process und Sidecar. Linux/macOS bleiben gemäß Projektentscheidung bis M9-07 zurückgestellt.

## Öffnen und Migration

- `DatabaseLayout::open` prüft vor dem Enginezugriff Layout und `FORMAT`-Fähigkeiten. Der Open-Pfad führt eine unabhängige read-only Storage- und Recoveryprüfung aus und lehnt nicht saubere Projekte ab, statt sie beim Öffnen zu reparieren.
- `ManifestStore::read_current_with_format` liest Manifest und `CURRENT`-Version aus demselben geprüften Pointer-Frame. Die Desktop-Projektstatusmeldung weist `CURRENT v1` oder `CURRENT v2`, die ausdrückliche Upgradepolitik und die ausdrückliche Schemakonvertierungspolitik aus.
- Projekt öffnen führt kein Storageformat-Upgrade und keine Schemakonvertierung aus. Die Anwendung hat keine globale Mindest-Schemaversion, die ein appgesteuertes Upgrade erzwingt. Fachliche Schema-Migrationen bleiben eigene, bestätigte Aktionen.
- Die Desktopkonfiguration aktiviert kein Paket-Bundle und enthält keinen App-Updater. Der Nightly-Kanal hat laut Masterplan keine Kompatibilitätszusage. Ein künftiger Updater muss einen Kompatibilitätscheck vor der Installation anbieten; eine Datenmigration darf er niemals still auf der einzigen Nutzerdatenbank starten. Nightly-Prüfungen sind an einer Kopie oder nach verifiziertem Backup vorzunehmen.
- Das lokale Sidecar-Austausch-Szenario startet nach dem ordentlichen Ende des vorherigen Prozesses eine neue Engine auf demselben Projekt und prüft die erneute Writerlockübernahme. Der Engine-Start läuft dabei durch denselben Format-, Manifest-, Rechte- und Recoverycheck. Es ist kein Storage- oder Schemaupgrade.

## Portable Lock- und Secret-Grenzen

- Der exklusive Writerlock ist ein nicht wartender Betriebssystem-Dateilock auf der stabilen `LOCK`-Datei. Die Datei bleibt nach Freigabe bestehen; das Betriebssystem gibt den Besitz beim Schließen des Handles frei. Kooperierende WorldDB-Prozesse serialisieren dadurch den Schreibzugriff.
- Der read-only Lockpfad verlangt eine bereits vorhandene reguläre `LOCK`-Datei und kann nicht zu Recovery-/Schreibzugriff hochgestuft werden. Symlinks und Pfade außerhalb des Datenbankverzeichnisses werden abgewiesen. Unter Windows erlaubt das Öffnen des Lockhandles parallele Lese-, Schreib- und Löschhandles; die native Abnahme belegt das Windows-Verhalten.
- Netzwerkfreigaben und Cloud-Sync-Ordner erhalten keine portable Lockgarantie, solange ihre konkreten Dateilock- und Durabilitysemantiken nicht separat geprüft wurden. Diese Plattformabnahmen bleiben bis M9-07 zurückgestellt.
- Für Backup-MAC-Schlüssel ist kein portabler Betriebssystem-Schlüsselspeicher angebunden. Das Desktop-Backupformular nimmt keinen solchen Schlüssel entgegen. Wo der API-Schlüssel verwendet wird, bleibt das Geheimnis im vom Aufrufer verwalteten Prozessspeicher; in der Backupmanifestdatei steht nur die Schlüssel-ID, und `Debug` maskiert den Schlüssel. Eine Speicherlöschung/Zeroization wird nicht zugesichert.

## Nachweise

- Engine-Test `project::tests::opening_reports_current_format_without_rewriting_the_pointer`: bestanden; neuer Projektstand wird als `CURRENT v1` erkannt und `CURRENT` bleibt nach erneutem Öffnen bytegleich.
- Storageupgrade-Test `storage_upgrade::tests::current_upgrade_reopens_at_source_or_target_and_resumes_every_publish_boundary`: bestanden; nach dem bestätigten Upgrade meldet derselbe Manifestlesepfad `CURRENT v2`.
- Desktoptests: 36 In-Process und 37 Sidecar bestanden.
- Striktes Clippy (`-D warnings`): In-Process und Sidecar bestanden.
- Nativer Windows-Zwei-Fenster-Smoke: In-Process bestanden, 56 Prüfpunkte PASS, einschließlich `CURRENT v1`, ausdrücklicher Migrationspolitik und `migration_applied_during_open=false`.
- Nativer Windows-Zwei-Fenster-Smoke: Sidecar bestanden, dieselben 56 Prüfpunkte PASS.
- `cargo fmt --all -- --check`, Node-Syntax, PowerShell-Parser, Plancheck, Sourcecheck und `git diff --check`: bestanden.
- `cargo xtask verify`: 39 PASS, 1 erwarteter `ci-matrix`-SKIP, 0 FAIL.
