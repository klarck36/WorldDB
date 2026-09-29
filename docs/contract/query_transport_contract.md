# WorldDB 1.0 supplement – Query and transport DTOs

**Status:** accepted working contract for M0-04d

**Decision record:** [ADR-034](ADR-034-query-transport.md)

**Scope:** completes Master §§12, 16, 17, 31.5, and the desktop IPC boundary for query filters, sorting, token search, page DTOs, transport envelopes, cancellation, and protocol versioning. It adds no persisted First-Class type or WDB invariant ID and predates the M0-02a closure of all 52 HARD source gaps recorded in ADR-039; the two GUARDED gaps remain open.

## 1. One semantic request across adapters

The public query boundary is one owned, typed `QueryRequest`. The stable Rust facade accepts that value directly. CLI parsing and desktop IPC decoding construct the same value and map the same typed result and error enums; adapters never implement filters, sorting, resolution, authorization, defaults, or pagination themselves. No query string, SQL fragment, arbitrary JSON value, callback, or renderer-provided authorization decision is accepted.

The selected Database is bound to the trusted engine session. A request can select a typed snapshot but cannot provide a filesystem path, choose a `PrincipalId`, supply Capabilities, or override the authenticated host identity. The host binds the authenticated Principal and cancellation token to the request before it reaches `QueryContext`. Rust callers use the same trusted-session boundary.

```text
QueryRequest {
  context: QueryContextDto,
  operation: QueryOperation,
  filter: FilterExpr,
  projection: Projection,
  sort: List<SortTerm>,
  page: PageRequest
}

QueryContextDto {
  snapshot: SnapshotSelector,
  recorded_as_of: Revision,
  history_space: HistorySpaceId,
  layers: LayerSelection,
  world_time: WorldTimeSelector,
  perspective: PerspectiveScope,
  epistemic_mode: EpistemicMode,
  schema_mode: SchemaMode,
  authorization_mode: AuthorizationMode,
  budget: QueryBudget
}

SnapshotSelector = Current | AtRevision(Revision)
AuthorizationMode = AuthorizationNow | AuthorizationAtRevision(Revision)
QueryOperation = RawHistory | ResolvedView | Explain | TokenSearch(SearchSpec)
                | FullTextSearch(FullTextSpec) | GraphTraversal(GraphSpec)
                | Aggregate(AggregateSpec)

ExistingRelationshipKind = the closed typed relationship-kind variants in Master §§31.1–31.2

Projection = OperationDefault | Only(NonEmptySet<FieldSelector>)

GraphSpec {
  roots: NonEmptySet<RecordRef>,
  relationships: NonEmptySet<ExistingRelationshipKind>,
  direction: Outgoing | Incoming | Both,
  max_depth: UInt16,
  max_nodes: UInt64,
  max_edges: UInt64,
  cycle_policy: StopAtRepeatedNode | EmitRepeatedEdgeAndStop
}

AggregateSpec = Count | Exists
              | GroupedCount { group_by: NonEmptySet<FieldSelector> }

FullTextSpec { fields: NonEmptySet<FieldSelector>, expression: FullTextExpr }
FullTextExpr = Token(SearchToken) | All(NonEmptyList<FullTextExpr>)
             | Any(NonEmptyList<FullTextExpr>) | Not(FullTextExpr)
```

Every field above is required in the canonical request. A client may initialize its form from the Master-defined `SchemaMode::Historical` default, but the value is explicit in the DTO and every adapter maps it through the same constructor. `Current` is resolved once to a concrete `SnapshotId` at request start. The resulting snapshot pins the data and schema revisions; `recorded_as_of` must not exceed it. A cursor page retains that same concrete snapshot. `LayerSelection` is validated against the pinned schema and cannot be widened afterward. Perspective and `EpistemicMode` must be a valid Master-defined pair; perspective-free `WorldState` requests use the Master exception.

All query budgets are finite and explicit: `QueryBudget { max_candidates: UInt64, max_work_units: UInt64, max_results: UInt64 }`. Each value is positive and no greater than the engine's configured hard maximum. `max_candidates` and `max_work_units` count only authorized, FieldRead-eligible logical candidates and their evaluation; inaccessible records cannot consume a caller-visible budget or change its terminal result. `max_results` counts caller-visible result rows. An elapsed-time deadline is not a semantic query limit. Cancellation is out-of-band state owned by the trusted host: Rust supplies a `CancellationToken`, CLI Ctrl-C signals it, and IPC sends `CancelRequest { target_request_id }` on the same authenticated session. Cancellation and budget exhaustion remain distinct terminal outcomes. Neither may be mapped to a successful complete page or complete aggregate.

## 2. Closed filters and field comparisons

```text
FilterExpr = MatchAll
           | All(NonEmptyList<FilterExpr>)
           | Any(NonEmptyList<FilterExpr>)
           | Not(FilterExpr)
           | Test(FilterTest)

FilterTest = RecordKindIn(NonEmptySet<RecordKind>)
           | RecordRefIn(NonEmptySet<RecordRef>)
           | SubjectIs(SubjectRef)
           | PredicateIs(PredicateId)
           | EventKindIs(EventKindId)
           | RecordedRevision(RevisionRange)
           | TimeOverlaps { field: AssertionValidity | EventTime,
                           window: AbsoluteTimeRange | CalendarRelativeWindow }
           | FieldExists(FieldSelector)
           | FieldEquals { field: FieldSelector, value: Value }
           | FieldRange { field: FieldSelector,
                         lower: Option<ValueBound>, upper: Option<ValueBound> }

ValueBound { value: Value, edge: Inclusive | Exclusive }
RevisionRange { lower: Option<RevisionBound>, upper: Option<RevisionBound> }
RevisionBound { value: Revision, edge: Inclusive | Exclusive }
```

This is a closed AST, not a string language. `MatchAll` is the only no-filter expression. `All` and `Any` have at least one child; `Not` has exactly one. Version 1.0 accepts at most 32 levels of nesting and 256 total tests. Empty boolean groups, unknown variants, duplicate JSON keys, excess depth/count, invalid ranges, and selector/value-kind mismatches fail with `InvalidQuery`; they are never repaired or ignored. `All`/`Any` children are canonicalized by typed encoding, so input list order cannot change the QueryHash; duplicate identical children are removed. Record IDs and values are compared only within their exact typed variants. Decimal comparison is exact. A field absent from a record does not satisfy `FieldExists`, `FieldEquals`, or `FieldRange`; absence is never treated as a null Value. A FieldRange is valid only for a ValueKind with an exact total order, must contain at least one bound, and its lower bound must precede its upper bound (or be equal with both edges inclusive). Revision ranges use the same bound-edge rule and must be non-empty.

`RecordedRevision` uses explicit inclusive/exclusive Revision bounds. `TimeOverlaps` uses the M0-04b typed Time contract, one Timeline, and half-open intervals. `AbsoluteTimeRange { start: Time, end: Time }` requires `start < end`. AssertionValidity and EventTime spans match when they overlap `[start, end)`; an instant matches when it lies inside it. An open span has no upper endpoint and therefore overlaps when its start precedes the query end. A `CalendarRelativeWindow` needs its explicit anchor, resolves by the M0-04b checked calendar operation, and contributes its resolved absolute interval to the query fingerprint. The engine never supplies the current time. Malformed values, incomparable Timelines, overflow, or historical-schema corruption fail explicitly; the engine never falls back to Current schema or a different Timeline.

Security evaluation precedes candidate creation and every filter, sort, search, aggregation, Explain, and serialization step. Every requested field used by a filter or sort must pass the applicable record-read and FieldRead checks before a candidate is created. A candidate lacking a required readable field is excluded before it can affect matches, counts, ordering, conflicts, or errors. A redacted DTO field is never accepted as a filter value. Hidden records and values cannot affect public outcome shape or counts.

The operation Capability is also mandatory and is never inferred from record-read access: `RawHistory` requires `RawHistoryRead` plus each matching record-class Read right; the administrative raw projection additionally requires `AdminRawRead`. `ResolvedView`, `TokenSearch`, `Explain`, `GraphTraversal`, and `Aggregate` require `QueryResolve`, `QuerySearch`, `QueryExplain`, `QueryGraphTraverse`, and `QueryAggregate`, respectively, plus applicable record/field rights. `FullTextSearch` requires `QueryFullText` and the applicable record/field rights. These checks use the M0-04c policy evaluator and safe public error mapping.

## 3. Search and deterministic sorting

`TokenSearch` is the deterministic 1.0 search operation. `SearchSpec { fields: NonEmptySet<FieldSelector>, terms: NonEmptyList<SearchToken>, match: AllTerms | AnyTerm }` requires fields that the pinned schema defines as text-valued. A `SearchToken` is non-empty valid UTF-8 and contains no ASCII whitespace. The 1.0 tokenizer splits stored text only on ASCII whitespace bytes `0x09..0x0D` and `0x20`; each resulting token is compared byte-for-byte and case-sensitively. There is no Unicode normalization, locale collation, case folding, stemming, phrase matching, or implicit search field. Duplicate terms are removed during canonicalization. Search does not return text snippets. `FullTextSearch` evaluates the closed boolean `FullTextExpr` over the same exact token profile; it adds no phrase mode, rank, or snippets and requires `QueryFullText`. An engine may omit the operation; if omitted it returns `UnsupportedQueryCapability` and does not silently substitute TokenSearch. `FullTextExpr` uses the same 32-level/256-term bound as FilterExpr; `All`/`Any` children use canonical typed ordering.

`GraphTraversal` uses typed `RecordRef` roots, existing closed relationship-kind variants, explicit direction, and positive engine-bounded node/edge budgets. `max_depth = 0` means roots only. `StopAtRepeatedNode` omits an edge that would revisit an emitted node; `EmitRepeatedEdgeAndStop` emits that visible edge once but does not expand the repeated node again. Traversal expands only authorized visible nodes and edges. `Aggregate` is exactly `Count`, `Exists`, or `GroupedCount` over visible resolved results as in Master §16; `GroupedCount` uses typed `FieldSelector`s and the exact ValueKind comparator rules above. A missing group field forms a separate Missing group; a redacted field is never a grouping value. Group keys use canonical typed ordering and `max_results` bounds returned group rows. Counts, groups, Explain details, and graph nodes/edges never include invisible records.

```text
SortTerm { field: SortField, direction: Ascending | Descending,
           missing: First | Last }

SortField = ResultKey | RecordedRevision | EntityId | PredicateId | EventTime
          | FieldValue(FieldSelector) | GroupValue(FieldSelector) | AggregateCount
```

At most eight SortTerms are accepted. A SortField must be valid for the selected operation; for example, `GroupValue` and `AggregateCount` are valid only for their corresponding aggregate output. A `FieldValue` sort requires a pinned-schema ValueKind with an exact total order; it is invalid for list/object-like Values, mixed ValueKinds, or incomparable Timelines. IDs use lexicographic order of their typed 16-byte representation; typed variant keys sort by their fixed 1.0 tag followed by the typed ID; Symbols and Strings use exact UTF-8 byte order; Decimal uses exact numeric order; Time uses exact normalized ticks on its one Timeline. `EventTime` sorts by variant tag (`Instant` before `Span`), then start Time, then end Time (closed end before open end); it is valid only when the query context selects one comparable Timeline. `AggregateCount` uses unsigned numeric order. `missing` applies only to an absent value and its First/Last placement is independent of sort direction; direction reverses only comparisons between present values. Redacted is not a sort value. Sort-field authorization follows the filter rule above.

If `sort` is empty, rows use `ResultKey ASC`, with missing fields last. Requested sort terms are applied in order; ties always use the operation's caller-visible typed `ResultKey ASC` as a final stable key. That key is formed from the logical result identity (a `RecordRef` for RawHistory; the canonical `(SubjectRef tag, typed subject ID, PredicateId, PerspectiveScope tag/ID, EpistemicMode tag)` tuple for a resolved result; the same result identity for Explain and Search). Each closed enum uses its fixed 1.0 variant tag order and each ID uses its typed 16-byte order. The tie-breaker cannot consult an identifier or field hidden from the caller. Search has no implicit relevance order or score sort. The same pinned snapshot, schema, authorization state, request, and engine contract therefore produce the same ordered logical page.

`Projection::OperationDefault` expands to the one protocol-defined standard result shape for its operation; it is not a UI- or CLI-specific selection. `Projection::Only` may request fields permitted for that operation. A caller lacking FieldRead for a projected output-only field receives the M0-04c `Redacted` state in the same fixed DTO position; a denied field cannot be used as a filter, sort, search, or group key. A field needed to evaluate a candidate still follows the pre-candidate exclusion rule above. Unknown and unauthorized targets keep the same protected public shape.

## 4. Pagination and result DTOs

```text
PageRequest { limit: UInt32, cursor: Option<OpaqueCursor> }
QueryPage {
  snapshot_id: SnapshotId,
  results: List<QueryResultDto>,
  next_cursor: Option<OpaqueCursor>
}

QueryResultDto = RawRecord(OwnedRecordDto)
               | ResolvedResult(Outcome, List<ContributorDto>)
               | Explanation(ExplainDto)
               | SearchHit(ResultKey, NonEmptySet<FieldSelector>)
               | GraphResult(GraphDto)
               | AggregateResult(AggregateDto)
```

`limit` is positive and bounded by the engine's configured maximum. Each page is an owned, complete page; it has no borrowed storage view. `next_cursor` is present only when more results exist. Construction consumes a candidate stream to a page boundary before returning the page. A non-skippable item error, cancellation, or budget exhaustion returns a terminal error and no page marked complete; an incomplete aggregate is never returned as complete. Skippable diagnostic items remain internal diagnostics and are not converted into domain results.

The opaque cursor is the Master §16/§31.5 256-bit random handle with the bounded server-side session state and MAC. State binds PrincipalId, effective Capability fingerprint, current and applicable historical SecurityEpoch, SnapshotId, canonical QueryHash, EngineSessionId, internal SortKey, page limit, and expiry. The QueryHash covers the protocol semantic version, operation and typed parameters, explicit context, concrete snapshot, resolved calendar windows, canonical filter, projection, and sorting; it excludes transport request IDs and cursor bytes. Before every continuation page the engine rechecks AuthorizationNow and all bound security epochs. Unknown, expired, malformed, manipulated, or security-invalidated cursors map to the same public `CursorInvalidated` outcome. A continuation never shifts to Current or another snapshot. Engine restart, session change, expiry, or relevant authorization change invalidates it.

## 5. Versioned CLI and IPC envelopes

```text
ProtocolVersion { major: UInt16, minor: UInt16 }   // 1.0
ProtocolFeature = FullTextSearch
RequestId = UUID
HandshakeRequest {
  request_id: RequestId,
  supported_versions: NonEmptySet<ProtocolVersion>,
  required_features: Set<ProtocolFeature>
}
HandshakeResponse {
  request_id: RequestId,
  selected_version: ProtocolVersion,
  supported_features: Set<ProtocolFeature>
}
RequestEnvelope {
  protocol: ProtocolVersion,
  request_id: RequestId,
  operation: RequestOperation
}
ResponseEnvelope {
  protocol: ProtocolVersion,
  request_id: RequestId,
  outcome: ResponseBody | PublicErrorDto
}

RequestOperation = Query(QueryRequest) | Cancel { target_request_id: RequestId }
ResponseBody = QueryPage(QueryPage)
             | CancelResult { target_request_id: RequestId,
                             disposition: CancellationSignalled | AlreadyTerminal }
```

On desktop IPC, a separate `HandshakeRequest` is the first frame; the engine selects the highest mutually supported version and returns `HandshakeResponse`. No overlap or missing required feature fails closed before requests are accepted. In 1.0 the only mandatory protocol version is 1.0; `FullTextSearch` is the only optional feature tag. Thereafter `RequestOperation` is a closed tagged union: `Query` carries one `QueryRequest`; `Cancel` names an active request, not a caller-supplied CancellationToken. Responses are correlated by `request_id`. IDs are unique among active requests in a session. IPC accepts a Cancel frame while its target Query is running; each accepted request receives exactly one terminal response, and a completion/cancellation race has one winner. CLI machine mode uses the same request and response envelopes without a handshake; its command-line parser constructs the same DTO, and JSON Lines output contains one complete JSON envelope per line. Human-readable CLI rendering is presentation only and has no separate query semantics.

Version 1.0 uses closed objects and closed enum tags. Every enum DTO uses adjacent JSON tagging with a stable `type` snake_case tag and a `data` object (unit variants use an empty object); object member order has no meaning. Unknown fields, duplicate keys, unknown tags, malformed canonical scalars, and unsupported major/minor versions fail with a stable protocol error; a receiver never guesses a default for an unknown value. A future minor may add only optional fields or feature tags whose omission preserves the 1.0 meaning. Required feature negotiation is explicit. A major version may change meaning and cannot be negotiated as compatible. Protocol versioning is independent from the on-disk storage format and migration version.

JSON encodings follow Master §12 exactly: UUID IDs are canonical UUID text; `i128`, `u128`, `Revision`, `Decimal`, `u64` budgets, and 64-bit sizes are validated canonical decimal strings; Bytes are unpadded Base64url; Time includes typed TimelineId, ticks string, and registered unit Symbol. JSON numbers are permitted only for bounded integers no greater than `Number.MAX_SAFE_INTEGER`. No IEEE floating-point value is accepted where a typed WorldDB scalar is required. DTOs contain no arbitrary `JsonValue` escape hatch.

Public error codes are stable typed values: `InvalidRequest`, `UnsupportedProtocolVersion`, `UnsupportedOperation`, `InvalidQuery`, `UnsupportedQueryCapability`, `Unauthorized`, `NotFound`, `SnapshotExpired`, `CursorInvalidated`, `Cancelled`, `BudgetExceeded`, `StorageRead`, `CorruptData`, and `Internal`. `PublicErrorDto` contains only the code, a retryability flag, an optional stable MessageKey, and code-specific safe fields from a closed schema. Security-mapped `NotFound`/`Unauthorized` retain the Master-equivalent response shape when existence is protected. Errors never expose Rust type names, causes, filesystem paths, SQL/query text, hidden IDs, cursor-decoding distinctions, or unsanitized backend messages.

## 6. Existing-contract cross-check

- Master §§12 and desktop IPC retain the canonical JSON precision and filesystem-boundary rules; this supplement defines the query DTO mapping carried by those envelopes.
- Master §16 retains Raw History, Resolved View, Explain, Search, graph, aggregation, stream-terminal, budget, and cancellation semantics. This supplement closes their common request context, filter/search/sort, page, and error transport shapes without changing resolution semantics.
- M0-04b owns typed Time, Timeline comparison, CalendarPeriod, and explicit relative-window arithmetic; M0-04c owns field authorization, non-interference, authenticated Principal context, and historical security permissions.
- Master §31.5 owns cursor confidentiality, authorization recheck, state binding, and session lifetime; the page DTO cannot weaken it.
- No new WDB invariant IDs or First-Class persisted types are introduced. Runtime parity, property, security, precision, fuzz, budget, and cancellation tests remain implementation obligations for M3/M4/M5/M6/M8.
