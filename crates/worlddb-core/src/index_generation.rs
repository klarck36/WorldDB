//! Versioned metadata and fail-safe query selection for derived index generations.

use std::fmt;

use crate::ids::Revision;
use crate::query_context::BudgetDimension;

/// Closed 1.0 index families. Wire tags are stable and are never reused.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IndexFamily {
    RecordId,
    OperationId,
    SchemaIdRevision,
    Lifecycle,
    AssertionPointHistory,
    AssertionValidity,
    MaskScopeContextPrecedence,
    EventSearch,
    EventRelation,
    EventMask,
    ProvenanceAdjacency,
}

impl IndexFamily {
    /// Every derived index family recognized by the 1.0 wire registry.
    pub const ALL: [Self; 11] = [
        Self::RecordId,
        Self::OperationId,
        Self::SchemaIdRevision,
        Self::Lifecycle,
        Self::AssertionPointHistory,
        Self::AssertionValidity,
        Self::MaskScopeContextPrecedence,
        Self::EventSearch,
        Self::EventRelation,
        Self::EventMask,
        Self::ProvenanceAdjacency,
    ];

    /// Stable tag used by the derived-index generation format.
    #[must_use]
    pub const fn wire_tag(self) -> u16 {
        match self {
            Self::RecordId => 1,
            Self::OperationId => 2,
            Self::SchemaIdRevision => 3,
            Self::Lifecycle => 4,
            Self::AssertionPointHistory => 5,
            Self::AssertionValidity => 6,
            Self::MaskScopeContextPrecedence => 7,
            Self::EventSearch => 8,
            Self::EventRelation => 9,
            Self::EventMask => 10,
            Self::ProvenanceAdjacency => 11,
        }
    }

    /// Resolves a stable tag, rejecting unregistered or future families.
    #[must_use]
    pub const fn from_wire_tag(tag: u16) -> Option<Self> {
        match tag {
            1 => Some(Self::RecordId),
            2 => Some(Self::OperationId),
            3 => Some(Self::SchemaIdRevision),
            4 => Some(Self::Lifecycle),
            5 => Some(Self::AssertionPointHistory),
            6 => Some(Self::AssertionValidity),
            7 => Some(Self::MaskScopeContextPrecedence),
            8 => Some(Self::EventSearch),
            9 => Some(Self::EventRelation),
            10 => Some(Self::EventMask),
            11 => Some(Self::ProvenanceAdjacency),
            _ => None,
        }
    }
}

/// Version of the logical key/value schema for an index family.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IndexSchemaVersion {
    major: u16,
    minor: u16,
}

impl IndexSchemaVersion {
    /// Initial closed 1.0 index schema.
    pub const V1_0: Self = Self { major: 1, minor: 0 };

    /// Creates a schema version; major zero is reserved.
    pub const fn new(major: u16, minor: u16) -> Result<Self, IndexMetadataError> {
        if major == 0 {
            Err(IndexMetadataError::ZeroMajorVersion)
        } else {
            Ok(Self { major, minor })
        }
    }

    /// Major schema version.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Minor schema version.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// Version of the serialized derived-index generation container.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IndexFormatVersion {
    major: u16,
    minor: u16,
}

impl IndexFormatVersion {
    /// Initial closed 1.0 generation format.
    pub const V1_0: Self = Self { major: 1, minor: 0 };

    /// Creates a format version; major zero is reserved.
    pub const fn new(major: u16, minor: u16) -> Result<Self, IndexMetadataError> {
        if major == 0 {
            Err(IndexMetadataError::ZeroMajorVersion)
        } else {
            Ok(Self { major, minor })
        }
    }

    /// Major format version.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Minor format version.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// Version of the deterministic builder algorithm for an index family.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IndexBuildVersion(u32);

impl IndexBuildVersion {
    /// Initial deterministic builder version.
    pub const V1: Self = Self(1);

    /// Creates a nonzero builder version.
    pub const fn new(value: u32) -> Result<Self, IndexMetadataError> {
        if value == 0 {
            Err(IndexMetadataError::ZeroBuildVersion)
        } else {
            Ok(Self(value))
        }
    }

    /// Numeric builder version.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Inclusive, gap-free revision interval represented by one index generation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IndexRevisionCoverage {
    from_inclusive: Revision,
    through_inclusive: Revision,
}

impl IndexRevisionCoverage {
    /// Creates a nonempty inclusive revision interval.
    pub const fn new(
        from_inclusive: Revision,
        through_inclusive: Revision,
    ) -> Result<Self, IndexMetadataError> {
        if from_inclusive.value() > through_inclusive.value() {
            Err(IndexMetadataError::ReversedRevisionCoverage {
                from: from_inclusive,
                through: through_inclusive,
            })
        } else {
            Ok(Self {
                from_inclusive,
                through_inclusive,
            })
        }
    }

    /// First revision included in the generation.
    #[must_use]
    pub const fn from_inclusive(self) -> Revision {
        self.from_inclusive
    }

    /// Last revision included in the generation.
    #[must_use]
    pub const fn through_inclusive(self) -> Revision {
        self.through_inclusive
    }

    /// Whether a requested revision is fully represented.
    #[must_use]
    pub const fn contains(self, revision: Revision) -> bool {
        revision.value() >= self.from_inclusive.value()
            && revision.value() <= self.through_inclusive.value()
    }
}

/// Validated metadata shared by the core planner and file-format adapter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IndexGenerationMetadata {
    family: IndexFamily,
    generation_id: u64,
    schema_version: IndexSchemaVersion,
    format_version: IndexFormatVersion,
    build_version: IndexBuildVersion,
    coverage: IndexRevisionCoverage,
}

impl IndexGenerationMetadata {
    /// Creates a complete descriptor; generation zero is reserved for absence.
    pub const fn new(
        family: IndexFamily,
        generation_id: u64,
        schema_version: IndexSchemaVersion,
        format_version: IndexFormatVersion,
        build_version: IndexBuildVersion,
        coverage: IndexRevisionCoverage,
    ) -> Result<Self, IndexMetadataError> {
        if generation_id == 0 {
            return Err(IndexMetadataError::ZeroGeneration);
        }
        Ok(Self {
            family,
            generation_id,
            schema_version,
            format_version,
            build_version,
            coverage,
        })
    }

    /// Index family carried by the generation.
    #[must_use]
    pub const fn family(self) -> IndexFamily {
        self.family
    }

    /// Nonzero generation identifier within the family.
    #[must_use]
    pub const fn generation_id(self) -> u64 {
        self.generation_id
    }

    /// Logical index key/value schema version.
    #[must_use]
    pub const fn schema_version(self) -> IndexSchemaVersion {
        self.schema_version
    }

    /// Serialized generation-container version.
    #[must_use]
    pub const fn format_version(self) -> IndexFormatVersion {
        self.format_version
    }

    /// Deterministic builder algorithm version.
    #[must_use]
    pub const fn build_version(self) -> IndexBuildVersion {
        self.build_version
    }

    /// Exact revision interval represented by the generation.
    #[must_use]
    pub const fn coverage(self) -> IndexRevisionCoverage {
        self.coverage
    }
}

/// Invalid metadata or coverage supplied while defining a generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexMetadataError {
    ZeroMajorVersion,
    ZeroBuildVersion,
    ZeroGeneration,
    ReversedRevisionCoverage { from: Revision, through: Revision },
}

impl fmt::Display for IndexMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMajorVersion => formatter.write_str("index major version must be nonzero"),
            Self::ZeroBuildVersion => formatter.write_str("index build version must be nonzero"),
            Self::ZeroGeneration => formatter.write_str("index generation zero is reserved"),
            Self::ReversedRevisionCoverage { from, through } => {
                write!(formatter, "index coverage starts at {from} after {through}")
            }
        }
    }
}

impl std::error::Error for IndexMetadataError {}

/// Compatibility requirements for one indexed query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexQueryRequirement {
    family: IndexFamily,
    revision: Revision,
    schema_version: IndexSchemaVersion,
    format_version: IndexFormatVersion,
    build_version: IndexBuildVersion,
}

impl IndexQueryRequirement {
    /// Binds one query to an index family, exact revision, and supported versions.
    #[must_use]
    pub const fn new(
        family: IndexFamily,
        revision: Revision,
        schema_version: IndexSchemaVersion,
        format_version: IndexFormatVersion,
        build_version: IndexBuildVersion,
    ) -> Self {
        Self {
            family,
            revision,
            schema_version,
            format_version,
            build_version,
        }
    }
}

/// Result of locating the requested derived index on disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexAvailability {
    Available(IndexGenerationMetadata),
    Missing,
    Corrupt,
}

/// Why an index cannot safely answer the requested query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexFallbackReason {
    Missing,
    Corrupt,
    FamilyMismatch,
    SchemaVersionMismatch,
    FormatVersionMismatch,
    BuildVersionMismatch,
    RevisionOutsideCoverage,
}

/// Full-scan budget assessment made by the query layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullScanBudget {
    Available,
    Exceeded(BudgetDimension),
}

/// Safe query path selected after checking index state and compatibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexAccessPlan {
    Indexed {
        generation_id: u64,
    },
    FullScan {
        reason: IndexFallbackReason,
    },
    BudgetExceeded {
        reason: IndexFallbackReason,
        dimension: BudgetDimension,
    },
}

/// Selects only an exact compatible index; otherwise requests a full scan or
/// returns an explicit budget failure. It never returns partial index results.
#[must_use]
pub fn plan_index_access(
    availability: IndexAvailability,
    requirement: IndexQueryRequirement,
    full_scan_budget: FullScanBudget,
) -> IndexAccessPlan {
    let reason = match availability {
        IndexAvailability::Available(metadata) => {
            let Some(reason) = incompatible_reason(metadata, requirement) else {
                return IndexAccessPlan::Indexed {
                    generation_id: metadata.generation_id,
                };
            };
            reason
        }
        IndexAvailability::Missing => IndexFallbackReason::Missing,
        IndexAvailability::Corrupt => IndexFallbackReason::Corrupt,
    };
    match full_scan_budget {
        FullScanBudget::Available => IndexAccessPlan::FullScan { reason },
        FullScanBudget::Exceeded(dimension) => {
            IndexAccessPlan::BudgetExceeded { reason, dimension }
        }
    }
}

fn incompatible_reason(
    metadata: IndexGenerationMetadata,
    requirement: IndexQueryRequirement,
) -> Option<IndexFallbackReason> {
    if metadata.family != requirement.family {
        return Some(IndexFallbackReason::FamilyMismatch);
    }
    if metadata.schema_version != requirement.schema_version {
        return Some(IndexFallbackReason::SchemaVersionMismatch);
    }
    if metadata.format_version != requirement.format_version {
        return Some(IndexFallbackReason::FormatVersionMismatch);
    }
    if metadata.build_version != requirement.build_version {
        return Some(IndexFallbackReason::BuildVersionMismatch);
    }
    if !metadata.coverage.contains(requirement.revision) {
        return Some(IndexFallbackReason::RevisionOutsideCoverage);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        FullScanBudget, IndexAccessPlan, IndexAvailability, IndexBuildVersion, IndexFallbackReason,
        IndexFamily, IndexFormatVersion, IndexGenerationMetadata, IndexMetadataError,
        IndexQueryRequirement, IndexRevisionCoverage, IndexSchemaVersion, plan_index_access,
    };
    use crate::ids::Revision;
    use crate::query_context::BudgetDimension;

    #[derive(Debug)]
    enum TestError {
        Revision,
        Metadata,
    }

    impl From<crate::ids::RevisionError> for TestError {
        fn from(_: crate::ids::RevisionError) -> Self {
            Self::Revision
        }
    }

    impl From<IndexMetadataError> for TestError {
        fn from(_: IndexMetadataError) -> Self {
            Self::Metadata
        }
    }

    fn revision(value: u64) -> Result<Revision, TestError> {
        Ok(Revision::new(value)?)
    }

    fn metadata(
        family: IndexFamily,
        generation: u64,
        schema: IndexSchemaVersion,
        format: IndexFormatVersion,
        build: IndexBuildVersion,
        from: u64,
        through: u64,
    ) -> Result<IndexGenerationMetadata, TestError> {
        let coverage = IndexRevisionCoverage::new(revision(from)?, revision(through)?)?;
        Ok(IndexGenerationMetadata::new(
            family, generation, schema, format, build, coverage,
        )?)
    }

    fn requirement(revision_value: u64) -> Result<IndexQueryRequirement, TestError> {
        Ok(IndexQueryRequirement::new(
            IndexFamily::AssertionPointHistory,
            revision(revision_value)?,
            IndexSchemaVersion::V1_0,
            IndexFormatVersion::V1_0,
            IndexBuildVersion::V1,
        ))
    }

    #[test]
    fn exactly_compatible_generation_covers_both_inclusive_boundaries() -> Result<(), TestError> {
        let generation = metadata(
            IndexFamily::AssertionPointHistory,
            3,
            IndexSchemaVersion::V1_0,
            IndexFormatVersion::V1_0,
            IndexBuildVersion::V1,
            4,
            8,
        )?;
        for revision_value in [4, 8] {
            assert_eq!(
                plan_index_access(
                    IndexAvailability::Available(generation),
                    requirement(revision_value)?,
                    FullScanBudget::Exceeded(BudgetDimension::WorkUnits),
                ),
                IndexAccessPlan::Indexed { generation_id: 3 }
            );
        }
        Ok(())
    }

    #[test]
    fn missing_corrupt_stale_or_incompatible_index_falls_back_completely() -> Result<(), TestError>
    {
        let good = metadata(
            IndexFamily::AssertionPointHistory,
            1,
            IndexSchemaVersion::V1_0,
            IndexFormatVersion::V1_0,
            IndexBuildVersion::V1,
            4,
            8,
        )?;
        let cases = [
            (
                IndexAvailability::Missing,
                requirement(5)?,
                IndexFallbackReason::Missing,
            ),
            (
                IndexAvailability::Corrupt,
                requirement(5)?,
                IndexFallbackReason::Corrupt,
            ),
            (
                IndexAvailability::Available(good),
                requirement(9)?,
                IndexFallbackReason::RevisionOutsideCoverage,
            ),
            (
                IndexAvailability::Available(metadata(
                    IndexFamily::RecordId,
                    2,
                    IndexSchemaVersion::V1_0,
                    IndexFormatVersion::V1_0,
                    IndexBuildVersion::V1,
                    4,
                    8,
                )?),
                requirement(5)?,
                IndexFallbackReason::FamilyMismatch,
            ),
            (
                IndexAvailability::Available(metadata(
                    IndexFamily::AssertionPointHistory,
                    2,
                    IndexSchemaVersion::new(2, 0)?,
                    IndexFormatVersion::V1_0,
                    IndexBuildVersion::V1,
                    4,
                    8,
                )?),
                requirement(5)?,
                IndexFallbackReason::SchemaVersionMismatch,
            ),
            (
                IndexAvailability::Available(metadata(
                    IndexFamily::AssertionPointHistory,
                    2,
                    IndexSchemaVersion::V1_0,
                    IndexFormatVersion::new(2, 0)?,
                    IndexBuildVersion::V1,
                    4,
                    8,
                )?),
                requirement(5)?,
                IndexFallbackReason::FormatVersionMismatch,
            ),
            (
                IndexAvailability::Available(metadata(
                    IndexFamily::AssertionPointHistory,
                    2,
                    IndexSchemaVersion::V1_0,
                    IndexFormatVersion::V1_0,
                    IndexBuildVersion::new(2)?,
                    4,
                    8,
                )?),
                requirement(5)?,
                IndexFallbackReason::BuildVersionMismatch,
            ),
        ];

        for (availability, requested, reason) in cases {
            assert_eq!(
                plan_index_access(availability, requested, FullScanBudget::Available),
                IndexAccessPlan::FullScan { reason }
            );
        }
        Ok(())
    }

    #[test]
    fn exhausted_full_scan_budget_is_an_explicit_error_path() -> Result<(), TestError> {
        let decision = plan_index_access(
            IndexAvailability::Missing,
            requirement(5)?,
            FullScanBudget::Exceeded(BudgetDimension::Candidates),
        );
        assert_eq!(
            decision,
            IndexAccessPlan::BudgetExceeded {
                reason: IndexFallbackReason::Missing,
                dimension: BudgetDimension::Candidates,
            }
        );
        assert!(!matches!(decision, IndexAccessPlan::Indexed { .. }));
        Ok(())
    }

    #[test]
    fn generation_metadata_rejects_reserved_or_reversed_values() -> Result<(), TestError> {
        let reversed = IndexRevisionCoverage::new(revision(9)?, revision(8)?);
        assert_eq!(
            reversed,
            Err(IndexMetadataError::ReversedRevisionCoverage {
                from: revision(9)?,
                through: revision(8)?,
            })
        );
        let valid_coverage = IndexRevisionCoverage::new(revision(0)?, revision(8)?)?;
        assert_eq!(
            IndexGenerationMetadata::new(
                IndexFamily::RecordId,
                0,
                IndexSchemaVersion::V1_0,
                IndexFormatVersion::V1_0,
                IndexBuildVersion::V1,
                valid_coverage,
            ),
            Err(IndexMetadataError::ZeroGeneration)
        );
        assert_eq!(
            IndexSchemaVersion::new(0, 1),
            Err(IndexMetadataError::ZeroMajorVersion)
        );
        assert_eq!(
            IndexFormatVersion::new(0, 1),
            Err(IndexMetadataError::ZeroMajorVersion)
        );
        assert_eq!(
            IndexBuildVersion::new(0),
            Err(IndexMetadataError::ZeroBuildVersion)
        );
        Ok(())
    }

    #[test]
    fn family_wire_tags_are_closed_and_stable() {
        let families = [
            IndexFamily::RecordId,
            IndexFamily::OperationId,
            IndexFamily::SchemaIdRevision,
            IndexFamily::Lifecycle,
            IndexFamily::AssertionPointHistory,
            IndexFamily::AssertionValidity,
            IndexFamily::MaskScopeContextPrecedence,
            IndexFamily::EventSearch,
            IndexFamily::EventRelation,
            IndexFamily::EventMask,
            IndexFamily::ProvenanceAdjacency,
        ];
        for (index, family) in families.into_iter().enumerate() {
            let expected = u16::try_from(index + 1).unwrap_or_default();
            assert_eq!(family.wire_tag(), expected);
            assert_eq!(IndexFamily::from_wire_tag(expected), Some(family));
        }
        assert_eq!(IndexFamily::from_wire_tag(0), None);
        assert_eq!(IndexFamily::from_wire_tag(12), None);
    }
}
