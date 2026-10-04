//! Authenticated, preview-bound HistorySpace content transfer commands.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use worlddb_core::{
    EventRelationId, ExternalReferenceDecision, HistorySpaceContentRef, HistorySpaceId, RecordRef,
    Revision,
};
use worlddb_storage_file::{
    FileHistorySpaceTransferManager, TransferEventRelationItem, TransferSourceItem,
};

use crate::{EngineError, EngineHost};

const PREVIEW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PREVIEWS: usize = 64;

/// One explicit request to inspect, preview, or publish a HistorySpace transfer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistorySpaceTransferCommand {
    /// Lists source-visible content and project-wide relations for a pinned revision.
    List {
        source_history_space_id: String,
        target_history_space_id: String,
        source_recorded_as_of: u64,
    },
    /// Creates a fully typed plan and returns a short-lived preview ticket.
    Preview {
        source_history_space_id: String,
        target_history_space_id: String,
        source_recorded_as_of: u64,
        selected_records: Vec<ContentIdentityInput>,
        selected_event_relations: Vec<String>,
        external_reference_policy: ExternalReferencePolicyInput,
    },
    /// Publishes only the plan bound to a live preview ticket.
    Commit {
        preview_ticket: String,
        acknowledge_lifecycle_omissions: bool,
    },
}

/// Closed renderer value for one source record identity. IDs remain hidden in the UI.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentIdentityInput {
    pub family: String,
    pub id: String,
}

/// Explicit policy for references outside the selected transfer set.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalReferencePolicyInput {
    Reject,
    RetainIfVisible,
}

/// Result of one HistorySpace transfer command.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistorySpaceTransferResponse {
    Catalog(TransferCatalogView),
    Preview(TransferPreviewView),
    Published(TransferPublishedView),
}

/// Safe source/target inventory for building an explicit selection.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferCatalogView {
    pub current_revision: u64,
    pub source_head: u64,
    pub target_head: u64,
    pub records: Vec<TransferContentView>,
    pub event_relations: Vec<TransferRelationView>,
}

/// One selectable content row; identity fields are used only as hidden values.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferContentView {
    pub family: String,
    pub id: String,
    pub label: String,
    pub recorded_revision: u64,
    pub archived: bool,
    pub selectable: bool,
}

/// One source-visible relation; identity is used only as a hidden value.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferRelationView {
    pub id: String,
    pub label: String,
    pub archived: bool,
    pub retracted: bool,
}

/// Validation outcome bound to an opaque host-created preview ticket.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferPreviewView {
    pub preview_ticket: String,
    pub source_revision: u64,
    pub target_head: u64,
    pub copied_record_count: usize,
    pub copied_relation_count: usize,
    pub omitted_lifecycle_count: usize,
    pub omitted_relation_retraction_count: usize,
    pub archived_records_start_unarchived: usize,
    pub archived_relations_start_unarchived: usize,
    pub confirmation_required: bool,
}

/// Receipt for a durable, audited transfer publication.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransferPublishedView {
    pub operation_id: String,
    pub revision: u64,
    pub copied_record_count: usize,
    pub copied_relation_count: usize,
}

pub(super) struct PendingTransferPreview {
    plan: worlddb_core::TransferPlan,
    expires_at: Instant,
}

impl EngineHost {
    /// Executes an authenticated HistorySpace content transfer workflow.
    pub fn history_space_transfer(
        &self,
        command: HistorySpaceTransferCommand,
    ) -> Result<HistorySpaceTransferResponse, EngineError> {
        execute(self, command)
    }
}

pub(super) fn execute(
    engine: &EngineHost,
    command: HistorySpaceTransferCommand,
) -> Result<HistorySpaceTransferResponse, EngineError> {
    let _guard = engine.schema_management.lock().map_err(|_| {
        EngineError::HistorySpaceTransfer("transfer manager state is unavailable".to_owned())
    })?;
    let principal = engine.principal_id.ok_or_else(|| {
        EngineError::HistorySpaceTransfer("an authenticated project session is required".to_owned())
    })?;
    let mut manager = FileHistorySpaceTransferManager::open(
        engine._layout.clone(),
        &engine._writer_lock,
        principal,
    )
    .map_err(transfer_error)?;

    match command {
        HistorySpaceTransferCommand::List {
            source_history_space_id,
            target_history_space_id,
            source_recorded_as_of,
        } => {
            let source = parse_history_space(&source_history_space_id)?;
            let target = parse_history_space(&target_history_space_id)?;
            let as_of = revision_from_u64(source_recorded_as_of)?;
            let records = manager
                .visible_content(source, target, as_of)
                .map_err(transfer_error)?
                .iter()
                .map(content_view)
                .collect();
            let relations = manager
                .visible_event_relations(source, as_of)
                .map_err(transfer_error)?
                .iter()
                .copied()
                .map(relation_view)
                .collect();
            Ok(HistorySpaceTransferResponse::Catalog(TransferCatalogView {
                current_revision: manager.revision().value(),
                source_head: manager
                    .history_space_head(source)
                    .map_err(transfer_error)?
                    .value(),
                target_head: manager
                    .history_space_head(target)
                    .map_err(transfer_error)?
                    .value(),
                records,
                event_relations: relations,
            }))
        }
        HistorySpaceTransferCommand::Preview {
            source_history_space_id,
            target_history_space_id,
            source_recorded_as_of,
            selected_records,
            selected_event_relations,
            external_reference_policy,
        } => {
            if selected_records.len() > 1_024 || selected_event_relations.len() > 1_024 {
                return Err(EngineError::HistorySpaceTransfer(
                    "transfer selection exceeds the supported limit".to_owned(),
                ));
            }
            let source = parse_history_space(&source_history_space_id)?;
            let target = parse_history_space(&target_history_space_id)?;
            let as_of = revision_from_u64(source_recorded_as_of)?;
            let selected_record_count = selected_records.len();
            let selected_records = selected_records
                .iter()
                .map(parse_content_identity)
                .collect::<Result<BTreeSet<_>, _>>()?;
            if selected_records.is_empty() {
                return Err(EngineError::HistorySpaceTransfer(
                    "select at least one content record".to_owned(),
                ));
            }
            if selected_records.len() != selected_record_count {
                return Err(EngineError::HistorySpaceTransfer(
                    "a content record was selected more than once".to_owned(),
                ));
            }
            let selected_relation_count = selected_event_relations.len();
            let selected_event_relations = selected_event_relations
                .iter()
                .map(|id| {
                    EventRelationId::from_str(id).map_err(|_| {
                        EngineError::HistorySpaceTransfer(
                            "an event relationship selection is invalid".to_owned(),
                        )
                    })
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            if selected_event_relations.len() != selected_relation_count {
                return Err(EngineError::HistorySpaceTransfer(
                    "an event relationship was selected more than once".to_owned(),
                ));
            }
            let decision = match external_reference_policy {
                ExternalReferencePolicyInput::Reject => ExternalReferenceDecision::Reject,
                ExternalReferencePolicyInput::RetainIfVisible => {
                    ExternalReferenceDecision::RetainVisible
                }
            };
            let plan = manager
                .build_plan(
                    source,
                    target,
                    as_of,
                    selected_records,
                    selected_event_relations,
                    decision,
                )
                .map_err(transfer_error)?;
            let copied_record_count = plan.record_id_map().len();
            let copied_relation_count = plan.event_relation_id_map().len();
            let preview = manager.preview(&plan).map_err(transfer_error)?;
            let all_records = manager
                .visible_content(source, target, as_of)
                .map_err(transfer_error)?;
            let all_relations = manager
                .visible_event_relations(source, as_of)
                .map_err(transfer_error)?;
            let archived_records_start_unarchived = plan
                .selected_records()
                .iter()
                .filter(|identity| {
                    all_records
                        .iter()
                        .any(|record| record.identity() == **identity && record.is_archived())
                })
                .count();
            let archived_relations_start_unarchived = plan
                .selected_event_relations()
                .iter()
                .filter(|identity| {
                    all_relations
                        .iter()
                        .any(|relation| relation.id() == **identity && relation.is_archived())
                })
                .count();
            let ticket = worlddb_core::storage_internal::generate_project_bootstrap_id::<
                worlddb_core::OperationId,
            >()
            .map_err(|_| {
                EngineError::HistorySpaceTransfer("preview identity is unavailable".to_owned())
            })?
            .to_string();
            let mut pending = engine.transfer_previews.lock().map_err(|_| {
                EngineError::HistorySpaceTransfer("transfer previews are unavailable".to_owned())
            })?;
            pending.retain(|_, preview| preview.expires_at > Instant::now());
            if pending.len() >= MAX_PREVIEWS {
                return Err(EngineError::HistorySpaceTransfer(
                    "too many active transfer previews; refresh the transfer screen".to_owned(),
                ));
            }
            pending.insert(
                ticket.clone(),
                PendingTransferPreview {
                    plan,
                    expires_at: Instant::now() + PREVIEW_TTL,
                },
            );
            Ok(HistorySpaceTransferResponse::Preview(TransferPreviewView {
                preview_ticket: ticket,
                source_revision: as_of.value(),
                target_head: manager
                    .history_space_head(target)
                    .map_err(transfer_error)?
                    .value(),
                copied_record_count,
                copied_relation_count,
                omitted_lifecycle_count: preview.omitted_effective_lifecycle_records().len(),
                omitted_relation_retraction_count: preview
                    .omitted_event_relation_retractions()
                    .len(),
                archived_records_start_unarchived,
                archived_relations_start_unarchived,
                confirmation_required: true,
            }))
        }
        HistorySpaceTransferCommand::Commit {
            preview_ticket,
            acknowledge_lifecycle_omissions,
        } => {
            let pending = engine
                .transfer_previews
                .lock()
                .map_err(|_| {
                    EngineError::HistorySpaceTransfer(
                        "transfer previews are unavailable".to_owned(),
                    )
                })?
                .remove(&preview_ticket)
                .ok_or_else(|| {
                    EngineError::HistorySpaceTransfer(
                        "transfer preview expired or was already used; create a new preview"
                            .to_owned(),
                    )
                })?;
            if pending.expires_at <= Instant::now() {
                return Err(EngineError::HistorySpaceTransfer(
                    "transfer preview expired; create a new preview".to_owned(),
                ));
            }
            let record_count = pending.plan.record_id_map().len();
            let relation_count = pending.plan.event_relation_id_map().len();
            let receipt = manager
                .commit(
                    &pending.plan,
                    crate::requested_operation_id_or(
                        worlddb_core::storage_internal::generate_schema_management_operation_id,
                    )
                    .map_err(|_| {
                        EngineError::HistorySpaceTransfer(
                            "operation identity is unavailable".to_owned(),
                        )
                    })?,
                    acknowledge_lifecycle_omissions,
                )
                .map_err(transfer_error)?;
            Ok(HistorySpaceTransferResponse::Published(
                TransferPublishedView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    copied_record_count: record_count,
                    copied_relation_count: relation_count,
                },
            ))
        }
    }
}

fn parse_history_space(value: &str) -> Result<HistorySpaceId, EngineError> {
    HistorySpaceId::from_str(value)
        .map_err(|_| EngineError::HistorySpaceTransfer("a selected branch is invalid".to_owned()))
}

fn revision_from_u64(value: u64) -> Result<Revision, EngineError> {
    Revision::try_from(value).map_err(|_| {
        EngineError::HistorySpaceTransfer("the selected revision is invalid".to_owned())
    })
}

fn parse_content_identity(
    input: &ContentIdentityInput,
) -> Result<HistorySpaceContentRef, EngineError> {
    macro_rules! parse_ref {
        ($id_type:ty, $variant:ident) => {
            <$id_type>::from_str(&input.id)
                .map(RecordRef::$variant)
                .map_err(|_| {
                    EngineError::HistorySpaceTransfer(
                        "a selected content identity is invalid".to_owned(),
                    )
                })
        };
    }
    let reference = match input.family.as_str() {
        "assertion" => parse_ref!(worlddb_core::AssertionId, Assertion)?,
        "mask" => parse_ref!(worlddb_core::MaskId, Mask)?,
        "replacement_boundary" => {
            parse_ref!(worlddb_core::ReplacementBoundaryId, ReplacementBoundary)?
        }
        "event" => parse_ref!(worlddb_core::EventId, Event)?,
        "event_mask" => parse_ref!(worlddb_core::EventMaskId, EventMask)?,
        "assertion_validity_closure" => parse_ref!(
            worlddb_core::AssertionValidityClosureId,
            AssertionValidityClosure
        )?,
        "assertion_retraction" => {
            parse_ref!(worlddb_core::AssertionRetractionId, AssertionRetraction)?
        }
        "mask_validity_closure" => {
            parse_ref!(worlddb_core::MaskValidityClosureId, MaskValidityClosure)?
        }
        "mask_retraction" => parse_ref!(worlddb_core::MaskRetractionId, MaskRetraction)?,
        "replacement_boundary_validity_closure" => parse_ref!(
            worlddb_core::ReplacementBoundaryValidityClosureId,
            ReplacementBoundaryValidityClosure
        )?,
        "replacement_boundary_retraction" => parse_ref!(
            worlddb_core::ReplacementBoundaryRetractionId,
            ReplacementBoundaryRetraction
        )?,
        "event_span_closure" => parse_ref!(worlddb_core::EventSpanClosureId, EventSpanClosure)?,
        "event_retraction" => parse_ref!(worlddb_core::EventRetractionId, EventRetraction)?,
        "event_mask_retraction" => {
            parse_ref!(worlddb_core::EventMaskRetractionId, EventMaskRetraction)?
        }
        _ => {
            return Err(EngineError::HistorySpaceTransfer(
                "a selected record type is unsupported".to_owned(),
            ));
        }
    };
    HistorySpaceContentRef::try_from(reference).map_err(|_| {
        EngineError::HistorySpaceTransfer("a selected record type is unsupported".to_owned())
    })
}

fn content_view(item: &TransferSourceItem) -> TransferContentView {
    let reference = item.identity().record_ref();
    let family = family_key(reference);
    TransferContentView {
        family: family.to_owned(),
        id: record_id(reference),
        label: family_label(family).to_owned(),
        recorded_revision: item.recorded_revision().value(),
        archived: item.is_archived(),
        selectable: item.can_transfer(),
    }
}

fn relation_view(item: TransferEventRelationItem) -> TransferRelationView {
    let label = match item.kind() {
        worlddb_core::EventRelationKind::Before => "Vorher",
        worlddb_core::EventRelationKind::SameTime => "Gleichzeitig",
        worlddb_core::EventRelationKind::Causes => "Verursacht",
    };
    TransferRelationView {
        id: item.id().to_string(),
        label: label.to_owned(),
        archived: item.is_archived(),
        retracted: item.is_retracted(),
    }
}

fn family_key(reference: RecordRef) -> &'static str {
    match reference {
        RecordRef::Assertion(_) => "assertion",
        RecordRef::Mask(_) => "mask",
        RecordRef::ReplacementBoundary(_) => "replacement_boundary",
        RecordRef::Event(_) => "event",
        RecordRef::EventMask(_) => "event_mask",
        RecordRef::AssertionValidityClosure(_) => "assertion_validity_closure",
        RecordRef::AssertionRetraction(_) => "assertion_retraction",
        RecordRef::MaskValidityClosure(_) => "mask_validity_closure",
        RecordRef::MaskRetraction(_) => "mask_retraction",
        RecordRef::ReplacementBoundaryValidityClosure(_) => "replacement_boundary_validity_closure",
        RecordRef::ReplacementBoundaryRetraction(_) => "replacement_boundary_retraction",
        RecordRef::EventSpanClosure(_) => "event_span_closure",
        RecordRef::EventRetraction(_) => "event_retraction",
        RecordRef::EventMaskRetraction(_) => "event_mask_retraction",
        _ => "unsupported",
    }
}

fn family_label(family: &str) -> &'static str {
    match family {
        "assertion" => "Aussage",
        "mask" => "Maske",
        "replacement_boundary" => "Ersetzungsgrenze",
        "event" => "Ereignis",
        "event_mask" => "Ereignismaske",
        "assertion_validity_closure" => "Gültigkeitsende einer Aussage",
        "assertion_retraction" => "Zurücknahme einer Aussage",
        "mask_validity_closure" => "Gültigkeitsende einer Maske",
        "mask_retraction" => "Zurücknahme einer Maske",
        "replacement_boundary_validity_closure" => "Gültigkeitsende einer Ersetzungsgrenze",
        "replacement_boundary_retraction" => "Zurücknahme einer Ersetzungsgrenze",
        "event_span_closure" => "Ende einer Ereignisspanne",
        "event_retraction" => "Zurücknahme eines Ereignisses",
        "event_mask_retraction" => "Zurücknahme einer Ereignismaske",
        _ => "Unbekannter Inhalt",
    }
}

fn record_id(reference: RecordRef) -> String {
    match reference {
        RecordRef::Assertion(id) => id.to_string(),
        RecordRef::Mask(id) => id.to_string(),
        RecordRef::ReplacementBoundary(id) => id.to_string(),
        RecordRef::Event(id) => id.to_string(),
        RecordRef::EventMask(id) => id.to_string(),
        RecordRef::AssertionValidityClosure(id) => id.to_string(),
        RecordRef::AssertionRetraction(id) => id.to_string(),
        RecordRef::MaskValidityClosure(id) => id.to_string(),
        RecordRef::MaskRetraction(id) => id.to_string(),
        RecordRef::ReplacementBoundaryValidityClosure(id) => id.to_string(),
        RecordRef::ReplacementBoundaryRetraction(id) => id.to_string(),
        RecordRef::EventSpanClosure(id) => id.to_string(),
        RecordRef::EventRetraction(id) => id.to_string(),
        RecordRef::EventMaskRetraction(id) => id.to_string(),
        _ => String::new(),
    }
}

fn transfer_error(error: impl std::fmt::Display) -> EngineError {
    EngineError::HistorySpaceTransfer(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        ContentIdentityInput, ExternalReferencePolicyInput, HistorySpaceTransferCommand,
        HistorySpaceTransferResponse, parse_content_identity,
    };

    #[test]
    fn content_identity_parser_is_closed_over_supported_transfer_families() -> Result<(), String> {
        let invalid = ContentIdentityInput {
            family: "operation".to_owned(),
            id: "00000000-0000-7000-8000-000000000001".to_owned(),
        };
        if parse_content_identity(&invalid).is_ok() {
            return Err("unsupported record reference entered the transfer plan".to_owned());
        }
        let request = HistorySpaceTransferCommand::Preview {
            source_history_space_id: "00000000-0000-7000-8000-000000000001".to_owned(),
            target_history_space_id: "00000000-0000-7000-8000-000000000002".to_owned(),
            source_recorded_as_of: 1,
            selected_records: vec![],
            selected_event_relations: vec![],
            external_reference_policy: ExternalReferencePolicyInput::Reject,
        };
        let encoded = serde_json::to_value(request).map_err(|error| error.to_string())?;
        if encoded["command"] != "preview" || encoded["selected_records"] != serde_json::json!([]) {
            return Err("transfer preview command wire shape changed".to_owned());
        }
        let response = HistorySpaceTransferResponse::Published(super::TransferPublishedView {
            operation_id: "00000000-0000-7000-8000-000000000001".to_owned(),
            revision: 9,
            copied_record_count: 2,
            copied_relation_count: 0,
        });
        let encoded = serde_json::to_value(response).map_err(|error| error.to_string())?;
        if encoded["kind"] != "published"
            || encoded["operation_id"] != "00000000-0000-7000-8000-000000000001"
            || encoded["revision"] != 9
        {
            return Err("transfer publication response wire shape changed".to_owned());
        }
        Ok(())
    }
}
