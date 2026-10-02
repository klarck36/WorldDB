# M7-13 – Logischer Import und ID-Remap

## Zweck und Abgrenzung

`LogicalImportManager::prepare` prüft ein kanonisches Logical-Export-Artefakt gegen einen expliziten Importplan und einen Identitätsbestand der Zieldatenbank. Der Schritt verändert noch keine Datenbank. Er erzeugt einen reproduzierbaren, typisierten Importplan, den ein späterer Adapter ausführen kann.

## Importplan und Remaps

`LogicalImportPlan` bindet die exakten Exportbytes über einen domain-separierten BLAKE3-Digest, die Quell-DatabaseId, die Ziel-DatabaseId und eine kanonisch sortierte Remaptabelle. Ein Remap darf nur innerhalb derselben Identitätsfamilie erfolgen; der Decoder weist nichtkanonische Reihenfolge, doppelte Quellen/Ziele, falsche Familien, geänderte Bytes und überschrittene Größenlimits zurück.

Die Remaptabelle deckt HistorySpaces, Layer, Perspectives, Timelines, Entities, Schema-IDs und typisierte `RecordRef`s ab. Importierte Identitäten, die schon im Zielbestand liegen, benötigen einen expliziten Remap. Jedes Remap-Ziel muss im Ziel frei sein und zwei importierte Identitäten dürfen nicht auf dasselbe Ziel zeigen. Unveränderte externe Referenzen müssen dagegen im Zielbestand vorhanden sein. Ziel-IDs werden nie stillschweigend übernommen.

## Referenz- und Schemaidentitäten

Vorbereitung validiert Lifecycle-, EventRelation- und Archive-Abhängigkeiten innerhalb des Exports sowie direkte Referenzen zwischen importierten Records und Schema-IDs. Dazu zählen HistorySpace-Eltern, Entity→EntityType, Entity-/Perspective-Retirements, Assertion-/Mask-/Event-Kontexte, Layer, Predicate, Perspective, Zeitachsen, EventKind-/Role-/Attribute-IDs, Evidence-/Provenance-Endpunkte und TransferLineage-Quellen/Ziele. Jede Referenz muss nach demselben Remap entweder auf eine importierte Identität oder auf eine Identität im Zielbestand zeigen; sonst wird der Import geschlossen abgewiesen.

`LogicalImportDestinationInventory` ist ein Identitäts-Snapshot, kein vollständiger Schema- oder Datenbanksnapshot. Dieser Schritt prüft daher das Vorhandensein und die Typfamilie von Referenzen; die Ausführung gegen eine konkrete persistente Datenbank und deren vollständige Schema-/Transaktionsvalidierung gehört zum nachgelagerten Adapter-/Storage-Vertrag.

## Determinismus

`stream_fingerprint` bindet den Digest der Quellbytes und die kanonischen Planbytes. Identische Quellbytes, Ziel-DatabaseId und Remap-Konfiguration ergeben damit denselben Fingerprint. `LogicalImport::map_identity` stellt die zentrale Abbildung bereit, damit Records und alle ihre Referenzen dieselbe Zuordnung verwenden.

## Abnahmegrenze

Die Windows-Prüfung ist in [`M7-13-verification.md`](M7-13-verification.md) dokumentiert. Dieser Schritt persistiert keine Records und führt keinen externen Importprozess aus; Protokollverhandlung, Prozessisolation und atomare Storage-Publikation folgen M7-13a und dem Backend-Vertrag. Linux/macOS bleiben gemäß Projektvorgabe für M9-07 zurückgestellt.
