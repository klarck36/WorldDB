//! Deterministic, read-only purge planning over a complete logical snapshot.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use worlddb_core::{
    DatabaseId, DomainId, IndexFamily, Record, RecordCodecError, RecordKind, Revision,
    encode_decoded_record,
};

use crate::IndexStorageInventory;
use crate::logical_export::{
    LogicalExport, LogicalExportError, LogicalExportStorageClass, record_ref,
};
use crate::logical_import::{
    LogicalImportError, LogicalImportIdentity, record_defined_identities, record_references,
};

const PURGE_PLAN_CONTEXT: &[u8] = b"WorldDB.PurgePlan.v1\0";
const MAX_PURGE_RECORDS: usize = 1_000_000;
const MAX_PURGE_TARGETS: usize = 65_536;
const MAX_EXTERNAL_ARTIFACTS: usize = 100_000;

/// A typed record and the digest of its exact canonical record frame.
///
/// The digest distinguishes schema revisions that share one logical identity. It is bound to
/// the source export, so it is a stable selector only for that exact exported snapshot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PurgeRecordId {
    identity: LogicalImportIdentity,
    content_digest: [u8; 32],
}

impl PurgeRecordId {
    /// Stable primary identity of this record in the source export.
    #[must_use]
    pub const fn identity(self) -> LogicalImportIdentity {
        self.identity
    }

    /// BLAKE3 digest of the canonical record frame, including optional wire flags.
    #[must_use]
    pub const fn content_digest(self) -> [u8; 32] {
        self.content_digest
    }
}

/// One derived index generation found in the source database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PurgeIndexGeneration {
    family: IndexFamily,
    generation_id: u64,
    file_digest: [u8; 32],
    current: bool,
}

impl PurgeIndexGeneration {
    /// Creates one inventory row. Generation zero is reserved.
    pub fn new(
        family: IndexFamily,
        generation_id: u64,
        file_digest: [u8; 32],
        current: bool,
    ) -> Result<Self, PurgeError> {
        if generation_id == 0 {
            return Err(PurgeError::InvalidIndexInventory);
        }
        Ok(Self {
            family,
            generation_id,
            file_digest,
            current,
        })
    }

    /// Closed index family.
    #[must_use]
    pub const fn family(self) -> IndexFamily {
        self.family
    }

    /// Immutable generation number.
    #[must_use]
    pub const fn generation_id(self) -> u64 {
        self.generation_id
    }

    /// Digest of the complete generation file.
    #[must_use]
    pub const fn file_digest(self) -> [u8; 32] {
        self.file_digest
    }

    /// Whether the current-family pointer names this generation.
    #[must_use]
    pub const fn current(self) -> bool {
        self.current
    }
}

/// Kind of an external backup or export retained outside the new database.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum PurgeExternalArtifactKind {
    /// Exact data backup.
    ExactBackup,
    /// Backup that also contains the declared audit history.
    AuditCompleteBackup,
    /// Canonical logical export.
    LogicalExport,
    /// Security-filtered sharing export.
    SharingExport,
}

/// One known external artifact. A purge cannot modify this artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PurgeExternalArtifact {
    kind: PurgeExternalArtifactKind,
    digest: [u8; 32],
}

impl PurgeExternalArtifact {
    /// Creates a known retained artifact inventory row.
    #[must_use]
    pub const fn new(kind: PurgeExternalArtifactKind, digest: [u8; 32]) -> Self {
        Self { kind, digest }
    }

    /// External artifact kind.
    #[must_use]
    pub const fn kind(self) -> PurgeExternalArtifactKind {
        self.kind
    }

    /// Digest of the exact external artifact bytes.
    #[must_use]
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }
}

/// Caller-supplied inventory of nonlogical storage and copies beyond the database root.
///
/// Index generations are local and must be completely inventoried before a plan can be
/// approved. External copies are reported as retained even when the caller cannot attest to a
/// complete search; that uncertainty is surfaced rather than treated as secure erasure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeSidecarInventory {
    database_id: DatabaseId,
    index_generations: Vec<PurgeIndexGeneration>,
    known_external_artifacts: Vec<PurgeExternalArtifact>,
    index_inventory_complete: bool,
    external_inventory_complete: bool,
}

impl PurgeSidecarInventory {
    /// Builds from a verified complete local index scan and a caller-supplied external-copy list.
    pub fn new(
        index_inventory: IndexStorageInventory,
        known_external_artifacts: Vec<PurgeExternalArtifact>,
        external_inventory_complete: bool,
    ) -> Result<Self, PurgeError> {
        Self::build(
            index_inventory.database_id(),
            index_inventory
                .generations()
                .iter()
                .map(|entry| PurgeIndexGeneration {
                    family: entry.family(),
                    generation_id: entry.generation_id(),
                    file_digest: entry.file_digest(),
                    current: entry.current(),
                })
                .collect(),
            known_external_artifacts,
            true,
            external_inventory_complete,
        )
    }

    /// Builds a preview from a partial index scan; it can never be approved for rewriting.
    pub fn from_partial_index_inventory(
        database_id: DatabaseId,
        index_generations: Vec<PurgeIndexGeneration>,
        known_external_artifacts: Vec<PurgeExternalArtifact>,
        external_inventory_complete: bool,
    ) -> Result<Self, PurgeError> {
        Self::build(
            database_id,
            index_generations,
            known_external_artifacts,
            false,
            external_inventory_complete,
        )
    }

    fn build(
        database_id: DatabaseId,
        mut index_generations: Vec<PurgeIndexGeneration>,
        mut known_external_artifacts: Vec<PurgeExternalArtifact>,
        index_inventory_complete: bool,
        external_inventory_complete: bool,
    ) -> Result<Self, PurgeError> {
        if index_generations.len() > MAX_PURGE_RECORDS
            || known_external_artifacts.len() > MAX_EXTERNAL_ARTIFACTS
        {
            return Err(PurgeError::ResourceLimit);
        }
        index_generations.sort_by_key(|entry| (entry.family.wire_tag(), entry.generation_id));
        if index_generations.windows(2).any(|pair| {
            matches!(pair, [left, right] if left.family == right.family && left.generation_id == right.generation_id)
        }) {
            return Err(PurgeError::InvalidIndexInventory);
        }
        for family in IndexFamily::ALL {
            if index_generations
                .iter()
                .filter(|entry| entry.family == family && entry.current)
                .count()
                > 1
            {
                return Err(PurgeError::InvalidIndexInventory);
            }
        }
        known_external_artifacts.sort_by_key(|entry| (entry.kind, entry.digest));
        Ok(Self {
            database_id,
            index_generations,
            known_external_artifacts,
            index_inventory_complete,
            external_inventory_complete,
        })
    }

    /// All inventoried immutable generations, including generations no longer current.
    #[must_use]
    pub fn index_generations(&self) -> &[PurgeIndexGeneration] {
        &self.index_generations
    }

    /// Known backups and exports that remain outside the rewritten database.
    #[must_use]
    pub fn known_external_artifacts(&self) -> &[PurgeExternalArtifact] {
        &self.known_external_artifacts
    }

    /// Whether every local index generation and pointer was inventoried.
    #[must_use]
    pub const fn index_inventory_complete(&self) -> bool {
        self.index_inventory_complete
    }

    /// Whether the caller's external backup/export search was declared complete.
    #[must_use]
    pub const fn external_inventory_complete(&self) -> bool {
        self.external_inventory_complete
    }
}

/// Explicit list of all transitive dependants approved for cascade removal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgeCascadePlan {
    dependants: Vec<PurgeRecordId>,
}

impl PurgeCascadePlan {
    /// Creates a sorted, duplicate-free approval set from a preview's dependant rows.
    pub fn new(mut dependants: Vec<PurgeRecordId>) -> Result<Self, PurgeError> {
        if dependants.len() > MAX_PURGE_RECORDS {
            return Err(PurgeError::ResourceLimit);
        }
        dependants.sort_unstable();
        if dependants
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left == right))
        {
            return Err(PurgeError::DuplicateCascadeRecord);
        }
        Ok(Self { dependants })
    }

    /// Exact dependant set explicitly approved for removal.
    #[must_use]
    pub fn dependants(&self) -> &[PurgeRecordId] {
        &self.dependants
    }
}

/// How a plan was explicitly approved after its full dependant set was displayed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurgeApproval {
    /// The target has no dependants, so no cascade is required.
    RejectIfReferenced,
    /// Every computed dependant was named by a matching `PurgeCascadePlan`.
    CascadePlan,
}

/// A deterministic, source-bound purge preview. Creating it does not mutate storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurgePlan {
    source_database_id: DatabaseId,
    source_snapshot_revision: Revision,
    source_artifact_digest: [u8; 32],
    targets: Vec<LogicalImportIdentity>,
    target_records: Vec<PurgeRecordId>,
    dependants: Vec<PurgeRecordId>,
    sidecars: PurgeSidecarInventory,
    approval: Option<PurgeApproval>,
    fingerprint: [u8; 32],
}

impl PurgePlan {
    /// Database identity represented by the exact source export.
    #[must_use]
    pub const fn source_database_id(&self) -> DatabaseId {
        self.source_database_id
    }

    /// Revision represented by the source export.
    #[must_use]
    pub const fn source_snapshot_revision(&self) -> Revision {
        self.source_snapshot_revision
    }

    /// Digest of the exact canonical logical-export bytes used for planning.
    #[must_use]
    pub const fn source_artifact_digest(&self) -> [u8; 32] {
        self.source_artifact_digest
    }

    /// Requested typed identities to purge.
    #[must_use]
    pub fn targets(&self) -> &[LogicalImportIdentity] {
        &self.targets
    }

    /// Exact source records that own the requested identities.
    #[must_use]
    pub fn target_records(&self) -> &[PurgeRecordId] {
        &self.target_records
    }

    /// Complete transitive dependant set, including Evidence, Provenance, masks, events,
    /// lifecycle records, and schema/catalog references present in the source export.
    #[must_use]
    pub fn dependants(&self) -> &[PurgeRecordId] {
        &self.dependants
    }

    /// All source records the approved rewrite would omit.
    #[must_use]
    pub fn affected_records(&self) -> Vec<PurgeRecordId> {
        let mut records = self.target_records.clone();
        records.extend_from_slice(&self.dependants);
        records.sort_unstable();
        records.dedup();
        records
    }

    pub(crate) fn matches_sidecars(&self, sidecars: &PurgeSidecarInventory) -> bool {
        self.sidecars == *sidecars
    }

    pub(crate) fn is_affected(
        &self,
        record: &worlddb_core::DecodedRecord,
    ) -> Result<bool, PurgeError> {
        let definitions = record_defined_identities(record.record());
        if definitions.is_empty() {
            return Err(PurgeError::RecordHasNoIdentity);
        }
        let id = purge_record_id(record.record(), record, &definitions)?;
        Ok(self.target_records.binary_search(&id).is_ok()
            || self.dependants.binary_search(&id).is_ok())
    }

    /// Index generations inventoried for disposal and rebuild in the new database.
    #[must_use]
    pub fn index_generations(&self) -> &[PurgeIndexGeneration] {
        self.sidecars.index_generations()
    }

    /// Every closed 1.0 index family must be rebuilt for the rewritten database.
    #[must_use]
    pub const fn index_families_to_rebuild() -> &'static [IndexFamily] {
        &IndexFamily::ALL
    }

    /// Whether every local index generation and pointer was inventoried.
    #[must_use]
    pub const fn index_inventory_complete(&self) -> bool {
        self.sidecars.index_inventory_complete()
    }

    /// Known external backups and exports. They remain outside the new database.
    #[must_use]
    pub fn retained_external_artifacts(&self) -> &[PurgeExternalArtifact] {
        self.sidecars.known_external_artifacts()
    }

    /// Whether the external-copy list is complete. A true value is a caller declaration only.
    #[must_use]
    pub const fn external_inventory_complete(&self) -> bool {
        self.sidecars.external_inventory_complete()
    }

    /// Approval mode, if the complete dependant set has been approved.
    #[must_use]
    pub const fn approval(&self) -> Option<PurgeApproval> {
        self.approval
    }

    /// Stable digest over source bytes, targets, exact affected records, inventories, and approval.
    #[must_use]
    pub const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Approves only when no references require a cascade.
    pub fn approve_reject_if_referenced(mut self) -> Result<Self, PurgeError> {
        self.require_complete_index_inventory()?;
        if !self.dependants.is_empty() {
            return Err(PurgeError::ReferencesRemain);
        }
        self.approval = Some(PurgeApproval::RejectIfReferenced);
        self.refresh_fingerprint()?;
        Ok(self)
    }

    /// Approves a cascade only when its explicit set exactly equals the computed closure.
    pub fn approve_cascade(mut self, cascade: &PurgeCascadePlan) -> Result<Self, PurgeError> {
        self.require_complete_index_inventory()?;
        if cascade.dependants != self.dependants {
            return Err(PurgeError::CascadePlanMismatch);
        }
        self.approval = Some(PurgeApproval::CascadePlan);
        self.refresh_fingerprint()?;
        Ok(self)
    }

    fn require_complete_index_inventory(&self) -> Result<(), PurgeError> {
        if self.sidecars.index_inventory_complete {
            Ok(())
        } else {
            Err(PurgeError::IndexInventoryIncomplete)
        }
    }

    fn refresh_fingerprint(&mut self) -> Result<(), PurgeError> {
        self.fingerprint = plan_fingerprint(self)?;
        Ok(())
    }
}

/// Read-only logical purge planner.
#[derive(Clone, Copy, Debug, Default)]
pub struct PurgePlanManager;

impl PurgePlanManager {
    /// Validates a complete logical snapshot and computes a deterministic dependency closure.
    ///
    /// The export must cover the complete revision range, every revision-bearing record class,
    /// and every HistorySpace. The caller supplies local index and external-copy inventories.
    /// This produces a preview only; M7-15 performs a separate offline rewrite into a new
    /// DatabaseId and must revalidate the source snapshot before publishing it.
    pub fn preview(
        source_artifact: &[u8],
        mut targets: Vec<LogicalImportIdentity>,
        sidecars: PurgeSidecarInventory,
    ) -> Result<PurgePlan, PurgeError> {
        let source = LogicalExport::decode(source_artifact).map_err(PurgeError::Export)?;
        validate_complete_export(&source)?;
        if sidecars.database_id != source.manifest().database_id() {
            return Err(PurgeError::IndexDatabaseMismatch);
        }
        if targets.is_empty() || targets.len() > MAX_PURGE_TARGETS {
            return Err(PurgeError::InvalidTargets);
        }
        targets.sort_unstable();
        if targets
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left == right))
        {
            return Err(PurgeError::DuplicateTarget);
        }
        if source.records().len() > MAX_PURGE_RECORDS {
            return Err(PurgeError::ResourceLimit);
        }

        let mut record_ids = Vec::new();
        let mut owned = Vec::new();
        let mut references = Vec::new();
        let mut unique_record_ids = BTreeSet::new();
        let mut owners = BTreeMap::<LogicalImportIdentity, Vec<usize>>::new();
        let mut dependants_by_identity = BTreeMap::<LogicalImportIdentity, Vec<usize>>::new();
        let mut identities = BTreeSet::from_iter(
            source
                .manifest()
                .visible_history_spaces()
                .iter()
                .map(|space| LogicalImportIdentity::HistorySpace(space.id())),
        );

        for (index, entry) in source.records().iter().enumerate() {
            let record = entry.record().record();
            let definitions = record_defined_identities(record);
            if definitions.is_empty() {
                return Err(PurgeError::RecordHasNoIdentity);
            }
            let id = purge_record_id(record, entry.record(), &definitions)?;
            if !unique_record_ids.insert(id) {
                return Err(PurgeError::DuplicateRecordIdentity);
            }
            record_ids.push(id);
            for identity in &definitions {
                identities.insert(*identity);
                owners.entry(*identity).or_default().push(index);
            }
            let record_refs = record_references(record);
            for identity in &record_refs {
                dependants_by_identity
                    .entry(*identity)
                    .or_default()
                    .push(index);
            }
            owned.push(definitions);
            references.push(record_refs);
        }

        for record_refs in &references {
            for identity in record_refs {
                if !identities.contains(identity) {
                    return Err(PurgeError::MissingReference);
                }
            }
        }
        for identity in &targets {
            if !owners.contains_key(identity) {
                return Err(PurgeError::UnknownTarget);
            }
        }

        let target_indices = targets
            .iter()
            .flat_map(|identity| owners.get(identity).into_iter().flatten().copied())
            .collect::<BTreeSet<_>>();
        let affected_indices =
            dependency_closure(&targets, &owners, &dependants_by_identity, &owned)?;
        let mut target_records = Vec::new();
        for index in &target_indices {
            target_records.push(
                record_ids
                    .get(*index)
                    .copied()
                    .ok_or(PurgeError::InventoryInconsistent)?,
            );
        }
        target_records.sort_unstable();
        let mut dependants = Vec::new();
        for index in affected_indices.difference(&target_indices) {
            dependants.push(
                record_ids
                    .get(*index)
                    .copied()
                    .ok_or(PurgeError::InventoryInconsistent)?,
            );
        }
        dependants.sort_unstable();
        let mut plan = PurgePlan {
            source_database_id: source.manifest().database_id(),
            source_snapshot_revision: source.manifest().snapshot_revision(),
            source_artifact_digest: *blake3::hash(source_artifact).as_bytes(),
            targets,
            target_records,
            dependants,
            sidecars,
            approval: None,
            fingerprint: [0; 32],
        };
        plan.refresh_fingerprint()?;
        Ok(plan)
    }
}

fn validate_complete_export(source: &LogicalExport) -> Result<(), PurgeError> {
    let manifest = source.manifest();
    if manifest.from_revision() != Revision::GENESIS
        || manifest.through_revision() != manifest.snapshot_revision()
    {
        return Err(PurgeError::IncompleteLogicalExport);
    }
    let selected: BTreeSet<_> = manifest.selected_record_kinds().iter().copied().collect();
    for kind in RecordKind::ALL {
        if !is_revisionless_kind(kind) && !selected.contains(&kind) {
            return Err(PurgeError::IncompleteLogicalExport);
        }
    }
    let selected_spaces: BTreeSet<_> = manifest.selected_history_spaces().iter().copied().collect();
    if manifest
        .visible_history_spaces()
        .iter()
        .any(|space| !space.selected())
        || manifest
            .visible_history_spaces()
            .iter()
            .any(|space| !selected_spaces.contains(&space.id()))
    {
        return Err(PurgeError::IncompleteLogicalExport);
    }
    let defined_spaces: BTreeSet<_> = source
        .records()
        .iter()
        .filter_map(|entry| match entry.record().record() {
            Record::HistorySpaceDefinition(definition) => Some(definition.history_space_id()),
            _ => None,
        })
        .collect();
    if defined_spaces.len() != selected_spaces.len() || defined_spaces != selected_spaces {
        return Err(PurgeError::IncompleteLogicalExport);
    }
    if !manifest
        .omitted_storage_classes()
        .contains(&LogicalExportStorageClass::DerivedIndexes)
    {
        return Err(PurgeError::IncompleteLogicalExport);
    }
    Ok(())
}

fn is_revisionless_kind(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::MigrationPlan
            | RecordKind::MigrationRun
            | RecordKind::MigrationStepCommitIdentity
    )
}

fn purge_record_id(
    record: &Record,
    decoded: &worlddb_core::DecodedRecord,
    definitions: &[LogicalImportIdentity],
) -> Result<PurgeRecordId, PurgeError> {
    let identity = record_ref(record)
        .map(LogicalImportIdentity::Record)
        .or_else(|| definitions.first().copied())
        .ok_or(PurgeError::RecordHasNoIdentity)?;
    let frame = encode_decoded_record(decoded).map_err(PurgeError::Record)?;
    Ok(PurgeRecordId {
        identity,
        content_digest: *blake3::hash(&frame).as_bytes(),
    })
}

fn dependency_closure(
    targets: &[LogicalImportIdentity],
    owners: &BTreeMap<LogicalImportIdentity, Vec<usize>>,
    dependants_by_identity: &BTreeMap<LogicalImportIdentity, Vec<usize>>,
    owned: &[Vec<LogicalImportIdentity>],
) -> Result<BTreeSet<usize>, PurgeError> {
    let mut pending = VecDeque::from(targets.to_vec());
    let mut visited_identities = BTreeSet::new();
    let mut affected = BTreeSet::new();
    while let Some(identity) = pending.pop_front() {
        if !visited_identities.insert(identity) {
            continue;
        }
        if let Some(owner_records) = owners.get(&identity) {
            for index in owner_records {
                if affected.insert(*index) {
                    pending.extend(
                        owned
                            .get(*index)
                            .ok_or(PurgeError::InventoryInconsistent)?
                            .iter()
                            .copied(),
                    );
                }
            }
        }
        if let Some(dependant_records) = dependants_by_identity.get(&identity) {
            for index in dependant_records {
                if affected.insert(*index) {
                    pending.extend(
                        owned
                            .get(*index)
                            .ok_or(PurgeError::InventoryInconsistent)?
                            .iter()
                            .copied(),
                    );
                }
            }
        }
    }
    Ok(affected)
}

fn plan_fingerprint(plan: &PurgePlan) -> Result<[u8; 32], PurgeError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(PURGE_PLAN_CONTEXT);
    hasher.update(&plan.source_database_id.to_bytes());
    hasher.update(&plan.source_snapshot_revision.value().to_le_bytes());
    hasher.update(&plan.source_artifact_digest);
    hasher.update(&plan.sidecars.database_id.to_bytes());
    hasher.update(&[u8::from(plan.sidecars.index_inventory_complete)]);
    hasher.update(&[u8::from(plan.sidecars.external_inventory_complete)]);
    hasher.update(&[match plan.approval {
        None => 0,
        Some(PurgeApproval::RejectIfReferenced) => 1,
        Some(PurgeApproval::CascadePlan) => 2,
    }]);
    hash_identities(&mut hasher, &plan.targets)?;
    hash_records(&mut hasher, &plan.target_records)?;
    hash_records(&mut hasher, &plan.dependants)?;
    hasher.update(
        &u32::try_from(plan.sidecars.index_generations.len())
            .map_err(|_| PurgeError::ResourceLimit)?
            .to_le_bytes(),
    );
    for generation in &plan.sidecars.index_generations {
        hasher.update(&generation.family.wire_tag().to_le_bytes());
        hasher.update(&generation.generation_id.to_le_bytes());
        hasher.update(&generation.file_digest);
        hasher.update(&[u8::from(generation.current)]);
    }
    hasher.update(
        &u32::try_from(plan.sidecars.known_external_artifacts.len())
            .map_err(|_| PurgeError::ResourceLimit)?
            .to_le_bytes(),
    );
    for artifact in &plan.sidecars.known_external_artifacts {
        hasher.update(&[artifact.kind as u8]);
        hasher.update(&artifact.digest);
    }
    for family in IndexFamily::ALL {
        hasher.update(&family.wire_tag().to_le_bytes());
    }
    Ok(*hasher.finalize().as_bytes())
}

fn hash_identities(
    hasher: &mut blake3::Hasher,
    identities: &[LogicalImportIdentity],
) -> Result<(), PurgeError> {
    hasher.update(
        &u32::try_from(identities.len())
            .map_err(|_| PurgeError::ResourceLimit)?
            .to_le_bytes(),
    );
    for identity in identities {
        let mut bytes = Vec::new();
        identity.encode(&mut bytes).map_err(PurgeError::Identity)?;
        hasher.update(&bytes);
    }
    Ok(())
}

fn hash_records(hasher: &mut blake3::Hasher, records: &[PurgeRecordId]) -> Result<(), PurgeError> {
    hasher.update(
        &u32::try_from(records.len())
            .map_err(|_| PurgeError::ResourceLimit)?
            .to_le_bytes(),
    );
    for record in records {
        let mut bytes = Vec::new();
        record
            .identity
            .encode(&mut bytes)
            .map_err(PurgeError::Identity)?;
        hasher.update(&bytes);
        hasher.update(&record.content_digest);
    }
    Ok(())
}

/// Failure to construct or explicitly approve a safe purge plan.
#[derive(Debug)]
pub enum PurgeError {
    /// The supplied logical export failed canonical integrity or validation.
    Export(LogicalExportError),
    /// The source export is scoped and does not cover the complete source snapshot.
    IncompleteLogicalExport,
    /// The target list is empty or exceeds the registered bound.
    InvalidTargets,
    /// One target was listed more than once.
    DuplicateTarget,
    /// A requested identity is not defined by the source export.
    UnknownTarget,
    /// A source reference has no corresponding identity in the export.
    MissingReference,
    /// A record has no stable logical identity.
    RecordHasNoIdentity,
    /// Two source records have the same canonical purge selector.
    DuplicateRecordIdentity,
    /// A planner-generated dependency index referred outside its record inventory.
    InventoryInconsistent,
    /// An index inventory contains an invalid or duplicate generation row.
    InvalidIndexInventory,
    /// The index inventory was read from another DatabaseId.
    IndexDatabaseMismatch,
    /// The local index inventory is incomplete, so approval fails closed.
    IndexInventoryIncomplete,
    /// The RejectIfReferenced mode found at least one dependant.
    ReferencesRemain,
    /// The explicit cascade list omitted or added a computed dependant.
    CascadePlanMismatch,
    /// One cascade record was listed more than once.
    DuplicateCascadeRecord,
    /// A canonical identity could not be encoded.
    Identity(LogicalImportError),
    /// A record could not be encoded canonically.
    Record(RecordCodecError),
    /// The inventory or fingerprint exceeds a registered resource bound.
    ResourceLimit,
}

impl fmt::Display for PurgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Export(error) => write!(formatter, "purge source export: {error}"),
            Self::IncompleteLogicalExport => formatter
                .write_str("purge planning requires a complete full-snapshot logical export"),
            Self::InvalidTargets => formatter.write_str("purge target list is invalid"),
            Self::DuplicateTarget => formatter.write_str("purge target occurs more than once"),
            Self::UnknownTarget => {
                formatter.write_str("purge target is absent from the source export")
            }
            Self::MissingReference => {
                formatter.write_str("source export has an unresolved reference")
            }
            Self::RecordHasNoIdentity => formatter.write_str("source record has no purge identity"),
            Self::DuplicateRecordIdentity => {
                formatter.write_str("source record purge identities collide")
            }
            Self::InventoryInconsistent => {
                formatter.write_str("purge dependency inventory is internally inconsistent")
            }
            Self::InvalidIndexInventory => {
                formatter.write_str("derived-index inventory is invalid")
            }
            Self::IndexDatabaseMismatch => {
                formatter.write_str("derived-index inventory belongs to a different database")
            }
            Self::IndexInventoryIncomplete => {
                formatter.write_str("derived-index inventory is incomplete")
            }
            Self::ReferencesRemain => {
                formatter.write_str("RejectIfReferenced found transitive dependants")
            }
            Self::CascadePlanMismatch => {
                formatter.write_str("CascadePlan does not exactly match all transitive dependants")
            }
            Self::DuplicateCascadeRecord => {
                formatter.write_str("CascadePlan repeats a dependant record")
            }
            Self::Identity(error) => write!(formatter, "purge identity encoding: {error}"),
            Self::Record(error) => write!(formatter, "purge record encoding: {error}"),
            Self::ResourceLimit => {
                formatter.write_str("purge inventory exceeds its resource limit")
            }
        }
    }
}

impl std::error::Error for PurgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Export(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::Record(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dependency_closure;
    use crate::LogicalImportIdentity;
    use std::collections::BTreeMap;
    use worlddb_core::{DomainId, EntityId, EntityRetirementId, RecordRef};

    fn entity(tail: u8) -> Result<EntityId, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        EntityId::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    #[test]
    fn dependant_closure_is_transitive_and_cycle_safe() -> Result<(), String> {
        let root = LogicalImportIdentity::Entity(entity(1)?);
        let first = LogicalImportIdentity::Record(RecordRef::EntityRetirement(
            EntityRetirementId::try_from_bytes({
                let mut bytes = [0_u8; 16];
                bytes[6] = 0x70;
                bytes[8] = 0x80;
                bytes[15] = 2;
                bytes
            })
            .map_err(|error| error.to_string())?,
        ));
        let second = LogicalImportIdentity::Record(RecordRef::Assertion(
            worlddb_core::AssertionId::try_from_bytes({
                let mut bytes = [0_u8; 16];
                bytes[6] = 0x70;
                bytes[8] = 0x80;
                bytes[15] = 3;
                bytes
            })
            .map_err(|error| error.to_string())?,
        ));
        let owners = BTreeMap::from([(root, vec![0]), (first, vec![1]), (second, vec![2])]);
        let dependants = BTreeMap::from([(root, vec![1]), (first, vec![2]), (second, vec![1])]);
        let owned = vec![vec![root], vec![first], vec![second]];
        let closure = dependency_closure(&[root], &owners, &dependants, &owned)
            .map_err(|error| error.to_string())?;
        assert_eq!(closure.into_iter().collect::<Vec<_>>(), vec![0, 1, 2]);
        Ok(())
    }
}
