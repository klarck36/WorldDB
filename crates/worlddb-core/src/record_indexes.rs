//! Typed lookup indexes for record, operation, schema, and lifecycle history.

use std::collections::BTreeMap;
use std::fmt;

use crate::ids::{OperationId, Revision, SchemaRevision};
use crate::record_refs::{LifecycleTargetRef, RecordRef, SchemaRecordRef};
use crate::resource_profile::{
    MemoryReservation, ProcessMemoryBudget, estimated_index_memory_bytes, process_memory_budget,
    reserve_index_memory_with_budget,
};
use crate::schema_history::SchemaDefinition;
use crate::wire_records::Record;

/// One address in the domain-history record store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordIdIndexEntry {
    record: RecordRef,
    created_revision: Revision,
    location: u64,
}

impl RecordIdIndexEntry {
    /// Describes one stored domain-history record without erasing its typed ID.
    #[must_use]
    pub const fn new(record: RecordRef, created_revision: Revision, location: u64) -> Self {
        Self {
            record,
            created_revision,
            location,
        }
    }

    /// Typed identity of the addressed record.
    #[must_use]
    pub const fn record(self) -> RecordRef {
        self.record
    }

    /// Revision that created the record.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }

    /// Backend-owned location token for the record payload.
    #[must_use]
    pub const fn location(self) -> u64 {
        self.location
    }
}

/// Record-ID lookup built from the same immutable rows used by the scan oracle.
#[derive(Clone, Debug, Default)]
pub struct RecordIdIndex {
    entries: Vec<RecordIdIndexEntry>,
    by_id: BTreeMap<RecordRef, usize>,
    _memory_reservation: Option<MemoryReservation>,
}

impl RecordIdIndex {
    /// Builds the index and rejects duplicate typed record identities.
    pub fn build(entries: Vec<RecordIdIndexEntry>) -> Result<Self, LookupIndexError> {
        Self::build_with_memory_budget(entries, process_memory_budget())
    }

    /// Builds the index under an explicit shared process memory ledger.
    pub fn build_with_memory_budget(
        entries: Vec<RecordIdIndexEntry>,
        budget: &ProcessMemoryBudget,
    ) -> Result<Self, LookupIndexError> {
        let bytes = estimated_index_memory_bytes(entries.len(), 192)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let reservation = reserve_index_memory_with_budget(budget, bytes)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let mut by_id = BTreeMap::new();
        for (position, entry) in entries.iter().enumerate() {
            if by_id.insert(entry.record, position).is_some() {
                return Err(LookupIndexError::DuplicateRecordId(entry.record));
            }
        }
        Ok(Self {
            entries,
            by_id,
            _memory_reservation: Some(reservation),
        })
    }

    /// Resolves an exact typed record identity.
    #[must_use]
    pub fn lookup(&self, record: RecordRef) -> Option<&RecordIdIndexEntry> {
        self.by_id
            .get(&record)
            .and_then(|position| self.entries.get(*position))
    }

    /// Number of distinct typed record identities represented.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the generation has no record identities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Closed durable operation status stored by the lookup index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationIndexStatus {
    /// WAL contains a prepare, but no verified commit marker exists yet.
    Indeterminate,
    /// A verified commit marker publishes the operation at this revision.
    Committed { revision: Revision },
}

/// One durable operation-ID status and storage location.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationIdIndexEntry {
    operation_id: OperationId,
    status: OperationIndexStatus,
    location: u64,
}

impl OperationIdIndexEntry {
    /// Describes one operation entry reconstructed from the durable journal.
    #[must_use]
    pub const fn new(
        operation_id: OperationId,
        status: OperationIndexStatus,
        location: u64,
    ) -> Self {
        Self {
            operation_id,
            status,
            location,
        }
    }

    /// Idempotency identity of this operation.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Durable state for this operation ID.
    #[must_use]
    pub const fn status(self) -> OperationIndexStatus {
        self.status
    }

    /// Backend-owned location token for the operation metadata.
    #[must_use]
    pub const fn location(self) -> u64 {
        self.location
    }
}

/// Operation-ID lookup. An operation ID can address at most one durable entry.
#[derive(Clone, Debug, Default)]
pub struct OperationIdIndex {
    entries: Vec<OperationIdIndexEntry>,
    by_id: BTreeMap<OperationId, usize>,
    _memory_reservation: Option<MemoryReservation>,
}

impl OperationIdIndex {
    /// Builds the index and rejects duplicate operation IDs.
    pub fn build(entries: Vec<OperationIdIndexEntry>) -> Result<Self, LookupIndexError> {
        Self::build_with_memory_budget(entries, process_memory_budget())
    }

    /// Builds the index under an explicit shared process memory ledger.
    pub fn build_with_memory_budget(
        entries: Vec<OperationIdIndexEntry>,
        budget: &ProcessMemoryBudget,
    ) -> Result<Self, LookupIndexError> {
        let bytes = estimated_index_memory_bytes(entries.len(), 192)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let reservation = reserve_index_memory_with_budget(budget, bytes)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let mut by_id = BTreeMap::new();
        for (position, entry) in entries.iter().enumerate() {
            if by_id.insert(entry.operation_id, position).is_some() {
                return Err(LookupIndexError::DuplicateOperationId(entry.operation_id));
            }
        }
        Ok(Self {
            entries,
            by_id,
            _memory_reservation: Some(reservation),
        })
    }

    /// Resolves the exact durable state for an operation ID.
    #[must_use]
    pub fn lookup(&self, operation_id: OperationId) -> Option<&OperationIdIndexEntry> {
        self.by_id
            .get(&operation_id)
            .and_then(|position| self.entries.get(*position))
    }
}

/// One schema identity at the shared database revision axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaIdRevisionIndexEntry {
    schema_id: SchemaRecordRef,
    revision: SchemaRevision,
    location: u64,
}

impl SchemaIdRevisionIndexEntry {
    /// Describes a concrete schema record revision.
    #[must_use]
    pub const fn new(schema_id: SchemaRecordRef, revision: SchemaRevision, location: u64) -> Self {
        Self {
            schema_id,
            revision,
            location,
        }
    }

    /// Typed schema identity.
    #[must_use]
    pub const fn schema_id(self) -> SchemaRecordRef {
        self.schema_id
    }

    /// Shared published revision represented by this schema record.
    #[must_use]
    pub const fn revision(self) -> SchemaRevision {
        self.revision
    }

    /// Backend-owned location token for the schema payload.
    #[must_use]
    pub const fn location(self) -> u64 {
        self.location
    }
}

/// Exact schema-ID/revision lookup plus ordered history for one schema ID.
#[derive(Clone, Debug, Default)]
pub struct SchemaIdRevisionIndex {
    entries: Vec<SchemaIdRevisionIndexEntry>,
    by_revision: BTreeMap<(SchemaRecordRef, SchemaRevision), usize>,
    by_id: BTreeMap<SchemaRecordRef, Vec<usize>>,
    _memory_reservation: Option<MemoryReservation>,
}

impl SchemaIdRevisionIndex {
    /// Builds the indexes and rejects duplicate ID/revision pairs.
    pub fn build(entries: Vec<SchemaIdRevisionIndexEntry>) -> Result<Self, LookupIndexError> {
        Self::build_with_memory_budget(entries, process_memory_budget())
    }

    /// Builds the index under an explicit shared process memory ledger.
    pub fn build_with_memory_budget(
        entries: Vec<SchemaIdRevisionIndexEntry>,
        budget: &ProcessMemoryBudget,
    ) -> Result<Self, LookupIndexError> {
        let bytes = estimated_index_memory_bytes(entries.len(), 384)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let reservation = reserve_index_memory_with_budget(budget, bytes)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        Self::build_with_reservation(entries, reservation)
    }

    fn build_with_reservation(
        entries: Vec<SchemaIdRevisionIndexEntry>,
        reservation: MemoryReservation,
    ) -> Result<Self, LookupIndexError> {
        let mut by_revision = BTreeMap::new();
        let mut by_id = BTreeMap::<SchemaRecordRef, Vec<usize>>::new();
        for (position, entry) in entries.iter().enumerate() {
            if by_revision
                .insert((entry.schema_id, entry.revision), position)
                .is_some()
            {
                return Err(LookupIndexError::DuplicateSchemaRevision {
                    schema_id: entry.schema_id,
                    revision: entry.revision,
                });
            }
            by_id.entry(entry.schema_id).or_default().push(position);
        }
        for positions in by_id.values_mut() {
            positions.sort_by_key(|position| entries.get(*position).map(|entry| entry.revision));
        }
        Ok(Self {
            entries,
            by_revision,
            by_id,
            _memory_reservation: Some(reservation),
        })
    }

    /// Extracts concrete schema identities and revisions from canonical history definitions.
    ///
    /// Layer snapshots are materialized state, not additional schema identities. Event roles
    /// and attributes inherit the revision of their enclosing EventKind definition.
    pub fn from_definitions(definitions: &[SchemaDefinition]) -> Result<Self, LookupIndexError> {
        Self::from_definitions_with_memory_budget(definitions, process_memory_budget())
    }

    /// Extracts and indexes schema identities under an explicit memory ledger.
    pub fn from_definitions_with_memory_budget(
        definitions: &[SchemaDefinition],
        budget: &ProcessMemoryBudget,
    ) -> Result<Self, LookupIndexError> {
        let entry_count = definitions
            .iter()
            .try_fold(0_usize, |count, definition| {
                let added = match definition {
                    SchemaDefinition::Layer(_)
                    | SchemaDefinition::EntityType(_)
                    | SchemaDefinition::Predicate(_)
                    | SchemaDefinition::Timeline(_) => 1,
                    SchemaDefinition::LayerSnapshot(_) => 0,
                    SchemaDefinition::TimeUnit(_) => 0,
                    SchemaDefinition::EventKind(value) => 1_usize
                        .checked_add(value.roles().len())?
                        .checked_add(value.attributes().len())?,
                };
                count.checked_add(added)
            })
            .ok_or(LookupIndexError::ResourceBudgetExceeded)?;
        let bytes = estimated_index_memory_bytes(entry_count, 384)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let reservation = reserve_index_memory_with_budget(budget, bytes)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(entry_count)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        for (source_position, definition) in definitions.iter().enumerate() {
            let location = checked_location(source_position)?;
            match definition {
                SchemaDefinition::Layer(value) => entries.push(SchemaIdRevisionIndexEntry::new(
                    SchemaRecordRef::Layer(value.layer_id()),
                    value.created_revision(),
                    location,
                )),
                SchemaDefinition::LayerSnapshot(_) => {}
                SchemaDefinition::EntityType(value) => {
                    entries.push(SchemaIdRevisionIndexEntry::new(
                        SchemaRecordRef::EntityType(value.entity_type_id()),
                        SchemaRevision::from_published_revision(value.created_revision()),
                        location,
                    ))
                }
                SchemaDefinition::Predicate(value) => {
                    entries.push(SchemaIdRevisionIndexEntry::new(
                        SchemaRecordRef::Predicate(value.predicate_id()),
                        SchemaRevision::from_published_revision(value.created_revision()),
                        location,
                    ))
                }
                SchemaDefinition::EventKind(value) => {
                    let revision =
                        SchemaRevision::from_published_revision(value.created_revision());
                    entries.push(SchemaIdRevisionIndexEntry::new(
                        SchemaRecordRef::EventKind(value.event_kind_id()),
                        revision,
                        location,
                    ));
                    for role in value.roles() {
                        entries.push(SchemaIdRevisionIndexEntry::new(
                            SchemaRecordRef::EventRole(role.event_role_id()),
                            revision,
                            location,
                        ));
                    }
                    for attribute in value.attributes() {
                        entries.push(SchemaIdRevisionIndexEntry::new(
                            SchemaRecordRef::EventAttribute(attribute.event_attribute_id()),
                            revision,
                            location,
                        ));
                    }
                }
                SchemaDefinition::Timeline(value) => {
                    entries.push(SchemaIdRevisionIndexEntry::new(
                        SchemaRecordRef::Timeline(value.timeline_id()),
                        SchemaRevision::from_published_revision(value.created_revision()),
                        location,
                    ));
                }
                SchemaDefinition::TimeUnit(_) => {}
            }
        }
        Self::build_with_reservation(entries, reservation)
    }

    /// Resolves one exact schema identity and revision.
    #[must_use]
    pub fn lookup(
        &self,
        schema_id: SchemaRecordRef,
        revision: SchemaRevision,
    ) -> Option<&SchemaIdRevisionIndexEntry> {
        self.by_revision
            .get(&(schema_id, revision))
            .and_then(|position| self.entries.get(*position))
    }

    /// Returns every indexed revision for one schema ID in ascending order.
    pub fn history(
        &self,
        schema_id: SchemaRecordRef,
    ) -> impl Iterator<Item = &SchemaIdRevisionIndexEntry> {
        self.by_id
            .get(&schema_id)
            .into_iter()
            .flatten()
            .filter_map(|position| self.entries.get(*position))
    }
}

/// One lifecycle edge from a base record to an immutable lifecycle record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleIndexEntry {
    target: LifecycleTargetRef,
    lifecycle_record: RecordRef,
    revision: Revision,
    location: u64,
}

impl LifecycleIndexEntry {
    /// Creates a validated target-to-lifecycle-record edge.
    pub fn new(
        target: LifecycleTargetRef,
        lifecycle_record: RecordRef,
        revision: Revision,
        location: u64,
    ) -> Result<Self, LookupIndexError> {
        if !lifecycle_target_matches(lifecycle_record, target) {
            return Err(LookupIndexError::LifecycleTargetMismatch {
                lifecycle_record,
                target,
            });
        }
        Ok(Self {
            target,
            lifecycle_record,
            revision,
            location,
        })
    }

    /// Base record addressed by this lifecycle event.
    #[must_use]
    pub const fn target(self) -> LifecycleTargetRef {
        self.target
    }

    /// Concrete immutable lifecycle-record reference.
    #[must_use]
    pub const fn lifecycle_record(self) -> RecordRef {
        self.lifecycle_record
    }

    /// Revision at which this lifecycle event was recorded.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Backend-owned location token for the lifecycle payload.
    #[must_use]
    pub const fn location(self) -> u64 {
        self.location
    }
}

/// Lifecycle history lookup by a closed, typed base-record reference.
#[derive(Clone, Debug, Default)]
pub struct LifecycleIndex {
    entries: Vec<LifecycleIndexEntry>,
    by_target: BTreeMap<LifecycleTargetRef, Vec<usize>>,
    by_record: BTreeMap<RecordRef, usize>,
    _memory_reservation: Option<MemoryReservation>,
}

impl LifecycleIndex {
    /// Builds target histories and rejects duplicate lifecycle identities.
    pub fn build(entries: Vec<LifecycleIndexEntry>) -> Result<Self, LookupIndexError> {
        Self::build_with_memory_budget(entries, process_memory_budget())
    }

    /// Builds the index under an explicit shared process memory ledger.
    pub fn build_with_memory_budget(
        entries: Vec<LifecycleIndexEntry>,
        budget: &ProcessMemoryBudget,
    ) -> Result<Self, LookupIndexError> {
        let bytes = estimated_index_memory_bytes(entries.len(), 384)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let reservation = reserve_index_memory_with_budget(budget, bytes)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        Self::build_with_reservation(entries, reservation)
    }

    fn build_with_reservation(
        entries: Vec<LifecycleIndexEntry>,
        reservation: MemoryReservation,
    ) -> Result<Self, LookupIndexError> {
        let mut by_target = BTreeMap::<LifecycleTargetRef, Vec<usize>>::new();
        let mut by_record = BTreeMap::new();
        for (position, entry) in entries.iter().enumerate() {
            if by_record.insert(entry.lifecycle_record, position).is_some() {
                return Err(LookupIndexError::DuplicateLifecycleRecord(
                    entry.lifecycle_record,
                ));
            }
            by_target.entry(entry.target).or_default().push(position);
        }
        for positions in by_target.values_mut() {
            positions.sort_by_key(|position| {
                entries
                    .get(*position)
                    .map(|entry| (entry.revision, entry.lifecycle_record))
            });
        }
        Ok(Self {
            entries,
            by_target,
            by_record,
            _memory_reservation: Some(reservation),
        })
    }

    /// Extracts all indexed domain lifecycle records from a decoded record stream.
    pub fn from_records(records: &[Record]) -> Result<Self, LookupIndexError> {
        Self::from_records_with_memory_budget(records, process_memory_budget())
    }

    /// Extracts and indexes lifecycle records under an explicit memory ledger.
    pub fn from_records_with_memory_budget(
        records: &[Record],
        budget: &ProcessMemoryBudget,
    ) -> Result<Self, LookupIndexError> {
        let entry_count = records
            .iter()
            .filter(|record| Self::lifecycle_record_target(record).is_some())
            .count();
        let bytes = estimated_index_memory_bytes(entry_count, 384)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let reservation = reserve_index_memory_with_budget(budget, bytes)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(entry_count)
            .map_err(|_| LookupIndexError::ResourceBudgetExceeded)?;
        for (source_position, record) in records.iter().enumerate() {
            let location = checked_location(source_position)?;
            if let Some((target, lifecycle_record, revision)) =
                Self::lifecycle_record_target(record)
            {
                entries.push(LifecycleIndexEntry::new(
                    target,
                    lifecycle_record,
                    revision,
                    location,
                )?);
            }
        }
        Self::build_with_reservation(entries, reservation)
    }

    /// Returns the typed lifecycle edge represented by one domain record.
    fn lifecycle_record_target(
        record: &Record,
    ) -> Option<(LifecycleTargetRef, RecordRef, Revision)> {
        match record {
            Record::AssertionValidityClosure(value) => Some((
                LifecycleTargetRef::Assertion(value.assertion_id()),
                RecordRef::AssertionValidityClosure(value.id()),
                value.created_revision(),
            )),
            Record::AssertionRetraction(value) => Some((
                LifecycleTargetRef::Assertion(value.assertion_id()),
                RecordRef::AssertionRetraction(value.id()),
                value.created_revision(),
            )),
            Record::MaskValidityClosure(value) => Some((
                LifecycleTargetRef::Mask(value.mask_id()),
                RecordRef::MaskValidityClosure(value.id()),
                value.created_revision(),
            )),
            Record::MaskRetraction(value) => Some((
                LifecycleTargetRef::Mask(value.mask_id()),
                RecordRef::MaskRetraction(value.id()),
                value.created_revision(),
            )),
            Record::ReplacementBoundaryValidityClosure(value) => Some((
                LifecycleTargetRef::ReplacementBoundary(value.replacement_boundary_id()),
                RecordRef::ReplacementBoundaryValidityClosure(value.id()),
                value.created_revision(),
            )),
            Record::ReplacementBoundaryRetraction(value) => Some((
                LifecycleTargetRef::ReplacementBoundary(value.replacement_boundary_id()),
                RecordRef::ReplacementBoundaryRetraction(value.id()),
                value.created_revision(),
            )),
            Record::EventSpanClosure(value) => Some((
                LifecycleTargetRef::Event(value.event_id()),
                RecordRef::EventSpanClosure(value.id()),
                value.created_revision(),
            )),
            Record::EventRetraction(value) => Some((
                LifecycleTargetRef::Event(value.event_id()),
                RecordRef::EventRetraction(value.id()),
                value.created_revision(),
            )),
            Record::EventMaskRetraction(value) => Some((
                LifecycleTargetRef::EventMask(value.event_mask_id()),
                RecordRef::EventMaskRetraction(value.id()),
                value.created_revision(),
            )),
            Record::EventRelationRetraction(value) => Some((
                LifecycleTargetRef::EventRelation(value.event_relation_id()),
                RecordRef::EventRelationRetraction(value.id()),
                value.created_revision(),
            )),
            Record::EvidenceRetraction(value) => Some((
                LifecycleTargetRef::Evidence(value.evidence_id()),
                RecordRef::EvidenceRetraction(value.id()),
                value.created_revision(),
            )),
            Record::ProvenanceRetraction(value) => Some((
                LifecycleTargetRef::Provenance(value.provenance_id()),
                RecordRef::ProvenanceRetraction(value.id()),
                value.created_revision(),
            )),
            // These closed record families do not use LifecycleTargetRef. Catalog retirement
            // and schema lifecycle remain in their separate typed histories.
            Record::HistorySpaceDefinition(_)
            | Record::Entity(_)
            | Record::EntityRetirement(_)
            | Record::PerspectiveDefinitionRevision(_)
            | Record::PerspectiveRetirement(_)
            | Record::LayerDefinition(_)
            | Record::LayerSchemaSnapshot(_)
            | Record::EntityTypeDefinition(_)
            | Record::PredicateDefinition(_)
            | Record::EventKindDefinition(_)
            | Record::TimelineDefinition(_)
            | Record::TimeUnitDefinition(_)
            | Record::MigrationPlan(_)
            | Record::MigrationRun(_)
            | Record::MigrationStepCommitIdentity(_)
            | Record::Assertion(_)
            | Record::Mask(_)
            | Record::ReplacementBoundary(_)
            | Record::ArchiveTransition(_)
            | Record::Event(_)
            | Record::EventMask(_)
            | Record::EventRelation(_)
            | Record::Source(_)
            | Record::Evidence(_)
            | Record::Provenance(_)
            | Record::TransferLineage(_) => None,
        }
    }

    /// Returns lifecycle events for a target through the selected revision.
    pub fn through(
        &self,
        target: LifecycleTargetRef,
        as_of: Revision,
    ) -> impl Iterator<Item = &LifecycleIndexEntry> {
        self.by_target
            .get(&target)
            .into_iter()
            .flatten()
            .filter_map(|position| self.entries.get(*position))
            .take_while(move |entry| entry.revision <= as_of)
    }

    /// Resolves one concrete lifecycle record by its typed identity.
    #[must_use]
    pub fn lookup_record(&self, record: RecordRef) -> Option<&LifecycleIndexEntry> {
        self.by_record
            .get(&record)
            .and_then(|position| self.entries.get(*position))
    }
}

fn checked_location(position: usize) -> Result<u64, LookupIndexError> {
    u64::try_from(position).map_err(|_| LookupIndexError::LocationSpaceExhausted)
}

fn lifecycle_target_matches(record: RecordRef, target: LifecycleTargetRef) -> bool {
    match record {
        RecordRef::AssertionValidityClosure(_) | RecordRef::AssertionRetraction(_) => {
            matches!(target, LifecycleTargetRef::Assertion(_))
        }
        RecordRef::MaskValidityClosure(_) | RecordRef::MaskRetraction(_) => {
            matches!(target, LifecycleTargetRef::Mask(_))
        }
        RecordRef::ReplacementBoundaryValidityClosure(_)
        | RecordRef::ReplacementBoundaryRetraction(_) => {
            matches!(target, LifecycleTargetRef::ReplacementBoundary(_))
        }
        RecordRef::EventSpanClosure(_) | RecordRef::EventRetraction(_) => {
            matches!(target, LifecycleTargetRef::Event(_))
        }
        RecordRef::EventMaskRetraction(_) => matches!(target, LifecycleTargetRef::EventMask(_)),
        RecordRef::EventRelationRetraction(_) => {
            matches!(target, LifecycleTargetRef::EventRelation(_))
        }
        RecordRef::EvidenceRetraction(_) => matches!(target, LifecycleTargetRef::Evidence(_)),
        RecordRef::ProvenanceRetraction(_) => matches!(target, LifecycleTargetRef::Provenance(_)),
        RecordRef::Assertion(_)
        | RecordRef::Mask(_)
        | RecordRef::ReplacementBoundary(_)
        | RecordRef::Event(_)
        | RecordRef::EventMask(_)
        | RecordRef::EventRelation(_)
        | RecordRef::Source(_)
        | RecordRef::Evidence(_)
        | RecordRef::Provenance(_)
        | RecordRef::EntityRetirement(_)
        | RecordRef::PerspectiveRetirement(_)
        | RecordRef::ArchiveTransition(_)
        | RecordRef::TransferLineage(_) => false,
    }
}

/// Invalid duplicate index keys or a lifecycle edge that crosses record families.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LookupIndexError {
    /// The shared process index-memory admission budget was exhausted.
    ResourceBudgetExceeded,
    /// One typed domain RecordRef appears more than once.
    DuplicateRecordId(RecordRef),
    /// One durable OperationId appears more than once.
    DuplicateOperationId(OperationId),
    /// One schema identity repeats a published schema revision.
    DuplicateSchemaRevision {
        schema_id: SchemaRecordRef,
        revision: SchemaRevision,
    },
    /// One concrete lifecycle RecordRef appears more than once.
    DuplicateLifecycleRecord(RecordRef),
    /// Lifecycle record type does not address the supplied target family.
    LifecycleTargetMismatch {
        lifecycle_record: RecordRef,
        target: LifecycleTargetRef,
    },
    /// The derived entry count cannot be represented by the locator type.
    LocationSpaceExhausted,
}

impl fmt::Display for LookupIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceBudgetExceeded => {
                formatter.write_str("lookup index exceeded the process index-memory budget")
            }
            Self::DuplicateRecordId(record) => write!(formatter, "duplicate record ID {record:?}"),
            Self::DuplicateOperationId(operation) => {
                write!(formatter, "duplicate operation ID {operation}")
            }
            Self::DuplicateSchemaRevision {
                schema_id,
                revision,
            } => write!(
                formatter,
                "schema identity {schema_id:?} repeats revision {revision}"
            ),
            Self::DuplicateLifecycleRecord(record) => {
                write!(formatter, "duplicate lifecycle record {record:?}")
            }
            Self::LifecycleTargetMismatch {
                lifecycle_record,
                target,
            } => write!(
                formatter,
                "lifecycle record {lifecycle_record:?} cannot target {target:?}"
            ),
            Self::LocationSpaceExhausted => {
                formatter.write_str("lookup index location space exhausted")
            }
        }
    }
}

impl std::error::Error for LookupIndexError {}

#[cfg(test)]
mod tests {
    use super::{
        LifecycleIndex, LifecycleIndexEntry, LookupIndexError, OperationIdIndex,
        OperationIdIndexEntry, OperationIndexStatus, RecordIdIndex, RecordIdIndexEntry,
        SchemaIdRevisionIndex, SchemaIdRevisionIndexEntry,
    };
    use crate::ids::{
        AssertionId, AssertionRetractionId, DomainId, EntityTypeId, EventAttributeId, EventId,
        EventKindId, EventRoleId, LayerId, OperationId, PredicateId, Revision, SchemaRevision,
    };
    use crate::record_refs::{LifecycleTargetRef, RecordRef, SchemaRecordRef};
    use crate::resource_profile::{ProcessResourceProfile, ResourceClass};
    use crate::schema::{
        ConstraintSet, EntityTypeConstraint, EntityTypeDefinition, EventAttributeDefinition,
        EventKindDefinition, EventRoleDefinition, EventTimeConstraint, EventTimeForm, Lifecycle,
        RoleCardinality, SchemaDefinitionError, ValueKind,
    };
    use crate::schema_history::SchemaDefinition;
    use crate::wire_records::Record;
    use crate::{LayerDefinition, Symbol};

    #[derive(Debug)]
    enum TestError {
        Id(crate::ids::IdValidationError),
        Revision(crate::ids::RevisionError),
        Index(LookupIndexError),
        Resource(crate::ResourceBudgetError),
        Symbol(crate::SymbolError),
        Schema(SchemaDefinitionError),
    }

    impl From<crate::ids::IdValidationError> for TestError {
        fn from(error: crate::ids::IdValidationError) -> Self {
            Self::Id(error)
        }
    }

    impl From<crate::ids::RevisionError> for TestError {
        fn from(error: crate::ids::RevisionError) -> Self {
            Self::Revision(error)
        }
    }

    impl From<LookupIndexError> for TestError {
        fn from(error: LookupIndexError) -> Self {
            Self::Index(error)
        }
    }

    impl From<crate::ResourceBudgetError> for TestError {
        fn from(error: crate::ResourceBudgetError) -> Self {
            Self::Resource(error)
        }
    }

    impl From<crate::SymbolError> for TestError {
        fn from(error: crate::SymbolError) -> Self {
            Self::Symbol(error)
        }
    }

    impl From<SchemaDefinitionError> for TestError {
        fn from(error: SchemaDefinitionError) -> Self {
            Self::Schema(error)
        }
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Id(error) => std::fmt::Display::fmt(error, formatter),
                Self::Revision(error) => std::fmt::Display::fmt(error, formatter),
                Self::Index(error) => std::fmt::Display::fmt(error, formatter),
                Self::Resource(error) => std::fmt::Display::fmt(error, formatter),
                Self::Symbol(error) => std::fmt::Display::fmt(error, formatter),
                Self::Schema(error) => std::fmt::Display::fmt(error, formatter),
            }
        }
    }

    impl std::error::Error for TestError {}

    type TestResult = Result<(), TestError>;

    fn uuid<T: DomainId>(value: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0; 16];
        bytes[6] = 0x40;
        bytes[8] = 0x80;
        bytes[15] = value;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, crate::ids::RevisionError> {
        Revision::new(value)
    }

    fn schema_revision(value: u64) -> Result<SchemaRevision, crate::ids::RevisionError> {
        Ok(SchemaRevision::from_published_revision(revision(value)?))
    }

    #[test]
    fn lookup_indexes_admit_and_release_their_shared_memory_reservation() -> TestResult {
        let profile = ProcessResourceProfile::new(
            1,
            512,
            1024,
            crate::ResourceClassLimits::new(512, 512, 512, 512, 512),
        )?;
        let budget = profile.memory_budget();
        let entry = RecordIdIndexEntry::new(
            RecordRef::Assertion(uuid::<AssertionId>(90)?),
            revision(1)?,
            0,
        );
        let index = RecordIdIndex::build_with_memory_budget(vec![entry], &budget)?;
        assert_eq!(budget.reserved_bytes(), Ok(192));
        assert_eq!(
            budget.profile().class_limit_bytes(ResourceClass::Index),
            512
        );
        drop(index);
        assert_eq!(budget.reserved_bytes(), Ok(0));

        let small_profile = ProcessResourceProfile::new(
            1,
            512,
            1024,
            crate::ResourceClassLimits::new(512, 512, 100, 512, 512),
        )?;
        let small_budget = small_profile.memory_budget();
        let entry = RecordIdIndexEntry::new(
            RecordRef::Assertion(uuid::<AssertionId>(91)?),
            revision(1)?,
            0,
        );
        assert_eq!(
            RecordIdIndex::build_with_memory_budget(vec![entry], &small_budget).err(),
            Some(LookupIndexError::ResourceBudgetExceeded)
        );
        assert_eq!(small_budget.reserved_bytes(), Ok(0));
        Ok(())
    }

    #[test]
    fn record_id_hits_and_misses_match_the_full_scan_oracle() -> TestResult {
        let rows = vec![
            RecordIdIndexEntry::new(
                RecordRef::Assertion(uuid::<AssertionId>(1)?),
                revision(1)?,
                11,
            ),
            RecordIdIndexEntry::new(RecordRef::Event(uuid::<EventId>(2)?), revision(2)?, 22),
        ];
        let index = RecordIdIndex::build(rows.clone())?;
        for query in rows.iter().map(|row| row.record()) {
            let oracle = rows.iter().find(|row| row.record() == query).copied();
            assert_eq!(index.lookup(query).copied(), oracle);
        }
        let missing = RecordRef::Assertion(uuid::<AssertionId>(9)?);
        assert_eq!(
            index.lookup(missing),
            rows.iter().find(|row| row.record() == missing)
        );
        Ok(())
    }

    #[test]
    fn operation_id_hits_and_misses_match_the_full_scan_oracle() -> TestResult {
        let rows = vec![
            OperationIdIndexEntry::new(
                uuid::<OperationId>(3)?,
                OperationIndexStatus::Indeterminate,
                30,
            ),
            OperationIdIndexEntry::new(
                uuid::<OperationId>(4)?,
                OperationIndexStatus::Committed {
                    revision: revision(4)?,
                },
                40,
            ),
        ];
        let index = OperationIdIndex::build(rows.clone())?;
        for query in rows.iter().map(|row| row.operation_id()) {
            let oracle = rows.iter().find(|row| row.operation_id() == query).copied();
            assert_eq!(index.lookup(query).copied(), oracle);
        }
        let missing = uuid::<OperationId>(8)?;
        assert_eq!(
            index.lookup(missing),
            rows.iter().find(|row| row.operation_id() == missing)
        );
        Ok(())
    }

    #[test]
    fn schema_id_revision_and_history_match_the_full_scan_oracle() -> TestResult {
        let layer = SchemaRecordRef::Layer(uuid::<LayerId>(5)?);
        let predicate = SchemaRecordRef::Predicate(uuid::<PredicateId>(6)?);
        let rows = vec![
            SchemaIdRevisionIndexEntry::new(layer, schema_revision(3)?, 31),
            SchemaIdRevisionIndexEntry::new(predicate, schema_revision(3)?, 32),
            SchemaIdRevisionIndexEntry::new(layer, schema_revision(1)?, 11),
        ];
        let index = SchemaIdRevisionIndex::build(rows.clone())?;
        for row in &rows {
            let oracle = rows.iter().find(|candidate| {
                candidate.schema_id() == row.schema_id() && candidate.revision() == row.revision()
            });
            assert_eq!(index.lookup(row.schema_id(), row.revision()), oracle);
        }
        let expected = rows
            .iter()
            .filter(|row| row.schema_id() == layer)
            .map(|row| row.revision())
            .collect::<Vec<_>>();
        assert_eq!(
            index
                .history(layer)
                .map(|row| row.revision())
                .collect::<Vec<_>>(),
            {
                let mut sorted = expected;
                sorted.sort();
                sorted
            }
        );
        assert_eq!(index.lookup(layer, schema_revision(2)?), None);
        Ok(())
    }

    #[test]
    fn schema_index_builds_from_typed_schema_definitions() -> TestResult {
        let earlier_id = uuid::<EntityTypeId>(11)?;
        let entity_type_id = uuid::<EntityTypeId>(12)?;
        let schema_id = SchemaRecordRef::EntityType(entity_type_id);
        let earlier = SchemaDefinition::EntityType(EntityTypeDefinition::new(
            earlier_id,
            Symbol::new("person")?,
            None,
            Lifecycle::Active,
            revision(1)?,
        ));
        let role_id = uuid::<EventRoleId>(14)?;
        let attribute_id = uuid::<EventAttributeId>(15)?;
        let event_kind_id = uuid::<EventKindId>(13)?;
        let definition = SchemaDefinition::EventKind(EventKindDefinition::new(
            event_kind_id,
            Symbol::new("occurred")?,
            vec![EventRoleDefinition::new(
                role_id,
                Symbol::new("actor")?,
                EntityTypeConstraint::Exact(entity_type_id),
                RoleCardinality::new(1, Some(1))?,
            )],
            vec![EventAttributeDefinition::new(
                attribute_id,
                Symbol::new("summary")?,
                ValueKind::String,
                None,
                ConstraintSet::unconstrained(),
                None,
                true,
            )?],
            EventTimeConstraint::new(EventTimeForm::InstantOnly, None)?,
            Lifecycle::Active,
            revision(2)?,
        )?);
        let index = SchemaIdRevisionIndex::from_definitions(&[earlier, definition])?;
        let schema_revision = schema_revision(2)?;
        let event_kind = SchemaRecordRef::EventKind(event_kind_id);
        let event_role = SchemaRecordRef::EventRole(role_id);
        let event_attribute = SchemaRecordRef::EventAttribute(attribute_id);
        assert_eq!(
            index
                .lookup(event_kind, schema_revision)
                .map(|row| row.location()),
            Some(1)
        );
        assert_eq!(
            index
                .lookup(event_role, schema_revision)
                .map(|row| row.location()),
            Some(1)
        );
        assert_eq!(
            index
                .lookup(event_attribute, schema_revision)
                .map(|row| row.location()),
            Some(1)
        );
        assert_eq!(index.history(event_kind).count(), 1);
        assert_eq!(index.lookup(schema_id, schema_revision), None);
        Ok(())
    }

    #[test]
    fn lifecycle_history_and_type_safety_match_the_full_scan_oracle() -> TestResult {
        let assertion = uuid::<AssertionId>(7)?;
        let target = LifecycleTargetRef::Assertion(assertion);
        let row = LifecycleIndexEntry::new(
            target,
            RecordRef::AssertionRetraction(uuid::<AssertionRetractionId>(9)?),
            revision(4)?,
            49,
        )?;
        let rows = vec![row];
        let index = LifecycleIndex::build(rows.clone())?;
        let as_of = revision(4)?;
        let oracle = rows
            .iter()
            .filter(|row| row.target() == target && row.revision() <= as_of)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(
            index.through(target, as_of).copied().collect::<Vec<_>>(),
            oracle
        );
        assert!(index.through(target, revision(3)?).next().is_none());
        assert_eq!(index.lookup_record(row.lifecycle_record()), Some(&row));
        assert!(matches!(
            LifecycleIndexEntry::new(
                LifecycleTargetRef::Event(uuid::<EventId>(1)?),
                row.lifecycle_record(),
                revision(5)?,
                50,
            ),
            Err(LookupIndexError::LifecycleTargetMismatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn lifecycle_index_extracts_typed_records_from_the_history_stream() -> TestResult {
        let assertion_id = uuid::<AssertionId>(21)?;
        let lifecycle_id = uuid::<AssertionRetractionId>(22)?;
        let records = [
            Record::LayerDefinition(LayerDefinition::new(
                uuid::<LayerId>(20)?,
                Symbol::new("base")?,
                None,
                0,
                Lifecycle::Active,
                schema_revision(1)?,
            )),
            Record::AssertionRetraction(crate::assertions::AssertionRetraction::from_wire_fields(
                lifecycle_id,
                assertion_id,
                "corrected".to_owned(),
                revision(4)?,
            )),
        ];
        let index = LifecycleIndex::from_records(&records)?;
        let target = LifecycleTargetRef::Assertion(assertion_id);
        assert_eq!(
            index
                .through(target, revision(4)?)
                .map(|entry| entry.lifecycle_record())
                .collect::<Vec<_>>(),
            vec![RecordRef::AssertionRetraction(lifecycle_id)]
        );
        assert!(index.through(target, revision(3)?).next().is_none());
        assert_eq!(
            index
                .lookup_record(RecordRef::AssertionRetraction(lifecycle_id))
                .map(|row| row.location()),
            Some(1)
        );
        Ok(())
    }

    #[test]
    fn duplicate_keys_are_rejected_for_every_lookup_family() -> TestResult {
        let record = RecordRef::Assertion(uuid::<AssertionId>(1)?);
        assert!(matches!(
            RecordIdIndex::build(vec![
                RecordIdIndexEntry::new(record, revision(1)?, 1),
                RecordIdIndexEntry::new(record, revision(2)?, 2),
            ]),
            Err(LookupIndexError::DuplicateRecordId(_))
        ));

        let operation = uuid::<OperationId>(2)?;
        assert!(matches!(
            OperationIdIndex::build(vec![
                OperationIdIndexEntry::new(operation, OperationIndexStatus::Indeterminate, 1),
                OperationIdIndexEntry::new(operation, OperationIndexStatus::Indeterminate, 2),
            ]),
            Err(LookupIndexError::DuplicateOperationId(_))
        ));

        let schema = SchemaRecordRef::Layer(uuid::<LayerId>(3)?);
        let repeated_revision = schema_revision(1)?;
        assert!(matches!(
            SchemaIdRevisionIndex::build(vec![
                SchemaIdRevisionIndexEntry::new(schema, repeated_revision, 1),
                SchemaIdRevisionIndexEntry::new(schema, repeated_revision, 2),
            ]),
            Err(LookupIndexError::DuplicateSchemaRevision { .. })
        ));
        Ok(())
    }
}
