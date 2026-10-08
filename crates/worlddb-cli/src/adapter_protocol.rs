//! Bounded, versioned process protocol for import/export adapters.

use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use worlddb_process_adapter::IsolatedChild;

const MANIFEST_MAGIC: &[u8; 8] = b"WDBAM\0\0\x01";
const HANDSHAKE_MAGIC: &[u8; 8] = b"WDBAH\0\0\x01";
const OUTPUT_MAGIC: &[u8; 8] = b"WDBAO\0\0\x01";
const MANIFEST_CONTEXT: &[u8] = b"WorldDB.AdapterManifest.v1\0";
const HANDSHAKE_CONTEXT: &[u8] = b"WorldDB.AdapterHandshake.v1\0";
const OUTPUT_CONTEXT: &[u8] = b"WorldDB.AdapterOutput.v1\0";
const DIGEST_BYTES: usize = 32;
const FRAME_HEADER_BYTES: usize = 16;
const MAX_TIMEOUT_MILLIS: u64 = 60 * 60 * 1_000;
const MAX_MEMORY_BYTES: u64 = 1_024 * 1_024 * 1_024;
const MAX_IO_BYTES: u64 = 512 * 1_024 * 1_024;
const MAX_IO_FRAME_BYTES: usize = MAX_IO_BYTES as usize + FRAME_HEADER_BYTES + DIGEST_BYTES;
const MAX_MAPPING_BYTES: usize = 64 * 1_024 * 1_024;
const MAX_MANIFEST_BYTES: usize = MAX_MAPPING_BYTES + 128 * 1_024;
const MAX_HANDSHAKE_BYTES: usize = 32 * 1_024;
const MAX_MANIFEST_FRAME_BYTES: usize = MAX_MANIFEST_BYTES + FRAME_HEADER_BYTES + DIGEST_BYTES;
const MAX_CAPABILITIES: usize = 256;
const MAX_CAPABILITY_BYTES: usize = 64;
const STDERR_TRACKING_THRESHOLD_BYTES: usize = 64 * 1_024;
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Maximum encoded manifest size accepted by the CLI before allocation.
pub const MAX_ADAPTER_MANIFEST_ENCODED_BYTES: usize = MAX_MANIFEST_BYTES;

/// Protocol version advertised by an adapter process.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
}

impl ProtocolVersion {
    /// Creates a protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Major protocol version.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Minor protocol version.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// Closed import/export operation supplied to one adapter invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterOperation {
    /// Convert a foreign representation into a canonical logical export.
    Import,
    /// Convert a validated logical export into a foreign representation.
    Export,
}

/// Canonical ASCII capability symbol understood by protocol negotiation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterCapability(String);

impl AdapterCapability {
    /// Parses a lowercase ASCII capability token.
    pub fn new(value: impl Into<String>) -> Result<Self, AdapterProtocolError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty()
            || bytes.len() > MAX_CAPABILITY_BYTES
            || !bytes.first().is_some_and(u8::is_ascii_lowercase)
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        {
            return Err(AdapterProtocolError::InvalidCapability);
        }
        Ok(Self(value))
    }

    /// Capability token in canonical lowercase ASCII form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Manifest-declared time, memory, and byte budgets for one adapter process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterBudget {
    timeout_millis: u64,
    memory_limit_bytes: u64,
    max_input_bytes: u64,
    max_output_bytes: u64,
}

impl AdapterBudget {
    /// Creates resource limits bounded by the WorldDB 1.0 hard ceilings.
    pub fn new(
        timeout_millis: u64,
        memory_limit_bytes: u64,
        max_input_bytes: u64,
        max_output_bytes: u64,
    ) -> Result<Self, AdapterProtocolError> {
        if timeout_millis == 0
            || timeout_millis > MAX_TIMEOUT_MILLIS
            || memory_limit_bytes == 0
            || memory_limit_bytes > MAX_MEMORY_BYTES
            || max_input_bytes > MAX_IO_BYTES
            || max_output_bytes > MAX_IO_BYTES
            || max_input_bytes
                .checked_add(max_output_bytes)
                .is_none_or(|sum| sum > MAX_IO_BYTES)
        {
            return Err(AdapterProtocolError::ResourceLimit);
        }
        Ok(Self {
            timeout_millis,
            memory_limit_bytes,
            max_input_bytes,
            max_output_bytes,
        })
    }

    /// Wall-clock time allowed for negotiation and execution.
    #[must_use]
    pub const fn timeout_millis(self) -> u64 {
        self.timeout_millis
    }

    /// Adapter-declared working-memory budget.
    #[must_use]
    pub const fn memory_limit_bytes(self) -> u64 {
        self.memory_limit_bytes
    }

    /// Maximum payload bytes the host sends to the adapter.
    #[must_use]
    pub const fn max_input_bytes(self) -> u64 {
        self.max_input_bytes
    }

    /// Maximum payload bytes the host accepts from the adapter.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

/// Exact operation context sent to an isolated adapter before any data bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterManifest {
    operation: AdapterOperation,
    protocol_major: u16,
    minimum_protocol_minor: u16,
    seed: [u8; 32],
    mapping_plan: Vec<u8>,
    budget: AdapterBudget,
    required_capabilities: Vec<AdapterCapability>,
}

impl AdapterManifest {
    /// Creates a canonical version-1 adapter manifest.
    pub fn new(
        operation: AdapterOperation,
        seed: [u8; 32],
        mapping_plan: Vec<u8>,
        budget: AdapterBudget,
        mut required_capabilities: Vec<AdapterCapability>,
    ) -> Result<Self, AdapterProtocolError> {
        if mapping_plan.len() > MAX_MAPPING_BYTES || required_capabilities.len() > MAX_CAPABILITIES
        {
            return Err(AdapterProtocolError::ResourceLimit);
        }
        required_capabilities.sort_unstable();
        if required_capabilities
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left == right))
        {
            return Err(AdapterProtocolError::DuplicateCapability);
        }
        Ok(Self {
            operation,
            protocol_major: 1,
            minimum_protocol_minor: 0,
            seed,
            mapping_plan,
            budget,
            required_capabilities,
        })
    }

    /// Import or export operation.
    #[must_use]
    pub const fn operation(&self) -> AdapterOperation {
        self.operation
    }

    /// Deterministic 256-bit seed assigned by the trusted host.
    #[must_use]
    pub const fn seed(&self) -> [u8; 32] {
        self.seed
    }

    /// Exact canonical ID mapping plan bytes.
    #[must_use]
    pub fn mapping_plan(&self) -> &[u8] {
        &self.mapping_plan
    }

    /// Manifested resource limits.
    #[must_use]
    pub const fn budget(&self) -> AdapterBudget {
        self.budget
    }

    /// Capabilities that must be advertised before any payload is sent.
    #[must_use]
    pub fn required_capabilities(&self) -> &[AdapterCapability] {
        &self.required_capabilities
    }

    /// Encodes canonical bytes with a domain-separated BLAKE3 digest.
    pub fn encode(&self) -> Result<Vec<u8>, AdapterProtocolError> {
        let capability_bytes = capability_wire_size(&self.required_capabilities)?;
        let payload_capacity = 2_usize
            .checked_add(2)
            .and_then(|size| size.checked_add(1 + 32 + 32 + 4 + 2))
            .and_then(|size| size.checked_add(self.mapping_plan.len()))
            .and_then(|size| size.checked_add(capability_bytes))
            .ok_or(AdapterProtocolError::ResourceLimit)?;
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(payload_capacity)
            .map_err(|_| AdapterProtocolError::AllocationFailed)?;
        push_u16(&mut payload, self.protocol_major);
        push_u16(&mut payload, self.minimum_protocol_minor);
        payload.push(match self.operation {
            AdapterOperation::Import => 1,
            AdapterOperation::Export => 2,
        });
        payload.extend_from_slice(&self.seed);
        push_u64(&mut payload, self.budget.timeout_millis);
        push_u64(&mut payload, self.budget.memory_limit_bytes);
        push_u64(&mut payload, self.budget.max_input_bytes);
        push_u64(&mut payload, self.budget.max_output_bytes);
        push_u32(
            &mut payload,
            u32::try_from(self.mapping_plan.len())
                .map_err(|_| AdapterProtocolError::ResourceLimit)?,
        );
        payload.extend_from_slice(&self.mapping_plan);
        push_u16(
            &mut payload,
            u16::try_from(self.required_capabilities.len())
                .map_err(|_| AdapterProtocolError::ResourceLimit)?,
        );
        for capability in &self.required_capabilities {
            payload.push(
                u8::try_from(capability.0.len())
                    .map_err(|_| AdapterProtocolError::ResourceLimit)?,
            );
            payload.extend_from_slice(capability.0.as_bytes());
        }
        encode_checked_frame(
            MANIFEST_MAGIC,
            MANIFEST_CONTEXT,
            &payload,
            MAX_MANIFEST_FRAME_BYTES,
        )
    }

    /// Decodes and re-encodes a manifest to require its one canonical form.
    pub fn decode(bytes: &[u8]) -> Result<Self, AdapterProtocolError> {
        let payload =
            decode_checked_frame(bytes, MANIFEST_MAGIC, MANIFEST_CONTEXT, MAX_MANIFEST_BYTES)?;
        let mut cursor = Cursor::new(payload);
        let protocol_major = cursor.u16()?;
        let minimum_protocol_minor = cursor.u16()?;
        if protocol_major != 1 || minimum_protocol_minor != 0 {
            return Err(AdapterProtocolError::ProtocolMismatch);
        }
        let operation = match cursor.u8()? {
            1 => AdapterOperation::Import,
            2 => AdapterOperation::Export,
            _ => return Err(AdapterProtocolError::InvalidManifest),
        };
        let seed = cursor.array_32()?;
        let budget =
            AdapterBudget::new(cursor.u64()?, cursor.u64()?, cursor.u64()?, cursor.u64()?)?;
        let mapping_bytes =
            usize::try_from(cursor.u32()?).map_err(|_| AdapterProtocolError::ResourceLimit)?;
        if mapping_bytes > MAX_MAPPING_BYTES {
            return Err(AdapterProtocolError::ResourceLimit);
        }
        let mapping_source = cursor.take(mapping_bytes)?;
        let mut mapping_plan = Vec::new();
        mapping_plan
            .try_reserve_exact(mapping_bytes)
            .map_err(|_| AdapterProtocolError::AllocationFailed)?;
        mapping_plan.extend_from_slice(mapping_source);
        let capability_count = usize::from(cursor.u16()?);
        if capability_count > MAX_CAPABILITIES {
            return Err(AdapterProtocolError::ResourceLimit);
        }
        let mut required_capabilities = Vec::new();
        required_capabilities
            .try_reserve_exact(capability_count)
            .map_err(|_| AdapterProtocolError::AllocationFailed)?;
        for _ in 0..capability_count {
            let length = usize::from(cursor.u8()?);
            let token = cursor.take(length)?;
            let token =
                std::str::from_utf8(token).map_err(|_| AdapterProtocolError::InvalidCapability)?;
            required_capabilities.push(AdapterCapability::new(token)?);
        }
        if !cursor.is_empty() {
            return Err(AdapterProtocolError::InvalidManifest);
        }
        let mut manifest = Self::new(operation, seed, mapping_plan, budget, required_capabilities)?;
        manifest.protocol_major = protocol_major;
        manifest.minimum_protocol_minor = minimum_protocol_minor;
        if manifest.encode()?.as_slice() != bytes {
            return Err(AdapterProtocolError::NonCanonicalEncoding);
        }
        Ok(manifest)
    }
}

/// Adapter's offered protocol version and supported capabilities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterHandshake {
    protocol: ProtocolVersion,
    capabilities: Vec<AdapterCapability>,
}

impl AdapterHandshake {
    /// Creates a canonical adapter handshake.
    pub fn new(
        protocol: ProtocolVersion,
        mut capabilities: Vec<AdapterCapability>,
    ) -> Result<Self, AdapterProtocolError> {
        if capabilities.len() > MAX_CAPABILITIES {
            return Err(AdapterProtocolError::ResourceLimit);
        }
        capabilities.sort_unstable();
        if capabilities
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left == right))
        {
            return Err(AdapterProtocolError::DuplicateCapability);
        }
        Ok(Self {
            protocol,
            capabilities,
        })
    }

    /// Adapter protocol version.
    #[must_use]
    pub const fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// Capabilities offered by the adapter.
    #[must_use]
    pub fn capabilities(&self) -> &[AdapterCapability] {
        &self.capabilities
    }

    fn encode_payload(&self) -> Result<Vec<u8>, AdapterProtocolError> {
        let payload_capacity = 6_usize
            .checked_add(capability_wire_size(&self.capabilities)?)
            .ok_or(AdapterProtocolError::ResourceLimit)?;
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(payload_capacity)
            .map_err(|_| AdapterProtocolError::AllocationFailed)?;
        push_u16(&mut payload, self.protocol.major);
        push_u16(&mut payload, self.protocol.minor);
        push_u16(
            &mut payload,
            u16::try_from(self.capabilities.len())
                .map_err(|_| AdapterProtocolError::ResourceLimit)?,
        );
        for capability in &self.capabilities {
            payload.push(
                u8::try_from(capability.0.len())
                    .map_err(|_| AdapterProtocolError::ResourceLimit)?,
            );
            payload.extend_from_slice(capability.0.as_bytes());
        }
        Ok(payload)
    }

    fn decode_payload(payload: &[u8]) -> Result<Self, AdapterProtocolError> {
        let mut cursor = Cursor::new(payload);
        let protocol = ProtocolVersion::new(cursor.u16()?, cursor.u16()?);
        let count = usize::from(cursor.u16()?);
        if count > MAX_CAPABILITIES {
            return Err(AdapterProtocolError::ResourceLimit);
        }
        let mut capabilities = Vec::new();
        capabilities
            .try_reserve_exact(count)
            .map_err(|_| AdapterProtocolError::AllocationFailed)?;
        for _ in 0..count {
            let length = usize::from(cursor.u8()?);
            let token = std::str::from_utf8(cursor.take(length)?)
                .map_err(|_| AdapterProtocolError::InvalidCapability)?;
            capabilities.push(AdapterCapability::new(token)?);
        }
        if !cursor.is_empty() {
            return Err(AdapterProtocolError::InvalidHandshake);
        }
        let handshake = Self::new(protocol, capabilities)?;
        if handshake.encode_payload()?.as_slice() != payload {
            return Err(AdapterProtocolError::NonCanonicalEncoding);
        }
        Ok(handshake)
    }
}

/// Fully validated output from one successfully exited adapter process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterOutput {
    adapter_process_id: u32,
    negotiated_protocol: ProtocolVersion,
    bytes: Vec<u8>,
    diagnostics_exceeded_threshold: bool,
}

impl AdapterOutput {
    /// Operating-system process ID observed by the trusted host.
    #[must_use]
    pub const fn adapter_process_id(&self) -> u32 {
        self.adapter_process_id
    }

    /// Protocol version accepted during capability negotiation.
    #[must_use]
    pub const fn negotiated_protocol(&self) -> ProtocolVersion {
        self.negotiated_protocol
    }

    /// Complete bounded payload. It is returned only after a clean process exit.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether discarded adapter diagnostics exceeded 64 KiB.
    #[must_use]
    pub const fn diagnostics_exceeded_threshold(&self) -> bool {
        self.diagnostics_exceeded_threshold
    }
}

/// Process-isolated host for an import/export adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct AdapterProcessHost;

impl AdapterProcessHost {
    /// Negotiates first, then sends one bounded payload to a child process.
    ///
    /// The child inherits no environment variables and receives no database
    /// path, engine session, storage handle, or Core object. Callers validate
    /// the returned bytes as a canonical logical artifact before importing or
    /// publishing them.
    pub fn run(
        &self,
        mut command: Command,
        manifest: &AdapterManifest,
        input: Vec<u8>,
    ) -> Result<AdapterOutput, AdapterProtocolError> {
        if u64::try_from(input.len()).map_err(|_| AdapterProtocolError::ResourceLimit)?
            > manifest.budget.max_input_bytes
        {
            return Err(AdapterProtocolError::InputTooLarge);
        }
        let manifest_bytes = manifest.encode()?;
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(manifest.budget.timeout_millis))
            .ok_or(AdapterProtocolError::ResourceLimit)?;
        let mut child = IsolatedChild::spawn(&mut command, manifest.budget.memory_limit_bytes)
            .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
        let process_id = child.id();

        let result = Self::supervise(
            &mut child,
            process_id,
            deadline,
            manifest,
            manifest_bytes,
            input,
        );
        if result.is_err() {
            terminate_child(&mut child);
        }
        result
    }

    fn supervise(
        child: &mut IsolatedChild,
        process_id: u32,
        deadline: Instant,
        manifest: &AdapterManifest,
        manifest_bytes: Vec<u8>,
        input: Vec<u8>,
    ) -> Result<AdapterOutput, AdapterProtocolError> {
        let stdin = child
            .take_stdin()
            .ok_or(AdapterProtocolError::ProcessPipeUnavailable)?;
        let stdout = child
            .take_stdout()
            .ok_or(AdapterProtocolError::ProcessPipeUnavailable)?;
        let stderr = child
            .take_stderr()
            .ok_or(AdapterProtocolError::ProcessPipeUnavailable)?;

        let (handshake_tx, handshake_rx) = mpsc::sync_channel(1);
        let (output_tx, output_rx) = mpsc::sync_channel(1);
        let (reader_gate_tx, reader_gate_rx) = mpsc::sync_channel(1);
        let (writer_gate_tx, writer_gate_rx) = mpsc::sync_channel(1);
        let max_output_bytes = manifest.budget.max_output_bytes;

        let reader_thread = thread::Builder::new()
            .name(String::from("worlddb-adapter-stdout"))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let handshake = read_adapter_handshake(&mut reader);
                if handshake_tx.send(handshake).is_err() {
                    return;
                }
                if !matches!(reader_gate_rx.recv(), Ok(true)) {
                    return;
                }
                let output = read_adapter_output(&mut reader, max_output_bytes);
                let _ = output_tx.send(output);
            })
            .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;

        let writer_thread = thread::Builder::new()
            .name(String::from("worlddb-adapter-stdin"))
            .spawn(move || {
                let mut writer = stdin;
                writer
                    .write_all(&manifest_bytes)
                    .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
                writer
                    .flush()
                    .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
                if !matches!(writer_gate_rx.recv(), Ok(true)) {
                    return Ok(());
                }
                write_u64(
                    &mut writer,
                    u64::try_from(input.len()).map_err(|_| AdapterProtocolError::ResourceLimit)?,
                )?;
                writer
                    .write_all(&input)
                    .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
                writer
                    .flush()
                    .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))
            })
            .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;

        let stderr_thread = thread::Builder::new()
            .name(String::from("worlddb-adapter-stderr"))
            .spawn(move || drain_stderr(stderr))
            .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;

        let handshake = receive_before_deadline(&handshake_rx, child, deadline)?;
        let negotiated_protocol = negotiate(manifest, &handshake)?;
        reader_gate_tx
            .send(true)
            .map_err(|_| AdapterProtocolError::SupervisorFailed)?;
        writer_gate_tx
            .send(true)
            .map_err(|_| AdapterProtocolError::SupervisorFailed)?;

        let bytes = receive_before_deadline(&output_rx, child, deadline)?;
        let status = wait_for_exit(child, deadline)?;
        if !status.success() {
            return Err(AdapterProtocolError::AdapterCrashed(status.code()));
        }
        child
            .terminate_process_tree()
            .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
        join_result(reader_thread)?;
        join_writer(writer_thread)?;
        let diagnostics_exceeded_threshold = join_stderr(stderr_thread)?;
        Ok(AdapterOutput {
            adapter_process_id: process_id,
            negotiated_protocol,
            bytes,
            diagnostics_exceeded_threshold,
        })
    }
}

/// Reads the host's canonical manifest frame from an adapter's standard input.
pub fn read_adapter_manifest<R: Read>(
    reader: &mut R,
) -> Result<AdapterManifest, AdapterProtocolError> {
    let bytes = read_checked_frame(
        reader,
        MANIFEST_MAGIC,
        MANIFEST_CONTEXT,
        MAX_MANIFEST_FRAME_BYTES,
    )?;
    let frame = encode_checked_frame(
        MANIFEST_MAGIC,
        MANIFEST_CONTEXT,
        &bytes,
        MAX_MANIFEST_FRAME_BYTES,
    )?;
    AdapterManifest::decode(&frame)
}

/// Writes an adapter's offered protocol version and capabilities to standard output.
pub fn write_adapter_handshake<W: Write>(
    writer: &mut W,
    handshake: &AdapterHandshake,
) -> Result<(), AdapterProtocolError> {
    let payload = handshake.encode_payload()?;
    write_checked_frame(
        writer,
        HANDSHAKE_MAGIC,
        HANDSHAKE_CONTEXT,
        &payload,
        MAX_HANDSHAKE_BYTES,
    )?;
    writer
        .flush()
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))
}

/// Reads the one input payload after the host accepted the adapter handshake.
pub fn read_adapter_payload<R: Read>(
    reader: &mut R,
    manifest: &AdapterManifest,
) -> Result<Vec<u8>, AdapterProtocolError> {
    let length =
        usize::try_from(read_u64(reader)?).map_err(|_| AdapterProtocolError::ResourceLimit)?;
    let length_u64 = u64::try_from(length).map_err(|_| AdapterProtocolError::ResourceLimit)?;
    if length_u64 > manifest.budget.max_input_bytes || length > MAX_IO_BYTES as usize {
        return Err(AdapterProtocolError::InputTooLarge);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| AdapterProtocolError::AllocationFailed)?;
    bytes.resize(length, 0);
    reader
        .read_exact(&mut bytes)
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
    Ok(bytes)
}

/// Writes one bounded adapter output frame to standard output.
pub fn write_adapter_output<W: Write>(
    writer: &mut W,
    bytes: &[u8],
) -> Result<(), AdapterProtocolError> {
    if bytes.len() > MAX_IO_BYTES as usize {
        return Err(AdapterProtocolError::OutputTooLarge);
    }
    write_checked_frame(
        writer,
        OUTPUT_MAGIC,
        OUTPUT_CONTEXT,
        bytes,
        MAX_IO_FRAME_BYTES,
    )?;
    writer
        .flush()
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))
}

fn read_adapter_handshake<R: Read>(
    reader: &mut R,
) -> Result<AdapterHandshake, AdapterProtocolError> {
    let bytes = read_checked_frame(
        reader,
        HANDSHAKE_MAGIC,
        HANDSHAKE_CONTEXT,
        MAX_HANDSHAKE_BYTES,
    )?;
    AdapterHandshake::decode_payload(&bytes)
}

fn read_adapter_output<R: Read>(
    reader: &mut R,
    max_output_bytes: u64,
) -> Result<Vec<u8>, AdapterProtocolError> {
    let payload_limit = usize::try_from(max_output_bytes)
        .map_err(|_| AdapterProtocolError::ResourceLimit)?
        .min(MAX_IO_BYTES as usize);
    let frame_limit = payload_limit
        .checked_add(FRAME_HEADER_BYTES + DIGEST_BYTES)
        .ok_or(AdapterProtocolError::ResourceLimit)?;
    let bytes =
        read_checked_frame(reader, OUTPUT_MAGIC, OUTPUT_CONTEXT, frame_limit).map_err(|error| {
            match error {
                AdapterProtocolError::ResourceLimit => AdapterProtocolError::OutputTooLarge,
                other => other,
            }
        })?;
    let length = u64::try_from(bytes.len()).map_err(|_| AdapterProtocolError::ResourceLimit)?;
    if length > max_output_bytes {
        return Err(AdapterProtocolError::OutputTooLarge);
    }
    let mut trailing = [0_u8; 1];
    match reader.read(&mut trailing) {
        Ok(0) => Ok(bytes),
        Ok(_) => Err(AdapterProtocolError::TrailingResponseBytes),
        Err(error) => Err(AdapterProtocolError::ProcessIo(error.kind())),
    }
}

fn negotiate(
    manifest: &AdapterManifest,
    handshake: &AdapterHandshake,
) -> Result<ProtocolVersion, AdapterProtocolError> {
    if handshake.protocol.major != manifest.protocol_major
        || handshake.protocol.minor < manifest.minimum_protocol_minor
    {
        return Err(AdapterProtocolError::ProtocolMismatch);
    }
    for required in &manifest.required_capabilities {
        if handshake.capabilities.binary_search(required).is_err() {
            return Err(AdapterProtocolError::RequiredCapabilityMissing(
                required.clone(),
            ));
        }
    }
    Ok(ProtocolVersion::new(
        manifest.protocol_major,
        handshake.protocol.minor,
    ))
}

fn receive_before_deadline<T>(
    receiver: &Receiver<Result<T, AdapterProtocolError>>,
    child: &mut IsolatedChild,
    deadline: Instant,
) -> Result<T, AdapterProtocolError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AdapterProtocolError::Timeout);
        }
        let wait = remaining.min(SUPERVISOR_POLL_INTERVAL);
        match receiver.recv_timeout(wait) {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) => return Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?
                {
                    if !status.success() {
                        return Err(AdapterProtocolError::AdapterCrashed(status.code()));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AdapterProtocolError::SupervisorFailed);
            }
        }
    }
}

fn wait_for_exit(
    child: &mut IsolatedChild,
    deadline: Instant,
) -> Result<ExitStatus, AdapterProtocolError> {
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(AdapterProtocolError::Timeout);
        }
        thread::sleep(SUPERVISOR_POLL_INTERVAL);
    }
}

fn terminate_child(child: &mut IsolatedChild) {
    let running = child.try_wait().ok().flatten().is_none();
    if running {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn drain_stderr<R: Read>(mut reader: R) -> bool {
    let mut tracked_bytes = 0_usize;
    let mut exceeded_threshold = false;
    let mut buffer = [0_u8; 4_096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => return exceeded_threshold,
            Ok(read) => {
                let remaining = STDERR_TRACKING_THRESHOLD_BYTES.saturating_sub(tracked_bytes);
                let tracked = read.min(remaining);
                tracked_bytes = tracked_bytes.saturating_add(tracked);
                exceeded_threshold |= tracked < read;
            }
        }
    }
}

fn join_result(handle: JoinHandle<()>) -> Result<(), AdapterProtocolError> {
    handle
        .join()
        .map_err(|_| AdapterProtocolError::SupervisorFailed)
}

fn join_writer(
    handle: JoinHandle<Result<(), AdapterProtocolError>>,
) -> Result<(), AdapterProtocolError> {
    handle
        .join()
        .map_err(|_| AdapterProtocolError::SupervisorFailed)?
}

fn join_stderr(handle: JoinHandle<bool>) -> Result<bool, AdapterProtocolError> {
    handle
        .join()
        .map_err(|_| AdapterProtocolError::SupervisorFailed)
}

fn write_checked_frame<W: Write>(
    writer: &mut W,
    magic: &[u8; 8],
    context: &[u8],
    payload: &[u8],
    max_bytes: usize,
) -> Result<(), AdapterProtocolError> {
    let total = FRAME_HEADER_BYTES
        .checked_add(payload.len())
        .and_then(|length| length.checked_add(DIGEST_BYTES))
        .ok_or(AdapterProtocolError::ResourceLimit)?;
    if total > max_bytes {
        return Err(AdapterProtocolError::ResourceLimit);
    }
    let payload_length =
        u64::try_from(payload.len()).map_err(|_| AdapterProtocolError::ResourceLimit)?;
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    header[..8].copy_from_slice(magic);
    header[8..].copy_from_slice(&payload_length.to_be_bytes());
    let digest = frame_digest_parts(context, &header, payload);
    writer
        .write_all(&header)
        .and_then(|()| writer.write_all(payload))
        .and_then(|()| writer.write_all(&digest))
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))
}

fn read_checked_frame<R: Read>(
    reader: &mut R,
    expected_magic: &[u8; 8],
    context: &[u8],
    max_bytes: usize,
) -> Result<Vec<u8>, AdapterProtocolError> {
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
    if header.get(..8) != Some(expected_magic.as_slice()) {
        return Err(AdapterProtocolError::InvalidFrame);
    }
    let raw_length: [u8; 8] = header
        .get(8..16)
        .ok_or(AdapterProtocolError::InvalidFrame)?
        .try_into()
        .map_err(|_| AdapterProtocolError::InvalidFrame)?;
    let payload_length = usize::try_from(u64::from_be_bytes(raw_length))
        .map_err(|_| AdapterProtocolError::ResourceLimit)?;
    let total = FRAME_HEADER_BYTES
        .checked_add(payload_length)
        .and_then(|length| length.checked_add(DIGEST_BYTES))
        .ok_or(AdapterProtocolError::ResourceLimit)?;
    if total > max_bytes {
        return Err(AdapterProtocolError::ResourceLimit);
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(payload_length)
        .map_err(|_| AdapterProtocolError::AllocationFailed)?;
    payload.resize(payload_length, 0);
    reader
        .read_exact(&mut payload)
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
    let mut claimed_digest = [0_u8; DIGEST_BYTES];
    reader
        .read_exact(&mut claimed_digest)
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
    if frame_digest_parts(context, &header, &payload) != claimed_digest {
        return Err(AdapterProtocolError::FrameDigestMismatch);
    }
    Ok(payload)
}

fn encode_checked_frame(
    magic: &[u8; 8],
    context: &[u8],
    payload: &[u8],
    max_bytes: usize,
) -> Result<Vec<u8>, AdapterProtocolError> {
    let total = FRAME_HEADER_BYTES
        .checked_add(payload.len())
        .and_then(|length| length.checked_add(DIGEST_BYTES))
        .ok_or(AdapterProtocolError::ResourceLimit)?;
    if total > max_bytes {
        return Err(AdapterProtocolError::ResourceLimit);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| AdapterProtocolError::AllocationFailed)?;
    bytes.extend_from_slice(magic);
    push_u64(
        &mut bytes,
        u64::try_from(payload.len()).map_err(|_| AdapterProtocolError::ResourceLimit)?,
    );
    bytes.extend_from_slice(payload);
    let digest = frame_digest(context, &bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn decode_checked_frame<'a>(
    bytes: &'a [u8],
    magic: &[u8; 8],
    context: &[u8],
    max_bytes: usize,
) -> Result<&'a [u8], AdapterProtocolError> {
    if bytes.len() < FRAME_HEADER_BYTES + DIGEST_BYTES
        || bytes.len() > max_bytes
        || bytes.get(..8) != Some(magic.as_slice())
    {
        return Err(AdapterProtocolError::InvalidFrame);
    }
    let length_bytes: [u8; 8] = bytes
        .get(8..16)
        .ok_or(AdapterProtocolError::InvalidFrame)?
        .try_into()
        .map_err(|_| AdapterProtocolError::InvalidFrame)?;
    let payload_length = usize::try_from(u64::from_be_bytes(length_bytes))
        .map_err(|_| AdapterProtocolError::ResourceLimit)?;
    let expected_total = FRAME_HEADER_BYTES
        .checked_add(payload_length)
        .and_then(|length| length.checked_add(DIGEST_BYTES))
        .ok_or(AdapterProtocolError::ResourceLimit)?;
    if expected_total != bytes.len() {
        return Err(AdapterProtocolError::InvalidFrame);
    }
    let digest_offset = bytes
        .len()
        .checked_sub(DIGEST_BYTES)
        .ok_or(AdapterProtocolError::InvalidFrame)?;
    let payload = bytes
        .get(FRAME_HEADER_BYTES..digest_offset)
        .ok_or(AdapterProtocolError::InvalidFrame)?;
    let claimed: [u8; DIGEST_BYTES] = bytes
        .get(digest_offset..)
        .ok_or(AdapterProtocolError::InvalidFrame)?
        .try_into()
        .map_err(|_| AdapterProtocolError::InvalidFrame)?;
    let digest_content = bytes
        .get(..digest_offset)
        .ok_or(AdapterProtocolError::InvalidFrame)?;
    if frame_digest(context, digest_content) != claimed {
        return Err(AdapterProtocolError::FrameDigestMismatch);
    }
    Ok(payload)
}

fn frame_digest(context: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(context);
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

fn frame_digest_parts(context: &[u8], header: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(context);
    hasher.update(header);
    hasher.update(payload);
    *hasher.finalize().as_bytes()
}

fn capability_wire_size(capabilities: &[AdapterCapability]) -> Result<usize, AdapterProtocolError> {
    capabilities.iter().try_fold(0_usize, |size, capability| {
        size.checked_add(1)
            .and_then(|current| current.checked_add(capability.0.len()))
            .ok_or(AdapterProtocolError::ResourceLimit)
    })
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn write_u64<W: Write>(writer: &mut W, value: u64) -> Result<(), AdapterProtocolError> {
    writer
        .write_all(&value.to_be_bytes())
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, AdapterProtocolError> {
    let mut bytes = [0_u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| AdapterProtocolError::ProcessIo(error.kind()))?;
    Ok(u64::from_be_bytes(bytes))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], AdapterProtocolError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(AdapterProtocolError::InvalidManifest)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(AdapterProtocolError::InvalidManifest)?;
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, AdapterProtocolError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(AdapterProtocolError::InvalidManifest)
    }

    fn u16(&mut self) -> Result<u16, AdapterProtocolError> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| AdapterProtocolError::InvalidManifest)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, AdapterProtocolError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| AdapterProtocolError::InvalidManifest)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, AdapterProtocolError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| AdapterProtocolError::InvalidManifest)?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn array_32(&mut self) -> Result<[u8; 32], AdapterProtocolError> {
        self.take(32)?
            .try_into()
            .map_err(|_| AdapterProtocolError::InvalidManifest)
    }

    fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }
}

/// Failure to validate, negotiate, or supervise one isolated adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterProtocolError {
    /// Manifest fields or framing are invalid.
    InvalidManifest,
    /// The adapter handshake fields or framing are invalid.
    InvalidHandshake,
    /// A capability is not a lowercase ASCII token.
    InvalidCapability,
    /// A capability occurs more than once in a canonical set.
    DuplicateCapability,
    /// A frame is truncated, has trailing bytes, or uses the wrong magic.
    InvalidFrame,
    /// A frame's integrity digest does not match.
    FrameDigestMismatch,
    /// An encoding was valid but not canonical.
    NonCanonicalEncoding,
    /// The child selected a different required protocol major or too-old minor.
    ProtocolMismatch,
    /// A required capability was not present in the adapter handshake.
    RequiredCapabilityMissing(AdapterCapability),
    /// A payload exceeds a declared or absolute byte limit.
    InputTooLarge,
    /// An adapter output exceeds its declared or absolute byte limit.
    OutputTooLarge,
    /// A process emits bytes after its one output frame.
    TrailingResponseBytes,
    /// An output frame is malformed or incomplete.
    InvalidOutput,
    /// The adapter exited unsuccessfully; its output is discarded.
    AdapterCrashed(Option<i32>),
    /// The adapter exceeded its wall-clock deadline.
    Timeout,
    /// A bounded allocation could not be reserved.
    AllocationFailed,
    /// A declared or computed value exceeds a hard ceiling.
    ResourceLimit,
    /// The operating system refused to create or operate a child process pipe.
    ProcessIo(io::ErrorKind),
    /// The child did not expose the standard streams required by the protocol.
    ProcessPipeUnavailable,
    /// The host-side protocol supervisor could not complete its own thread/channel work.
    SupervisorFailed,
}

impl fmt::Display for AdapterProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest => formatter.write_str("adapter manifest is invalid"),
            Self::InvalidHandshake => formatter.write_str("adapter handshake is invalid"),
            Self::InvalidCapability => formatter.write_str("adapter capability token is invalid"),
            Self::DuplicateCapability => formatter.write_str("adapter capability is duplicated"),
            Self::InvalidFrame => formatter.write_str("adapter protocol frame is invalid"),
            Self::FrameDigestMismatch => formatter.write_str("adapter frame digest does not match"),
            Self::NonCanonicalEncoding => formatter.write_str("adapter encoding is not canonical"),
            Self::ProtocolMismatch => {
                formatter.write_str("adapter protocol version is unsupported")
            }
            Self::RequiredCapabilityMissing(capability) => write!(
                formatter,
                "adapter does not support required capability {}",
                capability.as_str()
            ),
            Self::InputTooLarge => formatter.write_str("adapter input exceeds its byte limit"),
            Self::OutputTooLarge => formatter.write_str("adapter output exceeds its byte limit"),
            Self::TrailingResponseBytes => {
                formatter.write_str("adapter wrote bytes after its output frame")
            }
            Self::InvalidOutput => formatter.write_str("adapter output frame is invalid"),
            Self::AdapterCrashed(_) => formatter.write_str("adapter process failed"),
            Self::Timeout => formatter.write_str("adapter process exceeded its time limit"),
            Self::AllocationFailed => formatter.write_str("adapter protocol allocation failed"),
            Self::ResourceLimit => {
                formatter.write_str("adapter request exceeds a hard resource limit")
            }
            Self::ProcessIo(_) => formatter.write_str("adapter process communication failed"),
            Self::ProcessPipeUnavailable => {
                formatter.write_str("adapter process pipe is unavailable")
            }
            Self::SupervisorFailed => formatter.write_str("adapter process supervisor failed"),
        }
    }
}

impl std::error::Error for AdapterProtocolError {}

#[cfg(test)]
pub(crate) fn fuzz_probe(target: &str, bytes: &[u8]) -> bool {
    use std::io::Cursor;

    match target {
        "cli_adapter_manifest" => read_adapter_manifest(&mut Cursor::new(bytes)).is_ok(),
        "cli_adapter_payload" => read_u64(&mut Cursor::new(bytes)).is_ok(),
        "cli_adapter_handshake" => read_adapter_handshake(&mut Cursor::new(bytes)).is_ok(),
        "cli_adapter_output" => read_adapter_output(&mut Cursor::new(bytes), MAX_IO_BYTES).is_ok(),
        "cli_adapter_frame" => {
            decode_checked_frame(
                bytes,
                MANIFEST_MAGIC,
                MANIFEST_CONTEXT,
                MAX_MANIFEST_FRAME_BYTES,
            )
            .is_ok()
                || decode_checked_frame(
                    bytes,
                    HANDSHAKE_MAGIC,
                    HANDSHAKE_CONTEXT,
                    MAX_HANDSHAKE_BYTES,
                )
                .is_ok()
                || decode_checked_frame(bytes, OUTPUT_MAGIC, OUTPUT_CONTEXT, MAX_IO_FRAME_BYTES)
                    .is_ok()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        AdapterBudget, AdapterCapability, AdapterHandshake, AdapterManifest, AdapterOperation,
        AdapterProtocolError, OUTPUT_CONTEXT, OUTPUT_MAGIC, ProtocolVersion, negotiate,
        read_adapter_output, write_checked_frame,
    };

    fn sample_manifest() -> Option<AdapterManifest> {
        let budget = AdapterBudget::new(5_000, 64 * 1_024 * 1_024, 1_024, 1_024).ok()?;
        let capability = AdapterCapability::new("logical_records_v1").ok()?;
        AdapterManifest::new(
            AdapterOperation::Import,
            [3; 32],
            b"id-map".to_vec(),
            budget,
            vec![capability],
        )
        .ok()
    }

    #[test]
    fn manifest_encoding_is_stable_and_round_trips() {
        let manifest = sample_manifest();
        assert!(manifest.is_some());
        let Some(manifest) = manifest else {
            return;
        };
        let first = manifest.encode();
        let second = manifest.encode();
        assert_eq!(first, second);
        let decoded = first.and_then(|bytes| AdapterManifest::decode(&bytes));
        assert_eq!(decoded, Ok(manifest));
    }

    #[test]
    fn manifest_reader_accepts_the_canonical_frame() -> Result<(), AdapterProtocolError> {
        let Some(manifest) = sample_manifest() else {
            return Err(AdapterProtocolError::InvalidManifest);
        };
        let bytes = manifest.encode()?;
        assert_eq!(
            super::read_adapter_manifest(&mut Cursor::new(bytes.clone()))?,
            manifest
        );
        assert!(super::fuzz_probe("cli_adapter_manifest", &bytes));
        Ok(())
    }

    #[test]
    fn manifest_rejects_combined_io_budget_above_the_hard_ceiling() {
        let result = AdapterBudget::new(
            5_000,
            64 * 1_024 * 1_024,
            300 * 1_024 * 1_024,
            300 * 1_024 * 1_024,
        );
        assert!(matches!(result, Err(AdapterProtocolError::ResourceLimit)));
    }

    #[test]
    fn required_capability_is_checked_before_io_is_negotiated() {
        let manifest = sample_manifest();
        assert!(manifest.is_some());
        let Some(manifest) = manifest else {
            return;
        };
        let handshake = AdapterHandshake::new(ProtocolVersion::new(1, 0), Vec::new());
        assert!(handshake.is_ok());
        let Some(handshake) = handshake.ok() else {
            return;
        };
        let result = negotiate(&manifest, &handshake);
        assert!(matches!(
            result,
            Err(AdapterProtocolError::RequiredCapabilityMissing(_))
        ));
    }

    #[test]
    fn oversized_output_is_rejected_from_its_header_before_payload_allocation() {
        let mut header = [0_u8; 16];
        header[..8].copy_from_slice(OUTPUT_MAGIC);
        header[8..].copy_from_slice(&1_024_u64.to_be_bytes());
        let result = read_adapter_output(&mut Cursor::new(header), 8);
        assert!(matches!(result, Err(AdapterProtocolError::OutputTooLarge)));
    }

    #[test]
    fn checked_frames_round_trip_with_domain_separated_digests() {
        let mut encoded = Vec::new();
        let written = write_checked_frame(
            &mut encoded,
            OUTPUT_MAGIC,
            OUTPUT_CONTEXT,
            b"test-payload",
            1_024,
        );
        assert!(written.is_ok());
        let decoded = read_adapter_output(&mut Cursor::new(encoded), 1_024);
        assert_eq!(decoded, Ok(b"test-payload".to_vec()));
    }
}
