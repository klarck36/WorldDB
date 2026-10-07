//! Shared bounded campaign runner for Rust parser targets outside worlddb-core.

use std::fmt::Write as _;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

fn env_string(name: &str, default: Option<&str>) -> Result<String, String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        Ok(_) => Err(format!("{name} must not be empty")),
        Err(std::env::VarError::NotPresent) => default
            .map(str::to_owned)
            .ok_or_else(|| format!("{name} is required")),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is not Unicode")),
    }
}

fn parse_u64(name: &str, default: Option<&str>) -> Result<u64, String> {
    let value = env_string(name, default)?;
    let (digits, radix) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or((value.as_str(), 10), |digits| (digits, 16));
    u64::from_str_radix(digits, radix).map_err(|error| format!("invalid {name}: {error}"))
}

fn parse_usize(name: &str, default: &str) -> Result<usize, String> {
    let value = env_string(name, Some(default))?;
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("invalid {name}: {error}"))?;
    if parsed == 0 {
        return Err(format!("{name} must be positive"));
    }
    Ok(parsed)
}

fn workspace_root() -> Result<PathBuf, String> {
    let mut directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if directory.join("policy/fuzz-targets.tsv").is_file() {
            return Ok(directory);
        }
        if !directory.pop() {
            return Err("could not locate the WorldDB workspace root".to_owned());
        }
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_hex(bytes: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    let digits = bytes
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if digits.len() % 2 != 0 {
        return Err(format!(
            "hex seed has an odd number of digits: {}",
            path.display()
        ));
    }
    let mut decoded = Vec::with_capacity(digits.len() / 2);
    for pair in digits.chunks_exact(2) {
        let high = pair.first().copied().and_then(hex_nibble);
        let low = pair.get(1).copied().and_then(hex_nibble);
        let (Some(high), Some(low)) = (high, low) else {
            return Err(format!("invalid hex seed: {}", path.display()));
        };
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "seed corpus contains a symlink: {}",
            path.display()
        ));
    }
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "seed path is neither a file nor directory: {}",
            path.display()
        ));
    }
    let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut children = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    children.sort();
    for child in children {
        collect_files(&child, files)?;
    }
    Ok(())
}

fn load_seeds(root: &Path, max_bytes: usize) -> Result<(Vec<Vec<u8>>, Vec<String>), String> {
    let corpus = env_string("WORLDDB_FUZZ_SEED_CORPUS", None)?;
    let mut files = Vec::new();
    for item in corpus.split(';') {
        if item.is_empty() {
            return Err("WORLDDB_FUZZ_SEED_CORPUS contains an empty path".to_owned());
        }
        let path = Path::new(item);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        collect_files(&resolved, &mut files)?;
    }
    files.sort();
    files.dedup();
    let mut seeds = Vec::new();
    let mut names = Vec::new();
    for path in files {
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let length = usize::try_from(metadata.len())
            .map_err(|_| format!("seed too large: {}", path.display()))?;
        if length > max_bytes {
            return Err(format!(
                "seed exceeds max input size ({max_bytes} bytes): {}",
                path.display()
            ));
        }
        let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let extension = path.extension().and_then(|extension| extension.to_str());
        if extension == Some("hex") {
            seeds.push(decode_hex(&bytes, &path)?);
            names.push(path.to_string_lossy().into_owned());
        } else if extension == Some("txt") {
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| format!("text seed is not UTF-8: {}", path.display()))?;
            for (line_number, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.len() > max_bytes {
                    return Err(format!(
                        "text seed exceeds max input size: {}",
                        path.display()
                    ));
                }
                seeds.push(line.as_bytes().to_vec());
                names.push(format!("{}:{}", path.display(), line_number + 1));
            }
        } else {
            seeds.push(bytes);
            names.push(path.to_string_lossy().into_owned());
        }
    }
    if seeds.is_empty() {
        return Err("seed corpus did not contain any inputs".to_owned());
    }
    Ok((seeds, names))
}

fn mutate(seed: &[u8], choice: u64, rng: &mut Rng, max_bytes: usize) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    match choice % 8 {
        0 => bytes,
        1 => Vec::new(),
        2 => {
            bytes.truncate(bytes.len().saturating_sub(1));
            bytes
        }
        3 => {
            if !bytes.is_empty() {
                let index = (rng.next() as usize) % bytes.len();
                if let Some(byte) = bytes.get_mut(index) {
                    *byte ^= 1_u8 << (rng.next() as u32 % 8);
                }
            }
            bytes
        }
        4 => {
            if bytes.len() < max_bytes {
                bytes.push(rng.next() as u8);
            }
            bytes
        }
        5 => {
            if !bytes.is_empty() {
                let index = (rng.next() as usize) % bytes.len();
                if let Some(byte) = bytes.get_mut(index) {
                    *byte = 0;
                }
            }
            bytes
        }
        6 => {
            if bytes.len() < max_bytes {
                bytes.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
                bytes.truncate(max_bytes);
            }
            bytes
        }
        _ => {
            if bytes.len() > 1 {
                let index = (rng.next() as usize) % bytes.len();
                bytes.drain(index..=index);
            }
            bytes
        }
    }
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

struct CampaignReport<'a> {
    target: &'a str,
    seed: u64,
    source_revision: &'a str,
    elapsed: Duration,
    rounds: u64,
    accepted: u64,
    rejected: u64,
    corpus_inputs: usize,
}

fn report(
    summary: CampaignReport<'_>,
    crashes: &[String],
    crash_corpus_files: &[String],
) -> String {
    let mut json = String::new();
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(summary.elapsed.as_secs());
    let _ = writeln!(json, "{{");
    let _ = writeln!(
        json,
        "  \"target_id\": \"{}\",",
        json_escape(summary.target)
    );
    let _ = writeln!(json, "  \"seed\": \"0x{:016x}\",", summary.seed);
    let _ = writeln!(json, "  \"started_unix_seconds\": {started},");
    let _ = writeln!(
        json,
        "  \"elapsed_seconds\": {:.3},",
        summary.elapsed.as_secs_f64()
    );
    let _ = writeln!(json, "  \"rounds\": {},", summary.rounds);
    let _ = writeln!(json, "  \"inputs_accepted\": {},", summary.accepted);
    let _ = writeln!(json, "  \"inputs_rejected\": {},", summary.rejected);
    let _ = writeln!(json, "  \"corpus_inputs\": {},", summary.corpus_inputs);
    let _ = writeln!(
        json,
        "  \"source_revision\": \"{}\",",
        json_escape(summary.source_revision)
    );
    let _ = writeln!(json, "  \"crashes\": [");
    for (index, crash) in crashes.iter().enumerate() {
        let comma = if index + 1 == crashes.len() { "" } else { "," };
        let _ = writeln!(json, "    \"{}\"{comma}", json_escape(crash));
    }
    let _ = writeln!(json, "  ],");
    let _ = writeln!(json, "  \"crash_corpus_files\": [");
    for (index, path) in crash_corpus_files.iter().enumerate() {
        let comma = if index + 1 == crash_corpus_files.len() {
            ""
        } else {
            ","
        };
        let _ = writeln!(json, "    \"{}\"{comma}", json_escape(path));
    }
    let _ = writeln!(json, "  ]");
    let _ = writeln!(json, "}}");
    json
}

fn write_crash_input(directory: &Path, input: &[u8], index: u64) -> Result<String, String> {
    fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let filename = format!("crash-{index:08}.bin");
    let path = directory.join(&filename);
    fs::write(&path, input).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(filename)
}

fn archive_crash_input(input: &[u8], index: u64) -> Result<Option<String>, String> {
    let directory = match std::env::var("WORLDDB_FUZZ_CRASH_CORPUS") {
        Ok(directory) if !directory.is_empty() => PathBuf::from(directory),
        Ok(_) => return Err("WORLDDB_FUZZ_CRASH_CORPUS must not be empty".to_owned()),
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("WORLDDB_FUZZ_CRASH_CORPUS is not Unicode".to_owned());
        }
    };
    write_crash_input(&directory, input, index).map(Some)
}

fn run_one(
    probe: fn(&str, &[u8]) -> Result<bool, String>,
    target: String,
    input: Vec<u8>,
    timeout: Duration,
) -> Result<bool, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(|| probe(&target, &input)));
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(Ok(accepted))) => Ok(accepted),
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) => Err("parser panicked".to_owned()),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "parser exceeded {}s input timeout",
            timeout.as_secs()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("parser worker exited without a result".to_owned())
        }
    }
}

/// Run one target's corpus and deterministic mutations until the configured wall duration.
pub fn run_campaign(
    allowed_targets: &[&str],
    probe: fn(&str, &[u8]) -> Result<bool, String>,
) -> Result<(), String> {
    let target = env_string("WORLDDB_FUZZ_TARGET_ID", None)?;
    if !allowed_targets.contains(&target.as_str()) {
        return Err(format!(
            "target is not registered for this runner: {target}"
        ));
    }
    let duration = Duration::from_secs(parse_u64("WORLDDB_FUZZ_SECONDS", Some("3600"))?.max(1));
    let seed = parse_u64("WORLDDB_FUZZ_SEED", Some("0x574f524c44444231"))?;
    let max_bytes = parse_usize("WORLDDB_FUZZ_MAX_INPUT_BYTES", "268435456")?;
    let timeout_seconds = parse_u64("WORLDDB_FUZZ_INPUT_TIMEOUT_SECONDS", Some("5"))?;
    if timeout_seconds == 0 {
        return Err("WORLDDB_FUZZ_INPUT_TIMEOUT_SECONDS must be positive".to_owned());
    }
    let timeout = Duration::from_secs(timeout_seconds);
    let report_path = env_string("WORLDDB_FUZZ_REPORT_PATH", None)?;
    let root = workspace_root()?;
    let source_revision = env_string("WORLDDB_SOURCE_REVISION", None)?;
    if source_revision.len() != 40
        || !source_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("WORLDDB_SOURCE_REVISION must be a full lowercase Git hash".to_owned());
    }
    let (seeds, names) = load_seeds(&root, max_bytes)?;
    let started = Instant::now();
    let mut rng = Rng(seed);
    let mut rounds = 0_u64;
    let mut accepted = 0_u64;
    let mut rejected = 0_u64;
    let mut crashes = Vec::new();
    let mut crash_corpus_files = Vec::new();
    while started.elapsed() < duration {
        let index = (rng.next() as usize) % seeds.len();
        let Some(seed_input) = seeds.get(index) else {
            return Err("seed corpus unexpectedly became empty".to_owned());
        };
        let input = mutate(seed_input, rounds, &mut rng, max_bytes);
        let result = run_one(probe, target.clone(), input.clone(), timeout);
        rounds = rounds.saturating_add(1);
        match result {
            Ok(true) => accepted = accepted.saturating_add(1),
            Ok(false) => rejected = rejected.saturating_add(1),
            Err(error) => {
                match archive_crash_input(&input, rounds) {
                    Ok(Some(path)) => crash_corpus_files.push(path),
                    Ok(None) => {}
                    Err(archive_error) => crashes.push(format!(
                        "round {rounds}: failed to archive crash input: {archive_error}"
                    )),
                }
                let name = names
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("unknown seed");
                crashes.push(format!("round {rounds} ({name}): {error}"));
                break;
            }
        }
    }
    let elapsed = started.elapsed();
    let report_path = PathBuf::from(report_path);
    let report_path = if report_path.is_absolute() {
        report_path
    } else {
        root.join(report_path)
    };
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let json = report(
        CampaignReport {
            target: &target,
            seed,
            source_revision: &source_revision,
            elapsed,
            rounds,
            accepted,
            rejected,
            corpus_inputs: seeds.len(),
        },
        &crashes,
        &crash_corpus_files,
    );
    fs::write(&report_path, json).map_err(|error| format!("{}: {error}", report_path.display()))?;
    if let Some(crash) = crashes.first() {
        return Err(crash.clone());
    }
    if elapsed < duration {
        return Err("campaign ended before its requested wall duration".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::write_crash_input;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn crash_corpus_preserves_the_complete_input_bytes() -> Result<(), String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("worlddb-fuzz-crash-{}-{nonce}", std::process::id()));
        let input = [0_u8, 0xff, b'W', b'D', 0x80, 0x7f];
        let filename = write_crash_input(&directory, &input, 7)?;
        let archived = fs::read(PathBuf::from(&directory).join(filename))
            .map_err(|error| error.to_string())?;
        if archived != input {
            return Err("archived crash input differs from the original bytes".to_owned());
        }
        fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
        Ok(())
    }
}
