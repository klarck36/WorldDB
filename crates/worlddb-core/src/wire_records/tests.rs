//! Record codec conformance tests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{
    Record, RecordKind, decode_record, decode_record_batch_with_limits, decode_record_with_limits,
    encode_decoded_record, encode_record, encode_record_with_flags,
};
use crate::{
    ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition, Assertion, AssertionDraft,
    AssertionRetraction, AssertionValidity, AssertionValidityClosure, Bytes, CalendarPeriod,
    Cardinality, ConstraintSet, ContextKey, DecimalFieldMetadata, DecodeResource, DecoderLimits,
    DomainId, Duration, Entity, EntityRetirement, EntityTypeConstraint, EntityTypeDefinition,
    EpistemicMode, Event, EventAttributeDefinition, EventAttributeValue, EventDraft,
    EventKindDefinition, EventMask, EventMaskRetraction, EventParticipant, EventRelation,
    EventRelationInputKind, EventRelationRetraction, EventRetraction, EventRoleDefinition,
    EventSpanClosure, EventTimeConstraint, EventTimeForm, Evidence, EvidenceRelation,
    EvidenceRetraction, EvidenceTargetRef, FrameHeader, HistorySpaceContentRef,
    HistorySpaceDefinition, Int, JobBudget, LayerDefinition, LayerSchemaSnapshot, Lifecycle, Mask,
    MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure,
    MigrationCalendarDirection, MigrationCalendarShift, MigrationCategory, MigrationPlan,
    MigrationPlanSpec, MigrationRun, MigrationRunState, MigrationStepCommitIdentity,
    MigrationStepTargetSchema, MigrationTargetSchema, MigrationTransformerVersion,
    PerspectiveDefinitionRevision, PerspectiveRetirement, PerspectiveScope, Polarity,
    PredicateDefinition, PredicateDefinitionSpec, PropositionKey, ProvenanceEdge,
    ProvenanceEndpointRef, ProvenanceRelation, ProvenanceRetraction, RecordCodecError, RecordRef,
    ReplacementBoundary, ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure,
    ResolutionPolicy, Revision, RoleCardinality, SchemaDefinitionId, SchemaIdentityTransition,
    SchemaRevision, Source, SourceContentDigest, SourceLocator, SourceMetadata,
    SourceMetadataEntry, SourceSchemaPrecondition, Subject, Symbol, Time, TimeInterval, Timeline,
    TimelineId, TlvDecoder, TlvEncoder, TransferLineage, TransferLineageId, Value, ValueConstraint,
    ValueKind, WorldTime, decode_frame, encode_frame, encode_id,
};

fn id<T: DomainId>(tail: u8) -> Option<T> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).ok()
}

fn symbol(text: &str) -> Option<Symbol> {
    Symbol::new(text).ok()
}

fn migration_plan_frame_with_field_replaced(field_tag: u32, value: &[u8]) -> Option<Vec<u8>> {
    let fixture_name = if field_tag == 14 {
        "migration_plan_step_targets"
    } else {
        "migration_plan_calendar_shift"
    };
    let record = fixtures()?
        .into_iter()
        .find(|(name, _)| *name == fixture_name)?
        .1;
    let encoded = encode_record(&record).ok()?;
    let frame = decode_frame(&encoded).ok()?;
    let mut decoder = TlvDecoder::new(frame.payload());
    let mut encoder = TlvEncoder::new();
    let mut replaced = false;
    while let Some(field) = decoder.next_field().ok()? {
        let field_value = if field.tag() == field_tag {
            replaced = true;
            value
        } else {
            field.value()
        };
        encoder.push(field.tag(), field_value).ok()?;
    }
    if !replaced {
        return None;
    }
    encode_frame(frame.header(), &encoder.finish()).ok()
}

fn fixtures() -> Option<Vec<(&'static str, Record)>> {
    let revision = Revision::GENESIS;
    let schema_revision = SchemaRevision::from_published_revision(revision);
    let history_id = id(1)?;
    let layer_id = id(2)?;
    let migration_id = id(3)?;
    let migration_timeline_id = id(16)?;
    let run_id = id(4)?;
    let step_id = id(5)?;
    let operation_id = id(6)?;
    let predicate_id = id(7)?;
    let entity_type = id(8)?;
    let event_kind_id = id(9)?;
    let role_id = id(10)?;
    let attribute_id = id(11)?;
    let entity_id = id(12)?;
    let perspective_id = id(13)?;
    let retirement_id = id(14)?;
    let perspective_retirement_id = id(15)?;

    let history = HistorySpaceDefinition::new(history_id, None, revision).ok()?;
    let layer = LayerDefinition::new(
        layer_id,
        symbol("base")?,
        Some(String::from("Base layer")),
        0,
        Lifecycle::Active,
        schema_revision,
    );
    let layer_snapshot =
        LayerSchemaSnapshot::new(schema_revision, vec![layer.clone()], layer_id).ok()?;
    let bool_set = crate::NonEmptySet::new(vec![true, false]).ok()?;
    let predicate_constraints =
        ConstraintSet::new(vec![ValueConstraint::BoolSet(bool_set)]).ok()?;
    let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
        predicate_id,
        symbol: symbol("enabled")?,
        subject_constraint: EntityTypeConstraint::AnyEntity,
        value_kind: ValueKind::Bool,
        object_constraint: None,
        cardinality: Cardinality::Single,
        resolution_policy: ResolutionPolicy::SingleValueReplace,
        constraints: predicate_constraints,
        decimal_metadata: None,
        lifecycle: Lifecycle::Active,
        created_revision: revision,
    })
    .ok()?;
    let role = EventRoleDefinition::new(
        role_id,
        symbol("actor")?,
        EntityTypeConstraint::Exact(entity_type),
        RoleCardinality::new(1, Some(1)).ok()?,
    );
    let decimal_range = crate::InclusiveRange::new(
        Some(crate::Decimal::new(false, 125, 2).ok()?),
        Some(crate::Decimal::new(false, 5, 0).ok()?),
    )
    .ok()?;
    let attribute_constraints =
        ConstraintSet::new(vec![ValueConstraint::DecimalRange(decimal_range)]).ok()?;
    let attribute = EventAttributeDefinition::new(
        attribute_id,
        symbol("approved")?,
        ValueKind::Decimal,
        None,
        attribute_constraints,
        Some(DecimalFieldMetadata::new(Some(2), None, Some(2)).ok()?),
        true,
    )
    .ok()?;
    let event_kind = EventKindDefinition::new(
        event_kind_id,
        symbol("approval")?,
        vec![role],
        vec![attribute],
        EventTimeConstraint::new(
            EventTimeForm::InstantOrSpan,
            Some(CalendarPeriod::new(1, 2, 3).ok()?),
        )
        .ok()?,
        Lifecycle::Active,
        revision,
    )
    .ok()?;
    let schema_change = SchemaIdentityTransition::new(
        None,
        Some(SchemaDefinitionId::Predicate(predicate_id)),
        MigrationCategory::Additive,
    )
    .ok()?;
    let migration_plan_spec = MigrationPlanSpec {
        migration_id,
        category: MigrationCategory::Additive,
        source_schema: SourceSchemaPrecondition::new(schema_revision, [0x11; 32]),
        target_schema: MigrationTargetSchema::new(
            SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            [0x22; 32],
        ),
        steps: vec![step_id],
        step_targets: None,
        schema_changes: vec![schema_change],
        transformer_version: MigrationTransformerVersion::new(1).ok()?,
        calendar_shift: None,
        budget: JobBudget::new(1_000, 1024 * 1024).ok()?,
    };
    let migration_plan = MigrationPlan::new(migration_plan_spec.clone()).ok()?;
    let migration_plan_calendar_shift = MigrationPlan::new(MigrationPlanSpec {
        calendar_shift: Some(MigrationCalendarShift::new(
            migration_timeline_id,
            42,
            CalendarPeriod::new(1, 2, 3).ok()?,
            MigrationCalendarDirection::Future,
        )),
        ..migration_plan_spec.clone()
    })
    .ok()?;
    let migration_plan_step_targets = MigrationPlan::new(MigrationPlanSpec {
        step_targets: Some(vec![MigrationStepTargetSchema::new(
            step_id,
            MigrationTargetSchema::new(
                SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
                [0x22; 32],
            ),
        )]),
        ..migration_plan_spec
    })
    .ok()?;
    let perspective_definition = PerspectiveDefinitionRevision::new(
        perspective_id,
        Some(String::from("Research")),
        Some(String::from("Curated perspective")),
        revision,
    )
    .ok()?;

    let mut records = vec![
        (
            "history_space_root",
            Record::HistorySpaceDefinition(history),
        ),
        (
            "entity",
            Record::Entity(Entity::new(entity_id, entity_type, revision)),
        ),
        (
            "entity_retirement",
            Record::EntityRetirement(EntityRetirement::new(retirement_id, entity_id, revision)),
        ),
        (
            "perspective_definition",
            Record::PerspectiveDefinitionRevision(perspective_definition),
        ),
        (
            "perspective_retirement",
            Record::PerspectiveRetirement(PerspectiveRetirement::new(
                perspective_retirement_id,
                perspective_id,
                revision,
            )),
        ),
        ("layer_definition", Record::LayerDefinition(layer)),
        (
            "layer_schema_snapshot",
            Record::LayerSchemaSnapshot(layer_snapshot),
        ),
        (
            "entity_type_definition",
            Record::EntityTypeDefinition(EntityTypeDefinition::new(
                entity_type,
                symbol("person")?,
                Some(String::from("A person")),
                Lifecycle::Active,
                revision,
            )),
        ),
        (
            "predicate_definition",
            Record::PredicateDefinition(predicate),
        ),
        (
            "event_kind_definition",
            Record::EventKindDefinition(event_kind),
        ),
        ("migration_plan", Record::MigrationPlan(migration_plan)),
        (
            "migration_plan_calendar_shift",
            Record::MigrationPlan(migration_plan_calendar_shift),
        ),
        (
            "migration_plan_step_targets",
            Record::MigrationPlan(migration_plan_step_targets),
        ),
        (
            "migration_run",
            Record::MigrationRun(MigrationRun::new(
                run_id,
                migration_id,
                MigrationRunState::Running,
            )),
        ),
        (
            "migration_step_commit",
            Record::MigrationStepCommitIdentity(MigrationStepCommitIdentity::new(
                migration_id,
                run_id,
                step_id,
                operation_id,
            )),
        ),
        (
            "migration_step_commit_fingerprinted",
            Record::MigrationStepCommitIdentity(
                MigrationStepCommitIdentity::with_input_fingerprint(
                    migration_id,
                    run_id,
                    step_id,
                    operation_id,
                    [0x55; 32],
                ),
            ),
        ),
        (
            "migration_step_commit_guarded",
            Record::MigrationStepCommitIdentity(
                MigrationStepCommitIdentity::with_plan_and_input_fingerprint(
                    migration_id,
                    run_id,
                    step_id,
                    operation_id,
                    [0x55; 32],
                    [0x66; 32],
                    [0x77; 32],
                ),
            ),
        ),
    ];
    records.extend(lifecycle_fixtures()?);
    records.extend(event_fixtures()?);
    records.extend(source_provenance_fixtures()?);
    Some(records)
}

fn source_provenance_fixtures() -> Option<Vec<(&'static str, Record)>> {
    let first = Revision::GENESIS;
    let second = Revision::FIRST_COMMIT;
    let source = Source::new(
        id(70)?,
        symbol("archive")?,
        Some(SourceLocator::new("https://example.invalid/archive/70").ok()?),
        Some(SourceContentDigest::new(Bytes::new(vec![0xde, 0xad, 0xbe, 0xef])).ok()?),
        SourceMetadata::new(vec![
            SourceMetadataEntry::new(symbol("origin")?, Value::String(String::from("imported"))),
            SourceMetadataEntry::new(symbol("page")?, Value::UInt(crate::UInt::new(12))),
        ])
        .ok()?,
        first,
    );
    let evidence = Evidence::new(
        id(71)?,
        source.id(),
        EvidenceTargetRef::Assertion(id(26)?),
        EvidenceRelation::Supports,
        first,
    );
    let provenance = ProvenanceEdge::new(
        id(72)?,
        ProvenanceEndpointRef::Source(source.id()),
        ProvenanceEndpointRef::Assertion(id(26)?),
        ProvenanceRelation::DerivedFrom,
        first,
    )
    .ok()?;
    let evidence_retraction =
        EvidenceRetraction::new(id(73)?, &evidence, "source replaced", second).ok()?;
    let provenance_retraction =
        ProvenanceRetraction::new(id(74)?, &provenance, "lineage corrected", second).ok()?;
    let transfer_lineage = TransferLineage::new(
        id::<TransferLineageId>(75)?,
        id(76)?,
        id(77)?,
        HistorySpaceContentRef::Mask(id(78)?),
        HistorySpaceContentRef::Mask(id(79)?),
        first,
    )
    .ok()?;
    Some(vec![
        ("source_metadata", Record::Source(source)),
        ("evidence_assertion_supports", Record::Evidence(evidence)),
        (
            "provenance_source_to_assertion",
            Record::Provenance(provenance),
        ),
        (
            "evidence_retraction",
            Record::EvidenceRetraction(evidence_retraction),
        ),
        (
            "provenance_retraction",
            Record::ProvenanceRetraction(provenance_retraction),
        ),
        (
            "transfer_lineage",
            Record::TransferLineage(transfer_lineage),
        ),
    ])
}

fn event_fixtures() -> Option<Vec<(&'static str, Record)>> {
    let first = Revision::GENESIS;
    let second = Revision::FIRST_COMMIT;
    let history_id = id(1)?;
    let layer_id = id(2)?;
    let entity_id = id(12)?;
    let entity_type_id = id(8)?;
    let event_kind_id = id(43)?;
    let role_id = id(44)?;
    let attribute_id = id(45)?;
    let timeline = Timeline::new(id(25)?);
    let start = WorldTime::from_nanoseconds(timeline, 10);
    let end = WorldTime::from_nanoseconds(timeline, 20);
    let close_at = WorldTime::from_nanoseconds(timeline, 15);
    let role = EventRoleDefinition::new(
        role_id,
        symbol("actor")?,
        EntityTypeConstraint::Exact(entity_type_id),
        RoleCardinality::new(1, Some(1)).ok()?,
    );
    let attribute = EventAttributeDefinition::new(
        attribute_id,
        symbol("count")?,
        ValueKind::Int,
        None,
        ConstraintSet::unconstrained(),
        None,
        true,
    )
    .ok()?;
    let event_kind = EventKindDefinition::new(
        event_kind_id,
        symbol("activity")?,
        vec![role.clone()],
        vec![attribute.clone()],
        EventTimeConstraint::new(EventTimeForm::InstantOrSpan, None).ok()?,
        Lifecycle::Active,
        first,
    )
    .ok()?;
    let open_event_kind = EventKindDefinition::new(
        id(46)?,
        symbol("open_activity")?,
        vec![role],
        vec![attribute],
        EventTimeConstraint::new(EventTimeForm::OpenSpanAllowed, None).ok()?,
        Lifecycle::Active,
        first,
    )
    .ok()?;
    let participants = vec![EventParticipant::new(role_id, entity_id)];
    let attributes = vec![EventAttributeValue::new(
        attribute_id,
        Value::Int(Int::new(7)),
    )];
    let instant = Event::new(
        id(50)?,
        EventDraft::new(
            history_id,
            layer_id,
            &event_kind,
            participants.clone(),
            attributes.clone(),
            crate::EventTime::Instant(start),
        )
        .ok()?,
        first,
    )
    .ok()?;
    let closed_span = Event::new(
        id(51)?,
        EventDraft::new(
            history_id,
            layer_id,
            &event_kind,
            participants.clone(),
            attributes.clone(),
            crate::EventTime::span(start, Some(end)).ok()?,
        )
        .ok()?,
        first,
    )
    .ok()?;
    let open_span = Event::new(
        id(52)?,
        EventDraft::new(
            history_id,
            layer_id,
            &open_event_kind,
            participants,
            attributes,
            crate::EventTime::Span { start, end: None },
        )
        .ok()?,
        first,
    )
    .ok()?;
    let event_mask = EventMask::new(id(55)?, history_id, layer_id, instant.id(), first);
    let span_closure = EventSpanClosure::new(id(53)?, &open_span, close_at, second).ok()?;
    let event_retraction =
        EventRetraction::new(id(54)?, &instant, "source correction", second).ok()?;
    let event_mask_retraction =
        EventMaskRetraction::new(id(56)?, &event_mask, "mask superseded", second).ok()?;
    let before = EventRelation::new(
        id(60)?,
        instant.id(),
        closed_span.id(),
        EventRelationInputKind::Before,
        first,
    )
    .ok()?;
    let after_input = EventRelation::new(
        id(61)?,
        closed_span.id(),
        instant.id(),
        EventRelationInputKind::After,
        first,
    )
    .ok()?;
    let same_time = EventRelation::new(
        id(62)?,
        closed_span.id(),
        instant.id(),
        EventRelationInputKind::SameTime,
        first,
    )
    .ok()?;
    let causes = EventRelation::new(
        id(63)?,
        instant.id(),
        closed_span.id(),
        EventRelationInputKind::Causes,
        first,
    )
    .ok()?;
    let relation_retraction =
        EventRelationRetraction::new(id(64)?, &before, "relation superseded", second).ok()?;

    Some(vec![
        ("event_instant", Record::Event(instant)),
        ("event_closed_span", Record::Event(closed_span)),
        ("event_open_span", Record::Event(open_span)),
        ("event_span_closure", Record::EventSpanClosure(span_closure)),
        (
            "event_retraction",
            Record::EventRetraction(event_retraction),
        ),
        ("event_mask", Record::EventMask(event_mask)),
        (
            "event_mask_retraction",
            Record::EventMaskRetraction(event_mask_retraction),
        ),
        ("event_relation_before", Record::EventRelation(before)),
        (
            "event_relation_after_input",
            Record::EventRelation(after_input),
        ),
        ("event_relation_same_time", Record::EventRelation(same_time)),
        ("event_relation_causes", Record::EventRelation(causes)),
        (
            "event_relation_retraction",
            Record::EventRelationRetraction(relation_retraction),
        ),
    ])
}

fn lifecycle_fixtures() -> Option<Vec<(&'static str, Record)>> {
    let first = Revision::GENESIS;
    let second = Revision::FIRST_COMMIT;
    let third = Revision::new(2).ok()?;
    let history_id = id(1)?;
    let layer_id = id(2)?;
    let entity_id = id(12)?;
    let predicate_id = id(7)?;
    let perspective_id = id(13)?;
    let timeline_id = id(25)?;
    let context = ContextKey::new(
        history_id,
        layer_id,
        PerspectiveScope::World,
        EpistemicMode::WorldState,
    )
    .ok()?;
    let timeline = Timeline::new(timeline_id);
    let start = WorldTime::from_nanoseconds(timeline, 10);
    let close_at = WorldTime::from_nanoseconds(timeline, 15);
    let end = WorldTime::from_nanoseconds(timeline, 20);
    let validity =
        AssertionValidity::new(TimeInterval::new(timeline, Some(start), Some(end)).ok()?);
    let subject = Subject::new(entity_id);
    let assertion = Assertion::new(
        id(26)?,
        AssertionDraft::new(
            context,
            subject,
            predicate_id,
            Value::Bool(true),
            Polarity::Positive,
            validity,
        ),
        first,
    );
    let negative_assertion = Assertion::new(
        id(27)?,
        AssertionDraft::new(
            context,
            subject,
            predicate_id,
            Value::Bool(false),
            Polarity::Negative,
            validity,
        ),
        first,
    );
    let perspective_context = ContextKey::new(
        history_id,
        layer_id,
        PerspectiveScope::Perspective(perspective_id),
        EpistemicMode::Claims,
    )
    .ok()?;
    let perspective_assertion = Assertion::new(
        id(41)?,
        AssertionDraft::new(
            perspective_context,
            subject,
            predicate_id,
            Value::Bool(true),
            Polarity::Positive,
            validity,
        ),
        first,
    );
    let assertion_closure =
        AssertionValidityClosure::new(id(28)?, &assertion, close_at, second).ok()?;
    let assertion_retraction =
        AssertionRetraction::new(id(29)?, &assertion, "corrected source", second).ok()?;

    let exact_mask = Mask::new(
        id(30)?,
        context,
        MaskSelector::ExactAssertion(assertion.id()),
        Some(validity),
        first,
    )
    .ok()?;
    let proposition_mask = Mask::new(
        id(31)?,
        context,
        MaskSelector::Proposition(PropositionKey::new(
            subject,
            predicate_id,
            Value::Bool(false),
            Polarity::Negative,
        )),
        None,
        first,
    )
    .ok()?;
    let slot = MaskSlotSelector::new(
        subject,
        predicate_id,
        PerspectiveScope::World,
        EpistemicMode::WorldState,
    )
    .ok()?;
    let slot_mask = Mask::new(id(32)?, context, MaskSelector::Slot(slot), None, first).ok()?;
    let perspective_slot = MaskSlotSelector::new(
        subject,
        predicate_id,
        PerspectiveScope::Perspective(perspective_id),
        EpistemicMode::Claims,
    )
    .ok()?;
    let perspective_slot_mask = Mask::new(
        id(42)?,
        perspective_context,
        MaskSelector::Slot(perspective_slot),
        None,
        first,
    )
    .ok()?;
    let mask_closure = MaskValidityClosure::new(id(33)?, &exact_mask, close_at, second).ok()?;
    let mask_retraction =
        MaskRetraction::new(id(34)?, &exact_mask, "duplicate mask", second).ok()?;

    let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
        predicate_id,
        symbol: symbol("multi_value")?,
        subject_constraint: EntityTypeConstraint::AnyEntity,
        value_kind: ValueKind::Bool,
        object_constraint: None,
        cardinality: Cardinality::Multi,
        resolution_policy: ResolutionPolicy::MultiValueReplace,
        constraints: ConstraintSet::unconstrained(),
        decimal_metadata: None,
        lifecycle: Lifecycle::Active,
        created_revision: first,
    })
    .ok()?;
    let boundary =
        ReplacementBoundary::new(id(35)?, context, subject, &predicate, Some(validity), first)
            .ok()?;
    let open_boundary =
        ReplacementBoundary::new(id(36)?, context, subject, &predicate, None, first).ok()?;
    let boundary_closure =
        ReplacementBoundaryValidityClosure::new(id(37)?, &boundary, close_at, second).ok()?;
    let boundary_retraction =
        ReplacementBoundaryRetraction::new(id(38)?, &boundary, "replacement superseded", second)
            .ok()?;
    let archive = ArchiveTransition::new(
        id(39)?,
        ArchiveTargetRef::Assertion(assertion.id()),
        ArchiveAction::Archive,
        ArchiveState::Unarchived,
        third,
    )
    .ok()?;
    let unarchive = ArchiveTransition::new(
        id(40)?,
        ArchiveTargetRef::Mask(exact_mask.id()),
        ArchiveAction::Unarchive,
        ArchiveState::Archived,
        third,
    )
    .ok()?;

    Some(vec![
        ("assertion_positive", Record::Assertion(assertion)),
        ("assertion_negative", Record::Assertion(negative_assertion)),
        (
            "assertion_perspective_claim",
            Record::Assertion(perspective_assertion),
        ),
        (
            "assertion_validity_closure",
            Record::AssertionValidityClosure(assertion_closure),
        ),
        (
            "assertion_retraction",
            Record::AssertionRetraction(assertion_retraction),
        ),
        ("mask_exact_assertion", Record::Mask(exact_mask)),
        ("mask_proposition", Record::Mask(proposition_mask)),
        ("mask_slot", Record::Mask(slot_mask)),
        ("mask_perspective_slot", Record::Mask(perspective_slot_mask)),
        (
            "mask_validity_closure",
            Record::MaskValidityClosure(mask_closure),
        ),
        ("mask_retraction", Record::MaskRetraction(mask_retraction)),
        (
            "replacement_boundary_bounded",
            Record::ReplacementBoundary(boundary),
        ),
        (
            "replacement_boundary_open",
            Record::ReplacementBoundary(open_boundary),
        ),
        (
            "replacement_boundary_validity_closure",
            Record::ReplacementBoundaryValidityClosure(boundary_closure),
        ),
        (
            "replacement_boundary_retraction",
            Record::ReplacementBoundaryRetraction(boundary_retraction),
        ),
        (
            "archive_transition_archive",
            Record::ArchiveTransition(archive),
        ),
        (
            "archive_transition_unarchive",
            Record::ArchiveTransition(unarchive),
        ),
    ])
}

fn record_ref_fixtures() -> Option<Vec<(&'static str, RecordRef)>> {
    Some(vec![
        ("assertion", RecordRef::Assertion(id(1)?)),
        ("mask", RecordRef::Mask(id(2)?)),
        (
            "replacement_boundary",
            RecordRef::ReplacementBoundary(id(3)?),
        ),
        ("event", RecordRef::Event(id(4)?)),
        ("event_mask", RecordRef::EventMask(id(5)?)),
        ("event_relation", RecordRef::EventRelation(id(6)?)),
        ("source", RecordRef::Source(id(7)?)),
        ("evidence", RecordRef::Evidence(id(8)?)),
        ("provenance", RecordRef::Provenance(id(9)?)),
        (
            "assertion_validity_closure",
            RecordRef::AssertionValidityClosure(id(10)?),
        ),
        (
            "assertion_retraction",
            RecordRef::AssertionRetraction(id(11)?),
        ),
        (
            "mask_validity_closure",
            RecordRef::MaskValidityClosure(id(12)?),
        ),
        ("mask_retraction", RecordRef::MaskRetraction(id(13)?)),
        (
            "replacement_boundary_validity_closure",
            RecordRef::ReplacementBoundaryValidityClosure(id(14)?),
        ),
        (
            "replacement_boundary_retraction",
            RecordRef::ReplacementBoundaryRetraction(id(15)?),
        ),
        ("event_span_closure", RecordRef::EventSpanClosure(id(16)?)),
        ("event_retraction", RecordRef::EventRetraction(id(17)?)),
        (
            "event_mask_retraction",
            RecordRef::EventMaskRetraction(id(18)?),
        ),
        (
            "event_relation_retraction",
            RecordRef::EventRelationRetraction(id(19)?),
        ),
        (
            "evidence_retraction",
            RecordRef::EvidenceRetraction(id(20)?),
        ),
        (
            "provenance_retraction",
            RecordRef::ProvenanceRetraction(id(21)?),
        ),
        ("entity_retirement", RecordRef::EntityRetirement(id(22)?)),
        (
            "perspective_retirement",
            RecordRef::PerspectiveRetirement(id(23)?),
        ),
        ("archive_transition", RecordRef::ArchiveTransition(id(24)?)),
        ("transfer_lineage", RecordRef::TransferLineage(id(25)?)),
    ])
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn unhex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            Some((hex_nibble(pair.first().copied()?)? << 4) | hex_nibble(pair.get(1).copied()?)?)
        })
        .collect()
}

#[test]
fn all_implemented_records_match_fixed_golden_frames_and_round_trip() {
    let mut golden = BTreeMap::new();
    for (line_number, line) in include_str!("../../tests/data/record-v1.0-golden.tsv")
        .lines()
        .enumerate()
    {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "golden line {line_number}");
        let (Some(name), Some(raw_kind), Some(frame_hex)) =
            (columns.first(), columns.get(1), columns.get(2))
        else {
            continue;
        };
        let kind = u32::from_str_radix(raw_kind, 16);
        assert!(kind.is_ok(), "invalid kind on golden line {line_number}");
        if let Ok(kind) = kind {
            assert!(golden.insert(*name, (kind, *frame_hex)).is_none());
        }
    }

    let records = fixtures();
    assert!(records.is_some(), "record fixtures must be valid");
    let Some(records) = records else {
        return;
    };
    assert_eq!(golden.len(), records.len());
    for (name, record) in records {
        let expected = golden.get(name);
        assert!(expected.is_some(), "missing golden vector {name}");
        let Some((expected_kind, expected_hex)) = expected else {
            continue;
        };
        let expected_bytes = unhex(expected_hex);
        assert!(expected_bytes.is_some(), "invalid golden hex for {name}");
        let Some(expected_bytes) = expected_bytes else {
            continue;
        };
        let encoded = encode_record(&record);
        assert!(encoded.is_ok(), "encoding failed for {name}: {encoded:?}");
        let Some(encoded) = encoded.ok() else {
            continue;
        };
        assert_eq!(hex(&encoded), hex(&expected_bytes), "golden frame {name}");
        let kind_bytes = encoded.get(28..32);
        assert!(kind_bytes.is_some(), "frame kind header is truncated");
        let Some(kind_bytes) = kind_bytes else {
            continue;
        };
        let actual_kind = <[u8; 4]>::try_from(kind_bytes);
        assert!(actual_kind.is_ok(), "frame kind header width is invalid");
        let Some(actual_kind) = actual_kind.ok() else {
            continue;
        };
        assert_eq!(
            u32::from_le_bytes(actual_kind),
            *expected_kind,
            "kind for {name}"
        );
        let decoded = decode_record(&encoded);
        assert!(decoded.is_ok(), "decoding failed for {name}: {decoded:?}");
        if let Ok(decoded) = decoded {
            assert_eq!(encode_decoded_record(&decoded), Ok(encoded));
        }
    }
}

#[test]
fn runtime_record_assignments_match_the_policy_registry() {
    let mut assignments = BTreeMap::new();
    for (line_number, line) in include_str!("../../../../policy/record-wire-kinds.tsv")
        .lines()
        .enumerate()
    {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 4, "registry line {line_number}");
        let (Some(raw_kind), Some(name), Some(extensibility)) =
            (columns.first(), columns.get(1), columns.get(2))
        else {
            continue;
        };
        let number = u32::from_str_radix(raw_kind, 16);
        assert!(
            number.is_ok(),
            "invalid kind on registry line {line_number}"
        );
        assert_eq!(*extensibility, "closed", "extensibility for {name}");
        if let Ok(number) = number {
            assert!(assignments.insert(number, *name).is_none());
        }
    }
    assert_eq!(assignments.len(), 36);
    for (kind, name) in [
        (RecordKind::HistorySpaceDefinition, "HistorySpaceDefinition"),
        (RecordKind::Entity, "Entity"),
        (RecordKind::EntityRetirement, "EntityRetirement"),
        (
            RecordKind::PerspectiveDefinitionRevision,
            "PerspectiveDefinitionRevision",
        ),
        (RecordKind::PerspectiveRetirement, "PerspectiveRetirement"),
        (RecordKind::LayerDefinition, "LayerDefinition"),
        (RecordKind::LayerSchemaSnapshot, "LayerSchemaSnapshot"),
        (RecordKind::EntityTypeDefinition, "EntityTypeDefinition"),
        (RecordKind::PredicateDefinition, "PredicateDefinition"),
        (RecordKind::EventKindDefinition, "EventKindDefinition"),
        (RecordKind::MigrationPlan, "MigrationPlan"),
        (RecordKind::MigrationRun, "MigrationRun"),
        (
            RecordKind::MigrationStepCommitIdentity,
            "MigrationStepCommitIdentity",
        ),
        (RecordKind::Assertion, "Assertion"),
        (
            RecordKind::AssertionValidityClosure,
            "AssertionValidityClosure",
        ),
        (RecordKind::AssertionRetraction, "AssertionRetraction"),
        (RecordKind::Mask, "Mask"),
        (RecordKind::MaskValidityClosure, "MaskValidityClosure"),
        (RecordKind::MaskRetraction, "MaskRetraction"),
        (RecordKind::ReplacementBoundary, "ReplacementBoundary"),
        (
            RecordKind::ReplacementBoundaryValidityClosure,
            "ReplacementBoundaryValidityClosure",
        ),
        (
            RecordKind::ReplacementBoundaryRetraction,
            "ReplacementBoundaryRetraction",
        ),
        (RecordKind::ArchiveTransition, "ArchiveTransition"),
        (RecordKind::Event, "Event"),
        (RecordKind::EventMask, "EventMask"),
        (RecordKind::EventSpanClosure, "EventSpanClosure"),
        (RecordKind::EventRetraction, "EventRetraction"),
        (RecordKind::EventMaskRetraction, "EventMaskRetraction"),
        (RecordKind::EventRelation, "EventRelation"),
        (
            RecordKind::EventRelationRetraction,
            "EventRelationRetraction",
        ),
        (RecordKind::Source, "Source"),
        (RecordKind::Evidence, "Evidence"),
        (RecordKind::Provenance, "Provenance"),
        (RecordKind::EvidenceRetraction, "EvidenceRetraction"),
        (RecordKind::ProvenanceRetraction, "ProvenanceRetraction"),
        (RecordKind::TransferLineage, "TransferLineage"),
    ] {
        assert_eq!(assignments.get(&kind.number()), Some(&name));
        assert_eq!(RecordKind::from_number(kind.number()), Some(kind));
    }
}

#[test]
fn every_registered_record_kind_has_a_fixed_roundtrip_golden_vector() {
    let mut registered = std::collections::BTreeSet::new();
    for (line_number, line) in include_str!("../../../../policy/record-wire-kinds.tsv")
        .lines()
        .enumerate()
    {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 4, "record kind line {line_number}");
        if let Some(raw_kind) = columns.first() {
            let kind = u32::from_str_radix(raw_kind, 16);
            assert!(kind.is_ok(), "invalid kind on line {line_number}");
            if let Ok(kind) = kind {
                assert!(registered.insert(kind), "duplicate record kind {kind:#x}");
            }
        }
    }

    let mut golden_kinds = std::collections::BTreeSet::new();
    for (line_number, line) in include_str!("../../tests/data/record-v1.0-golden.tsv")
        .lines()
        .enumerate()
    {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "record golden line {line_number}");
        let kind = columns
            .get(1)
            .and_then(|raw_kind| u32::from_str_radix(raw_kind, 16).ok());
        assert!(
            kind.is_some(),
            "invalid record golden kind on line {line_number}"
        );
        if let Some(kind) = kind {
            golden_kinds.insert(kind);
        }
    }
    assert_eq!(registered.len(), 36);
    assert_eq!(golden_kinds, registered);
}

#[test]
fn source_meta_decoder_enforces_the_closed_evidence_and_provenance_matrices() {
    let references = record_ref_fixtures();
    assert!(
        references.is_some(),
        "RecordRef matrix fixtures must be valid"
    );
    let Some(references) = references else {
        return;
    };
    let Some(evidence_id) = id::<crate::EvidenceId>(90) else {
        return;
    };
    let Some(source_id) = id::<crate::SourceId>(91) else {
        return;
    };
    let Some(provenance_id) = id::<crate::ProvenanceId>(92) else {
        return;
    };

    for (_, target) in &references {
        let target_bytes = crate::encode_record_ref(*target);
        assert!(target_bytes.is_ok(), "RecordRef should encode: {target:?}");
        let Some(target_bytes) = target_bytes.ok() else {
            continue;
        };
        let evidence_payload = super::encode_fields(
            RecordKind::Evidence,
            vec![
                (1, encode_id(evidence_id).to_vec()),
                (2, encode_id(source_id).to_vec()),
                (3, target_bytes),
                (4, vec![1]),
                (5, super::encode_revision(Revision::GENESIS)),
            ],
        );
        assert!(evidence_payload.is_ok());
        let allowed = crate::EvidenceTargetRef::try_from(*target).is_ok();
        if let Ok(evidence_payload) = evidence_payload {
            assert_eq!(
                super::meta::decode_evidence(&evidence_payload, &crate::DecoderLimits::DEFAULT)
                    .is_ok(),
                allowed,
                "EvidenceTargetRef admission for {target:?}"
            );
        }
    }

    let mut expanded = references
        .iter()
        .map(|(_, reference)| *reference)
        .collect::<Vec<_>>();
    for reference in references.iter().map(|(_, reference)| *reference) {
        let encoded = crate::encode_record_ref(reference);
        assert!(encoded.is_ok());
        if let Ok(mut encoded) = encoded {
            let Some(last) = encoded.last_mut() else {
                continue;
            };
            *last = last.wrapping_add(64);
            let alternate = crate::decode_record_ref(&encoded);
            assert!(alternate.is_ok());
            if let Ok(alternate) = alternate {
                expanded.push(alternate);
            }
        }
    }

    let relations = [
        (ProvenanceRelation::Corrects, 1),
        (ProvenanceRelation::DerivedFrom, 2),
        (ProvenanceRelation::ResultedFrom, 3),
    ];
    for from in &expanded {
        for to in &expanded {
            for (relation, relation_code) in relations {
                let from_bytes = crate::encode_record_ref(*from);
                let to_bytes = crate::encode_record_ref(*to);
                assert!(from_bytes.is_ok() && to_bytes.is_ok());
                let (Ok(from_bytes), Ok(to_bytes)) = (from_bytes, to_bytes) else {
                    continue;
                };
                let payload = super::encode_fields(
                    RecordKind::Provenance,
                    vec![
                        (1, encode_id(provenance_id).to_vec()),
                        (2, from_bytes),
                        (3, to_bytes),
                        (4, vec![relation_code]),
                        (5, super::encode_revision(Revision::GENESIS)),
                    ],
                );
                assert!(payload.is_ok());
                let expected = match (
                    crate::ProvenanceEndpointRef::try_from(*from),
                    crate::ProvenanceEndpointRef::try_from(*to),
                ) {
                    (Ok(from), Ok(to)) => {
                        ProvenanceEdge::new(provenance_id, from, to, relation, Revision::GENESIS)
                            .is_ok()
                    }
                    _ => false,
                };
                if let Ok(payload) = payload {
                    assert_eq!(
                        super::meta::decode_provenance(&payload, &crate::DecoderLimits::DEFAULT)
                            .is_ok(),
                        expected,
                        "{relation:?} endpoint pair {from:?} -> {to:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn every_layer_capable_record_wire_payload_has_an_explicit_layer_id() {
    let records = fixtures();
    assert!(records.is_some(), "record fixtures must be valid");
    let Some(records) = records else {
        return;
    };
    let Some(expected_layer) = id::<crate::LayerId>(2).map(encode_id) else {
        return;
    };

    fn field(bytes: &[u8], tag: u32) -> Option<Vec<u8>> {
        let mut decoder = crate::TlvDecoder::new(bytes);
        while let Some(item) = decoder.next_field().ok()? {
            if item.tag() == tag {
                return Some(item.value().to_vec());
            }
        }
        None
    }

    for (name, record) in &records {
        let (top_field, context_field) = match *name {
            "assertion_positive" | "mask_exact_assertion" | "replacement_boundary_bounded" => {
                (2, Some(2))
            }
            "event_instant" | "event_mask" => (3, None),
            _ => continue,
        };
        let encoded = encode_record(record);
        assert!(encoded.is_ok(), "could not encode {name}: {encoded:?}");
        let Some(encoded) = encoded.ok() else {
            continue;
        };
        let frame = crate::decode_frame(&encoded);
        assert!(frame.is_ok(), "could not decode frame for {name}");
        let Some(payload) = frame.ok().map(|frame| frame.payload().to_vec()) else {
            continue;
        };
        let layer_bytes = field(&payload, top_field);
        let layer_bytes = match context_field {
            Some(nested_field) => layer_bytes.and_then(|context| field(&context, nested_field)),
            None => layer_bytes,
        };
        assert_eq!(
            layer_bytes.as_deref(),
            Some(expected_layer.as_slice()),
            "explicit LayerId in {name}"
        );
    }

    let Some((_, event)) = records.iter().find(|(name, _)| *name == "event_instant") else {
        return;
    };
    let encoded = encode_record(event);
    assert!(encoded.is_ok());
    let Some(encoded) = encoded.ok() else {
        return;
    };
    let frame = crate::decode_frame(&encoded);
    assert!(frame.is_ok());
    let Some(payload) = frame.ok().map(|frame| frame.payload()) else {
        return;
    };
    let fields = super::decode_fields(RecordKind::Event, payload, &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(fields.is_ok());
    let Some(fields) = fields.ok() else {
        return;
    };
    let missing_layer = super::encode_fields(
        RecordKind::Event,
        fields
            .into_iter()
            .filter(|(tag, _)| *tag != 3)
            .map(|(tag, value)| (tag, value.to_vec()))
            .collect(),
    );
    assert!(missing_layer.is_ok());
    if let Ok(payload) = missing_layer {
        let malformed = encode_frame(FrameHeader::new(RecordKind::Event.number()), &payload);
        assert!(malformed.is_ok());
        if let Ok(malformed) = malformed {
            assert!(matches!(
                crate::decode_record(&malformed),
                Err(RecordCodecError::MissingField {
                    kind: 0x1201,
                    field: 3
                })
            ));
        }
    }
}

#[test]
fn decoder_frame_field_array_depth_and_batch_budgets_fail_closed() {
    let records = fixtures();
    assert!(records.is_some(), "record fixtures must be valid");
    let Some(records) = records else { return };
    let entity_record = records
        .iter()
        .find(|(_, record)| matches!(record, Record::Entity(_)));
    assert!(entity_record.is_some(), "Entity fixture must be present");
    let Some((_, record)) = entity_record else {
        return;
    };
    let frame = encode_record(record);
    assert!(frame.is_ok());
    let Ok(frame) = frame else {
        return;
    };

    let small_frame_budget = DecoderLimits {
        max_frame_bytes: frame.len() - 1,
        ..DecoderLimits::DEFAULT
    };
    assert!(matches!(
        decode_record_with_limits(&frame, &small_frame_budget),
        Err(RecordCodecError::Frame(
            crate::FrameError::FrameTooLarge { .. }
        ))
    ));

    let few_fields = DecoderLimits {
        max_fields_per_record: 2,
        ..DecoderLimits::DEFAULT
    };
    assert!(matches!(
        decode_record_with_limits(&frame, &few_fields),
        Err(RecordCodecError::Wire(
            crate::WireError::ResourceLimitExceeded {
                resource: DecodeResource::FieldsPerRecord,
                limit: 2,
                actual: 3,
            }
        ))
    ));

    let shallow = DecoderLimits {
        max_nesting_depth: 6,
        ..DecoderLimits::DEFAULT
    };
    assert!(matches!(
        decode_record_with_limits(&frame, &shallow),
        Err(RecordCodecError::Wire(
            crate::WireError::ResourceLimitExceeded {
                resource: DecodeResource::NestingDepth,
                limit: 6,
                actual: 7,
            }
        ))
    ));

    let low_array_count = DecoderLimits {
        max_array_items: 1,
        ..DecoderLimits::DEFAULT
    };
    assert_eq!(
        super::decode_array_with_limits(&[2, 0, 0], &low_array_count),
        Err(crate::WireError::ResourceLimitExceeded {
            resource: DecodeResource::ArrayItems,
            limit: 1,
            actual: 2,
        })
    );
    let mut oversized_array_item = vec![1];
    oversized_array_item.extend(crate::numbers::encode_u128_varint(u128::MAX));
    assert!(matches!(
        super::decode_array_with_limits(&oversized_array_item, &DecoderLimits::DEFAULT),
        Err(crate::WireError::Truncated { .. })
    ));

    let low_collection_budget = DecoderLimits {
        max_collection_bytes: 0,
        ..DecoderLimits::DEFAULT
    };
    assert_eq!(
        super::decode_array_with_limits(&[1, 0], &low_collection_budget),
        Err(crate::WireError::ResourceLimitExceeded {
            resource: DecodeResource::CollectionBytes,
            limit: 0,
            actual: std::mem::size_of::<&[u8]>(),
        })
    );

    let too_many_records = DecoderLimits {
        max_records_per_batch: 1,
        ..DecoderLimits::DEFAULT
    };
    assert!(matches!(
        decode_record_batch_with_limits(&[&frame, &frame], &too_many_records),
        Err(RecordCodecError::Wire(
            crate::WireError::ResourceLimitExceeded {
                resource: DecodeResource::BatchRecords,
                limit: 1,
                actual: 2,
            }
        ))
    ));

    let too_many_batch_bytes = DecoderLimits {
        max_batch_bytes: 1,
        ..DecoderLimits::DEFAULT
    };
    assert!(matches!(
        decode_record_batch_with_limits(&[&frame], &too_many_batch_bytes),
        Err(RecordCodecError::Wire(crate::WireError::ResourceLimitExceeded {
            resource: DecodeResource::BatchBytes,
            limit: 1,
            actual,
        }))
        if actual == frame.len()
    ));

    let Some(source_id) = id::<crate::SourceId>(201) else {
        return;
    };
    let Some(source_kind) = symbol("src") else {
        return;
    };
    let digest = SourceContentDigest::new(Bytes::new(vec![1, 2, 3, 4]));
    assert!(digest.is_ok(), "non-empty digest fixture must be valid");
    let Some(digest) = digest.ok() else {
        return;
    };
    let source = Source::new(
        source_id,
        source_kind,
        None,
        Some(digest),
        SourceMetadata::default(),
        Revision::GENESIS,
    );
    let source_frame = encode_record(&Record::Source(source));
    assert!(source_frame.is_ok());
    let Ok(source_frame) = source_frame else {
        return;
    };
    let small_value_budget = DecoderLimits {
        max_string_or_bytes: 3,
        ..DecoderLimits::DEFAULT
    };
    assert!(matches!(
        decode_record_with_limits(&source_frame, &small_value_budget),
        Err(RecordCodecError::Wire(
            crate::WireError::ResourceLimitExceeded {
                resource: DecodeResource::StringOrBytes,
                limit: 3,
                actual: 4,
            }
        ))
    ));
}

#[test]
fn source_decoder_rejects_noncanonical_metadata_and_empty_optional_values() {
    let Some(source_id) = id::<crate::SourceId>(93) else {
        return;
    };
    let empty_metadata = super::encode_array(&[]);
    let reversed_metadata = [
        super::encode_fields(
            RecordKind::Source,
            vec![
                (1, super::encode_string("page")),
                (2, crate::encode_value(&Value::UInt(crate::UInt::new(2)))),
            ],
        ),
        super::encode_fields(
            RecordKind::Source,
            vec![
                (1, super::encode_string("origin")),
                (2, crate::encode_value(&Value::String(String::from("book")))),
            ],
        ),
    ];
    assert!(reversed_metadata[0].is_ok() && reversed_metadata[1].is_ok());
    let (Ok(first), Ok(second)) = (reversed_metadata[0].clone(), reversed_metadata[1].clone())
    else {
        return;
    };
    let unsorted_metadata = super::encode_array(&[first, second]);

    let build_payload = |locator: Vec<u8>, digest: Vec<u8>, metadata: Vec<u8>| {
        super::encode_fields(
            RecordKind::Source,
            vec![
                (1, encode_id(source_id).to_vec()),
                (2, super::encode_string("archive")),
                (3, locator),
                (4, digest),
                (5, metadata),
                (6, super::encode_revision(Revision::GENESIS)),
            ],
        )
    };

    let noncanonical = build_payload(vec![0], vec![0], unsorted_metadata);
    assert!(noncanonical.is_ok());
    if let Ok(payload) = noncanonical {
        assert!(matches!(
            super::meta::decode_source(&payload, &crate::DecoderLimits::DEFAULT),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x1301,
                field: 5
            })
        ));
    }

    let empty_locator = build_payload(vec![1, 0], vec![0], empty_metadata.clone());
    assert!(empty_locator.is_ok());
    if let Ok(payload) = empty_locator {
        assert!(matches!(
            super::meta::decode_source(&payload, &crate::DecoderLimits::DEFAULT),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x1301,
                field: 3
            })
        ));
    }

    let empty_digest = build_payload(vec![0], vec![1], empty_metadata);
    assert!(empty_digest.is_ok());
    if let Ok(payload) = empty_digest {
        assert!(matches!(
            super::meta::decode_source(&payload, &crate::DecoderLimits::DEFAULT),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x1301,
                field: 4
            })
        ));
    }
}

#[test]
fn migration_plan_decoder_reports_precise_invalid_fields() {
    let invalid_category = migration_plan_frame_with_field_replaced(2, &[6]);
    assert!(invalid_category.is_some());
    if let Some(frame) = invalid_category {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 2
            })
        ));
    }

    let misclassified_category = migration_plan_frame_with_field_replaced(2, &[5]);
    assert!(misclassified_category.is_some());
    if let Some(frame) = misclassified_category {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 2
            })
        ));
    }

    let zero_work_budget = migration_plan_frame_with_field_replaced(9, &[0]);
    assert!(zero_work_budget.is_some());
    if let Some(frame) = zero_work_budget {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 9
            })
        ));
    }

    let zero_memory_budget = migration_plan_frame_with_field_replaced(10, &[0]);
    assert!(zero_memory_budget.is_some());
    if let Some(frame) = zero_memory_budget {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 10
            })
        ));
    }

    let invalid_step_targets = migration_plan_frame_with_field_replaced(14, &[0]);
    assert!(invalid_step_targets.is_some());
    if let Some(frame) = invalid_step_targets {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 14
            })
        ));
    }

    for invalid_shift in [&[2][..], &[1][..]] {
        let invalid_calendar_shift = migration_plan_frame_with_field_replaced(13, invalid_shift);
        assert!(invalid_calendar_shift.is_some());
        if let Some(frame) = invalid_calendar_shift {
            assert!(matches!(
                decode_record(&frame),
                Err(RecordCodecError::InvalidFieldValue {
                    kind: 0x100b,
                    field: 13
                })
            ));
        }
    }

    let timeline_id = id::<TimelineId>(16);
    assert!(timeline_id.is_some());
    let Some(timeline_id) = timeline_id else {
        return;
    };
    let invalid_profile = super::encode_fields(
        RecordKind::MigrationPlan,
        vec![
            (1, encode_id(timeline_id).to_vec()),
            (2, vec![2]),
            (3, 0_i128.to_be_bytes().to_vec()),
            (4, vec![0]),
            (5, vec![0]),
            (6, vec![0]),
            (7, vec![2]),
        ],
    );
    assert!(invalid_profile.is_ok());
    let Some(invalid_profile) = invalid_profile.ok() else {
        return;
    };
    let invalid_calendar_profile = migration_plan_frame_with_field_replaced(13, &invalid_profile);
    assert!(invalid_calendar_profile.is_some());
    if let Some(frame) = invalid_calendar_profile {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 13
            })
        ));
    }

    let nested_invalid_category = super::encode_fields(
        RecordKind::MigrationPlan,
        vec![(1, vec![0]), (2, vec![0]), (3, vec![6])],
    );
    assert!(nested_invalid_category.is_ok());
    let Some(nested_invalid_category) = nested_invalid_category.ok() else {
        return;
    };
    let schema_changes = super::encode_array(&[nested_invalid_category]);
    let invalid_nested_category = migration_plan_frame_with_field_replaced(12, &schema_changes);
    assert!(invalid_nested_category.is_some());
    if let Some(frame) = invalid_nested_category {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x100b,
                field: 3
            })
        ));
    }
}

#[test]
fn runtime_record_ref_tags_match_the_complete_policy_registry() {
    let references = record_ref_fixtures();
    assert!(references.is_some());
    let Some(references) = references else {
        return;
    };
    let mut assignments = BTreeMap::new();
    for (line_number, line) in include_str!("../../../../policy/record-ref-wire-tags.tsv")
        .lines()
        .enumerate()
    {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "RecordRef registry line {line_number}");
        let (Some(raw_tag), Some(name)) = (columns.first(), columns.get(1)) else {
            continue;
        };
        let tag = raw_tag.parse::<u16>();
        assert!(tag.is_ok(), "invalid tag on registry line {line_number}");
        if let Ok(tag) = tag {
            assert!(assignments.insert(tag, *name).is_none());
        }
    }
    assert_eq!(assignments.len(), 25);
    assert_eq!(references.len(), assignments.len());
    for (_, reference) in references {
        assert_eq!(
            assignments.get(&reference.wire_tag().value()),
            Some(&reference.variant_name()),
            "registry name for RecordRef tag {}",
            reference.wire_tag().value()
        );
    }
}

#[test]
fn domain_record_refs_match_fixed_golden_vectors() {
    let references = record_ref_fixtures();
    assert!(references.is_some(), "RecordRef fixtures must be valid");
    let Some(references) = references else {
        return;
    };
    let supported = references;
    let mut golden = BTreeMap::new();
    for (line_number, line) in include_str!("../../tests/data/record-ref-v1.0-golden.tsv")
        .lines()
        .enumerate()
    {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "RecordRef golden line {line_number}");
        let (Some(name), Some(tag), Some(bytes)) =
            (columns.first(), columns.get(1), columns.get(2))
        else {
            continue;
        };
        let tag = tag.parse::<u16>();
        assert!(tag.is_ok(), "invalid RecordRef tag on line {line_number}");
        if let Ok(tag) = tag {
            assert!(golden.insert(*name, (tag, *bytes)).is_none());
        }
    }
    assert_eq!(golden.len(), supported.len());
    for (name, reference) in supported {
        let expected = golden.get(name);
        assert!(expected.is_some(), "missing RecordRef golden vector {name}");
        let Some((expected_tag, expected_hex)) = expected else {
            continue;
        };
        assert_eq!(
            reference.wire_tag().value(),
            *expected_tag,
            "tag for {name}"
        );
        let encoded = crate::encode_record_ref(reference);
        assert!(
            encoded.is_ok(),
            "RecordRef encoding failed for {name}: {encoded:?}"
        );
        let Some(encoded) = encoded.ok() else {
            continue;
        };
        let expected_bytes = unhex(expected_hex);
        assert!(expected_bytes.is_some(), "invalid RecordRef hex for {name}");
        let Some(expected_bytes) = expected_bytes else {
            continue;
        };
        assert_eq!(encoded, expected_bytes, "RecordRef golden {name}");
        assert_eq!(crate::decode_record_ref(&encoded), Ok(reference));
    }
}

#[test]
fn every_closed_archive_target_round_trips_and_archive_cannot_target_itself() {
    let references = record_ref_fixtures();
    assert!(references.is_some());
    let Some(references) = references else {
        return;
    };
    let mut target_count = 0;
    for (_, reference) in references {
        match ArchiveTargetRef::try_from(reference) {
            Ok(target) => {
                let encoded = super::lifecycle::encode_archive_target(target);
                let decoded = super::lifecycle::decode_archive_target(
                    RecordKind::ArchiveTransition,
                    2,
                    &encoded,
                    &crate::DecoderLimits::DEFAULT,
                );
                assert_eq!(decoded, Ok(target));
                target_count += 1;
            }
            Err(crate::RecordRefConversionError::NotArchiveTarget) => {
                assert!(matches!(reference, RecordRef::ArchiveTransition(_)));
            }
            Err(error) => assert_eq!(
                error,
                crate::RecordRefConversionError::NotArchiveTarget,
                "unexpected archive-target conversion"
            ),
        }
    }
    assert_eq!(target_count, 24);

    let Some(archive_id) = id::<crate::ArchiveTransitionId>(24) else {
        return;
    };
    let mut self_target = vec![24_u8];
    self_target.extend(encode_id(archive_id));
    assert!(matches!(
        super::lifecycle::decode_archive_target(
            RecordKind::ArchiveTransition,
            2,
            &self_target,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::InvalidFieldValue {
            kind: 0x110a,
            field: 2
        })
    ));
    let mut unknown_target = vec![26_u8];
    unknown_target.extend(encode_id(archive_id));
    assert!(matches!(
        super::lifecycle::decode_archive_target(
            RecordKind::ArchiveTransition,
            2,
            &unknown_target,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::UnknownRecordRefTag { tag: 26 })
    ));
}

#[test]
fn event_relations_normalize_after_canonicalize_same_time_and_reject_after_on_wire() {
    let records = fixtures();
    assert!(records.is_some());
    let Some(records) = records else {
        return;
    };
    let after_input = records
        .into_iter()
        .find(|(name, _)| *name == "event_relation_after_input")
        .map(|(_, record)| record);
    assert!(after_input.is_some());
    let Some(Record::EventRelation(after_input)) = after_input else {
        return;
    };
    let Some(earlier_id) = id::<crate::EventId>(50) else {
        return;
    };
    let Some(later_id) = id::<crate::EventId>(51) else {
        return;
    };
    assert_eq!(after_input.kind(), crate::EventRelationKind::Before);
    assert_eq!(after_input.from_event(), earlier_id);
    assert_eq!(after_input.to_event(), later_id);

    let Some(relation_id) = id::<crate::EventRelationId>(60) else {
        return;
    };
    let mut after_on_wire = TlvEncoder::new();
    assert!(after_on_wire.push(1, &encode_id(relation_id)).is_ok());
    assert!(after_on_wire.push(2, &encode_id(earlier_id)).is_ok());
    assert!(after_on_wire.push(3, &encode_id(later_id)).is_ok());
    assert!(after_on_wire.push(4, &[4]).is_ok());
    assert!(after_on_wire.push(5, &[0]).is_ok());
    let frame = encode_frame(
        FrameHeader::new(RecordKind::EventRelation.number()),
        &after_on_wire.finish(),
    );
    assert!(frame.is_ok());
    if let Ok(frame) = frame {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x1206,
                field: 4
            })
        ));
    }

    let mut self_relation = TlvEncoder::new();
    assert!(self_relation.push(1, &encode_id(relation_id)).is_ok());
    assert!(self_relation.push(2, &encode_id(earlier_id)).is_ok());
    assert!(self_relation.push(3, &encode_id(earlier_id)).is_ok());
    assert!(self_relation.push(4, &[1]).is_ok());
    assert!(self_relation.push(5, &[0]).is_ok());
    let frame = encode_frame(
        FrameHeader::new(RecordKind::EventRelation.number()),
        &self_relation.finish(),
    );
    assert!(frame.is_ok());
    if let Ok(frame) = frame {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::InvalidFieldValue {
                kind: 0x1206,
                field: 2
            })
        ));
    }

    let mut reversed_same_time = TlvEncoder::new();
    assert!(reversed_same_time.push(1, &encode_id(relation_id)).is_ok());
    assert!(reversed_same_time.push(2, &encode_id(later_id)).is_ok());
    assert!(reversed_same_time.push(3, &encode_id(earlier_id)).is_ok());
    assert!(reversed_same_time.push(4, &[2]).is_ok());
    assert!(reversed_same_time.push(5, &[0]).is_ok());
    let frame = encode_frame(
        FrameHeader::new(RecordKind::EventRelation.number()),
        &reversed_same_time.finish(),
    );
    assert!(frame.is_ok());
    if let Ok(frame) = frame {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::NonCanonicalRecord)
        ));
    }
}

#[test]
fn event_time_decoder_rejects_unknown_forms_and_invalid_closed_spans() {
    assert!(matches!(
        super::events::decode_event_time(
            RecordKind::Event,
            7,
            &[3],
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::InvalidFieldValue {
            kind: 0x1201,
            field: 7
        })
    ));
    let timeline_id = id::<TimelineId>(25);
    assert!(timeline_id.is_some());
    let Some(timeline_id) = timeline_id else {
        return;
    };
    let timeline = Timeline::new(timeline_id);
    let start = WorldTime::from_nanoseconds(timeline, 20);
    let end = WorldTime::from_nanoseconds(timeline, 10);
    let encoded_start = super::lifecycle::encode_world_time(start);
    let encoded_end = super::lifecycle::encode_world_time(end);
    assert!(encoded_start.is_ok());
    assert!(encoded_end.is_ok());
    let (Some(encoded_start), Some(encoded_end)) = (encoded_start.ok(), encoded_end.ok()) else {
        return;
    };
    let interval = super::lifecycle::nested(vec![
        (1, encoded_start),
        (2, super::lifecycle::optional(Some(encoded_end))),
    ]);
    assert!(interval.is_ok());
    let Some(interval) = interval.ok() else {
        return;
    };
    let mut tagged = vec![2];
    tagged.extend(interval);
    assert!(matches!(
        super::events::decode_event_time(
            RecordKind::Event,
            7,
            &tagged,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::InvalidFieldValue {
            kind: 0x1201,
            field: 7
        })
    ));
}

#[test]
fn event_participant_and_attribute_arrays_reject_noncanonical_order() {
    let Some(first_role) = id::<crate::EventRoleId>(1) else {
        return;
    };
    let Some(second_role) = id::<crate::EventRoleId>(2) else {
        return;
    };
    let Some(first_entity) = id::<crate::EntityId>(1) else {
        return;
    };
    let Some(second_entity) = id::<crate::EntityId>(2) else {
        return;
    };
    let first = super::lifecycle::nested(vec![
        (1, encode_id(first_role).to_vec()),
        (2, encode_id(first_entity).to_vec()),
    ]);
    let second = super::lifecycle::nested(vec![
        (1, encode_id(second_role).to_vec()),
        (2, encode_id(second_entity).to_vec()),
    ]);
    assert!(first.is_ok());
    assert!(second.is_ok());
    let (Some(first), Some(second)) = (first.ok(), second.ok()) else {
        return;
    };
    let reversed_participants = super::encode_array(&[second, first]);
    assert!(matches!(
        super::events::decode_participants(
            RecordKind::Event,
            5,
            &reversed_participants,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::NonCanonicalRecord)
    ));

    let Some(first_attribute) = id::<crate::EventAttributeId>(1) else {
        return;
    };
    let Some(second_attribute) = id::<crate::EventAttributeId>(2) else {
        return;
    };
    let first = super::lifecycle::nested(vec![
        (1, encode_id(first_attribute).to_vec()),
        (2, crate::encode_value(&Value::Bool(false))),
    ]);
    let second = super::lifecycle::nested(vec![
        (1, encode_id(second_attribute).to_vec()),
        (2, crate::encode_value(&Value::Bool(true))),
    ]);
    assert!(first.is_ok());
    assert!(second.is_ok());
    let (Some(first), Some(second)) = (first.ok(), second.ok()) else {
        return;
    };
    let reversed_attributes = super::encode_array(&[second, first]);
    assert!(matches!(
        super::events::decode_attributes(
            RecordKind::Event,
            6,
            &reversed_attributes,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::NonCanonicalRecord)
    ));
}

#[test]
fn every_record_ref_roundtrips_and_unknown_noncanonical_or_malformed_refs_are_rejected() {
    let Some(source_id) = id::<crate::SourceId>(7) else {
        return;
    };
    let mut source = vec![7];
    source.extend(encode_id(source_id));
    assert_eq!(
        crate::decode_record_ref(&source),
        Ok(RecordRef::Source(source_id))
    );
    assert_eq!(
        crate::encode_record_ref(RecordRef::Source(source_id)),
        Ok(source)
    );
    assert!(matches!(
        crate::decode_record_ref(&[0x7f]),
        Err(RecordCodecError::UnknownRecordRefTag { tag: 127 })
    ));
    assert!(crate::decode_record_ref(&[1, 0]).is_err());

    let mut invalid_id = vec![1];
    invalid_id.extend([0_u8; 16]);
    assert!(crate::decode_record_ref(&invalid_id).is_err());

    let mut noncanonical = vec![0x81, 0];
    noncanonical.extend([0_u8; 16]);
    assert!(crate::decode_record_ref(&noncanonical).is_err());

    let mut overflowing = crate::numbers::encode_u128_varint(65_536);
    overflowing.extend([0_u8; 16]);
    assert!(matches!(
        crate::decode_record_ref(&overflowing),
        Err(RecordCodecError::RecordRefTagOverflow { tag: 65_536 })
    ));
}

#[test]
fn mask_selector_rejects_unknown_tags_and_invalid_nested_polarity() {
    assert!(matches!(
        super::lifecycle::decode_selector(
            RecordKind::Mask,
            3,
            &[4],
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::InvalidFieldValue {
            kind: 0x1104,
            field: 3
        })
    ));
    let Some(subject_id) = id::<crate::EntityId>(12) else {
        return;
    };
    let Some(predicate_id) = id::<crate::PredicateId>(7) else {
        return;
    };
    let mut selector = TlvEncoder::new();
    assert!(selector.push(1, &encode_id(subject_id)).is_ok());
    assert!(selector.push(2, &encode_id(predicate_id)).is_ok());
    assert!(
        selector
            .push(3, &crate::encode_value(&Value::Bool(true)))
            .is_ok()
    );
    assert!(selector.push(4, &[0]).is_ok());
    let mut tagged = vec![2];
    tagged.extend(selector.finish());
    assert!(matches!(
        super::lifecycle::decode_selector(
            RecordKind::Mask,
            3,
            &tagged,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::InvalidFieldValue {
            kind: 0x1104,
            field: 3
        })
    ));

    let mut unknown_nested = TlvEncoder::new();
    assert!(unknown_nested.push(1, &encode_id(subject_id)).is_ok());
    assert!(unknown_nested.push(2, &encode_id(predicate_id)).is_ok());
    assert!(
        unknown_nested
            .push(3, &crate::encode_value(&Value::Bool(true)))
            .is_ok()
    );
    assert!(unknown_nested.push(4, &[1]).is_ok());
    assert!(unknown_nested.push(5, &[]).is_ok());
    let mut tagged = vec![2];
    tagged.extend(unknown_nested.finish());
    assert!(matches!(
        super::lifecycle::decode_selector(
            RecordKind::Mask,
            3,
            &tagged,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::UnknownField {
            kind: 0x1104,
            field: 5
        })
    ));
}

#[test]
fn decoded_records_preserve_all_optional_frame_flags() {
    let records = fixtures();
    assert!(records.is_some());
    let Some(records) = records else {
        return;
    };
    let record = records
        .into_iter()
        .find(|(name, _)| *name == "entity")
        .map(|(_, record)| record);
    assert!(record.is_some());
    let Some(record) = record else {
        return;
    };
    let flags = 0x8000_0000_0000_0041;
    let encoded = encode_record_with_flags(&record, flags);
    assert!(encoded.is_ok());
    if let Ok(encoded) = encoded {
        let decoded = decode_record(&encoded);
        assert!(decoded.is_ok());
        if let Ok(decoded) = decoded {
            assert_eq!(decoded.optional_flags(), flags);
            assert_eq!(encode_decoded_record(&decoded), Ok(encoded));
        }
    }
}

#[test]
fn unknown_record_kinds_and_closed_record_fields_are_rejected() {
    let unknown = encode_frame(FrameHeader::new(0xffff), &[]);
    assert!(unknown.is_ok());
    if let Ok(unknown) = unknown {
        assert!(matches!(
            decode_record(&unknown),
            Err(RecordCodecError::UnknownRecordKind { kind: 0xffff })
        ));
    }

    let history_id = id::<crate::HistorySpaceId>(1);
    assert!(history_id.is_some());
    let Some(history_id) = history_id else {
        return;
    };
    let mut payload = TlvEncoder::new();
    assert!(payload.push(1, &encode_id(history_id)).is_ok());
    assert!(payload.push(2, &[]).is_ok());
    assert!(payload.push(3, &[0]).is_ok());
    assert!(payload.push(4, &[]).is_ok());
    let unknown_field = encode_frame(
        FrameHeader::new(RecordKind::HistorySpaceDefinition.number()),
        &payload.finish(),
    );
    assert!(unknown_field.is_ok());
    if let Ok(unknown_field) = unknown_field {
        assert!(matches!(
            decode_record(&unknown_field),
            Err(RecordCodecError::UnknownField {
                kind: 0x1001,
                field: 4,
            })
        ));
    }
}

#[test]
fn missing_required_closed_record_fields_are_rejected() {
    let history_id = id::<crate::HistorySpaceId>(1);
    assert!(history_id.is_some());
    let Some(history_id) = history_id else {
        return;
    };
    let mut payload = TlvEncoder::new();
    assert!(payload.push(1, &encode_id(history_id)).is_ok());
    assert!(payload.push(2, &[]).is_ok());
    let missing = encode_frame(
        FrameHeader::new(RecordKind::HistorySpaceDefinition.number()),
        &payload.finish(),
    );
    assert!(missing.is_ok());
    if let Ok(missing) = missing {
        assert!(matches!(
            decode_record(&missing),
            Err(RecordCodecError::MissingField {
                kind: 0x1001,
                field: 3
            })
        ));
    }
}

fn constraint_fixtures() -> Option<Vec<ValueConstraint>> {
    let timeline = id::<TimelineId>(25)?;
    let time_min = Time::new(timeline, -4, symbol("second")?);
    let time_max = Time::new(timeline, 12, symbol("second")?);
    let decimal_min = crate::Decimal::new(false, 125, 2).ok()?;
    let decimal_max = crate::Decimal::new(false, 5, 0).ok()?;
    Some(vec![
        ValueConstraint::BoolSet(crate::NonEmptySet::new(vec![true, false]).ok()?),
        ValueConstraint::IntRange(
            crate::InclusiveRange::new(Some(Int::new(-5)), Some(Int::new(10))).ok()?,
        ),
        ValueConstraint::UIntRange(
            crate::InclusiveRange::new(Some(crate::UInt::new(0)), Some(crate::UInt::new(10)))
                .ok()?,
        ),
        ValueConstraint::DecimalRange(
            crate::InclusiveRange::new(Some(decimal_min), Some(decimal_max)).ok()?,
        ),
        ValueConstraint::StringByteLength(
            crate::InclusiveRange::new(Some(crate::UInt::new(1)), Some(crate::UInt::new(64)))
                .ok()?,
        ),
        ValueConstraint::SymbolSet(
            crate::NonEmptySet::new(vec![symbol("alpha")?, symbol("beta")?]).ok()?,
        ),
        ValueConstraint::TimeRange(crate::TimeRange::new(Some(time_min), Some(time_max)).ok()?),
        ValueConstraint::DurationRange(
            crate::InclusiveRange::new(
                Some(Duration::from_nanoseconds(-10)),
                Some(Duration::from_nanoseconds(20)),
            )
            .ok()?,
        ),
        ValueConstraint::BytesLength(
            crate::InclusiveRange::new(Some(crate::UInt::new(0)), Some(crate::UInt::new(1024)))
                .ok()?,
        ),
    ])
}

#[test]
fn every_closed_value_constraint_variant_has_a_canonical_nested_round_trip() {
    let constraints = constraint_fixtures();
    assert!(constraints.is_some());
    let Some(constraints) = constraints else {
        return;
    };
    for constraint in constraints {
        let encoded = super::schema::encode_constraint(&constraint);
        assert!(encoded.is_ok());
        let Some(encoded) = encoded.ok() else {
            continue;
        };
        let decoded = super::schema::decode_constraint(
            RecordKind::PredicateDefinition,
            8,
            &encoded,
            &crate::DecoderLimits::DEFAULT,
        );
        assert!(decoded.is_ok());
        if let Ok(decoded) = decoded {
            assert_eq!(super::schema::encode_constraint(&decoded), Ok(encoded));
        }
    }
}

#[test]
fn malformed_arrays_and_unknown_nested_constraint_codes_are_rejected() {
    assert!(super::decode_array(&[0, 0]).is_err());
    let mut encoder = TlvEncoder::new();
    assert!(encoder.push(1, &[99]).is_ok());
    assert!(encoder.push(2, &[]).is_ok());
    let encoded = encoder.finish();
    assert!(matches!(
        super::schema::decode_constraint(
            RecordKind::PredicateDefinition,
            8,
            &encoded,
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::InvalidFieldValue { field: 8, .. })
    ));

    let mut unknown_nested_field = TlvEncoder::new();
    assert!(unknown_nested_field.push(1, &[1]).is_ok());
    assert!(unknown_nested_field.push(2, &[]).is_ok());
    assert!(unknown_nested_field.push(3, &[]).is_ok());
    assert!(matches!(
        super::schema::decode_constraint(
            RecordKind::PredicateDefinition,
            8,
            &unknown_nested_field.finish(),
            &crate::DecoderLimits::DEFAULT
        ),
        Err(RecordCodecError::UnknownField {
            kind: 0x1009,
            field: 3,
        })
    ));
}

#[test]
fn semantically_valid_but_noncanonical_sets_are_rejected() {
    let history_id = id::<crate::PredicateId>(7);
    assert!(history_id.is_some());
    let Some(predicate_id) = history_id else {
        return;
    };
    let mut rule = TlvEncoder::new();
    assert!(rule.push(1, &[1]).is_ok());
    let reversed_bool_set = super::encode_array(&[vec![1, 1], vec![1, 0]]);
    assert!(rule.push(2, &reversed_bool_set).is_ok());
    let constraints = super::encode_array(&[rule.finish()]);

    let mut payload = TlvEncoder::new();
    assert!(payload.push(1, &encode_id(predicate_id)).is_ok());
    assert!(payload.push(2, &super::encode_string("enabled")).is_ok());
    assert!(payload.push(3, &[1]).is_ok());
    assert!(payload.push(4, &[1]).is_ok());
    assert!(payload.push(5, &[0]).is_ok());
    assert!(payload.push(6, &[1]).is_ok());
    assert!(payload.push(7, &[1]).is_ok());
    assert!(payload.push(8, &constraints).is_ok());
    assert!(payload.push(9, &[0]).is_ok());
    assert!(payload.push(10, &[1]).is_ok());
    assert!(payload.push(11, &[0]).is_ok());
    let frame = encode_frame(
        FrameHeader::new(RecordKind::PredicateDefinition.number()),
        &payload.finish(),
    );
    assert!(frame.is_ok());
    if let Ok(frame) = frame {
        assert!(matches!(
            decode_record(&frame),
            Err(RecordCodecError::NonCanonicalRecord)
        ));
    }
}
