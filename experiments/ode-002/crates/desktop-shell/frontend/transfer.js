const PROTOCOL_VERSION = 1;
const CHUNK_BYTES = 256 * 1024;

export async function transferBytes(invoke, sessionId, bytes, signal, onProgress = () => {}) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength === 0) {
    throw new TypeError("Transferdaten müssen ein nichtleeres Uint8Array sein.");
  }

  const transfer = await invoke("begin_transfer", {
    sessionId,
    request: {
      protocol_version: PROTOCOL_VERSION,
      total_bytes: bytes.byteLength,
      chunk_bytes: CHUNK_BYTES,
    },
  });
  if (transfer.protocol_version !== PROTOCOL_VERSION) {
    throw new Error("Unsupported protocol version");
  }

  let finished = false;
  let bytesSent = 0;
  try {
    for (let sequence = 0; bytesSent < bytes.byteLength; sequence += 1) {
      if (signal?.aborted) {
        const result = await invoke("finish_transfer", {
          sessionId,
          request: {
            protocol_version: PROTOCOL_VERSION,
            transfer_id: transfer.transfer_id,
            cancelled: true,
          },
        });
        finished = true;
        return result;
      }

      const end = Math.min(bytesSent + transfer.max_chunk_bytes, bytes.byteLength);
      const chunk = bytes.subarray(bytesSent, end);
      const acknowledgement = await invoke("push_transfer_chunk", chunk, {
        headers: {
          "x-worlddb-protocol-version": String(PROTOCOL_VERSION),
          "x-worlddb-session": sessionId,
          "x-worlddb-transfer": transfer.transfer_id,
          "x-worlddb-sequence": String(sequence),
        },
      });
      if (
        acknowledgement.protocol_version !== PROTOCOL_VERSION ||
        acknowledgement.sequence !== sequence ||
        acknowledgement.bytes_received !== end
      ) {
        throw new Error("Invalid transfer acknowledgement");
      }
      bytesSent = end;
      onProgress(bytesSent, bytes.byteLength);
    }

    const result = await invoke("finish_transfer", {
      sessionId,
      request: {
        protocol_version: PROTOCOL_VERSION,
        transfer_id: transfer.transfer_id,
        cancelled: false,
      },
    });
    finished = true;
    return result;
  } finally {
    if (!finished) {
      await invoke("finish_transfer", {
        sessionId,
        request: {
          protocol_version: PROTOCOL_VERSION,
          transfer_id: transfer.transfer_id,
          cancelled: true,
        },
      }).catch(() => {});
    }
  }
}
