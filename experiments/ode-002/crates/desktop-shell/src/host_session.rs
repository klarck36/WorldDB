use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

const SESSION_LIFETIME: Duration = Duration::from_secs(15 * 60);
const MAX_HOST_SESSIONS: usize = 16;
const ALLOWED_WINDOWS: [&str; 2] = ["primary", "secondary"];
const IPC_PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HostCapability {
    HealthRead,
    TransferWrite,
    ProjectOpen,
    ProjectCreate,
    ProjectClose,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct HostIdentity(Vec<u8>);

impl HostIdentity {
    pub(super) fn current() -> Result<Self, HostIdentityError> {
        worlddb_process_adapter::current_process_identity_bytes()
            .map(Self)
            .map_err(|error| match error {
                worlddb_process_adapter::ProcessIdentityError::UnsupportedPlatform => {
                    HostIdentityError::UnsupportedPlatform
                }
                worlddb_process_adapter::ProcessIdentityError::OperatingSystemFailure => {
                    HostIdentityError::OperatingSystemFailure
                }
            })
    }

    pub(super) fn project_principal(&self) -> Result<worlddb_core::PrincipalId, HostIdentityError> {
        worlddb_core::derive_host_account_principal(&self.0)
            .map_err(|_| HostIdentityError::InvalidMapping)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HostIdentityError {
    UnsupportedPlatform,
    OperatingSystemFailure,
    InvalidMapping,
}

#[derive(Serialize)]
pub(super) struct HostSessionTicket {
    pub protocol_version: u16,
    pub session_id: String,
    pub expires_in_seconds: u64,
}

struct SessionRecord {
    identity: HostIdentity,
    window_label: String,
    capabilities: [HostCapability; 5],
    expires_at: Instant,
}

pub(super) struct HostSessionManager {
    identity: HostIdentity,
    sessions: Mutex<HashMap<[u8; 16], SessionRecord>>,
}

impl HostSessionManager {
    pub(super) fn new(identity: HostIdentity) -> Self {
        Self {
            identity,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn project_principal(&self) -> Result<worlddb_core::PrincipalId, SessionError> {
        self.identity
            .project_principal()
            .map_err(|_| SessionError::Unavailable)
    }

    pub(super) fn issue(&self, window_label: &str) -> Result<HostSessionTicket, SessionError> {
        self.issue_at(window_label, Instant::now())
    }

    fn issue_at(
        &self,
        window_label: &str,
        now: Instant,
    ) -> Result<HostSessionTicket, SessionError> {
        if !ALLOWED_WINDOWS.contains(&window_label) {
            return Err(SessionError::Unauthorized);
        }

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        sessions.retain(|_, record| record.expires_at > now);
        if sessions.len() >= MAX_HOST_SESSIONS {
            return Err(SessionError::Unavailable);
        }

        let mut session_id = [0_u8; 16];
        for _ in 0..4 {
            getrandom::fill(&mut session_id).map_err(|_| SessionError::Unavailable)?;
            if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(session_id) {
                let expires_at = now + SESSION_LIFETIME;
                entry.insert(SessionRecord {
                    identity: self.identity.clone(),
                    window_label: window_label.to_owned(),
                    capabilities: [
                        HostCapability::HealthRead,
                        HostCapability::TransferWrite,
                        HostCapability::ProjectOpen,
                        HostCapability::ProjectCreate,
                        HostCapability::ProjectClose,
                    ],
                    expires_at,
                });
                return Ok(HostSessionTicket {
                    protocol_version: IPC_PROTOCOL_VERSION,
                    session_id: encode_session_id(session_id),
                    expires_in_seconds: SESSION_LIFETIME.as_secs(),
                });
            }
        }
        Err(SessionError::Unavailable)
    }

    pub(super) fn authorize(
        &self,
        window_label: &str,
        session_id: &str,
        capability: HostCapability,
    ) -> Result<(), SessionError> {
        self.authorize_at(window_label, session_id, capability, Instant::now())
    }

    fn authorize_at(
        &self,
        window_label: &str,
        session_id: &str,
        capability: HostCapability,
        now: Instant,
    ) -> Result<(), SessionError> {
        if !ALLOWED_WINDOWS.contains(&window_label) {
            return Err(SessionError::Unauthorized);
        }
        let session_id = decode_session_id(session_id).ok_or(SessionError::Unauthorized)?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let Some(record) = sessions.get(&session_id) else {
            return Err(SessionError::Unauthorized);
        };
        if record.expires_at <= now {
            sessions.remove(&session_id);
            return Err(SessionError::Unauthorized);
        }
        if record.window_label != window_label
            || record.identity != self.identity
            || !record.capabilities.contains(&capability)
        {
            return Err(SessionError::Unauthorized);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SessionError {
    Unauthorized,
    Unavailable,
}

fn encode_session_id(session_id: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(32);
    for byte in session_id {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_session_id(encoded: &str) -> Option<[u8; 16]> {
    if encoded.len() != 32 {
        return None;
    }
    let mut session_id = [0_u8; 16];
    for (index, byte) in session_id.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&encoded[start..start + 2], 16).ok()?;
    }
    Some(session_id)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{HostCapability, HostIdentity, HostSessionManager, SESSION_LIFETIME};

    fn manager() -> HostSessionManager {
        HostSessionManager::new(HostIdentity(vec![1, 2, 3, 4]))
    }

    #[test]
    fn session_is_opaque_and_bound_to_its_native_window() {
        let sessions = manager();
        let ticket = sessions.issue("primary").expect("session issued");
        assert_eq!(ticket.protocol_version, 1);
        assert_eq!(ticket.session_id.len(), 32);
        assert_eq!(ticket.expires_in_seconds, 900);
        assert!(
            sessions
                .authorize("primary", &ticket.session_id, HostCapability::HealthRead)
                .is_ok()
        );
        assert!(
            sessions
                .authorize("primary", &ticket.session_id, HostCapability::TransferWrite)
                .is_ok()
        );
        assert!(
            sessions
                .authorize("primary", &ticket.session_id, HostCapability::ProjectOpen)
                .is_ok()
        );
        assert!(
            sessions
                .authorize("secondary", &ticket.session_id, HostCapability::HealthRead)
                .is_err()
        );
        assert!(
            sessions
                .authorize("external", &ticket.session_id, HostCapability::HealthRead)
                .is_err()
        );
    }

    #[test]
    fn malformed_unknown_and_expired_sessions_fail_closed() {
        let sessions = manager();
        assert!(
            sessions
                .authorize("primary", "../database", HostCapability::HealthRead)
                .is_err()
        );
        assert!(
            sessions
                .authorize("primary", &"00".repeat(16), HostCapability::HealthRead)
                .is_err()
        );

        let now = Instant::now();
        let ticket = sessions.issue_at("primary", now).expect("session issued");
        assert!(
            sessions
                .authorize_at(
                    "primary",
                    &ticket.session_id,
                    HostCapability::HealthRead,
                    now + SESSION_LIFETIME + Duration::from_nanos(1),
                )
                .is_err()
        );
    }

    #[test]
    fn repeated_session_requests_receive_independent_handles() {
        let sessions = manager();
        let first = sessions.issue("primary").expect("first session");
        let second = sessions.issue("primary").expect("second session");
        assert_ne!(first.session_id, second.session_id);
    }

    #[test]
    fn session_table_is_bounded() {
        let sessions = manager();
        for _ in 0..16 {
            sessions.issue("primary").expect("session issued");
        }
        assert!(sessions.issue("primary").is_err());
    }

    #[test]
    fn host_account_maps_to_a_stable_valid_worlddb_principal() {
        use worlddb_core::DomainId;

        let identity = vec![1, 2, 3, 4];
        let first = HostIdentity(identity.clone())
            .project_principal()
            .expect("valid principal");
        let repeated = HostIdentity(vec![1, 2, 3, 4])
            .project_principal()
            .expect("same principal");
        let other = HostIdentity(vec![1, 2, 3, 5])
            .project_principal()
            .expect("different principal");
        assert_eq!(first, repeated);
        assert_ne!(first, other);
        assert_eq!(first.to_bytes()[6] >> 4, 8);
        assert_eq!(first.to_bytes()[8] >> 6, 2);
        assert_eq!(
            first,
            worlddb_ode_engine::derive_host_account_principal(&identity).expect("same mapping")
        );
    }
}
