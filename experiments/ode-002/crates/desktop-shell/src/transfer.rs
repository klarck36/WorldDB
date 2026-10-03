use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use worlddb_ode_engine::{MAX_STREAM_BYTES, StreamReport};

pub(super) const IPC_PROTOCOL_VERSION: u16 = 1;
pub(super) const MAX_TRANSFER_CHUNK_BYTES: u32 = 256 * 1024;

const TRANSFER_LIFETIME: Duration = Duration::from_secs(5 * 60);
const MAX_ACTIVE_TRANSFERS: usize = 16;
const MAX_ACTIVE_PER_SESSION: usize = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BeginTransferRequestV1 {
    pub protocol_version: u16,
    pub total_bytes: u64,
    pub chunk_bytes: u32,
}

#[derive(Serialize)]
pub(super) struct BeginTransferResponseV1 {
    pub protocol_version: u16,
    pub transfer_id: String,
    pub max_chunk_bytes: u32,
    pub expires_in_seconds: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct ChunkAcknowledgementV1 {
    pub protocol_version: u16,
    pub transfer_id: String,
    pub sequence: u64,
    pub bytes_received: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FinishTransferRequestV1 {
    pub protocol_version: u16,
    pub transfer_id: String,
    pub cancelled: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct TransferCompletionV1 {
    pub protocol_version: u16,
    pub transfer_id: String,
    pub bytes_received: u64,
    pub digest: String,
    pub cancelled: bool,
}

struct TransferRecord {
    owner_session: String,
    expires_at: Instant,
    next_sequence: u64,
    bytes_received: u64,
    total_bytes: u64,
    chunk_bytes: u32,
}

pub(super) struct TransferManager {
    transfers: Mutex<HashMap<[u8; 16], TransferRecord>>,
}

impl Default for TransferManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferManager {
    pub(super) fn new() -> Self {
        Self {
            transfers: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn begin<F>(
        &self,
        session_id: &str,
        request: BeginTransferRequestV1,
        start_engine_stream: F,
    ) -> Result<BeginTransferResponseV1, TransferError>
    where
        F: FnOnce(&str, u64, u32) -> Result<(), String>,
    {
        self.begin_at(session_id, request, Instant::now(), start_engine_stream)
    }

    fn begin_at<F>(
        &self,
        session_id: &str,
        request: BeginTransferRequestV1,
        now: Instant,
        start_engine_stream: F,
    ) -> Result<BeginTransferResponseV1, TransferError>
    where
        F: FnOnce(&str, u64, u32) -> Result<(), String>,
    {
        if request.protocol_version != IPC_PROTOCOL_VERSION
            || request.total_bytes == 0
            || request.total_bytes > MAX_STREAM_BYTES
            || request.chunk_bytes == 0
            || request.chunk_bytes > MAX_TRANSFER_CHUNK_BYTES
        {
            return Err(TransferError::Rejected);
        }
        let mut transfers = self
            .transfers
            .lock()
            .map_err(|_| TransferError::Unavailable)?;
        transfers.retain(|_, transfer| transfer.expires_at > now);
        if transfers.len() >= MAX_ACTIVE_TRANSFERS
            || transfers
                .values()
                .filter(|transfer| transfer.owner_session == session_id)
                .count()
                >= MAX_ACTIVE_PER_SESSION
        {
            return Err(TransferError::Unavailable);
        }

        for _ in 0..4 {
            let mut transfer_id = [0_u8; 16];
            getrandom::fill(&mut transfer_id).map_err(|_| TransferError::Unavailable)?;
            if let std::collections::hash_map::Entry::Vacant(entry) = transfers.entry(transfer_id) {
                let transfer_id_text = encode_id(transfer_id);
                start_engine_stream(&transfer_id_text, request.total_bytes, request.chunk_bytes)
                    .map_err(|_| TransferError::Unavailable)?;
                entry.insert(TransferRecord {
                    owner_session: session_id.to_owned(),
                    expires_at: now + TRANSFER_LIFETIME,
                    next_sequence: 0,
                    bytes_received: 0,
                    total_bytes: request.total_bytes,
                    chunk_bytes: request.chunk_bytes,
                });
                return Ok(BeginTransferResponseV1 {
                    protocol_version: IPC_PROTOCOL_VERSION,
                    transfer_id: transfer_id_text,
                    max_chunk_bytes: request.chunk_bytes,
                    expires_in_seconds: TRANSFER_LIFETIME.as_secs(),
                });
            }
        }
        Err(TransferError::Unavailable)
    }

    pub(super) fn push_chunk<F>(
        &self,
        session_id: &str,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
        accept_engine_chunk: F,
    ) -> Result<ChunkAcknowledgementV1, TransferError>
    where
        F: FnOnce(&str, u64, &[u8]) -> Result<u64, String>,
    {
        self.push_chunk_at(
            session_id,
            transfer_id,
            sequence,
            chunk,
            Instant::now(),
            accept_engine_chunk,
        )
    }

    fn push_chunk_at<F>(
        &self,
        session_id: &str,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
        now: Instant,
        accept_engine_chunk: F,
    ) -> Result<ChunkAcknowledgementV1, TransferError>
    where
        F: FnOnce(&str, u64, &[u8]) -> Result<u64, String>,
    {
        if chunk.is_empty() || chunk.len() > MAX_TRANSFER_CHUNK_BYTES as usize {
            return Err(TransferError::Rejected);
        }
        let transfer_key = decode_id(transfer_id).ok_or(TransferError::Unauthorized)?;
        let mut transfers = self
            .transfers
            .lock()
            .map_err(|_| TransferError::Unavailable)?;
        let Some(transfer) = transfers.get_mut(&transfer_key) else {
            return Err(TransferError::Unauthorized);
        };
        if transfer.expires_at <= now {
            transfers.remove(&transfer_key);
            return Err(TransferError::Unauthorized);
        }
        if transfer.owner_session != session_id {
            return Err(TransferError::Unauthorized);
        }
        if sequence != transfer.next_sequence {
            return Err(TransferError::Rejected);
        }
        if chunk.len() > transfer.chunk_bytes as usize
            || transfer.bytes_received + chunk.len() as u64 > transfer.total_bytes
        {
            return Err(TransferError::Rejected);
        }
        let expected_bytes = transfer.bytes_received + chunk.len() as u64;
        let acknowledged_bytes = accept_engine_chunk(transfer_id, sequence, chunk)
            .map_err(|_| TransferError::Unavailable)?;
        if acknowledged_bytes != expected_bytes {
            return Err(TransferError::Rejected);
        }
        transfer.bytes_received = expected_bytes;
        transfer.next_sequence += 1;
        Ok(ChunkAcknowledgementV1 {
            protocol_version: IPC_PROTOCOL_VERSION,
            transfer_id: transfer_id.to_owned(),
            sequence,
            bytes_received: transfer.bytes_received,
        })
    }

    pub(super) fn finish<F>(
        &self,
        session_id: &str,
        request: FinishTransferRequestV1,
        finish_engine_stream: F,
    ) -> Result<TransferCompletionV1, TransferError>
    where
        F: FnOnce(&str, bool) -> Result<StreamReport, String>,
    {
        if request.protocol_version != IPC_PROTOCOL_VERSION {
            return Err(TransferError::Rejected);
        }
        self.finish_at(session_id, request, Instant::now(), finish_engine_stream)
    }

    fn finish_at<F>(
        &self,
        session_id: &str,
        request: FinishTransferRequestV1,
        now: Instant,
        finish_engine_stream: F,
    ) -> Result<TransferCompletionV1, TransferError>
    where
        F: FnOnce(&str, bool) -> Result<StreamReport, String>,
    {
        let transfer_key = decode_id(&request.transfer_id).ok_or(TransferError::Unauthorized)?;
        let mut transfers = self
            .transfers
            .lock()
            .map_err(|_| TransferError::Unavailable)?;
        let Some(transfer) = transfers.get(&transfer_key) else {
            return Err(TransferError::Unauthorized);
        };
        if transfer.expires_at <= now {
            transfers.remove(&transfer_key);
            return Err(TransferError::Unauthorized);
        }
        if transfer.owner_session != session_id {
            return Err(TransferError::Unauthorized);
        }
        if request.cancelled && transfer.bytes_received >= transfer.total_bytes
            || !request.cancelled && transfer.bytes_received != transfer.total_bytes
        {
            return Err(TransferError::Rejected);
        }
        let expected_bytes = transfer.bytes_received;
        let report = finish_engine_stream(&request.transfer_id, request.cancelled)
            .map_err(|_| TransferError::Unavailable)?;
        let _ = transfers.remove(&transfer_key);
        if report.bytes_read != expected_bytes || report.cancelled != request.cancelled {
            return Err(TransferError::Rejected);
        }
        Ok(TransferCompletionV1 {
            protocol_version: IPC_PROTOCOL_VERSION,
            transfer_id: request.transfer_id,
            bytes_received: report.bytes_read,
            digest: report.digest,
            cancelled: report.cancelled,
        })
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.transfers.lock().expect("transfer table").len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferError {
    Unauthorized,
    Rejected,
    Unavailable,
}

fn encode_id(id: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(32);
    for byte in id {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_id(encoded: &str) -> Option<[u8; 16]> {
    if encoded.len() != 32 {
        return None;
    }
    let mut id = [0_u8; 16];
    for (index, byte) in id.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&encoded[start..start + 2], 16).ok()?;
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use worlddb_ode_engine::StreamReport;

    use super::{
        BeginTransferRequestV1, FinishTransferRequestV1, IPC_PROTOCOL_VERSION,
        MAX_TRANSFER_CHUNK_BYTES, TRANSFER_LIFETIME, TransferError, TransferManager,
    };

    fn begin_request(total_bytes: u64, chunk_bytes: u32) -> BeginTransferRequestV1 {
        BeginTransferRequestV1 {
            protocol_version: IPC_PROTOCOL_VERSION,
            total_bytes,
            chunk_bytes,
        }
    }

    fn finish_request(transfer_id: String, cancelled: bool) -> FinishTransferRequestV1 {
        FinishTransferRequestV1 {
            protocol_version: IPC_PROTOCOL_VERSION,
            transfer_id,
            cancelled,
        }
    }

    fn begin(
        manager: &TransferManager,
        total_bytes: u64,
        chunk_bytes: u32,
    ) -> super::BeginTransferResponseV1 {
        manager
            .begin(
                "session-a",
                begin_request(total_bytes, chunk_bytes),
                |_, _, _| Ok(()),
            )
            .unwrap()
    }

    fn push(
        manager: &TransferManager,
        session_id: &str,
        transfer_id: &str,
        sequence: u64,
        chunk: &[u8],
        acknowledged_bytes: u64,
    ) -> Result<super::ChunkAcknowledgementV1, TransferError> {
        manager.push_chunk(session_id, transfer_id, sequence, chunk, |_, _, _| {
            Ok(acknowledged_bytes)
        })
    }

    fn finish(
        manager: &TransferManager,
        session_id: &str,
        request: FinishTransferRequestV1,
        bytes_received: u64,
    ) -> Result<super::TransferCompletionV1, TransferError> {
        manager.finish(session_id, request, |_, cancelled| {
            Ok(StreamReport {
                bytes_read: bytes_received,
                digest: "00".repeat(32),
                cancelled,
            })
        })
    }

    #[test]
    fn chunk_acknowledgement_is_per_sequence_and_session() {
        let manager = TransferManager::new();
        let transfer = begin(&manager, 10, 4);
        let first = push(&manager, "session-a", &transfer.transfer_id, 0, &[1; 4], 4).unwrap();
        assert_eq!(first.bytes_received, 4);
        assert_eq!(
            push(&manager, "session-a", &transfer.transfer_id, 0, &[2; 4], 8),
            Err(TransferError::Rejected)
        );
        assert_eq!(
            push(&manager, "session-b", &transfer.transfer_id, 1, &[2; 4], 8),
            Err(TransferError::Unauthorized)
        );
        let second = push(&manager, "session-a", &transfer.transfer_id, 1, &[2; 4], 8).unwrap();
        assert_eq!(second.bytes_received, 8);
        let third = push(&manager, "session-a", &transfer.transfer_id, 2, &[3; 2], 10).unwrap();
        assert_eq!(third.bytes_received, 10);
        let complete = finish(
            &manager,
            "session-a",
            finish_request(transfer.transfer_id, false),
            10,
        )
        .unwrap();
        assert_eq!(complete.bytes_received, 10);
        assert!(!complete.cancelled);
    }

    #[test]
    fn cancelled_transfer_accepts_an_arbitrary_prefix_and_is_consumed() {
        let manager = TransferManager::new();
        let transfer = begin(&manager, 10, 4);
        push(&manager, "session-a", &transfer.transfer_id, 0, &[1; 3], 3).unwrap();
        let result = finish(
            &manager,
            "session-a",
            finish_request(transfer.transfer_id.clone(), true),
            3,
        )
        .unwrap();
        assert_eq!(result.bytes_received, 3);
        assert!(result.cancelled);
        assert_eq!(manager.active_count(), 0);
        assert_eq!(
            finish(
                &manager,
                "session-a",
                finish_request(transfer.transfer_id, true),
                0
            ),
            Err(TransferError::Unauthorized)
        );
    }

    #[test]
    fn malformed_sizes_unknown_versions_and_oversize_chunks_are_rejected() {
        let manager = TransferManager::new();
        assert!(
            manager
                .begin("session-a", begin_request(0, 4), |_, _, _| panic!(
                    "invalid request reached engine"
                ))
                .is_err()
        );
        assert!(
            manager
                .begin(
                    "session-a",
                    begin_request(10, MAX_TRANSFER_CHUNK_BYTES + 1),
                    |_, _, _| panic!("invalid request reached engine")
                )
                .is_err()
        );
        let mut request = begin_request(10, 4);
        request.protocol_version += 1;
        assert!(
            manager
                .begin("session-a", request, |_, _, _| panic!(
                    "invalid request reached engine"
                ))
                .is_err()
        );

        let transfer = begin(&manager, 10, 4);
        assert!(
            manager
                .push_chunk(
                    "session-a",
                    &transfer.transfer_id,
                    0,
                    &vec![0; MAX_TRANSFER_CHUNK_BYTES as usize + 1],
                    |_, _, _| panic!("oversize chunk reached engine"),
                )
                .is_err()
        );
        assert!(
            manager
                .push_chunk(
                    "session-a",
                    &transfer.transfer_id,
                    0,
                    &[0; 5],
                    |_, _, _| panic!("invalid chunk reached engine")
                )
                .is_err()
        );
        assert!(
            manager
                .finish(
                    "session-a",
                    finish_request(transfer.transfer_id, false),
                    |_, _| panic!("incomplete transfer reached engine")
                )
                .is_err()
        );
    }

    #[test]
    fn versioned_dtos_reject_renderer_selected_paths_and_principals() {
        let begin_with_path = serde_json::json!({
            "protocol_version": 1,
            "total_bytes": 8,
            "chunk_bytes": 4,
            "project_path": "C:/renderer/chosen/database"
        });
        assert!(serde_json::from_value::<BeginTransferRequestV1>(begin_with_path).is_err());

        let finish_with_principal = serde_json::json!({
            "protocol_version": 1,
            "transfer_id": "00000000000000000000000000000001",
            "cancelled": false,
            "principal": "renderer-selected-principal"
        });
        assert!(serde_json::from_value::<FinishTransferRequestV1>(finish_with_principal).is_err());
    }

    #[test]
    fn incomplete_finish_and_wrong_session_do_not_complete_a_transfer() {
        let manager = TransferManager::new();
        let transfer = begin(&manager, 10, 4);
        assert!(
            manager
                .finish(
                    "session-a",
                    finish_request(transfer.transfer_id.clone(), false),
                    |_, _| panic!("incomplete transfer reached engine")
                )
                .is_err()
        );
        assert_eq!(manager.active_count(), 1);

        let transfer = begin(&manager, 10, 4);
        assert!(
            manager
                .finish(
                    "session-b",
                    finish_request(transfer.transfer_id.clone(), true),
                    |_, _| panic!("wrong session reached engine")
                )
                .is_err()
        );
        assert_eq!(manager.active_count(), 2);
        push(&manager, "session-a", &transfer.transfer_id, 0, &[1; 4], 4)
            .expect("owner can continue after foreign-session rejection");
    }

    #[test]
    fn expired_transfers_are_removed_and_the_table_is_bounded_per_session() {
        let manager = TransferManager::new();
        let now = Instant::now();
        for _ in 0..4 {
            manager
                .begin_at("session-a", begin_request(10, 4), now, |_, _, _| Ok(()))
                .unwrap();
        }
        assert!(
            manager
                .begin_at("session-a", begin_request(10, 4), now, |_, _, _| panic!(
                    "limit failure reached engine"
                ))
                .is_err()
        );
        manager
            .begin_at(
                "session-a",
                begin_request(10, 4),
                now + TRANSFER_LIFETIME + Duration::from_nanos(1),
                |_, _, _| Ok(()),
            )
            .unwrap();
        assert_eq!(manager.active_count(), 1);
    }
}
