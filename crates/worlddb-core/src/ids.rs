//! Validated identity and revision types from Master §3.1.

use std::fmt;
use std::hash::Hash;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::{Builder, Uuid, Variant};

const UUID_BYTES: usize = 16;
#[allow(dead_code, reason = "WDB-EXC-0001")]
const UUID_V7_TIMESTAMP_MAX: u64 = (1_u64 << 48) - 1;

/// Namespace in which an identity is unique.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IdNamespace {
    /// Database identity space.
    Database,
    /// Project-wide identity space.
    Project,
    /// Project schema identity space.
    ProjectSchema,
    /// Engine session identity space.
    EngineSession,
    /// Storage backend identity space.
    Backend,
    /// Audit subsystem identity space.
    Audit,
    /// Security identity space.
    Security,
}

/// Persistence boundary recorded for each public identity type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IdPersistence {
    /// Stored as database domain history or metadata.
    Database,
    /// Stored as project catalog metadata.
    Project,
    /// Stored as project schema history.
    ProjectSchema,
    /// Stored in commit metadata or the write-ahead log.
    CommitMetadataAndWal,
    /// Stored in the operation deduplication index.
    DedupIndex,
    /// Stored in the backend file format.
    BackendFile,
    /// Stored by the separate audit subsystem.
    AuditSubsystem,
    /// Stored while a resumable job is active.
    ResumableJob,
    /// Stored in security history.
    SecurityHistory,
    /// Exists only for one engine session and is not persistable.
    SessionLocal,
}

/// Wire boundary recorded for each identity and revision type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IdWire {
    /// Canonical lowercase UUID text or the 16-byte UUID representation.
    CanonicalUuid,
    /// UUID representation scoped to the engine session.
    SessionLocalUuid,
    /// UUID representation owned by the storage file format.
    FileFormatUuid,
    /// UUID representation owned by the audit format.
    AuditFormatUuid,
    /// UUID representation exposed only through security-filtered APIs.
    SecurityFilteredUuid,
    /// Canonical decimal string on text transports and `u64` in binary formats.
    CanonicalDecimalString,
}

/// Persistence and wire scope of one identity type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IdScope {
    /// Rust type name for this entry.
    pub type_name: &'static str,
    /// Namespace that owns the identity.
    pub namespace: IdNamespace,
    /// Persistence boundary.
    pub persistence: IdPersistence,
    /// Wire boundary.
    pub wire: IdWire,
    /// Whether the type is part of a public WorldDB API.
    pub public_api: bool,
}

impl IdScope {
    /// Returns whether this scope permits persistence.
    #[must_use]
    pub const fn is_persistent(self) -> bool {
        !matches!(self.persistence, IdPersistence::SessionLocal)
    }
}

mod sealed {
    /// Prevents callers from implementing identity traits for unchecked types.
    pub trait DomainId {}

    /// Restricts persistence eligibility to registered identity types.
    pub trait PersistentId {}

    /// Restricts wire eligibility to registered identity types.
    pub trait WireId {}
}

/// A registered, validated 128-bit WorldDB identity.
///
/// UUID timestamps are metadata from the generator only. They do not establish
/// domain ordering, WorldDB time, authenticity, or authorization.
pub trait DomainId:
    sealed::DomainId + Copy + Eq + fmt::Debug + fmt::Display + Hash + Ord + Sized
{
    /// Scope assigned to this concrete identity type.
    const SCOPE: IdScope;

    /// Validates and constructs an identity from its exact 16-byte form.
    fn try_from_bytes(bytes: [u8; UUID_BYTES]) -> Result<Self, IdValidationError>;

    /// Returns the exact UUID bytes in network order.
    fn as_bytes(&self) -> &[u8; UUID_BYTES];

    /// Copies the exact UUID bytes in network order.
    fn to_bytes(self) -> [u8; UUID_BYTES];

    /// Returns the canonical lowercase UUID text.
    fn to_canonical_string(self) -> String {
        Uuid::from_bytes(self.to_bytes()).to_string()
    }
}

/// Marker for registered identities that may be written to persistent storage.
///
/// The trait is sealed. `SnapshotId` intentionally does not implement it.
pub trait PersistentId: DomainId + sealed::PersistentId {}

/// Marker for registered identities with a defined wire representation.
pub trait WireId: DomainId + sealed::WireId {}

/// Invalid 128-bit identity input.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IdValidationError {
    /// Input was not a canonical lowercase UUID string.
    InvalidText,
    /// UUID text parsed but used a noncanonical spelling.
    NonCanonicalText,
    /// The all-zero UUID is reserved and cannot identify an object.
    ReservedZero,
    /// The all-`ff` UUID is reserved and cannot identify an object.
    ReservedAllFf,
    /// UUID variant is not the RFC variant used by WorldDB IDs.
    InvalidVariant,
    /// UUID version is not a standardized RFC version from 1 through 8.
    InvalidVersion,
}

impl fmt::Display for IdValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidText => formatter.write_str("invalid UUID text"),
            Self::NonCanonicalText => formatter.write_str("UUID text is not canonical lowercase"),
            Self::ReservedZero => formatter.write_str("all-zero UUID is reserved"),
            Self::ReservedAllFf => formatter.write_str("all-ff UUID is reserved"),
            Self::InvalidVariant => formatter.write_str("UUID variant is not RFC 9562"),
            Self::InvalidVersion => formatter.write_str("UUID version is not standardized"),
        }
    }
}

impl std::error::Error for IdValidationError {}

/// Fallible UUIDv7 generation errors.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IdGenerationError {
    /// System clock predates the Unix epoch.
    ClockBeforeUnixEpoch,
    /// The timestamp cannot be represented by UUIDv7's 48-bit millisecond field.
    TimestampOutOfRange,
    /// Operating-system cryptographic randomness was unavailable.
    EntropyUnavailable,
    /// The UUID builder unexpectedly produced bytes that failed WorldDB validation.
    InvalidGeneratedId(IdValidationError),
}

impl fmt::Display for IdGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClockBeforeUnixEpoch => formatter.write_str("system clock predates Unix epoch"),
            Self::TimestampOutOfRange => {
                formatter.write_str("system clock exceeds UUIDv7 timestamp range")
            }
            Self::EntropyUnavailable => {
                formatter.write_str("operating-system cryptographic randomness unavailable")
            }
            Self::InvalidGeneratedId(error) => {
                write!(formatter, "UUIDv7 builder returned invalid bytes: {error}")
            }
        }
    }
}

impl std::error::Error for IdGenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidGeneratedId(error) => Some(error),
            Self::ClockBeforeUnixEpoch | Self::TimestampOutOfRange | Self::EntropyUnavailable => {
                None
            }
        }
    }
}

fn validate_uuid_bytes(bytes: [u8; UUID_BYTES]) -> Result<(), IdValidationError> {
    let uuid = Uuid::from_bytes(bytes);
    if uuid.is_nil() {
        return Err(IdValidationError::ReservedZero);
    }
    if uuid.as_u128() == u128::MAX {
        return Err(IdValidationError::ReservedAllFf);
    }
    if uuid.get_variant() != Variant::RFC4122 {
        return Err(IdValidationError::InvalidVariant);
    }
    if !(1..=8).contains(&uuid.get_version_num()) {
        return Err(IdValidationError::InvalidVersion);
    }
    Ok(())
}

macro_rules! persistence_marker {
    ($id_type:ident, persistent) => {
        impl sealed::PersistentId for $id_type {}
        impl PersistentId for $id_type {}
    };
    ($id_type:ident, session_local) => {};
}

macro_rules! define_domain_ids {
    ($(($id_type:ident, $namespace:ident, $persistence:ident, $wire:ident, $public_api:literal, $persistence_trait:ident)),+ $(,)?) => {
        $(
            /// A distinct validated WorldDB UUID identity type.
            #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
            pub struct $id_type([u8; UUID_BYTES]);

            impl $id_type {
                /// Persistence and wire scope of this identity type.
                pub const SCOPE: IdScope = IdScope {
                    type_name: stringify!($id_type),
                    namespace: IdNamespace::$namespace,
                    persistence: IdPersistence::$persistence,
                    wire: IdWire::$wire,
                    public_api: $public_api,
                };
            }

            impl sealed::DomainId for $id_type {}
            impl sealed::WireId for $id_type {}
            impl WireId for $id_type {}
            persistence_marker!($id_type, $persistence_trait);

            impl DomainId for $id_type {
                const SCOPE: IdScope = Self::SCOPE;

                fn try_from_bytes(bytes: [u8; UUID_BYTES]) -> Result<Self, IdValidationError> {
                    validate_uuid_bytes(bytes)?;
                    Ok(Self(bytes))
                }

                fn as_bytes(&self) -> &[u8; UUID_BYTES] {
                    &self.0
                }

                fn to_bytes(self) -> [u8; UUID_BYTES] {
                    self.0
                }
            }

            impl TryFrom<[u8; UUID_BYTES]> for $id_type {
                type Error = IdValidationError;

                fn try_from(bytes: [u8; UUID_BYTES]) -> Result<Self, Self::Error> {
                    Self::try_from_bytes(bytes)
                }
            }

            impl FromStr for $id_type {
                type Err = IdValidationError;

                fn from_str(value: &str) -> Result<Self, Self::Err> {
                    let uuid = Uuid::parse_str(value).map_err(|_| IdValidationError::InvalidText)?;
                    if uuid.to_string() != value {
                        return Err(IdValidationError::NonCanonicalText);
                    }
                    Self::try_from_bytes(uuid.into_bytes())
                }
            }

            impl fmt::Display for $id_type {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(formatter, "{}", Uuid::from_bytes(self.0))
                }
            }

            impl fmt::Debug for $id_type {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.debug_tuple(stringify!($id_type)).field(&self.to_string()).finish()
                }
            }
        )+

        /// Complete registered 128-bit identity taxonomy and its storage/wire scopes.
        pub const DOMAIN_ID_SCOPES: &[IdScope] = &[$($id_type::SCOPE),+];
    };
}

define_domain_ids!(
    (
        DatabaseId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        HistorySpaceId,
        Project,
        Project,
        CanonicalUuid,
        true,
        persistent
    ),
    (LayerId, Project, Project, CanonicalUuid, true, persistent),
    (
        PerspectiveId,
        Project,
        Project,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        TimelineId,
        Project,
        Project,
        CanonicalUuid,
        true,
        persistent
    ),
    (EntityId, Project, Project, CanonicalUuid, true, persistent),
    (
        PredicateId,
        Project,
        Project,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EntityTypeId,
        Project,
        Project,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        AssertionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (MaskId, Database, Database, CanonicalUuid, true, persistent),
    (
        ReplacementBoundaryId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (EventId, Database, Database, CanonicalUuid, true, persistent),
    (
        EventMaskId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventRelationId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventKindId,
        ProjectSchema,
        ProjectSchema,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventRoleId,
        ProjectSchema,
        ProjectSchema,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventAttributeId,
        ProjectSchema,
        ProjectSchema,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        SourceId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EvidenceId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        ProvenanceId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        AssertionValidityClosureId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        AssertionRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        MaskValidityClosureId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        MaskRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        ReplacementBoundaryValidityClosureId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        ReplacementBoundaryRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventSpanClosureId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventMaskRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EventRelationRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EvidenceRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        ProvenanceRetractionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        EntityRetirementId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        PerspectiveRetirementId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        ArchiveTransitionId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        MigrationId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        MigrationRunId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        MigrationStepId,
        Database,
        Database,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        TransactionId,
        Database,
        CommitMetadataAndWal,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        OperationId,
        Database,
        DedupIndex,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        SnapshotId,
        EngineSession,
        SessionLocal,
        SessionLocalUuid,
        true,
        session_local
    ),
    (
        SegmentId,
        Backend,
        BackendFile,
        FileFormatUuid,
        false,
        persistent
    ),
    (
        ClientRequestId,
        Audit,
        AuditSubsystem,
        AuditFormatUuid,
        true,
        persistent
    ),
    (
        AuditRecordId,
        Audit,
        AuditSubsystem,
        AuditFormatUuid,
        true,
        persistent
    ),
    (
        AuditOperationId,
        Audit,
        AuditSubsystem,
        AuditFormatUuid,
        true,
        persistent
    ),
    (
        JobId,
        Database,
        ResumableJob,
        CanonicalUuid,
        true,
        persistent
    ),
    (
        PrincipalId,
        Security,
        SecurityHistory,
        SecurityFilteredUuid,
        true,
        persistent
    ),
    (
        SecurityPolicyRecordId,
        Security,
        SecurityHistory,
        SecurityFilteredUuid,
        true,
        persistent
    ),
    (
        RoleId,
        Security,
        SecurityHistory,
        SecurityFilteredUuid,
        true,
        persistent
    ),
    (
        RoleAssignmentId,
        Security,
        SecurityHistory,
        SecurityFilteredUuid,
        true,
        persistent
    ),
    (
        PolicyRuleId,
        Security,
        SecurityHistory,
        SecurityFilteredUuid,
        true,
        persistent
    ),
);

/// Creates one UUIDv7 identity using the system clock and OS cryptographic entropy.
///
/// This function remains inside the private `ids` module so callers receive IDs
/// through product actions, not through a free-standing generator API.
#[allow(dead_code, reason = "WDB-EXC-0001")]
pub fn generate_id<T: DomainId>() -> Result<T, IdGenerationError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IdGenerationError::ClockBeforeUnixEpoch)?
        .as_millis();
    let timestamp = u64::try_from(timestamp).map_err(|_| IdGenerationError::TimestampOutOfRange)?;
    let mut entropy = [0_u8; 10];
    getrandom::fill(&mut entropy).map_err(|_| IdGenerationError::EntropyUnavailable)?;
    let bytes = build_uuid_v7(timestamp, &entropy)?;
    T::try_from_bytes(bytes).map_err(IdGenerationError::InvalidGeneratedId)
}

#[allow(dead_code, reason = "WDB-EXC-0001")]
fn build_uuid_v7(
    timestamp_millis: u64,
    entropy: &[u8; 10],
) -> Result<[u8; UUID_BYTES], IdGenerationError> {
    if timestamp_millis > UUID_V7_TIMESTAMP_MAX {
        return Err(IdGenerationError::TimestampOutOfRange);
    }
    let bytes = Builder::from_unix_timestamp_millis(timestamp_millis, entropy)
        .into_uuid()
        .into_bytes();
    validate_uuid_bytes(bytes).map_err(IdGenerationError::InvalidGeneratedId)?;
    Ok(bytes)
}

/// Validated database revision on the shared data/schema revision axis.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(u64);

impl Revision {
    /// Empty database genesis revision.
    pub const GENESIS: Self = Self(0);
    /// First published commit revision.
    pub const FIRST_COMMIT: Self = Self(1);
    /// Scope used by durable storage and wire encoders.
    pub const SCOPE: IdScope = IdScope {
        type_name: "Revision",
        namespace: IdNamespace::Database,
        persistence: IdPersistence::Database,
        wire: IdWire::CanonicalDecimalString,
        public_api: true,
    };

    /// Constructs a revision, reserving `u64::MAX` for overflow safety.
    pub const fn new(value: u64) -> Result<Self, RevisionError> {
        if value == u64::MAX {
            Err(RevisionError::ReservedMaximum)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the underlying revision number.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Returns the next publishable revision or a typed overflow error.
    pub const fn next_commit(self) -> Result<Self, RevisionError> {
        match self.0.checked_add(1) {
            Some(value) => Self::new(value),
            None => Err(RevisionError::Overflow),
        }
    }
}

impl TryFrom<u64> for Revision {
    type Error = RevisionError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for Revision {
    type Err = RevisionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(RevisionError::NonCanonicalDecimal);
        }
        let number = value
            .parse::<u64>()
            .map_err(|_| RevisionError::IntegerOverflow)?;
        Self::new(number)
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Invalid revision input or exhausted revision space.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RevisionError {
    /// Text is not the canonical unsigned decimal form.
    NonCanonicalDecimal,
    /// Decimal input does not fit in `u64`.
    IntegerOverflow,
    /// `u64::MAX` is reserved and cannot be published.
    ReservedMaximum,
    /// No next revision can be represented.
    Overflow,
}

impl fmt::Display for RevisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonCanonicalDecimal => formatter.write_str("revision is not canonical decimal"),
            Self::IntegerOverflow => formatter.write_str("revision exceeds u64"),
            Self::ReservedMaximum => formatter.write_str("u64::MAX is reserved for overflow"),
            Self::Overflow => formatter.write_str("no next revision is representable"),
        }
    }
}

impl std::error::Error for RevisionError {}

/// Semantic schema position on the database's shared `Revision` axis.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaRevision(Revision);

impl SchemaRevision {
    /// Scope used by durable storage and wire encoders.
    pub const SCOPE: IdScope = IdScope {
        type_name: "SchemaRevision",
        namespace: IdNamespace::Database,
        persistence: IdPersistence::Database,
        wire: IdWire::CanonicalDecimalString,
        public_api: true,
    };

    /// Tags a revision known by the caller to have been published as schema history.
    ///
    /// The existence check belongs to the history reader/writer; this type has no
    /// independent counter and shares the `Revision` numeric axis.
    #[must_use]
    pub const fn from_published_revision(revision: Revision) -> Self {
        Self(revision)
    }

    /// Returns the shared revision-axis value.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.0
    }
}

impl FromStr for SchemaRevision {
    type Err = RevisionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<Revision>().map(Self::from_published_revision)
    }
}

impl fmt::Display for SchemaRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Monotonic security-policy version on the shared revision history.
///
/// Zero is the initial policy snapshot. The maximum value is valid; advancing
/// beyond it returns an explicit exhaustion error instead of wrapping.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecurityEpoch(u64);

impl SecurityEpoch {
    /// Initial epoch established with the first project policy snapshot.
    pub const INITIAL: Self = Self(0);

    /// Scope assigned to persisted security-policy versions.
    pub const SCOPE: IdScope = IdScope {
        type_name: "SecurityEpoch",
        namespace: IdNamespace::Security,
        persistence: IdPersistence::SecurityHistory,
        wire: IdWire::CanonicalDecimalString,
        public_api: true,
    };

    /// Constructs an epoch value without reserving a numeric sentinel.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric epoch.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Advances once or reports that the unsigned epoch space is exhausted.
    pub const fn next(self) -> Result<Self, SecurityEpochError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(SecurityEpochError::Exhausted),
        }
    }
}

impl fmt::Display for SecurityEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Failure to advance the security-policy version.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SecurityEpochError {
    /// The epoch reached the largest representable unsigned value.
    Exhausted,
}

impl fmt::Display for SecurityEpochError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted => formatter.write_str("security epoch is exhausted"),
        }
    }
}

impl std::error::Error for SecurityEpochError {}

#[cfg(test)]
mod tests {
    use super::{
        AssertionId, ClientRequestId, DOMAIN_ID_SCOPES, DomainId, EntityId, EventId,
        IdGenerationError, IdNamespace, IdPersistence, IdScope, IdValidationError, IdWire,
        PolicyRuleId, Revision, RevisionError, RoleAssignmentId, RoleId, SchemaRevision,
        SecurityEpoch, SecurityEpochError, SecurityPolicyRecordId, SnapshotId, UUID_BYTES,
        build_uuid_v7, generate_id,
    };
    use std::str::FromStr;
    use uuid::{Uuid, Variant};

    const VALID_V7: &str = "00000000-0000-7000-8000-000000000001";

    #[test]
    fn parses_and_formats_canonical_lowercase_uuid() {
        let parsed = AssertionId::from_str(VALID_V7);
        assert!(parsed.is_ok());
        if let Ok(id) = parsed {
            assert_eq!(id.to_string(), VALID_V7);
            let expected = Uuid::parse_str(VALID_V7);
            assert!(expected.is_ok());
            if let Ok(expected) = expected {
                assert_eq!(id.to_bytes(), expected.into_bytes());
            }
        }
    }

    #[test]
    fn uuid_timestamp_bits_do_not_define_domain_time() {
        // This extreme timestamp is structurally valid. Domain IDs do not
        // reject it based on chronology or expose it as WorldDB time.
        assert!(EntityId::from_str("ffffffff-ffff-7fff-8000-000000000001").is_ok());
    }

    #[test]
    fn rejects_noncanonical_text_and_invalid_uuid_generator_fields() {
        assert_eq!(
            AssertionId::from_str("00000000-0000-7000-8000-00000000ABCD"),
            Err(IdValidationError::NonCanonicalText)
        );

        let invalid_version =
            Uuid::from_u128(0x0000_0000_0000_9000_8000_0000_0000_0001).into_bytes();
        assert_eq!(
            EntityId::try_from_bytes(invalid_version),
            Err(IdValidationError::InvalidVersion)
        );

        let invalid_variant =
            Uuid::from_u128(0x0000_0000_0000_7000_c000_0000_0000_0001).into_bytes();
        assert_eq!(
            EventId::try_from_bytes(invalid_variant),
            Err(IdValidationError::InvalidVariant)
        );
    }

    #[test]
    fn accepts_all_standard_rfc_uuid_versions() {
        for version in 1..=8 {
            let mut bytes = [0_u8; UUID_BYTES];
            bytes[6] = version << 4;
            bytes[8] = 0x80;
            bytes[15] = 1;

            assert!(EntityId::try_from_bytes(bytes).is_ok(), "version {version}");
        }
    }

    #[test]
    fn rejects_reserved_uuid_sentinels() {
        assert_eq!(
            AssertionId::try_from_bytes([0; 16]),
            Err(IdValidationError::ReservedZero)
        );
        assert_eq!(
            AssertionId::try_from_bytes([u8::MAX; 16]),
            Err(IdValidationError::ReservedAllFf)
        );
    }

    #[test]
    fn generator_emits_valid_v7_and_checks_the_48_bit_timestamp() {
        let bytes = build_uuid_v7(0, &[0; 10]);
        assert!(bytes.is_ok());
        if let Ok(bytes) = bytes {
            let uuid = Uuid::from_bytes(bytes);
            assert_eq!(uuid.get_version_num(), 7);
            assert_eq!(uuid.get_variant(), Variant::RFC4122);
            assert_eq!(
                EntityId::try_from_bytes(bytes),
                EntityId::from_str("00000000-0000-7000-8000-000000000000")
            );
        }

        let too_large = build_uuid_v7(1_u64 << 48, &[0; 10]);
        assert_eq!(too_large, Err(IdGenerationError::TimestampOutOfRange));
    }

    #[test]
    fn system_generator_uses_the_validated_uuidv7_path() {
        let generated = generate_id::<EntityId>();
        assert!(generated.is_ok());
        if let Ok(id) = generated {
            let uuid = Uuid::from_bytes(id.to_bytes());
            assert_eq!(uuid.get_version_num(), 7);
            assert_eq!(uuid.get_variant(), Variant::RFC4122);
        }
    }

    #[test]
    fn every_identity_type_has_one_explicit_persistence_and_wire_scope() {
        let expected_names = [
            "DatabaseId",
            "HistorySpaceId",
            "LayerId",
            "PerspectiveId",
            "TimelineId",
            "EntityId",
            "PredicateId",
            "EntityTypeId",
            "AssertionId",
            "MaskId",
            "ReplacementBoundaryId",
            "EventId",
            "EventMaskId",
            "EventRelationId",
            "EventKindId",
            "EventRoleId",
            "EventAttributeId",
            "SourceId",
            "EvidenceId",
            "ProvenanceId",
            "AssertionValidityClosureId",
            "AssertionRetractionId",
            "MaskValidityClosureId",
            "MaskRetractionId",
            "ReplacementBoundaryValidityClosureId",
            "ReplacementBoundaryRetractionId",
            "EventSpanClosureId",
            "EventRetractionId",
            "EventMaskRetractionId",
            "EventRelationRetractionId",
            "EvidenceRetractionId",
            "ProvenanceRetractionId",
            "EntityRetirementId",
            "PerspectiveRetirementId",
            "ArchiveTransitionId",
            "MigrationId",
            "MigrationRunId",
            "MigrationStepId",
            "TransactionId",
            "OperationId",
            "SnapshotId",
            "SegmentId",
            "ClientRequestId",
            "AuditRecordId",
            "AuditOperationId",
            "JobId",
            "PrincipalId",
            "SecurityPolicyRecordId",
            "RoleId",
            "RoleAssignmentId",
            "PolicyRuleId",
        ];
        assert_eq!(DOMAIN_ID_SCOPES.len(), expected_names.len());
        assert_eq!(
            DOMAIN_ID_SCOPES
                .iter()
                .map(|scope| scope.type_name)
                .collect::<Vec<_>>(),
            expected_names
        );
        for (index, scope) in DOMAIN_ID_SCOPES.iter().enumerate() {
            assert!(!scope.type_name.is_empty());
            assert!(
                DOMAIN_ID_SCOPES
                    .iter()
                    .take(index)
                    .all(|previous| previous.type_name != scope.type_name)
            );
            assert_ne!(scope.wire, IdWire::CanonicalDecimalString);
        }

        assert_eq!(SnapshotId::SCOPE.persistence, IdPersistence::SessionLocal);
        assert_eq!(SnapshotId::SCOPE.wire, IdWire::SessionLocalUuid);
        assert!(!SnapshotId::SCOPE.is_persistent());
        assert_eq!(ClientRequestId::SCOPE.namespace, IdNamespace::Audit);
        assert_eq!(
            ClientRequestId::SCOPE.persistence,
            IdPersistence::AuditSubsystem
        );
        assert_eq!(ClientRequestId::SCOPE.wire, IdWire::AuditFormatUuid);
        assert!(ClientRequestId::SCOPE.is_persistent());
        assert_eq!(SecurityEpoch::INITIAL.value(), 0);
        assert_eq!(SecurityEpoch::INITIAL.next(), Ok(SecurityEpoch::new(1)));
        assert_eq!(
            SecurityEpoch::new(u64::MAX).next(),
            Err(SecurityEpochError::Exhausted)
        );
        assert_eq!(SecurityEpoch::SCOPE.namespace, IdNamespace::Security);
        assert_eq!(
            SecurityEpoch::SCOPE.persistence,
            IdPersistence::SecurityHistory
        );
        assert_eq!(
            AssertionId::SCOPE,
            IdScope {
                type_name: "AssertionId",
                namespace: IdNamespace::Database,
                persistence: IdPersistence::Database,
                wire: IdWire::CanonicalUuid,
                public_api: true,
            }
        );
        for security_scope in [
            SecurityPolicyRecordId::SCOPE,
            RoleId::SCOPE,
            RoleAssignmentId::SCOPE,
            PolicyRuleId::SCOPE,
        ] {
            assert_eq!(security_scope.namespace, IdNamespace::Security);
            assert_eq!(security_scope.persistence, IdPersistence::SecurityHistory);
            assert_eq!(security_scope.wire, IdWire::SecurityFilteredUuid);
            assert!(security_scope.public_api);
        }
    }

    #[test]
    fn revisions_reserve_max_and_keep_genesis_and_first_commit() {
        assert_eq!(Revision::GENESIS.value(), 0);
        assert_eq!(Revision::FIRST_COMMIT.value(), 1);
        assert_eq!(Revision::GENESIS.next_commit(), Ok(Revision::FIRST_COMMIT));
        assert_eq!(Revision::new(u64::MAX), Err(RevisionError::ReservedMaximum));
        let before_reserved = Revision::new(u64::MAX - 1);
        assert!(before_reserved.is_ok());
        if let Ok(revision) = before_reserved {
            assert_eq!(revision.next_commit(), Err(RevisionError::ReservedMaximum));
        }
    }

    #[test]
    fn revision_and_schema_revision_parse_canonical_shared_axis_values() {
        assert_eq!(Revision::from_str("0"), Ok(Revision::GENESIS));
        assert_eq!(
            Revision::from_str("01"),
            Err(RevisionError::NonCanonicalDecimal)
        );
        assert_eq!(
            Revision::from_str("+1"),
            Err(RevisionError::NonCanonicalDecimal)
        );
        assert_eq!(
            Revision::from_str("18446744073709551615"),
            Err(RevisionError::ReservedMaximum)
        );
        assert_eq!(
            Revision::from_str("18446744073709551616"),
            Err(RevisionError::IntegerOverflow)
        );

        let schema_revision = SchemaRevision::from_str("42");
        assert!(schema_revision.is_ok());
        if let Ok(value) = schema_revision {
            assert_eq!(value.revision().value(), 42);
            assert_eq!(value.to_string(), "42");
        }
    }
}
