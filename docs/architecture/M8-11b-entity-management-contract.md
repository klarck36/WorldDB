# M8-11b – Entity management contract

**Status:** implemented and verified on Windows
**Scope:** project-wide Entity catalog management in the authenticated ODE-002 desktop host
**Normative source:** M0-04 / ADR-030 and the schema, revision, authorization, and audit contracts

## Catalog and lifecycle

An Entity is a project-wide, immutable catalog identity with one permanent `EntityTypeId` and a shared database creation revision. The UI does not invent an Entity name or metadata map: labels and other facts belong in typed Assertions. Retyping, deletion, reactivation, and ID reuse are unavailable.

Creation accepts only an EntityType visible in the live schema. `Active` is the ordinary path. `Deprecated` requires an explicit renderer opt-in and the existing `SchemaManage` capability in addition to `EntityCreate`; a typed warning accompanies the successful publication. `Retired` EntityTypes cannot receive new Entities. A type's later retirement does not change the permanent assignment already stored on an Entity.

Entity retirement appends a concrete `EntityRetirement` lifecycle record on the same shared `Revision` axis. It is irreversible and does not alter or delete prior catalog entries or domain records. Historical reads retain the pre-retirement state. Duplicate retirement and unknown identities fail without publication.

## Queries and user interface

The catalog exposes `current`, `historical` (`RecordedAsOf`), and `explicit` shared-revision views. The Entity list and its EntityType lifecycle labels come from the same selected revision. Create and retire controls are available only for the live view. The EntityType assignment is shown on each row; the application-generated identity remains internal to IPC and is never rendered.

The desktop supports selecting an Active or Deprecated EntityType, visibly explains and requires consent for Deprecated creation, and confirms terminal retirement. Project-state events refresh open native windows after publication. The panel explains that Entity facts and names are typed Assertions.

## Authorization, audit, and publication

The native Tauri host binds commands to its authenticated project session and host-selected principal. `FileEntityManager` requires `SchemaRead` to open its schema context. Catalog reads require `EntityRead`; creation requires `EntityCreate`, plus `SchemaManage` for a Deprecated type; retirement requires `EntityRead` and `EntityRetire`. Current policy is checked on each operation.

Create and retire commands bind an expected base revision. The manager checks the loaded base and the live WAL head, rejecting stale writes without publishing. The Entity record or retirement, carried-forward security-policy history, manifest update, and Required Audit record commit through one WAL transaction. Audit actions are `EntityCreation` and `EntityRetirement`, both classified as `EntityCatalog`. Unknown durable outcomes are recovered and reported without claiming an unproven success.

The IPC protocol is a closed typed command set. Renderer-selected project paths, principals, and record bytes are not accepted. User-facing rejection remains the stable `entity_rejected` code.

## Windows acceptance coverage

Storage tests cover append-only history, Required Audit binding, Deprecated opt-in, Retired type rejection, read/create/retire authorization, and stale-live-head rejection. ODE tests lock the command and response JSON shapes. The real two-window smoke exercises create, warning, retirement, historical and explicit reads, secondary-window refresh, and existing IPC security checks in both in-process and sidecar profiles. Linux/macOS runs remain deferred to M9-07 as requested.
