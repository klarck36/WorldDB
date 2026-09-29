#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::{Builder, Uuid};

const MAX_UUID_V7_TIMESTAMP_MS: u64 = (1u64 << 48) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalV7Error {
    ClockBeforeUnixEpoch,
    TimestampOutOfRange,
    EntropyUnavailable,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct AdapterDto {
    pub operation_id: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Parser, PartialEq)]
#[command(name = "worlddb")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Command {
    Verify,
}

pub fn generate_uuid_crate_candidate() -> Uuid {
    Uuid::now_v7()
}

fn current_unix_milliseconds() -> Result<u64, InternalV7Error> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| InternalV7Error::ClockBeforeUnixEpoch)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| InternalV7Error::TimestampOutOfRange)
}

fn generate_internal_v7_with(
    timestamp_ms: Result<u64, InternalV7Error>,
    fill_random: impl FnOnce(&mut [u8; 10]) -> Result<(), InternalV7Error>,
) -> Result<Uuid, InternalV7Error> {
    let timestamp_ms = timestamp_ms?;
    if timestamp_ms > MAX_UUID_V7_TIMESTAMP_MS {
        return Err(InternalV7Error::TimestampOutOfRange);
    }

    let mut random_bytes = [0; 10];
    fill_random(&mut random_bytes)?;
    Ok(Builder::from_unix_timestamp_millis(timestamp_ms, &random_bytes).into_uuid())
}

pub fn generate_internal_v7_candidate() -> Result<Uuid, InternalV7Error> {
    generate_internal_v7_with(current_unix_milliseconds(), |random_bytes| {
        getrandom::fill(random_bytes).map_err(|_| InternalV7Error::EntropyUnavailable)
    })
}

pub fn digest_candidate(bytes: &[u8]) -> blake3::Hash {
    blake3::hash(bytes)
}

pub fn encode_adapter_dto(dto: &AdapterDto) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(dto)
}

pub fn emit_diagnostic() {
    tracing::info!(operation = "msrv-spike", "dependency surface compiles");
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;
    use uuid::{ContextV7, Timestamp, Variant, Version};

    #[test]
    fn uuid_crate_v7_candidate_and_cli_are_available() {
        let id = generate_uuid_crate_candidate();
        assert_eq!(id.as_bytes().len(), 16);
        assert_eq!(id.get_version(), Some(Version::SortRand));
        assert_eq!(id.get_variant(), Variant::RFC4122);
        let cli = Cli::try_parse_from(["worlddb", "verify"]).expect("valid spike command");
        assert_eq!(cli.command, Command::Verify);
    }

    #[test]
    fn internal_candidate_matches_rfc_9562_uuid_v7_vector() {
        let random = [0x0c, 0xc3, 0x18, 0xc4, 0xdc, 0x0c, 0x0c, 0x07, 0x39, 0x8f];
        let id = generate_internal_v7_with(Ok(1_645_557_742_000), |output| {
            *output = random;
            Ok(())
        })
        .expect("RFC vector timestamp and random bytes are valid");

        assert_eq!(id.to_string(), "017f22e2-79b0-7cc3-98c4-dc0c0c07398f");
        assert_eq!(id.get_version(), Some(Version::SortRand));
        assert_eq!(id.get_variant(), Variant::RFC4122);
    }

    #[test]
    fn uuid_crate_context_is_monotonic_and_collision_free_at_one_millisecond() {
        let timestamp_ms = 1_645_557_742_000u64;
        let context = ContextV7::new();
        let mut previous = None;
        let mut ids = HashSet::new();

        for _ in 0..4096 {
            let timestamp = Timestamp::from_unix(
                &context,
                timestamp_ms / 1000,
                ((timestamp_ms % 1000) * 1_000_000) as u32,
            );
            let id = Uuid::new_v7(timestamp);
            assert_eq!(id.get_version(), Some(Version::SortRand));
            assert_eq!(id.get_variant(), Variant::RFC4122);
            if let Some(previous) = previous {
                assert!(previous < id, "UUIDv7 context lost creation order");
            }
            assert!(ids.insert(id), "UUIDv7 context generated a collision");
            previous = Some(id);
        }
    }

    #[test]
    fn internal_candidate_propagates_clock_and_entropy_failures() {
        let mut rng_called = false;
        let clock_error = generate_internal_v7_with(
            Err(InternalV7Error::ClockBeforeUnixEpoch),
            |_| {
                rng_called = true;
                Ok(())
            },
        );
        assert_eq!(clock_error, Err(InternalV7Error::ClockBeforeUnixEpoch));
        assert!(!rng_called, "entropy is not requested after a clock failure");

        let entropy_error = generate_internal_v7_with(Ok(1_645_557_742_000), |_| {
            Err(InternalV7Error::EntropyUnavailable)
        });
        assert_eq!(entropy_error, Err(InternalV7Error::EntropyUnavailable));

        let range_error = generate_internal_v7_with(Ok(MAX_UUID_V7_TIMESTAMP_MS + 1), |_| {
            rng_called = true;
            Ok(())
        });
        assert_eq!(range_error, Err(InternalV7Error::TimestampOutOfRange));
        assert!(!rng_called, "entropy is not requested for an invalid timestamp");

        let boundary = generate_internal_v7_with(Ok(MAX_UUID_V7_TIMESTAMP_MS), |output| {
            *output = [0; 10];
            Ok(())
        })
        .expect("maximum 48-bit timestamp is valid");
        let boundary_timestamp = MAX_UUID_V7_TIMESTAMP_MS.to_be_bytes();
        assert_eq!(&boundary.as_bytes()[..6], &boundary_timestamp[2..]);
    }

    #[test]
    fn internal_candidate_uses_system_clock_and_os_csprng() {
        let id = generate_internal_v7_candidate().expect("clock and OS CSPRNG are available");
        assert_eq!(id.get_version(), Some(Version::SortRand));
        assert_eq!(id.get_variant(), Variant::RFC4122);
    }

    #[test]
    fn internal_candidate_has_no_collisions_in_fixed_timestamp_csprng_sample() {
        let timestamp_ms = 1_645_557_742_000u64;
        let mut ids = HashSet::new();

        for _ in 0..4096 {
            let id = generate_internal_v7_with(Ok(timestamp_ms), |random_bytes| {
                getrandom::fill(random_bytes).map_err(|_| InternalV7Error::EntropyUnavailable)
            })
            .expect("OS CSPRNG is available for the sample");
            assert!(ids.insert(id), "CSPRNG sample contained a UUID collision");
        }
    }

    #[test]
    fn internal_random_tail_does_not_promise_same_millisecond_order() {
        let timestamp_ms = 1_645_557_742_000u64;
        let high = generate_internal_v7_with(Ok(timestamp_ms), |output| {
            *output = [u8::MAX; 10];
            Ok(())
        })
        .expect("valid high random payload");
        let low = generate_internal_v7_with(Ok(timestamp_ms), |output| {
            *output = [0; 10];
            Ok(())
        })
        .expect("valid low random payload");
        let next_millisecond = generate_internal_v7_with(Ok(timestamp_ms + 1), |output| {
            *output = [0; 10];
            Ok(())
        })
        .expect("valid next millisecond");

        assert!(low < high, "random tail ordering is independent of call order");
        assert!(high < next_millisecond, "the UUIDv7 timestamp prefix sorts first");
    }

    proptest! {
        #[test]
        fn adapter_dto_round_trips_json(payload in prop::collection::vec(any::<u8>(), 0..256)) {
            let dto = AdapterDto {
                operation_id: generate_uuid_crate_candidate().to_string(),
                payload: payload.clone(),
            };
            let encoded = encode_adapter_dto(&dto).expect("serializes adapter DTO");
            let decoded: AdapterDto = serde_json::from_slice(&encoded).expect("deserializes adapter DTO");
            prop_assert_eq!(decoded.payload, payload);
            prop_assert_eq!(decoded.operation_id, dto.operation_id);
        }
    }

    proptest! {
        #[test]
        fn internal_candidate_preserves_uuid_v7_layout(
            timestamp_ms in 0u64..(1u64 << 48),
            random_bytes in any::<[u8; 10]>(),
        ) {
            let id = generate_internal_v7_with(Ok(timestamp_ms), |output| {
                *output = random_bytes;
                Ok(())
            }).expect("48-bit timestamp is in range");
            let timestamp_bytes = timestamp_ms.to_be_bytes();

            prop_assert_eq!(&id.as_bytes()[..6], &timestamp_bytes[2..]);
            prop_assert_eq!(id.get_version(), Some(Version::SortRand));
            prop_assert_eq!(id.get_variant(), Variant::RFC4122);
            let encoded = id.to_string();
            prop_assert!(encoded.bytes().all(|byte| !byte.is_ascii_uppercase()));
            let decoded = Uuid::parse_str(&encoded).expect("crate parser accepts its own UUID");
            prop_assert_eq!(decoded, id);
        }
    }

    #[test]
    fn hash_and_diagnostic_candidates_are_callable() {
        assert_eq!(digest_candidate(b"worlddb"), digest_candidate(b"worlddb"));
        emit_diagnostic();
    }
}
