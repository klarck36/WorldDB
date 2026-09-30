//! Typed schema definitions with constructor-checked combinations.

use std::collections::BTreeSet;
use std::fmt;

use crate::ids::{EntityTypeId, EventAttributeId, EventKindId, EventRoleId, PredicateId, Revision};
use crate::temporal::Duration;
use crate::values::{Symbol, Time, Value};
use crate::{Decimal, Int, UInt};

/// The exact scalar family accepted by a schema field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValueKind {
    /// `Value::Bool`.
    Bool,
    /// `Value::Int`.
    Int,
    /// `Value::UInt`.
    UInt,
    /// `Value::Decimal`.
    Decimal,
    /// `Value::String`.
    String,
    /// `Value::Symbol`.
    Symbol,
    /// `Value::Entity`.
    Entity,
    /// `Value::Time`.
    Time,
    /// `Value::Duration`.
    Duration,
    /// `Value::Bytes`.
    Bytes,
}

impl ValueKind {
    /// Returns the exact variant of a core value.
    #[must_use]
    pub const fn of(value: &Value) -> Self {
        match value {
            Value::Bool(_) => Self::Bool,
            Value::Int(_) => Self::Int,
            Value::UInt(_) => Self::UInt,
            Value::Decimal(_) => Self::Decimal,
            Value::String(_) => Self::String,
            Value::Symbol(_) => Self::Symbol,
            Value::Entity(_) => Self::Entity,
            Value::Time(_) => Self::Time,
            Value::Duration(_) => Self::Duration,
            Value::Bytes(_) => Self::Bytes,
        }
    }
}

/// A predicate's subject or entity-valued object's allowed entity type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EntityTypeConstraint {
    /// Any existing entity type is accepted.
    AnyEntity,
    /// Only entities assigned permanently to this type are accepted.
    Exact(EntityTypeId),
}

/// Predicate cardinality for stored assertions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Cardinality {
    /// One logical value slot; contradictions remain representable.
    Single,
    /// Multiple logical value slots.
    Multi,
}

/// The predicate's explicit rule for resolving same-precedence values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResolutionPolicy {
    /// Resolve a single slot, reporting disagreement as conflict.
    SingleValueReplace,
    /// Overlay multi-valued candidates without replacing equal-precedence peers.
    MultiValueOverlay,
    /// Use an explicit replacement boundary for a complete multi-value set.
    MultiValueReplace,
}

/// Lifecycle state stored in each revisioned schema definition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Lifecycle {
    /// New references and writes are allowed by schema lifecycle.
    Active,
    /// New writes require the later explicit opt-in/capability/warning contract.
    Deprecated,
    /// New references and writes are forbidden; historical reads remain valid.
    Retired,
}

impl Lifecycle {
    /// Returns whether `next` preserves the terminal lifecycle progression.
    #[must_use]
    pub const fn may_follow(self, next: Self) -> bool {
        match self {
            Self::Active => matches!(next, Self::Active | Self::Deprecated),
            Self::Deprecated => matches!(next, Self::Deprecated | Self::Retired),
            Self::Retired => matches!(next, Self::Retired),
        }
    }
}

/// Decimal display and measurement metadata kept beside a schema field.
///
/// Each present precision is a count of fractional decimal places. Currency
/// scale is likewise a count of fractional places. These values never enter a
/// `Decimal` and do not affect its equality or canonical scalar representation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DecimalFieldMetadata {
    display_precision: Option<u32>,
    measurement_precision: Option<u32>,
    currency_scale: Option<u32>,
}

impl DecimalFieldMetadata {
    /// Creates non-empty metadata for a Decimal field.
    pub fn new(
        display_precision: Option<u32>,
        measurement_precision: Option<u32>,
        currency_scale: Option<u32>,
    ) -> Result<Self, SchemaDefinitionError> {
        if display_precision.is_none()
            && measurement_precision.is_none()
            && currency_scale.is_none()
        {
            return Err(SchemaDefinitionError::EmptyDecimalMetadata);
        }
        Ok(Self {
            display_precision,
            measurement_precision,
            currency_scale,
        })
    }

    /// Returns requested fractional digits for display, if declared.
    #[must_use]
    pub const fn display_precision(self) -> Option<u32> {
        self.display_precision
    }

    /// Returns measured fractional precision, if declared.
    #[must_use]
    pub const fn measurement_precision(self) -> Option<u32> {
        self.measurement_precision
    }

    /// Returns currency fractional digits, if declared.
    #[must_use]
    pub const fn currency_scale(self) -> Option<u32> {
        self.currency_scale
    }
}

/// A non-empty set stored in exact scalar order with no duplicate members.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NonEmptySet<T>(Vec<T>);

impl<T: Ord> NonEmptySet<T> {
    /// Sorts members canonically and rejects empty or duplicate input.
    pub fn new(mut members: Vec<T>) -> Result<Self, SchemaDefinitionError> {
        if members.is_empty() {
            return Err(SchemaDefinitionError::EmptySet);
        }
        members.sort();
        if has_adjacent_duplicates(&members) {
            return Err(SchemaDefinitionError::DuplicateSetMember);
        }
        Ok(Self(members))
    }

    /// Returns the canonical members.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

/// An inclusive range with at least one bound and valid endpoint order.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InclusiveRange<T> {
    min: Option<T>,
    max: Option<T>,
}

impl<T: Ord> InclusiveRange<T> {
    /// Creates an inclusive range; equal bounds form a valid singleton.
    pub fn new(min: Option<T>, max: Option<T>) -> Result<Self, SchemaDefinitionError> {
        match (&min, &max) {
            (None, None) => return Err(SchemaDefinitionError::RangeNeedsBound),
            (Some(minimum), Some(maximum)) if minimum > maximum => {
                return Err(SchemaDefinitionError::ReversedRange);
            }
            _ => {}
        }
        Ok(Self { min, max })
    }

    /// Returns the inclusive lower bound, if present.
    #[must_use]
    pub fn min(&self) -> Option<&T> {
        self.min.as_ref()
    }

    /// Returns the inclusive upper bound, if present.
    #[must_use]
    pub fn max(&self) -> Option<&T> {
        self.max.as_ref()
    }
}

/// A same-timeline time range whose unit order is checked after schema resolution.
#[derive(Clone, Debug)]
pub struct TimeRange {
    min: Option<Time>,
    max: Option<Time>,
}

impl TimeRange {
    /// Creates an inclusive time range and rejects absent or cross-timeline bounds.
    ///
    /// If endpoint units differ, the selected schema snapshot must resolve them
    /// and verify `min <= max` before the constraint is used. Same-unit endpoints
    /// are checked here without conversion.
    pub fn new(min: Option<Time>, max: Option<Time>) -> Result<Self, SchemaDefinitionError> {
        if min.is_none() && max.is_none() {
            return Err(SchemaDefinitionError::RangeNeedsBound);
        }
        if let (Some(minimum), Some(maximum)) = (&min, &max) {
            if minimum.timeline_id() != maximum.timeline_id() {
                return Err(SchemaDefinitionError::CrossTimelineRange);
            }
            if minimum.unit() == maximum.unit() && minimum.ticks() > maximum.ticks() {
                return Err(SchemaDefinitionError::ReversedRange);
            }
        }
        Ok(Self { min, max })
    }

    /// Returns the inclusive lower endpoint, if present.
    #[must_use]
    pub fn min(&self) -> Option<&Time> {
        self.min.as_ref()
    }

    /// Returns the inclusive upper endpoint, if present.
    #[must_use]
    pub fn max(&self) -> Option<&Time> {
        self.max.as_ref()
    }
}

/// One closed, value-typed schema constraint.
#[derive(Clone, Debug)]
pub enum ValueConstraint {
    /// Restricts a Bool field to a non-empty set.
    BoolSet(NonEmptySet<bool>),
    /// Inclusive range over signed integers.
    IntRange(InclusiveRange<Int>),
    /// Inclusive range over unsigned integers.
    UIntRange(InclusiveRange<UInt>),
    /// Inclusive exact-numeric range over decimals.
    DecimalRange(InclusiveRange<Decimal>),
    /// Inclusive range over UTF-8 byte length.
    StringByteLength(InclusiveRange<UInt>),
    /// Restricts a Symbol field to a non-empty set.
    SymbolSet(NonEmptySet<Symbol>),
    /// Inclusive range on one timeline.
    TimeRange(TimeRange),
    /// Inclusive range over physical nanoseconds.
    DurationRange(InclusiveRange<Duration>),
    /// Inclusive range over byte length.
    BytesLength(InclusiveRange<UInt>),
}

impl ValueConstraint {
    /// Returns the ValueKind this rule accepts.
    #[must_use]
    pub const fn value_kind(&self) -> ValueKind {
        match self {
            Self::BoolSet(_) => ValueKind::Bool,
            Self::IntRange(_) => ValueKind::Int,
            Self::UIntRange(_) => ValueKind::UInt,
            Self::DecimalRange(_) => ValueKind::Decimal,
            Self::StringByteLength(_) => ValueKind::String,
            Self::SymbolSet(_) => ValueKind::Symbol,
            Self::TimeRange(_) => ValueKind::Time,
            Self::DurationRange(_) => ValueKind::Duration,
            Self::BytesLength(_) => ValueKind::Bytes,
        }
    }

    const fn variant_index(&self) -> usize {
        match self {
            Self::BoolSet(_) => 0,
            Self::IntRange(_) => 1,
            Self::UIntRange(_) => 2,
            Self::DecimalRange(_) => 3,
            Self::StringByteLength(_) => 4,
            Self::SymbolSet(_) => 5,
            Self::TimeRange(_) => 6,
            Self::DurationRange(_) => 7,
            Self::BytesLength(_) => 8,
        }
    }
}

/// A canonical conjunction of typed value constraints.
#[derive(Clone, Debug, Default)]
pub struct ConstraintSet {
    rules: Vec<ValueConstraint>,
}

impl ConstraintSet {
    /// Creates an unconstrained set.
    #[must_use]
    pub const fn unconstrained() -> Self {
        Self { rules: Vec::new() }
    }

    /// Sorts rule variants into stable order and rejects duplicate rule kinds.
    pub fn new(mut rules: Vec<ValueConstraint>) -> Result<Self, SchemaDefinitionError> {
        let mut seen = BTreeSet::new();
        for rule in &rules {
            let index = rule.variant_index();
            if !seen.insert(index) {
                return Err(SchemaDefinitionError::DuplicateConstraint(
                    rule.value_kind(),
                ));
            }
        }
        rules.sort_by_key(ValueConstraint::variant_index);
        Ok(Self { rules })
    }

    /// Returns the canonical ordered rules.
    #[must_use]
    pub fn rules(&self) -> &[ValueConstraint] {
        &self.rules
    }

    fn validate_for(&self, value_kind: ValueKind) -> Result<(), SchemaDefinitionError> {
        if let Some(rule) = self
            .rules
            .iter()
            .find(|rule| rule.value_kind() != value_kind)
        {
            return Err(SchemaDefinitionError::ConstraintValueKindMismatch {
                field: value_kind,
                constraint: rule.value_kind(),
            });
        }
        Ok(())
    }
}

/// A project-schema EntityType definition revision.
#[derive(Clone, Debug)]
pub struct EntityTypeDefinition {
    entity_type_id: EntityTypeId,
    symbol: Symbol,
    description: Option<String>,
    lifecycle: Lifecycle,
    created_revision: Revision,
}

impl EntityTypeDefinition {
    /// Creates a typed EntityType schema revision.
    #[must_use]
    pub const fn new(
        entity_type_id: EntityTypeId,
        symbol: Symbol,
        description: Option<String>,
        lifecycle: Lifecycle,
        created_revision: Revision,
    ) -> Self {
        Self {
            entity_type_id,
            symbol,
            description,
            lifecycle,
            created_revision,
        }
    }

    /// Appends a lifecycle-only revision without changing the stable ID.
    pub fn revise_lifecycle(
        &self,
        lifecycle: Lifecycle,
        created_revision: Revision,
    ) -> Result<Self, SchemaDefinitionError> {
        validate_lifecycle_revision(
            self.lifecycle,
            lifecycle,
            self.created_revision,
            created_revision,
        )?;
        Ok(Self {
            entity_type_id: self.entity_type_id,
            symbol: self.symbol.clone(),
            description: self.description.clone(),
            lifecycle,
            created_revision,
        })
    }

    /// Returns the stable schema identity.
    #[must_use]
    pub const fn entity_type_id(&self) -> EntityTypeId {
        self.entity_type_id
    }

    /// Returns the stable symbol in this definition revision.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the optional human-readable description.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the lifecycle state at this revision.
    #[must_use]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Returns the revision that introduced this definition state.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// A project-schema Predicate definition revision.
#[derive(Clone, Debug)]
pub struct PredicateDefinition {
    predicate_id: PredicateId,
    symbol: Symbol,
    subject_constraint: EntityTypeConstraint,
    value_kind: ValueKind,
    object_constraint: Option<EntityTypeConstraint>,
    cardinality: Cardinality,
    resolution_policy: ResolutionPolicy,
    constraints: ConstraintSet,
    decimal_metadata: Option<DecimalFieldMetadata>,
    lifecycle: Lifecycle,
    created_revision: Revision,
}

/// Typed input fields for [`PredicateDefinition::new`].
#[derive(Clone, Debug)]
pub struct PredicateDefinitionSpec {
    /// Stable schema identity.
    pub predicate_id: PredicateId,
    /// Validated predicate symbol.
    pub symbol: Symbol,
    /// Allowed subject entity type.
    pub subject_constraint: EntityTypeConstraint,
    /// Exact accepted scalar variant.
    pub value_kind: ValueKind,
    /// Required exactly when `value_kind` is Entity.
    pub object_constraint: Option<EntityTypeConstraint>,
    /// Single- or multi-value schema cardinality.
    pub cardinality: Cardinality,
    /// Explicit resolution behavior compatible with cardinality.
    pub resolution_policy: ResolutionPolicy,
    /// Closed typed constraints for this field.
    pub constraints: ConstraintSet,
    /// Optional Decimal-only field metadata.
    pub decimal_metadata: Option<DecimalFieldMetadata>,
    /// Lifecycle state for this schema revision.
    pub lifecycle: Lifecycle,
    /// Revision that introduced this definition state.
    pub created_revision: Revision,
}

impl PredicateDefinition {
    /// Validates and creates a Predicate schema revision.
    pub fn new(spec: PredicateDefinitionSpec) -> Result<Self, SchemaDefinitionError> {
        validate_object_constraint(spec.value_kind, spec.object_constraint)?;
        validate_cardinality_policy(spec.cardinality, spec.resolution_policy)?;
        spec.constraints.validate_for(spec.value_kind)?;
        if spec.decimal_metadata.is_some() && spec.value_kind != ValueKind::Decimal {
            return Err(SchemaDefinitionError::DecimalMetadataForNonDecimal);
        }
        Ok(Self {
            predicate_id: spec.predicate_id,
            symbol: spec.symbol,
            subject_constraint: spec.subject_constraint,
            value_kind: spec.value_kind,
            object_constraint: spec.object_constraint,
            cardinality: spec.cardinality,
            resolution_policy: spec.resolution_policy,
            constraints: spec.constraints,
            decimal_metadata: spec.decimal_metadata,
            lifecycle: spec.lifecycle,
            created_revision: spec.created_revision,
        })
    }

    /// Appends a lifecycle-only revision without changing the stable ID or semantics.
    pub fn revise_lifecycle(
        &self,
        lifecycle: Lifecycle,
        created_revision: Revision,
    ) -> Result<Self, SchemaDefinitionError> {
        validate_lifecycle_revision(
            self.lifecycle,
            lifecycle,
            self.created_revision,
            created_revision,
        )?;
        Ok(Self {
            predicate_id: self.predicate_id,
            symbol: self.symbol.clone(),
            subject_constraint: self.subject_constraint,
            value_kind: self.value_kind,
            object_constraint: self.object_constraint,
            cardinality: self.cardinality,
            resolution_policy: self.resolution_policy,
            constraints: self.constraints.clone(),
            decimal_metadata: self.decimal_metadata,
            lifecycle,
            created_revision,
        })
    }

    /// Returns the stable predicate identity.
    #[must_use]
    pub const fn predicate_id(&self) -> PredicateId {
        self.predicate_id
    }

    /// Returns the predicate symbol.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the subject entity-type constraint.
    #[must_use]
    pub const fn subject_constraint(&self) -> EntityTypeConstraint {
        self.subject_constraint
    }

    /// Returns the exact ValueKind accepted by this predicate.
    #[must_use]
    pub const fn value_kind(&self) -> ValueKind {
        self.value_kind
    }

    /// Returns the object constraint required for entity-valued predicates.
    #[must_use]
    pub const fn object_constraint(&self) -> Option<EntityTypeConstraint> {
        self.object_constraint
    }

    /// Returns predicate cardinality.
    #[must_use]
    pub const fn cardinality(&self) -> Cardinality {
        self.cardinality
    }

    /// Returns the explicit same-precedence resolution policy.
    #[must_use]
    pub const fn resolution_policy(&self) -> ResolutionPolicy {
        self.resolution_policy
    }

    /// Returns the closed, typed field constraints.
    #[must_use]
    pub const fn constraints(&self) -> &ConstraintSet {
        &self.constraints
    }

    /// Returns Decimal-only display/measurement metadata.
    #[must_use]
    pub const fn decimal_metadata(&self) -> Option<DecimalFieldMetadata> {
        self.decimal_metadata
    }

    /// Returns the lifecycle state at this revision.
    #[must_use]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Returns the revision that introduced this definition state.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// Canonical Gregorian UTC calendar span parameter for schema and query use.
///
/// Months are normalized to `0..12`; this type is never a `Value` variant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CalendarPeriod {
    years: u32,
    months: u8,
    days: u32,
}

impl CalendarPeriod {
    /// Creates a canonical non-negative calendar period.
    pub const fn new(years: u32, months: u8, days: u32) -> Result<Self, SchemaDefinitionError> {
        if months >= 12 {
            return Err(SchemaDefinitionError::NonCanonicalCalendarMonths(months));
        }
        Ok(Self {
            years,
            months,
            days,
        })
    }

    /// Returns the year component.
    #[must_use]
    pub const fn years(self) -> u32 {
        self.years
    }

    /// Returns the month component in `0..12`.
    #[must_use]
    pub const fn months(self) -> u8 {
        self.months
    }

    /// Returns the civil-day component.
    #[must_use]
    pub const fn days(self) -> u32 {
        self.days
    }
}

/// Permitted EventTime forms and an optional schema-defined maximum span.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EventTimeConstraint {
    form: EventTimeForm,
    max_calendar_span: Option<CalendarPeriod>,
}

impl EventTimeConstraint {
    /// Validates the form and optional calendar-span limit.
    pub const fn new(
        form: EventTimeForm,
        max_calendar_span: Option<CalendarPeriod>,
    ) -> Result<Self, SchemaDefinitionError> {
        if matches!(form, EventTimeForm::InstantOnly) && max_calendar_span.is_some() {
            return Err(SchemaDefinitionError::CalendarSpanOnInstantOnly);
        }
        Ok(Self {
            form,
            max_calendar_span,
        })
    }

    /// Returns the accepted EventTime form.
    #[must_use]
    pub const fn form(self) -> EventTimeForm {
        self.form
    }

    /// Returns the optional maximum calendar span.
    #[must_use]
    pub const fn max_calendar_span(self) -> Option<CalendarPeriod> {
        self.max_calendar_span
    }
}

/// Closed EventTime shape accepted by an EventKind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventTimeForm {
    /// Only an instant is accepted.
    InstantOnly,
    /// Only a span is accepted.
    SpanOnly,
    /// An instant or closed span is accepted.
    InstantOrSpan,
    /// An instant, closed span, or pending open span is accepted.
    OpenSpanAllowed,
}

/// Inclusive participant-count bounds for one EventRole.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RoleCardinality {
    min: u32,
    max: Option<u32>,
}

impl RoleCardinality {
    /// Creates checked `min..=max` cardinality; `None` means no upper bound.
    pub const fn new(min: u32, max: Option<u32>) -> Result<Self, SchemaDefinitionError> {
        if let Some(maximum) = max {
            if min > maximum {
                return Err(SchemaDefinitionError::RoleMaximumBelowMinimum);
            }
        }
        Ok(Self { min, max })
    }

    /// Returns the minimum participant count; positive values make the role required.
    #[must_use]
    pub const fn min(self) -> u32 {
        self.min
    }

    /// Returns the maximum participant count, if bounded.
    #[must_use]
    pub const fn max(self) -> Option<u32> {
        self.max
    }
}

/// One allowed participant role in an EventKind schema.
#[derive(Clone, Debug)]
pub struct EventRoleDefinition {
    event_role_id: EventRoleId,
    symbol: Symbol,
    entity_constraint: EntityTypeConstraint,
    cardinality: RoleCardinality,
}

impl EventRoleDefinition {
    /// Creates an allowed participant role and its minimum/maximum cardinality.
    #[must_use]
    pub const fn new(
        event_role_id: EventRoleId,
        symbol: Symbol,
        entity_constraint: EntityTypeConstraint,
        cardinality: RoleCardinality,
    ) -> Self {
        Self {
            event_role_id,
            symbol,
            entity_constraint,
            cardinality,
        }
    }

    /// Returns the stable role identity.
    #[must_use]
    pub const fn event_role_id(&self) -> EventRoleId {
        self.event_role_id
    }

    /// Returns the role symbol.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the required EntityType constraint for participants.
    #[must_use]
    pub const fn entity_constraint(&self) -> EntityTypeConstraint {
        self.entity_constraint
    }

    /// Returns the inclusive participant cardinality.
    #[must_use]
    pub const fn cardinality(&self) -> RoleCardinality {
        self.cardinality
    }
}

/// One allowed typed attribute in an EventKind schema.
#[derive(Clone, Debug)]
pub struct EventAttributeDefinition {
    event_attribute_id: EventAttributeId,
    symbol: Symbol,
    value_kind: ValueKind,
    object_constraint: Option<EntityTypeConstraint>,
    constraints: ConstraintSet,
    decimal_metadata: Option<DecimalFieldMetadata>,
    required: bool,
}

impl EventAttributeDefinition {
    /// Validates and creates a typed event attribute.
    pub fn new(
        event_attribute_id: EventAttributeId,
        symbol: Symbol,
        value_kind: ValueKind,
        object_constraint: Option<EntityTypeConstraint>,
        constraints: ConstraintSet,
        decimal_metadata: Option<DecimalFieldMetadata>,
        required: bool,
    ) -> Result<Self, SchemaDefinitionError> {
        validate_object_constraint(value_kind, object_constraint)?;
        constraints.validate_for(value_kind)?;
        if decimal_metadata.is_some() && value_kind != ValueKind::Decimal {
            return Err(SchemaDefinitionError::DecimalMetadataForNonDecimal);
        }
        Ok(Self {
            event_attribute_id,
            symbol,
            value_kind,
            object_constraint,
            constraints,
            decimal_metadata,
            required,
        })
    }

    /// Returns the stable attribute identity.
    #[must_use]
    pub const fn event_attribute_id(&self) -> EventAttributeId {
        self.event_attribute_id
    }

    /// Returns the attribute symbol.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the required ValueKind.
    #[must_use]
    pub const fn value_kind(&self) -> ValueKind {
        self.value_kind
    }

    /// Returns the entity type constraint, when the attribute stores an Entity.
    #[must_use]
    pub const fn object_constraint(&self) -> Option<EntityTypeConstraint> {
        self.object_constraint
    }

    /// Returns the allowed-value constraints.
    #[must_use]
    pub const fn constraints(&self) -> &ConstraintSet {
        &self.constraints
    }

    /// Returns Decimal-only display/measurement metadata.
    #[must_use]
    pub const fn decimal_metadata(&self) -> Option<DecimalFieldMetadata> {
        self.decimal_metadata
    }

    /// Returns whether every Event of this kind must provide the attribute.
    #[must_use]
    pub const fn required(&self) -> bool {
        self.required
    }
}

/// Revisioned complete event schema, including roles, attributes, and time shape.
#[derive(Clone, Debug)]
pub struct EventKindDefinition {
    event_kind_id: EventKindId,
    symbol: Symbol,
    roles: Vec<EventRoleDefinition>,
    attributes: Vec<EventAttributeDefinition>,
    event_time_constraint: EventTimeConstraint,
    lifecycle: Lifecycle,
    created_revision: Revision,
}

impl EventKindDefinition {
    /// Creates a complete, canonically ordered EventKind schema revision.
    pub fn new(
        event_kind_id: EventKindId,
        symbol: Symbol,
        mut roles: Vec<EventRoleDefinition>,
        mut attributes: Vec<EventAttributeDefinition>,
        event_time_constraint: EventTimeConstraint,
        lifecycle: Lifecycle,
        created_revision: Revision,
    ) -> Result<Self, SchemaDefinitionError> {
        roles.sort_by_key(EventRoleDefinition::event_role_id);
        if roles.windows(2).any(|pair| {
            matches!((pair.first(), pair.get(1)), (Some(left), Some(right)) if left.event_role_id == right.event_role_id)
        }) {
            return Err(SchemaDefinitionError::DuplicateEventRoleId);
        }
        let mut role_symbols: Vec<&Symbol> = roles.iter().map(|role| &role.symbol).collect();
        role_symbols.sort();
        if has_adjacent_duplicates(&role_symbols) {
            return Err(SchemaDefinitionError::DuplicateEventRoleSymbol);
        }

        attributes.sort_by_key(EventAttributeDefinition::event_attribute_id);
        if attributes.windows(2).any(|pair| {
            matches!((pair.first(), pair.get(1)), (Some(left), Some(right)) if left.event_attribute_id == right.event_attribute_id)
        }) {
            return Err(SchemaDefinitionError::DuplicateEventAttributeId);
        }
        let mut attribute_symbols: Vec<&Symbol> = attributes
            .iter()
            .map(|attribute| &attribute.symbol)
            .collect();
        attribute_symbols.sort();
        if has_adjacent_duplicates(&attribute_symbols) {
            return Err(SchemaDefinitionError::DuplicateEventAttributeSymbol);
        }

        Ok(Self {
            event_kind_id,
            symbol,
            roles,
            attributes,
            event_time_constraint,
            lifecycle,
            created_revision,
        })
    }

    /// Appends a lifecycle-only revision without changing the stable ID or schema.
    pub fn revise_lifecycle(
        &self,
        lifecycle: Lifecycle,
        created_revision: Revision,
    ) -> Result<Self, SchemaDefinitionError> {
        validate_lifecycle_revision(
            self.lifecycle,
            lifecycle,
            self.created_revision,
            created_revision,
        )?;
        Ok(Self {
            event_kind_id: self.event_kind_id,
            symbol: self.symbol.clone(),
            roles: self.roles.clone(),
            attributes: self.attributes.clone(),
            event_time_constraint: self.event_time_constraint,
            lifecycle,
            created_revision,
        })
    }

    /// Returns the stable EventKind identity.
    #[must_use]
    pub const fn event_kind_id(&self) -> EventKindId {
        self.event_kind_id
    }

    /// Returns the EventKind symbol.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the allowed roles in stable identity order.
    #[must_use]
    pub fn roles(&self) -> &[EventRoleDefinition] {
        &self.roles
    }

    /// Returns the allowed attributes in stable identity order.
    #[must_use]
    pub fn attributes(&self) -> &[EventAttributeDefinition] {
        &self.attributes
    }

    /// Returns the EventTime shape and maximum-span rule.
    #[must_use]
    pub const fn event_time_constraint(&self) -> EventTimeConstraint {
        self.event_time_constraint
    }

    /// Returns the lifecycle state at this revision.
    #[must_use]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Returns the revision that introduced this definition state.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

fn validate_object_constraint(
    value_kind: ValueKind,
    object_constraint: Option<EntityTypeConstraint>,
) -> Result<(), SchemaDefinitionError> {
    match (value_kind, object_constraint) {
        (ValueKind::Entity, None) => Err(SchemaDefinitionError::MissingEntityObjectConstraint),
        (ValueKind::Entity, Some(_)) => Ok(()),
        (_, Some(_)) => Err(SchemaDefinitionError::ObjectConstraintForNonEntity),
        (_, None) => Ok(()),
    }
}

fn has_adjacent_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values.windows(2).any(
        |pair| matches!((pair.first(), pair.get(1)), (Some(left), Some(right)) if left == right),
    )
}

fn validate_cardinality_policy(
    cardinality: Cardinality,
    resolution_policy: ResolutionPolicy,
) -> Result<(), SchemaDefinitionError> {
    match (cardinality, resolution_policy) {
        (Cardinality::Single, ResolutionPolicy::SingleValueReplace)
        | (Cardinality::Multi, ResolutionPolicy::MultiValueOverlay)
        | (Cardinality::Multi, ResolutionPolicy::MultiValueReplace) => Ok(()),
        _ => Err(SchemaDefinitionError::CardinalityPolicyMismatch),
    }
}

fn validate_lifecycle_revision(
    current: Lifecycle,
    next: Lifecycle,
    current_revision: Revision,
    next_revision: Revision,
) -> Result<(), SchemaDefinitionError> {
    if !current.may_follow(next) {
        return Err(SchemaDefinitionError::LifecycleRegression);
    }
    if next_revision <= current_revision {
        return Err(SchemaDefinitionError::SchemaRevisionNotIncreasing);
    }
    Ok(())
}

/// Structural failure while constructing a validated schema definition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SchemaDefinitionError {
    /// A Decimal metadata object has no declared field.
    EmptyDecimalMetadata,
    /// Decimal metadata was attached to a non-Decimal field.
    DecimalMetadataForNonDecimal,
    /// A range has neither a lower nor an upper endpoint.
    RangeNeedsBound,
    /// A range's lower endpoint sorts after its upper endpoint.
    ReversedRange,
    /// A Time range crosses timeline identities.
    CrossTimelineRange,
    /// A set constraint contains no members.
    EmptySet,
    /// A set constraint contains the same member more than once.
    DuplicateSetMember,
    /// A ConstraintSet has more than one rule for a ValueKind.
    DuplicateConstraint(ValueKind),
    /// A rule's value family differs from the predicate/attribute ValueKind.
    ConstraintValueKindMismatch {
        /// The field's declared ValueKind.
        field: ValueKind,
        /// The ValueKind accepted by the offending constraint.
        constraint: ValueKind,
    },
    /// Entity-valued fields require an explicit entity-type constraint.
    MissingEntityObjectConstraint,
    /// A non-Entity field cannot carry an entity object constraint.
    ObjectConstraintForNonEntity,
    /// Cardinality and resolution policy are incompatible.
    CardinalityPolicyMismatch,
    /// CalendarPeriod months must be in the canonical `0..12` range.
    NonCanonicalCalendarMonths(u8),
    /// Only span-capable EventTime forms may declare a maximum calendar span.
    CalendarSpanOnInstantOnly,
    /// Role cardinality maximum is lower than its minimum.
    RoleMaximumBelowMinimum,
    /// An EventKind declares the same role identity twice.
    DuplicateEventRoleId,
    /// An EventKind declares the same role symbol twice.
    DuplicateEventRoleSymbol,
    /// An EventKind declares the same attribute identity twice.
    DuplicateEventAttributeId,
    /// An EventKind declares the same attribute symbol twice.
    DuplicateEventAttributeSymbol,
    /// A lifecycle update would reactivate a deprecated or retired schema item.
    LifecycleRegression,
    /// A new schema definition state must use a later published revision.
    SchemaRevisionNotIncreasing,
}

impl fmt::Display for SchemaDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDecimalMetadata => formatter.write_str("Decimal metadata is empty"),
            Self::DecimalMetadataForNonDecimal => {
                formatter.write_str("Decimal metadata requires a Decimal ValueKind")
            }
            Self::RangeNeedsBound => formatter.write_str("range requires at least one bound"),
            Self::ReversedRange => formatter.write_str("range minimum exceeds maximum"),
            Self::CrossTimelineRange => formatter.write_str("Time range crosses timelines"),
            Self::EmptySet => formatter.write_str("set constraint must contain a member"),
            Self::DuplicateSetMember => {
                formatter.write_str("set constraint has a duplicate member")
            }
            Self::DuplicateConstraint(kind) => {
                write!(formatter, "duplicate constraint for {kind:?}")
            }
            Self::ConstraintValueKindMismatch { field, constraint } => write!(
                formatter,
                "constraint for {constraint:?} cannot be attached to {field:?}"
            ),
            Self::MissingEntityObjectConstraint => {
                formatter.write_str("Entity ValueKind requires an object constraint")
            }
            Self::ObjectConstraintForNonEntity => {
                formatter.write_str("object constraint is only valid for Entity ValueKind")
            }
            Self::CardinalityPolicyMismatch => {
                formatter.write_str("cardinality and resolution policy are incompatible")
            }
            Self::NonCanonicalCalendarMonths(months) => {
                write!(
                    formatter,
                    "CalendarPeriod month component {months} is not in 0..12"
                )
            }
            Self::CalendarSpanOnInstantOnly => {
                formatter.write_str("InstantOnly cannot declare a maximum calendar span")
            }
            Self::RoleMaximumBelowMinimum => {
                formatter.write_str("role cardinality maximum is lower than its minimum")
            }
            Self::DuplicateEventRoleId => formatter.write_str("duplicate EventRoleId"),
            Self::DuplicateEventRoleSymbol => formatter.write_str("duplicate EventRole symbol"),
            Self::DuplicateEventAttributeId => formatter.write_str("duplicate EventAttributeId"),
            Self::DuplicateEventAttributeSymbol => {
                formatter.write_str("duplicate EventAttribute symbol")
            }
            Self::LifecycleRegression => {
                formatter.write_str("schema lifecycle cannot move backward")
            }
            Self::SchemaRevisionNotIncreasing => {
                formatter.write_str("schema definition revision must increase")
            }
        }
    }
}

impl std::error::Error for SchemaDefinitionError {}

#[cfg(test)]
mod tests {
    use super::{
        CalendarPeriod, Cardinality, ConstraintSet, DecimalFieldMetadata, EntityTypeConstraint,
        EntityTypeDefinition, EventAttributeDefinition, EventKindDefinition, EventRoleDefinition,
        EventTimeConstraint, EventTimeForm, InclusiveRange, Lifecycle, NonEmptySet,
        PredicateDefinition, PredicateDefinitionSpec, ResolutionPolicy, RoleCardinality,
        SchemaDefinitionError, ValueConstraint, ValueKind,
    };
    use crate::ids::{
        DomainId, EntityTypeId, EventAttributeId, EventKindId, EventRoleId, PredicateId, Revision,
    };
    use crate::{Decimal, Int, Symbol, Value};
    use std::str::FromStr;

    fn test_id<T: DomainId>() -> Option<T> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = 1;
        T::try_from_bytes(bytes).ok()
    }

    #[test]
    fn value_kind_is_an_exact_match_for_the_closed_value_enum() {
        let value = Value::Bool(true);
        assert_eq!(ValueKind::of(&value), ValueKind::Bool);
        let decimal = Decimal::from_str("1.00");
        assert!(decimal.is_ok());
        if let Ok(decimal) = decimal {
            let value = Value::Decimal(decimal);
            assert_eq!(ValueKind::of(&value), ValueKind::Decimal);
        }
    }

    #[test]
    fn predicate_constructor_checks_object_and_cardinality_policy_pairs() {
        let predicate_id = test_id::<PredicateId>();
        assert!(predicate_id.is_some());
        let symbol = Symbol::new("relation");
        assert!(symbol.is_ok());
        if let (Some(predicate_id), Ok(symbol)) = (predicate_id, symbol) {
            let missing_entity_constraint = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id,
                symbol: symbol.clone(),
                subject_constraint: EntityTypeConstraint::AnyEntity,
                value_kind: ValueKind::Entity,
                object_constraint: None,
                cardinality: Cardinality::Single,
                resolution_policy: ResolutionPolicy::SingleValueReplace,
                constraints: ConstraintSet::unconstrained(),
                decimal_metadata: None,
                lifecycle: Lifecycle::Active,
                created_revision: Revision::GENESIS,
            });
            assert_eq!(
                missing_entity_constraint.err(),
                Some(SchemaDefinitionError::MissingEntityObjectConstraint)
            );

            let entity_type_id = test_id::<EntityTypeId>();
            assert!(entity_type_id.is_some());
            if let Some(entity_type_id) = entity_type_id {
                let valid_entity_constraint = PredicateDefinition::new(PredicateDefinitionSpec {
                    predicate_id,
                    symbol: symbol.clone(),
                    subject_constraint: EntityTypeConstraint::AnyEntity,
                    value_kind: ValueKind::Entity,
                    object_constraint: Some(EntityTypeConstraint::Exact(entity_type_id)),
                    cardinality: Cardinality::Single,
                    resolution_policy: ResolutionPolicy::SingleValueReplace,
                    constraints: ConstraintSet::unconstrained(),
                    decimal_metadata: None,
                    lifecycle: Lifecycle::Active,
                    created_revision: Revision::GENESIS,
                });
                assert!(valid_entity_constraint.is_ok());

                let forbidden_object_constraint =
                    PredicateDefinition::new(PredicateDefinitionSpec {
                        predicate_id,
                        symbol: symbol.clone(),
                        subject_constraint: EntityTypeConstraint::AnyEntity,
                        value_kind: ValueKind::String,
                        object_constraint: Some(EntityTypeConstraint::Exact(entity_type_id)),
                        cardinality: Cardinality::Single,
                        resolution_policy: ResolutionPolicy::SingleValueReplace,
                        constraints: ConstraintSet::unconstrained(),
                        decimal_metadata: None,
                        lifecycle: Lifecycle::Active,
                        created_revision: Revision::GENESIS,
                    });
                assert_eq!(
                    forbidden_object_constraint.err(),
                    Some(SchemaDefinitionError::ObjectConstraintForNonEntity)
                );
            }

            for (cardinality, resolution_policy, accepted) in [
                (
                    Cardinality::Single,
                    ResolutionPolicy::SingleValueReplace,
                    true,
                ),
                (
                    Cardinality::Single,
                    ResolutionPolicy::MultiValueOverlay,
                    false,
                ),
                (
                    Cardinality::Single,
                    ResolutionPolicy::MultiValueReplace,
                    false,
                ),
                (
                    Cardinality::Multi,
                    ResolutionPolicy::SingleValueReplace,
                    false,
                ),
                (
                    Cardinality::Multi,
                    ResolutionPolicy::MultiValueOverlay,
                    true,
                ),
                (
                    Cardinality::Multi,
                    ResolutionPolicy::MultiValueReplace,
                    true,
                ),
            ] {
                let definition = PredicateDefinition::new(PredicateDefinitionSpec {
                    predicate_id,
                    symbol: symbol.clone(),
                    subject_constraint: EntityTypeConstraint::AnyEntity,
                    value_kind: ValueKind::String,
                    object_constraint: None,
                    cardinality,
                    resolution_policy,
                    constraints: ConstraintSet::unconstrained(),
                    decimal_metadata: None,
                    lifecycle: Lifecycle::Active,
                    created_revision: Revision::GENESIS,
                });
                assert_eq!(definition.is_ok(), accepted);
                if !accepted {
                    assert_eq!(
                        definition.err(),
                        Some(SchemaDefinitionError::CardinalityPolicyMismatch)
                    );
                }
            }
        }
    }

    #[test]
    fn single_cardinality_is_only_schema_resolution_policy() {
        let predicate_id = test_id::<PredicateId>();
        assert!(predicate_id.is_some());
        let symbol = Symbol::new("single_fact");
        assert!(symbol.is_ok());
        if let (Some(predicate_id), Ok(symbol)) = (predicate_id, symbol) {
            let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id,
                symbol,
                subject_constraint: EntityTypeConstraint::AnyEntity,
                value_kind: ValueKind::Bool,
                object_constraint: None,
                cardinality: Cardinality::Single,
                resolution_policy: ResolutionPolicy::SingleValueReplace,
                constraints: ConstraintSet::unconstrained(),
                decimal_metadata: None,
                lifecycle: Lifecycle::Active,
                created_revision: Revision::GENESIS,
            });
            assert!(predicate.is_ok());
            if let Ok(predicate) = predicate {
                assert_eq!(predicate.cardinality(), Cardinality::Single);
                assert_eq!(
                    predicate.resolution_policy(),
                    ResolutionPolicy::SingleValueReplace
                );
                // The schema definition contains no assertion count or conflict suppression.
            }
        }
    }

    #[test]
    fn decimal_precision_metadata_is_separate_from_numeric_identity() {
        let integer_spelling = Decimal::from_str("1");
        let padded_spelling = Decimal::from_str("1.00");
        let low_precision = DecimalFieldMetadata::new(Some(0), None, None);
        let high_precision = DecimalFieldMetadata::new(Some(2), Some(3), Some(2));
        assert!(integer_spelling.is_ok());
        assert!(padded_spelling.is_ok());
        assert!(low_precision.is_ok());
        assert!(high_precision.is_ok());
        assert_eq!(
            DecimalFieldMetadata::new(None, None, None),
            Err(SchemaDefinitionError::EmptyDecimalMetadata)
        );

        let metadata = high_precision.ok();
        if let (Ok(left), Ok(right), Ok(low), Ok(high)) = (
            integer_spelling,
            padded_spelling,
            low_precision,
            high_precision,
        ) {
            assert_eq!(left, right);
            assert_ne!(low, high);
        }

        let predicate_id = test_id::<PredicateId>();
        let symbol = Symbol::new("amount");
        assert!(predicate_id.is_some());
        assert!(symbol.is_ok());
        if let (Some(predicate_id), Ok(symbol), Ok(low), Ok(high)) =
            (predicate_id, symbol, low_precision, high_precision)
        {
            let low_definition = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id,
                symbol: symbol.clone(),
                subject_constraint: EntityTypeConstraint::AnyEntity,
                value_kind: ValueKind::Decimal,
                object_constraint: None,
                cardinality: Cardinality::Single,
                resolution_policy: ResolutionPolicy::SingleValueReplace,
                constraints: ConstraintSet::unconstrained(),
                decimal_metadata: Some(low),
                lifecycle: Lifecycle::Active,
                created_revision: Revision::GENESIS,
            });
            let high_definition = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id,
                symbol,
                subject_constraint: EntityTypeConstraint::AnyEntity,
                value_kind: ValueKind::Decimal,
                object_constraint: None,
                cardinality: Cardinality::Single,
                resolution_policy: ResolutionPolicy::SingleValueReplace,
                constraints: ConstraintSet::unconstrained(),
                decimal_metadata: Some(high),
                lifecycle: Lifecycle::Active,
                created_revision: Revision::FIRST_COMMIT,
            });
            assert!(low_definition.is_ok());
            assert!(high_definition.is_ok());
            if let (Ok(low_definition), Ok(high_definition)) = (low_definition, high_definition) {
                assert_eq!(
                    low_definition.predicate_id(),
                    high_definition.predicate_id()
                );
                assert_ne!(
                    low_definition.decimal_metadata(),
                    high_definition.decimal_metadata()
                );
                let revision_two = Revision::new(2);
                assert!(revision_two.is_ok());
                if let Ok(revision_two) = revision_two {
                    let deprecated =
                        low_definition.revise_lifecycle(Lifecycle::Deprecated, revision_two);
                    assert!(deprecated.is_ok());
                    if let Ok(deprecated) = deprecated {
                        assert_eq!(deprecated.predicate_id(), low_definition.predicate_id());
                        assert_eq!(
                            deprecated.decimal_metadata(),
                            low_definition.decimal_metadata()
                        );
                    }
                }
            }
        }

        let predicate_id = test_id::<PredicateId>();
        let symbol = Symbol::new("text_field");
        assert!(predicate_id.is_some());
        assert!(symbol.is_ok());
        if let (Some(predicate_id), Ok(symbol), Some(metadata)) = (predicate_id, symbol, metadata) {
            let non_decimal_metadata = PredicateDefinition::new(PredicateDefinitionSpec {
                predicate_id,
                symbol,
                subject_constraint: EntityTypeConstraint::AnyEntity,
                value_kind: ValueKind::String,
                object_constraint: None,
                cardinality: Cardinality::Single,
                resolution_policy: ResolutionPolicy::SingleValueReplace,
                constraints: ConstraintSet::unconstrained(),
                decimal_metadata: Some(metadata),
                lifecycle: Lifecycle::Active,
                created_revision: Revision::GENESIS,
            });
            assert_eq!(
                non_decimal_metadata.err(),
                Some(SchemaDefinitionError::DecimalMetadataForNonDecimal)
            );
        }
    }

    #[test]
    fn constraints_are_nonempty_typed_and_canonically_ordered() {
        let values = NonEmptySet::new(vec![true, false]);
        assert!(values.is_ok());
        if let Ok(values) = values {
            assert_eq!(values.as_slice(), &[false, true]);
        }
        assert_eq!(
            NonEmptySet::<bool>::new(Vec::new()),
            Err(SchemaDefinitionError::EmptySet)
        );
        assert_eq!(
            NonEmptySet::new(vec![true, true]),
            Err(SchemaDefinitionError::DuplicateSetMember)
        );
        assert_eq!(
            InclusiveRange::new(None::<Int>, None),
            Err(SchemaDefinitionError::RangeNeedsBound)
        );
        assert_eq!(
            InclusiveRange::new(Some(Int::new(2)), Some(Int::new(1))),
            Err(SchemaDefinitionError::ReversedRange)
        );

        let lower = InclusiveRange::new(Some(Int::new(-2)), Some(Int::new(2)));
        let bools = NonEmptySet::new(vec![true]);
        assert!(lower.is_ok());
        assert!(bools.is_ok());
        if let (Ok(lower), Ok(bools)) = (lower, bools) {
            let constraints = ConstraintSet::new(vec![
                ValueConstraint::IntRange(lower.clone()),
                ValueConstraint::BoolSet(bools),
            ]);
            assert!(constraints.is_ok());
            if let Ok(constraints) = constraints {
                assert!(matches!(
                    constraints.rules().first(),
                    Some(ValueConstraint::BoolSet(_))
                ));
                assert_eq!(
                    ConstraintSet::new(vec![
                        ValueConstraint::IntRange(lower.clone()),
                        ValueConstraint::IntRange(lower),
                    ])
                    .err(),
                    Some(SchemaDefinitionError::DuplicateConstraint(ValueKind::Int))
                );
            }
        }
    }

    #[test]
    fn predicate_constraints_must_match_its_value_kind() {
        let range = InclusiveRange::new(Some(Int::new(1)), Some(Int::new(4)));
        assert!(range.is_ok());
        if let Ok(range) = range {
            let constraints = ConstraintSet::new(vec![ValueConstraint::IntRange(range)]);
            assert!(constraints.is_ok());
            if let Ok(constraints) = constraints {
                let predicate_id = test_id::<PredicateId>();
                let symbol = Symbol::new("text_value");
                assert!(predicate_id.is_some());
                assert!(symbol.is_ok());
                if let (Some(predicate_id), Ok(symbol)) = (predicate_id, symbol) {
                    let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
                        predicate_id,
                        symbol,
                        subject_constraint: EntityTypeConstraint::AnyEntity,
                        value_kind: ValueKind::String,
                        object_constraint: None,
                        cardinality: Cardinality::Single,
                        resolution_policy: ResolutionPolicy::SingleValueReplace,
                        constraints,
                        decimal_metadata: None,
                        lifecycle: Lifecycle::Active,
                        created_revision: Revision::GENESIS,
                    });
                    assert_eq!(
                        predicate.err(),
                        Some(SchemaDefinitionError::ConstraintValueKindMismatch {
                            field: ValueKind::String,
                            constraint: ValueKind::Int,
                        })
                    );
                }
            }
        }
    }

    #[test]
    fn lifecycle_updates_preserve_schema_ids_and_cannot_reactivate() {
        let entity_type_id = test_id::<EntityTypeId>();
        assert!(entity_type_id.is_some());
        let symbol = Symbol::new("character");
        assert!(symbol.is_ok());
        if let (Some(entity_type_id), Ok(symbol)) = (entity_type_id, symbol) {
            let definition = EntityTypeDefinition::new(
                entity_type_id,
                symbol,
                None,
                Lifecycle::Active,
                Revision::GENESIS,
            );
            assert_eq!(
                definition
                    .revise_lifecycle(Lifecycle::Retired, Revision::FIRST_COMMIT)
                    .err(),
                Some(SchemaDefinitionError::LifecycleRegression)
            );
            assert_eq!(
                definition
                    .revise_lifecycle(Lifecycle::Deprecated, Revision::GENESIS)
                    .err(),
                Some(SchemaDefinitionError::SchemaRevisionNotIncreasing)
            );
            let deprecated =
                definition.revise_lifecycle(Lifecycle::Deprecated, Revision::FIRST_COMMIT);
            assert!(deprecated.is_ok());
            if let Ok(deprecated) = deprecated {
                assert_eq!(deprecated.entity_type_id(), entity_type_id);
                assert_eq!(deprecated.lifecycle(), Lifecycle::Deprecated);
                let revision_two = Revision::new(2);
                assert!(revision_two.is_ok());
                if let Ok(revision_two) = revision_two {
                    assert_eq!(
                        deprecated
                            .revise_lifecycle(Lifecycle::Active, revision_two)
                            .err(),
                        Some(SchemaDefinitionError::LifecycleRegression)
                    );
                }
            }
        }
    }

    #[test]
    fn event_kind_schema_captures_roles_attributes_and_time_constraints() {
        let role_cardinality = RoleCardinality::new(1, Some(2));
        assert!(role_cardinality.is_ok());
        assert_eq!(
            RoleCardinality::new(2, Some(1)),
            Err(SchemaDefinitionError::RoleMaximumBelowMinimum)
        );
        let period = CalendarPeriod::new(0, 1, 0);
        assert!(period.is_ok());
        assert_eq!(
            CalendarPeriod::new(0, 12, 0),
            Err(SchemaDefinitionError::NonCanonicalCalendarMonths(12))
        );
        if let (Ok(role_cardinality), Ok(period)) = (role_cardinality, period) {
            assert_eq!(
                EventTimeConstraint::new(EventTimeForm::InstantOnly, Some(period)),
                Err(SchemaDefinitionError::CalendarSpanOnInstantOnly)
            );
            let time_constraint =
                EventTimeConstraint::new(EventTimeForm::OpenSpanAllowed, Some(period));
            let role_id = test_id::<EventRoleId>();
            let role_symbol = Symbol::new("witness");
            let attribute_id = test_id::<EventAttributeId>();
            let attribute_symbol = Symbol::new("note");
            let event_kind_id = test_id::<EventKindId>();
            let event_kind_symbol = Symbol::new("observation");
            assert!(role_id.is_some());
            assert!(role_symbol.is_ok());
            assert!(attribute_id.is_some());
            assert!(attribute_symbol.is_ok());
            assert!(event_kind_id.is_some());
            assert!(event_kind_symbol.is_ok());
            if let (
                Some(role_id),
                Ok(role_symbol),
                Some(attribute_id),
                Ok(attribute_symbol),
                Some(event_kind_id),
                Ok(event_kind_symbol),
            ) = (
                role_id,
                role_symbol,
                attribute_id,
                attribute_symbol,
                event_kind_id,
                event_kind_symbol,
            ) {
                let role = EventRoleDefinition::new(
                    role_id,
                    role_symbol,
                    EntityTypeConstraint::AnyEntity,
                    role_cardinality,
                );
                let attribute = EventAttributeDefinition::new(
                    attribute_id,
                    attribute_symbol,
                    ValueKind::String,
                    None,
                    ConstraintSet::unconstrained(),
                    None,
                    false,
                );
                assert!(time_constraint.is_ok());
                assert!(attribute.is_ok());
                if let (Ok(time_constraint), Ok(attribute)) = (time_constraint, attribute) {
                    let event_kind = EventKindDefinition::new(
                        event_kind_id,
                        event_kind_symbol,
                        vec![role],
                        vec![attribute],
                        time_constraint,
                        Lifecycle::Active,
                        Revision::GENESIS,
                    );
                    assert!(event_kind.is_ok());
                    if let Ok(event_kind) = event_kind {
                        assert_eq!(event_kind.roles().len(), 1);
                        assert_eq!(event_kind.attributes().len(), 1);
                        assert_eq!(
                            event_kind.event_time_constraint().form(),
                            EventTimeForm::OpenSpanAllowed
                        );
                        let next_revision = event_kind
                            .revise_lifecycle(Lifecycle::Deprecated, Revision::FIRST_COMMIT);
                        assert!(next_revision.is_ok());
                        if let Ok(next_revision) = next_revision {
                            assert_eq!(next_revision.event_kind_id(), event_kind.event_kind_id());
                            assert_eq!(
                                next_revision
                                    .roles()
                                    .first()
                                    .map(EventRoleDefinition::event_role_id),
                                event_kind
                                    .roles()
                                    .first()
                                    .map(EventRoleDefinition::event_role_id)
                            );
                            assert_eq!(
                                next_revision
                                    .attributes()
                                    .first()
                                    .map(EventAttributeDefinition::event_attribute_id),
                                event_kind
                                    .attributes()
                                    .first()
                                    .map(EventAttributeDefinition::event_attribute_id)
                            );
                        }
                    }
                }
            }
        }
    }
}
