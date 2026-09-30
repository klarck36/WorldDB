//! Typed integer and exact decimal scalars with canonical encodings.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A signed 128-bit WorldDB integer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Int(i128);

impl Int {
    /// Constructs an integer without narrowing or coercion.
    #[must_use]
    pub const fn new(value: i128) -> Self {
        Self(value)
    }

    /// Returns the exact signed value.
    #[must_use]
    pub const fn value(self) -> i128 {
        self.0
    }

    /// Encodes this value as a minimal ZigZag unsigned LEB128 varint.
    #[must_use]
    pub fn to_canonical_bytes(self) -> Vec<u8> {
        encode_u128_varint(zigzag_encode(self.0))
    }

    /// Decodes exactly one minimal ZigZag unsigned LEB128 varint.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IntegerError> {
        let encoded = decode_u128_varint(bytes)?;
        Ok(Self(zigzag_decode(encoded)))
    }
}

impl FromStr for Int {
    type Err = IntegerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let negative = value.starts_with('-');
        let digits = if negative { &value[1..] } else { value };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(IntegerError::InvalidText);
        }
        if !is_canonical_unsigned_text(digits) || (negative && digits == "0") {
            return Err(IntegerError::NonCanonicalText);
        }
        value
            .parse::<i128>()
            .map(Self)
            .map_err(|_| IntegerError::Overflow)
    }
}

impl fmt::Display for Int {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// An unsigned 128-bit WorldDB integer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UInt(u128);

impl UInt {
    /// Constructs an integer without narrowing or coercion.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Returns the exact unsigned value.
    #[must_use]
    pub const fn value(self) -> u128 {
        self.0
    }

    /// Encodes this value as a minimal unsigned LEB128 varint.
    #[must_use]
    pub fn to_canonical_bytes(self) -> Vec<u8> {
        encode_u128_varint(self.0)
    }

    /// Decodes exactly one minimal unsigned LEB128 varint.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IntegerError> {
        decode_u128_varint(bytes).map(Self)
    }
}

impl FromStr for UInt {
    type Err = IntegerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(IntegerError::InvalidText);
        }
        if !is_canonical_unsigned_text(value) {
            return Err(IntegerError::NonCanonicalText);
        }
        value
            .parse::<u128>()
            .map(Self)
            .map_err(|_| IntegerError::Overflow)
    }
}

impl fmt::Display for UInt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// An exact decimal normalized to a sign, unsigned coefficient, and `i32` scale.
///
/// Its value is `coefficient * 10^(-scale)`, with the sign applied separately.
/// Nonzero coefficients never have a trailing decimal zero. Zero is always
/// positive with scale zero, which makes structural equality and hashing exact
/// numeric equality.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Decimal {
    negative: bool,
    coefficient: u128,
    scale: i32,
}

impl Decimal {
    /// Constructs and normalizes an exact decimal value.
    pub fn new(negative: bool, coefficient: u128, scale: i32) -> Result<Self, DecimalError> {
        if coefficient == 0 {
            return Ok(Self {
                negative: false,
                coefficient: 0,
                scale: 0,
            });
        }

        let mut normalized_coefficient = coefficient;
        let mut normalized_scale = scale;
        while normalized_coefficient % 10 == 0 {
            normalized_coefficient /= 10;
            normalized_scale = normalized_scale
                .checked_sub(1)
                .ok_or(DecimalError::ScaleOutOfRange)?;
        }
        Ok(Self {
            negative,
            coefficient: normalized_coefficient,
            scale: normalized_scale,
        })
    }

    /// Parses a decimal spelling only if it is already the unique canonical text.
    pub fn from_canonical_string(value: &str) -> Result<Self, DecimalError> {
        let decimal = Self::from_str(value)?;
        if decimal.to_canonical_string(value.len())? != value {
            return Err(DecimalError::NonCanonicalText);
        }
        Ok(decimal)
    }

    /// Returns whether the nonzero value is negative.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.negative
    }

    /// Returns the normalized unsigned coefficient.
    #[must_use]
    pub const fn coefficient(self) -> u128 {
        self.coefficient
    }

    /// Returns the normalized base-ten scale.
    #[must_use]
    pub const fn scale(self) -> i32 {
        self.scale
    }

    /// Encodes sign, minimal unsigned coefficient varint, and fixed-width `i32le` scale.
    #[must_use]
    pub fn to_canonical_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24);
        bytes.push(u8::from(self.negative));
        bytes.extend(encode_u128_varint(self.coefficient));
        bytes.extend(self.scale.to_le_bytes());
        bytes
    }

    /// Decodes one canonical decimal and rejects alternate representations.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, DecimalError> {
        if bytes.len() < 6 {
            return Err(DecimalError::InvalidEncoding);
        }
        let sign_byte = bytes
            .first()
            .copied()
            .ok_or(DecimalError::InvalidEncoding)?;
        let negative = match sign_byte {
            0 => false,
            1 => true,
            _ => return Err(DecimalError::InvalidSign),
        };
        let scale_offset = bytes.len() - 4;
        let coefficient_bytes = bytes
            .get(1..scale_offset)
            .ok_or(DecimalError::InvalidEncoding)?;
        let scale_slice = bytes
            .get(scale_offset..)
            .ok_or(DecimalError::InvalidEncoding)?;
        let coefficient =
            decode_u128_varint(coefficient_bytes).map_err(DecimalError::IntegerEncoding)?;
        let scale_bytes: [u8; 4] = scale_slice
            .try_into()
            .map_err(|_| DecimalError::InvalidEncoding)?;
        let scale = i32::from_le_bytes(scale_bytes);
        let decimal = Self::new(negative, coefficient, scale)?;
        if decimal.negative != negative
            || decimal.coefficient != coefficient
            || decimal.scale != scale
        {
            return Err(DecimalError::NonCanonicalEncoding);
        }
        Ok(decimal)
    }

    /// Formats the unique plain-decimal spelling, subject to an explicit output limit.
    ///
    /// The limit prevents large `i32` scales from causing unbounded string
    /// allocation. A caller with a larger transport budget may pass that budget.
    pub fn to_canonical_string(self, max_bytes: usize) -> Result<String, DecimalError> {
        if self.coefficient == 0 {
            return if max_bytes >= 1 {
                Ok(String::from("0"))
            } else {
                Err(DecimalError::OutputLimit)
            };
        }

        let digits = self.coefficient.to_string();
        let sign_bytes = usize::from(self.negative);
        let scale = i64::from(self.scale);
        let body_bytes = if scale <= 0 {
            (digits.len() as u128) + ((-scale) as u128)
        } else if scale >= digits.len() as i64 {
            2 + ((scale - digits.len() as i64) as u128) + digits.len() as u128
        } else {
            digits.len() as u128 + 1
        };
        let output_bytes = body_bytes + sign_bytes as u128;
        if output_bytes > max_bytes as u128 {
            return Err(DecimalError::OutputLimit);
        }

        let mut output = String::with_capacity(output_bytes as usize);
        if self.negative {
            output.push('-');
        }
        if scale <= 0 {
            output.push_str(&digits);
            output.extend(std::iter::repeat_n('0', (-scale) as usize));
        } else if scale >= digits.len() as i64 {
            output.push_str("0.");
            output.extend(std::iter::repeat_n(
                '0',
                (scale - digits.len() as i64) as usize,
            ));
            output.push_str(&digits);
        } else {
            let decimal_point = digits.len() - scale as usize;
            output.push_str(&digits[..decimal_point]);
            output.push('.');
            output.push_str(&digits[decimal_point..]);
        }
        Ok(output)
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;

    /// Parses plain decimal notation without binary floating-point conversion.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (negative, unsigned) = match value.strip_prefix('-') {
            Some(unsigned) => (true, unsigned),
            None => (false, value),
        };
        if unsigned.is_empty() {
            return Err(DecimalError::InvalidText);
        }
        let mut pieces = unsigned.split('.');
        let integer = pieces.next().ok_or(DecimalError::InvalidText)?;
        let fraction = pieces.next();
        if pieces.next().is_some()
            || integer.is_empty()
            || !integer.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.is_some_and(|part| {
                part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            return Err(DecimalError::InvalidText);
        }

        let fractional_digits = fraction.map_or(0, str::len);
        let total_digits = integer
            .len()
            .checked_add(fractional_digits)
            .ok_or(DecimalError::CoefficientOverflow)?;
        let fraction = fraction.unwrap_or("");
        let digit_stream = integer.bytes().chain(fraction.bytes());
        let Some(first_nonzero) = digit_stream.clone().position(|byte| byte != b'0') else {
            return Self::new(false, 0, 0);
        };
        let trailing_zeros = fraction
            .bytes()
            .rev()
            .chain(integer.bytes().rev())
            .take_while(|byte| *byte == b'0')
            .count();
        let coefficient_end = total_digits - trailing_zeros;
        let mut coefficient = 0_u128;
        for (index, byte) in digit_stream.enumerate() {
            if (first_nonzero..coefficient_end).contains(&index) {
                coefficient = coefficient
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(u128::from(byte - b'0')))
                    .ok_or(DecimalError::CoefficientOverflow)?;
            }
        }
        let mut scale =
            i64::try_from(fractional_digits).map_err(|_| DecimalError::ScaleOutOfRange)?;
        scale = scale
            .checked_sub(i64::try_from(trailing_zeros).map_err(|_| DecimalError::ScaleOutOfRange)?)
            .ok_or(DecimalError::ScaleOutOfRange)?;
        let scale = i32::try_from(scale).map_err(|_| DecimalError::ScaleOutOfRange)?;
        Self::new(negative, coefficient, scale)
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => compare_magnitude(*other, *self),
            (false, false) => compare_magnitude(*self, *other),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Invalid canonical integer text or integer wire encoding.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntegerError {
    /// Input was empty or did not follow the decimal integer grammar.
    InvalidText,
    /// Input text had a noncanonical sign or leading zero.
    NonCanonicalText,
    /// The integer exceeded the target 128-bit range.
    Overflow,
    /// The varint had no bytes.
    EmptyEncoding,
    /// The varint ended before its terminating byte.
    UnterminatedEncoding,
    /// The varint used more bytes than its value requires.
    NonCanonicalEncoding,
    /// Bytes followed a complete scalar encoding.
    TrailingBytes,
}

impl fmt::Display for IntegerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidText => formatter.write_str("invalid integer text"),
            Self::NonCanonicalText => formatter.write_str("integer text is not canonical"),
            Self::Overflow => formatter.write_str("integer exceeds the 128-bit range"),
            Self::EmptyEncoding => formatter.write_str("integer encoding is empty"),
            Self::UnterminatedEncoding => formatter.write_str("integer varint is unterminated"),
            Self::NonCanonicalEncoding => formatter.write_str("integer varint is not minimal"),
            Self::TrailingBytes => formatter.write_str("integer encoding has trailing bytes"),
        }
    }
}

impl std::error::Error for IntegerError {}

/// Invalid exact decimal text, scale, or canonical wire representation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DecimalError {
    /// Text was not plain decimal notation.
    InvalidText,
    /// Text was valid decimal notation but not the unique normalized spelling.
    NonCanonicalText,
    /// The coefficient does not fit in `u128`.
    CoefficientOverflow,
    /// Normalization or parsing exceeded the `i32` scale range.
    ScaleOutOfRange,
    /// Decimal bytes used a sign byte other than positive `0` or negative `1`.
    InvalidSign,
    /// Decimal bytes were truncated or structurally malformed.
    InvalidEncoding,
    /// The input was valid in shape but not the unique normalized representation.
    NonCanonicalEncoding,
    /// The coefficient varint was malformed or nonminimal.
    IntegerEncoding(IntegerError),
    /// The requested plain-decimal text would exceed the caller's byte budget.
    OutputLimit,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidText => formatter.write_str("invalid decimal text"),
            Self::NonCanonicalText => formatter.write_str("decimal text is not canonical"),
            Self::CoefficientOverflow => formatter.write_str("decimal coefficient exceeds u128"),
            Self::ScaleOutOfRange => formatter.write_str("decimal scale exceeds i32"),
            Self::InvalidSign => formatter.write_str("decimal sign byte is invalid"),
            Self::InvalidEncoding => formatter.write_str("decimal encoding is malformed"),
            Self::NonCanonicalEncoding => formatter.write_str("decimal encoding is not canonical"),
            Self::IntegerEncoding(error) => write!(formatter, "decimal coefficient: {error}"),
            Self::OutputLimit => formatter.write_str("decimal text exceeds the output limit"),
        }
    }
}

impl std::error::Error for DecimalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IntegerEncoding(error) => Some(error),
            _ => None,
        }
    }
}

fn is_canonical_unsigned_text(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

pub(crate) fn encode_u128_varint(mut value: u128) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(19);
    while value >= 0x80 {
        bytes.push(((value as u8) & 0x7f) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}

fn decode_u128_varint(bytes: &[u8]) -> Result<u128, IntegerError> {
    let (value, encoded_len) = decode_u128_varint_prefix(bytes)?;
    if encoded_len != bytes.len() {
        return Err(IntegerError::TrailingBytes);
    }
    Ok(value)
}

pub(crate) fn decode_u128_varint_prefix(bytes: &[u8]) -> Result<(u128, usize), IntegerError> {
    if bytes.is_empty() {
        return Err(IntegerError::EmptyEncoding);
    }
    let mut value = 0_u128;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index >= 19 {
            return Err(IntegerError::Overflow);
        }
        let payload = u128::from(byte & 0x7f);
        if index == 18 && payload > 3 {
            return Err(IntegerError::Overflow);
        }
        value |= payload << (index * 7);
        if byte & 0x80 == 0 {
            if index > 0 && payload == 0 {
                return Err(IntegerError::NonCanonicalEncoding);
            }
            return Ok((value, index + 1));
        }
    }
    Err(IntegerError::UnterminatedEncoding)
}

const fn zigzag_encode(value: i128) -> u128 {
    ((value as u128) << 1) ^ ((value >> 127) as u128)
}

const fn zigzag_decode(value: u128) -> i128 {
    let magnitude = (value >> 1) as i128;
    if value & 1 == 0 {
        magnitude
    } else {
        -magnitude - 1
    }
}

fn compare_magnitude(left: Decimal, right: Decimal) -> Ordering {
    match (left.coefficient == 0, right.coefficient == 0) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }

    let left_digits = left.coefficient.to_string();
    let right_digits = right.coefficient.to_string();
    let left_exponent = left_digits.len() as i64 - 1 - i64::from(left.scale);
    let right_exponent = right_digits.len() as i64 - 1 - i64::from(right.scale);
    match left_exponent.cmp(&right_exponent) {
        Ordering::Equal => {
            let width = left_digits.len().max(right_digits.len());
            for index in 0..width {
                let left_digit = left_digits.as_bytes().get(index).copied().unwrap_or(b'0');
                let right_digit = right_digits.as_bytes().get(index).copied().unwrap_or(b'0');
                match left_digit.cmp(&right_digit) {
                    Ordering::Equal => {}
                    order => return order,
                }
            }
            Ordering::Equal
        }
        order => order,
    }
}

#[cfg(test)]
mod tests {
    use super::{Decimal, DecimalError, Int, IntegerError, UInt};
    use std::str::FromStr;

    #[test]
    fn signed_and_unsigned_integer_boundaries_round_trip_canonically() {
        for value in [i128::MIN, -1, 0, 1, i128::MAX] {
            let integer = Int::new(value);
            let bytes = integer.to_canonical_bytes();
            assert_eq!(Int::from_canonical_bytes(&bytes), Ok(integer));
        }
        for value in [0, 1, u128::MAX] {
            let integer = UInt::new(value);
            let bytes = integer.to_canonical_bytes();
            assert_eq!(UInt::from_canonical_bytes(&bytes), Ok(integer));
        }
    }

    #[test]
    fn integer_text_is_canonical_and_never_narrows() {
        assert_eq!(
            Int::from_str("-170141183460469231731687303715884105728"),
            Ok(Int::new(i128::MIN))
        );
        assert_eq!(
            UInt::from_str("340282366920938463463374607431768211455"),
            Ok(UInt::new(u128::MAX))
        );
        assert_eq!(Int::from_str("-0"), Err(IntegerError::NonCanonicalText));
        assert_eq!(Int::from_str("+1"), Err(IntegerError::InvalidText));
        assert_eq!(Int::from_str("01"), Err(IntegerError::NonCanonicalText));
        assert_eq!(UInt::from_str("01"), Err(IntegerError::NonCanonicalText));
        assert_eq!(
            Int::from_str("170141183460469231731687303715884105728"),
            Err(IntegerError::Overflow)
        );
        assert_eq!(
            UInt::from_str("340282366920938463463374607431768211456"),
            Err(IntegerError::Overflow)
        );
    }

    #[test]
    fn integer_varints_reject_nonminimal_truncated_and_overflowing_forms() {
        assert_eq!(
            UInt::from_canonical_bytes(&[0x80, 0]),
            Err(IntegerError::NonCanonicalEncoding)
        );
        assert_eq!(
            UInt::from_canonical_bytes(&[0x80]),
            Err(IntegerError::UnterminatedEncoding)
        );
        assert_eq!(
            UInt::from_canonical_bytes(&[0xff; 20]),
            Err(IntegerError::Overflow)
        );
        assert_eq!(
            UInt::from_canonical_bytes(&[1, 0]),
            Err(IntegerError::TrailingBytes)
        );
    }

    #[test]
    fn integer_and_decimal_encodings_match_canonical_vectors() {
        assert_eq!(Int::new(-1).to_canonical_bytes(), [1]);
        assert_eq!(UInt::new(128).to_canonical_bytes(), [0x80, 1]);

        let decimal = Decimal::new(false, 120, 2);
        assert_eq!(decimal, Decimal::new(false, 12, 1));
        if let Ok(decimal) = decimal {
            assert_eq!(decimal.to_canonical_bytes(), [0, 12, 1, 0, 0, 0]);
        }
    }

    #[test]
    fn decimal_scale_normalizes_without_changing_numeric_identity() {
        let one = Decimal::new(false, 1, 0);
        let one_tenth_parts = Decimal::new(false, 10, 1);
        let hundred_hundredths = Decimal::new(false, 100, 2);
        assert_eq!(one, one_tenth_parts);
        assert_eq!(one, hundred_hundredths);
        assert_eq!(Decimal::from_str("1.00"), one);
        assert_eq!(Decimal::from_str("001.000"), one);
        assert_eq!(Decimal::from_canonical_string("1"), one);
        assert_eq!(
            Decimal::from_canonical_string("1.00"),
            Err(DecimalError::NonCanonicalText)
        );
        assert_eq!(Decimal::from_str("-0.000"), Decimal::new(false, 0, 0));
        assert!(Decimal::from_str("340282366920938463463374607431768211455").is_ok());
        assert_eq!(
            Decimal::from_str("340282366920938463463374607431768211456"),
            Err(DecimalError::CoefficientOverflow)
        );
        assert_eq!(Decimal::from_str("1e0"), Err(DecimalError::InvalidText));
        assert_eq!(
            Decimal::new(false, 10, i32::MIN),
            Err(DecimalError::ScaleOutOfRange)
        );
    }

    #[test]
    fn decimal_numeric_order_is_exact_across_scales_and_signs() {
        let negative_two = Decimal::from_str("-2");
        let negative_point = Decimal::from_str("-1.99");
        let zero = Decimal::from_str("0");
        let small_positive = Decimal::from_str("0.000001");
        let positive_one = Decimal::from_str("1.2");
        let equal_scale_variant = Decimal::from_str("1.20");
        if let (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e), Ok(f)) = (
            negative_two,
            negative_point,
            zero,
            small_positive,
            positive_one,
            equal_scale_variant,
        ) {
            assert!(a < b && b < c && c < d && d < e);
            assert_eq!(e, f);
        }
    }

    #[test]
    fn decimal_bytes_round_trip_and_reject_noncanonical_alternatives() {
        let decimal = Decimal::from_str("-123.4500");
        assert!(decimal.is_ok());
        if let Ok(decimal) = decimal {
            let bytes = decimal.to_canonical_bytes();
            assert_eq!(Decimal::from_canonical_bytes(&bytes), Ok(decimal));
            assert_eq!(decimal.to_canonical_string(32), Ok(String::from("-123.45")));
        }

        assert_eq!(
            Decimal::from_canonical_bytes(&[0, 10, 1, 0, 0, 0]),
            Err(DecimalError::NonCanonicalEncoding)
        );
        assert_eq!(
            Decimal::from_canonical_bytes(&[1, 0, 0, 0, 0, 0]),
            Err(DecimalError::NonCanonicalEncoding)
        );
        assert_eq!(
            Decimal::from_canonical_bytes(&[2, 0, 0, 0, 0, 0]),
            Err(DecimalError::InvalidSign)
        );
        assert_eq!(
            Decimal::from_canonical_bytes(&[0, 0x81, 0, 0, 0, 0, 0]),
            Err(DecimalError::IntegerEncoding(
                IntegerError::NonCanonicalEncoding
            ))
        );
        assert_eq!(
            Decimal::new(true, 1, i32::MAX)
                .and_then(|value| value.to_canonical_string(8).map(|_| value)),
            Err(DecimalError::OutputLimit)
        );
    }
}
