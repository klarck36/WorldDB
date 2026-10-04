//! Authenticated factual-record, Event graph, and resolution-preview commands.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    ArchiveAction, ArchiveHistoryReferenceModel, ArchiveState, ArchiveTargetRecord,
    ArchiveTargetRef, ArchiveTransitionId, AssertionDraft, AssertionId, AssertionRetractionId,
    AssertionValidity, Bytes, ContextKey, Decimal, DomainId, EntityId, EpistemicMode,
    EventAttributeId, EventAttributeValue, EventDraft, EventId, EventKindId, EventMaskId,
    EventMaskRetractionId, EventParticipant, EventRelationId, EventRelationInputKind,
    EventRelationKind, EventRelationRetractionId, EventRetractionId, EventRoleId,
    EventSpanClosureId, EventTime, EvidenceId, EvidenceRelation, EvidenceRetractionId,
    EvidenceTargetRef, HistorySpaceId, Int, LayerId, LayerSelection, MaskId, MaskRetractionId,
    MaskSelector, MaskSlotSelector, MultiValueConflict, MultiValueEntry, MultiValueOutcome,
    NonEmptySet, PerspectiveId, PerspectiveScope, Polarity, PredicateId, PropositionKey,
    ProvenanceEndpointRef, ProvenanceId, ProvenanceRelation, ProvenanceRetractionId,
    QueryEngineOutput, RecordRef, RecordedAsOf, ReplacementBoundaryId,
    ReplacementBoundaryRetractionId, ResolutionPreview, ResolvedOutcome, ResolvedView, Revision,
    SchemaDefinition, SchemaMode, SourceContentDigest, SourceId, SourceLocator, SourceMetadata,
    SourceMetadataEntry, Subject, Symbol, Time, TimeInterval, Timeline, TimelineId, UInt, Value,
    WorldTime, WorldTimeSelector,
};
use worlddb_storage_file::{
    AssertionCorrectionReceipt, EventCorrectionReceipt, FactResolutionPreviewRequest, FactSnapshot,
    FileFactManager, FileSchemaManager, ReplacementBoundaryDraft, SourceDraft,
    SourceSupersessionDraft,
};

use crate::{EngineError, EngineHost, EpistemicModeInput};

/// One closed factual-record or resolution-preview action from the renderer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum FactCommand {
    /// Lists visible factual record identities and lifecycle state.
    Snapshot,
    /// Creates a project-wide Source with typed fields and optional supersession lineage.
    CreateSource {
        expected_base_revision: u64,
        source_kind: String,
        locator: Option<String>,
        content_digest_hex: Option<String>,
        #[serde(default)]
        metadata: Vec<SourceMetadataInput>,
    },
    /// Replaces Source metadata with a new Source and an explicit DerivedFrom edge.
    SupersedeSource {
        expected_base_revision: u64,
        superseded_source_id: String,
        source_kind: String,
        locator: Option<String>,
        content_digest_hex: Option<String>,
        #[serde(default)]
        metadata: Vec<SourceMetadataInput>,
    },
    /// Adds Evidence from a Source to one authorized closed target.
    CreateEvidence {
        expected_base_revision: u64,
        source_id: String,
        target: EndpointInput,
        relation: EvidenceRelationInput,
    },
    /// Adds one typed Provenance edge between authorized closed endpoints.
    CreateProvenance {
        expected_base_revision: u64,
        from: EndpointInput,
        to: EndpointInput,
        relation: ProvenanceRelationInput,
    },
    /// Explicitly retracts one Evidence edge.
    RetractEvidence {
        expected_base_revision: u64,
        evidence_id: String,
        reason: String,
    },
    /// Explicitly retracts one Provenance edge.
    RetractProvenance {
        expected_base_revision: u64,
        provenance_id: String,
        reason: String,
    },
    /// Publishes one immutable Assertion.
    CreateAssertion {
        expected_base_revision: u64,
        context: FactContextInput,
        subject_id: String,
        predicate_id: String,
        value: FactValueInput,
        polarity: PolarityInput,
        validity: ValidityInput,
    },
    /// Publishes one Mask with exactly one closed selector form.
    CreateMask {
        expected_base_revision: u64,
        context: FactContextInput,
        selector: MaskSelectorInput,
        validity: Option<ValidityInput>,
    },
    /// Publishes one MultiValueReplace boundary.
    CreateReplacementBoundary {
        expected_base_revision: u64,
        context: FactContextInput,
        subject_id: String,
        predicate_id: String,
        validity: Option<ValidityInput>,
    },
    /// Publishes one schema-checked Event with explicit participants, attributes, and time.
    CreateEvent {
        expected_base_revision: u64,
        draft: EventDraftInput,
    },
    /// Publishes an EventMask after strict HistorySpace/Layer precedence validation.
    CreateEventMask {
        expected_base_revision: u64,
        history_space_id: String,
        layer_id: String,
        target_event_id: String,
    },
    /// Publishes a canonical relation after complete graph validation.
    CreateEventRelation {
        expected_base_revision: u64,
        from_event_id: String,
        to_event_id: String,
        relation_kind: EventRelationKindInput,
    },
    /// Closes an Event span created without an end coordinate.
    CloseEventSpan {
        expected_base_revision: u64,
        event_id: String,
        timeline_id: String,
        close_nanoseconds: String,
    },
    /// Replaces one assertion in place semantically using an explicit three-record effect.
    CorrectAssertion {
        operation_id: String,
        expected_base_revision: u64,
        target_assertion_id: String,
        expected_target_created_revision: u64,
        replacement_assertion_id: String,
        retraction_id: String,
        corrects_provenance_id: String,
        replacement: AssertionDraftInput,
        retraction_reason: String,
    },
    /// Adds a replacement Event and Corrects edge; it never creates EventRetraction.
    CorrectEvent {
        operation_id: String,
        expected_base_revision: u64,
        target_event_id: String,
        expected_target_created_revision: u64,
        replacement_event_id: String,
        corrects_provenance_id: String,
        replacement: EventDraftInput,
    },
    /// Returns the durable status for one logical write identity.
    CommitStatus { operation_id: String },
    /// Performs exactly one explicit retraction or archive-state transition.
    Lifecycle {
        expected_base_revision: u64,
        target: FactTargetInput,
        action: FactLifecycleActionInput,
    },
    /// Evaluates one slot through the productive resolution-preview path.
    Preview {
        context: FactContextInput,
        subject_id: String,
        predicate_id: String,
        world_time: WorldTimeSelectorInput,
    },
}

/// One named typed metadata value for a Source.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMetadataInput {
    pub key: String,
    pub value: FactValueInput,
}

/// Closed set of Source, Evidence, Provenance, and supported lifecycle endpoint families.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointFamilyInput {
    Assertion,
    Mask,
    ReplacementBoundary,
    Event,
    EventMask,
    Source,
    Evidence,
    Provenance,
    AssertionValidityClosure,
    AssertionRetraction,
    MaskValidityClosure,
    MaskRetraction,
    ReplacementBoundaryValidityClosure,
    ReplacementBoundaryRetraction,
    EventSpanClosure,
    EventRetraction,
    EventMaskRetraction,
    EventRelationRetraction,
    EvidenceRetraction,
    ProvenanceRetraction,
    EntityRetirement,
    PerspectiveRetirement,
    ArchiveTransition,
}

/// Selects one closed endpoint family and typed UUID text.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointInput {
    pub family: EndpointFamilyInput,
    pub record_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelationInput {
    Supports,
    Contradicts,
    Documents,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceRelationInput {
    Corrects,
    DerivedFrom,
    ResultedFrom,
}

/// Complete explicit replacement Assertion payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionDraftInput {
    pub context: FactContextInput,
    pub subject_id: String,
    pub predicate_id: String,
    pub value: FactValueInput,
    pub polarity: PolarityInput,
    pub validity: ValidityInput,
}

/// Complete explicit replacement Event payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventDraftInput {
    pub history_space_id: String,
    pub layer_id: String,
    pub event_kind_id: String,
    pub participants: Vec<EventParticipantInput>,
    pub attributes: Vec<EventAttributeInput>,
    pub event_time: EventTimeInput,
}

/// Accepted EventRelation spellings. `After` is stored as inverse `Before`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventRelationKindInput {
    Before,
    After,
    SameTime,
    Causes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventParticipantInput {
    pub role_id: String,
    pub entity_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventAttributeInput {
    pub attribute_id: String,
    pub value: FactValueInput,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventTimeInput {
    Instant {
        timeline_id: String,
        nanoseconds: String,
    },
    Span {
        timeline_id: String,
        start_nanoseconds: String,
        end_nanoseconds: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "family", rename_all = "snake_case", deny_unknown_fields)]
pub enum FactTargetInput {
    Assertion { assertion_id: String },
    Mask { mask_id: String },
    ReplacementBoundary { replacement_boundary_id: String },
    Event { event_id: String },
    EventMask { event_mask_id: String },
    EventRelation { event_relation_id: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum FactLifecycleActionInput {
    Retract { reason: String },
    Archive,
    Unarchive,
}

/// Explicit write/query context shared by factual-record forms.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FactContextInput {
    pub history_space_id: String,
    pub layer_id: String,
    pub perspective_id: Option<String>,
    pub epistemic_mode: EpistemicModeInput,
}

/// Closed assertion polarity accepted by the renderer.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolarityInput {
    Positive,
    Negative,
}

/// Closed scalar value grammar. Numeric values remain exact decimal strings.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FactValueInput {
    Bool(bool),
    Int(String),
    UInt(String),
    Decimal(String),
    String(String),
    Symbol(String),
    Entity(String),
    Time {
        timeline_id: String,
        ticks: String,
        unit_symbol: String,
    },
    Duration(String),
    BytesHex(String),
}

/// One closed Mask selector shape.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaskSelectorInput {
    ExactAssertion {
        assertion_id: String,
    },
    Proposition {
        subject_id: String,
        predicate_id: String,
        value: FactValueInput,
        polarity: PolarityInput,
    },
    Slot {
        subject_id: String,
        predicate_id: String,
    },
}

/// World-time selector for one resolution preview.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldTimeSelectorInput {
    AllTimes,
    At {
        timeline_id: String,
        nanoseconds: String,
    },
}

/// Optional half-open validity interval input. Missing endpoints are open.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidityInput {
    pub timeline_id: String,
    pub start_nanoseconds: Option<String>,
    pub end_nanoseconds: Option<String>,
}

/// Result of one authenticated factual-record action.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FactResponse {
    Catalog(FactCatalogView),
    Published(FactPublicationView),
    Preview(ResolutionPreviewView),
    AssertionCorrected(AssertionCorrectionView),
    EventCorrected(EventCorrectionView),
    EventGraphConflict(EventGraphConflictView),
    OperationStatus(FactOperationStatusView),
    LifecycleChanged(FactLifecycleView),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventGraphConflictView {
    pub explanation: String,
    pub relation_saved: bool,
    pub automatic_inference_applied: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactOperationStatusKind {
    NotCommitted,
    Committed,
    Indeterminate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FactOperationStatusView {
    pub operation_id: String,
    pub status: FactOperationStatusKind,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FactCatalogView {
    pub revision: u64,
    pub lifecycle_visible: bool,
    pub records: Vec<FactCatalogRecordView>,
    pub event_graph_guidance: Vec<String>,
    pub sources: Vec<SourceView>,
    pub evidence: Vec<EvidenceView>,
    pub provenance: Vec<ProvenanceView>,
    pub endpoint_options: Vec<EndpointOptionView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndpointOptionView {
    pub family: String,
    pub record_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceView {
    pub source_id: String,
    pub created_revision: u64,
    pub source_kind: String,
    pub locator: Option<String>,
    pub content_digest_hex: Option<String>,
    pub metadata: Vec<SourceMetadataView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceMetadataView {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvidenceView {
    pub evidence_id: String,
    pub source_id: String,
    pub target_family: String,
    pub target_record_id: String,
    pub relation: String,
    pub created_revision: u64,
    pub retracted: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProvenanceView {
    pub provenance_id: String,
    pub from_family: String,
    pub from_record_id: String,
    pub to_family: String,
    pub to_record_id: String,
    pub relation: String,
    pub created_revision: u64,
    pub retracted: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FactCatalogRecordView {
    pub family: String,
    pub record_id: String,
    pub created_revision: u64,
    pub retracted: Option<bool>,
    pub archived: Option<bool>,
    pub history_space_id: Option<String>,
    pub layer_id: Option<String>,
    pub perspective_id: Option<String>,
    pub epistemic_mode: Option<String>,
    pub subject_id: Option<String>,
    pub predicate_id: Option<String>,
    pub event_kind_id: Option<String>,
    pub target_event_id: Option<String>,
    pub from_event_id: Option<String>,
    pub to_event_id: Option<String>,
    pub relation_kind: Option<String>,
    pub timeline_id: Option<String>,
    pub time_start_nanoseconds: Option<String>,
    pub time_end_nanoseconds: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssertionCorrectionView {
    pub operation_id: String,
    pub revision: u64,
    pub target_assertion_id: String,
    pub replacement_assertion_id: String,
    pub retraction_id: String,
    pub corrects_provenance_id: String,
    pub atomic_effect: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventCorrectionView {
    pub operation_id: String,
    pub revision: u64,
    pub target_event_id: String,
    pub replacement_event_id: String,
    pub corrects_provenance_id: String,
    pub original_remains_active: bool,
    pub atomic_effect: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FactLifecycleView {
    pub operation_id: String,
    pub revision: u64,
    pub target_family: String,
    pub target_id: String,
    pub effect: String,
    pub record_id: String,
}

/// Safe receipt for one durable factual record.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FactPublicationView {
    pub operation_id: String,
    pub revision: u64,
    pub family: String,
    pub record_id: String,
}

/// Complete M8-14d result, kept distinct from the write receipt.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolutionPreviewView {
    pub revision: u64,
    pub result: ResolutionResultView,
}

/// Closed result forms for point, all-times, and complete-empty previews.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolutionResultView {
    Point {
        timeline_id: String,
        nanoseconds: String,
        outcome: ResolutionOutcomeView,
    },
    AllTimes {
        slices: Vec<ResolutionSliceView>,
    },
    CompleteEmpty,
}

/// One half-open temporal cell and its independent resolution outcome.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolutionSliceView {
    pub timeline_id: String,
    pub start_nanoseconds: Option<String>,
    pub end_nanoseconds: Option<String>,
    pub outcome: ResolutionOutcomeView,
}

/// Known, Unknown, or Conflict remains explicit at the renderer boundary.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolutionOutcomeView {
    Known {
        values: Vec<ResolutionValueView>,
        contributors: Vec<String>,
    },
    Unknown,
    Conflict {
        values: Vec<ResolutionValueView>,
        conflicts: Vec<ResolutionConflictView>,
        contributors: Vec<String>,
    },
}

/// One known value and its explicit assertion polarity and contributors.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolutionValueView {
    pub value: String,
    pub polarity: String,
    pub contributors: Vec<String>,
}

/// Positive and negative assertion identities for one contradictory value.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolutionConflictView {
    pub value: String,
    pub positive_contributors: Vec<String>,
    pub negative_contributors: Vec<String>,
}

impl EngineHost {
    /// Executes an authenticated factual-record write or resolution preview.
    pub fn facts(&self, command: FactCommand) -> Result<FactResponse, EngineError> {
        execute(self, command)
    }
}

fn execute(engine: &EngineHost, command: FactCommand) -> Result<FactResponse, EngineError> {
    let _guard = engine
        .schema_management
        .lock()
        .map_err(|_| EngineError::Fact("factual-record manager state is unavailable".to_owned()))?;
    let principal = engine.principal_id.ok_or_else(|| {
        EngineError::Fact("an authenticated project session is required".to_owned())
    })?;
    let mut manager =
        FileFactManager::open(engine._layout.clone(), &engine._writer_lock, principal)
            .map_err(fact_error)?;

    match command {
        FactCommand::Snapshot => {
            let revision = manager.revision();
            let snapshot = manager.snapshot_at(revision).map_err(fact_error)?;
            Ok(FactResponse::Catalog(fact_catalog_view(snapshot)?))
        }
        FactCommand::CreateSource {
            expected_base_revision,
            source_kind,
            locator,
            content_digest_hex,
            metadata,
        } => {
            let operation_id = operation_id()?;
            let source_id = identity::<SourceId>("Source")?;
            let draft = parse_source_fields(
                source_id,
                source_kind,
                locator,
                content_digest_hex,
                metadata,
            )?;
            let receipt = manager
                .create_source(revision(expected_base_revision)?, operation_id, draft)
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "source".to_owned(),
                record_id: source_id.to_string(),
            }))
        }
        FactCommand::SupersedeSource {
            expected_base_revision,
            superseded_source_id,
            source_kind,
            locator,
            content_digest_hex,
            metadata,
        } => {
            let operation_id = operation_id()?;
            let source_id = identity::<SourceId>("replacement Source")?;
            let provenance_id = identity::<ProvenanceId>("Source lineage")?;
            let replacement = parse_source_fields(
                source_id,
                source_kind,
                locator,
                content_digest_hex,
                metadata,
            )?;
            let receipt = manager
                .supersede_source(
                    revision(expected_base_revision)?,
                    operation_id,
                    SourceSupersessionDraft {
                        replacement,
                        provenance_id,
                        superseded_source_id: parse_id::<SourceId>(
                            &superseded_source_id,
                            "Source",
                        )?,
                    },
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "source_superseded".to_owned(),
                record_id: source_id.to_string(),
            }))
        }
        FactCommand::CreateEvidence {
            expected_base_revision,
            source_id,
            target,
            relation,
        } => {
            let operation_id = operation_id()?;
            let evidence_id = identity::<EvidenceId>("Evidence")?;
            let receipt = manager
                .create_evidence(
                    revision(expected_base_revision)?,
                    operation_id,
                    evidence_id,
                    parse_id::<SourceId>(&source_id, "Source")?,
                    parse_evidence_target(target)?,
                    relation.into(),
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "evidence".to_owned(),
                record_id: evidence_id.to_string(),
            }))
        }
        FactCommand::CreateProvenance {
            expected_base_revision,
            from,
            to,
            relation,
        } => {
            let operation_id = operation_id()?;
            let provenance_id = identity::<ProvenanceId>("Provenance")?;
            let receipt = manager
                .create_provenance(
                    revision(expected_base_revision)?,
                    operation_id,
                    provenance_id,
                    parse_provenance_endpoint(from)?,
                    parse_provenance_endpoint(to)?,
                    relation.into(),
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "provenance".to_owned(),
                record_id: provenance_id.to_string(),
            }))
        }
        FactCommand::RetractEvidence {
            expected_base_revision,
            evidence_id,
            reason,
        } => {
            let operation_id = operation_id()?;
            let retraction_id = identity::<EvidenceRetractionId>("EvidenceRetraction")?;
            let receipt = manager
                .retract_evidence(
                    revision(expected_base_revision)?,
                    operation_id,
                    retraction_id,
                    parse_id::<EvidenceId>(&evidence_id, "Evidence")?,
                    reason,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "evidence_retraction".to_owned(),
                record_id: retraction_id.to_string(),
            }))
        }
        FactCommand::RetractProvenance {
            expected_base_revision,
            provenance_id,
            reason,
        } => {
            let operation_id = operation_id()?;
            let retraction_id = identity::<ProvenanceRetractionId>("ProvenanceRetraction")?;
            let receipt = manager
                .retract_provenance(
                    revision(expected_base_revision)?,
                    operation_id,
                    retraction_id,
                    parse_id::<ProvenanceId>(&provenance_id, "Provenance")?,
                    reason,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "provenance_retraction".to_owned(),
                record_id: retraction_id.to_string(),
            }))
        }
        FactCommand::CreateAssertion {
            expected_base_revision,
            context,
            subject_id,
            predicate_id,
            value,
            polarity,
            validity,
        } => {
            let context = parse_context(context)?;
            let subject = Subject::new(parse_id::<EntityId>(&subject_id, "Entity")?);
            let predicate_id = parse_id::<PredicateId>(&predicate_id, "Predicate")?;
            let draft = AssertionDraft::new(
                context,
                subject,
                predicate_id,
                parse_value(value)?,
                polarity.into(),
                parse_validity(validity)?,
            );
            let operation_id = operation_id()?;
            let record_id = identity::<AssertionId>("Assertion")?;
            let receipt = manager
                .create_assertion(
                    revision(expected_base_revision)?,
                    operation_id,
                    record_id,
                    draft,
                    false,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "assertion".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CreateMask {
            expected_base_revision,
            context,
            selector,
            validity,
        } => {
            let context = parse_context(context)?;
            let selector = parse_mask_selector(selector, context)?;
            let validity = validity.map(parse_validity).transpose()?;
            let operation_id = operation_id()?;
            let record_id = identity::<MaskId>("Mask")?;
            let receipt = manager
                .create_mask(
                    revision(expected_base_revision)?,
                    operation_id,
                    record_id,
                    context,
                    selector,
                    validity,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "mask".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CreateReplacementBoundary {
            expected_base_revision,
            context,
            subject_id,
            predicate_id,
            validity,
        } => {
            let context = parse_context(context)?;
            let subject = Subject::new(parse_id::<EntityId>(&subject_id, "Entity")?);
            let predicate_id = parse_id::<PredicateId>(&predicate_id, "Predicate")?;
            let validity = validity.map(parse_validity).transpose()?;
            let operation_id = operation_id()?;
            let record_id = identity::<ReplacementBoundaryId>("ReplacementBoundary")?;
            let receipt = manager
                .create_replacement_boundary(
                    revision(expected_base_revision)?,
                    operation_id,
                    record_id,
                    ReplacementBoundaryDraft::new(context, subject, predicate_id, validity),
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "replacement_boundary".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CreateEvent {
            expected_base_revision,
            draft,
        } => {
            let base_revision = revision(expected_base_revision)?;
            let schema_manager =
                FileSchemaManager::open(engine._layout.clone(), &engine._writer_lock, principal)
                    .map_err(|error| EngineError::Fact(error.to_string()))?;
            let schema = schema_manager
                .schema_at(SchemaMode::Current, base_revision)
                .map_err(|error| EngineError::Fact(error.to_string()))?;
            let draft = parse_event_draft(draft, &schema)?;
            let operation_id = operation_id()?;
            let record_id = identity::<EventId>("Event")?;
            let receipt = manager
                .create_event(base_revision, operation_id, record_id, draft)
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "event".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CreateEventMask {
            expected_base_revision,
            history_space_id,
            layer_id,
            target_event_id,
        } => {
            let operation_id = operation_id()?;
            let record_id = identity::<EventMaskId>("EventMask")?;
            let receipt = manager
                .create_event_mask(
                    revision(expected_base_revision)?,
                    operation_id,
                    record_id,
                    parse_id::<HistorySpaceId>(&history_space_id, "HistorySpace")?,
                    parse_id::<LayerId>(&layer_id, "Layer")?,
                    parse_id::<EventId>(&target_event_id, "Event")?,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "event_mask".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CreateEventRelation {
            expected_base_revision,
            from_event_id,
            to_event_id,
            relation_kind,
        } => {
            let operation_id = operation_id()?;
            let record_id = identity::<EventRelationId>("EventRelation")?;
            let receipt = match manager.create_event_relation(
                revision(expected_base_revision)?,
                operation_id,
                record_id,
                parse_id::<EventId>(&from_event_id, "source Event")?,
                parse_id::<EventId>(&to_event_id, "destination Event")?,
                relation_kind.into(),
            ) {
                Ok(receipt) => receipt,
                Err(worlddb_storage_file::FactManagementError::EventGraphConflict(explanation)) => {
                    return Ok(FactResponse::EventGraphConflict(EventGraphConflictView {
                        explanation,
                        relation_saved: false,
                        automatic_inference_applied: false,
                    }));
                }
                Err(error) => return Err(fact_error(error)),
            };
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "event_relation".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CloseEventSpan {
            expected_base_revision,
            event_id,
            timeline_id,
            close_nanoseconds,
        } => {
            let base_revision = revision(expected_base_revision)?;
            let schema_manager =
                FileSchemaManager::open(engine._layout.clone(), &engine._writer_lock, principal)
                    .map_err(|error| EngineError::Fact(error.to_string()))?;
            let schema = schema_manager
                .schema_at(SchemaMode::Current, base_revision)
                .map_err(|error| EngineError::Fact(error.to_string()))?;
            let timeline_id = parse_id::<TimelineId>(&timeline_id, "Timeline")?;
            if !schema
                .timeline(timeline_id)
                .is_some_and(|timeline| timeline.lifecycle() == worlddb_core::Lifecycle::Active)
            {
                return Err(EngineError::Fact(
                    "Event span closure requires an Active registered Timeline".to_owned(),
                ));
            }
            let close_at = parse_world_time(Timeline::new(timeline_id), &close_nanoseconds)?;
            let operation_id = operation_id()?;
            let record_id = identity::<EventSpanClosureId>("EventSpanClosure")?;
            let event_id = parse_id::<EventId>(&event_id, "Event")?;
            let receipt = manager
                .close_event_span(base_revision, operation_id, record_id, event_id, close_at)
                .map_err(fact_error)?;
            Ok(FactResponse::Published(FactPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                family: "event_span_closure".to_owned(),
                record_id: record_id.to_string(),
            }))
        }
        FactCommand::CorrectAssertion {
            operation_id,
            expected_base_revision,
            target_assertion_id,
            expected_target_created_revision,
            replacement_assertion_id,
            retraction_id,
            corrects_provenance_id,
            replacement,
            retraction_reason,
        } => {
            let replacement = AssertionDraft::new(
                parse_context(replacement.context)?,
                Subject::new(parse_id::<EntityId>(&replacement.subject_id, "Entity")?),
                parse_id::<PredicateId>(&replacement.predicate_id, "Predicate")?,
                parse_value(replacement.value)?,
                replacement.polarity.into(),
                parse_validity(replacement.validity)?,
            );
            let operation_id = parse_id::<worlddb_core::OperationId>(&operation_id, "Operation")?;
            let receipt = manager
                .correct_assertion(
                    revision(expected_base_revision)?,
                    operation_id,
                    parse_id::<AssertionId>(&target_assertion_id, "Assertion")?,
                    revision(expected_target_created_revision)?,
                    parse_id::<AssertionId>(&replacement_assertion_id, "replacement Assertion")?,
                    parse_id::<AssertionRetractionId>(&retraction_id, "AssertionRetraction")?,
                    parse_id::<ProvenanceId>(&corrects_provenance_id, "Corrects provenance")?,
                    replacement,
                    retraction_reason,
                    false,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::AssertionCorrected(assertion_correction_view(
                receipt,
            )))
        }
        FactCommand::CorrectEvent {
            operation_id,
            expected_base_revision,
            target_event_id,
            expected_target_created_revision,
            replacement_event_id,
            corrects_provenance_id,
            replacement,
        } => {
            let base_revision = revision(expected_base_revision)?;
            let schema_manager =
                FileSchemaManager::open(engine._layout.clone(), &engine._writer_lock, principal)
                    .map_err(|error| EngineError::Fact(error.to_string()))?;
            let schema = schema_manager
                .schema_at(SchemaMode::Current, base_revision)
                .map_err(|error| EngineError::Fact(error.to_string()))?;
            let replacement = parse_event_draft(replacement, &schema)?;
            let receipt = manager
                .correct_event(
                    base_revision,
                    parse_id::<worlddb_core::OperationId>(&operation_id, "Operation")?,
                    parse_id::<EventId>(&target_event_id, "Event")?,
                    revision(expected_target_created_revision)?,
                    parse_id::<EventId>(&replacement_event_id, "replacement Event")?,
                    parse_id::<ProvenanceId>(&corrects_provenance_id, "Corrects provenance")?,
                    replacement,
                )
                .map_err(fact_error)?;
            Ok(FactResponse::EventCorrected(event_correction_view(receipt)))
        }
        FactCommand::CommitStatus { operation_id } => {
            let operation_id = parse_id::<worlddb_core::OperationId>(&operation_id, "Operation")?;
            let status = manager.operation_status(operation_id).map_err(fact_error)?;
            let (status, revision) = match status {
                worlddb_storage_file::FactOperationStatus::NotCommitted => {
                    (FactOperationStatusKind::NotCommitted, None)
                }
                worlddb_storage_file::FactOperationStatus::Committed(revision) => {
                    (FactOperationStatusKind::Committed, Some(revision.value()))
                }
                worlddb_storage_file::FactOperationStatus::Indeterminate => {
                    (FactOperationStatusKind::Indeterminate, None)
                }
            };
            Ok(FactResponse::OperationStatus(FactOperationStatusView {
                operation_id: operation_id.to_string(),
                status,
                revision,
            }))
        }
        FactCommand::Lifecycle {
            expected_base_revision,
            target,
            action,
        } => execute_lifecycle(&mut manager, expected_base_revision, target, action),
        FactCommand::Preview {
            context,
            subject_id,
            predicate_id,
            world_time,
        } => {
            let context = parse_context(context)?;
            let subject = Subject::new(parse_id::<EntityId>(&subject_id, "Entity")?);
            let predicate_id = parse_id::<PredicateId>(&predicate_id, "Predicate")?;
            let world_time = parse_world_time_selector(world_time)?;
            let revision = manager.revision().value();
            let layer_selection = LayerSelection::Explicit(
                NonEmptySet::new(vec![context.layer_id()])
                    .map_err(|error| EngineError::Fact(error.to_string()))?,
            );
            let output = manager
                .resolution_preview(FactResolutionPreviewRequest {
                    history_space_id: context.history_space_id(),
                    layer_selection,
                    subject,
                    predicate_id,
                    perspective_scope: context.perspective_scope(),
                    epistemic_mode: context.epistemic_mode(),
                    world_time,
                })
                .map_err(fact_error)?;
            Ok(FactResponse::Preview(preview_view(revision, output)))
        }
    }
}

fn assertion_correction_view(receipt: AssertionCorrectionReceipt) -> AssertionCorrectionView {
    AssertionCorrectionView {
        operation_id: receipt.operation_id().to_string(),
        revision: receipt.revision().value(),
        target_assertion_id: receipt.target().to_string(),
        replacement_assertion_id: receipt.replacement().to_string(),
        retraction_id: receipt.retraction().to_string(),
        corrects_provenance_id: receipt.corrects().to_string(),
        atomic_effect: "replacement_assertion+target_retraction+corrects_provenance".to_owned(),
    }
}

fn event_correction_view(receipt: EventCorrectionReceipt) -> EventCorrectionView {
    EventCorrectionView {
        operation_id: receipt.operation_id().to_string(),
        revision: receipt.revision().value(),
        target_event_id: receipt.target().to_string(),
        replacement_event_id: receipt.replacement().to_string(),
        corrects_provenance_id: receipt.corrects().to_string(),
        original_remains_active: true,
        atomic_effect: "replacement_event+corrects_provenance".to_owned(),
    }
}

fn fact_catalog_view(snapshot: FactSnapshot) -> Result<FactCatalogView, EngineError> {
    let revision = snapshot.revision();
    let mut targets = Vec::new();
    targets.extend(snapshot.assertions().iter().map(|value| {
        ArchiveTargetRecord::new(
            ArchiveTargetRef::Assertion(value.id()),
            value.created_revision(),
        )
    }));
    targets.extend(snapshot.masks().iter().map(|value| {
        ArchiveTargetRecord::new(ArchiveTargetRef::Mask(value.id()), value.created_revision())
    }));
    targets.extend(snapshot.replacement_boundaries().iter().map(|value| {
        ArchiveTargetRecord::new(
            ArchiveTargetRef::ReplacementBoundary(value.id()),
            value.created_revision(),
        )
    }));
    targets.extend(snapshot.events().iter().map(|value| {
        ArchiveTargetRecord::new(
            ArchiveTargetRef::Event(value.event_id()),
            value.created_revision(),
        )
    }));
    targets.extend(snapshot.event_masks().iter().map(|value| {
        ArchiveTargetRecord::new(
            ArchiveTargetRef::EventMask(value.id()),
            value.created_revision(),
        )
    }));
    let archive = if snapshot.lifecycle_visible() {
        Some(
            ArchiveHistoryReferenceModel::new(targets, snapshot.archive_transitions().to_vec())
                .map_err(|error| EngineError::Fact(error.to_string()))?,
        )
    } else {
        None
    };
    let archived = |target| -> Result<Option<bool>, EngineError> {
        archive
            .as_ref()
            .map(|history| {
                history
                    .state_at(target, RecordedAsOf::from_published_revision(revision))
                    .map(|state| state == ArchiveState::Archived)
                    .map_err(|error| EngineError::Fact(error.to_string()))
            })
            .transpose()
    };
    let lifecycle = snapshot.lifecycle_visible();
    let mut records = Vec::new();
    for value in snapshot.assertions() {
        let context = value.context();
        let perspective_id = match context.perspective_scope() {
            PerspectiveScope::World => None,
            PerspectiveScope::Perspective(id) => Some(id.to_string()),
        };
        let epistemic_mode = match context.epistemic_mode() {
            EpistemicMode::WorldState => "world_state",
            EpistemicMode::Knows => "knows",
            EpistemicMode::Believes => "believes",
            EpistemicMode::Claims => "claims",
        };
        records.push(FactCatalogRecordView {
            family: "assertion".to_owned(),
            record_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: lifecycle.then(|| {
                snapshot
                    .assertion_retractions()
                    .iter()
                    .any(|item| item.assertion_id() == value.id())
            }),
            archived: archived(ArchiveTargetRef::Assertion(value.id()))?,
            history_space_id: Some(context.history_space_id().to_string()),
            layer_id: Some(context.layer_id().to_string()),
            perspective_id,
            epistemic_mode: Some(epistemic_mode.to_owned()),
            subject_id: Some(value.subject().entity_id().to_string()),
            predicate_id: Some(value.predicate_id().to_string()),
            event_kind_id: None,
            target_event_id: None,
            from_event_id: None,
            to_event_id: None,
            relation_kind: None,
            timeline_id: None,
            time_start_nanoseconds: None,
            time_end_nanoseconds: None,
        });
    }
    for value in snapshot.masks() {
        records.push(FactCatalogRecordView {
            family: "mask".to_owned(),
            record_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: lifecycle.then(|| {
                snapshot
                    .mask_retractions()
                    .iter()
                    .any(|item| item.mask_id() == value.id())
            }),
            archived: archived(ArchiveTargetRef::Mask(value.id()))?,
            history_space_id: None,
            layer_id: None,
            perspective_id: None,
            epistemic_mode: None,
            subject_id: None,
            predicate_id: None,
            event_kind_id: None,
            target_event_id: None,
            from_event_id: None,
            to_event_id: None,
            relation_kind: None,
            timeline_id: None,
            time_start_nanoseconds: None,
            time_end_nanoseconds: None,
        });
    }
    for value in snapshot.replacement_boundaries() {
        records.push(FactCatalogRecordView {
            family: "replacement_boundary".to_owned(),
            record_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: lifecycle.then(|| {
                snapshot
                    .replacement_boundary_retractions()
                    .iter()
                    .any(|item| item.replacement_boundary_id() == value.id())
            }),
            archived: archived(ArchiveTargetRef::ReplacementBoundary(value.id()))?,
            history_space_id: None,
            layer_id: None,
            perspective_id: None,
            epistemic_mode: None,
            subject_id: None,
            predicate_id: None,
            event_kind_id: None,
            target_event_id: None,
            from_event_id: None,
            to_event_id: None,
            relation_kind: None,
            timeline_id: None,
            time_start_nanoseconds: None,
            time_end_nanoseconds: None,
        });
    }
    for value in snapshot.events() {
        records.push(FactCatalogRecordView {
            family: "event".to_owned(),
            record_id: value.event_id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: lifecycle.then(|| {
                snapshot
                    .event_retractions()
                    .iter()
                    .any(|item| item.event_id() == value.event_id())
            }),
            archived: archived(ArchiveTargetRef::Event(value.event_id()))?,
            history_space_id: Some(value.history_space_id().to_string()),
            layer_id: Some(value.layer_id().to_string()),
            perspective_id: None,
            epistemic_mode: None,
            subject_id: None,
            predicate_id: None,
            event_kind_id: value.event_kind_id().map(|id| id.to_string()),
            target_event_id: None,
            from_event_id: None,
            to_event_id: None,
            relation_kind: None,
            timeline_id: None,
            time_start_nanoseconds: None,
            time_end_nanoseconds: None,
        });
    }
    for value in snapshot.event_masks() {
        records.push(FactCatalogRecordView {
            family: "event_mask".to_owned(),
            record_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: lifecycle.then(|| {
                snapshot
                    .event_mask_retractions()
                    .iter()
                    .any(|item| item.event_mask_id() == value.id())
            }),
            archived: archived(ArchiveTargetRef::EventMask(value.id()))?,
            history_space_id: Some(value.history_space_id().to_string()),
            layer_id: Some(value.layer_id().to_string()),
            perspective_id: None,
            epistemic_mode: None,
            subject_id: None,
            predicate_id: None,
            event_kind_id: None,
            target_event_id: Some(value.target_event().to_string()),
            from_event_id: None,
            to_event_id: None,
            relation_kind: None,
            timeline_id: None,
            time_start_nanoseconds: None,
            time_end_nanoseconds: None,
        });
    }
    for value in snapshot.event_relations() {
        records.push(FactCatalogRecordView {
            family: "event_relation".to_owned(),
            record_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: lifecycle.then(|| {
                snapshot
                    .event_relation_retractions()
                    .iter()
                    .any(|item| item.event_relation_id() == value.id())
            }),
            archived: None,
            history_space_id: None,
            layer_id: None,
            perspective_id: None,
            epistemic_mode: None,
            subject_id: None,
            predicate_id: None,
            event_kind_id: None,
            target_event_id: None,
            from_event_id: Some(value.from_event().to_string()),
            to_event_id: Some(value.to_event().to_string()),
            relation_kind: Some(event_relation_kind_label(value.kind()).to_owned()),
            timeline_id: None,
            time_start_nanoseconds: None,
            time_end_nanoseconds: None,
        });
    }
    for value in snapshot.event_span_closures() {
        let close_at = value.close_at_event_time();
        records.push(FactCatalogRecordView {
            family: "event_span_closure".to_owned(),
            record_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            retracted: None,
            archived: None,
            history_space_id: None,
            layer_id: None,
            perspective_id: None,
            epistemic_mode: None,
            subject_id: None,
            predicate_id: None,
            event_kind_id: None,
            target_event_id: Some(value.event_id().to_string()),
            from_event_id: None,
            to_event_id: None,
            relation_kind: None,
            timeline_id: Some(close_at.timeline().id().to_string()),
            time_start_nanoseconds: None,
            time_end_nanoseconds: Some(close_at.nanoseconds().to_string()),
        });
    }
    let sources = snapshot
        .sources()
        .iter()
        .map(|value| SourceView {
            source_id: value.id().to_string(),
            created_revision: value.created_revision().value(),
            source_kind: value.source_kind().as_str().to_owned(),
            locator: value.locator().map(|locator| locator.as_str().to_owned()),
            content_digest_hex: value
                .content_digest()
                .map(|digest| bytes_hex(digest.as_bytes().as_slice())),
            metadata: value
                .metadata()
                .as_slice()
                .iter()
                .map(|entry| SourceMetadataView {
                    key: entry.key().as_str().to_owned(),
                    value: format!("{:?}", entry.value()),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let evidence = snapshot
        .evidence()
        .iter()
        .map(|value| {
            let (target_family, target_record_id) = evidence_target_view(value.target());
            EvidenceView {
                evidence_id: value.id().to_string(),
                source_id: value.source_id().to_string(),
                target_family: target_family.to_owned(),
                target_record_id,
                relation: evidence_relation_label(value.relation()).to_owned(),
                created_revision: value.created_revision().value(),
                retracted: lifecycle.then(|| {
                    snapshot
                        .evidence_retractions()
                        .iter()
                        .any(|item| item.evidence_id() == value.id())
                }),
            }
        })
        .collect::<Vec<_>>();
    let provenance = snapshot
        .provenance()
        .iter()
        .map(|value| {
            let (from_family, from_record_id) = provenance_endpoint_view(value.from());
            let (to_family, to_record_id) = provenance_endpoint_view(value.to());
            ProvenanceView {
                provenance_id: value.id().to_string(),
                from_family: from_family.to_owned(),
                from_record_id,
                to_family: to_family.to_owned(),
                to_record_id,
                relation: provenance_relation_label(value.relation()).to_owned(),
                created_revision: value.created_revision().value(),
                retracted: lifecycle.then(|| {
                    snapshot
                        .provenance_retractions()
                        .iter()
                        .any(|item| item.provenance_id() == value.id())
                }),
            }
        })
        .collect::<Vec<_>>();
    let endpoint_options = snapshot
        .provenance_endpoints()
        .iter()
        .copied()
        .map(|value| {
            let (family, record_id) = provenance_endpoint_view(value);
            EndpointOptionView {
                family: family.to_owned(),
                record_id,
            }
        })
        .collect::<Vec<_>>();
    records.extend(
        sources
            .iter()
            .map(|value| catalog_meta_record("source", &value.source_id, value.created_revision)),
    );
    records.extend(
        evidence.iter().map(|value| {
            catalog_meta_record("evidence", &value.evidence_id, value.created_revision)
        }),
    );
    records.extend(provenance.iter().map(|value| {
        catalog_meta_record("provenance", &value.provenance_id, value.created_revision)
    }));
    records.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.created_revision.cmp(&right.created_revision))
            .then(left.record_id.cmp(&right.record_id))
    });
    Ok(FactCatalogView {
        revision: revision.value(),
        lifecycle_visible: lifecycle,
        records,
        event_graph_guidance: vec![
            "Zeitnähe, Reihenfolge und Überlappung erzeugen niemals automatisch Before, SameTime oder Causes.".to_owned(),
            "After wird als umgekehrtes Before gespeichert; SameTime wird als ungeordnetes Paar gespeichert.".to_owned(),
            "Before muss auch nach Zusammenfassen von SameTime-Gruppen azyklisch bleiben; Causes hat einen separaten azyklischen Graphen.".to_owned(),
        ],
        sources,
        evidence,
        provenance,
        endpoint_options,
    })
}

fn catalog_meta_record(
    family: &str,
    record_id: &str,
    created_revision: u64,
) -> FactCatalogRecordView {
    FactCatalogRecordView {
        family: family.to_owned(),
        record_id: record_id.to_owned(),
        created_revision,
        retracted: None,
        archived: None,
        history_space_id: None,
        layer_id: None,
        perspective_id: None,
        epistemic_mode: None,
        subject_id: None,
        predicate_id: None,
        event_kind_id: None,
        target_event_id: None,
        from_event_id: None,
        to_event_id: None,
        relation_kind: None,
        timeline_id: None,
        time_start_nanoseconds: None,
        time_end_nanoseconds: None,
    }
}

fn evidence_relation_label(value: EvidenceRelation) -> &'static str {
    match value {
        EvidenceRelation::Supports => "supports",
        EvidenceRelation::Contradicts => "contradicts",
        EvidenceRelation::Documents => "documents",
    }
}

fn provenance_relation_label(value: ProvenanceRelation) -> &'static str {
    match value {
        ProvenanceRelation::Corrects => "corrects",
        ProvenanceRelation::DerivedFrom => "derived_from",
        ProvenanceRelation::ResultedFrom => "resulted_from",
    }
}

fn evidence_target_view(value: EvidenceTargetRef) -> (&'static str, String) {
    use EvidenceTargetRef as E;
    match value {
        E::Assertion(id) => ("assertion", id.to_string()),
        E::Mask(id) => ("mask", id.to_string()),
        E::ReplacementBoundary(id) => ("replacement_boundary", id.to_string()),
        E::Event(id) => ("event", id.to_string()),
        E::EventMask(id) => ("event_mask", id.to_string()),
        E::AssertionValidityClosure(id) => ("assertion_validity_closure", id.to_string()),
        E::AssertionRetraction(id) => ("assertion_retraction", id.to_string()),
        E::MaskValidityClosure(id) => ("mask_validity_closure", id.to_string()),
        E::MaskRetraction(id) => ("mask_retraction", id.to_string()),
        E::ReplacementBoundaryValidityClosure(id) => {
            ("replacement_boundary_validity_closure", id.to_string())
        }
        E::ReplacementBoundaryRetraction(id) => ("replacement_boundary_retraction", id.to_string()),
        E::EventSpanClosure(id) => ("event_span_closure", id.to_string()),
        E::EventRetraction(id) => ("event_retraction", id.to_string()),
        E::EventMaskRetraction(id) => ("event_mask_retraction", id.to_string()),
        E::EventRelationRetraction(id) => ("event_relation_retraction", id.to_string()),
        E::EvidenceRetraction(id) => ("evidence_retraction", id.to_string()),
        E::ProvenanceRetraction(id) => ("provenance_retraction", id.to_string()),
        E::EntityRetirement(id) => ("entity_retirement", id.to_string()),
        E::PerspectiveRetirement(id) => ("perspective_retirement", id.to_string()),
        E::Provenance(id) => ("provenance", id.to_string()),
        E::ArchiveTransition(id) => ("archive_transition", id.to_string()),
    }
}

fn provenance_endpoint_view(value: ProvenanceEndpointRef) -> (&'static str, String) {
    use ProvenanceEndpointRef as P;
    match value {
        P::Assertion(id) => ("assertion", id.to_string()),
        P::Mask(id) => ("mask", id.to_string()),
        P::ReplacementBoundary(id) => ("replacement_boundary", id.to_string()),
        P::Event(id) => ("event", id.to_string()),
        P::EventMask(id) => ("event_mask", id.to_string()),
        P::Source(id) => ("source", id.to_string()),
        P::Evidence(id) => ("evidence", id.to_string()),
        P::Provenance(id) => ("provenance", id.to_string()),
        P::AssertionValidityClosure(id) => ("assertion_validity_closure", id.to_string()),
        P::AssertionRetraction(id) => ("assertion_retraction", id.to_string()),
        P::MaskValidityClosure(id) => ("mask_validity_closure", id.to_string()),
        P::MaskRetraction(id) => ("mask_retraction", id.to_string()),
        P::ReplacementBoundaryValidityClosure(id) => {
            ("replacement_boundary_validity_closure", id.to_string())
        }
        P::ReplacementBoundaryRetraction(id) => ("replacement_boundary_retraction", id.to_string()),
        P::EventSpanClosure(id) => ("event_span_closure", id.to_string()),
        P::EventRetraction(id) => ("event_retraction", id.to_string()),
        P::EventMaskRetraction(id) => ("event_mask_retraction", id.to_string()),
        P::EventRelationRetraction(id) => ("event_relation_retraction", id.to_string()),
        P::EvidenceRetraction(id) => ("evidence_retraction", id.to_string()),
        P::ProvenanceRetraction(id) => ("provenance_retraction", id.to_string()),
        P::EntityRetirement(id) => ("entity_retirement", id.to_string()),
        P::PerspectiveRetirement(id) => ("perspective_retirement", id.to_string()),
        P::ArchiveTransition(id) => ("archive_transition", id.to_string()),
    }
}

fn event_relation_kind_label(kind: EventRelationKind) -> &'static str {
    match kind {
        EventRelationKind::Before => "before",
        EventRelationKind::SameTime => "same_time",
        EventRelationKind::Causes => "causes",
    }
}

fn execute_lifecycle(
    manager: &mut FileFactManager<'_>,
    expected_base_revision: u64,
    target: FactTargetInput,
    action: FactLifecycleActionInput,
) -> Result<FactResponse, EngineError> {
    let base = revision(expected_base_revision)?;
    let operation_id = operation_id()?;
    let (target_family, target_id, effect, receipt) = match action {
        FactLifecycleActionInput::Retract { reason } => match target {
            FactTargetInput::Assertion { assertion_id } => {
                let id = parse_id::<AssertionId>(&assertion_id, "Assertion")?;
                let record_id = identity::<AssertionRetractionId>("AssertionRetraction")?;
                let receipt = manager
                    .retract_assertion(base, operation_id, record_id, id, reason)
                    .map_err(fact_error)?;
                ("assertion", id.to_string(), "retracted", receipt)
            }
            FactTargetInput::Mask { mask_id } => {
                let id = parse_id::<MaskId>(&mask_id, "Mask")?;
                let record_id = identity::<MaskRetractionId>("MaskRetraction")?;
                let receipt = manager
                    .retract_mask(base, operation_id, record_id, id, reason)
                    .map_err(fact_error)?;
                ("mask", id.to_string(), "retracted", receipt)
            }
            FactTargetInput::ReplacementBoundary {
                replacement_boundary_id,
            } => {
                let id = parse_id::<ReplacementBoundaryId>(
                    &replacement_boundary_id,
                    "ReplacementBoundary",
                )?;
                let record_id =
                    identity::<ReplacementBoundaryRetractionId>("ReplacementBoundaryRetraction")?;
                let receipt = manager
                    .retract_replacement_boundary(base, operation_id, record_id, id, reason)
                    .map_err(fact_error)?;
                ("replacement_boundary", id.to_string(), "retracted", receipt)
            }
            FactTargetInput::Event { event_id } => {
                let id = parse_id::<EventId>(&event_id, "Event")?;
                let record_id = identity::<EventRetractionId>("EventRetraction")?;
                let receipt = manager
                    .retract_event(base, operation_id, record_id, id, reason)
                    .map_err(fact_error)?;
                ("event", id.to_string(), "retracted", receipt)
            }
            FactTargetInput::EventMask { event_mask_id } => {
                let id = parse_id::<EventMaskId>(&event_mask_id, "EventMask")?;
                let record_id = identity::<EventMaskRetractionId>("EventMaskRetraction")?;
                let receipt = manager
                    .retract_event_mask(base, operation_id, record_id, id, reason)
                    .map_err(fact_error)?;
                ("event_mask", id.to_string(), "retracted", receipt)
            }
            FactTargetInput::EventRelation { event_relation_id } => {
                let id = parse_id::<EventRelationId>(&event_relation_id, "EventRelation")?;
                let record_id = identity::<EventRelationRetractionId>("EventRelationRetraction")?;
                let receipt =
                    match manager.retract_event_relation(base, operation_id, record_id, id, reason)
                    {
                        Ok(receipt) => receipt,
                        Err(worlddb_storage_file::FactManagementError::EventGraphConflict(
                            explanation,
                        )) => {
                            return Ok(FactResponse::EventGraphConflict(EventGraphConflictView {
                                explanation,
                                relation_saved: false,
                                automatic_inference_applied: false,
                            }));
                        }
                        Err(error) => return Err(fact_error(error)),
                    };
                ("event_relation", id.to_string(), "retracted", receipt)
            }
        },
        archive_action => {
            let action = match archive_action {
                FactLifecycleActionInput::Archive => ArchiveAction::Archive,
                FactLifecycleActionInput::Unarchive => ArchiveAction::Unarchive,
                FactLifecycleActionInput::Retract { .. } => unreachable!(),
            };
            let (target, family, id) = parse_archive_target(target)?;
            let receipt = manager
                .transition_archive(
                    base,
                    operation_id,
                    identity::<ArchiveTransitionId>("ArchiveTransition")?,
                    target,
                    action,
                )
                .map_err(fact_error)?;
            let effect = match action {
                ArchiveAction::Archive => "archived",
                ArchiveAction::Unarchive => "unarchived",
            };
            (family, id, effect, receipt)
        }
    };
    Ok(FactResponse::LifecycleChanged(FactLifecycleView {
        operation_id: receipt.operation_id().to_string(),
        revision: receipt.revision().value(),
        target_family: target_family.to_owned(),
        target_id,
        effect: effect.to_owned(),
        record_id: lifecycle_record_id(receipt.record_ref()),
    }))
}

fn lifecycle_record_id(reference: RecordRef) -> String {
    match reference {
        RecordRef::AssertionRetraction(id) => id.to_string(),
        RecordRef::MaskRetraction(id) => id.to_string(),
        RecordRef::ReplacementBoundaryRetraction(id) => id.to_string(),
        RecordRef::EventRetraction(id) => id.to_string(),
        RecordRef::EventMaskRetraction(id) => id.to_string(),
        RecordRef::EventRelationRetraction(id) => id.to_string(),
        RecordRef::EventSpanClosure(id) => id.to_string(),
        RecordRef::ArchiveTransition(id) => id.to_string(),
        _ => String::new(),
    }
}

fn parse_archive_target(
    input: FactTargetInput,
) -> Result<(ArchiveTargetRef, &'static str, String), EngineError> {
    match input {
        FactTargetInput::Assertion { assertion_id } => {
            let id = parse_id::<AssertionId>(&assertion_id, "Assertion")?;
            Ok((ArchiveTargetRef::Assertion(id), "assertion", id.to_string()))
        }
        FactTargetInput::Mask { mask_id } => {
            let id = parse_id::<MaskId>(&mask_id, "Mask")?;
            Ok((ArchiveTargetRef::Mask(id), "mask", id.to_string()))
        }
        FactTargetInput::ReplacementBoundary {
            replacement_boundary_id,
        } => {
            let id =
                parse_id::<ReplacementBoundaryId>(&replacement_boundary_id, "ReplacementBoundary")?;
            Ok((
                ArchiveTargetRef::ReplacementBoundary(id),
                "replacement_boundary",
                id.to_string(),
            ))
        }
        FactTargetInput::Event { event_id } => {
            let id = parse_id::<EventId>(&event_id, "Event")?;
            Ok((ArchiveTargetRef::Event(id), "event", id.to_string()))
        }
        FactTargetInput::EventMask { event_mask_id } => {
            let id = parse_id::<EventMaskId>(&event_mask_id, "EventMask")?;
            Ok((
                ArchiveTargetRef::EventMask(id),
                "event_mask",
                id.to_string(),
            ))
        }
        FactTargetInput::EventRelation { .. } => Err(EngineError::Fact(
            "EventRelations use explicit retraction and cannot be archived".to_owned(),
        )),
    }
}

fn parse_source_fields(
    source_id: SourceId,
    source_kind: String,
    locator: Option<String>,
    content_digest_hex: Option<String>,
    metadata: Vec<SourceMetadataInput>,
) -> Result<SourceDraft, EngineError> {
    let source_kind = Symbol::new(source_kind)
        .map_err(|error| EngineError::Fact(format!("invalid Source kind: {error}")))?;
    let locator = locator
        .map(SourceLocator::new)
        .transpose()
        .map_err(|error| EngineError::Fact(error.to_string()))?;
    let content_digest = content_digest_hex
        .map(|hex| {
            SourceContentDigest::new(Bytes::new(parse_hex(&hex)?))
                .map_err(|error| EngineError::Fact(error.to_string()))
        })
        .transpose()?;
    let metadata = metadata
        .into_iter()
        .map(|entry| {
            let key = Symbol::new(entry.key).map_err(|error| {
                EngineError::Fact(format!("invalid Source metadata key: {error}"))
            })?;
            Ok(SourceMetadataEntry::new(key, parse_value(entry.value)?))
        })
        .collect::<Result<Vec<_>, EngineError>>()?;
    let metadata =
        SourceMetadata::new(metadata).map_err(|error| EngineError::Fact(error.to_string()))?;
    Ok(SourceDraft {
        source_id,
        source_kind,
        locator,
        content_digest,
        metadata,
    })
}

fn parse_evidence_target(input: EndpointInput) -> Result<EvidenceTargetRef, EngineError> {
    EvidenceTargetRef::try_from(parse_endpoint_record_ref(input)?).map_err(|_| {
        EngineError::Fact("the selected family cannot be an Evidence target".to_owned())
    })
}

fn parse_provenance_endpoint(input: EndpointInput) -> Result<ProvenanceEndpointRef, EngineError> {
    ProvenanceEndpointRef::try_from(parse_endpoint_record_ref(input)?).map_err(|_| {
        EngineError::Fact("the selected family cannot be a Provenance endpoint".to_owned())
    })
}

fn parse_endpoint_record_ref(input: EndpointInput) -> Result<RecordRef, EngineError> {
    let id = input.record_id;
    Ok(match input.family {
        EndpointFamilyInput::Assertion => {
            RecordRef::Assertion(parse_id::<AssertionId>(&id, "Assertion")?)
        }
        EndpointFamilyInput::Mask => RecordRef::Mask(parse_id::<MaskId>(&id, "Mask")?),
        EndpointFamilyInput::ReplacementBoundary => {
            RecordRef::ReplacementBoundary(parse_id::<ReplacementBoundaryId>(
                &id,
                "ReplacementBoundary",
            )?)
        }
        EndpointFamilyInput::Event => RecordRef::Event(parse_id::<EventId>(&id, "Event")?),
        EndpointFamilyInput::EventMask => {
            RecordRef::EventMask(parse_id::<EventMaskId>(&id, "EventMask")?)
        }
        EndpointFamilyInput::Source => RecordRef::Source(parse_id::<SourceId>(&id, "Source")?),
        EndpointFamilyInput::Evidence => {
            RecordRef::Evidence(parse_id::<EvidenceId>(&id, "Evidence")?)
        }
        EndpointFamilyInput::Provenance => {
            RecordRef::Provenance(parse_id::<ProvenanceId>(&id, "Provenance")?)
        }
        EndpointFamilyInput::AssertionValidityClosure => RecordRef::AssertionValidityClosure(
            parse_id::<worlddb_core::AssertionValidityClosureId>(&id, "AssertionValidityClosure")?,
        ),
        EndpointFamilyInput::AssertionRetraction => {
            RecordRef::AssertionRetraction(parse_id::<AssertionRetractionId>(
                &id,
                "AssertionRetraction",
            )?)
        }
        EndpointFamilyInput::MaskValidityClosure => {
            RecordRef::MaskValidityClosure(parse_id::<worlddb_core::MaskValidityClosureId>(
                &id,
                "MaskValidityClosure",
            )?)
        }
        EndpointFamilyInput::MaskRetraction => {
            RecordRef::MaskRetraction(parse_id::<MaskRetractionId>(&id, "MaskRetraction")?)
        }
        EndpointFamilyInput::ReplacementBoundaryValidityClosure => {
            RecordRef::ReplacementBoundaryValidityClosure(parse_id::<
                worlddb_core::ReplacementBoundaryValidityClosureId,
            >(
                &id,
                "ReplacementBoundaryValidityClosure",
            )?)
        }
        EndpointFamilyInput::ReplacementBoundaryRetraction => {
            RecordRef::ReplacementBoundaryRetraction(parse_id::<ReplacementBoundaryRetractionId>(
                &id,
                "ReplacementBoundaryRetraction",
            )?)
        }
        EndpointFamilyInput::EventSpanClosure => {
            RecordRef::EventSpanClosure(parse_id::<EventSpanClosureId>(&id, "EventSpanClosure")?)
        }
        EndpointFamilyInput::EventRetraction => {
            RecordRef::EventRetraction(parse_id::<EventRetractionId>(&id, "EventRetraction")?)
        }
        EndpointFamilyInput::EventMaskRetraction => {
            RecordRef::EventMaskRetraction(parse_id::<EventMaskRetractionId>(
                &id,
                "EventMaskRetraction",
            )?)
        }
        EndpointFamilyInput::EventRelationRetraction => RecordRef::EventRelationRetraction(
            parse_id::<worlddb_core::EventRelationRetractionId>(&id, "EventRelationRetraction")?,
        ),
        EndpointFamilyInput::EvidenceRetraction => {
            RecordRef::EvidenceRetraction(parse_id::<EvidenceRetractionId>(
                &id,
                "EvidenceRetraction",
            )?)
        }
        EndpointFamilyInput::ProvenanceRetraction => {
            RecordRef::ProvenanceRetraction(parse_id::<ProvenanceRetractionId>(
                &id,
                "ProvenanceRetraction",
            )?)
        }
        EndpointFamilyInput::EntityRetirement => {
            RecordRef::EntityRetirement(parse_id::<worlddb_core::EntityRetirementId>(
                &id,
                "EntityRetirement",
            )?)
        }
        EndpointFamilyInput::PerspectiveRetirement => {
            RecordRef::PerspectiveRetirement(parse_id::<worlddb_core::PerspectiveRetirementId>(
                &id,
                "PerspectiveRetirement",
            )?)
        }
        EndpointFamilyInput::ArchiveTransition => {
            RecordRef::ArchiveTransition(parse_id::<worlddb_core::ArchiveTransitionId>(
                &id,
                "ArchiveTransition",
            )?)
        }
    })
}

impl From<EvidenceRelationInput> for EvidenceRelation {
    fn from(value: EvidenceRelationInput) -> Self {
        match value {
            EvidenceRelationInput::Supports => Self::Supports,
            EvidenceRelationInput::Contradicts => Self::Contradicts,
            EvidenceRelationInput::Documents => Self::Documents,
        }
    }
}

impl From<ProvenanceRelationInput> for ProvenanceRelation {
    fn from(value: ProvenanceRelationInput) -> Self {
        match value {
            ProvenanceRelationInput::Corrects => Self::Corrects,
            ProvenanceRelationInput::DerivedFrom => Self::DerivedFrom,
            ProvenanceRelationInput::ResultedFrom => Self::ResultedFrom,
        }
    }
}

fn parse_event_draft(
    input: EventDraftInput,
    schema: &worlddb_core::SchemaSnapshot,
) -> Result<EventDraft, EngineError> {
    let history_space = parse_id::<HistorySpaceId>(&input.history_space_id, "HistorySpace")?;
    let layer = parse_id::<LayerId>(&input.layer_id, "Layer")?;
    let event_kind_id = parse_id::<EventKindId>(&input.event_kind_id, "EventKind")?;
    let event_kind = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::EventKind(value) if value.event_kind_id() == event_kind_id => {
                Some(value)
            }
            _ => None,
        })
        .ok_or_else(|| EngineError::Fact("selected EventKind is unavailable".to_owned()))?;
    let participants = input
        .participants
        .into_iter()
        .map(|value| {
            Ok(EventParticipant::new(
                parse_id::<EventRoleId>(&value.role_id, "EventRole")?,
                parse_id::<EntityId>(&value.entity_id, "Entity")?,
            ))
        })
        .collect::<Result<Vec<_>, EngineError>>()?;
    let attributes = input
        .attributes
        .into_iter()
        .map(|value| {
            Ok(EventAttributeValue::new(
                parse_id::<EventAttributeId>(&value.attribute_id, "EventAttribute")?,
                parse_value(value.value)?,
            ))
        })
        .collect::<Result<Vec<_>, EngineError>>()?;
    EventDraft::new(
        history_space,
        layer,
        event_kind,
        participants,
        attributes,
        parse_event_time(input.event_time)?,
    )
    .map_err(|error| EngineError::Fact(error.to_string()))
}

fn parse_event_time(input: EventTimeInput) -> Result<EventTime, EngineError> {
    match input {
        EventTimeInput::Instant {
            timeline_id,
            nanoseconds,
        } => {
            let timeline = Timeline::new(parse_id::<TimelineId>(&timeline_id, "Timeline")?);
            Ok(EventTime::Instant(parse_world_time(
                timeline,
                &nanoseconds,
            )?))
        }
        EventTimeInput::Span {
            timeline_id,
            start_nanoseconds,
            end_nanoseconds,
        } => {
            let timeline = Timeline::new(parse_id::<TimelineId>(&timeline_id, "Timeline")?);
            let start = parse_world_time(timeline, &start_nanoseconds)?;
            let end = end_nanoseconds
                .as_deref()
                .map(|value| parse_world_time(timeline, value))
                .transpose()?;
            EventTime::span(start, end).map_err(|error| EngineError::Fact(error.to_string()))
        }
    }
}

impl From<PolarityInput> for Polarity {
    fn from(value: PolarityInput) -> Self {
        match value {
            PolarityInput::Positive => Self::Positive,
            PolarityInput::Negative => Self::Negative,
        }
    }
}

impl From<EventRelationKindInput> for EventRelationInputKind {
    fn from(value: EventRelationKindInput) -> Self {
        match value {
            EventRelationKindInput::Before => Self::Before,
            EventRelationKindInput::After => Self::After,
            EventRelationKindInput::SameTime => Self::SameTime,
            EventRelationKindInput::Causes => Self::Causes,
        }
    }
}

fn parse_context(input: FactContextInput) -> Result<ContextKey, EngineError> {
    let history_space = parse_id::<HistorySpaceId>(&input.history_space_id, "HistorySpace")?;
    let layer = parse_id::<LayerId>(&input.layer_id, "Layer")?;
    let scope = input
        .perspective_id
        .map(|value| parse_id::<PerspectiveId>(&value, "Perspective"))
        .transpose()?
        .map_or(PerspectiveScope::World, PerspectiveScope::Perspective);
    let mode = match input.epistemic_mode {
        EpistemicModeInput::WorldState => EpistemicMode::WorldState,
        EpistemicModeInput::Knows => EpistemicMode::Knows,
        EpistemicModeInput::Believes => EpistemicMode::Believes,
        EpistemicModeInput::Claims => EpistemicMode::Claims,
    };
    ContextKey::new(history_space, layer, scope, mode)
        .map_err(|error| EngineError::Fact(error.to_string()))
}

fn parse_mask_selector(
    input: MaskSelectorInput,
    context: ContextKey,
) -> Result<MaskSelector, EngineError> {
    match input {
        MaskSelectorInput::ExactAssertion { assertion_id } => Ok(MaskSelector::ExactAssertion(
            parse_id::<AssertionId>(&assertion_id, "Assertion")?,
        )),
        MaskSelectorInput::Proposition {
            subject_id,
            predicate_id,
            value,
            polarity,
        } => Ok(MaskSelector::Proposition(PropositionKey::new(
            Subject::new(parse_id::<EntityId>(&subject_id, "Entity")?),
            parse_id::<PredicateId>(&predicate_id, "Predicate")?,
            parse_value(value)?,
            polarity.into(),
        ))),
        MaskSelectorInput::Slot {
            subject_id,
            predicate_id,
        } => Ok(MaskSelector::Slot(
            MaskSlotSelector::new(
                Subject::new(parse_id::<EntityId>(&subject_id, "Entity")?),
                parse_id::<PredicateId>(&predicate_id, "Predicate")?,
                context.perspective_scope(),
                context.epistemic_mode(),
            )
            .map_err(|error| EngineError::Fact(error.to_string()))?,
        )),
    }
}

fn parse_value(input: FactValueInput) -> Result<Value, EngineError> {
    match input {
        FactValueInput::Bool(value) => Ok(Value::Bool(value)),
        FactValueInput::Int(value) => Int::from_str(&value)
            .map(Value::Int)
            .map_err(|_| EngineError::Fact("invalid exact signed integer".to_owned())),
        FactValueInput::UInt(value) => UInt::from_str(&value)
            .map(Value::UInt)
            .map_err(|_| EngineError::Fact("invalid exact unsigned integer".to_owned())),
        FactValueInput::Decimal(value) => Decimal::from_str(&value)
            .map(Value::Decimal)
            .map_err(|_| EngineError::Fact("invalid exact decimal".to_owned())),
        FactValueInput::String(value) => Ok(Value::String(value)),
        FactValueInput::Symbol(value) => Symbol::new(value)
            .map(Value::Symbol)
            .map_err(|error| EngineError::Fact(error.to_string())),
        FactValueInput::Entity(value) => parse_id::<EntityId>(&value, "Entity").map(Value::Entity),
        FactValueInput::Time {
            timeline_id,
            ticks,
            unit_symbol,
        } => {
            let timeline_id = parse_id::<TimelineId>(&timeline_id, "Timeline")?;
            let ticks = ticks
                .parse::<i128>()
                .map_err(|_| EngineError::Fact("invalid signed time ticks".to_owned()))?;
            let unit =
                Symbol::new(unit_symbol).map_err(|error| EngineError::Fact(error.to_string()))?;
            Ok(Value::Time(Time::new(timeline_id, ticks, unit)))
        }
        FactValueInput::Duration(value) => value
            .parse::<i128>()
            .map(|value| Value::Duration(worlddb_core::Duration::from_nanoseconds(value)))
            .map_err(|_| EngineError::Fact("invalid signed duration in nanoseconds".to_owned())),
        FactValueInput::BytesHex(value) => {
            parse_hex(&value).map(|bytes| Value::Bytes(Bytes::new(bytes)))
        }
    }
}

fn parse_validity(input: ValidityInput) -> Result<AssertionValidity, EngineError> {
    let timeline_id = parse_id::<TimelineId>(&input.timeline_id, "Timeline")?;
    let timeline = Timeline::new(timeline_id);
    let start = input
        .start_nanoseconds
        .as_deref()
        .map(|value| parse_world_time(timeline, value))
        .transpose()?;
    let end = input
        .end_nanoseconds
        .as_deref()
        .map(|value| parse_world_time(timeline, value))
        .transpose()?;
    TimeInterval::new(timeline, start, end)
        .map(AssertionValidity::new)
        .map_err(|error| EngineError::Fact(error.to_string()))
}

fn parse_world_time_selector(
    input: WorldTimeSelectorInput,
) -> Result<WorldTimeSelector, EngineError> {
    match input {
        WorldTimeSelectorInput::AllTimes => Ok(WorldTimeSelector::AllTimes),
        WorldTimeSelectorInput::At {
            timeline_id,
            nanoseconds,
        } => {
            let timeline_id = parse_id::<TimelineId>(&timeline_id, "Timeline")?;
            let world_time = parse_world_time(Timeline::new(timeline_id), &nanoseconds)?;
            Ok(WorldTimeSelector::At(world_time))
        }
    }
}

fn parse_world_time(timeline: Timeline, nanoseconds: &str) -> Result<WorldTime, EngineError> {
    let nanoseconds = nanoseconds
        .parse::<i128>()
        .map_err(|_| EngineError::Fact("invalid signed world-time nanoseconds".to_owned()))?;
    Ok(WorldTime::from_nanoseconds(timeline, nanoseconds))
}

fn parse_hex(value: &str) -> Result<Vec<u8>, EngineError> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(EngineError::Fact(
            "binary value must be even-length hexadecimal".to_owned(),
        ));
    }
    (0..value.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&value[offset..offset + 2], 16)
                .map_err(|_| EngineError::Fact("invalid hexadecimal byte".to_owned()))
        })
        .collect()
}

fn parse_id<T>(value: &str, label: &str) -> Result<T, EngineError>
where
    T: DomainId + FromStr,
    T::Err: fmt::Display,
{
    T::from_str(value).map_err(|_| EngineError::Fact(format!("invalid {label} identity")))
}

fn revision(value: u64) -> Result<Revision, EngineError> {
    Revision::new(value).map_err(|_| EngineError::Fact("invalid data revision".to_owned()))
}

fn operation_id() -> Result<worlddb_core::OperationId, EngineError> {
    worlddb_core::storage_internal::generate_schema_management_operation_id()
        .map_err(|_| EngineError::Fact("operation identity unavailable".to_owned()))
}

fn identity<T: DomainId>(label: &str) -> Result<T, EngineError> {
    worlddb_core::storage_internal::generate_project_bootstrap_id::<T>()
        .map_err(|_| EngineError::Fact(format!("{label} identity unavailable")))
}

fn preview_view(
    revision: u64,
    output: QueryEngineOutput<ResolutionPreview>,
) -> ResolutionPreviewView {
    let result = match output.query().value() {
        ResolutionPreview::Point {
            world_time,
            resolved_view,
        } => ResolutionResultView::Point {
            timeline_id: world_time.timeline().id().to_string(),
            nanoseconds: world_time.nanoseconds().to_string(),
            outcome: outcome_view(resolved_view),
        },
        ResolutionPreview::AllTimes { slices } => ResolutionResultView::AllTimes {
            slices: slices
                .iter()
                .map(|slice| {
                    let interval = slice.interval();
                    ResolutionSliceView {
                        timeline_id: interval.timeline().id().to_string(),
                        start_nanoseconds: interval
                            .start()
                            .map(|time| time.nanoseconds().to_string()),
                        end_nanoseconds: interval.end().map(|time| time.nanoseconds().to_string()),
                        outcome: outcome_view(slice.resolved_view()),
                    }
                })
                .collect(),
        },
        ResolutionPreview::CompleteEmpty => ResolutionResultView::CompleteEmpty,
    };
    ResolutionPreviewView { revision, result }
}

fn outcome_view(view: &ResolvedView) -> ResolutionOutcomeView {
    let all_contributors = view
        .contributors()
        .iter()
        .map(ToString::to_string)
        .collect();
    match view.outcome() {
        ResolvedOutcome::Single(outcome) => match outcome {
            worlddb_core::SingleValueOutcome::Known {
                value,
                polarity,
                contributors,
            } => ResolutionOutcomeView::Known {
                values: vec![ResolutionValueView {
                    value: value_label(value),
                    polarity: polarity_label(*polarity).to_owned(),
                    contributors: ids(contributors),
                }],
                contributors: all_contributors,
            },
            worlddb_core::SingleValueOutcome::Unknown => ResolutionOutcomeView::Unknown,
            worlddb_core::SingleValueOutcome::Conflict { .. } => ResolutionOutcomeView::Conflict {
                values: Vec::new(),
                conflicts: Vec::new(),
                contributors: all_contributors,
            },
        },
        ResolvedOutcome::Multi(outcome) => match outcome {
            MultiValueOutcome::Known { values } => ResolutionOutcomeView::Known {
                values: values.iter().map(value_view).collect(),
                contributors: all_contributors,
            },
            MultiValueOutcome::Unknown => ResolutionOutcomeView::Unknown,
            MultiValueOutcome::Conflict { values, conflicts } => ResolutionOutcomeView::Conflict {
                values: values.iter().map(value_view).collect(),
                conflicts: conflicts.iter().map(conflict_view).collect(),
                contributors: all_contributors,
            },
        },
    }
}

fn value_view(value: &MultiValueEntry) -> ResolutionValueView {
    ResolutionValueView {
        value: value_label(value.value()),
        polarity: polarity_label(value.polarity()).to_owned(),
        contributors: ids(value.contributors()),
    }
}

fn conflict_view(value: &MultiValueConflict) -> ResolutionConflictView {
    ResolutionConflictView {
        value: value_label(value.value()),
        positive_contributors: ids(value.positive_contributors()),
        negative_contributors: ids(value.negative_contributors()),
    }
}

fn ids(values: &[AssertionId]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn polarity_label(value: Polarity) -> &'static str {
    match value {
        Polarity::Positive => "positive",
        Polarity::Negative => "negative",
    }
}

fn value_label(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::UInt(value) => value.to_string(),
        Value::Decimal(value) => value
            .to_canonical_string(128)
            .unwrap_or_else(|_| "<decimal>".to_owned()),
        Value::String(value) => value.clone(),
        Value::Symbol(value) => value.as_str().to_owned(),
        Value::Entity(value) => value.to_string(),
        Value::Time(value) => format!("{}:{} {}", value.timeline_id(), value.ticks(), value.unit()),
        Value::Duration(value) => format!("{} ns", value.nanoseconds()),
        Value::Bytes(value) => bytes_hex(value.as_slice()),
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn fact_error(error: impl fmt::Display) -> EngineError {
    EngineError::Fact(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{FactValueInput, MaskSelectorInput, bytes_hex, parse_hex, parse_value};

    #[test]
    fn scalar_input_is_closed_and_keeps_exact_text_values() {
        assert!(matches!(
            parse_value(FactValueInput::Int("-170141183460469231731687303715884105728".to_owned())),
            Ok(worlddb_core::Value::Int(value)) if value.value() == i128::MIN
        ));
        assert!(matches!(
            parse_value(FactValueInput::UInt("340282366920938463463374607431768211455".to_owned())),
            Ok(worlddb_core::Value::UInt(value)) if value.value() == u128::MAX
        ));
        assert!(parse_value(FactValueInput::Symbol("Upper".to_owned())).is_err());
        assert!(
            serde_json::from_str::<MaskSelectorInput>(r#"{"kind":"arbitrary","anything":true}"#)
                .is_err()
        );
    }

    #[test]
    fn binary_input_requires_even_length_hexadecimal() {
        assert_eq!(
            parse_hex("00aF").expect("valid hexadecimal bytes"),
            vec![0x00, 0xaf]
        );
        assert_eq!(bytes_hex(&[0x00, 0xaf]), "00af");
        assert!(parse_hex("f").is_err());
        assert!(parse_hex("gg").is_err());
    }
}
