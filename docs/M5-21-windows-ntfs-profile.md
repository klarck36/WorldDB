# M5-21 – Windows/NTFS-Schreibprofil für ODE-004

**Ergebnis:** Windows-Profilbasis festgelegt am 1. Oktober 2026. Dieses Ergebnis wählt einen Kandidaten für M5-22; es erteilt noch keine Produktions- oder Machine-Durability-Freigabe.

## Aufgenommene Umgebung

- Betriebssystem: Windows 11 Home, Version `10.0.26200`, Build `26200`.
- Zielplattform der lokalen Rust-Toolchain: `x86_64-pc-windows-msvc`, Rust `1.85.0`.
- Testvolume: `C:`, lokales festes NTFS-Volume, HealthStatus `Healthy`.
- Vom System gemeldeter Datenträger: Samsung SSD 970 EVO Plus 1TB, NVMe. Firmwarezustand, Geräte-/Controller-Schreibcache und Power-Loss-Protection wurden nicht gemessen.
- Storage-Integrationstests schreiben ihre temporären Daten unter `C:\Users\wedde\AppData\Local\Temp\` auf demselben NTFS-Volume. Der Projektcheckout liegt dagegen unter OneDrive und qualifiziert diesen synchronisierten Pfad ausdrücklich nicht.

## Gewählter Kandidat

`windows_ntfs_fixed_local_v1` umfasst nur lokale, feste NTFS-Volumes auf dem Windows-x64-Pfad. Für einen solchen Pfad setzt der Dateiadapter folgende Operationen ein:

- WAL-Prepare, Commitmarker, Segmente und gestagte Manifeste werden mit `std::fs::File::sync_all()` synchronisiert. Auf Fehler folgt kein erfolgreicher Commit; `sync_all` versucht, OS-interne Datei- und Metadaten vor Rückkehr an das Dateisystem zu übergeben ([Rust-Dokumentation](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all)).
- `CURRENT` wird ausschließlich innerhalb desselben Datenbankvolumes über `MoveFileExW` mit `MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING` ersetzt. Unveränderliche Manifestgenerationen verwenden denselben Write-through-Publish-Pfad ohne Replace. Verzeichnis-Sync wird mit einem Windows-Verzeichnis-Handle (`FILE_FLAG_BACKUP_SEMANTICS`) und `sync_all()` angefordert; ein Fehler verhindert die erfolgreiche Rückmeldung.
- Microsoft dokumentiert für `MOVEFILE_WRITE_THROUGH`, dass der Aufruf erst nach dem abgeschlossenen Move zurückkehrt; die ausdrücklich dokumentierte Flush-Zusage bezieht sich auf den Copy-/Delete-Fall. Dieser Adapter verwendet gleichvolumige Pfade und behauptet aus dem Flag allein keine Hardware- oder Stromausfalldurabilität ([Microsoft-Dokumentation zu `MoveFileExW`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)).
- Das stabile Writer-Lockfile wird über `fs4::FileExt::try_lock()` exklusiv und ohne Warten gesperrt. Der Windows-Unterbau verwendet `LockFileEx` mit `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`; der Handle bleibt für die Datenbanklebensdauer offen. Windows hebt den Lock beim Schließen des Handles auf ([Microsoft-Dokumentation zu `LockFileEx`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)).

Die bestehenden Windows-Verträge in `docs/M5-09-verification.md` belegen lokale Pfadnormalisierung, UNC-Ablehnung, Writer-Lock, Sharing-Konflikt und Pointer-Replacement. M5-22 ergänzt die deterministische Crashmatrix für dieses Profil.

## Grenzen und nicht freigegebene Pfade

- Das Profil behauptet derzeit **keine** Machine-Durability und keine bestätigte Persistenz bei echtem Stromausfall. Die vorhandenen Sync-Aufrufe und Prozess-/Fehlerinjektionstests ersetzen weder eine Hardware-Power-Cut-Messung noch einen Nachweis über den NVMe-Controller-Cache. `DurabilityLevel::Machine` bleibt bis zu den einschlägigen Implementierungs- und Gate-Nachweisen gesperrt.
- OneDrive- und sonstige synchronisierte Verzeichnisse sind wegen ODE-004 ausgeschlossen. Das gilt auch dann, wenn die zugrunde liegende Partition NTFS ist. UNC/SMB-/Netzwerkpfade werden vom Windows-Publishadapter fail-closed abgelehnt.
- APFS und ext4 bleiben read-only/unsupported, bis M4-15, M5-08 und M5-10 auf den passenden Hosts belegt sind. ReFS, FAT/exFAT, Wechseldatenträger und sonstige nicht profilierte Volumes erhalten ebenfalls keine Schreibfreigabe.
- ODE-006 betrifft den macOS-`F_FULLFSYNC`-Pfad. Der Windows-Kandidat trifft dazu keine APFS-Entscheidung; M4-15 bleibt für einen späteren Mac-Lauf offen.

## Abnahme für den Windows-Slice

- Host- und Volumeinventar oben wurde lokal read-only erhoben.
- M5-09 Windows-/NTFS-Verträge: PASS; siehe `docs/M5-09-verification.md`.
- Profilkennung, erlaubte Umgebung, Sync-/Lock-Mechanismen, Grenzen und explizit ausgeschlossene Dateisysteme sind vor M5-22 dokumentiert.
- Die Freigabe bleibt auf Windows/NTFS begrenzt; die Plattformabnahmen M4-15, M5-08, M5-10 und das finale M5-Gate M5-23 bleiben offen.
