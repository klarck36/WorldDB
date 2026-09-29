#![forbid(unsafe_code)]

//! Deterministic, dependency-free support for WorldDB tests.

#[cfg(all(feature = "fault-injection", not(debug_assertions)))]
compile_error!("WorldDB test fault hooks cannot be built with a release profile");

use std::env;
use std::fmt;

/// Stable fallback seed used when `WORLDDB_TEST_SEED` is not set.
pub const DEFAULT_SEED: Seed = Seed::from_u64(0x574f_524c_4444_4231);

/// An explicit, reproducible 64-bit test seed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Seed(u64);

impl Seed {
    /// Creates a seed from its numeric value.
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric value of this seed.
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Parses an unsigned decimal or `0x`-prefixed hexadecimal seed.
    pub fn parse(value: &str) -> Result<Self, SeedError> {
        let (digits, radix) = match value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            Some(hexadecimal) => (hexadecimal, 16),
            None => (value, 10),
        };
        if digits.is_empty() {
            return Err(SeedError::Empty);
        }
        u64::from_str_radix(digits, radix)
            .map(Self)
            .map_err(|_| SeedError::Invalid(value.to_owned()))
    }

    /// Reads `WORLDDB_TEST_SEED`, or returns [`DEFAULT_SEED`] when unset.
    pub fn from_environment() -> Result<Self, SeedError> {
        match env::var("WORLDDB_TEST_SEED") {
            Ok(value) => Self::parse(&value),
            Err(env::VarError::NotPresent) => Ok(DEFAULT_SEED),
            Err(env::VarError::NotUnicode(_)) => Err(SeedError::NonUnicodeEnvironmentValue),
        }
    }
}

impl fmt::Display for Seed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{:016x}", self.0)
    }
}

/// An invalid value supplied as a deterministic seed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SeedError {
    Empty,
    Invalid(String),
    NonUnicodeEnvironmentValue,
}

impl fmt::Display for SeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("seed must not be empty"),
            Self::Invalid(value) => write!(formatter, "invalid 64-bit seed {value:?}"),
            Self::NonUnicodeEnvironmentValue => {
                formatter.write_str("WORLDDB_TEST_SEED is not valid Unicode")
            }
        }
    }
}

impl std::error::Error for SeedError {}

/// Small, deterministic SplitMix64 generator for test data only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Starts a stream from an explicit seed.
    pub const fn new(seed: Seed) -> Self {
        Self {
            state: seed.value(),
        }
    }

    /// Produces the next value in the stable SplitMix64 stream.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

/// Test-only, one-shot fault hooks. This module is unavailable in release builds.
#[cfg(feature = "fault-injection")]
pub mod fault {
    use super::Seed;
    use std::fmt;

    /// Stable marker used by artifact checks to identify the opt-in hook API.
    pub const MARKER: &str = "WorldDB::fault-injection::M0-13";

    /// A one-shot fault hook, inert until explicitly armed.
    #[derive(Debug, Default)]
    pub struct FaultHook {
        armed: bool,
    }

    impl FaultHook {
        /// Creates an armed one-shot hook.
        pub const fn armed() -> Self {
            Self { armed: true }
        }

        /// Triggers the hook once and associates the failure with a replayable seed.
        pub fn trigger(&mut self, seed: Seed, point: &'static str) -> Result<(), InjectedFault> {
            if self.armed {
                self.armed = false;
                Err(InjectedFault { seed, point })
            } else {
                Ok(())
            }
        }
    }

    /// A deliberately injected and reproducible test failure.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct InjectedFault {
        seed: Seed,
        point: &'static str,
    }

    impl fmt::Display for InjectedFault {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "injected fault at {} with seed {}",
                self.point, self.seed
            )
        }
    }

    impl std::error::Error for InjectedFault {}
}

#[cfg(test)]
mod tests {
    use super::{Seed, SplitMix64};
    use std::collections::HashSet;

    const FUZZ_SEEDS: &str = include_str!("../testdata/m0-13/fuzz-seeds/v1/seeds.tsv");
    const GOLDEN_STREAM: &str = include_str!("../testdata/m0-13/golden/v1/splitmix64.tsv");
    const FULL_SCAN: &str = include_str!("../testdata/m0-13/full-scan/v1/records.tsv");

    #[test]
    fn parses_and_formats_decimal_and_hex_seeds() -> Result<(), String> {
        let decimal = Seed::parse("42").map_err(|error| error.to_string())?;
        let hexadecimal = Seed::parse("0x2a").map_err(|error| error.to_string())?;
        if decimal != hexadecimal || decimal.to_string() != "0x000000000000002a" {
            return Err("seed normalization did not preserve the numeric value".to_owned());
        }
        Ok(())
    }

    #[test]
    fn rejects_empty_invalid_and_out_of_range_seeds() {
        assert!(Seed::parse("").is_err());
        assert!(Seed::parse("0x").is_err());
        assert!(Seed::parse("-1").is_err());
        assert!(Seed::parse("18446744073709551616").is_err());
    }

    #[test]
    fn environment_seed_is_explicit_or_uses_the_stable_fallback() -> Result<(), String> {
        let expected = match std::env::var("WORLDDB_TEST_SEED") {
            Ok(value) => Seed::parse(&value).map_err(|error| error.to_string())?,
            Err(std::env::VarError::NotPresent) => super::DEFAULT_SEED,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err("WORLDDB_TEST_SEED is not valid Unicode".to_owned());
            }
        };
        let actual = Seed::from_environment().map_err(|error| error.to_string())?;
        if actual != expected {
            return Err(format!("expected seed {expected}, got {actual}"));
        }
        Ok(())
    }

    #[test]
    fn versioned_seed_corpus_is_parseable_and_unique() -> Result<(), String> {
        let mut lines = FUZZ_SEEDS.lines();
        if lines.next() != Some("seed_id\tseed\tnote") {
            return Err("unexpected fuzz seed corpus header".to_owned());
        }
        let mut ids = HashSet::new();
        let mut seeds = HashSet::new();
        for line in lines {
            let Some((id, remainder)) = line.split_once('\t') else {
                return Err("fuzz seed corpus row is missing columns".to_owned());
            };
            let Some((seed, note)) = remainder.split_once('\t') else {
                return Err("fuzz seed corpus row is missing its note".to_owned());
            };
            if id.is_empty() || note.is_empty() || !ids.insert(id) {
                return Err("fuzz seed IDs must be non-empty and unique".to_owned());
            }
            let parsed = Seed::parse(seed).map_err(|error| error.to_string())?;
            if !seeds.insert(parsed.value()) {
                return Err("fuzz corpus seeds must be unique".to_owned());
            }
        }
        if ids.len() != 8 {
            return Err(format!("expected 8 versioned seeds, found {}", ids.len()));
        }
        Ok(())
    }

    #[test]
    fn splitmix64_matches_the_versioned_golden_stream() -> Result<(), String> {
        let mut lines = GOLDEN_STREAM.lines();
        if lines.next() != Some("seed\tindex\tvalue") {
            return Err("unexpected golden stream header".to_owned());
        }
        let mut row_count = 0usize;
        for line in lines {
            let mut fields = line.split('\t');
            let Some(seed_text) = fields.next() else {
                return Err("golden stream row has no seed".to_owned());
            };
            let Some(index_text) = fields.next() else {
                return Err("golden stream row has no index".to_owned());
            };
            let Some(value_text) = fields.next() else {
                return Err("golden stream row has no value".to_owned());
            };
            if fields.next().is_some() {
                return Err("golden stream row has too many columns".to_owned());
            }
            let seed = Seed::parse(seed_text).map_err(|error| error.to_string())?;
            let expected_index = index_text
                .parse::<usize>()
                .map_err(|error| error.to_string())?;
            let expected_value =
                u64::from_str_radix(value_text, 16).map_err(|error| error.to_string())?;
            let actual_value = SplitMix64::new(seed).next_u64();
            if expected_index != 0 || actual_value != expected_value {
                return Err(format!("golden mismatch for seed {seed_text}"));
            }
            row_count += 1;
        }
        if row_count != 4 {
            return Err(format!("expected 4 golden vectors, found {row_count}"));
        }
        Ok(())
    }

    #[test]
    fn synthetic_full_scan_fixture_visits_every_ordered_record() -> Result<(), String> {
        let mut lines = FULL_SCAN.lines();
        if lines.next() != Some("sequence\tpayload") {
            return Err("unexpected full-scan fixture header".to_owned());
        }
        let mut count = 0usize;
        for line in lines {
            let Some((sequence, payload)) = line.split_once('\t') else {
                return Err("full-scan fixture row is missing columns".to_owned());
            };
            count += 1;
            let expected = format!("{count:04}");
            if sequence != expected || payload != format!("opaque-{count:04}") {
                return Err(format!(
                    "full-scan row {count} is out of order or malformed"
                ));
            }
        }
        if count != 16 {
            return Err(format!("expected 16 full-scan rows, found {count}"));
        }
        Ok(())
    }

    #[test]
    fn same_seed_replays_the_same_stream() {
        let seed = Seed::from_u64(0x574f_524c_4444_4231);
        let mut first = SplitMix64::new(seed);
        let mut replay = SplitMix64::new(seed);
        for _ in 0..32 {
            assert_eq!(first.next_u64(), replay.next_u64());
        }
    }

    #[cfg(feature = "fault-injection")]
    #[test]
    fn fault_hook_is_one_shot_and_includes_the_replay_seed() {
        let seed = Seed::from_u64(7);
        let mut hook = super::fault::FaultHook::armed();
        let failure = hook.trigger(seed, "unit-test");
        assert_eq!(
            failure.map_err(|error| error.to_string()),
            Err("injected fault at unit-test with seed 0x0000000000000007".to_owned())
        );
        assert_eq!(hook.trigger(seed, "unit-test"), Ok(()));
    }
}
