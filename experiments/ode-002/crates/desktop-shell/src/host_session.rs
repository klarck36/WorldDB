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
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct HostIdentity(Vec<u8>);

impl HostIdentity {
    pub(super) fn current() -> Result<Self, HostIdentityError> {
        #[cfg(windows)]
        {
            process_user_sid().map(Self)
        }
        #[cfg(not(windows))]
        {
            Err(HostIdentityError::UnsupportedPlatform)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HostIdentityError {
    #[cfg(not(windows))]
    UnsupportedPlatform,
    #[cfg(windows)]
    OperatingSystemFailure,
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
    capabilities: [HostCapability; 2],
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
                    capabilities: [HostCapability::HealthRead, HostCapability::TransferWrite],
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

#[cfg(windows)]
fn process_user_sid() -> Result<Vec<u8>, HostIdentityError> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Token(HANDLE);

    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let mut token_handle: HANDLE = ptr::null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) };
    if opened == 0 || token_handle.is_null() {
        return Err(HostIdentityError::OperatingSystemFailure);
    }
    let token = Token(token_handle);

    let mut required_bytes = 0_u32;
    unsafe {
        let _ = GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required_bytes);
    }
    if required_bytes < size_of::<TOKEN_USER>() as u32 || required_bytes > 4096 {
        return Err(HostIdentityError::OperatingSystemFailure);
    }
    let word_count = (required_bytes as usize).div_ceil(size_of::<usize>());
    let mut aligned_buffer = vec![0_usize; word_count];
    let buffer = aligned_buffer.as_mut_ptr().cast::<c_void>();
    let read_token = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer,
            required_bytes,
            &mut required_bytes,
        )
    };
    if read_token == 0 {
        return Err(HostIdentityError::OperatingSystemFailure);
    }

    let token_user = unsafe { &*buffer.cast::<TOKEN_USER>() };
    if token_user.User.Sid.is_null() {
        return Err(HostIdentityError::OperatingSystemFailure);
    }
    let sid_length = unsafe { GetLengthSid(token_user.User.Sid) } as usize;
    if sid_length == 0 || sid_length > 1024 {
        return Err(HostIdentityError::OperatingSystemFailure);
    }
    let sid = unsafe { std::slice::from_raw_parts(token_user.User.Sid.cast::<u8>(), sid_length) };
    Ok(sid.to_vec())
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
}
