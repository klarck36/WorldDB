//! Deterministic, explicitly planned import of canonical logical exports.

use std::collections::BTreeSet;
use std::fmt;

use worlddb_core::{
    ArchiveTargetRef, DatabaseId, DomainId, EntityId, EntityTypeConstraint, EntityTypeId,
    EventAttributeId, EventKindId, EventRoleId, EvidenceTargetRef, HistorySpaceId, LayerId,
    MaskSelector, PerspectiveId, PerspectiveScope, PredicateId, ProvenanceEndpointRef, Record,
    RecordRef, TimelineId, Value, decode_record_ref, encode_record_ref,
};

use crate::logical_export::{
    LogicalExport, LogicalExportEntry, LogicalExportError, record_ref, validate_import_references,
};

const LOGICAL_IMPORT_PLAN_MAGIC: &[u8; 8] = b"WDBLIP\0\x01";
const LOGICAL_IMPORT_PLAN_CONTEXT: &[u8] = b"WorldDB.LogicalImportPlan.v1\0";
const LOGICAL_IMPORT_STREAM_CONTEXT: &[u8] = b"WorldDB.LogicalImportStream.v1\0";
const LOGICAL_IMPORT_MAX_PLAN_BYTES: usize = 64 * 1024 * 1024;
const LOGICAL_IMPORT_MAX_MAPPINGS: usize = 1_000_000;
const LOGICAL_IMPORT_HEADER_BYTES: usize = 8 + 8;
const LOGICAL_IMPORT_DIGEST_BYTES: usize = 32;

/// One identity family understood by the logical-import planner.
///
/// Mappings are typed: an EntityId cannot be mapped to a LayerId, and one
/// RecordRef variant cannot be mapped to another variant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LogicalImportIdentity {
    /// A HistorySpace and its ancestry references.
    HistorySpace(HistorySpaceId),
    /// A project layer identity.
    Layer(LayerId),
    /// A project perspective identity.
    Perspective(PerspectiveId),
    /// A project timeline identity.
    Timeline(TimelineId),
    /// An entity identity.
    Entity(EntityId),
    /// An entity-type schema identity.
    EntityType(EntityTypeId),
    /// A predicate schema identity.
    Predicate(PredicateId),
    /// An event-kind schema identity.
    EventKind(EventKindId),
    /// An event-role schema identity.
    EventRole(EventRoleId),
    /// An event-attribute schema identity.
    EventAttribute(EventAttributeId),
    /// A closed typed identity for one durable record.
    Record(RecordRef),
}

impl LogicalImportIdentity {
    /// Returns every durable identity defined by one record.
    ///
    /// A record may define several identities (for example, an event-kind schema
    /// definition owns its role and attribute identities). Callers that build a
    /// destination inventory should deduplicate these values across revisions.
    #[must_use]
    pub fn defined_by_record(record: &Record) -> Vec<Self> {
        record_defined_identities(record)
    }

    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::HistorySpace(_) => 1,
            Self::Layer(_) => 2,
            Self::Perspective(_) => 3,
            Self::Timeline(_) => 4,
            Self::Entity(_) => 5,
            Self::EntityType(_) => 6,
            Self::Predicate(_) => 7,
            Self::EventKind(_) => 8,
            Self::EventRole(_) => 9,
            Self::EventAttribute(_) => 10,
            Self::Record(_) => 11,
        }
    }

    fn same_family(self, other: Self) -> bool {
        match (self, other) {
            (Self::Record(left), Self::Record(right)) => left.wire_tag() == right.wire_tag(),
            _ => self.tag() == other.tag(),
        }
    }

    pub(crate) fn encode(self, output: &mut Vec<u8>) -> Result<(), LogicalImportError> {
        output.push(self.tag());
        match self {
            Self::HistorySpace(id) => output.extend_from_slice(&id.to_bytes()),
            Self::Layer(id) => output.extend_from_slice(&id.to_bytes()),
            Self::Perspective(id) => output.extend_from_slice(&id.to_bytes()),
            Self::Timeline(id) => output.extend_from_slice(&id.to_bytes()),
            Self::Entity(id) => output.extend_from_slice(&id.to_bytes()),
            Self::EntityType(id) => output.extend_from_slice(&id.to_bytes()),
            Self::Predicate(id) => output.extend_from_slice(&id.to_bytes()),
            Self::EventKind(id) => output.extend_from_slice(&id.to_bytes()),
            Self::EventRole(id) => output.extend_from_slice(&id.to_bytes()),
            Self::EventAttribute(id) => output.extend_from_slice(&id.to_bytes()),
            Self::Record(reference) => {
                let bytes = encode_record_ref(reference).map_err(LogicalImportError::Record)?;
                let length =
                    u8::try_from(bytes.len()).map_err(|_| LogicalImportError::ResourceLimit)?;
                output.push(length);
                output.extend_from_slice(&bytes);
            }
        }
        Ok(())
    }

    fn decode(tag: u8, cursor: &mut Cursor<'_>) -> Result<Self, LogicalImportError> {
        Ok(match tag {
            1 => Self::HistorySpace(decode_id(cursor.array_16()?)?),
            2 => Self::Layer(decode_id(cursor.array_16()?)?),
            3 => Self::Perspective(decode_id(cursor.array_16()?)?),
            4 => Self::Timeline(decode_id(cursor.array_16()?)?),
            5 => Self::Entity(decode_id(cursor.array_16()?)?),
            6 => Self::EntityType(decode_id(cursor.array_16()?)?),
            7 => Self::Predicate(decode_id(cursor.array_16()?)?),
            8 => Self::EventKind(decode_id(cursor.array_16()?)?),
            9 => Self::EventRole(decode_id(cursor.array_16()?)?),
            10 => Self::EventAttribute(decode_id(cursor.array_16()?)?),
            11 => {
                let length = usize::from(cursor.u8()?);
                if !(17..=18).contains(&length) {
                    return Err(LogicalImportError::InvalidPlanEncoding);
                }
                Self::Record(
                    decode_record_ref(cursor.take(length)?).map_err(LogicalImportError::Record)?,
                )
            }
            _ => return Err(LogicalImportError::InvalidPlanEncoding),
        })
    }
}

fn decode_id<T: DomainId>(bytes: [u8; 16]) -> Result<T, LogicalImportError> {
    T::try_from_bytes(bytes).map_err(|_| LogicalImportError::InvalidPlanEncoding)
}

/// One explicitly approved source-to-destination identity assignment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LogicalImportIdMapping {
    source: LogicalImportIdentity,
    target: LogicalImportIdentity,
}

impl LogicalImportIdMapping {
    /// Creates a mapping between identities from the same closed type family.
    pub fn new(
        source: LogicalImportIdentity,
        target: LogicalImportIdentity,
    ) -> Result<Self, LogicalImportError> {
        if !source.same_family(target) {
            return Err(LogicalImportError::IdentityFamilyMismatch);
        }
        if source == target {
            return Err(LogicalImportError::IdentityMappingIsUnchanged);
        }
        Ok(Self { source, target })
    }

    /// Identity from the source export.
    #[must_use]
    pub const fn source(self) -> LogicalImportIdentity {
        self.source
    }

    /// Identity assigned by this import plan.
    #[must_use]
    pub const fn target(self) -> LogicalImportIdentity {
        self.target
    }
}

/// Canonical, digest-protected record of one explicit ID-remap decision set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalImportPlan {
    source_database_id: DatabaseId,
    source_artifact_digest: [u8; 32],
    destination_database_id: DatabaseId,
    mappings: Vec<LogicalImportIdMapping>,
}

impl LogicalImportPlan {
    /// Binds a sorted mapping set to exact canonical export bytes and a target database.
    pub fn new(
        source_artifact: &[u8],
        destination_database_id: DatabaseId,
        mut mappings: Vec<LogicalImportIdMapping>,
    ) -> Result<Self, LogicalImportError> {
        let source = LogicalExport::decode(source_artifact).map_err(LogicalImportError::Export)?;
        if mappings.len() > LOGICAL_IMPORT_MAX_MAPPINGS {
            return Err(LogicalImportError::ResourceLimit);
        }
        mappings.sort_unstable_by_key(|mapping| mapping.source);
        validate_mapping_set(&mappings)?;
        Ok(Self {
            source_database_id: source.manifest().database_id(),
            source_artifact_digest: artifact_digest(source_artifact),
            destination_database_id,
            mappings,
        })
    }

    /// Source database identity recorded in the bound export.
    #[must_use]
    pub const fn source_database_id(&self) -> DatabaseId {
        self.source_database_id
    }

    /// Destination database identity this plan targets.
    #[must_use]
    pub const fn destination_database_id(&self) -> DatabaseId {
        self.destination_database_id
    }

    /// Exact source-to-target remaps, sorted by source identity.
    #[must_use]
    pub fn mappings(&self) -> &[LogicalImportIdMapping] {
        &self.mappings
    }

    /// Encodes a canonical plan with a domain-separated BLAKE3 integrity digest.
    pub fn encode(&self) -> Result<Vec<u8>, LogicalImportError> {
        validate_mapping_set(&self.mappings)?;
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.source_database_id.to_bytes());
        payload.extend_from_slice(&self.source_artifact_digest);
        payload.extend_from_slice(&self.destination_database_id.to_bytes());
        push_u32(
            &mut payload,
            u32::try_from(self.mappings.len()).map_err(|_| LogicalImportError::ResourceLimit)?,
        );
        for mapping in &self.mappings {
            mapping.source.encode(&mut payload)?;
            mapping.target.encode(&mut payload)?;
            if payload.len() > LOGICAL_IMPORT_MAX_PLAN_BYTES {
                return Err(LogicalImportError::ResourceLimit);
            }
        }

        let total_len = LOGICAL_IMPORT_HEADER_BYTES
            .checked_add(payload.len())
            .and_then(|length| length.checked_add(LOGICAL_IMPORT_DIGEST_BYTES))
            .ok_or(LogicalImportError::ResourceLimit)?;
        if total_len > LOGICAL_IMPORT_MAX_PLAN_BYTES {
            return Err(LogicalImportError::ResourceLimit);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total_len)
            .map_err(|_| LogicalImportError::AllocationFailed)?;
        bytes.extend_from_slice(LOGICAL_IMPORT_PLAN_MAGIC);
        push_u64(
            &mut bytes,
            u64::try_from(payload.len()).map_err(|_| LogicalImportError::ResourceLimit)?,
        );
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&plan_digest(&bytes));
        Ok(bytes)
    }

    /// Decodes one plan and rejects altered, noncanonical, or unbounded encodings.
    pub fn decode(bytes: &[u8]) -> Result<Self, LogicalImportError> {
        if bytes.len() < LOGICAL_IMPORT_HEADER_BYTES + LOGICAL_IMPORT_DIGEST_BYTES
            || bytes.len() > LOGICAL_IMPORT_MAX_PLAN_BYTES
            || bytes.get(..8) != Some(LOGICAL_IMPORT_PLAN_MAGIC.as_slice())
        {
            return Err(LogicalImportError::InvalidPlanEncoding);
        }
        let digest_offset = bytes
            .len()
            .checked_sub(LOGICAL_IMPORT_DIGEST_BYTES)
            .ok_or(LogicalImportError::InvalidPlanEncoding)?;
        let claimed: [u8; 32] = bytes
            .get(digest_offset..)
            .ok_or(LogicalImportError::InvalidPlanEncoding)?
            .try_into()
            .map_err(|_| LogicalImportError::InvalidPlanEncoding)?;
        if plan_digest(
            bytes
                .get(..digest_offset)
                .ok_or(LogicalImportError::InvalidPlanEncoding)?,
        ) != claimed
        {
            return Err(LogicalImportError::PlanDigestMismatch);
        }
        let payload_length =
            usize::try_from(read_u64(bytes, 8)?).map_err(|_| LogicalImportError::ResourceLimit)?;
        let payload_end = LOGICAL_IMPORT_HEADER_BYTES
            .checked_add(payload_length)
            .ok_or(LogicalImportError::InvalidPlanEncoding)?;
        if payload_end != digest_offset {
            return Err(LogicalImportError::InvalidPlanEncoding);
        }
        let mut cursor = Cursor::new(
            bytes
                .get(LOGICAL_IMPORT_HEADER_BYTES..payload_end)
                .ok_or(LogicalImportError::InvalidPlanEncoding)?,
        );
        let source_database_id = decode_id(cursor.array_16()?)?;
        let source_artifact_digest = cursor.array_32()?;
        let destination_database_id = decode_id(cursor.array_16()?)?;
        let mapping_count = cursor.count(LOGICAL_IMPORT_MAX_MAPPINGS)?;
        let mut mappings = Vec::new();
        mappings
            .try_reserve_exact(mapping_count)
            .map_err(|_| LogicalImportError::AllocationFailed)?;
        for _ in 0..mapping_count {
            let source = LogicalImportIdentity::decode(cursor.u8()?, &mut cursor)?;
            let target = LogicalImportIdentity::decode(cursor.u8()?, &mut cursor)?;
            mappings.push(LogicalImportIdMapping::new(source, target)?);
        }
        if !cursor.is_empty() {
            return Err(LogicalImportError::InvalidPlanEncoding);
        }
        validate_mapping_set(&mappings)?;
        if mappings
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left.source >= right.source))
        {
            return Err(LogicalImportError::NonCanonicalPlan);
        }
        let plan = Self {
            source_database_id,
            source_artifact_digest,
            destination_database_id,
            mappings,
        };
        if plan.encode()?.as_slice() != bytes {
            return Err(LogicalImportError::NonCanonicalPlan);
        }
        Ok(plan)
    }
}

/// Current typed identities already present in the destination database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalImportDestinationInventory {
    database_id: DatabaseId,
    identities: BTreeSet<LogicalImportIdentity>,
}

impl LogicalImportDestinationInventory {
    /// Builds a closed, duplicate-free destination identity inventory.
    pub fn new(
        database_id: DatabaseId,
        identities: impl IntoIterator<Item = LogicalImportIdentity>,
    ) -> Result<Self, LogicalImportError> {
        let mut known = BTreeSet::new();
        for identity in identities {
            if !known.insert(identity) {
                return Err(LogicalImportError::DuplicateDestinationIdentity(identity));
            }
        }
        Ok(Self {
            database_id,
            identities: known,
        })
    }

    /// Identity of the destination snapshot from which the inventory was read.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Known durable identities in that destination snapshot.
    #[must_use]
    pub fn identities(&self) -> &BTreeSet<LogicalImportIdentity> {
        &self.identities
    }
}

/// A validated import stream bound to one source artifact, destination, and ID plan.
#[derive(Clone, Debug)]
pub struct LogicalImport {
    source: LogicalExport,
    plan: LogicalImportPlan,
    stream_fingerprint: [u8; 32],
}

impl LogicalImport {
    /// Canonically ordered source records to be interpreted using this plan.
    #[must_use]
    pub fn records(&self) -> &[LogicalExportEntry] {
        self.source.records()
    }

    /// The exact source logical export, including its scope and source snapshot.
    #[must_use]
    pub const fn source(&self) -> &LogicalExport {
        &self.source
    }

    /// The explicit protocolled identity assignment used for this import.
    #[must_use]
    pub const fn plan(&self) -> &LogicalImportPlan {
        &self.plan
    }

    /// Stable fingerprint of the decoded record stream plus its complete plan binding.
    #[must_use]
    pub const fn stream_fingerprint(&self) -> [u8; 32] {
        self.stream_fingerprint
    }

    /// Resolves any typed source identity through the exact same mapping table.
    #[must_use]
    pub fn map_identity(&self, identity: LogicalImportIdentity) -> LogicalImportIdentity {
        self.plan
            .mappings
            .binary_search_by_key(&identity, |mapping| mapping.source)
            .ok()
            .and_then(|index| self.plan.mappings.get(index))
            .map_or(identity, |mapping| mapping.target)
    }

    /// Returns the target identity assigned to one included durable record.
    #[must_use]
    pub fn mapped_record_identity(&self, record: &Record) -> Option<RecordRef> {
        let source = record_ref(record)?;
        match self.map_identity(LogicalImportIdentity::Record(source)) {
            LogicalImportIdentity::Record(target) => Some(target),
            _ => None,
        }
    }
}

/// Prepares deterministic import streams without publishing to persistent storage.
#[derive(Clone, Copy, Debug, Default)]
pub struct LogicalImportManager;

impl LogicalImportManager {
    /// Verifies source bytes, plan binding, dependencies, collisions, and mapped uniqueness.
    pub fn prepare(
        source_artifact: &[u8],
        plan_bytes: &[u8],
        destination: &LogicalImportDestinationInventory,
    ) -> Result<LogicalImport, LogicalImportError> {
        let source = LogicalExport::decode(source_artifact).map_err(LogicalImportError::Export)?;
        let plan = LogicalImportPlan::decode(plan_bytes)?;
        if plan.source_database_id != source.manifest().database_id()
            || plan.source_artifact_digest != artifact_digest(source_artifact)
        {
            return Err(LogicalImportError::SourceArtifactMismatch);
        }
        if plan.destination_database_id != destination.database_id {
            return Err(LogicalImportError::DestinationMismatch);
        }
        validate_import_references(&source).map_err(|_| LogicalImportError::MissingReference)?;
        let identities = collect_importable_identities(&source)?;
        validate_plan_against_inventories(&plan, &identities, destination)?;
        validate_referenced_identities(&source, &identities, &plan, destination)?;

        let encoded_plan = plan.encode()?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(LOGICAL_IMPORT_STREAM_CONTEXT);
        hasher.update(&artifact_digest(source_artifact));
        hasher.update(&encoded_plan);
        let stream_fingerprint = *hasher.finalize().as_bytes();
        Ok(LogicalImport {
            source,
            plan,
            stream_fingerprint,
        })
    }
}

fn collect_importable_identities(
    source: &LogicalExport,
) -> Result<BTreeSet<LogicalImportIdentity>, LogicalImportError> {
    let mut identities = BTreeSet::new();
    for space in source.manifest().visible_history_spaces() {
        identities.insert(LogicalImportIdentity::HistorySpace(space.id()));
    }
    for entry in source.records() {
        let value = entry.record().record();
        match value {
            Record::HistorySpaceDefinition(definition) => {
                identities.insert(LogicalImportIdentity::HistorySpace(
                    definition.history_space_id(),
                ));
            }
            Record::Entity(entity) => {
                identities.insert(LogicalImportIdentity::Entity(entity.entity_id()));
            }
            Record::PerspectiveDefinitionRevision(definition) => {
                identities.insert(LogicalImportIdentity::Perspective(
                    definition.perspective_id(),
                ));
            }
            Record::PerspectiveRetirement(_) => {}
            Record::LayerDefinition(definition) => {
                identities.insert(LogicalImportIdentity::Layer(definition.layer_id()));
            }
            Record::LayerSchemaSnapshot(snapshot) => {
                identities.insert(LogicalImportIdentity::Layer(snapshot.base_layer_id()));
                identities.extend(
                    snapshot
                        .definitions()
                        .iter()
                        .map(|definition| LogicalImportIdentity::Layer(definition.layer_id())),
                );
            }
            Record::EntityTypeDefinition(definition) => {
                identities.insert(LogicalImportIdentity::EntityType(
                    definition.entity_type_id(),
                ));
            }
            Record::PredicateDefinition(definition) => {
                identities.insert(LogicalImportIdentity::Predicate(definition.predicate_id()));
            }
            Record::EventKindDefinition(definition) => {
                identities.insert(LogicalImportIdentity::EventKind(definition.event_kind_id()));
                identities.extend(
                    definition
                        .roles()
                        .iter()
                        .map(|role| LogicalImportIdentity::EventRole(role.event_role_id())),
                );
                identities.extend(definition.attributes().iter().map(|attribute| {
                    LogicalImportIdentity::EventAttribute(attribute.event_attribute_id())
                }));
            }
            _ => {}
        }
        if let Some(reference) = record_ref(value) {
            if !identities.insert(LogicalImportIdentity::Record(reference)) {
                return Err(LogicalImportError::DuplicateImportedRecordIdentity(
                    reference,
                ));
            }
        }
    }
    Ok(identities)
}

/// Returns every durable identity defined by one record, including its typed record reference.
///
/// Purge planning uses this beside `record_references` to build the same dependency graph used
/// by import validation. Multiple schema revisions may define the same non-record identity.
pub(crate) fn record_defined_identities(record: &Record) -> Vec<LogicalImportIdentity> {
    let mut identities = BTreeSet::new();
    match record {
        Record::HistorySpaceDefinition(value) => {
            identities.insert(LogicalImportIdentity::HistorySpace(
                value.history_space_id(),
            ));
        }
        Record::Entity(value) => {
            identities.insert(LogicalImportIdentity::Entity(value.entity_id()));
        }
        Record::PerspectiveDefinitionRevision(value) => {
            identities.insert(LogicalImportIdentity::Perspective(value.perspective_id()));
        }
        Record::LayerDefinition(value) => {
            identities.insert(LogicalImportIdentity::Layer(value.layer_id()));
        }
        Record::LayerSchemaSnapshot(value) => {
            identities.insert(LogicalImportIdentity::Layer(value.base_layer_id()));
            identities.extend(
                value
                    .definitions()
                    .iter()
                    .map(|definition| LogicalImportIdentity::Layer(definition.layer_id())),
            );
        }
        Record::EntityTypeDefinition(value) => {
            identities.insert(LogicalImportIdentity::EntityType(value.entity_type_id()));
        }
        Record::PredicateDefinition(value) => {
            identities.insert(LogicalImportIdentity::Predicate(value.predicate_id()));
        }
        Record::EventKindDefinition(value) => {
            identities.insert(LogicalImportIdentity::EventKind(value.event_kind_id()));
            identities.extend(
                value
                    .roles()
                    .iter()
                    .map(|role| LogicalImportIdentity::EventRole(role.event_role_id())),
            );
            identities.extend(value.attributes().iter().map(|attribute| {
                LogicalImportIdentity::EventAttribute(attribute.event_attribute_id())
            }));
        }
        _ => {}
    }
    if let Some(reference) = record_ref(record) {
        identities.insert(LogicalImportIdentity::Record(reference));
    }
    identities.into_iter().collect()
}

fn validate_referenced_identities(
    source: &LogicalExport,
    imported: &BTreeSet<LogicalImportIdentity>,
    plan: &LogicalImportPlan,
    destination: &LogicalImportDestinationInventory,
) -> Result<(), LogicalImportError> {
    let mapped_imported = imported
        .iter()
        .map(|identity| map_identity(plan, *identity))
        .collect::<BTreeSet<_>>();
    for entry in source.records() {
        for reference in record_references(entry.record().record()) {
            let mapped = map_identity(plan, reference);
            if !mapped_imported.contains(&mapped) && !destination.identities.contains(&mapped) {
                return Err(LogicalImportError::MissingReference);
            }
        }
    }
    Ok(())
}

fn map_identity(
    plan: &LogicalImportPlan,
    identity: LogicalImportIdentity,
) -> LogicalImportIdentity {
    plan.mappings
        .binary_search_by_key(&identity, |mapping| mapping.source)
        .ok()
        .and_then(|index| plan.mappings.get(index))
        .map_or(identity, |mapping| mapping.target)
}

pub(crate) fn record_references(record: &Record) -> Vec<LogicalImportIdentity> {
    let mut references = Vec::new();
    match record {
        Record::HistorySpaceDefinition(value) => {
            if let Some(parent) = value.parent_history_space_id() {
                push(&mut references, LogicalImportIdentity::HistorySpace(parent));
            }
        }
        Record::Entity(value) => push(
            &mut references,
            LogicalImportIdentity::EntityType(value.entity_type_id()),
        ),
        Record::EntityRetirement(value) => push(
            &mut references,
            LogicalImportIdentity::Entity(value.entity_id()),
        ),
        Record::PerspectiveRetirement(value) => push(
            &mut references,
            LogicalImportIdentity::Perspective(value.perspective_id()),
        ),
        Record::LayerSchemaSnapshot(value) => {
            push(
                &mut references,
                LogicalImportIdentity::Layer(value.base_layer_id()),
            );
            for definition in value.definitions() {
                push(
                    &mut references,
                    LogicalImportIdentity::Layer(definition.layer_id()),
                );
            }
        }
        Record::PredicateDefinition(value) => {
            push_entity_constraint(&mut references, value.subject_constraint());
            if let Some(constraint) = value.object_constraint() {
                push_entity_constraint(&mut references, constraint);
            }
            push_time_constraints(&mut references, value.constraints());
        }
        Record::EventKindDefinition(value) => {
            for role in value.roles() {
                push_entity_constraint(&mut references, role.entity_constraint());
            }
            for attribute in value.attributes() {
                if let Some(constraint) = attribute.object_constraint() {
                    push_entity_constraint(&mut references, constraint);
                }
                push_time_constraints(&mut references, attribute.constraints());
            }
        }
        Record::Assertion(value) => {
            push_context(&mut references, value.context());
            push(
                &mut references,
                LogicalImportIdentity::Entity(value.subject().entity_id()),
            );
            push(
                &mut references,
                LogicalImportIdentity::Predicate(value.predicate_id()),
            );
            push_value(&mut references, value.value());
            push_timeline(&mut references, value.validity().interval().timeline().id());
        }
        Record::AssertionValidityClosure(value) => {
            push_record(&mut references, RecordRef::Assertion(value.assertion_id()));
            push_timeline(&mut references, value.close_at_world_time().timeline().id());
        }
        Record::AssertionRetraction(value) => {
            push_record(&mut references, RecordRef::Assertion(value.assertion_id()));
        }
        Record::Mask(value) => {
            push_context(&mut references, value.context());
            match value.selector() {
                MaskSelector::ExactAssertion(id) => {
                    push_record(&mut references, RecordRef::Assertion(*id));
                }
                MaskSelector::Proposition(key) => {
                    push(
                        &mut references,
                        LogicalImportIdentity::Entity(key.subject().entity_id()),
                    );
                    push(
                        &mut references,
                        LogicalImportIdentity::Predicate(key.predicate_id()),
                    );
                    push_value(&mut references, key.value());
                }
                MaskSelector::Slot(slot) => {
                    push(
                        &mut references,
                        LogicalImportIdentity::Entity(slot.subject().entity_id()),
                    );
                    push(
                        &mut references,
                        LogicalImportIdentity::Predicate(slot.predicate_id()),
                    );
                    push_perspective_scope(&mut references, slot.perspective_scope());
                }
            }
            if let Some(validity) = value.validity() {
                push_timeline(&mut references, validity.interval().timeline().id());
            }
        }
        Record::MaskValidityClosure(value) => {
            push_record(&mut references, RecordRef::Mask(value.mask_id()));
            push_timeline(&mut references, value.close_at_world_time().timeline().id());
        }
        Record::MaskRetraction(value) => {
            push_record(&mut references, RecordRef::Mask(value.mask_id()));
        }
        Record::ReplacementBoundary(value) => {
            push_context(&mut references, value.context());
            push(
                &mut references,
                LogicalImportIdentity::Entity(value.subject().entity_id()),
            );
            push(
                &mut references,
                LogicalImportIdentity::Predicate(value.predicate_id()),
            );
            if let Some(validity) = value.validity() {
                push_timeline(&mut references, validity.interval().timeline().id());
            }
        }
        Record::ReplacementBoundaryValidityClosure(value) => {
            push_record(
                &mut references,
                RecordRef::ReplacementBoundary(value.replacement_boundary_id()),
            );
            push_timeline(&mut references, value.close_at_world_time().timeline().id());
        }
        Record::ReplacementBoundaryRetraction(value) => {
            push_record(
                &mut references,
                RecordRef::ReplacementBoundary(value.replacement_boundary_id()),
            );
        }
        Record::ArchiveTransition(value) => {
            push_record(&mut references, archive_target_record_ref(value.target()));
        }
        Record::Event(value) => {
            push(
                &mut references,
                LogicalImportIdentity::HistorySpace(value.history_space_id()),
            );
            push(
                &mut references,
                LogicalImportIdentity::Layer(value.layer_id()),
            );
            push(
                &mut references,
                LogicalImportIdentity::EventKind(value.event_kind_id()),
            );
            for participant in value.participants().as_slice() {
                push(
                    &mut references,
                    LogicalImportIdentity::EventRole(participant.role_id()),
                );
                push(
                    &mut references,
                    LogicalImportIdentity::Entity(participant.entity_id()),
                );
            }
            for attribute in value.attributes().as_slice() {
                push(
                    &mut references,
                    LogicalImportIdentity::EventAttribute(attribute.attribute_id()),
                );
                push_value(&mut references, attribute.value());
            }
            match value.event_time() {
                worlddb_core::EventTime::Instant(time) => {
                    push_timeline(&mut references, time.timeline().id());
                }
                worlddb_core::EventTime::Span { start, end } => {
                    push_timeline(&mut references, start.timeline().id());
                    if let Some(end) = end {
                        push_timeline(&mut references, end.timeline().id());
                    }
                }
            }
        }
        Record::EventMask(value) => {
            push(
                &mut references,
                LogicalImportIdentity::HistorySpace(value.history_space_id()),
            );
            push(
                &mut references,
                LogicalImportIdentity::Layer(value.layer_id()),
            );
            push_record(&mut references, RecordRef::Event(value.target_event()));
        }
        Record::EventSpanClosure(value) => {
            push_record(&mut references, RecordRef::Event(value.event_id()));
            push_timeline(&mut references, value.close_at_event_time().timeline().id());
        }
        Record::EventRetraction(value) => {
            push_record(&mut references, RecordRef::Event(value.event_id()));
        }
        Record::EventMaskRetraction(value) => {
            push_record(&mut references, RecordRef::EventMask(value.event_mask_id()));
        }
        Record::EventRelation(value) => {
            push_record(&mut references, RecordRef::Event(value.from_event()));
            push_record(&mut references, RecordRef::Event(value.to_event()));
        }
        Record::EventRelationRetraction(value) => {
            push_record(
                &mut references,
                RecordRef::EventRelation(value.event_relation_id()),
            );
        }
        Record::Evidence(value) => {
            push_record(&mut references, RecordRef::Source(value.source_id()));
            push_record(&mut references, evidence_target_record_ref(value.target()));
        }
        Record::Provenance(value) => {
            push_record(
                &mut references,
                provenance_endpoint_record_ref(value.from()),
            );
            push_record(&mut references, provenance_endpoint_record_ref(value.to()));
        }
        Record::EvidenceRetraction(value) => {
            push_record(&mut references, RecordRef::Evidence(value.evidence_id()));
        }
        Record::ProvenanceRetraction(value) => {
            push_record(
                &mut references,
                RecordRef::Provenance(value.provenance_id()),
            );
        }
        Record::TransferLineage(value) => {
            push(
                &mut references,
                LogicalImportIdentity::HistorySpace(value.source_history_space_id()),
            );
            push(
                &mut references,
                LogicalImportIdentity::HistorySpace(value.target_history_space_id()),
            );
            push_record(&mut references, value.source().record_ref());
            push_record(&mut references, value.target().record_ref());
        }
        Record::PerspectiveDefinitionRevision(_)
        | Record::LayerDefinition(_)
        | Record::EntityTypeDefinition(_)
        | Record::Source(_)
        | Record::MigrationPlan(_)
        | Record::MigrationRun(_)
        | Record::MigrationStepCommitIdentity(_) => {}
    }
    references
}

fn push(references: &mut Vec<LogicalImportIdentity>, identity: LogicalImportIdentity) {
    references.push(identity);
}

fn push_record(references: &mut Vec<LogicalImportIdentity>, reference: RecordRef) {
    push(references, LogicalImportIdentity::Record(reference));
}

fn push_timeline(references: &mut Vec<LogicalImportIdentity>, id: TimelineId) {
    push(references, LogicalImportIdentity::Timeline(id));
}

fn push_perspective_scope(references: &mut Vec<LogicalImportIdentity>, scope: PerspectiveScope) {
    if let PerspectiveScope::Perspective(id) = scope {
        push(references, LogicalImportIdentity::Perspective(id));
    }
}

fn push_context(references: &mut Vec<LogicalImportIdentity>, context: worlddb_core::ContextKey) {
    push(
        references,
        LogicalImportIdentity::HistorySpace(context.history_space_id()),
    );
    push(references, LogicalImportIdentity::Layer(context.layer_id()));
    push_perspective_scope(references, context.perspective_scope());
}

fn push_value(references: &mut Vec<LogicalImportIdentity>, value: &Value) {
    match value {
        Value::Entity(id) => push(references, LogicalImportIdentity::Entity(*id)),
        Value::Time(time) => push_timeline(references, time.timeline_id()),
        _ => {}
    }
}

fn push_entity_constraint(
    references: &mut Vec<LogicalImportIdentity>,
    constraint: EntityTypeConstraint,
) {
    if let EntityTypeConstraint::Exact(id) = constraint {
        push(references, LogicalImportIdentity::EntityType(id));
    }
}

fn push_time_constraints(
    references: &mut Vec<LogicalImportIdentity>,
    constraints: &worlddb_core::ConstraintSet,
) {
    for constraint in constraints.rules() {
        if let worlddb_core::ValueConstraint::TimeRange(range) = constraint {
            if let Some(min) = range.min() {
                push_timeline(references, min.timeline_id());
            }
            if let Some(max) = range.max() {
                push_timeline(references, max.timeline_id());
            }
        }
    }
}

fn evidence_target_record_ref(target: EvidenceTargetRef) -> RecordRef {
    match target {
        EvidenceTargetRef::Assertion(id) => RecordRef::Assertion(id),
        EvidenceTargetRef::Mask(id) => RecordRef::Mask(id),
        EvidenceTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        EvidenceTargetRef::Event(id) => RecordRef::Event(id),
        EvidenceTargetRef::EventMask(id) => RecordRef::EventMask(id),
        EvidenceTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        EvidenceTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        EvidenceTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        EvidenceTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        EvidenceTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        EvidenceTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        EvidenceTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        EvidenceTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        EvidenceTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        EvidenceTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        EvidenceTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        EvidenceTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        EvidenceTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        EvidenceTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        EvidenceTargetRef::Provenance(id) => RecordRef::Provenance(id),
        EvidenceTargetRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn provenance_endpoint_record_ref(endpoint: ProvenanceEndpointRef) -> RecordRef {
    match endpoint {
        ProvenanceEndpointRef::Assertion(id) => RecordRef::Assertion(id),
        ProvenanceEndpointRef::Mask(id) => RecordRef::Mask(id),
        ProvenanceEndpointRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ProvenanceEndpointRef::Event(id) => RecordRef::Event(id),
        ProvenanceEndpointRef::EventMask(id) => RecordRef::EventMask(id),
        ProvenanceEndpointRef::Source(id) => RecordRef::Source(id),
        ProvenanceEndpointRef::Evidence(id) => RecordRef::Evidence(id),
        ProvenanceEndpointRef::Provenance(id) => RecordRef::Provenance(id),
        ProvenanceEndpointRef::AssertionValidityClosure(id) => {
            RecordRef::AssertionValidityClosure(id)
        }
        ProvenanceEndpointRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ProvenanceEndpointRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ProvenanceEndpointRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ProvenanceEndpointRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ProvenanceEndpointRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ProvenanceEndpointRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ProvenanceEndpointRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ProvenanceEndpointRef::EventRelationRetraction(id) => {
            RecordRef::EventRelationRetraction(id)
        }
        ProvenanceEndpointRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ProvenanceEndpointRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ProvenanceEndpointRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ProvenanceEndpointRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ProvenanceEndpointRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> RecordRef {
    match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        ArchiveTargetRef::EventMask(id) => RecordRef::EventMask(id),
        ArchiveTargetRef::EventRelation(id) => RecordRef::EventRelation(id),
        ArchiveTargetRef::Source(id) => RecordRef::Source(id),
        ArchiveTargetRef::Evidence(id) => RecordRef::Evidence(id),
        ArchiveTargetRef::Provenance(id) => RecordRef::Provenance(id),
        ArchiveTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        ArchiveTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ArchiveTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ArchiveTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ArchiveTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ArchiveTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ArchiveTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ArchiveTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ArchiveTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ArchiveTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        ArchiveTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ArchiveTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ArchiveTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ArchiveTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ArchiveTargetRef::TransferLineage(id) => RecordRef::TransferLineage(id),
    }
}

fn validate_plan_against_inventories(
    plan: &LogicalImportPlan,
    source_identities: &BTreeSet<LogicalImportIdentity>,
    destination: &LogicalImportDestinationInventory,
) -> Result<(), LogicalImportError> {
    for mapping in &plan.mappings {
        if !source_identities.contains(&mapping.source) {
            return Err(LogicalImportError::MappingSourceNotInExport(mapping.source));
        }
        if destination.identities.contains(&mapping.target) {
            return Err(LogicalImportError::RemapTargetOccupied(mapping.target));
        }
    }
    for identity in source_identities {
        if destination.identities.contains(identity)
            && plan
                .mappings
                .binary_search_by_key(identity, |mapping| mapping.source)
                .is_err()
        {
            return Err(LogicalImportError::IdentityCollision(*identity));
        }
    }
    let mut mapped_identities = BTreeSet::new();
    for identity in source_identities {
        let target = plan
            .mappings
            .binary_search_by_key(identity, |mapping| mapping.source)
            .ok()
            .and_then(|index| plan.mappings.get(index))
            .map_or(*identity, |mapping| mapping.target);
        if !mapped_identities.insert(target) {
            return Err(LogicalImportError::DuplicateMappedIdentity(target));
        }
        if destination.identities.contains(&target) {
            return Err(LogicalImportError::RemapTargetOccupied(target));
        }
    }
    Ok(())
}

fn validate_mapping_set(mappings: &[LogicalImportIdMapping]) -> Result<(), LogicalImportError> {
    if mappings.len() > LOGICAL_IMPORT_MAX_MAPPINGS {
        return Err(LogicalImportError::ResourceLimit);
    }
    let mut sources = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for mapping in mappings {
        if !mapping.source.same_family(mapping.target) {
            return Err(LogicalImportError::IdentityFamilyMismatch);
        }
        if mapping.source == mapping.target {
            return Err(LogicalImportError::IdentityMappingIsUnchanged);
        }
        if !sources.insert(mapping.source) {
            return Err(LogicalImportError::DuplicateMappingSource(mapping.source));
        }
        if !targets.insert(mapping.target) {
            return Err(LogicalImportError::DuplicateMappingTarget(mapping.target));
        }
    }
    Ok(())
}

fn artifact_digest(source_artifact: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.LogicalImport.SourceArtifact.v1\0");
    hasher.update(source_artifact);
    *hasher.finalize().as_bytes()
}

fn plan_digest(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(LOGICAL_IMPORT_PLAN_CONTEXT);
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LogicalImportError> {
    let value: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(LogicalImportError::InvalidPlanEncoding)?
        .try_into()
        .map_err(|_| LogicalImportError::InvalidPlanEncoding)?;
    Ok(u64::from_be_bytes(value))
}

/// Why the logical import could not be prepared or its plan could not be decoded.
#[derive(Debug)]
pub enum LogicalImportError {
    /// The source artifact was malformed or failed logical-export validation.
    Export(LogicalExportError),
    /// In-scope lifecycle, event-relation, archive, or transfer references are incomplete.
    MissingReference,
    /// A durable record identity occurs more than once in the import stream.
    DuplicateImportedRecordIdentity(RecordRef),
    /// A plan entry maps different identity families.
    IdentityFamilyMismatch,
    /// A plan entry redundantly maps an identity to itself.
    IdentityMappingIsUnchanged,
    /// A plan maps one source identity more than once.
    DuplicateMappingSource(LogicalImportIdentity),
    /// Multiple source identities map to one destination identity.
    DuplicateMappingTarget(LogicalImportIdentity),
    /// A destination inventory contains an identity more than once.
    DuplicateDestinationIdentity(LogicalImportIdentity),
    /// The remap names no identity in the source export.
    MappingSourceNotInExport(LogicalImportIdentity),
    /// The source identity already exists in the destination and lacks an explicit remap.
    IdentityCollision(LogicalImportIdentity),
    /// A mapped target identity is already occupied by the destination.
    RemapTargetOccupied(LogicalImportIdentity),
    /// Two imported source identities resolve to one destination identity.
    DuplicateMappedIdentity(LogicalImportIdentity),
    /// The encoded plan has invalid framing, fields, or identities.
    InvalidPlanEncoding,
    /// The plan digest does not match its bytes.
    PlanDigestMismatch,
    /// A valid plan uses a noncanonical mapping order or duplicate encoding.
    NonCanonicalPlan,
    /// The plan was made for different source bytes or a different source database.
    SourceArtifactMismatch,
    /// The destination inventory differs from the database bound into the plan.
    DestinationMismatch,
    /// The import exceeds an explicit byte or item bound.
    ResourceLimit,
    /// The host allocator could not reserve the bounded plan buffer.
    AllocationFailed,
    /// Record-ref serialization unexpectedly failed.
    Record(worlddb_core::RecordCodecError),
}

impl fmt::Display for LogicalImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Export(error) => write!(formatter, "logical export: {error}"),
            Self::MissingReference => formatter.write_str("logical import has a missing reference"),
            Self::DuplicateImportedRecordIdentity(reference) => {
                write!(
                    formatter,
                    "logical import repeats record identity {reference:?}"
                )
            }
            Self::IdentityFamilyMismatch => {
                formatter.write_str("ID remap crosses identity families")
            }
            Self::IdentityMappingIsUnchanged => {
                formatter.write_str("ID remap must assign a different identity")
            }
            Self::DuplicateMappingSource(identity) => {
                write!(formatter, "ID remap repeats source identity {identity:?}")
            }
            Self::DuplicateMappingTarget(identity) => {
                write!(formatter, "ID remap repeats target identity {identity:?}")
            }
            Self::DuplicateDestinationIdentity(identity) => {
                write!(
                    formatter,
                    "destination inventory repeats identity {identity:?}"
                )
            }
            Self::MappingSourceNotInExport(identity) => {
                write!(
                    formatter,
                    "ID remap source {identity:?} is absent from the export"
                )
            }
            Self::IdentityCollision(identity) => write!(
                formatter,
                "destination identity collision requires an explicit remap: {identity:?}"
            ),
            Self::RemapTargetOccupied(identity) => {
                write!(
                    formatter,
                    "ID remap target is already occupied: {identity:?}"
                )
            }
            Self::DuplicateMappedIdentity(identity) => {
                write!(
                    formatter,
                    "multiple imported identities resolve to {identity:?}"
                )
            }
            Self::InvalidPlanEncoding => {
                formatter.write_str("logical import plan encoding is invalid")
            }
            Self::PlanDigestMismatch => {
                formatter.write_str("logical import plan digest does not match")
            }
            Self::NonCanonicalPlan => formatter.write_str("logical import plan is not canonical"),
            Self::SourceArtifactMismatch => {
                formatter.write_str("logical import plan is bound to different source bytes")
            }
            Self::DestinationMismatch => {
                formatter.write_str("logical import plan is bound to a different destination")
            }
            Self::ResourceLimit => formatter.write_str("logical import exceeds a resource limit"),
            Self::AllocationFailed => formatter.write_str("logical import allocation failed"),
            Self::Record(error) => write!(formatter, "record reference: {error}"),
        }
    }
}

impl std::error::Error for LogicalImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Export(error) => Some(error),
            Self::Record(error) => Some(error),
            _ => None,
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], LogicalImportError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(LogicalImportError::InvalidPlanEncoding)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(LogicalImportError::InvalidPlanEncoding)?;
        self.offset = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, LogicalImportError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(LogicalImportError::InvalidPlanEncoding)
    }

    fn u32(&mut self) -> Result<u32, LogicalImportError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| LogicalImportError::InvalidPlanEncoding)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn array_16(&mut self) -> Result<[u8; 16], LogicalImportError> {
        self.take(16)?
            .try_into()
            .map_err(|_| LogicalImportError::InvalidPlanEncoding)
    }

    fn array_32(&mut self) -> Result<[u8; 32], LogicalImportError> {
        self.take(32)?
            .try_into()
            .map_err(|_| LogicalImportError::InvalidPlanEncoding)
    }

    fn count(&mut self, limit: usize) -> Result<usize, LogicalImportError> {
        let count = usize::try_from(self.u32()?).map_err(|_| LogicalImportError::ResourceLimit)?;
        if count > limit {
            return Err(LogicalImportError::ResourceLimit);
        }
        Ok(count)
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
