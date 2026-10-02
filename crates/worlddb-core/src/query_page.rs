//! Bounded page requests and server-side continuation state for productive queries.

use std::num::NonZeroU32;
use std::{error, fmt};

use crate::cursor::{CursorStateStore, QueryHash};
use crate::query_engine::QueryExecutionPath;
use crate::query_ports::OwnedQueryResult;
use crate::security::Capability;

/// One bounded page request. Cursor bytes are opaque to callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageRequest {
    limit: NonZeroU32,
    cursor: Option<Vec<u8>>,
}

impl PageRequest {
    /// Creates a request with a positive result limit.
    pub fn new(limit: u32, cursor: Option<Vec<u8>>) -> Result<Self, PageRequestError> {
        Ok(Self {
            limit: NonZeroU32::new(limit).ok_or(PageRequestError::ZeroLimit)?,
            cursor,
        })
    }

    /// Maximum number of caller-visible rows requested for this page.
    #[must_use]
    pub const fn limit(&self) -> NonZeroU32 {
        self.limit
    }

    /// Opaque continuation from a prior page, if any.
    #[must_use]
    pub fn cursor(&self) -> Option<&[u8]> {
        self.cursor.as_deref()
    }
}

/// Invalid page-request shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageRequestError {
    /// A page must request at least one caller-visible result.
    ZeroLimit,
    /// The request exceeds the configured engine page maximum.
    LimitExceedsEngineMaximum,
}

impl fmt::Display for PageRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroLimit => "page limit must be positive",
            Self::LimitExceedsEngineMaximum => "page limit exceeds the engine maximum",
        })
    }
}

impl error::Error for PageRequestError {}

/// Query operation whose capability must be checked for every page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageOperation {
    RawHistory,
    AdminRawHistory,
    ResolvedView,
    Explain,
    TokenSearch,
    FullTextSearch,
    GraphTraversal,
    Aggregate,
}

impl PageOperation {
    pub(crate) const fn capability(self) -> Capability {
        match self {
            Self::RawHistory | Self::AdminRawHistory => Capability::RawHistoryRead,
            Self::ResolvedView => Capability::QueryResolve,
            Self::Explain => Capability::QueryExplain,
            Self::TokenSearch => Capability::QuerySearch,
            Self::FullTextSearch => Capability::QueryFullText,
            Self::GraphTraversal => Capability::QueryGraphTraverse,
            Self::Aggregate => Capability::QueryAggregate,
        }
    }

    pub(crate) const fn requires_admin_raw(self) -> bool {
        matches!(self, Self::AdminRawHistory)
    }
}

/// Trusted query/session parameters used while producing one page.
pub struct PageExecution<'a> {
    pub(crate) cursors: &'a mut CursorStateStore,
    pub(crate) query_hash: QueryHash,
    pub(crate) operation: PageOperation,
    pub(crate) now_ms: u64,
    pub(crate) first_expires_at_ms: u64,
    pub(crate) max_page_size: NonZeroU32,
    pub(crate) path: QueryExecutionPath,
}

impl<'a> PageExecution<'a> {
    /// Binds session cursor state, canonical query identity, operation, and engine limits.
    pub fn new(
        cursors: &'a mut CursorStateStore,
        query_hash: QueryHash,
        operation: PageOperation,
        now_ms: u64,
        first_expires_at_ms: u64,
        max_page_size: u32,
        path: QueryExecutionPath,
    ) -> Result<Self, PageRequestError> {
        Ok(Self {
            cursors,
            query_hash,
            operation,
            now_ms,
            first_expires_at_ms,
            max_page_size: NonZeroU32::new(max_page_size)
                .ok_or(PageRequestError::LimitExceedsEngineMaximum)?,
            path,
        })
    }
}

/// Complete owned query page. A continuation exists only if a further visible row was found.
#[derive(Clone, Debug)]
pub struct QueryPage<T: 'static> {
    query: OwnedQueryResult<Vec<T>>,
    next_cursor: Option<Vec<u8>>,
    path: QueryExecutionPath,
}

impl<T: 'static> QueryPage<T> {
    pub(crate) const fn new(
        query: OwnedQueryResult<Vec<T>>,
        next_cursor: Option<Vec<u8>>,
        path: QueryExecutionPath,
    ) -> Self {
        Self {
            query,
            next_cursor,
            path,
        }
    }

    /// Complete, owned query result with its snapshot and security binding.
    #[must_use]
    pub const fn query(&self) -> &OwnedQueryResult<Vec<T>> {
        &self.query
    }

    /// Caller-visible rows in this page.
    #[must_use]
    pub fn results(&self) -> &[T] {
        self.query.value()
    }

    /// Opaque continuation, present only when an additional visible row exists.
    #[must_use]
    pub fn next_cursor(&self) -> Option<&[u8]> {
        self.next_cursor.as_deref()
    }

    /// Index or scan path selected by the trusted query adapter.
    #[must_use]
    pub const fn path(&self) -> QueryExecutionPath {
        self.path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageCursorState {
    pub(crate) limit: u32,
    pub(crate) expires_at_ms: u64,
    pub(crate) candidates_seen: u64,
    pub(crate) work_units_seen: u64,
    pub(crate) results_sent: u64,
    pub(crate) after_sort_key: Vec<u8>,
}

impl PageCursorState {
    const VERSION: u8 = 1;
    const FIXED_BYTES: usize = 1 + 4 + 8 + 8 + 8 + 8 + 4;

    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let key_len = u32::try_from(self.after_sort_key.len()).ok()?;
        let mut bytes = Vec::with_capacity(Self::FIXED_BYTES + self.after_sort_key.len());
        bytes.push(Self::VERSION);
        bytes.extend_from_slice(&self.limit.to_be_bytes());
        bytes.extend_from_slice(&self.expires_at_ms.to_be_bytes());
        bytes.extend_from_slice(&self.candidates_seen.to_be_bytes());
        bytes.extend_from_slice(&self.work_units_seen.to_be_bytes());
        bytes.extend_from_slice(&self.results_sent.to_be_bytes());
        bytes.extend_from_slice(&key_len.to_be_bytes());
        bytes.extend_from_slice(&self.after_sort_key);
        Some(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::FIXED_BYTES || bytes.first().copied() != Some(Self::VERSION) {
            return None;
        }
        let limit = u32::from_be_bytes(bytes.get(1..5)?.try_into().ok()?);
        if limit == 0 {
            return None;
        }
        let expires_at_ms = u64::from_be_bytes(bytes.get(5..13)?.try_into().ok()?);
        let candidates_seen = u64::from_be_bytes(bytes.get(13..21)?.try_into().ok()?);
        let work_units_seen = u64::from_be_bytes(bytes.get(21..29)?.try_into().ok()?);
        let results_sent = u64::from_be_bytes(bytes.get(29..37)?.try_into().ok()?);
        let key_len =
            usize::try_from(u32::from_be_bytes(bytes.get(37..41)?.try_into().ok()?)).ok()?;
        if bytes.len() != Self::FIXED_BYTES.checked_add(key_len)? {
            return None;
        }
        Some(Self {
            limit,
            expires_at_ms,
            candidates_seen,
            work_units_seen,
            results_sent,
            after_sort_key: bytes.get(Self::FIXED_BYTES..)?.to_vec(),
        })
    }
}
