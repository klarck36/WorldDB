//! Authenticated Assertion, Mask, ReplacementBoundary, and resolution-preview commands.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    AssertionDraft, AssertionId, AssertionValidity, Bytes, ContextKey, Decimal, DomainId, EntityId,
    EpistemicMode, HistorySpaceId, Int, LayerId, LayerSelection, MaskId, MaskSelector,
    MaskSlotSelector, MultiValueConflict, MultiValueEntry, MultiValueOutcome, NonEmptySet,
    PerspectiveId, PerspectiveScope, Polarity, PredicateId, PropositionKey, QueryEngineOutput,
    ReplacementBoundaryId, ResolutionPreview, ResolvedOutcome, ResolvedView, Revision, Subject,
    Symbol, Time, TimeInterval, Timeline, TimelineId, UInt, Value, WorldTime, WorldTimeSelector,
};
use worlddb_storage_file::{
    FactResolutionPreviewRequest, FileFactManager, ReplacementBoundaryDraft,
};

use crate::{EngineError, EngineHost, EpistemicModeInput};

/// One closed factual-record or resolution-preview action from the renderer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum FactCommand {
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
    /// Evaluates one slot through the productive resolution-preview path.
    Preview {
        context: FactContextInput,
        subject_id: String,
        predicate_id: String,
        world_time: WorldTimeSelectorInput,
    },
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
    Published(FactPublicationView),
    Preview(ResolutionPreviewView),
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

impl From<PolarityInput> for Polarity {
    fn from(value: PolarityInput) -> Self {
        match value {
            PolarityInput::Positive => Self::Positive,
            PolarityInput::Negative => Self::Negative,
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
