//! Canonical derived-index generation envelopes and safe query fallback.

use std::fmt;

use worlddb_core::{
    DecoderLimits, FrameError, FrameHeader, FullScanBudget, IndexAccessPlan, IndexAvailability,
    IndexBuildVersion, IndexFallbackReason, IndexFamily, IndexFormatVersion,
    IndexGenerationMetadata, IndexMetadataError, IndexQueryRequirement, IndexRevisionCoverage,
    IndexSchemaVersion, Revision, TlvDecoder, TlvEncoder, WireError, decode_frame_with_limits,
    encode_frame, plan_index_access,
};

/// Frame kind for one immutable derived-index generation.
pub const INDEX_GENERATION_FRAME_KIND: u32 = 0x5744_4947;

const FAMILY_TAG: u32 = 1;
const GENERATION_TAG: u32 = 2;
const SCHEMA_VERSION_TAG: u32 = 3;
const FORMAT_VERSION_TAG: u32 = 4;
const BUILD_VERSION_TAG: u32 = 5;
const COVERAGE_FROM_TAG: u32 = 6;
const COVERAGE_THROUGH_TAG: u32 = 7;
const PAYLOAD_TAG: u32 = 8;
const INDEX_METADATA_TLV_BYTES: usize = 52;
const FRAME_OVERHEAD_BYTES: usize = 72;

/// A decoded index generation whose metadata and frame checksum were verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexGeneration<'a> {
    metadata: IndexGenerationMetadata,
    payload: &'a [u8],
    file_digest: [u8; 32],
}

impl<'a> IndexGeneration<'a> {
    /// Validated family, versions, generation number, and revision coverage.
    #[must_use]
    pub const fn metadata(self) -> IndexGenerationMetadata {
        self.metadata
    }

    /// Opaque, family-specific derived bytes.
    #[must_use]
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }

    /// BLAKE3 digest of the complete encoded file, suitable for a manifest reference.
    #[must_use]
    pub const fn file_digest(self) -> [u8; 32] {
        self.file_digest
    }
}

/// Result of selecting a verified index file or a safe fallback path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexFileDecision<'a> {
    UseIndex(IndexGeneration<'a>),
    FullScan {
        reason: IndexFallbackReason,
    },
    BudgetExceeded {
        reason: IndexFallbackReason,
        dimension: worlddb_core::BudgetDimension,
    },
}

/// Resource counted while checking an encoded index generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexResource {
    FrameBytes,
    PayloadBytes,
}

/// A malformed, incompatible-to-parse, or oversized index generation file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexGenerationError {
    Frame(FrameError),
    Wire(WireError),
    Metadata(IndexMetadataError),
    UnexpectedFrameKind {
        actual: u32,
    },
    UnknownField {
        tag: u32,
    },
    MissingField {
        tag: u32,
    },
    InvalidFieldWidth {
        tag: u32,
        expected: usize,
        actual: usize,
    },
    InvalidIndexFamily {
        tag: u16,
    },
    InvalidRevision {
        tag: u32,
    },
    ResourceLimitExceeded {
        resource: IndexResource,
        limit: usize,
        actual: usize,
    },
}

impl fmt::Display for IndexGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => write!(formatter, "index generation frame is invalid: {error}"),
            Self::Wire(error) => write!(formatter, "index generation fields are invalid: {error}"),
            Self::Metadata(error) => {
                write!(formatter, "index generation metadata is invalid: {error}")
            }
            Self::UnexpectedFrameKind { actual } => {
                write!(
                    formatter,
                    "unexpected index generation frame kind {actual:#010x}"
                )
            }
            Self::UnknownField { tag } => {
                write!(formatter, "unknown index generation field {tag}")
            }
            Self::MissingField { tag } => {
                write!(
                    formatter,
                    "required index generation field {tag} is missing"
                )
            }
            Self::InvalidFieldWidth {
                tag,
                expected,
                actual,
            } => write!(
                formatter,
                "index generation field {tag} is {actual} bytes; expected {expected}"
            ),
            Self::InvalidIndexFamily { tag } => {
                write!(formatter, "index family tag {tag} is not registered")
            }
            Self::InvalidRevision { tag } => {
                write!(
                    formatter,
                    "index generation field {tag} is not a valid revision"
                )
            }
            Self::ResourceLimitExceeded {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "index generation {resource:?} is {actual} bytes; configured maximum is {limit}"
            ),
        }
    }
}

impl std::error::Error for IndexGenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::Metadata(error) => Some(error),
            Self::UnexpectedFrameKind { .. }
            | Self::UnknownField { .. }
            | Self::MissingField { .. }
            | Self::InvalidFieldWidth { .. }
            | Self::InvalidIndexFamily { .. }
            | Self::InvalidRevision { .. }
            | Self::ResourceLimitExceeded { .. } => None,
        }
    }
}

/// Encodes one canonical generation frame under explicit finite resource limits.
pub fn encode_index_generation(
    metadata: IndexGenerationMetadata,
    payload: &[u8],
    limits: &DecoderLimits,
) -> Result<Vec<u8>, IndexGenerationError> {
    check_payload_limit(payload.len(), limits)?;
    let varint_bytes = encoded_length_width(payload.len());
    let frame_bytes = FRAME_OVERHEAD_BYTES
        .checked_add(INDEX_METADATA_TLV_BYTES)
        .and_then(|size| size.checked_add(1))
        .and_then(|size| size.checked_add(varint_bytes))
        .and_then(|size| size.checked_add(payload.len()))
        .ok_or(IndexGenerationError::ResourceLimitExceeded {
            resource: IndexResource::FrameBytes,
            limit: limits.max_frame_bytes,
            actual: usize::MAX,
        })?;
    if frame_bytes > limits.max_frame_bytes {
        return Err(IndexGenerationError::ResourceLimitExceeded {
            resource: IndexResource::FrameBytes,
            limit: limits.max_frame_bytes,
            actual: frame_bytes,
        });
    }

    let mut fields = TlvEncoder::new();
    let family = metadata.family().wire_tag().to_le_bytes();
    let generation = metadata.generation_id().to_le_bytes();
    let schema_version = [
        metadata.schema_version().major().to_le_bytes(),
        metadata.schema_version().minor().to_le_bytes(),
    ]
    .concat();
    let format_version = [
        metadata.format_version().major().to_le_bytes(),
        metadata.format_version().minor().to_le_bytes(),
    ]
    .concat();
    let build_version = metadata.build_version().value().to_le_bytes();
    let coverage_from = metadata.coverage().from_inclusive().value().to_le_bytes();
    let coverage_through = metadata
        .coverage()
        .through_inclusive()
        .value()
        .to_le_bytes();
    for (tag, value) in [
        (FAMILY_TAG, family.as_slice()),
        (GENERATION_TAG, generation.as_slice()),
        (SCHEMA_VERSION_TAG, schema_version.as_slice()),
        (FORMAT_VERSION_TAG, format_version.as_slice()),
        (BUILD_VERSION_TAG, build_version.as_slice()),
        (COVERAGE_FROM_TAG, coverage_from.as_slice()),
        (COVERAGE_THROUGH_TAG, coverage_through.as_slice()),
        (PAYLOAD_TAG, payload),
    ] {
        fields
            .push(tag, value)
            .map_err(IndexGenerationError::Wire)?;
    }
    encode_frame(
        FrameHeader::new(INDEX_GENERATION_FRAME_KIND),
        &fields.finish(),
    )
    .map_err(IndexGenerationError::Frame)
}

/// Decodes and verifies a generation frame, returning borrowed payload bytes.
pub fn decode_index_generation<'a>(
    bytes: &'a [u8],
    limits: &DecoderLimits,
) -> Result<IndexGeneration<'a>, IndexGenerationError> {
    let frame = decode_frame_with_limits(bytes, limits).map_err(IndexGenerationError::Frame)?;
    if frame.header().kind() != INDEX_GENERATION_FRAME_KIND {
        return Err(IndexGenerationError::UnexpectedFrameKind {
            actual: frame.header().kind(),
        });
    }
    let (metadata, payload) = decode_metadata(frame.payload(), limits)?;
    check_payload_limit(payload.len(), limits)?;
    Ok(IndexGeneration {
        metadata,
        payload,
        file_digest: *blake3::hash(bytes).as_bytes(),
    })
}

/// Selects an index only after checksum, metadata, version, family, and revision checks.
/// Any malformed or incompatible file falls back to a complete scan or an explicit budget
/// error; the payload is exposed only for a generation proven usable by the query.
#[must_use]
pub fn select_index_generation<'a>(
    bytes: Option<&'a [u8]>,
    requirement: IndexQueryRequirement,
    full_scan_budget: FullScanBudget,
    limits: &DecoderLimits,
) -> IndexFileDecision<'a> {
    let Some(bytes) = bytes else {
        return map_plan(
            plan_index_access(IndexAvailability::Missing, requirement, full_scan_budget),
            None,
        );
    };
    match decode_index_generation(bytes, limits) {
        Ok(generation) => map_plan(
            plan_index_access(
                IndexAvailability::Available(generation.metadata()),
                requirement,
                full_scan_budget,
            ),
            Some(generation),
        ),
        Err(_) => map_plan(
            plan_index_access(IndexAvailability::Corrupt, requirement, full_scan_budget),
            None,
        ),
    }
}

fn map_plan(
    plan: IndexAccessPlan,
    generation: Option<IndexGeneration<'_>>,
) -> IndexFileDecision<'_> {
    match plan {
        IndexAccessPlan::Indexed { .. } => {
            generation
                .map(IndexFileDecision::UseIndex)
                .unwrap_or(IndexFileDecision::FullScan {
                    reason: IndexFallbackReason::Corrupt,
                })
        }
        IndexAccessPlan::FullScan { reason } => IndexFileDecision::FullScan { reason },
        IndexAccessPlan::BudgetExceeded { reason, dimension } => {
            IndexFileDecision::BudgetExceeded { reason, dimension }
        }
    }
}

fn decode_metadata<'a>(
    payload: &'a [u8],
    limits: &DecoderLimits,
) -> Result<(IndexGenerationMetadata, &'a [u8]), IndexGenerationError> {
    let mut fields = [None; 8];
    let mut decoder = TlvDecoder::with_limits(payload, *limits);
    while let Some(field) = decoder.next_field().map_err(IndexGenerationError::Wire)? {
        let tag = field.tag();
        let Some(index) = tag
            .checked_sub(1)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Err(IndexGenerationError::UnknownField { tag });
        };
        let Some(slot) = fields.get_mut(index) else {
            return Err(IndexGenerationError::UnknownField { tag });
        };
        *slot = Some(field.value());
    }

    let family_tag = read_u16(FAMILY_TAG, required_field(&fields, FAMILY_TAG)?)?;
    let family = IndexFamily::from_wire_tag(family_tag)
        .ok_or(IndexGenerationError::InvalidIndexFamily { tag: family_tag })?;
    let generation_id = read_u64(GENERATION_TAG, required_field(&fields, GENERATION_TAG)?)?;
    let schema_raw = fixed::<4>(
        SCHEMA_VERSION_TAG,
        required_field(&fields, SCHEMA_VERSION_TAG)?,
    )?;
    let schema_version = IndexSchemaVersion::new(
        u16::from_le_bytes([schema_raw[0], schema_raw[1]]),
        u16::from_le_bytes([schema_raw[2], schema_raw[3]]),
    )
    .map_err(IndexGenerationError::Metadata)?;
    let format_raw = fixed::<4>(
        FORMAT_VERSION_TAG,
        required_field(&fields, FORMAT_VERSION_TAG)?,
    )?;
    let format_version = IndexFormatVersion::new(
        u16::from_le_bytes([format_raw[0], format_raw[1]]),
        u16::from_le_bytes([format_raw[2], format_raw[3]]),
    )
    .map_err(IndexGenerationError::Metadata)?;
    let build_version = IndexBuildVersion::new(read_u32(
        BUILD_VERSION_TAG,
        required_field(&fields, BUILD_VERSION_TAG)?,
    )?)
    .map_err(IndexGenerationError::Metadata)?;
    let from = Revision::new(read_u64(
        COVERAGE_FROM_TAG,
        required_field(&fields, COVERAGE_FROM_TAG)?,
    )?)
    .map_err(|_| IndexGenerationError::InvalidRevision {
        tag: COVERAGE_FROM_TAG,
    })?;
    let through = Revision::new(read_u64(
        COVERAGE_THROUGH_TAG,
        required_field(&fields, COVERAGE_THROUGH_TAG)?,
    )?)
    .map_err(|_| IndexGenerationError::InvalidRevision {
        tag: COVERAGE_THROUGH_TAG,
    })?;
    let coverage =
        IndexRevisionCoverage::new(from, through).map_err(IndexGenerationError::Metadata)?;
    let metadata = IndexGenerationMetadata::new(
        family,
        generation_id,
        schema_version,
        format_version,
        build_version,
        coverage,
    )
    .map_err(IndexGenerationError::Metadata)?;
    let index_payload = required_field(&fields, PAYLOAD_TAG)?;
    Ok((metadata, index_payload))
}

fn required_field<'a>(
    fields: &[Option<&'a [u8]>; 8],
    tag: u32,
) -> Result<&'a [u8], IndexGenerationError> {
    let index = usize::try_from(tag.saturating_sub(1))
        .map_err(|_| IndexGenerationError::MissingField { tag })?;
    fields
        .get(index)
        .and_then(|field| *field)
        .ok_or(IndexGenerationError::MissingField { tag })
}

fn fixed<const N: usize>(tag: u32, bytes: &[u8]) -> Result<[u8; N], IndexGenerationError> {
    bytes
        .try_into()
        .map_err(|_| IndexGenerationError::InvalidFieldWidth {
            tag,
            expected: N,
            actual: bytes.len(),
        })
}

fn read_u16(tag: u32, bytes: &[u8]) -> Result<u16, IndexGenerationError> {
    Ok(u16::from_le_bytes(fixed::<2>(tag, bytes)?))
}

fn read_u32(tag: u32, bytes: &[u8]) -> Result<u32, IndexGenerationError> {
    Ok(u32::from_le_bytes(fixed::<4>(tag, bytes)?))
}

fn read_u64(tag: u32, bytes: &[u8]) -> Result<u64, IndexGenerationError> {
    Ok(u64::from_le_bytes(fixed::<8>(tag, bytes)?))
}

fn encoded_length_width(mut length: usize) -> usize {
    let mut width = 1;
    while length >= 128 {
        length >>= 7;
        width += 1;
    }
    width
}

fn check_payload_limit(actual: usize, limits: &DecoderLimits) -> Result<(), IndexGenerationError> {
    if actual > limits.max_collection_bytes {
        return Err(IndexGenerationError::ResourceLimitExceeded {
            resource: IndexResource::PayloadBytes,
            limit: limits.max_collection_bytes,
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use worlddb_core::{
        DecoderLimits, FullScanBudget, IndexBuildVersion, IndexFallbackReason, IndexFamily,
        IndexFormatVersion, IndexGenerationMetadata, IndexQueryRequirement, IndexRevisionCoverage,
        IndexSchemaVersion, Revision,
    };

    use super::{
        INDEX_GENERATION_FRAME_KIND, IndexFileDecision as FileDecision,
        IndexGenerationError as FileError, IndexResource, decode_index_generation,
        encode_index_generation, select_index_generation,
    };

    #[derive(Debug)]
    enum TestError {
        Revision,
        Metadata,
        Generation,
    }

    impl From<worlddb_core::RevisionError> for TestError {
        fn from(_: worlddb_core::RevisionError) -> Self {
            Self::Revision
        }
    }

    impl From<worlddb_core::IndexMetadataError> for TestError {
        fn from(_: worlddb_core::IndexMetadataError) -> Self {
            Self::Metadata
        }
    }

    impl From<FileError> for TestError {
        fn from(_: FileError) -> Self {
            Self::Generation
        }
    }

    fn revision(value: u64) -> Result<Revision, TestError> {
        Ok(Revision::new(value)?)
    }

    fn metadata(from: u64, through: u64) -> Result<IndexGenerationMetadata, TestError> {
        let coverage = IndexRevisionCoverage::new(revision(from)?, revision(through)?)?;
        Ok(IndexGenerationMetadata::new(
            IndexFamily::AssertionPointHistory,
            7,
            IndexSchemaVersion::V1_0,
            IndexFormatVersion::V1_0,
            IndexBuildVersion::V1,
            coverage,
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
    fn generation_round_trips_closed_metadata_and_payload() -> Result<(), TestError> {
        let encoded = encode_index_generation(
            metadata(2, 9)?,
            b"opaque-family-index-payload",
            &DecoderLimits::DEFAULT,
        )?;
        let decoded = decode_index_generation(&encoded, &DecoderLimits::DEFAULT)?;
        assert_eq!(decoded.metadata(), metadata(2, 9)?);
        assert_eq!(decoded.payload(), b"opaque-family-index-payload");
        assert_eq!(decoded.file_digest(), *blake3::hash(&encoded).as_bytes());
        Ok(())
    }

    #[test]
    fn frame_integrity_failure_is_never_exposed_as_index_payload() -> Result<(), TestError> {
        let mut encoded =
            encode_index_generation(metadata(2, 9)?, b"complete", &DecoderLimits::DEFAULT)?;
        let byte = encoded
            .get_mut(worlddb_core::FRAME_HEADER_LEN + 4)
            .ok_or(FileError::Frame(worlddb_core::FrameError::Truncated))?;
        *byte ^= 0x40;
        assert!(matches!(
            decode_index_generation(&encoded, &DecoderLimits::DEFAULT),
            Err(FileError::Frame(worlddb_core::FrameError::ChecksumMismatch))
        ));
        assert_eq!(
            select_index_generation(
                Some(&encoded),
                requirement(3)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::Corrupt
            }
        );
        assert_eq!(
            select_index_generation(
                Some(&encoded),
                requirement(3)?,
                FullScanBudget::Exceeded(worlddb_core::BudgetDimension::WorkUnits),
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::BudgetExceeded {
                reason: IndexFallbackReason::Corrupt,
                dimension: worlddb_core::BudgetDimension::WorkUnits,
            }
        );
        Ok(())
    }

    #[test]
    fn only_compatible_revision_coverage_exposes_the_payload() -> Result<(), TestError> {
        let encoded =
            encode_index_generation(metadata(2, 9)?, b"complete", &DecoderLimits::DEFAULT)?;
        assert!(matches!(
            select_index_generation(
                Some(&encoded),
                requirement(9)?,
                FullScanBudget::Exceeded(worlddb_core::BudgetDimension::WorkUnits),
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::UseIndex(_)
        ));
        assert_eq!(
            select_index_generation(
                Some(&encoded),
                requirement(10)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::RevisionOutsideCoverage
            }
        );
        Ok(())
    }

    #[test]
    fn missing_file_selects_full_scan_or_explicit_budget_error() -> Result<(), TestError> {
        assert_eq!(
            select_index_generation(
                None,
                requirement(3)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::Missing
            }
        );
        assert_eq!(
            select_index_generation(
                None,
                requirement(3)?,
                FullScanBudget::Exceeded(worlddb_core::BudgetDimension::Candidates),
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::BudgetExceeded {
                reason: IndexFallbackReason::Missing,
                dimension: worlddb_core::BudgetDimension::Candidates,
            }
        );
        Ok(())
    }

    #[test]
    fn version_mismatch_falls_back_without_reading_payload_as_usable() -> Result<(), TestError> {
        let current = metadata(2, 9)?;
        let newer_schema = IndexSchemaVersion::new(2, 0)?;
        let mismatch = IndexGenerationMetadata::new(
            current.family(),
            current.generation_id(),
            newer_schema,
            current.format_version(),
            current.build_version(),
            current.coverage(),
        )?;
        let encoded = encode_index_generation(mismatch, b"do-not-use", &DecoderLimits::DEFAULT)?;
        assert_eq!(
            select_index_generation(
                Some(&encoded),
                requirement(3)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::SchemaVersionMismatch
            }
        );
        Ok(())
    }

    #[test]
    fn malformed_kind_and_unknown_worlddb_frame_version_fall_back() -> Result<(), TestError> {
        let wrong_kind = worlddb_core::encode_frame(
            worlddb_core::FrameHeader::new(INDEX_GENERATION_FRAME_KIND + 1),
            &[],
        )
        .map_err(FileError::Frame)?;
        assert_eq!(
            select_index_generation(
                Some(&wrong_kind),
                requirement(3)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::Corrupt
            }
        );

        let mut future_frame =
            encode_index_generation(metadata(2, 9)?, b"payload", &DecoderLimits::DEFAULT)?;
        if let Some(major) = future_frame.get_mut(8..10) {
            major.copy_from_slice(&2_u16.to_le_bytes());
        }
        let payload_end = future_frame.len() - worlddb_core::CHECKSUM_LEN;
        let payload_bytes = future_frame
            .get(..payload_end)
            .ok_or(FileError::Frame(worlddb_core::FrameError::Truncated))?;
        let checksum = blake3::hash(payload_bytes);
        let checksum_bytes = future_frame
            .get_mut(payload_end..)
            .ok_or(FileError::Frame(worlddb_core::FrameError::Truncated))?;
        checksum_bytes.copy_from_slice(checksum.as_bytes());
        assert_eq!(
            select_index_generation(
                Some(&future_frame),
                requirement(3)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::Corrupt
            }
        );
        Ok(())
    }

    #[test]
    fn generation_encode_and_decode_obey_explicit_finite_limits() -> Result<(), TestError> {
        let payload = [0_u8; 128];
        let generation_metadata = metadata(0, 1)?;
        let bytes =
            encode_index_generation(generation_metadata, &payload, &DecoderLimits::DEFAULT)?;
        let exact_frame_limit = DecoderLimits {
            max_frame_bytes: bytes.len(),
            ..DecoderLimits::DEFAULT
        };
        assert_eq!(
            encode_index_generation(generation_metadata, &payload, &exact_frame_limit)?.len(),
            bytes.len()
        );
        let one_byte_too_small = DecoderLimits {
            max_frame_bytes: bytes.len().saturating_sub(1),
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            encode_index_generation(generation_metadata, &payload, &one_byte_too_small),
            Err(FileError::ResourceLimitExceeded {
                resource: IndexResource::FrameBytes,
                actual,
                ..
            }) if actual == bytes.len()
        ));

        let decode_limits = DecoderLimits {
            max_collection_bytes: 32,
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            decode_index_generation(&bytes, &decode_limits),
            Err(FileError::ResourceLimitExceeded {
                resource: IndexResource::PayloadBytes,
                limit: 32,
                actual: 128
            })
        ));
        assert_eq!(
            select_index_generation(
                Some(&bytes),
                requirement(1)?,
                FullScanBudget::Available,
                &decode_limits,
            ),
            FileDecision::FullScan {
                reason: IndexFallbackReason::Corrupt
            }
        );
        Ok(())
    }

    #[test]
    fn empty_payload_is_a_valid_generation_but_missing_metadata_is_not() -> Result<(), TestError> {
        let bytes = encode_index_generation(metadata(0, 0)?, &[], &DecoderLimits::DEFAULT)?;
        assert_eq!(
            decode_index_generation(&bytes, &DecoderLimits::DEFAULT)?.payload(),
            &[]
        );
        assert_eq!(
            select_index_generation(
                Some(&bytes),
                requirement(0)?,
                FullScanBudget::Available,
                &DecoderLimits::DEFAULT,
            ),
            FileDecision::UseIndex(decode_index_generation(&bytes, &DecoderLimits::DEFAULT)?)
        );
        let no_payload_field = worlddb_core::encode_frame(
            worlddb_core::FrameHeader::new(INDEX_GENERATION_FRAME_KIND),
            &[],
        )
        .map_err(FileError::Frame)?;
        assert!(matches!(
            decode_index_generation(&no_payload_field, &DecoderLimits::DEFAULT),
            Err(FileError::MissingField { tag: 1 })
        ));
        Ok(())
    }
}
