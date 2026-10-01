//! Closed core assertion values and their validated scalar wrappers.

use std::fmt;
use std::str::FromStr;

use crate::ids::{EntityId, TimelineId};
use crate::temporal::Duration;
use crate::{Decimal, Int, UInt};

/// A schema symbol restricted to ASCII lowercase letters, digits, and `_`.
///
/// The first byte must be an ASCII lowercase letter; later bytes may also be
/// ASCII digits or underscores. Symbols are byte-exact and are never Unicode
/// normalized. The grammar is `[a-z][a-z0-9_]*`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Symbol(String);

impl Symbol {
    /// Validates and owns a value matching `[a-z][a-z0-9_]*`.
    pub fn new(value: impl Into<String>) -> Result<Self, SymbolError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return Err(SymbolError::Empty);
        };
        if !first.is_ascii_lowercase() {
            return Err(SymbolError::InvalidFirstByte(first));
        }
        for (index, byte) in bytes.enumerate() {
            if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
                return Err(SymbolError::InvalidByte {
                    index: index + 1,
                    byte,
                });
            }
        }
        Ok(Self(value))
    }

    /// Returns the validated, byte-exact symbol text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the owned symbol text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Symbol {
    type Err = SymbolError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// A rejected schema symbol, with the offending byte when applicable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolError {
    /// Symbols must contain at least one byte.
    Empty,
    /// The first byte was not an ASCII lowercase letter.
    InvalidFirstByte(u8),
    /// A later byte was not an ASCII lowercase letter, digit, or underscore.
    InvalidByte {
        /// Zero-based byte position in the input.
        index: usize,
        /// The byte that violated the grammar.
        byte: u8,
    },
}

impl fmt::Display for SymbolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("symbol must not be empty"),
            Self::InvalidFirstByte(byte) => write!(
                formatter,
                "symbol byte 0 is 0x{byte:02x}; expected an ASCII lowercase letter"
            ),
            Self::InvalidByte { index, byte } => write!(
                formatter,
                "symbol byte {index} is 0x{byte:02x}; expected ASCII lowercase, digit, or underscore"
            ),
        }
    }
}

impl std::error::Error for SymbolError {}

/// An owned byte sequence used as a core assertion value.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Bytes(Vec<u8>);

impl Bytes {
    /// Stores the supplied bytes without interpreting them as text.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Returns the exact byte sequence.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns the owned byte sequence.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }

    /// Returns the byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the sequence is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(bytes)
    }
}

impl From<&[u8]> for Bytes {
    fn from(bytes: &[u8]) -> Self {
        Self::new(bytes.to_vec())
    }
}

/// A time value retaining its timeline, signed ticks, and registered unit symbol.
///
/// The selected schema snapshot must resolve the TimelineId and unit Symbol
/// before comparison or acceptance. No equality or ordering trait is provided
/// for unresolved input: equal instants expressed in different units require
/// that schema resolution. `WorldTime` is the checked comparison coordinate.
#[derive(Clone, Debug)]
pub struct Time {
    timeline_id: TimelineId,
    ticks: i128,
    unit: Symbol,
}

impl Time {
    /// Creates a representation-preserving time coordinate.
    ///
    /// This constructor validates the value shape. A schema-aware operation
    /// must still resolve the timeline and unit and check normalization.
    #[must_use]
    pub const fn new(timeline_id: TimelineId, ticks: i128, unit: Symbol) -> Self {
        Self {
            timeline_id,
            ticks,
            unit,
        }
    }

    /// Returns the stable timeline identity.
    #[must_use]
    pub const fn timeline_id(&self) -> TimelineId {
        self.timeline_id
    }

    /// Returns the original signed tick count.
    #[must_use]
    pub const fn ticks(&self) -> i128 {
        self.ticks
    }

    /// Returns the registered unit symbol that gives the ticks their scale.
    #[must_use]
    pub fn unit(&self) -> &Symbol {
        &self.unit
    }
}

/// The complete set of 1.0 assertion value types.
///
/// This enum intentionally has no conversion from floats or containers and no
/// `Ord` implementation. It also does not derive equality: `Time` equality is
/// based on schema-resolved nanoseconds within a timeline, not its retained
/// tick/unit spelling. Callers must compare resolved values under the selected
/// schema snapshot.
#[derive(Clone, Debug)]
pub enum Value {
    /// A boolean scalar.
    Bool(bool),
    /// A signed 128-bit integer.
    Int(Int),
    /// An unsigned 128-bit integer.
    UInt(UInt),
    /// An exact, canonical decimal number.
    Decimal(Decimal),
    /// Valid UTF-8 text, compared byte-for-byte without normalization.
    String(String),
    /// A validated schema symbol.
    Symbol(Symbol),
    /// A typed reference to an entity.
    Entity(EntityId),
    /// A timeline coordinate whose unit is resolved by schema.
    Time(Time),
    /// A signed physical duration in nanoseconds.
    Duration(Duration),
    /// An uninterpreted byte sequence.
    Bytes(Bytes),
}

/// Compares canonical scalar variants and signals when temporal schema
/// resolution is required before equality can be decided.
pub(crate) fn canonical_value_equality(left: &Value, right: &Value) -> Option<bool> {
    Some(match (left, right) {
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Int(left), Value::Int(right)) => left == right,
        (Value::UInt(left), Value::UInt(right)) => left == right,
        (Value::Decimal(left), Value::Decimal(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Symbol(left), Value::Symbol(right)) => left == right,
        (Value::Entity(left), Value::Entity(right)) => left == right,
        (Value::Duration(left), Value::Duration(right)) => left == right,
        (Value::Bytes(left), Value::Bytes(right)) => left == right,
        (Value::Time(_), Value::Time(_)) => return None,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::{Bytes, Symbol, SymbolError, Time, Value};
    use crate::ids::TimelineId;
    use crate::temporal::Duration;
    use crate::{Decimal, Int, UInt};
    use std::str::FromStr;

    #[test]
    fn symbol_grammar_accepts_only_ascii_lowercase_identifiers() {
        for valid in ["a", "ns", "unit_2", "a0_b9"] {
            let parsed = Symbol::from_str(valid);
            assert!(parsed.is_ok(), "rejected valid symbol {valid:?}");
            if let Ok(symbol) = parsed {
                assert_eq!(symbol.as_str(), valid);
            }
        }

        for invalid in ["", "_name", "2name", "Upper", "a-b", "a.b", "naïve"] {
            assert!(Symbol::from_str(invalid).is_err(), "accepted {invalid:?}");
        }
        assert_eq!(
            Symbol::from_str("a-b"),
            Err(SymbolError::InvalidByte {
                index: 1,
                byte: b'-'
            })
        );
        assert_eq!(
            Symbol::from_str("É"),
            Err(SymbolError::InvalidFirstByte(0xc3))
        );
    }

    #[test]
    fn strings_remain_utf8_and_byte_exact_while_symbols_are_restricted() {
        let text = String::from("Grüße\0 世界");
        let value = Value::String(text.clone());
        assert!(matches!(value, Value::String(stored) if stored == text));
        assert!(Symbol::new("Grüße").is_err());
    }

    #[test]
    fn time_retains_its_timeline_ticks_and_unit_representation() {
        let timeline_id = TimelineId::from_str("00000000-0000-7000-8000-000000000007");
        let unit = Symbol::new("ns");
        assert!(timeline_id.is_ok());
        assert!(unit.is_ok());
        if let (Ok(timeline_id), Ok(unit)) = (timeline_id, unit) {
            let time = Time::new(timeline_id, i128::MIN, unit);
            assert_eq!(time.timeline_id(), timeline_id);
            assert_eq!(time.ticks(), i128::MIN);
            assert_eq!(time.unit().as_str(), "ns");
        }
    }

    #[test]
    fn closed_value_catalog_constructs_each_allowed_variant() {
        let decimal = Decimal::from_str("1.00");
        let symbol = Symbol::new("label");
        let entity_id = crate::EntityId::from_str("00000000-0000-7000-8000-000000000009");
        let timeline_id = TimelineId::from_str("00000000-0000-7000-8000-000000000007");
        let unit = Symbol::new("ms");
        assert!(decimal.is_ok());
        assert!(symbol.is_ok());
        assert!(entity_id.is_ok());
        assert!(timeline_id.is_ok());
        assert!(unit.is_ok());

        if let (Ok(decimal), Ok(symbol), Ok(entity_id), Ok(timeline_id), Ok(unit)) =
            (decimal, symbol, entity_id, timeline_id, unit)
        {
            let values = [
                Value::Bool(true),
                Value::Int(Int::new(i128::MIN)),
                Value::UInt(UInt::new(u128::MAX)),
                Value::Decimal(decimal),
                Value::String(String::from("text")),
                Value::Symbol(symbol),
                Value::Entity(entity_id),
                Value::Time(Time::new(timeline_id, 42, unit)),
                Value::Duration(Duration::from_nanoseconds(-1)),
                Value::Bytes(Bytes::new([0, 0xff, b'x'])),
            ];

            assert_eq!(values.len(), 10);
            assert!(
                matches!(&values[9], Value::Bytes(bytes) if bytes.as_slice() == [0, 0xff, b'x'])
            );
        }
    }
}
