//! Deterministic scalar precision campaign used by the M1 gate.
//!
//! Run explicitly with a recorded `WORLDDB_SOURCE_REVISION` and seed. The
//! ignored test executes at least ten million signed integer, unsigned
//! integer, and decimal roundtrips and writes a JSON result to `docs/`.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use worlddb_core::{Decimal, Int, UInt};

const MINIMUM_ROUNDS: u64 = 10_000_000;

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn next_u128(&mut self) -> u128 {
        (u128::from(self.next_u64()) << 64) | u128::from(self.next_u64())
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

fn parse_seed() -> Result<u64, String> {
    let value = std::env::var("WORLDDB_M120_PRECISION_SEED")
        .unwrap_or_else(|_| "0x574f524c44444232".to_owned());
    let (digits, radix) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or((value.as_str(), 10), |digits| (digits, 16));
    u64::from_str_radix(digits, radix).map_err(|error| format!("invalid precision seed: {error}"))
}

fn parse_rounds() -> Result<u64, String> {
    let value = std::env::var("WORLDDB_M120_PRECISION_ROUNDS")
        .unwrap_or_else(|_| MINIMUM_ROUNDS.to_string());
    let rounds = value
        .parse::<u64>()
        .map_err(|error| format!("invalid precision round count: {error}"))?;
    if rounds < MINIMUM_ROUNDS {
        return Err(format!(
            "precision campaign requires at least {MINIMUM_ROUNDS} rounds"
        ));
    }
    Ok(rounds)
}

fn report_path() -> PathBuf {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("WORLDDB_M120_PRECISION_REPORT").map_or_else(
        || workspace_root.join("docs/M1-20-precision.json"),
        |path| {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        },
    )
}

fn json_report(
    seed: u64,
    source_revision: &str,
    started_unix: u64,
    elapsed: Duration,
    rounds: u64,
    counts: [u64; 4],
    failure: Option<(&str, u64)>,
) -> String {
    let mut report = String::new();
    let _ = writeln!(report, "{{");
    let _ = writeln!(
        report,
        "  \"run_id\": \"M1-20-precision-{seed:016x}-{started_unix}\","
    );
    let _ = writeln!(report, "  \"seed\": \"0x{seed:016x}\",");
    let _ = writeln!(report, "  \"source_revision\": \"{source_revision}\",");
    let _ = writeln!(report, "  \"started_unix_seconds\": {started_unix},");
    let _ = writeln!(
        report,
        "  \"elapsed_seconds\": {:.3},",
        elapsed.as_secs_f64()
    );
    let _ = writeln!(report, "  \"rounds_requested\": {rounds},");
    let _ = writeln!(report, "  \"signed_integer_roundtrips\": {},", counts[0]);
    let _ = writeln!(report, "  \"unsigned_integer_roundtrips\": {},", counts[1]);
    let _ = writeln!(report, "  \"decimal_bytes_roundtrips\": {},", counts[2]);
    let _ = writeln!(report, "  \"decimal_text_roundtrips\": {},", counts[3]);
    match failure {
        Some((kind, iteration)) => {
            let _ = writeln!(report, "  \"failure_kind\": \"{kind}\",");
            let _ = writeln!(report, "  \"failure_iteration\": {iteration},");
            let _ = writeln!(report, "  \"result\": \"FAIL\"");
        }
        None => {
            let _ = writeln!(report, "  \"failure_kind\": null,");
            let _ = writeln!(report, "  \"failure_iteration\": null,");
            let _ = writeln!(report, "  \"result\": \"PASS_LOCAL\"");
        }
    }
    let _ = writeln!(report, "}}");
    report
}

#[test]
#[ignore = "ten-million-case deterministic scalar precision campaign; invoke explicitly with a recorded commit and seed"]
fn seeded_integer_and_decimal_precision_campaign() -> Result<(), String> {
    let source_revision = parse_source_revision()?;
    let seed = parse_seed()?;
    let rounds = parse_rounds()?;
    let started_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let started = Instant::now();
    let mut rng = Rng(seed);
    let mut counts = [0_u64; 4];
    let mut failure = None;

    for iteration in 0..rounds {
        let raw = rng.next_u128();
        let signed = Int::new(raw as i128);
        if Int::from_canonical_bytes(&signed.to_canonical_bytes()) != Ok(signed) {
            failure = Some(("signed_integer", iteration));
            break;
        }
        counts[0] = counts[0].saturating_add(1);

        let unsigned = UInt::new(raw);
        if UInt::from_canonical_bytes(&unsigned.to_canonical_bytes()) != Ok(unsigned) {
            failure = Some(("unsigned_integer", iteration));
            break;
        }
        counts[1] = counts[1].saturating_add(1);

        let negative = rng.next_u64() & 1 != 0;
        let coefficient = rng.next_u128();
        let scale = (rng.next_u64() % 257) as i32 - 128;
        let decimal = Decimal::new(negative, coefficient, scale).map_err(|error| {
            format!("generated decimal was rejected at iteration {iteration}: {error}")
        })?;
        if Decimal::from_canonical_bytes(&decimal.to_canonical_bytes()) != Ok(decimal) {
            failure = Some(("decimal_bytes", iteration));
            break;
        }
        counts[2] = counts[2].saturating_add(1);

        let text = decimal.to_canonical_string(256).map_err(|error| {
            format!("generated decimal formatting failed at iteration {iteration}: {error}")
        })?;
        if Decimal::from_canonical_string(&text) != Ok(decimal) {
            failure = Some(("decimal_text", iteration));
            break;
        }
        counts[3] = counts[3].saturating_add(1);
    }

    let report = json_report(
        seed,
        &source_revision,
        started_unix,
        started.elapsed(),
        rounds,
        counts,
        failure,
    );
    let path = report_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, report).map_err(|error| error.to_string())?;

    if let Some((kind, iteration)) = failure {
        Err(format!(
            "{kind} roundtrip failed at iteration {iteration}; report={}",
            path.display()
        ))
    } else {
        eprintln!(
            "M1-20 precision: seed=0x{seed:016x}, rounds={rounds}, source={source_revision}, elapsed={:.1}s, report={}",
            started.elapsed().as_secs_f64(),
            path.display()
        );
        Ok(())
    }
}
