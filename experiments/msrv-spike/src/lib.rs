#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

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

pub fn generate_id_candidate() -> uuid::Uuid {
    uuid::Uuid::now_v7()
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

    #[test]
    fn typed_uuid_and_cli_candidates_are_available() {
        let id = generate_id_candidate();
        assert_eq!(id.as_bytes().len(), 16);
        let cli = Cli::try_parse_from(["worlddb", "verify"]).expect("valid spike command");
        assert_eq!(cli.command, Command::Verify);
    }

    proptest! {
        #[test]
        fn adapter_dto_round_trips_json(payload in prop::collection::vec(any::<u8>(), 0..256)) {
            let dto = AdapterDto {
                operation_id: generate_id_candidate().to_string(),
                payload: payload.clone(),
            };
            let encoded = encode_adapter_dto(&dto).expect("serializes adapter DTO");
            let decoded: AdapterDto = serde_json::from_slice(&encoded).expect("deserializes adapter DTO");
            prop_assert_eq!(decoded.payload, payload);
            prop_assert_eq!(decoded.operation_id, dto.operation_id);
        }
    }

    #[test]
    fn hash_and_diagnostic_candidates_are_callable() {
        assert_eq!(digest_candidate(b"worlddb"), digest_candidate(b"worlddb"));
        emit_diagnostic();
    }
}
