# ADR-041 – Eigenständige Berechtigung für Storageformat-Upgrades

**Status:** Angenommen für M7-10b; additive Kompatibilitätsänderung
**Entscheidungsebene:** Storageformat-Upgrade, getrennt von Schema-Migration und Backup/Restore
**Evidenz:** `docs/M7-10b-storage-format-upgrade.md`; `crates/worlddb-core/src/security.rs`; `crates/worlddb-storage-file/src/storage_upgrade.rs`

## Kontext

M7-07 verlangt eine explizite, separat autorisierte Aktion, bevor ein Storageformat-Upgrade ausgeführt wird. `MigrationExecute` bezeichnet das fachliche Schema-Migrationsprotokoll. `BackupCreate` und `BackupRestore` erlauben jeweils nur ihre Backup-/Restoreoperation. Keine dieser drei Berechtigungen darf implizit die Formatumstellung freigeben.

Der Security-Policy-Vertrag erlaubt neue Operationen mit einer expliziten additiven `Capability`-Variante und Kompatibilitätsprüfung. Die Runtime serialisiert Capability-Tags anhand ihrer Position in `Capability::ALL`; deshalb muss die neue Variante angehängt werden.

## Entscheidung

1. Das geschlossene Capability-Inventar erhält `StorageFormatUpgrade` als neue, eigenständig grantbare Berechtigung. Sie gilt für genau den Datenbank-/Projekt-Scope.
2. `Capability::StorageFormatUpgrade` wird als letztes Element in Enum und kanonischer `Capability::ALL`-Liste angefügt. Bestehende Capability-Tags bleiben unverändert; der neue Tag ist `77`.
3. Die Berechtigung wird keinem vorhandenen Standardrollen-Bundle automatisch hinzugefügt. Ein Upgrade benötigt zusätzlich `BackupCreate`, `BackupRestore`, einen zu Plan und Source gebundenen Safe-Restore-Point sowie eine explizite bestätigte Aktion.
4. `MigrationExecute`, Backup-/Restore-Berechtigungen und die Rolle `GM` implizieren `StorageFormatUpgrade` nicht. Das normale Öffnen, Recovery und Resave führen kein Upgrade aus.

## Kompatibilität

Vorhandene Policy-Records bleiben bytegleich lesbar; kein vorhandener Tag wird umnummeriert. Ein Record, der den neuen Tag `77` tatsächlich enthält, wird von einem älteren Binary als unbekannter Capability-Tag fail-closed abgewiesen. Ein älteres Binary darf solche Policy-Records nicht entfernen oder resaven. Damit ist die Berechtigung additiv, aber ein Downgrade nach ihrer ersten Speicherung ohne vorgelagerte explizite Policy-Änderung nicht unterstützt.

Die wirksame Rechtefingerprint-Reihenfolge wächst um eine Position. Storageupgrade-Aktionen binden den Fingerprint zum Bestätigungszeitpunkt und vergleichen ihn bei Ausführung und Resume erneut; eine geänderte Berechtigung macht die Aktion ungültig. Der neue Grant wird nicht in bereits bestehende Regeln hineingedeutet.

## Verifikation

M7-10b weist nach, dass fehlendes `StorageFormatUpgrade` die Ausführung ablehnt, Backup/Restore getrennt benötigt werden und bestätigte Upgradeaktionen an Plan, Restorebeleg, Akteur und aktuelle Rechte gebunden sind. Die Manifest-/Journal-Faulttests prüfen getrennt davon, dass kein ungeprüfter oder stiller Upgradepfad beim Open entsteht.
