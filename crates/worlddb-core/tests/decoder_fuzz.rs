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
    if targets.len() != 79 {
        return Err(format!(
            "expected 79 decoder targets, found {}",
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
    let targets = inventory()?;
    let mutators = mutator_ids()?;
    let duration = parse_duration()?;
    let seed = parse_seed()?;
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
            for mutator in &mutators {
                let input = mutate(&golden, target, mutator, &mut rng);
                let limits = limits_for(mutator, input.len());
                let result =
                    catch_unwind(AssertUnwindSafe(|| run_decoder(target, &input, &limits)));
                let count = counts
                    .get_mut(index)
                    .ok_or_else(|| format!("decoder target index {index} is out of range"))?;
                *count = (*count).saturating_add(1);
                if result.is_err() {
                    let mut input_hex = String::new();
                    for byte in input.iter().take(256) {
                        let _ = write!(input_hex, "{byte:02x}");
                    }
                    crashes.push(format!("{}:{mutator}:{input_hex}", target.decoder_id));
                    break;
                }
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
    for target in &targets {
        *family_counts.entry(target.family).or_default() += 1;
        let seed = load_seed(*target)?;
        let limits = DecoderLimits::DEFAULT;
        let canonical = catch_unwind(AssertUnwindSafe(|| run_decoder(*target, &seed, &limits)));
        if !matches!(canonical, Ok(true)) {
            return Err(format!(
                "canonical golden was rejected or panicked in {}",
                target.decoder_id
            ));
        }
        for mutator in mutators.iter().copied().filter(|id| *id != "canonical") {
            let input = mutate(&seed, *target, mutator, &mut rng);
            let limits = limits_for(mutator, input.len());
            let result = catch_unwind(AssertUnwindSafe(|| run_decoder(*target, &input, &limits)));
            if result.is_err() {
                return Err(format!("seed {mutator} panicked in {}", target.decoder_id));
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
    ] {
        if !family_counts.contains_key(family) {
            return Err(format!("decoder inventory is missing family {family}"));
        }
    }
    Ok(())
}
