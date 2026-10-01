//! Opaque, session-local query cursor handles and bounded state storage.

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;

use crate::ids::{PrincipalId, SecurityEpoch};
use crate::record_refs::SnapshotRef;
use crate::security::{PolicyTarget, SecurityPolicyView};

const CURSOR_FORMAT_VERSION: u8 = 1;
const RANDOM_HANDLE_BYTES: usize = 32;
const MAC_BYTES: usize = 32;

/// Limits for the session-local cursor state store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorStoreLimits {
    max_entries: NonZeroUsize,
    max_state_bytes: NonZeroUsize,
    max_ttl_ms: u64,
}

impl CursorStoreLimits {
    /// Creates positive entry/byte limits and a positive maximum cursor lifetime.
    pub fn new(
        max_entries: usize,
        max_state_bytes: usize,
        max_ttl_ms: u64,
    ) -> Result<Self, CursorStateError> {
        Ok(Self {
            max_entries: NonZeroUsize::new(max_entries).ok_or(CursorStateError::ZeroEntryLimit)?,
            max_state_bytes: NonZeroUsize::new(max_state_bytes)
                .ok_or(CursorStateError::ZeroByteLimit)?,
            max_ttl_ms: if max_ttl_ms == 0 {
                return Err(CursorStateError::ZeroTtlLimit);
            } else {
                max_ttl_ms
            },
        })
    }
}

/// Public-safe wire handle. It contains only a format byte, expiry, random nonce, and MAC.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CursorToken {
    expires_at_ms: u64,
    handle: [u8; RANDOM_HANDLE_BYTES],
    mac: [u8; MAC_BYTES],
}

/// Canonical 256-bit query identity retained only in server-side cursor state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QueryHash([u8; 32]);

impl QueryHash {
    /// Wraps the canonical query hash bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the fixed hash bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Trusted current continuation context supplied by the engine before a page is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorSecurityContext {
    principal_id: PrincipalId,
    current_security_epoch: SecurityEpoch,
    evaluated_security_epoch: SecurityEpoch,
    target: PolicyTarget,
}

impl CursorSecurityContext {
    /// Binds the authenticated principal, current and query-evaluated epochs, and policy target.
    #[must_use]
    pub const fn new(
        principal_id: PrincipalId,
        current_security_epoch: SecurityEpoch,
        evaluated_security_epoch: SecurityEpoch,
        target: PolicyTarget,
    ) -> Self {
        Self {
            principal_id,
            current_security_epoch,
            evaluated_security_epoch,
            target,
        }
    }
}

/// Query identity, opaque state, and lifetime used to create one continuation cursor.
pub struct CursorInsertRequest {
    snapshot: SnapshotRef,
    query_hash: QueryHash,
    state: Vec<u8>,
    now_ms: u64,
    expires_at_ms: u64,
}

impl CursorInsertRequest {
    /// Groups the snapshot/query binding with private continuation state and expiry.
    #[must_use]
    pub const fn new(
        snapshot: SnapshotRef,
        query_hash: QueryHash,
        state: Vec<u8>,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Self {
        Self {
            snapshot,
            query_hash,
            state,
            now_ms,
            expires_at_ms,
        }
    }
}

impl CursorToken {
    /// Encodes the fixed-size public-safe wire token.
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        let mut wire = Vec::with_capacity(1 + 8 + RANDOM_HANDLE_BYTES + MAC_BYTES);
        wire.push(CURSOR_FORMAT_VERSION);
        wire.extend_from_slice(&self.expires_at_ms.to_be_bytes());
        wire.extend_from_slice(&self.handle);
        wire.extend_from_slice(&self.mac);
        wire
    }

    /// Parses a token. Every malformed shape/version shares the same invalidated result.
    pub fn decode(wire: &[u8]) -> Result<Self, CursorStateError> {
        if wire.len() != 1 + 8 + RANDOM_HANDLE_BYTES + MAC_BYTES
            || wire.first().copied() != Some(CURSOR_FORMAT_VERSION)
        {
            return Err(CursorStateError::CursorInvalidated);
        }
        let expiry_bytes = wire.get(1..9).ok_or(CursorStateError::CursorInvalidated)?;
        let handle_bytes = wire.get(9..41).ok_or(CursorStateError::CursorInvalidated)?;
        let mac_bytes = wire
            .get(41..73)
            .ok_or(CursorStateError::CursorInvalidated)?;
        let expires_at_ms = u64::from_be_bytes(
            expiry_bytes
                .try_into()
                .map_err(|_| CursorStateError::CursorInvalidated)?,
        );
        let mut handle = [0_u8; RANDOM_HANDLE_BYTES];
        handle.copy_from_slice(handle_bytes);
        let mut mac = [0_u8; MAC_BYTES];
        mac.copy_from_slice(mac_bytes);
        Ok(Self {
            expires_at_ms,
            handle,
            mac,
        })
    }

    /// Non-sensitive expiry carried in the wire format.
    #[must_use]
    pub const fn expires_at_ms(self) -> u64 {
        self.expires_at_ms
    }
}

struct CursorEntry {
    expires_at_ms: u64,
    snapshot: SnapshotRef,
    query_hash: QueryHash,
    authorization: Option<CursorAuthorizationBinding>,
    state: Vec<u8>,
}

#[derive(Clone, Copy)]
struct CursorAuthorizationBinding {
    principal_id: PrincipalId,
    capability_fingerprint: [u8; 32],
    current_security_epoch: SecurityEpoch,
    evaluated_security_epoch: SecurityEpoch,
    target: PolicyTarget,
}

/// Bounded in-memory cursor store. A new instance has a fresh OS-random MAC key,
/// so cursors from an old process/session cannot resolve after restart.
pub struct CursorStateStore {
    mac_key: [u8; MAC_BYTES],
    limits: CursorStoreLimits,
    entries: HashMap<[u8; RANDOM_HANDLE_BYTES], CursorEntry>,
    state_bytes: usize,
}

impl CursorStateStore {
    /// Starts a new engine-session store with fresh cryptographic key material.
    pub fn new(limits: CursorStoreLimits) -> Result<Self, CursorStateError> {
        let mut mac_key = [0_u8; MAC_BYTES];
        getrandom::fill(&mut mac_key).map_err(|_| CursorStateError::EntropyUnavailable)?;
        Ok(Self {
            mac_key,
            limits,
            entries: HashMap::new(),
            state_bytes: 0,
        })
    }

    /// Inserts bounded opaque session state and returns a random authenticated handle.
    pub(crate) fn insert(
        &mut self,
        snapshot: SnapshotRef,
        query_hash: QueryHash,
        state: Vec<u8>,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<CursorToken, CursorStateError> {
        self.discard_expired(now_ms);
        let ttl = expires_at_ms
            .checked_sub(now_ms)
            .filter(|ttl| *ttl > 0)
            .ok_or(CursorStateError::InvalidExpiry)?;
        if ttl > self.limits.max_ttl_ms {
            return Err(CursorStateError::TtlExceedsLimit);
        }
        if state.len() > self.limits.max_state_bytes.get() {
            return Err(CursorStateError::StateTooLarge);
        }
        let next_bytes = self
            .state_bytes
            .checked_add(state.len())
            .ok_or(CursorStateError::CapacityExceeded)?;
        if self.entries.len() >= self.limits.max_entries.get()
            || next_bytes > self.limits.max_state_bytes.get()
        {
            return Err(CursorStateError::CapacityExceeded);
        }

        let handle = self.unique_random_handle()?;
        let mac = cursor_mac(&self.mac_key, expires_at_ms, &handle);
        self.entries.insert(
            handle,
            CursorEntry {
                expires_at_ms,
                snapshot,
                query_hash,
                authorization: None,
                state,
            },
        );
        self.state_bytes = next_bytes;
        Ok(CursorToken {
            expires_at_ms,
            handle,
            mac,
        })
    }

    /// Inserts a cursor bound to the authenticated principal, all effective capabilities,
    /// current/evaluated security epochs, and the policy target used by the query.
    /// The supplied policy is the current AuthorizationNow projection.
    pub fn insert_authorized(
        &mut self,
        request: CursorInsertRequest,
        security: CursorSecurityContext,
        policy_view: SecurityPolicyView<'_>,
    ) -> Result<CursorToken, CursorStateError> {
        if security.current_security_epoch != policy_view.current_epoch()
            || security.evaluated_security_epoch != policy_view.evaluated_epoch()
            || security.principal_id != policy_view.principal_id()
        {
            return Err(CursorStateError::CursorInvalidated);
        }
        let token = self.insert(
            request.snapshot,
            request.query_hash,
            request.state,
            request.now_ms,
            request.expires_at_ms,
        )?;
        let entry = self
            .entries
            .get_mut(&token.handle)
            .ok_or(CursorStateError::CursorInvalidated)?;
        entry.authorization = Some(CursorAuthorizationBinding {
            principal_id: security.principal_id,
            capability_fingerprint: policy_view
                .current_snapshot()
                .effective_capability_fingerprint(security.principal_id, security.target),
            current_security_epoch: security.current_security_epoch,
            evaluated_security_epoch: security.evaluated_security_epoch,
            target: security.target,
        });
        Ok(token)
    }

    /// Resolves only after re-evaluating the principal's full effective capability set
    /// against the current AuthorizationNow policy and matching both security epochs.
    /// Unknown, expired, tampered, stale-policy, principal-mismatched, and unauthorized
    /// continuations all produce the same `CursorInvalidated` result.
    pub fn resolve_authorized(
        &mut self,
        wire: &[u8],
        now_ms: u64,
        snapshot: SnapshotRef,
        query_hash: QueryHash,
        security: CursorSecurityContext,
        policy_view: SecurityPolicyView<'_>,
    ) -> Result<&[u8], CursorStateError> {
        let token = CursorToken::decode(wire)?;
        let binding = self
            .entries
            .get(&token.handle)
            .and_then(|entry| entry.authorization)
            .ok_or(CursorStateError::CursorInvalidated)?;
        if binding.principal_id != security.principal_id
            || binding.current_security_epoch != security.current_security_epoch
            || binding.evaluated_security_epoch != security.evaluated_security_epoch
            || binding.current_security_epoch != policy_view.current_epoch()
            || binding.evaluated_security_epoch != policy_view.evaluated_epoch()
            || security.principal_id != policy_view.principal_id()
            || binding.target != security.target
            || binding.capability_fingerprint
                != policy_view
                    .current_snapshot()
                    .effective_capability_fingerprint(security.principal_id, security.target)
        {
            return Err(CursorStateError::CursorInvalidated);
        }
        self.resolve_inner(wire, now_ms, snapshot, query_hash, true)
    }

    /// Resolves state or returns the same invalidation outcome for unknown, expired,
    /// malformed, manipulated, or old-session tokens.
    fn resolve_inner(
        &mut self,
        wire: &[u8],
        now_ms: u64,
        snapshot: SnapshotRef,
        query_hash: QueryHash,
        require_authorization: bool,
    ) -> Result<&[u8], CursorStateError> {
        let token = CursorToken::decode(wire)?;
        let expected_mac = cursor_mac(&self.mac_key, token.expires_at_ms, &token.handle);
        if !constant_time_equal(&token.mac, &expected_mac) {
            return Err(CursorStateError::CursorInvalidated);
        }
        if token.expires_at_ms <= now_ms {
            self.remove_entry(&token.handle);
            return Err(CursorStateError::CursorInvalidated);
        }
        let Some(entry_metadata) = self.entries.get(&token.handle).map(|entry| {
            (
                entry.expires_at_ms,
                entry.snapshot,
                entry.query_hash,
                entry.authorization.is_some(),
            )
        }) else {
            return Err(CursorStateError::CursorInvalidated);
        };
        if entry_metadata
            != (
                token.expires_at_ms,
                snapshot,
                query_hash,
                require_authorization,
            )
        {
            return Err(CursorStateError::CursorInvalidated);
        }
        self.entries
            .get(&token.handle)
            .map(|entry| entry.state.as_slice())
            .ok_or(CursorStateError::CursorInvalidated)
    }

    /// Number of retained live entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no cursor state is retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of bytes currently retained in opaque state payloads.
    #[must_use]
    pub const fn state_bytes(&self) -> usize {
        self.state_bytes
    }

    fn unique_random_handle(&self) -> Result<[u8; RANDOM_HANDLE_BYTES], CursorStateError> {
        for _ in 0..8 {
            let mut handle = [0_u8; RANDOM_HANDLE_BYTES];
            getrandom::fill(&mut handle).map_err(|_| CursorStateError::EntropyUnavailable)?;
            if !self.entries.contains_key(&handle) {
                return Ok(handle);
            }
        }
        Err(CursorStateError::RandomHandleCollision)
    }

    fn discard_expired(&mut self, now_ms: u64) {
        let expired = self
            .entries
            .iter()
            .filter_map(|(handle, entry)| (entry.expires_at_ms <= now_ms).then_some(*handle))
            .collect::<Vec<_>>();
        for handle in expired {
            self.remove_entry(&handle);
        }
    }

    fn remove_entry(&mut self, handle: &[u8; RANDOM_HANDLE_BYTES]) {
        if let Some(entry) = self.entries.remove(handle) {
            self.state_bytes = self.state_bytes.saturating_sub(entry.state.len());
        }
    }
}

fn cursor_mac(
    key: &[u8; MAC_BYTES],
    expires_at_ms: u64,
    handle: &[u8; RANDOM_HANDLE_BYTES],
) -> [u8; MAC_BYTES] {
    let mut message = Vec::with_capacity(1 + 8 + RANDOM_HANDLE_BYTES);
    message.push(CURSOR_FORMAT_VERSION);
    message.extend_from_slice(&expires_at_ms.to_be_bytes());
    message.extend_from_slice(handle);
    *blake3::keyed_hash(key, &message).as_bytes()
}

fn constant_time_equal(left: &[u8; MAC_BYTES], right: &[u8; MAC_BYTES]) -> bool {
    let difference = left
        .iter()
        .zip(right.iter())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        });
    difference == 0
}

/// Cursor token, store capacity, entropy, or lookup failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CursorStateError {
    /// All invalid/expired/manipulated/unknown/session-mismatched handles share this result.
    CursorInvalidated,
    /// The configured live-entry limit is zero.
    ZeroEntryLimit,
    /// The configured total state-byte limit is zero.
    ZeroByteLimit,
    /// The configured maximum token lifetime is zero.
    ZeroTtlLimit,
    /// Requested expiry is not after the current time.
    InvalidExpiry,
    /// Requested expiry exceeds the engine-configured maximum lifetime.
    TtlExceedsLimit,
    /// State exceeds the configured byte limit.
    StateTooLarge,
    /// Entry count or aggregate state bytes would exceed configured capacity.
    CapacityExceeded,
    /// The operating system did not provide cryptographic entropy.
    EntropyUnavailable,
    /// Random-handle collisions exceeded the bounded retry count.
    RandomHandleCollision,
}

impl fmt::Display for CursorStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CursorInvalidated => "cursor is invalidated",
            Self::ZeroEntryLimit => "cursor entry limit must be positive",
            Self::ZeroByteLimit => "cursor state-byte limit must be positive",
            Self::ZeroTtlLimit => "cursor lifetime limit must be positive",
            Self::InvalidExpiry => "cursor expiry must be in the future",
            Self::TtlExceedsLimit => "cursor expiry exceeds the configured lifetime",
            Self::StateTooLarge => "cursor state exceeds the configured byte limit",
            Self::CapacityExceeded => "cursor state store capacity is exhausted",
            Self::EntropyUnavailable => "cryptographic entropy is unavailable",
            Self::RandomHandleCollision => "random cursor handle collision limit reached",
        })
    }
}

impl std::error::Error for CursorStateError {}

#[cfg(test)]
mod tests {
    use super::{
        CursorInsertRequest, CursorSecurityContext, CursorStateError, CursorStateStore,
        CursorStoreLimits, CursorToken, QueryHash,
    };
    use crate::ids::{DomainId, PolicyRuleId, PrincipalId, SecurityEpoch};
    use crate::record_refs::SnapshotRef;

    fn limits(
        entries: usize,
        bytes: usize,
        ttl: u64,
    ) -> Result<CursorStoreLimits, CursorStateError> {
        CursorStoreLimits::new(entries, bytes, ttl)
    }

    #[test]
    fn wire_handle_hides_payload_and_tampering_has_uniform_invalidation()
    -> Result<(), CursorStateError> {
        let secret = b"private sort key and record identity";
        let mut store = CursorStateStore::new(limits(4, 256, 10_000)?)?;
        let snapshot = test_snapshot()?;
        let query_hash = QueryHash::new([7; 32]);
        let token = store.insert(snapshot, query_hash, secret.to_vec(), 1_000, 5_000)?;
        let wire = token.encode();
        assert_eq!(wire.len(), 73);
        assert!(!wire.windows(secret.len()).any(|window| window == secret));
        assert_eq!(CursorToken::decode(&wire), Ok(token));

        for position in [0_usize, 8, 9, 41] {
            let mut tampered = wire.clone();
            let byte = tampered
                .get_mut(position)
                .ok_or(CursorStateError::CursorInvalidated)?;
            *byte ^= 1;
            assert_eq!(
                store.resolve_inner(&tampered, 2_000, snapshot, query_hash, false),
                Err(CursorStateError::CursorInvalidated)
            );
        }
        assert_eq!(
            store.resolve_inner(&wire, 2_000, snapshot, query_hash, false),
            Ok(secret.as_slice())
        );
        assert_eq!(
            store.resolve_inner(&wire, 2_000, test_snapshot_with_tail(2)?, query_hash, false),
            Err(CursorStateError::CursorInvalidated)
        );
        assert_eq!(
            store.resolve_inner(&wire, 2_000, snapshot, QueryHash::new([8; 32]), false),
            Err(CursorStateError::CursorInvalidated)
        );
        let truncated_wire = wire
            .get(..wire.len().saturating_sub(1))
            .ok_or(CursorStateError::CursorInvalidated)?;
        assert_eq!(
            CursorToken::decode(truncated_wire),
            Err(CursorStateError::CursorInvalidated)
        );
        Ok(())
    }

    #[test]
    fn expiry_unknown_and_restart_share_cursor_invalidated() -> Result<(), CursorStateError> {
        let mut first_session = CursorStateStore::new(limits(2, 64, 5_000)?)?;
        let snapshot = test_snapshot()?;
        let query_hash = QueryHash::new([9; 32]);
        let token = first_session.insert(snapshot, query_hash, vec![1, 2, 3], 10, 100)?;
        let wire = token.encode();
        let mut restarted_session = CursorStateStore::new(limits(2, 64, 5_000)?)?;
        assert_eq!(
            restarted_session.resolve_inner(&wire, 11, snapshot, query_hash, false),
            Err(CursorStateError::CursorInvalidated)
        );
        assert_eq!(
            first_session.resolve_inner(&wire, 100, snapshot, query_hash, false),
            Err(CursorStateError::CursorInvalidated)
        );
        assert!(first_session.is_empty());
        assert_eq!(
            first_session.resolve_inner(&wire, 101, snapshot, query_hash, false),
            Err(CursorStateError::CursorInvalidated)
        );
        Ok(())
    }

    #[test]
    fn state_store_enforces_entry_byte_and_lifetime_limits() -> Result<(), CursorStateError> {
        let mut store = CursorStateStore::new(limits(1, 5, 100)?)?;
        let snapshot = test_snapshot()?;
        let query_hash = QueryHash::new([3; 32]);
        store.insert(snapshot, query_hash, vec![1, 2, 3, 4, 5], 0, 100)?;
        assert_eq!(store.state_bytes(), 5);
        assert_eq!(
            store.insert(snapshot, query_hash, vec![1], 0, 50),
            Err(CursorStateError::CapacityExceeded)
        );
        assert_eq!(
            store.insert(snapshot, query_hash, vec![1, 2, 3, 4, 5, 6], 0, 50),
            Err(CursorStateError::StateTooLarge)
        );
        assert_eq!(
            store.insert(snapshot, query_hash, vec![1], 0, 101),
            Err(CursorStateError::TtlExceedsLimit)
        );
        assert_eq!(
            store.insert(snapshot, query_hash, vec![1], 10, 10),
            Err(CursorStateError::InvalidExpiry)
        );
        assert!(CursorStoreLimits::new(0, 1, 1).is_err());
        assert!(CursorStoreLimits::new(1, 0, 1).is_err());
        assert!(CursorStoreLimits::new(1, 1, 0).is_err());
        Ok(())
    }

    #[test]
    fn authorized_cursor_rechecks_principal_capabilities_and_epochs() -> Result<(), CursorStateError>
    {
        use crate::security::{Capability, PolicyTarget};

        let principal = test_principal(20)?;
        let other_principal = test_principal(21)?;
        let snapshot = test_snapshot()?;
        let target = PolicyTarget::default();
        let security = |principal_id, current, evaluated| {
            super::CursorSecurityContext::new(
                principal_id,
                SecurityEpoch::new(current),
                SecurityEpoch::new(evaluated),
                target,
            )
        };
        let initial_snapshot = policy_snapshot(
            principal,
            &[Capability::QueryResolve, Capability::ProjectRead],
        )?;
        let initial_history = policy_history(
            initial_snapshot.clone(),
            initial_snapshot.clone(),
            SecurityEpoch::INITIAL,
        )?;
        let initial_view = policy_view(&initial_history, principal)?;
        let mut store = CursorStateStore::new(limits(4, 256, 10_000)?)?;
        let query_hash = QueryHash::new([22; 32]);
        let token = store.insert_authorized(
            super::CursorInsertRequest::new(snapshot, query_hash, vec![4, 5, 6], 1_000, 5_000),
            security(principal, 0, 0),
            initial_view,
        )?;
        let wire = token.encode();

        assert_eq!(
            store.resolve_inner(&wire, 2_000, snapshot, query_hash, false),
            Err(CursorStateError::CursorInvalidated),
            "legacy resolution must not bypass cursor reauthorization"
        );
        assert_eq!(
            store.resolve_authorized(
                &wire,
                2_000,
                snapshot,
                query_hash,
                security(principal, 0, 0),
                initial_view,
            ),
            Ok([4, 5, 6].as_slice())
        );

        let expanded_policy = policy_snapshot(
            principal,
            &[
                Capability::QueryResolve,
                Capability::ProjectRead,
                Capability::QuerySearch,
            ],
        )?;
        let expanded_history = policy_history(
            initial_snapshot.clone(),
            expanded_policy,
            SecurityEpoch::INITIAL,
        )?;
        let expanded_view = policy_view(&expanded_history, principal)?;
        assert_eq!(
            store.resolve_authorized(
                &wire,
                2_000,
                snapshot,
                query_hash,
                security(principal, 0, 0),
                expanded_view,
            ),
            Err(CursorStateError::CursorInvalidated),
            "effective capability changes invalidate even if the supplied epoch was not advanced"
        );
        let next_epoch_history = policy_history(
            initial_snapshot.clone(),
            initial_snapshot.clone(),
            SecurityEpoch::new(1),
        )?;
        let next_epoch_view = policy_view(&next_epoch_history, principal)?;
        assert_eq!(
            store.resolve_authorized(
                &wire,
                2_000,
                snapshot,
                query_hash,
                security(principal, 1, 1),
                next_epoch_view,
            ),
            Err(CursorStateError::CursorInvalidated)
        );
        assert_eq!(
            store.resolve_authorized(
                &wire,
                2_000,
                snapshot,
                query_hash,
                security(principal, 0, 1),
                initial_view,
            ),
            Err(CursorStateError::CursorInvalidated)
        );
        assert_eq!(
            store.resolve_authorized(
                &wire,
                2_000,
                snapshot,
                query_hash,
                security(other_principal, 0, 0),
                initial_view,
            ),
            Err(CursorStateError::CursorInvalidated)
        );

        let revoked_policy = policy_snapshot(principal, &[Capability::ProjectRead])?;
        let revoked_history =
            policy_history(initial_snapshot, revoked_policy, SecurityEpoch::INITIAL)?;
        let revoked_view = policy_view(&revoked_history, principal)?;
        assert_eq!(
            store.resolve_authorized(
                &wire,
                2_000,
                snapshot,
                query_hash,
                security(principal, 0, 0),
                revoked_view,
            ),
            Err(CursorStateError::CursorInvalidated)
        );
        Ok(())
    }

    #[test]
    fn historical_query_cursor_pins_evaluated_epoch_and_rechecks_now()
    -> Result<(), CursorStateError> {
        use crate::security::Capability;

        let principal = test_principal(30)?;
        let snapshot = test_snapshot()?;
        let initial = policy_snapshot(
            principal,
            &[Capability::QueryResolve, Capability::ProjectRead],
        )?;
        let current = policy_snapshot(
            principal,
            &[
                Capability::QueryResolve,
                Capability::ProjectRead,
                Capability::SecurityPermissionHistoryRead,
            ],
        )?;
        let history = policy_history(initial, current, SecurityEpoch::new(1))?;
        let historical_view = history
            .select(
                crate::query_context::AuthorizationMode::AtRevision(crate::ids::Revision::GENESIS),
                principal,
                crate::ids::Revision::new(1).map_err(|_| CursorStateError::CursorInvalidated)?,
            )
            .map_err(|_| CursorStateError::CursorInvalidated)?;
        assert_eq!(historical_view.current_epoch(), SecurityEpoch::new(1));
        assert_eq!(historical_view.evaluated_epoch(), SecurityEpoch::INITIAL);

        let target = crate::security::PolicyTarget::default();
        let security = CursorSecurityContext::new(
            principal,
            SecurityEpoch::new(1),
            SecurityEpoch::INITIAL,
            target,
        );
        let query_hash = QueryHash::new([31; 32]);
        let mut store = CursorStateStore::new(limits(2, 64, 100)?)?;
        let token = store.insert_authorized(
            CursorInsertRequest::new(snapshot, query_hash, vec![9], 10, 90),
            security,
            historical_view,
        )?;
        assert_eq!(
            store.resolve_authorized(
                &token.encode(),
                20,
                snapshot,
                query_hash,
                security,
                historical_view,
            ),
            Ok([9].as_slice())
        );

        let now_view = history
            .select(
                crate::query_context::AuthorizationMode::Now,
                principal,
                crate::ids::Revision::new(1).map_err(|_| CursorStateError::CursorInvalidated)?,
            )
            .map_err(|_| CursorStateError::CursorInvalidated)?;
        assert_eq!(
            store.resolve_authorized(
                &token.encode(),
                20,
                snapshot,
                query_hash,
                CursorSecurityContext::new(
                    principal,
                    SecurityEpoch::new(1),
                    SecurityEpoch::new(1),
                    target,
                ),
                now_view,
            ),
            Err(CursorStateError::CursorInvalidated),
            "switching a continuation to a different security time basis is invalid"
        );
        Ok(())
    }

    fn policy_snapshot(
        principal_id: PrincipalId,
        capabilities: &[crate::security::Capability],
    ) -> Result<crate::security::SecurityPolicySnapshot, CursorStateError> {
        let mut rules = Vec::with_capacity(capabilities.len());
        for (index, capability) in capabilities.iter().enumerate() {
            let tail = u8::try_from(index + 40).map_err(|_| CursorStateError::CursorInvalidated)?;
            let id = PolicyRuleId::try_from_bytes(test_uuid_bytes(tail))
                .map_err(|_| CursorStateError::CursorInvalidated)?;
            rules.push(crate::security::CapabilityRule::new(
                id,
                crate::security::PolicySubject::Principal(principal_id),
                crate::security::CapabilityGrant::new(
                    *capability,
                    crate::security::GrantEffect::Allow,
                ),
                crate::security::PolicyScope::project(),
            ));
        }
        crate::security::SecurityPolicySnapshot::new(
            vec![crate::security::Principal::new(principal_id)],
            Vec::new(),
            Vec::new(),
            rules,
        )
        .map_err(|_| CursorStateError::CursorInvalidated)
    }

    fn policy_history(
        initial: crate::security::SecurityPolicySnapshot,
        current: crate::security::SecurityPolicySnapshot,
        current_epoch: SecurityEpoch,
    ) -> Result<crate::security::SecurityPolicyHistory, CursorStateError> {
        let revision =
            crate::ids::Revision::new(1).map_err(|_| CursorStateError::CursorInvalidated)?;
        crate::security::SecurityPolicyHistory::new(
            revision,
            vec![
                crate::security::SecurityPolicyVersion::new(
                    crate::ids::Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    initial,
                ),
                crate::security::SecurityPolicyVersion::new(revision, current_epoch, current),
            ],
        )
        .map_err(|_| CursorStateError::CursorInvalidated)
    }

    fn policy_view(
        history: &crate::security::SecurityPolicyHistory,
        principal_id: PrincipalId,
    ) -> Result<crate::security::SecurityPolicyView<'_>, CursorStateError> {
        history
            .select(
                crate::query_context::AuthorizationMode::Now,
                principal_id,
                crate::ids::Revision::new(1).map_err(|_| CursorStateError::CursorInvalidated)?,
            )
            .map_err(|_| CursorStateError::CursorInvalidated)
    }

    fn test_principal(tail: u8) -> Result<PrincipalId, CursorStateError> {
        PrincipalId::try_from_bytes(test_uuid_bytes(tail))
            .map_err(|_| CursorStateError::CursorInvalidated)
    }

    fn test_uuid_bytes(tail: u8) -> [u8; 16] {
        [1, 0, 0, 0, 0, 0, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, tail]
    }

    fn test_snapshot() -> Result<SnapshotRef, CursorStateError> {
        test_snapshot_with_tail(1)
    }

    fn test_snapshot_with_tail(tail: u8) -> Result<SnapshotRef, CursorStateError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        crate::ids::DomainId::try_from_bytes(bytes)
            .map(SnapshotRef::new)
            .map_err(|_| CursorStateError::CursorInvalidated)
    }
}
