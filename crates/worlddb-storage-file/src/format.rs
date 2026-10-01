//! Versioned format probe and capability preservation.

use std::fmt;

use worlddb_core::{
    CHECKSUM_LEN, FRAME_HEADER_LEN, FrameError, FrameHeader, decode_frame, encode_frame,
};

/// Frame kind stored in the database root's `FORMAT` probe file.
pub const FORMAT_FILE_KIND: u32 = 0x5744_4246;

/// Required capabilities supported by this 1.0 format probe.
const SUPPORTED_REQUIRED_CAPABILITIES: u64 = 0;

/// Optional capabilities understood semantically by this 1.0 format probe.
/// Unknown optional bits are retained as opaque values across resaves.
const RECOGNIZED_OPTIONAL_CAPABILITIES: u64 = 0;

/// Exact file length of the empty format-probe frame.
pub const FORMAT_FILE_BYTES: usize = FRAME_HEADER_LEN + CHECKSUM_LEN;

/// Raw format capabilities retained from a successfully probed database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatCapabilities {
    required_flags: u64,
    optional_flags: u64,
}

impl FormatCapabilities {
    /// Returns the 1.0 capability set for a newly created database.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            required_flags: 0,
            optional_flags: 0,
        }
    }

    /// Required capability bits in the stored format probe.
    #[must_use]
    pub const fn required_flags(self) -> u64 {
        self.required_flags
    }

    /// Exact optional capability bits in the stored format probe.
    #[must_use]
    pub const fn optional_flags(self) -> u64 {
        self.optional_flags
    }

    /// Optional bits not interpreted by this implementation.
    #[must_use]
    pub const fn unknown_optional_flags(self) -> u64 {
        self.optional_flags & !RECOGNIZED_OPTIONAL_CAPABILITIES
    }

    pub(crate) fn encode(self) -> Result<Vec<u8>, FrameError> {
        let header = FrameHeader::new(FORMAT_FILE_KIND)
            .with_required_flags(self.required_flags)
            .with_optional_flags(self.optional_flags);
        encode_frame(header, &[])
    }
}

/// Why a format probe could not be safely opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatProbeError {
    /// Format file is not exactly one empty probe frame.
    InvalidFileLength { expected: usize, actual: usize },
    /// Frame checksum, version, or required flags were invalid.
    Frame(FrameError),
    /// A frame other than the registered format-probe kind was stored.
    UnexpectedFrameKind { actual: u32 },
    /// The format-probe frame must not contain an uninterpreted payload.
    UnexpectedPayload,
    /// A required capability is not supported by this implementation.
    UnsupportedRequiredCapabilities { flags: u64 },
}

impl fmt::Display for FormatProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileLength { expected, actual } => write!(
                formatter,
                "format probe must be {expected} bytes; found {actual} bytes"
            ),
            Self::Frame(error) => write!(formatter, "invalid format-probe frame: {error}"),
            Self::UnexpectedFrameKind { actual } => {
                write!(
                    formatter,
                    "unexpected format-probe frame kind {actual:#010x}"
                )
            }
            Self::UnexpectedPayload => {
                formatter.write_str("format-probe frame payload must be empty")
            }
            Self::UnsupportedRequiredCapabilities { flags } => write!(
                formatter,
                "database requires unsupported format capabilities {flags:#018x}"
            ),
        }
    }
}

impl std::error::Error for FormatProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(error) => Some(error),
            _ => None,
        }
    }
}

/// Probes a complete 1.0 format frame, rejecting unsupported required bits
/// while retaining every optional bit without assigning it semantics.
pub fn probe_format(bytes: &[u8]) -> Result<FormatCapabilities, FormatProbeError> {
    if bytes.len() != FORMAT_FILE_BYTES {
        return Err(FormatProbeError::InvalidFileLength {
            expected: FORMAT_FILE_BYTES,
            actual: bytes.len(),
        });
    }
    let frame = match decode_frame(bytes) {
        Ok(frame) => frame,
        Err(FrameError::UnknownRequiredFlags { flags }) => {
            return Err(FormatProbeError::UnsupportedRequiredCapabilities { flags });
        }
        Err(error) => return Err(FormatProbeError::Frame(error)),
    };
    let header = frame.header();
    if header.kind() != FORMAT_FILE_KIND {
        return Err(FormatProbeError::UnexpectedFrameKind {
            actual: header.kind(),
        });
    }
    if !frame.payload().is_empty() {
        return Err(FormatProbeError::UnexpectedPayload);
    }
    let unsupported_required = header.required_flags() & !SUPPORTED_REQUIRED_CAPABILITIES;
    if unsupported_required != 0 {
        return Err(FormatProbeError::UnsupportedRequiredCapabilities {
            flags: unsupported_required,
        });
    }
    Ok(FormatCapabilities {
        required_flags: header.required_flags(),
        optional_flags: header.optional_flags(),
    })
}

#[cfg(test)]
mod tests {
    use worlddb_core::{FrameHeader, encode_frame};

    use super::{FORMAT_FILE_KIND, FormatCapabilities, FormatProbeError, probe_format};

    #[test]
    fn unknown_optional_capability_bits_are_retained_without_interpretation() -> Result<(), String>
    {
        let flags = 0x8000_0042_0000_0001;
        let frame = encode_frame(
            FrameHeader::new(FORMAT_FILE_KIND).with_optional_flags(flags),
            &[],
        )
        .map_err(|error| error.to_string())?;
        let capabilities = probe_format(&frame).map_err(|error| error.to_string())?;
        assert_eq!(capabilities.optional_flags(), flags);
        assert_eq!(capabilities.unknown_optional_flags(), flags);
        assert_eq!(capabilities.required_flags(), 0);
        let resaved = capabilities.encode().map_err(|error| error.to_string())?;
        assert_eq!(probe_format(&resaved), Ok(capabilities));
        Ok(())
    }

    #[test]
    fn invalid_kind_payload_and_length_fail_closed() -> Result<(), String> {
        let wrong_kind =
            encode_frame(FrameHeader::new(7), &[]).map_err(|error| error.to_string())?;
        assert!(matches!(
            probe_format(&wrong_kind),
            Err(FormatProbeError::UnexpectedFrameKind { actual: 7 })
        ));

        let payload = encode_frame(FrameHeader::new(FORMAT_FILE_KIND), &[1])
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            probe_format(&payload),
            Err(FormatProbeError::InvalidFileLength { .. })
        ));

        assert!(matches!(
            probe_format(&[]),
            Err(FormatProbeError::InvalidFileLength { .. })
        ));
        Ok(())
    }

    #[test]
    fn current_capability_profile_is_empty() {
        assert_eq!(FormatCapabilities::current().required_flags(), 0);
        assert_eq!(FormatCapabilities::current().optional_flags(), 0);
    }
}
