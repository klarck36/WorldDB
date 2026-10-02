#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use worlddb_testkit::{Seed, SplitMix64};

const CORPUS_VERSION: &str = "m6-14a-v1";
const ENTITIES_DOMAIN: u64 = 0x454e_5449_5449_4553;
const ASSERTIONS_DOMAIN: u64 = 0x4153_5345_5254_494f;
const EVENTS_DOMAIN: u64 = 0x4556_454e_5453_0001;
const HISTORY_SPACES_DOMAIN: u64 = 0x4849_5354_4f52_5901;
const PROVENANCE_DOMAIN: u64 = 0x5052_4f56_454e_414e;

#[derive(Clone, Copy)]
struct Counts {
    entities: u64,
    assertions: u64,
    events: u64,
    history_spaces: u16,
    provenance_edges: u64,
}

impl Counts {
    const FULL: Self = Self {
        entities: 100_000,
        assertions: 1_000_000,
        events: 10_000,
        history_spaces: 100,
        provenance_edges: 10_000_000,
    };

    const SMOKE: Self = Self {
        entities: 256,
        assertions: 2_048,
        events: 64,
        history_spaces: 12,
        provenance_edges: 4_096,
    };
}

struct Options {
    output_dir: PathBuf,
    seed: Seed,
    counts: Counts,
    profile: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("m6-14a-corpus: {error}");
        std::process::exit(2);
    }
}

fn run() -> io::Result<()> {
    let options =
        parse_options().map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    prepare_output_dir(&options.output_dir)?;

    let mut entity_rng = domain_rng(options.seed, ENTITIES_DOMAIN);
    write_entities(
        &options.output_dir.join("entities.bin"),
        options.counts.entities,
        options.counts.history_spaces,
        &mut entity_rng,
    )?;

    let mut assertion_rng = domain_rng(options.seed, ASSERTIONS_DOMAIN);
    let masked_assertions = write_assertions(
        &options.output_dir.join("assertions.bin"),
        &options.output_dir.join("masks.bin"),
        options.counts,
        &mut assertion_rng,
    )?;

    let mut event_rng = domain_rng(options.seed, EVENTS_DOMAIN);
    write_events(
        &options.output_dir.join("events.bin"),
        options.counts,
        &mut event_rng,
    )?;

    let mut history_rng = domain_rng(options.seed, HISTORY_SPACES_DOMAIN);
    write_history_spaces(
        &options.output_dir.join("history_spaces.bin"),
        options.counts.history_spaces,
        &mut history_rng,
    )?;

    let mut provenance_rng = domain_rng(options.seed, PROVENANCE_DOMAIN);
    write_provenance_edges(
        &options.output_dir.join("provenance_edges.bin"),
        options.counts,
        masked_assertions,
        &mut provenance_rng,
    )?;

    println!(
        "corpus_version={CORPUS_VERSION} profile={} seed={} entities={} assertions={} masks={} events={} history_spaces={} provenance_edges={}",
        options.profile,
        options.seed,
        options.counts.entities,
        options.counts.assertions,
        masked_assertions,
        options.counts.events,
        options.counts.history_spaces,
        options.counts.provenance_edges,
    );
    Ok(())
}

fn parse_options() -> Result<Options, String> {
    let mut arguments = env::args_os().skip(1);
    let mut output_dir = None;
    let mut seed = None;
    let mut profile = None;
    while let Some(argument) = arguments.next() {
        let Some(argument) = argument.to_str() else {
            return Err("arguments must be valid Unicode".to_owned());
        };
        match argument {
            "--output-dir" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--output-dir needs a path".to_owned())?;
                output_dir = Some(PathBuf::from(value));
            }
            "--seed" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--seed needs a decimal or hexadecimal value".to_owned())?;
                let value = value
                    .to_str()
                    .ok_or_else(|| "--seed must be valid Unicode".to_owned())?;
                seed = Some(Seed::parse(value).map_err(|error| error.to_string())?);
            }
            "--profile" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--profile needs `full` or `smoke`".to_owned())?;
                let value = value
                    .to_str()
                    .ok_or_else(|| "--profile must be valid Unicode".to_owned())?;
                profile = Some(value.to_owned());
            }
            _ => return Err(format!("unknown argument {argument:?}")),
        }
    }
    let output_dir = output_dir.ok_or_else(|| {
        "usage: m6-14a-corpus --output-dir <empty-directory> --seed <value> --profile <full|smoke>"
            .to_owned()
    })?;
    let seed = seed.ok_or_else(|| "--seed is required".to_owned())?;
    let profile = profile.ok_or_else(|| "--profile is required".to_owned())?;
    let (counts, profile) = match profile.as_str() {
        "full" => (Counts::FULL, "full"),
        "smoke" => (Counts::SMOKE, "smoke"),
        _ => return Err("--profile must be `full` or `smoke`".to_owned()),
    };
    Ok(Options {
        output_dir,
        seed,
        counts,
        profile,
    })
}

fn prepare_output_dir(path: &Path) -> io::Result<()> {
    if path.exists() {
        let mut entries = fs::read_dir(path)?;
        if entries.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "output directory must be empty",
            ));
        }
    } else {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn domain_rng(seed: Seed, domain: u64) -> SplitMix64 {
    SplitMix64::new(Seed::from_u64(seed.value() ^ domain))
}

fn writer(path: &Path) -> io::Result<BufWriter<File>> {
    Ok(BufWriter::with_capacity(1024 * 1024, File::create(path)?))
}

fn write_row(output: &mut BufWriter<File>, row: &[u8], expected_len: usize) -> io::Result<()> {
    if row.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("record has {} bytes, expected {expected_len}", row.len()),
        ));
    }
    output.write_all(row)
}

fn push_u8(row: &mut Vec<u8>, value: u8) {
    row.push(value);
}

fn push_u16(row: &mut Vec<u8>, value: u16) {
    row.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(row: &mut Vec<u8>, value: u32) {
    row.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(row: &mut Vec<u8>, value: u64) {
    row.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(row: &mut Vec<u8>, value: i64) {
    row.extend_from_slice(&value.to_le_bytes());
}

fn bounded(rng: &mut SplitMix64, upper: u32) -> io::Result<u32> {
    if upper == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bounded random selection needs a positive upper bound",
        ));
    }
    u32::try_from(rng.next_u64() % u64::from(upper))
        .map_err(|_| io::Error::other("bounded random value is out of range"))
}

fn skewed_id(rng: &mut SplitMix64, hot_set: u32, total: u32, hot_percent: u64) -> io::Result<u32> {
    let sample = rng.next_u64();
    let (upper, offset) = if sample % 100 < hot_percent {
        (hot_set, 1)
    } else {
        (total, 1)
    };
    bounded(rng, upper)?
        .checked_add(offset)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "skewed ID overflowed its range"))
}

fn write_entities(
    path: &Path,
    count: u64,
    history_space_count: u16,
    rng: &mut SplitMix64,
) -> io::Result<()> {
    let mut output = writer(path)?;
    let mut row = Vec::with_capacity(16);
    for entity_id in 1..=count {
        row.clear();
        push_u64(&mut row, entity_id);
        push_u32(&mut row, bounded(rng, 64)?);
        let history_space = u16::try_from(bounded(rng, u32::from(history_space_count))? + 1)
            .map_err(|_| io::Error::other("entity history-space ID is out of range"))?;
        push_u16(&mut row, history_space);
        push_u16(
            &mut row,
            u16::try_from(rng.next_u64() & 0x000f)
                .map_err(|_| io::Error::other("entity flags are out of range"))?,
        );
        write_row(&mut output, &row, 16)?;
    }
    output.flush()
}

fn write_assertions(
    assertions_path: &Path,
    masks_path: &Path,
    counts: Counts,
    rng: &mut SplitMix64,
) -> io::Result<u64> {
    let mut assertions = writer(assertions_path)?;
    let mut masks = writer(masks_path)?;
    let mut row = Vec::with_capacity(48);
    let mut mask_row = Vec::with_capacity(16);
    let mut mask_count = 0_u64;
    let entity_count = u32::try_from(counts.entities)
        .map_err(|_| io::Error::other("entity count exceeds the assertion schema"))?;
    let history_space_count = u32::from(counts.history_spaces);
    let validity = [
        (-100, 100),
        (0, 1),
        (100, 200),
        (-1_000, -900),
        (50, 60),
        (500, 5_000),
    ];
    for assertion_id in 1..=counts.assertions {
        let entity_id = skewed_id(rng, 128.min(entity_count), entity_count, 80)?;
        let predicate_id = skewed_id(rng, 16, 1_024, 75)?;
        let history_space_id = u16::try_from(bounded(rng, history_space_count)? + 1)
            .map_err(|_| io::Error::other("assertion history-space ID is out of range"))?;
        let layer_id = u8::try_from(bounded(rng, 8)?)
            .map_err(|_| io::Error::other("assertion layer ID is out of range"))?;
        let polarity = u8::from(rng.next_u64() & 1 == 1);
        let value_id = skewed_id(rng, 256, 16_384, 85)?;
        let validity_index = usize::try_from(bounded(rng, 6)?)
            .map_err(|_| io::Error::other("validity index is out of range"))?;
        let Some((valid_start, valid_end)) = validity.get(validity_index).copied() else {
            return Err(io::Error::other("validity index is out of range"));
        };
        let masked = rng.next_u64() % 100 < 20;
        row.clear();
        push_u64(&mut row, assertion_id);
        push_u32(&mut row, entity_id);
        push_u32(&mut row, predicate_id);
        push_u16(&mut row, history_space_id);
        push_u8(&mut row, layer_id);
        push_u8(&mut row, polarity);
        push_u32(&mut row, value_id);
        push_i64(&mut row, valid_start);
        push_i64(&mut row, valid_end);
        push_u32(&mut row, u32::from(masked));
        push_u32(&mut row, 0);
        write_row(&mut assertions, &row, 48)?;
        if masked {
            mask_count = mask_count
                .checked_add(1)
                .ok_or_else(|| io::Error::other("mask count overflowed"))?;
            mask_row.clear();
            push_u64(&mut mask_row, mask_count);
            push_u64(&mut mask_row, assertion_id);
            write_row(&mut masks, &mask_row, 16)?;
        }
    }
    assertions.flush()?;
    masks.flush()?;
    Ok(mask_count)
}

fn write_events(path: &Path, counts: Counts, rng: &mut SplitMix64) -> io::Result<()> {
    let mut output = writer(path)?;
    let mut row = Vec::with_capacity(40);
    let history_space_count = u32::from(counts.history_spaces);
    let entity_count = u32::try_from(counts.entities)
        .map_err(|_| io::Error::other("entity count exceeds event endpoint schema"))?;
    for event_id in 1..=counts.events {
        let event_kind = skewed_id(rng, 4, 256, 75)?;
        let participant_role = skewed_id(rng, 2, 32, 80)?;
        let participant_entity = skewed_id(rng, 128.min(entity_count), entity_count, 80)?;
        let history_space_id = u16::try_from(bounded(rng, history_space_count)? + 1)
            .map_err(|_| io::Error::other("event history-space ID is out of range"))?;
        let timeline_id = u16::try_from(bounded(rng, 16)? + 1)
            .map_err(|_| io::Error::other("timeline ID is out of range"))?;
        let start = i64::from(bounded(rng, 2_000_000)?) - 1_000_000;
        let duration = i64::from(bounded(rng, 100_000)?);
        let end = start
            .checked_add(duration)
            .ok_or_else(|| io::Error::other("event end time overflowed"))?;
        let span = rng.next_u64() & 1 == 1;
        row.clear();
        push_u64(&mut row, event_id);
        push_u16(
            &mut row,
            u16::try_from(event_kind)
                .map_err(|_| io::Error::other("event kind ID is out of range"))?,
        );
        push_u16(
            &mut row,
            u16::try_from(participant_role)
                .map_err(|_| io::Error::other("event role ID is out of range"))?,
        );
        push_u32(&mut row, participant_entity);
        push_u16(&mut row, history_space_id);
        push_u16(&mut row, timeline_id);
        push_i64(&mut row, start);
        push_i64(&mut row, if span { end } else { start });
        push_u32(&mut row, u32::from(span));
        write_row(&mut output, &row, 40)?;
    }
    output.flush()
}

fn write_history_spaces(path: &Path, count: u16, rng: &mut SplitMix64) -> io::Result<()> {
    let mut output = writer(path)?;
    let mut row = Vec::with_capacity(16);
    for history_space_id in 1..=count {
        let parent_id = if history_space_id == 1 {
            u16::MAX
        } else {
            history_space_id / 2
        };
        row.clear();
        push_u16(&mut row, history_space_id);
        push_u16(&mut row, parent_id);
        push_u64(&mut row, u64::from(history_space_id) * 1_000);
        push_u8(
            &mut row,
            u8::try_from(rng.next_u64() % 100)
                .map_err(|_| io::Error::other("history-space skew is out of range"))?,
        );
        push_u8(
            &mut row,
            u8::try_from(rng.next_u64() % 8 + 1)
                .map_err(|_| io::Error::other("history-space layer count is out of range"))?,
        );
        push_u16(&mut row, 0);
        write_row(&mut output, &row, 16)?;
    }
    output.flush()
}

fn endpoint(rng: &mut SplitMix64, counts: Counts, mask_count: u64) -> io::Result<(u16, u32)> {
    let selector = bounded(rng, 100)?;
    let (kind, total) = if selector < 70 {
        (
            1_u16,
            u32::try_from(counts.assertions)
                .map_err(|_| io::Error::other("assertion count exceeds the endpoint schema"))?,
        )
    } else if selector < 90 {
        (
            2_u16,
            u32::try_from(counts.entities)
                .map_err(|_| io::Error::other("entity count exceeds endpoint schema"))?,
        )
    } else if selector < 98 {
        (
            3_u16,
            u32::try_from(counts.events)
                .map_err(|_| io::Error::other("event count exceeds endpoint schema"))?,
        )
    } else {
        (
            4_u16,
            u32::try_from(mask_count)
                .map_err(|_| io::Error::other("mask count exceeds endpoint schema"))?,
        )
    };
    if total == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "provenance endpoint family cannot be empty",
        ));
    }
    Ok((kind, bounded(rng, total)? + 1))
}

fn write_provenance_edges(
    path: &Path,
    counts: Counts,
    mask_count: u64,
    rng: &mut SplitMix64,
) -> io::Result<()> {
    let mut output = writer(path)?;
    let mut row = Vec::with_capacity(32);
    let history_space_count = u32::from(counts.history_spaces);
    for edge_id in 1..=counts.provenance_edges {
        let (from_kind, from_id) = endpoint(rng, counts, mask_count)?;
        let (to_kind, to_id) = endpoint(rng, counts, mask_count)?;
        let relation = u16::try_from(bounded(rng, 16)?)
            .map_err(|_| io::Error::other("provenance relation is out of range"))?;
        let history_space_id = u16::try_from(bounded(rng, history_space_count)? + 1)
            .map_err(|_| io::Error::other("provenance history-space ID is out of range"))?;
        let revision = rng.next_u64() % 1_000_000 + 1;
        let flags = u32::from(rng.next_u64() % 100 < 5);
        row.clear();
        push_u64(&mut row, edge_id);
        push_u16(&mut row, from_kind);
        push_u32(&mut row, from_id);
        push_u16(&mut row, to_kind);
        push_u32(&mut row, to_id);
        push_u16(&mut row, relation);
        push_u16(&mut row, history_space_id);
        push_u64(&mut row, revision);
        push_u32(&mut row, flags);
        write_row(&mut output, &row, 36)?;
    }
    output.flush()
}
