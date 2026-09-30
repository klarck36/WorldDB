//! Canonical 1.0 frames, TLV fields, and scalar values.
//!
//! The wire contract fixes the frame shape and scalar representations. This
//! module assigns the previously unassigned 1.0 frame magic `WorldDB\0` and
//! value tags 1 through 10 in [`ValueTag`] declaration order.

use std::fmt;

use crate::ids::{DomainId, IdValidationError, WireId};
use crate::numbers::{decode_u128_varint_prefix, encode_u128_varint};
use crate::{
    Bytes, Decimal, DecimalError, Duration, Int, IntegerError, Symbol, SymbolError, Time, UInt,
    Value,
};

/// The fixed eight-byte 1.0 frame identifier.
pub const FRAME_MAGIC: [u8; 8] = *b"WorldDB\0";
/// The supported persistent format major version.
pub const FORMAT_MAJOR: u16 = 1;
/// The supported persistent format minor version.
pub const FORMAT_MINOR: u16 = 0;
/// The frame header size, excluding the trailing checksum.
pub const FRAME_HEADER_LEN: usize = 40;
/// The size of a BLAKE3 checksum.
pub const CHECKSUM_LEN: usize = 32;

/// Default resource policy for decoding the 1.0 wire format.
///
/// These limits are implementation policy, not part of the persistent format.
/// Applications may pass a different policy to the `*_with_limits` entry
/// points when their resource envelope allows it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecoderLimits {
    /// Maximum complete frame size, including header and checksum.
    pub max_frame_bytes: usize,
    /// Maximum copied UTF-8 string, symbol, time-unit, or opaque byte value.
    pub max_string_or_bytes: usize,
    /// Maximum TLV fields in one record or nested object.
    pub max_fields_per_record: usize,
    /// Maximum items in one encoded array.
    pub max_array_items: usize,
    /// Maximum decoded collection reservation in bytes.
    pub max_collection_bytes: usize,
    /// Maximum encoded bytes accepted for one record batch.
    pub max_batch_bytes: usize,
    /// Maximum number of records accepted in one decoded batch.
    pub max_records_per_batch: usize,
    /// Maximum structural depth admitted by the fixed 1.0 decoder grammars.
    /// The deepest current record path requires depth 7; scalar and audit
    /// formats need less. This is a resource policy, not a wire-format rule.
    pub max_nesting_depth: usize,
}

impl DecoderLimits {
    /// The default 1.0 decoder resource policy.
    pub const DEFAULT: Self = Self {
        max_frame_bytes: 64 * 1024 * 1024,
        max_string_or_bytes: 16 * 1024 * 1024,
        max_fields_per_record: 256,
        max_array_items: 1_000_000,
        max_collection_bytes: 64 * 1024 * 1024,
        max_batch_bytes: 256 * 1024 * 1024,
        max_records_per_batch: 1_000_000,
        max_nesting_depth: 8,
    };

    pub(crate) fn check_nesting_depth(&self, required: usize) -> Result<(), WireError> {
        if required > self.max_nesting_depth {
            return Err(WireError::ResourceLimitExceeded {
                resource: DecodeResource::NestingDepth,
                limit: self.max_nesting_depth,
                actual: required,
            });
        }
        Ok(())
    }
}

impl Default for DecoderLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The resource whose configured decoder budget was exceeded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeResource {
    /// A copied string, symbol, time unit, or byte value.
    StringOrBytes,
    /// Fields in one TLV object.
    FieldsPerRecord,
    /// Items in one encoded array.
    ArrayItems,
    /// Memory reserved for a decoded collection.
    CollectionBytes,
    /// Encoded bytes in a record batch.
    BatchBytes,
    /// Record count in a decoded batch.
    BatchRecords,
    /// Structural nesting in a versioned wire grammar.
    NestingDepth,
}

/// A frame header for the current 1.0 format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    format_major: u16,
    format_minor: u16,
    required_flags: u64,
    optional_flags: u64,
    kind: u32,
}

impl FrameHeader {
    /// Creates a current-version frame header with no feature flags.
    #[must_use]
    pub const fn new(kind: u32) -> Self {
        Self {
            format_major: FORMAT_MAJOR,
            format_minor: FORMAT_MINOR,
            required_flags: 0,
            optional_flags: 0,
            kind,
        }
    }

    /// Sets required capability bits. Version 1.0 currently defines none.
    #[must_use]
    pub const fn with_required_flags(mut self, flags: u64) -> Self {
        self.required_flags = flags;
        self
    }

    /// Sets optional capability bits; decoders preserve their exact value.
    #[must_use]
    pub const fn with_optional_flags(mut self, flags: u64) -> Self {
        self.optional_flags = flags;
        self
    }

    /// Returns the frame kind number.
    #[must_use]
    pub const fn kind(self) -> u32 {
        self.kind
    }

    /// Returns required capability bits.
    #[must_use]
    pub const fn required_flags(self) -> u64 {
        self.required_flags
    }

    /// Returns optional capability bits.
    #[must_use]
    pub const fn optional_flags(self) -> u64 {
        self.optional_flags
    }

    /// Returns the major format version.
    #[must_use]
    pub const fn format_major(self) -> u16 {
        self.format_major
    }

    /// Returns the minor format version.
    #[must_use]
    pub const fn format_minor(self) -> u16 {
        self.format_minor
    }
}

/// A borrowed, checksum-verified frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame<'a> {
    header: FrameHeader,
    payload: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Returns the verified frame header.
    #[must_use]
    pub const fn header(self) -> FrameHeader {
        self.header
    }

    /// Returns the exact payload bytes.
    #[must_use]
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }
}

/// A malformed frame or an unsupported format/capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    /// The input ended before a complete header and checksum were present.
    Truncated,
    /// The frame magic did not match the 1.0 constant.
    InvalidMagic,
    /// The major/minor pair is not supported by this implementation.
    UnsupportedVersion { major: u16, minor: u16 },
    /// A required capability bit is not understood by this implementation.
    UnknownRequiredFlags { flags: u64 },
    /// The declared payload length cannot fit in this process address space.
    PayloadLengthOverflow,
    /// The complete input frame exceeds the configured decoder budget.
    FrameTooLarge { limit: usize, actual: usize },
    /// Header, payload, or checksum lengths do not match the input exactly.
    LengthMismatch,
    /// The checksum does not match the header and payload bytes.
    ChecksumMismatch,
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => formatter.write_str("frame is truncated"),
            Self::InvalidMagic => formatter.write_str("frame magic is invalid"),
            Self::UnsupportedVersion { major, minor } => {
                write!(formatter, "unsupported frame version {major}.{minor}")
            }
            Self::UnknownRequiredFlags { flags } => {
                write!(formatter, "unknown required frame flags 0x{flags:016x}")
            }
            Self::PayloadLengthOverflow => {
                formatter.write_str("frame payload length exceeds the address space")
            }
            Self::FrameTooLarge { limit, actual } => write!(
                formatter,
                "frame is {actual} bytes; configured maximum is {limit} bytes"
            ),
            Self::LengthMismatch => {
                formatter.write_str("frame length does not match its declared payload length")
            }
            Self::ChecksumMismatch => formatter.write_str("frame checksum does not match"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Encodes one 1.0 frame and appends its BLAKE3 checksum.
pub fn encode_frame(header: FrameHeader, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if header.format_major != FORMAT_MAJOR || header.format_minor != FORMAT_MINOR {
        return Err(FrameError::UnsupportedVersion {
            major: header.format_major,
            minor: header.format_minor,
        });
    }
    if header.required_flags != 0 {
        return Err(FrameError::UnknownRequiredFlags {
            flags: header.required_flags,
        });
    }
    let payload_len =
        u64::try_from(payload.len()).map_err(|_| FrameError::PayloadLengthOverflow)?;
    let signed_len = FRAME_HEADER_LEN
        .checked_add(payload.len())
        .ok_or(FrameError::PayloadLengthOverflow)?;
    let capacity = signed_len
        .checked_add(CHECKSUM_LEN)
        .ok_or(FrameError::PayloadLengthOverflow)?;

    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&FRAME_MAGIC);
    bytes.extend_from_slice(&header.format_major.to_le_bytes());
    bytes.extend_from_slice(&header.format_minor.to_le_bytes());
    bytes.extend_from_slice(&header.required_flags.to_le_bytes());
    bytes.extend_from_slice(&header.optional_flags.to_le_bytes());
    bytes.extend_from_slice(&header.kind.to_le_bytes());
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes.extend_from_slice(payload);
    let checksum = blake3::hash(&bytes);
    bytes.extend_from_slice(checksum.as_bytes());
    Ok(bytes)
}

/// Verifies and decodes exactly one complete 1.0 frame without copying payload.
pub fn decode_frame(bytes: &[u8]) -> Result<Frame<'_>, FrameError> {
    decode_frame_with_limits(bytes, &DecoderLimits::DEFAULT)
}

/// Verifies and decodes a complete 1.0 frame under an explicit resource policy.
pub fn decode_frame_with_limits<'a>(
    bytes: &'a [u8],
    limits: &DecoderLimits,
) -> Result<Frame<'a>, FrameError> {
    if bytes.len() > limits.max_frame_bytes {
        return Err(FrameError::FrameTooLarge {
            limit: limits.max_frame_bytes,
            actual: bytes.len(),
        });
    }
    let minimum_len = FRAME_HEADER_LEN
        .checked_add(CHECKSUM_LEN)
        .ok_or(FrameError::PayloadLengthOverflow)?;
    if bytes.len() < minimum_len {
        return Err(FrameError::Truncated);
    }
    if bytes.get(0..FRAME_MAGIC.len()) != Some(FRAME_MAGIC.as_slice()) {
        return Err(FrameError::InvalidMagic);
    }

    let major = u16::from_le_bytes(read_array::<2>(bytes, 8)?);
    let minor = u16::from_le_bytes(read_array::<2>(bytes, 10)?);
    if major != FORMAT_MAJOR || minor != FORMAT_MINOR {
        return Err(FrameError::UnsupportedVersion { major, minor });
    }
    let required_flags = u64::from_le_bytes(read_array::<8>(bytes, 12)?);
    if required_flags != 0 {
        return Err(FrameError::UnknownRequiredFlags {
            flags: required_flags,
        });
    }
    let optional_flags = u64::from_le_bytes(read_array::<8>(bytes, 20)?);
    let kind = u32::from_le_bytes(read_array::<4>(bytes, 28)?);
    let payload_len_u64 = u64::from_le_bytes(read_array::<8>(bytes, 32)?);
    let declared_frame_len =
        u128::from(payload_len_u64) + (FRAME_HEADER_LEN as u128) + (CHECKSUM_LEN as u128);
    if declared_frame_len > limits.max_frame_bytes as u128 {
        return Err(FrameError::FrameTooLarge {
            limit: limits.max_frame_bytes,
            actual: usize::try_from(declared_frame_len).unwrap_or(usize::MAX),
        });
    }
    let payload_len =
        usize::try_from(payload_len_u64).map_err(|_| FrameError::PayloadLengthOverflow)?;
    let payload_end = FRAME_HEADER_LEN
        .checked_add(payload_len)
        .ok_or(FrameError::PayloadLengthOverflow)?;
    let expected_len = payload_end
        .checked_add(CHECKSUM_LEN)
        .ok_or(FrameError::PayloadLengthOverflow)?;
    if expected_len != bytes.len() {
        return Err(FrameError::LengthMismatch);
    }
    let signed_bytes = bytes.get(..payload_end).ok_or(FrameError::Truncated)?;
    let checksum = bytes
        .get(payload_end..expected_len)
        .ok_or(FrameError::Truncated)?;
    if blake3::hash(signed_bytes).as_bytes().as_slice() != checksum {
        return Err(FrameError::ChecksumMismatch);
    }
    let payload = bytes
        .get(FRAME_HEADER_LEN..payload_end)
        .ok_or(FrameError::Truncated)?;

    Ok(Frame {
        header: FrameHeader {
            format_major: major,
            format_minor: minor,
            required_flags,
            optional_flags,
            kind,
        },
        payload,
    })
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], FrameError> {
    let end = offset
        .checked_add(N)
        .ok_or(FrameError::PayloadLengthOverflow)?;
    bytes
        .get(offset..end)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(FrameError::Truncated)
}

/// The stable 1.0 tag assigned to each closed core value variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ValueTag {
    /// Boolean value; payload is one byte, `0` or `1`.
    Bool = 1,
    /// Signed i128 using a minimal ZigZag unsigned LEB128.
    Int = 2,
    /// Unsigned u128 using a minimal unsigned LEB128.
    UInt = 3,
    /// Canonical sign, coefficient varint, and little-endian i32 scale.
    Decimal = 4,
    /// Length-prefixed valid UTF-8 bytes, preserved byte-for-byte.
    String = 5,
    /// Length-prefixed validated ASCII schema symbol.
    Symbol = 6,
    /// Raw 16-byte entity identity.
    Entity = 7,
    /// TimelineId, signed tick varint, then length-prefixed unit symbol.
    Time = 8,
    /// Signed i128 nanoseconds using a minimal ZigZag unsigned LEB128.
    Duration = 9,
    /// Length-prefixed opaque bytes.
    Bytes = 10,
}

impl ValueTag {
    fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Bool),
            2 => Some(Self::Int),
            3 => Some(Self::UInt),
            4 => Some(Self::Decimal),
            5 => Some(Self::String),
            6 => Some(Self::Symbol),
            7 => Some(Self::Entity),
            8 => Some(Self::Time),
            9 => Some(Self::Duration),
            10 => Some(Self::Bytes),
            _ => None,
        }
    }
}

/// Encodes an identity as its exact 16-byte wire representation.
#[must_use]
pub fn encode_id<T: WireId>(value: T) -> [u8; 16] {
    value.to_bytes()
}

/// Validates and decodes exactly one raw 16-byte identity.
pub fn decode_id<T: WireId>(bytes: &[u8]) -> Result<T, WireError> {
    let raw = bytes.try_into().map_err(|_| WireError::LengthMismatch {
        offset: bytes.len(),
    })?;
    T::try_from_bytes(raw).map_err(WireError::Identity)
}

/// Encodes a closed core `Value` using its stable 1.0 tag and scalar bytes.
#[must_use]
pub fn encode_value(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    match value {
        Value::Bool(value) => {
            append_tag(&mut bytes, ValueTag::Bool);
            bytes.push(u8::from(*value));
        }
        Value::Int(value) => {
            append_tag(&mut bytes, ValueTag::Int);
            bytes.extend(value.to_canonical_bytes());
        }
        Value::UInt(value) => {
            append_tag(&mut bytes, ValueTag::UInt);
            bytes.extend(value.to_canonical_bytes());
        }
        Value::Decimal(value) => {
            append_tag(&mut bytes, ValueTag::Decimal);
            bytes.extend(value.to_canonical_bytes());
        }
        Value::String(value) => {
            append_tag(&mut bytes, ValueTag::String);
            append_length_prefixed(&mut bytes, value.as_bytes());
        }
        Value::Symbol(value) => {
            append_tag(&mut bytes, ValueTag::Symbol);
            append_length_prefixed(&mut bytes, value.as_str().as_bytes());
        }
        Value::Entity(value) => {
            append_tag(&mut bytes, ValueTag::Entity);
            bytes.extend_from_slice(&encode_id(*value));
        }
        Value::Time(value) => {
            append_tag(&mut bytes, ValueTag::Time);
            bytes.extend_from_slice(&encode_id(value.timeline_id()));
            bytes.extend(Int::new(value.ticks()).to_canonical_bytes());
            append_length_prefixed(&mut bytes, value.unit().as_str().as_bytes());
        }
        Value::Duration(value) => {
            append_tag(&mut bytes, ValueTag::Duration);
            bytes.extend(Int::new(value.nanoseconds()).to_canonical_bytes());
        }
        Value::Bytes(value) => {
            append_tag(&mut bytes, ValueTag::Bytes);
            append_length_prefixed(&mut bytes, value.as_slice());
        }
    }
    bytes
}

fn append_tag(bytes: &mut Vec<u8>, tag: ValueTag) {
    bytes.extend(encode_u128_varint(tag as u128));
}

fn append_length_prefixed(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend(encode_u128_varint(value.len() as u128));
    bytes.extend_from_slice(value);
}

/// Decodes one closed core `Value`, rejecting unknown tags and alternate bytes.
pub fn decode_value(bytes: &[u8]) -> Result<Value, WireError> {
    decode_value_with_limits(bytes, &DecoderLimits::DEFAULT)
}

/// Decodes one closed core `Value` under an explicit resource policy.
pub fn decode_value_with_limits(bytes: &[u8], limits: &DecoderLimits) -> Result<Value, WireError> {
    limits.check_nesting_depth(2)?;
    let (raw_tag, tag_len) = decode_varint_at(bytes, 0)?;
    let tag = u32::try_from(raw_tag).map_err(|_| WireError::ValueTagOverflow { offset: 0 })?;
    let payload = bytes
        .get(tag_len..)
        .ok_or(WireError::Truncated { offset: tag_len })?;
    let value_offset = tag_len;
    match ValueTag::from_raw(tag).ok_or(WireError::UnknownValueTag { offset: 0, tag })? {
        ValueTag::Bool => match payload {
            [0] => Ok(Value::Bool(false)),
            [1] => Ok(Value::Bool(true)),
            [byte] => Err(WireError::InvalidBoolean {
                offset: value_offset,
                byte: *byte,
            }),
            _ => Err(WireError::LengthMismatch {
                offset: value_offset,
            }),
        },
        ValueTag::Int => Int::from_canonical_bytes(payload)
            .map(Value::Int)
            .map_err(WireError::Integer),
        ValueTag::UInt => UInt::from_canonical_bytes(payload)
            .map(Value::UInt)
            .map_err(WireError::Integer),
        ValueTag::Decimal => Decimal::from_canonical_bytes(payload)
            .map(Value::Decimal)
            .map_err(WireError::Decimal),
        ValueTag::String => decode_string(payload, value_offset, limits).map(Value::String),
        ValueTag::Symbol => decode_string(payload, value_offset, limits)
            .and_then(|value| Symbol::new(value).map_err(WireError::Symbol))
            .map(Value::Symbol),
        ValueTag::Entity => decode_id(payload).map(Value::Entity),
        ValueTag::Time => decode_time(payload, value_offset, limits).map(Value::Time),
        ValueTag::Duration => Int::from_canonical_bytes(payload)
            .map(|value| Value::Duration(Duration::from_nanoseconds(value.value())))
            .map_err(WireError::Integer),
        ValueTag::Bytes => decode_bytes(payload, value_offset, limits).map(Value::Bytes),
    }
}

fn decode_string(bytes: &[u8], offset: usize, limits: &DecoderLimits) -> Result<String, WireError> {
    check_prefixed_value_size(bytes, limits)?;
    let (raw, consumed) =
        decode_length_prefixed(bytes, 0).map_err(|error| shift_error_offset(error, offset))?;
    if consumed != bytes.len() {
        return Err(WireError::TrailingBytes {
            offset: offset + consumed,
        });
    }
    let prefix_len = consumed.saturating_sub(raw.len());
    check_value_size(raw.len(), limits)?;
    let value = std::str::from_utf8(raw).map_err(|error| WireError::InvalidUtf8 {
        offset: offset
            .saturating_add(prefix_len)
            .saturating_add(error.valid_up_to()),
    })?;
    Ok(value.to_owned())
}

fn decode_bytes(bytes: &[u8], offset: usize, limits: &DecoderLimits) -> Result<Bytes, WireError> {
    check_prefixed_value_size(bytes, limits)?;
    let (raw, consumed) =
        decode_length_prefixed(bytes, 0).map_err(|error| shift_error_offset(error, offset))?;
    if consumed != bytes.len() {
        return Err(WireError::TrailingBytes {
            offset: offset + consumed,
        });
    }
    check_value_size(raw.len(), limits)?;
    Ok(Bytes::new(raw.to_vec()))
}

fn decode_time(bytes: &[u8], offset: usize, limits: &DecoderLimits) -> Result<Time, WireError> {
    use crate::ids::TimelineId;

    let timeline_bytes = bytes.get(..16).ok_or(WireError::Truncated { offset })?;
    let timeline_raw = timeline_bytes
        .try_into()
        .map_err(|_| WireError::Truncated { offset })?;
    let timeline_id = TimelineId::try_from_bytes(timeline_raw).map_err(WireError::Identity)?;
    let ticks_start = 16;
    let (_, ticks_len) =
        decode_varint_at(bytes, ticks_start).map_err(|error| shift_error_offset(error, offset))?;
    let ticks_bytes_end = ticks_start
        .checked_add(ticks_len)
        .ok_or(WireError::LengthOverflow { offset })?;
    let ticks_bytes = bytes
        .get(ticks_start..ticks_bytes_end)
        .ok_or(WireError::Truncated {
            offset: offset + ticks_start,
        })?;
    let ticks = Int::from_canonical_bytes(ticks_bytes)
        .map_err(WireError::Integer)?
        .value();
    let unit_offset = offset + ticks_bytes_end;
    check_prefixed_value_size_at(bytes, ticks_bytes_end, limits)
        .map_err(|error| shift_error_offset(error, offset))?;
    let (unit_bytes, consumed) = decode_length_prefixed(bytes, ticks_bytes_end)
        .map_err(|error| shift_error_offset(error, offset))?;
    if ticks_bytes_end + consumed != bytes.len() {
        return Err(WireError::TrailingBytes {
            offset: unit_offset + consumed,
        });
    }
    let unit_text = std::str::from_utf8(unit_bytes).map_err(|error| WireError::InvalidUtf8 {
        offset: unit_offset + error.valid_up_to(),
    })?;
    check_value_size(unit_text.len(), limits)?;
    let unit = Symbol::new(unit_text.to_owned()).map_err(WireError::Symbol)?;
    Ok(Time::new(timeline_id, ticks, unit))
}

fn check_value_size(length: usize, limits: &DecoderLimits) -> Result<(), WireError> {
    if length > limits.max_string_or_bytes {
        return Err(WireError::ResourceLimitExceeded {
            resource: DecodeResource::StringOrBytes,
            limit: limits.max_string_or_bytes,
            actual: length,
        });
    }
    Ok(())
}

fn check_prefixed_value_size(bytes: &[u8], limits: &DecoderLimits) -> Result<(), WireError> {
    check_prefixed_value_size_at(bytes, 0, limits)
}

fn check_prefixed_value_size_at(
    bytes: &[u8],
    offset: usize,
    limits: &DecoderLimits,
) -> Result<(), WireError> {
    let (length, _) = decode_varint_at(bytes, offset)?;
    if length > limits.max_string_or_bytes as u128 {
        return Err(WireError::ResourceLimitExceeded {
            resource: DecodeResource::StringOrBytes,
            limit: limits.max_string_or_bytes,
            actual: usize::try_from(length).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

fn decode_varint_at(bytes: &[u8], offset: usize) -> Result<(u128, usize), WireError> {
    let remaining = bytes.get(offset..).ok_or(WireError::Truncated { offset })?;
    decode_u128_varint_prefix(remaining).map_err(|error| WireError::Varint { offset, error })
}

fn decode_length_prefixed(bytes: &[u8], offset: usize) -> Result<(&[u8], usize), WireError> {
    let (length, prefix_len) = decode_varint_at(bytes, offset)?;
    let value_start = offset
        .checked_add(prefix_len)
        .ok_or(WireError::LengthOverflow { offset })?;
    let available = bytes
        .len()
        .checked_sub(value_start)
        .ok_or(WireError::Truncated {
            offset: value_start,
        })?;
    if length > available as u128 {
        return Err(WireError::Truncated {
            offset: value_start,
        });
    }
    let length = usize::try_from(length).map_err(|_| WireError::LengthOverflow { offset })?;
    let value_end = value_start
        .checked_add(length)
        .ok_or(WireError::LengthOverflow { offset })?;
    let value = bytes
        .get(value_start..value_end)
        .ok_or(WireError::Truncated {
            offset: value_start,
        })?;
    Ok((value, value_end - offset))
}

fn shift_error_offset(error: WireError, base: usize) -> WireError {
    let shift = |offset: usize| base.saturating_add(offset);
    match error {
        WireError::Varint { offset, error } => WireError::Varint {
            offset: shift(offset),
            error,
        },
        WireError::Truncated { offset } => WireError::Truncated {
            offset: shift(offset),
        },
        WireError::LengthOverflow { offset } => WireError::LengthOverflow {
            offset: shift(offset),
        },
        WireError::LengthMismatch { offset } => WireError::LengthMismatch {
            offset: shift(offset),
        },
        WireError::TrailingBytes { offset } => WireError::TrailingBytes {
            offset: shift(offset),
        },
        WireError::ValueTagOverflow { offset } => WireError::ValueTagOverflow {
            offset: shift(offset),
        },
        WireError::UnknownValueTag { offset, tag } => WireError::UnknownValueTag {
            offset: shift(offset),
            tag,
        },
        WireError::FieldTagOverflow { offset } => WireError::FieldTagOverflow {
            offset: shift(offset),
        },
        WireError::FieldNotIncreasing {
            offset,
            previous,
            current,
        } => WireError::FieldNotIncreasing {
            offset: shift(offset),
            previous,
            current,
        },
        WireError::InvalidBoolean { offset, byte } => WireError::InvalidBoolean {
            offset: shift(offset),
            byte,
        },
        WireError::InvalidUtf8 { offset } => WireError::InvalidUtf8 {
            offset: shift(offset),
        },
        WireError::ResourceLimitExceeded {
            resource,
            limit,
            actual,
        } => WireError::ResourceLimitExceeded {
            resource,
            limit,
            actual,
        },
        WireError::AllocationFailed { resource } => WireError::AllocationFailed { resource },
        WireError::Integer(error) => WireError::Integer(error),
        WireError::Decimal(error) => WireError::Decimal(error),
        WireError::Symbol(error) => WireError::Symbol(error),
        WireError::Identity(error) => WireError::Identity(error),
    }
}

/// One borrowed field from a length-prefixed TLV record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TlvField<'a> {
    tag: u32,
    value: &'a [u8],
}

impl<'a> TlvField<'a> {
    /// Returns the field's numeric tag.
    #[must_use]
    pub const fn tag(self) -> u32 {
        self.tag
    }

    /// Returns the exact field value bytes.
    #[must_use]
    pub const fn value(self) -> &'a [u8] {
        self.value
    }
}

/// A TLV record builder that accepts field tags only in strictly increasing order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TlvEncoder {
    bytes: Vec<u8>,
    previous_tag: Option<u32>,
}

impl TlvEncoder {
    /// Creates an empty record encoder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            previous_tag: None,
        }
    }

    /// Appends one field, rejecting duplicates or descending field numbers.
    pub fn push(&mut self, tag: u32, value: &[u8]) -> Result<(), WireError> {
        if let Some(previous) = self.previous_tag {
            if tag <= previous {
                return Err(WireError::FieldNotIncreasing {
                    offset: self.bytes.len(),
                    previous,
                    current: tag,
                });
            }
        }
        self.bytes.extend(encode_u128_varint(u128::from(tag)));
        self.bytes.extend(encode_u128_varint(value.len() as u128));
        self.bytes.extend_from_slice(value);
        self.previous_tag = Some(tag);
        Ok(())
    }

    /// Returns the completed record payload.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// A borrowed TLV record decoder that enforces minimal varints and field order.
#[derive(Clone, Debug)]
pub struct TlvDecoder<'a> {
    bytes: &'a [u8],
    offset: usize,
    previous_tag: Option<u32>,
    fields_read: usize,
    limits: DecoderLimits,
    failed: bool,
}

impl<'a> TlvDecoder<'a> {
    /// Creates a decoder over one record payload.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self::with_limits(bytes, DecoderLimits::DEFAULT)
    }

    /// Creates a decoder over one record payload with an explicit resource policy.
    #[must_use]
    pub const fn with_limits(bytes: &'a [u8], limits: DecoderLimits) -> Self {
        Self {
            bytes,
            offset: 0,
            previous_tag: None,
            fields_read: 0,
            limits,
            failed: false,
        }
    }

    /// Returns the next field, or `None` when the payload is exhausted.
    pub fn next_field(&mut self) -> Result<Option<TlvField<'a>>, WireError> {
        if self.failed || self.offset == self.bytes.len() {
            return Ok(None);
        }
        match self.read_field() {
            Ok(field) => Ok(Some(field)),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }

    fn read_field(&mut self) -> Result<TlvField<'a>, WireError> {
        if self.fields_read >= self.limits.max_fields_per_record {
            return Err(WireError::ResourceLimitExceeded {
                resource: DecodeResource::FieldsPerRecord,
                limit: self.limits.max_fields_per_record,
                actual: self.fields_read.saturating_add(1),
            });
        }
        let field_offset = self.offset;
        let (raw_tag, tag_len) = decode_varint_at(self.bytes, field_offset)?;
        let tag = u32::try_from(raw_tag).map_err(|_| WireError::FieldTagOverflow {
            offset: field_offset,
        })?;
        let length_offset = field_offset
            .checked_add(tag_len)
            .ok_or(WireError::LengthOverflow {
                offset: field_offset,
            })?;
        let (value, value_prefix_len) = decode_length_prefixed(self.bytes, length_offset)?;
        let end = length_offset
            .checked_add(value_prefix_len)
            .ok_or(WireError::LengthOverflow {
                offset: field_offset,
            })?;
        if let Some(previous) = self.previous_tag {
            if tag <= previous {
                return Err(WireError::FieldNotIncreasing {
                    offset: field_offset,
                    previous,
                    current: tag,
                });
            }
        }
        self.offset = end;
        self.previous_tag = Some(tag);
        self.fields_read += 1;
        Ok(TlvField { tag, value })
    }
}

/// A malformed scalar, TLV field, or canonical value encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireError {
    /// A varint is malformed or nonminimal at the reported byte offset.
    Varint { offset: usize, error: IntegerError },
    /// A signed or unsigned integer encoding is malformed.
    Integer(IntegerError),
    /// A decimal encoding is malformed or noncanonical.
    Decimal(DecimalError),
    /// A symbol does not satisfy the schema grammar.
    Symbol(SymbolError),
    /// An identity's 16 bytes are reserved or have an invalid UUID form.
    Identity(IdValidationError),
    /// The input ended before a declared value was complete.
    Truncated { offset: usize },
    /// A length or offset exceeds the process address space.
    LengthOverflow { offset: usize },
    /// The encoded length or fixed scalar width does not match the input.
    LengthMismatch { offset: usize },
    /// Bytes followed an otherwise complete value.
    TrailingBytes { offset: usize },
    /// A core value tag does not fit in a u32.
    ValueTagOverflow { offset: usize },
    /// A core value uses an unknown tag.
    UnknownValueTag { offset: usize, tag: u32 },
    /// A field tag does not fit in a u32.
    FieldTagOverflow { offset: usize },
    /// Field tags are duplicated or not in strictly increasing order.
    FieldNotIncreasing {
        offset: usize,
        previous: u32,
        current: u32,
    },
    /// A boolean is not encoded as exactly `0` or `1`.
    InvalidBoolean { offset: usize, byte: u8 },
    /// String or symbol bytes are not valid UTF-8.
    InvalidUtf8 { offset: usize },
    /// A configured decoding resource budget was exceeded.
    ResourceLimitExceeded {
        resource: DecodeResource,
        limit: usize,
        actual: usize,
    },
    /// A bounded decoder collection could not reserve its checked capacity.
    AllocationFailed { resource: DecodeResource },
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Varint { offset, error } => write!(formatter, "varint at byte {offset}: {error}"),
            Self::Integer(error) => write!(formatter, "integer encoding: {error}"),
            Self::Decimal(error) => write!(formatter, "decimal encoding: {error}"),
            Self::Symbol(error) => write!(formatter, "symbol encoding: {error}"),
            Self::Identity(error) => write!(formatter, "identity encoding: {error}"),
            Self::Truncated { offset } => write!(formatter, "truncated wire data at byte {offset}"),
            Self::LengthOverflow { offset } => {
                write!(formatter, "wire length overflow at byte {offset}")
            }
            Self::LengthMismatch { offset } => {
                write!(formatter, "wire length mismatch at byte {offset}")
            }
            Self::TrailingBytes { offset } => {
                write!(formatter, "trailing wire bytes at byte {offset}")
            }
            Self::ValueTagOverflow { offset } => {
                write!(formatter, "core value tag overflow at byte {offset}")
            }
            Self::UnknownValueTag { offset, tag } => {
                write!(formatter, "unknown core value tag {tag} at byte {offset}")
            }
            Self::FieldTagOverflow { offset } => {
                write!(formatter, "TLV field tag overflow at byte {offset}")
            }
            Self::FieldNotIncreasing {
                offset,
                previous,
                current,
            } => write!(
                formatter,
                "TLV field tag {current} at byte {offset} does not follow {previous}"
            ),
            Self::InvalidBoolean { offset, byte } => {
                write!(
                    formatter,
                    "invalid boolean byte 0x{byte:02x} at byte {offset}"
                )
            }
            Self::InvalidUtf8 { offset } => write!(formatter, "invalid UTF-8 at byte {offset}"),
            Self::ResourceLimitExceeded {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "decoder resource {resource:?} is {actual}; configured maximum is {limit}"
            ),
            Self::AllocationFailed { resource } => {
                write!(
                    formatter,
                    "could not reserve bounded decoder resource {resource:?}"
                )
            }
        }
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Varint { error, .. } | Self::Integer(error) => Some(error),
            Self::Decimal(error) => Some(error),
            Self::Symbol(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::Truncated { .. }
            | Self::LengthOverflow { .. }
            | Self::LengthMismatch { .. }
            | Self::TrailingBytes { .. }
            | Self::ValueTagOverflow { .. }
            | Self::UnknownValueTag { .. }
            | Self::FieldTagOverflow { .. }
            | Self::FieldNotIncreasing { .. }
            | Self::InvalidBoolean { .. }
            | Self::InvalidUtf8 { .. }
            | Self::ResourceLimitExceeded { .. }
            | Self::AllocationFailed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        DecodeResource, DecoderLimits, FrameError, FrameHeader, TlvDecoder, TlvEncoder, ValueTag,
        WireError, decode_frame, decode_frame_with_limits, decode_value, decode_value_with_limits,
        encode_frame, encode_value,
    };
    use crate::{Bytes, Decimal, Duration, EntityId, Int, Symbol, Time, TimelineId, UInt, Value};

    #[test]
    fn frame_round_trip_preserves_optional_flags_and_payload() {
        let header = FrameHeader::new(7).with_optional_flags(0x8000_0000_0000_0001);
        let encoded = encode_frame(header, &[0, 1, 2, 255]);
        assert!(encoded.is_ok());
        if let Ok(encoded) = encoded {
            let decoded = decode_frame(&encoded);
            assert!(decoded.is_ok());
            if let Ok(frame) = decoded {
                assert_eq!(frame.header(), header);
                assert_eq!(frame.payload(), &[0, 1, 2, 255]);
            }
        }
    }

    #[test]
    fn frame_rejects_tampered_payload_and_unknown_required_flags() {
        let encoded = encode_frame(FrameHeader::new(3), &[0xaa]);
        assert!(encoded.is_ok());
        if let Ok(encoded) = encoded {
            let mut changed = encoded.clone();
            if let Some(byte) = changed.get_mut(super::FRAME_HEADER_LEN) {
                *byte ^= 0x80;
            }
            assert_eq!(decode_frame(&changed), Err(FrameError::ChecksumMismatch));

            let mut wrong_version = encoded.clone();
            if let Some(byte) = wrong_version.get_mut(8) {
                *byte = 2;
            }
            assert_eq!(
                decode_frame(&wrong_version),
                Err(FrameError::UnsupportedVersion { major: 2, minor: 0 })
            );

            let mut unknown_required = encoded.clone();
            if let Some(byte) = unknown_required.get_mut(12) {
                *byte = 1;
            }
            assert_eq!(
                decode_frame(&unknown_required),
                Err(FrameError::UnknownRequiredFlags { flags: 1 })
            );
        }

        let invalid = FrameHeader::new(3).with_required_flags(1);
        assert_eq!(
            encode_frame(invalid, &[]),
            Err(FrameError::UnknownRequiredFlags { flags: 1 })
        );
    }

    #[test]
    fn decoder_limits_reject_large_frames_and_owned_values_before_copy() {
        let frame = encode_frame(FrameHeader::new(3), &[0xaa, 0xbb]);
        assert!(frame.is_ok());
        if let Ok(frame) = frame {
            let limits = DecoderLimits {
                max_frame_bytes: frame.len() - 1,
                ..DecoderLimits::DEFAULT
            };
            assert_eq!(
                decode_frame_with_limits(&frame, &limits),
                Err(FrameError::FrameTooLarge {
                    limit: limits.max_frame_bytes,
                    actual: frame.len(),
                })
            );

            let mut oversized_declared_length = frame.clone();
            if let Some(length) = oversized_declared_length.get_mut(32..40) {
                length.fill(0xff);
            }
            assert!(matches!(
                decode_frame_with_limits(&oversized_declared_length, &DecoderLimits::DEFAULT),
                Err(FrameError::FrameTooLarge {
                    limit,
                    actual: usize::MAX,
                })
                if limit == 64 * 1024 * 1024
            ));
        }

        let limits = DecoderLimits {
            max_string_or_bytes: 3,
            ..DecoderLimits::DEFAULT
        };
        let string = encode_value(&Value::String(String::from("four")));
        assert!(matches!(
            decode_value_with_limits(&string, &limits),
            Err(WireError::ResourceLimitExceeded {
                resource: DecodeResource::StringOrBytes,
                limit: 3,
                actual: 4,
            })
        ));
        let bytes = encode_value(&Value::Bytes(Bytes::new(vec![1, 2, 3, 4])));
        assert!(matches!(
            decode_value_with_limits(&bytes, &limits),
            Err(WireError::ResourceLimitExceeded {
                resource: DecodeResource::StringOrBytes,
                limit: 3,
                actual: 4,
            })
        ));

        let mut oversized_value_length = vec![ValueTag::String as u8];
        oversized_value_length.extend(crate::numbers::encode_u128_varint(u128::MAX));
        assert!(matches!(
            decode_value_with_limits(&oversized_value_length, &DecoderLimits::DEFAULT),
            Err(WireError::ResourceLimitExceeded {
                resource: DecodeResource::StringOrBytes,
                limit,
                actual: usize::MAX,
            })
            if limit == 16 * 1024 * 1024
        ));

        let mut oversized_field_length = crate::numbers::encode_u128_varint(1);
        oversized_field_length.extend(crate::numbers::encode_u128_varint(u128::MAX));
        assert!(matches!(
            TlvDecoder::new(&oversized_field_length).next_field(),
            Err(WireError::Truncated { .. })
        ));
    }

    #[test]
    fn decoder_depth_budget_is_checked_before_value_decoding() {
        let limits = DecoderLimits {
            max_nesting_depth: 1,
            ..DecoderLimits::DEFAULT
        };
        assert!(matches!(
            decode_value_with_limits(&[1, 1], &limits),
            Err(WireError::ResourceLimitExceeded {
                resource: DecodeResource::NestingDepth,
                limit: 1,
                actual: 2,
            })
        ));
    }

    #[test]
    fn all_value_families_round_trip_canonically() {
        let symbol = Symbol::new(String::from("hour_1"));
        let unit = Symbol::new(String::from("hour_1"));
        let decimal = Decimal::new(true, 1200, 3);
        let entity = EntityId::from_str("00000000-0000-7000-8000-000000000001");
        let timeline = TimelineId::from_str("00000000-0000-7000-8000-000000000002");
        assert!(symbol.is_ok());
        assert!(unit.is_ok());
        assert!(decimal.is_ok());
        assert!(entity.is_ok());
        assert!(timeline.is_ok());
        if let (Ok(symbol), Ok(unit), Ok(decimal), Ok(entity), Ok(timeline)) =
            (symbol, unit, decimal, entity, timeline)
        {
            let values = vec![
                Value::Bool(true),
                Value::Int(Int::new(i128::MIN)),
                Value::UInt(UInt::new(u128::MAX)),
                Value::Decimal(decimal),
                Value::String(String::from("e\u{301}")),
                Value::Symbol(symbol),
                Value::Entity(entity),
                Value::Time(Time::new(timeline, i128::MIN, unit)),
                Value::Duration(Duration::from_nanoseconds(i128::MAX)),
                Value::Bytes(Bytes::new(vec![0, 255])),
            ];
            for value in values {
                let encoded = encode_value(&value);
                let decoded = decode_value(&encoded);
                assert!(decoded.is_ok(), "{value:?}: {decoded:?}");
                if let Ok(decoded) = decoded {
                    assert_eq!(encode_value(&decoded), encoded);
                }
            }
        }
    }

    #[test]
    fn unknown_values_and_noncanonical_booleans_or_decimals_fail_closed() {
        assert!(matches!(
            decode_value(&[11]),
            Err(WireError::UnknownValueTag { tag: 11, .. })
        ));
        assert!(matches!(
            decode_value(&[1, 2]),
            Err(WireError::InvalidBoolean { byte: 2, .. })
        ));
        assert!(matches!(
            decode_value(&[4, 1, 0, 0, 0, 0, 0]),
            Err(WireError::Decimal(_))
        ));
    }

    #[test]
    fn tlv_fields_are_strictly_sorted_and_lengths_are_minimal() {
        let mut encoder = TlvEncoder::new();
        assert!(encoder.push(1, &[0xaa]).is_ok());
        assert!(encoder.push(3, &[]).is_ok());
        assert!(matches!(
            encoder.push(3, &[0xbb]),
            Err(WireError::FieldNotIncreasing { .. })
        ));
        let bytes = encoder.finish();
        let mut decoder = TlvDecoder::new(&bytes);
        let first = decoder.next_field();
        assert!(matches!(first, Ok(Some(field)) if field.tag() == 1 && field.value() == [0xaa]));
        let second = decoder.next_field();
        assert!(matches!(second, Ok(Some(field)) if field.tag() == 3 && field.value().is_empty()));
        assert_eq!(decoder.next_field(), Ok(None));

        let nonminimal = [0x81, 0x00, 0x00];
        assert!(matches!(
            TlvDecoder::new(&nonminimal).next_field(),
            Err(WireError::Varint { .. })
        ));

        let duplicate = [1, 0, 1, 0];
        let mut duplicate_decoder = TlvDecoder::new(&duplicate);
        assert!(matches!(duplicate_decoder.next_field(), Ok(Some(_))));
        assert!(matches!(
            duplicate_decoder.next_field(),
            Err(WireError::FieldNotIncreasing { .. })
        ));
    }
}
