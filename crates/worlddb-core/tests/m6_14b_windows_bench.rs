//! Manual full-corpus Windows performance probe for M6-14b.
//!
//! Run through tools/m6-14b/windows_query_history_probe.py. The test is ignored
//! during normal workspace verification because it loads and indexes one
//! million assertion records and runs 30 complete page walks per cache mode.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use worlddb_core::{
    ArchiveHistoryReferenceModel, ArchiveTargetRecord, ArchiveTargetRef, Assertion, AssertionDraft,
    AssertionHistoryRecord, AssertionPointHistoryIndex, AssertionPointIndexAccess,
    AssertionPointRequest, AssertionQueryStore, AssertionValidity, AuthorizationDecision,
    AuthorizationMode, AuthorizedAssertionMaskHistory, Capability, CapabilityGrant, Cardinality,
    ConstraintSet, ContextKey, CursorStateStore, CursorStoreLimits, DomainId, EpistemicMode,
    FullScanBudget, GrantEffect, HistoricalQueryBinding, HistorySpaceDefinition, HistorySpaceId,
    HistorySpaceReferenceModel, HistorySpaceView, IndexBuildVersion, IndexFamily,
    IndexFormatVersion, IndexGenerationMetadata, IndexRevisionCoverage, IndexSchemaVersion,
    LayerDefinition, LayerSchemaSnapshot, LayerSelection, Lifecycle, Mask, MaskId, MaskSelector,
    MultiValueSlot, PageExecution, PageOperation, PageRequest, PerspectiveScope, PolicyBundle,
    PolicyScope, PolicyTarget, PredicateDefinition, PredicateDefinitionSpec, Principal,
    PrincipalId, ProductiveQueryEngine, QueryBudget, QueryBudgetLimits, QueryContext,
    QueryContextInput, QueryExecutionPath, QueryHash, RecordedAsOf, ResolutionPolicy, Revision,
    RoleAssignment, RoleDefinition, SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode,
    SchemaRevision, SecurityContext, SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot,
    SecurityPolicyVersion, SnapshotBinding, SnapshotBindingInput, SnapshotId,
    SnapshotLifetimeLimits, SnapshotPinPurpose, SnapshotRef, SnapshotRegistry,
    SnapshotSecurityBinding, Subject, Symbol, TimeInterval, Timeline, TimelineId,
    ValidatedLayerSelection, Value, ValueKind, WorldTime, WorldTimeSelector,
};

const HISTORY_SPACE_COUNT: usize = 100;
const ASSERTION_COUNT: usize = 1_000_000;
const FINAL_REVISION: u64 = HISTORY_SPACE_COUNT as u64;
const SELECTED_HISTORY_SPACE: u16 = 100;
const PAGE_SIZE: u32 = 64;
const SAMPLE_PASSES: usize = 30;
const CPU_CACHE_EVICTION_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct CorpusHistoryRow {
    assertion_id: worlddb_core::AssertionId,
    history_space_id: HistorySpaceId,
    layer_id: worlddb_core::LayerId,
}

struct BenchData {
    history: HistorySpaceReferenceModel<AssertionHistoryRecord>,
    archive: ArchiveHistoryReferenceModel,
    layers: LayerSchemaSnapshot,
    policies: SecurityPolicyHistory,
    point_context: QueryContext,
    page_context: QueryContext,
    predicate: PredicateDefinition,
    point_index: AssertionPointHistoryIndex,
    point_metadata: IndexGenerationMetadata,
    masks: Vec<Mask>,
    archive_visible_mask_ids: BTreeSet<MaskId>,
    page_rows: Vec<CorpusHistoryRow>,
}

fn id<T: DomainId>(value: u64) -> Result<T, Box<dyn Error>> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8..].copy_from_slice(&value.to_be_bytes());
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(T::try_from_bytes(bytes)?)
}

fn revision(value: u64) -> Result<Revision, Box<dyn Error>> {
    Ok(Revision::new(value)?)
}

fn word_u16(bytes: &[u8], offset: usize) -> Result<u16, Box<dyn Error>> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or_else(|| io::Error::other("truncated u16 in M6-14a corpus"))?
            .try_into()?,
    ))
}

fn word_u32(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(|| io::Error::other("truncated u32 in M6-14a corpus"))?
            .try_into()?,
    ))
}

fn word_u64(bytes: &[u8], offset: usize) -> Result<u64, Box<dyn Error>> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or_else(|| io::Error::other("truncated u64 in M6-14a corpus"))?
            .try_into()?,
    ))
}

fn word_i64(bytes: &[u8], offset: usize) -> Result<i64, Box<dyn Error>> {
    Ok(i64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or_else(|| io::Error::other("truncated i64 in M6-14a corpus"))?
            .try_into()?,
    ))
}

fn corpus_spaces(
    corpus_dir: &Path,
) -> Result<(Vec<HistorySpaceDefinition>, Vec<u16>), Box<dyn Error>> {
    let bytes = fs::read(corpus_dir.join("history_spaces.bin"))?;
    if bytes.len() != HISTORY_SPACE_COUNT * 16 {
        return Err(io::Error::other("history_spaces.bin has an unexpected size").into());
    }
    let mut definitions = Vec::with_capacity(HISTORY_SPACE_COUNT);
    let mut parent_ids = Vec::with_capacity(HISTORY_SPACE_COUNT + 1);
    parent_ids.push(u16::MAX);
    for row in bytes.chunks_exact(16) {
        let numeric_id = word_u16(row, 0)?;
        let parent_id = word_u16(row, 2)?;
        let source_base_revision = word_u64(row, 4)?;
        if numeric_id as usize != definitions.len() + 1
            || source_base_revision != u64::from(numeric_id) * 1_000
            || word_u16(row, 14)? != 0
        {
            return Err(io::Error::other("history-space corpus contract mismatch").into());
        }
        let expected_parent = if numeric_id == 1 {
            u16::MAX
        } else {
            numeric_id / 2
        };
        if parent_id != expected_parent {
            return Err(io::Error::other("history-space parent tree is not canonical").into());
        }
        parent_ids.push(parent_id);

        // The corpus stores spaced revision labels rather than actual commits.
        // The monotone mapping base=i*1000 -> i-1 preserves ordering and every
        // parent cutoff while making the executable history compact.
        let base_revision = if numeric_id == 1 {
            Revision::GENESIS
        } else {
            revision(u64::from(numeric_id) - 1)?
        };
        definitions.push(HistorySpaceDefinition::new(
            id::<HistorySpaceId>(u64::from(numeric_id))?,
            (parent_id != u16::MAX)
                .then(|| id::<HistorySpaceId>(u64::from(parent_id)))
                .transpose()?,
            base_revision,
        )?);
    }
    Ok((definitions, parent_ids))
}

fn is_selected_ancestry(history_space_id: u16, parent_ids: &[u16]) -> bool {
    let mut current = SELECTED_HISTORY_SPACE;
    loop {
        if current == history_space_id {
            return true;
        }
        if current == 1 {
            return false;
        }
        let Some(parent) = parent_ids.get(usize::from(current)).copied() else {
            return false;
        };
        current = parent;
    }
}

fn build_schema_and_policy(
    layers: &LayerSchemaSnapshot,
) -> Result<
    (
        SchemaHistoryReferenceModel,
        SecurityPolicyHistory,
        PrincipalId,
    ),
    Box<dyn Error>,
> {
    let published = revision(FINAL_REVISION)?;
    let mut schema_history = SchemaHistoryReferenceModel::new();

    let predicates = (1_u64..=1_024)
        .map(|number| {
            Ok(PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id: id(number)?,
                symbol: Symbol::new(format!("p{number:04}"))?,
                subject_constraint: worlddb_core::EntityTypeConstraint::AnyEntity,
                value_kind: ValueKind::String,
                object_constraint: None,
                cardinality: Cardinality::Single,
                resolution_policy: ResolutionPolicy::SingleValueReplace,
                constraints: ConstraintSet::unconstrained(),
                decimal_metadata: None,
                lifecycle: Lifecycle::Active,
                created_revision: published,
            })?)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let mut definitions = Vec::with_capacity(predicates.len() + 1);
    definitions.push(SchemaDefinition::LayerSnapshot(layers.clone()));
    definitions.extend(predicates.into_iter().map(SchemaDefinition::Predicate));
    schema_history.publish(published, definitions)?;

    let principal_id = id::<PrincipalId>(1)?;
    let role_id = id::<worlddb_core::RoleId>(2)?;
    let assignment_id = id::<worlddb_core::RoleAssignmentId>(3)?;
    let capabilities = [
        Capability::HistorySpaceRead,
        Capability::LayerRead,
        Capability::AssertionRead,
        Capability::FieldRead,
        Capability::MaskRead,
        Capability::ReplacementBoundaryRead,
        Capability::QueryResolve,
        Capability::QueryExplain,
        Capability::RawHistoryRead,
    ];
    let role = RoleDefinition::new(
        role_id,
        "benchmark_reader",
        PolicyBundle::from_grants(
            capabilities
                .into_iter()
                .map(|capability| CapabilityGrant::new(capability, GrantEffect::Allow)),
        )?,
    )?;
    let policy = SecurityPolicySnapshot::new(
        vec![Principal::new(principal_id)],
        vec![role],
        vec![RoleAssignment::new(
            assignment_id,
            principal_id,
            role_id,
            PolicyScope::project(),
        )],
        Vec::new(),
    )?;
    let policy_history = SecurityPolicyHistory::new(
        published,
        (0..=FINAL_REVISION)
            .map(|value| {
                Ok(SecurityPolicyVersion::new(
                    revision(value)?,
                    SecurityEpoch::INITIAL,
                    policy.clone(),
                ))
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
    )?;
    Ok((schema_history, policy_history, principal_id))
}

fn query_budget() -> Result<QueryBudget, Box<dyn Error>> {
    let limits = QueryBudgetLimits::new(100_000, 120_000, 100_000)?;
    Ok(QueryBudget::new(100_000, 120_000, 100_000, limits)?)
}

fn make_query_context(
    layers: &LayerSchemaSnapshot,
    binding: HistoricalQueryBinding,
    principal_id: PrincipalId,
    world_time: WorldTimeSelector,
    snapshot_id: SnapshotId,
) -> Result<QueryContext, Box<dyn Error>> {
    let published = revision(FINAL_REVISION)?;
    let recorded_as_of = RecordedAsOf::from_published_revision(published);
    let validated_layers = ValidatedLayerSelection::resolve(layers, LayerSelection::AllActive)?;
    Ok(QueryContext::new(QueryContextInput {
        snapshot: SnapshotRef::new(snapshot_id),
        snapshot_revision: published,
        recorded_as_of,
        history_space: id::<HistorySpaceId>(u64::from(SELECTED_HISTORY_SPACE))?,
        layers: validated_layers,
        world_time,
        perspective: PerspectiveScope::World,
        epistemic_mode: EpistemicMode::WorldState,
        schema_binding: binding,
        security: SecurityContext::new(principal_id, AuthorizationMode::Now),
        budget: query_budget()?,
        cancellation: worlddb_core::CancellationToken::new(),
    })?)
}

fn build_bench_data(corpus_dir: &Path) -> Result<BenchData, Box<dyn Error>> {
    let timeline = Timeline::new(id::<TimelineId>(1)?);
    let (space_definitions, parent_ids) = corpus_spaces(corpus_dir)?;
    let published = revision(FINAL_REVISION)?;
    let schema_revision = SchemaRevision::from_published_revision(published);
    let base_layer = id::<worlddb_core::LayerId>(0)?;
    let layer_definitions = (0_u64..8)
        .map(|number| {
            Ok(LayerDefinition::new(
                id(number)?,
                Symbol::new(format!("layer_{number}"))?,
                None,
                i32::try_from(number)?,
                Lifecycle::Active,
                schema_revision,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let layers = LayerSchemaSnapshot::new(schema_revision, layer_definitions, base_layer)?;
    let (schema_history, policies, principal_id) = build_schema_and_policy(&layers)?;
    let recorded_as_of = RecordedAsOf::from_published_revision(published);
    let schema_binding =
        HistoricalQueryBinding::bind(&schema_history, recorded_as_of, SchemaMode::Historical)?;

    let time_patterns = [
        (-100, 100),
        (0, 1),
        (100, 200),
        (-1_000, -900),
        (50, 60),
        (500, 5_000),
    ];
    let validities = time_patterns
        .map(|(start, end)| {
            Ok(AssertionValidity::new(TimeInterval::new(
                timeline,
                Some(WorldTime::from_nanoseconds(timeline, i128::from(start))),
                Some(WorldTime::from_nanoseconds(timeline, i128::from(end))),
            )?))
        })
        .into_iter()
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let root_definition = space_definitions
        .first()
        .copied()
        .ok_or_else(|| io::Error::other("root HistorySpace is missing"))?;
    let mut history = HistorySpaceReferenceModel::new(vec![root_definition])?;
    let mut records_by_space = std::iter::repeat_with(Vec::<AssertionHistoryRecord>::new)
        .take(HISTORY_SPACE_COUNT + 1)
        .collect::<Vec<_>>();
    let mut page_rows = Vec::new();
    let mut masks = Vec::new();
    let mut archive_visible_mask_ids = BTreeSet::new();
    let mut archive_targets = Vec::with_capacity(ASSERTION_COUNT);
    let mut expected_mask_pairs = Vec::<(u64, u64)>::new();
    let assertion_bytes = fs::read(corpus_dir.join("assertions.bin"))?;
    if assertion_bytes.len() != ASSERTION_COUNT * 48 {
        return Err(io::Error::other("assertions.bin has an unexpected size").into());
    }
    let mut mask_id = 0_u64;
    for row in assertion_bytes.chunks_exact(48) {
        let assertion_number = word_u64(row, 0)?;
        let entity_number = word_u32(row, 8)?;
        let predicate_number = word_u32(row, 12)?;
        let space_number = word_u16(row, 16)?;
        let layer_number = u64::from(
            *row.get(18)
                .ok_or_else(|| io::Error::other("truncated layer ID in M6-14a corpus"))?,
        );
        let polarity_byte = *row
            .get(19)
            .ok_or_else(|| io::Error::other("truncated polarity in M6-14a corpus"))?;
        let value_number = word_u32(row, 20)?;
        let valid_start = word_i64(row, 24)?;
        let valid_end = word_i64(row, 32)?;
        let flags = word_u32(row, 40)?;
        let reserved = word_u32(row, 44)?;
        if assertion_number == 0
            || assertion_number > ASSERTION_COUNT as u64
            || !(1..=HISTORY_SPACE_COUNT as u16).contains(&space_number)
            || layer_number >= 8
            || polarity_byte > 1
            || flags & !1 != 0
            || reserved != 0
        {
            return Err(io::Error::other("assertion row violates the M6-14a contract").into());
        }
        let validity_index = time_patterns
            .iter()
            .position(|(start, end)| *start == valid_start && *end == valid_end)
            .ok_or_else(|| io::Error::other("unknown assertion validity interval"))?;
        let assertion_id = id::<worlddb_core::AssertionId>(assertion_number)?;
        let entity_id = id::<worlddb_core::EntityId>(u64::from(entity_number))?;
        let predicate_id = id::<worlddb_core::PredicateId>(u64::from(predicate_number))?;
        let history_space_id = id::<HistorySpaceId>(u64::from(space_number))?;
        let layer_id = id::<worlddb_core::LayerId>(layer_number)?;
        let validity = validities
            .get(validity_index)
            .copied()
            .ok_or_else(|| io::Error::other("assertion validity index is out of range"))?;
        let created_revision = revision(u64::from(space_number))?;
        archive_targets.push(ArchiveTargetRecord::new(
            ArchiveTargetRef::Assertion(assertion_id),
            created_revision,
        ));
        let context = ContextKey::new(
            history_space_id,
            layer_id,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let assertion = Assertion::new(
            assertion_id,
            AssertionDraft::new(
                context,
                Subject::new(entity_id),
                predicate_id,
                Value::String(format!("v{value_number:05}")),
                if polarity_byte == 0 {
                    worlddb_core::Polarity::Positive
                } else {
                    worlddb_core::Polarity::Negative
                },
                validity,
            ),
            created_revision,
        );
        records_by_space
            .get_mut(usize::from(space_number))
            .ok_or_else(|| io::Error::other("assertion HistorySpace is out of range"))?
            .push(AssertionHistoryRecord::from_assertion(assertion));

        if is_selected_ancestry(space_number, &parent_ids) {
            page_rows.push(CorpusHistoryRow {
                assertion_id,
                history_space_id,
                layer_id,
            });
        }
        if flags & 1 != 0 {
            mask_id = mask_id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("mask ID overflow"))?;
            expected_mask_pairs.push((mask_id, assertion_number));
            if is_selected_ancestry(space_number, &parent_ids) {
                let mask_id_value = id::<MaskId>(mask_id)?;
                let mask_context = ContextKey::new(
                    history_space_id,
                    id::<worlddb_core::LayerId>(7)?,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState,
                )?;
                masks.push(Mask::new(
                    mask_id_value,
                    mask_context,
                    MaskSelector::ExactAssertion(assertion_id),
                    Some(validity),
                    created_revision,
                )?);
                archive_visible_mask_ids.insert(mask_id_value);
            }
        }
    }
    if page_rows.is_empty() {
        return Err(io::Error::other("selected history ancestry is empty").into());
    }
    page_rows.sort_unstable_by_key(|row| row.assertion_id);
    if page_rows
        .windows(2)
        .any(|pair| match (pair.first(), pair.get(1)) {
            (Some(previous), Some(next)) => previous.assertion_id >= next.assertion_id,
            _ => true,
        })
    {
        return Err(io::Error::other("page corpus has duplicate or unordered IDs").into());
    }
    if mask_id as usize != expected_mask_pairs.len() {
        return Err(io::Error::other("mask count does not match the assertion corpus").into());
    }
    let mask_bytes = fs::read(corpus_dir.join("masks.bin"))?;
    if mask_bytes.len() != expected_mask_pairs.len() * 16 {
        return Err(io::Error::other("masks.bin count differs from assertions.bin").into());
    }
    for (index, row) in mask_bytes.chunks_exact(16).enumerate() {
        let expected_id = u64::try_from(index + 1)?;
        let expected_pair = expected_mask_pairs
            .get(index)
            .ok_or_else(|| io::Error::other("mask reference is missing"))?;
        if word_u64(row, 0)? != expected_id
            || word_u64(row, 8)? != expected_pair.1
            || expected_pair.0 != expected_id
        {
            return Err(io::Error::other("mask records do not match assertion flags").into());
        }
    }

    for numeric_id in 1..=HISTORY_SPACE_COUNT {
        if numeric_id > 1 {
            let definition = space_definitions
                .get(numeric_id - 1)
                .copied()
                .ok_or_else(|| io::Error::other("HistorySpace definition is missing"))?;
            history.add_history_space(definition)?;
        }
        let definition = space_definitions
            .get(numeric_id - 1)
            .copied()
            .ok_or_else(|| io::Error::other("HistorySpace definition is missing"))?;
        let records = records_by_space
            .get_mut(numeric_id)
            .ok_or_else(|| io::Error::other("HistorySpace assertion batch is missing"))?;
        let actual = history.publish(definition.history_space_id(), std::mem::take(records))?;
        if actual != revision(u64::try_from(numeric_id)?)? {
            return Err(io::Error::other("normalized history revision mismatch").into());
        }
    }
    if page_rows.len() < 64_000 {
        return Err(io::Error::other(
            "selected HistorySpace does not reach the 1,000-page workload",
        )
        .into());
    }

    let archive = ArchiveHistoryReferenceModel::new(archive_targets, Vec::new())?;
    let point_index = AssertionPointHistoryIndex::build(&history)?;
    let point_metadata = IndexGenerationMetadata::new(
        IndexFamily::AssertionPointHistory,
        1,
        IndexSchemaVersion::V1_0,
        IndexFormatVersion::V1_0,
        IndexBuildVersion::new(1)?,
        IndexRevisionCoverage::new(Revision::GENESIS, published)?,
    )?;
    let point_context = make_query_context(
        &layers,
        schema_binding,
        principal_id,
        WorldTimeSelector::At(WorldTime::from_nanoseconds(timeline, 50)),
        id::<SnapshotId>(10)?,
    )?;
    let mut page_context = make_query_context(
        &layers,
        schema_binding,
        principal_id,
        WorldTimeSelector::AllTimes,
        id::<SnapshotId>(11)?,
    )?;
    let mut ancestry = Vec::new();
    for numeric_id in 1..=HISTORY_SPACE_COUNT {
        if is_selected_ancestry(u16::try_from(numeric_id)?, &parent_ids) {
            ancestry.push(
                space_definitions
                    .get(numeric_id - 1)
                    .copied()
                    .ok_or_else(|| io::Error::other("HistorySpace ancestor is missing"))?,
            );
        }
    }
    let history_space_view = HistorySpaceView::new(ancestry)?;
    let binding = SnapshotBinding::new(SnapshotBindingInput {
        database_id: id::<worlddb_core::DatabaseId>(12)?,
        snapshot_id: page_context.snapshot().id(),
        data_revision: published,
        recorded_as_of,
        schema: schema_binding,
        history_space: history_space_view,
        layer_schema: layers.clone(),
        layer_selection: LayerSelection::AllActive,
        security: SnapshotSecurityBinding::new(
            principal_id,
            AuthorizationMode::Now,
            SecurityEpoch::INITIAL,
        ),
        backend_generation: 1,
    })?;
    let limits = SnapshotLifetimeLimits::new(60_000, 120_000, 600_000)?;
    let registry = SnapshotRegistry::new(limits);
    let lease = registry.pin(binding, SnapshotPinPurpose::Interactive, 0)?;
    page_context = QueryContext::new_leased(
        QueryContextInput {
            snapshot: page_context.snapshot(),
            snapshot_revision: page_context.snapshot_revision(),
            recorded_as_of: page_context.recorded_as_of(),
            history_space: page_context.history_space(),
            layers: page_context.layers().clone(),
            world_time: page_context.world_time(),
            perspective: page_context.perspective(),
            epistemic_mode: page_context.epistemic_mode(),
            schema_binding: page_context.schema_binding(),
            security: page_context.security(),
            budget: page_context.budget(),
            cancellation: page_context.cancellation().clone(),
        },
        lease,
        0,
    )?;
    let query_predicate_id = id::<worlddb_core::PredicateId>(1)?;
    let predicate = schema_history
        .schema_at(SchemaMode::Historical, recorded_as_of.revision())?
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::Predicate(predicate)
                if predicate.predicate_id() == query_predicate_id =>
            {
                Some(predicate.clone())
            }
            _ => None,
        })
        .ok_or_else(|| io::Error::other("point-query predicate is missing"))?;

    Ok(BenchData {
        history,
        archive,
        layers,
        policies,
        point_context,
        page_context,
        predicate,
        point_index,
        point_metadata,
        masks,
        archive_visible_mask_ids,
        page_rows,
    })
}

fn query_point(data: &BenchData) -> Result<(), Box<dyn Error>> {
    static NO_BOUNDARIES: std::sync::OnceLock<BTreeSet<worlddb_core::ReplacementBoundaryId>> =
        std::sync::OnceLock::new();
    let boundary_ids = NO_BOUNDARIES.get_or_init(BTreeSet::new);
    let subject = Subject::new(id::<worlddb_core::EntityId>(1)?);
    let predicate_id = id::<worlddb_core::PredicateId>(1)?;
    let slot = MultiValueSlot::new(subject, predicate_id);
    let masks =
        AuthorizedAssertionMaskHistory::new(&data.masks, &[], &[], &data.archive_visible_mask_ids);
    let boundaries = worlddb_core::ReplacementBoundarySource::new(&[], &[], &[], boundary_ids);
    let store =
        AssertionQueryStore::new(&data.history, &data.archive, &data.layers, &data.policies);
    let request = AssertionPointRequest::new(
        &data.point_context,
        masks,
        boundaries,
        AssertionPointIndexAccess::new(
            worlddb_core::IndexAvailability::Available(data.point_metadata),
            Some(&data.point_index),
        ),
        FullScanBudget::Available,
    );
    let result =
        ProductiveQueryEngine::resolved_point(store, request, slot, &data.predicate, |_, _| {
            Ok(false)
        })?;
    if result.path() != (QueryExecutionPath::Indexed { generation_id: 1 }) {
        return Err(io::Error::other("point benchmark fell back from its index").into());
    }
    Ok(())
}

fn evict_cpu_cache(buffer: &mut [u8], sequence: u64) {
    for (ordinal, byte) in buffer.iter_mut().step_by(64).enumerate() {
        *byte = byte.wrapping_add((ordinal as u8).wrapping_add(sequence as u8));
    }
    std::hint::black_box(buffer.get(buffer.len().saturating_sub(1)));
}

fn point_samples(data: &BenchData, output: &mut String) -> Result<(), Box<dyn Error>> {
    for _ in 0..5 {
        query_point(data)?;
    }
    let mut eviction = vec![0_u8; CPU_CACHE_EVICTION_BYTES];
    for operation in ["point_resolution_warm", "point_resolution_cold_cpu_cache"] {
        for iteration in 0..SAMPLE_PASSES {
            if operation.ends_with("cold_cpu_cache") {
                evict_cpu_cache(&mut eviction, iteration as u64);
            }
            let start = Instant::now();
            query_point(data)?;
            let elapsed_ns = start.elapsed().as_nanos();
            output.push_str(&format!("{operation},{iteration},0,1,{elapsed_ns}\n"));
        }
    }
    Ok(())
}

fn run_page_pass(
    data: &BenchData,
    pass: usize,
    cold: bool,
    eviction: &mut [u8],
    output: &mut String,
) -> Result<usize, Box<dyn Error>> {
    let mut cursors = CursorStateStore::new(CursorStoreLimits::new(8, 4_096, 60_000)?)?;
    let query_hash = QueryHash::new([0x14; 32]);
    let mut cursor = None;
    let mut page_number = 0_usize;
    let mut total_rows = 0_usize;
    loop {
        if cold {
            evict_cpu_cache(eviction, ((pass as u64) << 32) | page_number as u64);
        }
        let request = PageRequest::new(PAGE_SIZE, cursor)?;
        let execution = PageExecution::new(
            &mut cursors,
            query_hash,
            PageOperation::RawHistory,
            1,
            60_000,
            PAGE_SIZE,
            QueryExecutionPath::FullScan,
        )?;
        let source_rows = &data.page_rows;
        let start = Instant::now();
        let page = ProductiveQueryEngine::stream_page(
            request,
            execution,
            &data.page_context,
            &data.policies,
            move |after| {
                let offset = match after {
                    None => 0,
                    Some(key) if key.len() == 16 => source_rows
                        .partition_point(|row| row.assertion_id.to_bytes().as_slice() <= key),
                    Some(_) => {
                        return Err(worlddb_core::QueryEngineError::InvalidPageRequest);
                    }
                };
                let remaining = source_rows
                    .get(offset..)
                    .ok_or(worlddb_core::QueryEngineError::InvalidPageRequest)?;
                Ok(remaining.iter().copied().map(Ok))
            },
            |row: &CorpusHistoryRow| row.assertion_id.to_bytes().to_vec(),
            |row, policy, context| {
                let record = worlddb_core::RecordRef::Assertion(row.assertion_id);
                policy.authorize(
                    context.security().principal_id(),
                    Capability::AssertionRead,
                    PolicyTarget::new(
                        Some(row.history_space_id),
                        Some(row.layer_id),
                        Some(record),
                        None,
                        None,
                    ),
                ) == AuthorizationDecision::Allow
            },
        )?;
        let elapsed_ns = start.elapsed().as_nanos();
        page_number += 1;
        total_rows += page.results().len();
        output.push_str(&format!(
            "{},{},{},{},{}\n",
            if cold {
                "history_page_cold_cpu_cache"
            } else {
                "history_page_warm"
            },
            pass,
            page_number,
            page.results().len(),
            elapsed_ns
        ));
        cursor = page.next_cursor().map(<[u8]>::to_vec);
        if cursor.is_none() {
            break;
        }
    }
    if !cursors.is_empty() {
        return Err(io::Error::other("page cursor leaked after the final page").into());
    }
    if total_rows != data.page_rows.len() {
        return Err(io::Error::other("paged history omitted or duplicated corpus rows").into());
    }
    Ok(page_number)
}

#[test]
#[ignore = "manual full-corpus M6-14b Windows performance probe"]
fn m6_14b_windows_full_corpus_query_and_history_probe() -> Result<(), Box<dyn Error>> {
    if !cfg!(windows) {
        return Err(io::Error::other("this M6-14b probe requires Windows").into());
    }
    let corpus_dir = PathBuf::from(
        std::env::var_os("WORLDDB_M6_14A_CORPUS_DIR")
            .ok_or_else(|| io::Error::other("WORLDDB_M6_14A_CORPUS_DIR is not set"))?,
    );
    let output_dir = PathBuf::from(
        std::env::var_os("WORLDDB_M6_14B_OUTPUT_DIR")
            .ok_or_else(|| io::Error::other("WORLDDB_M6_14B_OUTPUT_DIR is not set"))?,
    );
    if !output_dir.is_dir() {
        return Err(io::Error::other("M6-14b output directory does not exist").into());
    }
    let started = Instant::now();
    let data = build_bench_data(&corpus_dir)?;
    let setup_seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "M6-14b loaded {} assertions, {} selected-history rows, {} ancestry masks; setup/index {:.3}s",
        ASSERTION_COUNT,
        data.page_rows.len(),
        data.masks.len(),
        setup_seconds
    );
    let page_count = data.page_rows.len().div_ceil(PAGE_SIZE as usize);
    if page_count < 1_000 {
        return Err(io::Error::other("page count fell below the 1,000-page requirement").into());
    }
    let mut samples = String::from("operation,iteration,page,result_count,elapsed_ns\n");
    point_samples(&data, &mut samples)?;
    let mut eviction = vec![0_u8; CPU_CACHE_EVICTION_BYTES];
    for pass in 0..SAMPLE_PASSES {
        let observed = run_page_pass(&data, pass, false, &mut eviction, &mut samples)?;
        if observed != page_count {
            return Err(io::Error::other("warm history pass page count changed").into());
        }
    }
    for pass in 0..SAMPLE_PASSES {
        let observed = run_page_pass(&data, pass, true, &mut eviction, &mut samples)?;
        if observed != page_count {
            return Err(io::Error::other("cold history pass page count changed").into());
        }
    }
    let path = output_dir.join("windows-query-history-probe.csv");
    fs::write(&path, samples)?;
    println!(
        "assertions={ASSERTION_COUNT} selected_history_rows={} page_size={PAGE_SIZE} pages={page_count} repeats={SAMPLE_PASSES} raw_csv={}",
        data.page_rows.len(),
        path.display()
    );
    Ok(())
}
