use std::str::FromStr;

use worlddb_core::{
    Bytes, Decimal, Duration, EntityId, FrameHeader, Int, Symbol, Time, TimelineId, UInt, Value,
    decode_frame, decode_value, encode_frame, encode_value,
};

const GOLDEN_VECTORS: &str = include_str!("data/wire-v1.0-golden.tsv");
const RECORD_GOLDEN_VECTORS: &str = include_str!("data/record-v1.0-golden.tsv");
const AUDIT_GOLDEN_VECTORS: &str = include_str!("data/audit-v1.0-golden.tsv");

#[derive(Clone, Copy, Debug)]
struct OracleFrame<'a> {
    major: u16,
    minor: u16,
    required_flags: u64,
    optional_flags: u64,
    kind: u32,
    payload: &'a [u8],
}

fn golden(name: &str) -> Option<Vec<u8>> {
    for line in GOLDEN_VECTORS.lines() {
        let mut columns = line.split('\t');
        if columns.next() != Some(name) {
            continue;
        }
        return columns.next().and_then(decode_hex);
    }
    None
}

fn decode_hex(input: &str) -> Option<Vec<u8>> {
    let digits = input.as_bytes();
    if digits.len() % 2 != 0 {
        return None;
    }
    let mut output = Vec::with_capacity(digits.len() / 2);
    let mut offset = 0_usize;
    while offset < digits.len() {
        let high = *digits.get(offset)?;
        let low_offset = offset.checked_add(1)?;
        let low = *digits.get(low_offset)?;
        output.push((hex_nibble(high)? << 4) | hex_nibble(low)?);
        offset = offset.checked_add(2)?;
    }
    Some(output)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn fixed<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], &'static str> {
    let end = offset.checked_add(N).ok_or("offset overflow")?;
    bytes
        .get(offset..end)
        .and_then(|slice| slice.try_into().ok())
        .ok_or("truncated fixed-width field")
}

/// Deliberately separate frame parser; it does not call the production decoder.
fn decode_frame_oracle(bytes: &[u8]) -> Result<OracleFrame<'_>, &'static str> {
    const HEADER: usize = 40;
    const DIGEST: usize = 32;
    const MAGIC: &[u8; 8] = b"WorldDB\0";
    if bytes.len() < HEADER + DIGEST {
        return Err("truncated frame");
    }
    if bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err("bad magic");
    }
    let major = u16::from_le_bytes(fixed::<2>(bytes, 8)?);
    let minor = u16::from_le_bytes(fixed::<2>(bytes, 10)?);
    let required_flags = u64::from_le_bytes(fixed::<8>(bytes, 12)?);
    let optional_flags = u64::from_le_bytes(fixed::<8>(bytes, 20)?);
    let kind = u32::from_le_bytes(fixed::<4>(bytes, 28)?);
    let body_len = usize::try_from(u64::from_le_bytes(fixed::<8>(bytes, 32)?))
        .map_err(|_| "payload length overflow")?;
    if major != 1 || minor != 0 {
        return Err("unsupported version");
    }
    if required_flags != 0 {
        return Err("unsupported required flags");
    }
    let body_end = HEADER
        .checked_add(body_len)
        .ok_or("payload length overflow")?;
    let frame_end = body_end
        .checked_add(DIGEST)
        .ok_or("frame length overflow")?;
    if frame_end != bytes.len() {
        return Err("frame length mismatch");
    }
    let covered = bytes.get(..body_end).ok_or("truncated checksum input")?;
    let checksum = bytes.get(body_end..frame_end).ok_or("truncated checksum")?;
    if blake3::hash(covered).as_bytes().as_slice() != checksum {
        return Err("checksum mismatch");
    }
    let payload = bytes.get(HEADER..body_end).ok_or("truncated payload")?;
    Ok(OracleFrame {
        major,
        minor,
        required_flags,
        optional_flags,
        kind,
        payload,
    })
}

/// Independent unsigned LEB128 reader used by the oracle below.
fn read_leb128(bytes: &[u8], cursor: &mut usize) -> Result<u128, &'static str> {
    let start = *cursor;
    let mut value = 0_u128;
    for shift in (0..=126).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or("unterminated varint")?;
        *cursor = cursor.checked_add(1).ok_or("cursor overflow")?;
        let payload = u128::from(byte & 0x7f);
        if shift == 126 && payload > 3 {
            return Err("u128 varint overflow");
        }
        value |= payload << shift;
        if byte & 0x80 == 0 {
            if *cursor - start > 1 && payload == 0 {
                return Err("nonminimal varint");
            }
            return Ok(value);
        }
    }
    Err("varint is longer than u128")
}

/// Deliberately separate TLV parser with independent ordering/length checks.
fn decode_tlv_oracle(bytes: &[u8]) -> Result<Vec<(u32, &[u8])>, &'static str> {
    let mut fields = Vec::new();
    let mut cursor = 0_usize;
    let mut previous = None;
    while cursor < bytes.len() {
        let tag =
            u32::try_from(read_leb128(bytes, &mut cursor)?).map_err(|_| "field tag overflow")?;
        let length = usize::try_from(read_leb128(bytes, &mut cursor)?)
            .map_err(|_| "field length overflow")?;
        if previous.is_some_and(|prior| tag <= prior) {
            return Err("fields are not strictly ordered");
        }
        let end = cursor.checked_add(length).ok_or("field length overflow")?;
        let value = bytes.get(cursor..end).ok_or("truncated field")?;
        fields.push((tag, value));
        previous = Some(tag);
        cursor = end;
    }
    Ok(fields)
}

/// Reads a closed value tag independently from the production value decoder.
fn decode_value_tag_oracle(bytes: &[u8]) -> Result<(u32, &[u8]), &'static str> {
    let mut cursor = 0_usize;
    let tag = u32::try_from(read_leb128(bytes, &mut cursor)?).map_err(|_| "value tag overflow")?;
    let payload = bytes.get(cursor..).ok_or("truncated value")?;
    if !(1..=10).contains(&tag) {
        return Err("unknown core value tag");
    }
    Ok((tag, payload))
}

#[test]
fn independent_decoder_and_production_decoder_agree_on_golden_frames() {
    let empty = golden("frame-empty-kind-01020304");
    assert!(empty.is_some(), "empty-frame golden vector is missing");
    if let Some(empty) = empty {
        let oracle = decode_frame_oracle(&empty);
        let production = decode_frame(&empty);
        assert!(oracle.is_ok(), "oracle failed: {oracle:?}");
        assert!(
            production.is_ok(),
            "production decoder failed: {production:?}"
        );
        if let (Ok(oracle), Ok(production)) = (oracle, production) {
            assert_eq!(oracle.major, 1);
            assert_eq!(oracle.minor, 0);
            assert_eq!(oracle.required_flags, 0);
            assert_eq!(oracle.optional_flags, 0);
            assert_eq!(oracle.kind, 0x0102_0304);
            assert!(oracle.payload.is_empty());
            assert_eq!(production.payload(), oracle.payload);
            assert_eq!(production.header().kind(), oracle.kind);
            assert_eq!(encode_frame(FrameHeader::new(0x0102_0304), &[]), Ok(empty));
        }
    }

    let record = golden("frame-record-kind-1-optional-2");
    assert!(record.is_some(), "record-frame golden vector is missing");
    if let Some(record) = record {
        let oracle = decode_frame_oracle(&record);
        let production = decode_frame(&record);
        assert!(oracle.is_ok(), "oracle failed: {oracle:?}");
        assert!(
            production.is_ok(),
            "production decoder failed: {production:?}"
        );
        if let (Ok(oracle), Ok(production)) = (oracle, production) {
            assert_eq!(oracle.optional_flags, 2);
            assert_eq!(oracle.kind, 1);
            let fields = decode_tlv_oracle(oracle.payload);
            assert!(fields.is_ok(), "TLV oracle failed: {fields:?}");
            if let Ok(fields) = fields {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields.first().map(|field| field.0), Some(1));
                assert_eq!(fields.first().map(|field| field.1), Some(&[2, 1][..]));
                assert_eq!(fields.get(1).map(|field| field.0), Some(3));
                assert_eq!(
                    fields.get(1).map(|field| field.1),
                    Some(&[4, 0, 1, 0, 0, 0, 0][..])
                );
            }

            let mut tlv = worlddb_core::TlvDecoder::new(production.payload());
            let first = tlv.next_field();
            assert!(
                matches!(first, Ok(Some(field)) if field.tag() == 1 && field.value() == [2, 1])
            );
            let second = tlv.next_field();
            assert!(
                matches!(second, Ok(Some(field)) if field.tag() == 3 && field.value() == [4, 0, 1, 0, 0, 0, 0])
            );
            assert_eq!(tlv.next_field(), Ok(None));

            let mut encoder = worlddb_core::TlvEncoder::new();
            assert!(encoder.push(1, &[2, 1]).is_ok());
            assert!(encoder.push(3, &[4, 0, 1, 0, 0, 0, 0]).is_ok());
            assert_eq!(encoder.finish(), oracle.payload);
            assert_eq!(
                encode_frame(FrameHeader::new(1).with_optional_flags(2), oracle.payload),
                Ok(record)
            );
        }
    }
}

#[test]
fn independent_frame_and_tlv_oracle_accepts_every_schema_project_golden() {
    let mut vector_count = 0;
    for (line_number, line) in RECORD_GOLDEN_VECTORS.lines().enumerate() {
        if line_number == 0 {
            continue;
        }
        let mut columns = line.split('\t');
        let name = columns.next().unwrap_or_default();
        let kind_hex = columns.next().unwrap_or_default();
        let bytes = columns.next().and_then(decode_hex);
        assert!(bytes.is_some(), "invalid record golden line {line_number}");
        let Some(bytes) = bytes else {
            continue;
        };
        let expected_kind = u32::from_str_radix(kind_hex, 16);
        assert!(expected_kind.is_ok(), "invalid kind for {name}");
        let Ok(expected_kind) = expected_kind else {
            continue;
        };
        let oracle = decode_frame_oracle(&bytes);
        assert!(oracle.is_ok(), "frame oracle rejected {name}: {oracle:?}");
        let Ok(oracle) = oracle else {
            continue;
        };
        assert_eq!(oracle.major, 1, "{name}");
        assert_eq!(oracle.minor, 0, "{name}");
        assert_eq!(oracle.required_flags, 0, "{name}");
        assert_eq!(oracle.optional_flags, 0, "{name}");
        assert_eq!(oracle.kind, expected_kind, "{name}");
        assert!(
            decode_tlv_oracle(oracle.payload).is_ok(),
            "TLV oracle rejected {name}"
        );
        let production = worlddb_core::decode_record(&bytes);
        assert!(
            production.is_ok(),
            "production decoder rejected {name}: {production:?}"
        );
        if let Ok(decoded) = production {
            assert_eq!(
                worlddb_core::encode_decoded_record(&decoded),
                Ok(bytes),
                "production codec changed {name}"
            );
        }
        vector_count += 1;
    }
    assert_eq!(vector_count, 52);
}

#[test]
fn independent_frame_and_tlv_oracle_accepts_every_separate_audit_golden() {
    let mut vector_count = 0;
    for (line_number, line) in AUDIT_GOLDEN_VECTORS.lines().enumerate() {
        if line_number == 0 {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "audit golden line {line_number}");
        let [name, raw_kind, frame_hex] = columns.as_slice() else {
            continue;
        };
        let expected_kind = u32::from_str_radix(raw_kind, 16);
        assert!(expected_kind.is_ok(), "invalid audit kind for {name}");
        let frame_bytes = decode_hex(frame_hex);
        assert!(frame_bytes.is_some(), "invalid audit frame hex for {name}");
        let Some(frame_bytes) = frame_bytes else {
            continue;
        };
        let Ok(expected_kind) = expected_kind else {
            continue;
        };
        let oracle = decode_frame_oracle(&frame_bytes);
        assert!(
            oracle.is_ok(),
            "audit frame oracle rejected {name}: {oracle:?}"
        );
        let Ok(oracle) = oracle else {
            continue;
        };
        assert_eq!(oracle.kind, expected_kind, "audit kind for {name}");
        assert_eq!(oracle.optional_flags, 0, "audit flags for {name}");
        let fields = decode_tlv_oracle(oracle.payload);
        assert!(
            fields.is_ok(),
            "audit TLV oracle rejected {name}: {fields:?}"
        );
        let Ok(fields) = fields else {
            continue;
        };
        let field_count = if expected_kind == 0xa001 { 10 } else { 9 };
        assert_eq!(fields.len(), field_count, "audit field count for {name}");
        for (index, (tag, _)) in fields.iter().enumerate() {
            assert_eq!(*tag, u32::try_from(index + 1).unwrap_or_default(), "{name}");
        }
        assert!(
            matches!(expected_kind, 0xa001 | 0xa002),
            "unregistered audit kind in golden file: {name}"
        );
        match expected_kind {
            0xa001 => {
                let decoded = worlddb_core::decode_audit_record(&frame_bytes);
                assert!(
                    decoded.is_ok(),
                    "audit decoder rejected {name}: {decoded:?}"
                );
                if let Ok(decoded) = decoded {
                    assert_eq!(worlddb_core::encode_audit_record(&decoded), Ok(frame_bytes));
                }
            }
            0xa002 => {
                let decoded = worlddb_core::decode_raw_read_attempt(&frame_bytes);
                assert!(
                    decoded.is_ok(),
                    "raw read decoder rejected {name}: {decoded:?}"
                );
                if let Ok(decoded) = decoded {
                    assert_eq!(
                        worlddb_core::encode_raw_read_attempt(&decoded),
                        Ok(frame_bytes)
                    );
                }
            }
            _ => continue,
        }
        vector_count += 1;
    }
    assert_eq!(vector_count, 3);
}

fn assert_value_golden(name: &str, expected_tag: u32, value: Value) {
    let expected = golden(name);
    assert!(expected.is_some(), "missing value golden vector {name}");
    if let Some(expected) = expected {
        assert_eq!(encode_value(&value), expected, "{name}");
        let oracle = decode_value_tag_oracle(&expected);
        assert!(oracle.is_ok(), "value oracle failed for {name}: {oracle:?}");
        if let Ok((tag, _)) = oracle {
            assert_eq!(tag, expected_tag, "{name}");
        }
        let decoded = decode_value(&expected);
        assert!(
            decoded.is_ok(),
            "value decoder failed for {name}: {decoded:?}"
        );
        if let Ok(decoded) = decoded {
            assert_eq!(encode_value(&decoded), expected, "{name}");
        }
    }
}

#[test]
fn all_ten_scalar_encodings_match_fixed_golden_vectors() {
    assert_value_golden("value-bool-true", 1, Value::Bool(true));
    assert_value_golden("value-int-minus-one", 2, Value::Int(Int::new(-1)));
    assert_value_golden("value-uint-128", 3, Value::UInt(UInt::new(128)));
    let decimal = Decimal::new(false, 1, 0);
    assert!(decimal.is_ok());
    if let Ok(decimal) = decimal {
        assert_value_golden("value-decimal-one", 4, Value::Decimal(decimal));
    }
    assert_value_golden("value-string-e-acute", 5, Value::String(String::from("é")));
    let symbol = Symbol::new(String::from("hour_1"));
    assert!(symbol.is_ok());
    if let Ok(symbol) = symbol {
        assert_value_golden("value-symbol-hour-1", 6, Value::Symbol(symbol));
    }
    let entity = EntityId::from_str("00000000-0000-7000-8000-000000000001");
    assert!(entity.is_ok());
    if let Ok(entity) = entity {
        assert_value_golden("value-entity-1", 7, Value::Entity(entity));
    }
    let timeline = TimelineId::from_str("00000000-0000-7000-8000-000000000002");
    let unit = Symbol::new(String::from("hour_1"));
    assert!(timeline.is_ok());
    assert!(unit.is_ok());
    if let (Ok(timeline), Ok(unit)) = (timeline, unit) {
        assert_value_golden(
            "value-time-minus-one-hour-1",
            8,
            Value::Time(Time::new(timeline, -1, unit)),
        );
    }
    assert_value_golden(
        "value-duration-minus-one-ns",
        9,
        Value::Duration(Duration::from_nanoseconds(-1)),
    );
    assert_value_golden(
        "value-bytes-00ff",
        10,
        Value::Bytes(Bytes::new(vec![0, 255])),
    );
}

#[test]
fn independent_oracle_rejects_malleable_tlv_and_value_tags() {
    assert!(decode_tlv_oracle(&[0x81, 0x00, 0x00]).is_err());
    assert!(decode_tlv_oracle(&[1, 0, 1, 0]).is_err());
    assert!(
        worlddb_core::TlvDecoder::new(&[0x81, 0x00, 0x00])
            .next_field()
            .is_err()
    );
    let mut duplicate = worlddb_core::TlvDecoder::new(&[1, 0, 1, 0]);
    assert!(duplicate.next_field().is_ok());
    assert!(duplicate.next_field().is_err());
    assert!(decode_value_tag_oracle(&[0x8b, 0x00]).is_err());
    assert!(decode_value(&[0x8b, 0x00]).is_err());
}
