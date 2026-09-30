import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  PROTOCOL_V1,
  MAX_BYTES_VALUE,
  MAX_JSON_ENVELOPE_BYTES,
  TransportError,
  canonicalUuid,
  decodeBase64Url,
  decodeEnvelope,
  decimalFromCanonicalText,
  decimalToCanonicalText,
  encodeBase64Url,
  encodeEnvelope,
  type DecimalParts,
  type RequestEnvelopeV1,
  type WireScalar,
} from "../src/transport.ts";

const GOLDENS = readFileSync(new URL("./data/transport-v1.0-golden.tsv", import.meta.url), "utf8");
const REQUEST_ID = "00000000-0000-7000-8000-000000000002";

interface GoldenVector {
  readonly id: string;
  readonly type: string;
  readonly value: string;
  readonly json: string;
}

function vectors(): GoldenVector[] {
  const lines = GOLDENS.trimEnd().split(/\r?\n/);
  assert.equal(lines[0], "vector\ttype\tvalue\tjson");
  return lines.slice(1).map((line) => {
    const columns = line.split("\t");
    assert.equal(columns.length, 4);
    const [id, type, value, json] = columns;
    assert.ok(id && type && value !== undefined && json);
    return { id, type, value, json };
  });
}

function scalarFromVector(vector: GoldenVector): WireScalar {
  switch (vector.type) {
    case "i128":
      return { kind: "i128", value: BigInt(vector.value) };
    case "u128":
      return { kind: "u128", value: BigInt(vector.value) };
    case "decimal":
      return { kind: "decimal", value: decimalFromCanonicalText(vector.value) };
    case "revision":
      return { kind: "revision", value: BigInt(vector.value) };
    case "uuid":
      return { kind: "uuid", value: vector.value };
    case "bytes":
      return { kind: "bytes", value: decodeBase64Url(vector.value) };
    default:
      throw new Error(`unknown golden scalar type ${vector.type}`);
  }
}

function envelope(scalar: WireScalar): RequestEnvelopeV1 {
  return {
    protocol: PROTOCOL_V1,
    request_id: REQUEST_ID,
    operation: { type: "transport_probe", data: { scalar } },
  };
}

function expectTransportError(action: () => unknown, code: string): void {
  assert.throws(action, (error: unknown) => {
    assert.ok(error instanceof TransportError);
    assert.equal(error.code, code);
    return true;
  });
}

test("versioned scalar envelopes match every fixed JSON golden and round-trip exactly", () => {
  const all = vectors();
  assert.equal(all.length, 9);
  for (const vector of all) {
    const source = envelope(scalarFromVector(vector));
    assert.equal(encodeEnvelope(source), vector.json, vector.id);
    const decoded = decodeEnvelope(vector.json);
    assert.deepEqual(decoded, source, vector.id);
    assert.equal(encodeEnvelope(decoded), vector.json, vector.id);
  }
});

test("i128, u128, and Revision reject noncanonical or out-of-range strings", () => {
  const base = vectors()[0]?.json;
  assert.ok(base);
  expectTransportError(() => decodeEnvelope(base.replace(/"value":"-170[^\"]+"/, '"value":"-0"')), "InvalidScalar");
  expectTransportError(() => decodeEnvelope(base.replace(/"value":"-170[^\"]+"/, '"value":"01"')), "InvalidScalar");
  const u128 = vectors().find((vector) => vector.id === "u128_max");
  assert.ok(u128);
  expectTransportError(
    () => decodeEnvelope(u128.json.replace(/"value":"[0-9]+"/, '"value":"340282366920938463463374607431768211456"')),
    "InvalidScalar",
  );
  const revision = vectors().find((vector) => vector.id === "revision_max_publishable");
  assert.ok(revision);
  expectTransportError(
    () => decodeEnvelope(revision.json.replace(/"value":"[0-9]+"/, '"value":"18446744073709551615"')),
    "InvalidScalar",
  );
});

test("Decimal text preserves normalized coefficient, sign, and scale without floats", () => {
  const values = [
    "-340282366920938463463374607431768211455",
    "0.00000000000000000000000000000000000001",
    "123456789012345678901234567890123456789",
  ];
  for (const text of values) {
    const parts: DecimalParts = decimalFromCanonicalText(text);
    assert.equal(decimalToCanonicalText(parts), text);
    const decoded = decodeEnvelope(encodeEnvelope(envelope({ kind: "decimal", value: parts })));
    assert.deepEqual(decoded.operation.data.scalar, { kind: "decimal", value: parts });
  }
  for (const invalid of ["-0", "01", "1.0", "1e3", "+1", "0.00"]) {
    expectTransportError(() => decimalFromCanonicalText(invalid), "InvalidScalar");
  }
});

test("UUID and Base64url text are canonical and preserve exact bytes", () => {
  assert.equal(canonicalUuid(REQUEST_ID), REQUEST_ID);
  for (const invalid of [
    "00000000-0000-7000-8000-00000000000A",
    "00000000-0000-9000-8000-000000000001",
    "00000000-0000-7000-7000-000000000001",
    "00000000-0000-0000-0000-000000000000",
    "ffffffff-ffff-ffff-ffff-ffffffffffff",
  ]) {
    expectTransportError(() => canonicalUuid(invalid), "InvalidScalar");
  }
  assert.equal(encodeBase64Url(new Uint8Array([0xfb, 0xff])), "-_8");
  assert.deepEqual(decodeBase64Url("-_8"), new Uint8Array([0xfb, 0xff]));
  assert.equal(encodeBase64Url(new Uint8Array()), "");
  for (const invalid of ["AQ==", "A", "_x", "a+b", "a/b"]) {
    expectTransportError(() => decodeBase64Url(invalid), "InvalidScalar");
  }
});

test("Bytes enforce the 16 MiB resource policy before encoding or allocation", () => {
  const oversizedBytes = new Uint8Array(MAX_BYTES_VALUE + 1);
  const maxEncodedLength = Math.floor((MAX_BYTES_VALUE * 4 + 2) / 3);
  expectTransportError(() => encodeBase64Url(oversizedBytes), "InvalidScalar");
  expectTransportError(() => decodeBase64Url("A".repeat(maxEncodedLength + 1)), "InvalidScalar");
});

test("JSON envelopes enforce the 64 MiB UTF-8 frame limit", () => {
  const multibytePayload = "€".repeat(Math.ceil(MAX_JSON_ENVELOPE_BYTES / 3));
  const oversizedJson = `{"padding":"${multibytePayload}"}`;
  assert.ok(oversizedJson.length < MAX_JSON_ENVELOPE_BYTES);
  assert.throws(() => decodeEnvelope(oversizedJson), (error: unknown) => {
    assert.ok(error instanceof TransportError);
    assert.equal(error.code, "InvalidRequest");
    assert.equal(error.message, "JSON envelope exceeds the transport resource policy");
    return true;
  });
});

test("JSON preflight bounds parser structure before JSON.parse", () => {
  assert.throws(() => decodeEnvelope("[]"), (error: unknown) => {
    assert.ok(error instanceof TransportError);
    assert.equal(error.code, "InvalidRequest");
    assert.equal(error.message, "JSON arrays are not part of the transport envelope");
    return true;
  });
  expectTransportError(
    () => decodeEnvelope('{"a":0,"b":0,"c":0,"d":0}'),
    "InvalidRequest",
  );

  let nested = "0";
  for (let index = 0; index < 17; index += 1) nested = `{"a":${nested}}`;
  assert.throws(() => decodeEnvelope(nested), (error: unknown) => {
    assert.ok(error instanceof TransportError);
    assert.equal(error.code, "InvalidRequest");
    assert.equal(error.message, "JSON envelope has too many objects");
    return true;
  });

  const maxStringChars = Math.floor((MAX_BYTES_VALUE * 4 + 2) / 3);
  const oversizedString = `{"value":"${"A".repeat(maxStringChars + 1)}"}`;
  assert.throws(() => decodeEnvelope(oversizedString), (error: unknown) => {
    assert.ok(error instanceof TransportError);
    assert.equal(error.code, "InvalidRequest");
    assert.equal(error.message, "JSON string exceeds the transport resource policy");
    return true;
  });
});

test("closed envelopes reject duplicates, unknown fields, and unsupported versions", () => {
  const json = vectors()[0]?.json;
  assert.ok(json);
  expectTransportError(() => decodeEnvelope(json.replace('"major":1,', '"major":1,"\\u006dajor":1,')), "DuplicateJsonKey");
  expectTransportError(() => decodeEnvelope(json.replace('"protocol":{', '"protocol":{"extra":0,')), "InvalidRequest");
  expectTransportError(() => decodeEnvelope(json.replace('"major":1', '"major":2')), "UnsupportedProtocolVersion");
  expectTransportError(() => decodeEnvelope(json.replace('"request_id":"', '"unknown":0,"request_id":"')), "InvalidRequest");
});
