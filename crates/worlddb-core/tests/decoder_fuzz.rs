//! Long-running deterministic M1 decoder campaign.
//!
//! Run with `WORLDDB_DECODER_FUZZ_SECONDS=3600` and an explicit
//! `WORLDDB_DECODER_FUZZ_SEED`, then inspect the JSON report under
//! `target/fuzz-results/`. This test is ignored by ordinary fast test runs.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use worlddb_core::{
    DecoderLimits, TlvDecoder, decode_audit_record_with_limits, decode_frame_with_limits,
    decode_raw_read_attempt_with_limits, decode_record_batch_with_limits, decode_record_ref,
    decode_record_with_limits, decode_value_with_limits,
};

const INVENTORY: &str = include_str!("../../../policy/decoder-inventory.tsv");
const SEEDS: &str = include_str!("../../../policy/decoder-seeds.tsv");
const VALUE_GOLDENS: &str = include_str!("data/wire-v1.0-golden.tsv");
const RECORD_GOLDENS: &str = include_str!("data/record-v1.0-golden.tsv");
const RECORD_REF_GOLDENS: &str = include_str!("data/record-ref-v1.0-golden.tsv");
const AUDIT_GOLDENS: &str = include_str!("data/audit-v1.0-golden.tsv");
const TEXT_GOLDENS: &str = include_str!("data/text-parser-v1.0-golden.tsv");
const CORE_BYTE_GOLDENS: &str = include_str!("data/core-bytes-v1.0-golden.tsv");
const MIGRATION_JOURNAL_SEED: &str =
    include_str!("../../../policy/fuzz-seeds/core/migration-run-journal.hex");
const RECORD_KIND_REGISTRY: &str = include_str!("../../../policy/record-wire-kinds.tsv");
const RECORD_REF_REGISTRY: &str = include_str!("../../../policy/record-ref-wire-tags.tsv");
const AUDIT_KIND_REGISTRY: &str = include_str!("../../../policy/audit-wire-kinds.tsv");

#[derive(Clone, Copy)]
struct Target<'a> {
    decoder_id: &'a str,
    family: &'a str,
    seed_id: &'a str,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

fn decode_hex(input: &str) -> Result<Vec<u8>, String> {
    if input.len() % 2 != 0 {
        return Err("hex seed has an odd number of digits".to_owned());
    }
    let mut bytes = Vec::new();
    for pair in input.as_bytes().chunks_exact(2) {
        let high = pair
            .first()
            .copied()
            .and_then(hex_nibble)
            .ok_or_else(|| "invalid hex seed".to_owned())?;
        let low = pair
            .get(1)
            .copied()
            .and_then(hex_nibble)
            .ok_or_else(|| "invalid hex seed".to_owned())?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn corpus_hex(corpus: &str, name: &str) -> Result<Vec<u8>, String> {
    for line in corpus.lines().skip(1) {
        let mut fields = line.split('\t');
        if fields.next() != Some(name) {
            continue;
        }
        let hex = if corpus == VALUE_GOLDENS {
            fields.next()
        } else {
            fields.last()
        };
        let Some(hex) = hex else {
            return Err(format!("golden row {name} has no hex column"));
        };
        return decode_hex(hex);
    }
    Err(format!("golden seed {name} is missing"))
}

fn corpus_text(name: &str) -> Result<Vec<u8>, String> {
    for line in TEXT_GOLDENS.lines().skip(1) {
        let mut columns = line.split('\t');
        if columns.next() != Some(name) {
            continue;
        }
        return columns
            .next()
            .map(str::as_bytes)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| format!("text seed {name} has no input column"));
    }
    Err(format!("text seed {name} is missing"))
}

fn corpus_core_bytes(name: &str) -> Result<Vec<u8>, String> {
    for line in CORE_BYTE_GOLDENS.lines().skip(1) {
        let mut columns = line.split('\t');
        if columns.next() != Some(name) {
            continue;
        }
        let hex = columns
            .next()
            .ok_or_else(|| format!("byte seed {name} has no hex column"))?;
        return decode_hex(hex);
    }
    Err(format!("byte seed {name} is missing"))
}

fn migration_journal_seed() -> Result<Vec<u8>, String> {
    use worlddb_core::{
        DomainId, MigrationId, MigrationPlanFingerprint, MigrationRunId,
        MigrationRunJournalSnapshot, MigrationRunJournalSpec, MigrationRunJournalStepSpec,
        MigrationStepId, MigrationTransformerVersion, OperationId, Revision, SchemaRevision,
    };

    fn id<T: DomainId>(last: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = last;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    let spec = MigrationRunJournalSpec::new(
        id::<MigrationId>(1)?,
        id::<MigrationRunId>(2)?,
        MigrationPlanFingerprint::from_bytes([3; 32]),
        SchemaRevision::from_published_revision(Revision::GENESIS),
        [4; 32],
        MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
        vec![MigrationRunJournalStepSpec::new(
            id::<MigrationStepId>(5)?,
            id::<OperationId>(7)?,
            [8; 32],
            Revision::FIRST_COMMIT,
        )],
    )
    .map_err(|error| error.to_string())?;
    let snapshot = MigrationRunJournalSnapshot::start(spec).map_err(|error| error.to_string())?;
    snapshot.encode().map_err(|error| error.to_string())
}

fn load_seed(target: Target<'_>) -> Result<Vec<u8>, String> {
    let (corpus, name) = target
        .seed_id
        .split_once('/')
        .ok_or_else(|| format!("invalid seed ID {}", target.seed_id))?;
    let bytes = match corpus {
        "value" | "frame" => corpus_hex(VALUE_GOLDENS, name)?,
        "record" => corpus_hex(RECORD_GOLDENS, name)?,
        "record_ref" => corpus_hex(RECORD_REF_GOLDENS, name)?,
        "audit" => corpus_hex(AUDIT_GOLDENS, name)?,
        "text" => corpus_text(name)?,
        "core_bytes" => corpus_core_bytes(name)?,
        "generated" if name == "migration_run_journal" => migration_journal_seed()?,
        _ => return Err(format!("unknown seed corpus {corpus}")),
    };
    if target.family != "tlv" {
        return Ok(bytes);
    }
    let frame = decode_frame_with_limits(&bytes, &DecoderLimits::DEFAULT)
        .map_err(|error| error.to_string())?;
    Ok(frame.payload().to_vec())
}

fn inventory() -> Result<Vec<Target<'static>>, String> {
    let mut targets = Vec::new();
    let mut typescript_targets = Vec::new();
    for (line_number, line) in INVENTORY.lines().enumerate().skip(1) {
        let mut columns = line.split('\t');
        let (Some(decoder_id), Some(family), Some(seed_id)) =
            (columns.next(), columns.next(), columns.next())
        else {
            return Err(format!("invalid decoder inventory row {}", line_number + 1));
        };
        if decoder_id.is_empty()
            || family.is_empty()
            || seed_id.is_empty()
            || columns.next().is_some()
        {
            return Err(format!("invalid decoder inventory row {}", line_number + 1));
        }
        let target = Target {
            decoder_id,
            family,
            seed_id,
        };
        if family == "typescript" {
            typescript_targets.push(target);
        } else {
            targets.push(target);
        }
    }
    if targets.len() != 91 {
        return Err(format!(
            "expected 91 decoder targets, found {}",
            targets.len()
        ));
    }
    if typescript_targets.len() != 1
        || typescript_targets.first().is_none_or(|target| {
            target.decoder_id != "typescript.json_envelope"
                || target.seed_id != "typescript/transport-v1.0-golden"
        })
    {
        return Err(
            "expected the TypeScript JSON envelope in the shared decoder inventory".to_owned(),
        );
    }
    Ok(targets)
}

fn select_targets(
    targets: Vec<Target<'static>>,
    selection: Option<&str>,
) -> Result<Vec<Target<'static>>, String> {
    let Some(selection) = selection else {
        return Ok(targets);
    };
    let selected = targets
        .into_iter()
        .filter(|target| target.decoder_id == selection)
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err(format!("unknown or duplicate decoder target {selection}"));
    }
    Ok(selected)
}

fn selected_target_id() -> Result<Option<String>, String> {
    match std::env::var("WORLDDB_DECODER_FUZZ_TARGET") {
        Ok(value) if value.is_empty() => {
            Err("WORLDDB_DECODER_FUZZ_TARGET must not be empty".to_owned())
        }
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err("WORLDDB_DECODER_FUZZ_TARGET is not Unicode".to_owned())
        }
    }
}

fn mutator_ids() -> Result<Vec<&'static str>, String> {
    let mut ids = Vec::new();
    for (line_number, line) in SEEDS.lines().enumerate().skip(1) {
        let mut columns = line.split('\t');
        let Some(id) = columns.next() else {
            return Err(format!("invalid decoder seed row {}", line_number + 1));
        };
        if id.is_empty() || columns.any(str::is_empty) || line.split('\t').count() != 4 {
            return Err(format!("invalid decoder seed row {}", line_number + 1));
        }
        ids.push(id);
    }
    if ids.len() != 9 {
        return Err(format!("expected 9 seed strategies, found {}", ids.len()));
    }
    for required in [
        "canonical",
        "empty_input",
        "truncate_last",
        "flip_one_bit",
        "append_junk",
        "zero_one_byte",
        "nonminimal_prefix",
        "oversized_declared_length",
        "tight_resource_limits",
    ] {
        if !ids.contains(&required) {
            return Err(format!("decoder seed manifest is missing {required}"));
        }
    }
    Ok(ids)
}

fn snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn encode_varint(mut value: u128) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return bytes;
        }
    }
}

fn mutate(seed: &[u8], target: Target<'_>, mutator: &str, rng: &mut Rng) -> Vec<u8> {
    match mutator {
        "canonical" => seed.to_vec(),
        "empty_input" => Vec::new(),
        "truncate_last" => seed
            .get(..seed.len().saturating_sub(1))
            .unwrap_or_default()
            .to_vec(),
        "flip_one_bit" => {
            let mut bytes = seed.to_vec();
            if !bytes.is_empty() {
                let index = (rng.next() as usize) % bytes.len();
                if let Some(byte) = bytes.get_mut(index) {
                    *byte ^= 1_u8 << (rng.next() as u32 % 8);
                }
            }
            bytes
        }
        "append_junk" => {
            let mut bytes = seed.to_vec();
            bytes.push(rng.next() as u8);
            bytes
        }
        "zero_one_byte" => {
            let mut bytes = seed.to_vec();
            if !bytes.is_empty() {
                let index = (rng.next() as usize) % bytes.len();
                if let Some(byte) = bytes.get_mut(index) {
                    *byte = 0;
                }
            }
            bytes
        }
        "nonminimal_prefix" if matches!(target.family, "value" | "tlv" | "record_ref") => {
            vec![0x80, 0x00]
        }
        "nonminimal_prefix" => {
            let mut bytes = seed.to_vec();
            if let Some(first) = bytes.first_mut() {
                *first ^= 0x80;
            }
            bytes
        }
        "oversized_declared_length" => match target.family {
            "value" => {
                let mut bytes = vec![5];
                bytes.extend(encode_varint(u128::MAX));
                bytes
            }
            "tlv" => {
                let mut bytes = vec![1];
                bytes.extend(encode_varint(u128::MAX));
                bytes
            }
            "record_ref" => {
                let mut bytes = seed.to_vec();
                bytes.push(0);
                bytes
            }
            "frame" | "record" | "audit_record" | "raw_read_attempt" => {
                let mut bytes = seed.to_vec();
                if let Some(length) = bytes.get_mut(32..40) {
                    length.fill(0xff);
                }
                bytes
            }
            _ => seed.to_vec(),
        },
        _ => seed.to_vec(),
    }
}

fn limits_for(mutator: &str, input_len: usize) -> DecoderLimits {
    if mutator == "tight_resource_limits" {
        return DecoderLimits {
            max_frame_bytes: 0,
            max_string_or_bytes: 0,
            max_fields_per_record: 0,
            max_array_items: 0,
            max_collection_bytes: 0,
            max_batch_bytes: 0,
            max_records_per_batch: 0,
            max_nesting_depth: 0,
        };
    }
    let mut limits = DecoderLimits::DEFAULT;
    limits.max_frame_bytes = 1024 * 1024;
    limits.max_string_or_bytes = 64 * 1024;
    limits.max_fields_per_record = 64;
    limits.max_array_items = 4096;
    limits.max_collection_bytes = 256 * 1024;
    limits.max_batch_bytes = 2 * 1024 * 1024;
    limits.max_records_per_batch = 4096;
    if mutator == "oversized_declared_length" {
        limits.max_string_or_bytes = 1024;
    }
    if mutator == "append_junk" && input_len > 0 {
        limits.max_frame_bytes = input_len.saturating_sub(1);
    }
    limits
}

fn run_decoder(target: Target<'_>, bytes: &[u8], limits: &DecoderLimits) -> bool {
    match target.family {
        "frame" => decode_frame_with_limits(bytes, limits).is_ok(),
        "value" => decode_value_with_limits(bytes, limits).is_ok(),
        "tlv" => {
            let mut decoder = TlvDecoder::with_limits(bytes, *limits);
            loop {
                match decoder.next_field() {
                    Ok(Some(_)) => {}
                    Ok(None) => return true,
                    Err(_) => return false,
                }
            }
        }
        "record" => decode_record_with_limits(bytes, limits).is_ok(),
        "record_ref" => decode_record_ref(bytes).is_ok(),
        "batch" => decode_record_batch_with_limits(&[bytes], limits).is_ok(),
        "audit_record" => decode_audit_record_with_limits(bytes, limits).is_ok(),
        "raw_read_attempt" => decode_raw_read_attempt_with_limits(bytes, limits).is_ok(),
        "int_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::Int::from_str(value).is_ok()),
        "uint_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::UInt::from_str(value).is_ok()),
        "decimal_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::Decimal::from_str(value).is_ok()),
        "symbol_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::Symbol::from_str(value).is_ok()),
        "id_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::EntityId::from_str(value).is_ok()),
        "revision_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::Revision::from_str(value).is_ok()),
        "schema_revision_text" => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| worlddb_core::SchemaRevision::from_str(value).is_ok()),
        "int_bytes" => worlddb_core::Int::from_canonical_bytes(bytes).is_ok(),
        "uint_bytes" => worlddb_core::UInt::from_canonical_bytes(bytes).is_ok(),
        "decimal_bytes" => worlddb_core::Decimal::from_canonical_bytes(bytes).is_ok(),
        "cursor_token" => worlddb_core::CursorToken::decode(bytes).is_ok(),
        "migration_run_journal" => worlddb_core::MigrationRunJournalSnapshot::decode(bytes).is_ok(),
        _ => false,
    }
}

fn parse_duration() -> Result<Duration, String> {
    let value = match std::env::var("WORLDDB_DECODER_FUZZ_SECONDS") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "3600".to_owned(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("WORLDDB_DECODER_FUZZ_SECONDS is not Unicode".to_owned());
        }
    };
    let seconds = value
        .parse::<u64>()
        .map_err(|error| format!("invalid fuzz duration: {error}"))?;
    Ok(Duration::from_secs(seconds.max(1)))
}

fn parse_seed() -> Result<u64, String> {
    let value = match std::env::var("WORLDDB_DECODER_FUZZ_SEED") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "0x574f524c44444231".to_owned(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("WORLDDB_DECODER_FUZZ_SEED is not Unicode".to_owned());
        }
    };
    let (digits, radix) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or((value.as_str(), 10), |digits| (digits, 16));
    u64::from_str_radix(digits, radix).map_err(|error| format!("invalid fuzz seed: {error}"))
}

fn parse_input_timeout() -> Result<Duration, String> {
    let value = match std::env::var("WORLDDB_DECODER_FUZZ_INPUT_TIMEOUT_SECONDS") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "5".to_owned(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("WORLDDB_DECODER_FUZZ_INPUT_TIMEOUT_SECONDS is not Unicode".to_owned());
        }
    };
    let seconds = value
        .parse::<u64>()
        .map_err(|error| format!("invalid per-input timeout: {error}"))?;
    if seconds == 0 {
        return Err("per-input timeout must be positive".to_owned());
    }
    Ok(Duration::from_secs(seconds))
}

fn parse_max_input_bytes() -> Result<usize, String> {
    let value = match std::env::var("WORLDDB_DECODER_FUZZ_MAX_INPUT_BYTES") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "268435456".to_owned(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("WORLDDB_DECODER_FUZZ_MAX_INPUT_BYTES is not Unicode".to_owned());
        }
    };
    let bytes = value.parse::<usize>().map_err(|_| {
        "WORLDDB_DECODER_FUZZ_MAX_INPUT_BYTES must be a positive integer".to_owned()
    })?;
    if bytes == 0 {
        return Err("WORLDDB_DECODER_FUZZ_MAX_INPUT_BYTES must be positive".to_owned());
    }
    Ok(bytes)
}

fn run_decoder_timed(
    target: Target<'static>,
    bytes: Vec<u8>,
    limits: DecoderLimits,
    timeout: Duration,
) -> Result<bool, String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(|| run_decoder(target, &bytes, &limits)));
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(accepted)) => Ok(accepted),
        Ok(Err(_)) => Err("decoder panicked".to_owned()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "decoder exceeded {}s per-input timeout",
            timeout.as_secs()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("decoder worker exited without a result".to_owned())
        }
    }
}

fn parse_source_revision() -> Result<String, String> {
    let revision = std::env::var("WORLDDB_SOURCE_REVISION")
        .map_err(|_| "WORLDDB_SOURCE_REVISION must name the local source commit".to_owned())?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            "WORLDDB_SOURCE_REVISION must be a full lowercase 40-character Git hash".to_owned(),
        );
    }
    Ok(revision)
}

fn json_report(
    seed: u64,
    source_revision: &str,
    started_unix: u64,
    elapsed: Duration,
    rounds: u64,
    counts: &[(String, u64)],
    crashes: &[String],
) -> String {
    let mut report = String::new();
    let _ = writeln!(report, "{{");
    let _ = writeln!(
        report,
        "  \"run_id\": \"M1-18-{seed:016x}-{started_unix}\","
    );
    let _ = writeln!(report, "  \"seed\": \"0x{seed:016x}\",");
    let _ = writeln!(report, "  \"started_unix_seconds\": {started_unix},");
    let _ = writeln!(
        report,
        "  \"elapsed_seconds\": {:.3},",
        elapsed.as_secs_f64()
    );
    let _ = writeln!(report, "  \"rounds\": {rounds},");
    let _ = writeln!(report, "  \"source_revision\": \"{source_revision}\",");
    let _ = writeln!(report, "  \"decoder_targets\": {},", counts.len());
    let _ = writeln!(report, "  \"calls_by_decoder\": {{");
    for (index, (decoder_id, calls)) in counts.iter().enumerate() {
        let comma = if index + 1 == counts.len() { "" } else { "," };
        let _ = writeln!(report, "    \"{decoder_id}\": {calls}{comma}");
    }
    let _ = writeln!(report, "  }},");
    let _ = writeln!(report, "  \"crashes\": [");
    for (index, crash) in crashes.iter().enumerate() {
        let comma = if index + 1 == crashes.len() { "" } else { "," };
        let _ = writeln!(report, "    \"{crash}\"{comma}");
    }
    let _ = writeln!(report, "  ],");
    let _ = writeln!(
        report,
        "  \"result\": \"{}\"",
        if crashes.is_empty() {
            "PASS_LOCAL"
        } else {
            "FAIL"
        }
    );
    let _ = writeln!(report, "}}");
    report
}

fn workspace_path(path: PathBuf) -> PathBuf {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    }
}

#[test]
#[ignore = "one-hour CPU fuzz campaign; invoke explicitly with a recorded seed"]
fn decoder_budget_fuzz_campaign() -> Result<(), String> {
    let targets = select_targets(inventory()?, selected_target_id()?.as_deref())?;
    let mutators = mutator_ids()?;
    let duration = parse_duration()?;
    let seed = parse_seed()?;
    let input_timeout = parse_input_timeout()?;
    let max_input_bytes = parse_max_input_bytes()?;
    let source_revision = parse_source_revision()?;
    let started_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let started = Instant::now();
    let mut rng = Rng(seed);
    let mut counts = vec![0_u64; targets.len()];
    let mut crashes = Vec::new();
    let mut rounds = 0_u64;

    loop {
        for (index, target) in targets.iter().copied().enumerate() {
            let golden = load_seed(target)?;
            if golden.len() > max_input_bytes {
                return Err(format!(
                    "golden seed for {} is {} bytes, exceeding max input {}",
                    target.decoder_id,
                    golden.len(),
                    max_input_bytes
                ));
            }
            for mutator in &mutators {
                let input = mutate(&golden, target, mutator, &mut rng);
                if input.len() > max_input_bytes {
                    crashes.push(format!(
                        "{}:{mutator}:input_exceeds_max:{}:{}",
                        target.decoder_id,
                        input.len(),
                        max_input_bytes
                    ));
                    break;
                }
                let limits = limits_for(mutator, input.len());
                let result = run_decoder_timed(target, input.clone(), limits, input_timeout);
                let count = counts
                    .get_mut(index)
                    .ok_or_else(|| format!("decoder target index {index} is out of range"))?;
                *count = (*count).saturating_add(1);
                if let Err(error) = result {
                    let mut input_hex = String::new();
                    for byte in input.iter().take(256) {
                        let _ = write!(input_hex, "{byte:02x}");
                    }
                    crashes.push(format!(
                        "{}:{mutator}:{error}:{input_hex}",
                        target.decoder_id
                    ));
                    break;
                }
            }
            if !crashes.is_empty() {
                break;
            }
        }
        rounds = rounds.saturating_add(1);
        if started.elapsed() >= duration || !crashes.is_empty() {
            break;
        }
    }

    if counts.iter().any(|count| *count == 0) {
        return Err(
            "fuzz campaign ended before every inventoried decoder was exercised".to_owned(),
        );
    }
    let per_decoder = targets
        .iter()
        .zip(counts.iter().copied())
        .map(|(target, calls)| (target.decoder_id.to_owned(), calls))
        .collect::<Vec<_>>();
    let report = json_report(
        seed,
        &source_revision,
        started_unix,
        started.elapsed(),
        rounds,
        &per_decoder,
        &crashes,
    );
    let report_path = match std::env::var("WORLDDB_DECODER_FUZZ_REPORT") {
        Ok(path) => workspace_path(PathBuf::from(path)),
        Err(_) => workspace_path(PathBuf::from(format!(
            "target/fuzz-results/m1-18-{seed:016x}.json"
        ))),
    };
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&report_path, report).map_err(|error| error.to_string())?;
    eprintln!(
        "M1-18 fuzz: seed=0x{seed:016x}, rounds={rounds}, elapsed={:.1}s, targets={}, crashes={}, report={}",
        started.elapsed().as_secs_f64(),
        targets.len(),
        crashes.len(),
        report_path.display()
    );
    if crashes.is_empty() {
        Ok(())
    } else {
        Err(format!("{} decoder panic(s) recorded", crashes.len()))
    }
}

#[test]
fn decoder_inventory_and_seed_manifest_cover_every_registered_codec() -> Result<(), String> {
    let targets = inventory()?;
    let mutators = mutator_ids()?;
    let ids = targets
        .iter()
        .map(|target| target.decoder_id)
        .collect::<std::collections::BTreeSet<_>>();
    if ids.len() != targets.len() {
        return Err("decoder inventory contains duplicate decoder IDs".to_owned());
    }
    let mut family_counts = BTreeMap::<&str, usize>::new();
    let mut rng = Rng(0x574f_524c_4444_4231);
    let input_timeout = Duration::from_secs(5);
    let generated_seed = migration_journal_seed()?;
    let fixture_seed = decode_hex(MIGRATION_JOURNAL_SEED.trim())?;
    if generated_seed != fixture_seed {
        return Err(
            "generated migration journal seed differs from its archived fixture".to_owned(),
        );
    }
    for target in &targets {
        *family_counts.entry(target.family).or_default() += 1;
        let seed = load_seed(*target)?;
        let limits = DecoderLimits::DEFAULT;
        let canonical = run_decoder_timed(*target, seed.clone(), limits, input_timeout);
        if !matches!(canonical, Ok(true)) {
            return Err(format!(
                "canonical golden was rejected or panicked in {}",
                target.decoder_id
            ));
        }
        for mutator in mutators.iter().copied().filter(|id| *id != "canonical") {
            let input = mutate(&seed, *target, mutator, &mut rng);
            let limits = limits_for(mutator, input.len());
            let result = run_decoder_timed(*target, input, limits, input_timeout);
            if let Err(error) = result {
                return Err(format!(
                    "seed {mutator} failed in {}: {error}",
                    target.decoder_id
                ));
            }
        }
    }
    for line in RECORD_KIND_REGISTRY.lines().skip(1) {
        let columns = line.split('\t').collect::<Vec<_>>();
        let record_name = columns
            .get(1)
            .ok_or_else(|| "record registry row has no name".to_owned())?;
        let expected = format!("record.{}", snake_case(record_name));
        if !ids.contains(expected.as_str()) {
            return Err(format!(
                "record decoder {record_name} is missing from inventory"
            ));
        }
    }
    for line in RECORD_REF_REGISTRY.lines().skip(1) {
        let columns = line.split('\t').collect::<Vec<_>>();
        let variant = columns
            .get(1)
            .ok_or_else(|| "RecordRef registry row has no variant".to_owned())?;
        let expected = format!("record_ref.{}", snake_case(variant));
        if !ids.contains(expected.as_str()) {
            return Err(format!(
                "RecordRef decoder {variant} is missing from inventory"
            ));
        }
    }
    for line in VALUE_GOLDENS.lines().skip(1) {
        let mut columns = line.split('\t');
        let Some(vector_id) = columns.next() else {
            continue;
        };
        let Some(suffix) = vector_id.strip_prefix("value-") else {
            continue;
        };
        let expected = format!("value.{suffix}");
        if !ids.contains(expected.as_str()) {
            return Err(format!("value decoder {suffix} is missing from inventory"));
        }
    }
    for line in AUDIT_KIND_REGISTRY.lines().skip(1) {
        let columns = line.split('\t').collect::<Vec<_>>();
        let kind = columns
            .get(1)
            .ok_or_else(|| "audit registry row has no kind".to_owned())?;
        let expected = match *kind {
            "AuditRecord" => "audit.audit_record",
            "RawReadAttempt" => "audit.raw_read_attempt",
            _ => return Err(format!("unmapped audit kind {kind}")),
        };
        if !ids.contains(expected) {
            return Err(format!(
                "audit decoder {expected} is missing from inventory"
            ));
        }
    }
    for family in [
        "record",
        "record_ref",
        "value",
        "frame",
        "tlv",
        "batch",
        "audit_record",
        "raw_read_attempt",
        "int_text",
        "uint_text",
        "decimal_text",
        "symbol_text",
        "id_text",
        "revision_text",
        "schema_revision_text",
        "int_bytes",
        "uint_bytes",
        "decimal_bytes",
        "cursor_token",
        "migration_run_journal",
    ] {
        if !family_counts.contains_key(family) {
            return Err(format!("decoder inventory is missing family {family}"));
        }
    }
    Ok(())
}

#[test]
fn decoder_campaign_target_selector_is_exact_and_fail_closed() -> Result<(), String> {
    let targets = inventory()?;
    let selected = select_targets(targets.clone(), Some("core.int_decimal"))?;
    if selected.len() != 1
        || selected
            .first()
            .is_none_or(|target| target.decoder_id != "core.int_decimal")
    {
        return Err("decoder selector did not isolate the requested target".to_owned());
    }
    if select_targets(targets, Some("core.missing_target")).is_ok() {
        return Err("decoder selector accepted an unregistered target".to_owned());
    }
    Ok(())
}
