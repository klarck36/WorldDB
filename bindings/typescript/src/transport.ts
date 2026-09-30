/** Precision-safe JSON transport DTOs for the version 1.0 scalar probe. */

export const PROTOCOL_V1 = { major: 1, minor: 0 } as const;
export const MAX_JSON_ENVELOPE_BYTES = 64 * 1024 * 1024;
export const MAX_DECIMAL_TEXT_CHARS = 16 * 1024 * 1024;
export const MAX_BYTES_VALUE = 16 * 1024 * 1024;
const MAX_BASE64URL_CHARS = Math.floor((MAX_BYTES_VALUE * 4 + 2) / 3);

const I128_MIN = -(1n << 127n);
const I128_MAX = (1n << 127n) - 1n;
const U128_MAX = (1n << 128n) - 1n;
const U64_MAX = (1n << 64n) - 1n;
const I32_MIN = -(2 ** 31);
const I32_MAX = 2 ** 31 - 1;
const MAX_JSON_NESTING = 64;
const MAX_JSON_OBJECTS = 16;
const MAX_JSON_OBJECT_FIELDS = 3;
const MAX_JSON_PROPERTIES = 32;
const MAX_JSON_STRING_TOKENS = 24;
const MAX_JSON_KEY_CHARS = 128;
const MAX_JSON_ATOM_CHARS = 32;

export type TransportErrorCode =
  | "MalformedJson"
  | "DuplicateJsonKey"
  | "InvalidRequest"
  | "UnsupportedProtocolVersion"
  | "InvalidScalar";

export class TransportError extends Error {
  readonly code: TransportErrorCode;

  constructor(code: TransportErrorCode, message: string) {
    super(message);
    this.name = "TransportError";
    this.code = code;
  }
}

export interface DecimalParts {
  readonly negative: boolean;
  readonly coefficient: bigint;
  readonly scale: number;
}

export type WireScalar =
  | { readonly kind: "i128"; readonly value: bigint }
  | { readonly kind: "u128"; readonly value: bigint }
  | { readonly kind: "decimal"; readonly value: DecimalParts }
  | { readonly kind: "revision"; readonly value: bigint }
  | { readonly kind: "uuid"; readonly value: string }
  | { readonly kind: "bytes"; readonly value: Uint8Array };

export interface RequestEnvelopeV1 {
  readonly protocol: typeof PROTOCOL_V1;
  readonly request_id: string;
  readonly operation: {
    readonly type: "transport_probe";
    readonly data: { readonly scalar: WireScalar };
  };
}

interface ProtocolVersionDto {
  readonly major: number;
  readonly minor: number;
}

type ScalarDto =
  | { readonly type: "i128"; readonly data: { readonly value: string } }
  | { readonly type: "u128"; readonly data: { readonly value: string } }
  | { readonly type: "decimal"; readonly data: { readonly value: string } }
  | { readonly type: "revision"; readonly data: { readonly value: string } }
  | { readonly type: "uuid"; readonly data: { readonly value: string } }
  | { readonly type: "bytes"; readonly data: { readonly value: string } };

interface RequestEnvelopeDtoV1 {
  readonly protocol: ProtocolVersionDto;
  readonly request_id: string;
  readonly operation: {
    readonly type: "transport_probe";
    readonly data: { readonly scalar: ScalarDto };
  };
}

function invalidScalar(message: string): never {
  throw new TransportError("InvalidScalar", message);
}

function invalidRequest(message: string): never {
  throw new TransportError("InvalidRequest", message);
}

function requireCanonicalIntegerText(value: string, signed: boolean, maxChars: number): bigint {
  if (value.length === 0 || value.length > maxChars) {
    return invalidScalar("integer text is outside the canonical range");
  }
  const grammar = signed ? /^(?:0|-?[1-9][0-9]*)$/ : /^(?:0|[1-9][0-9]*)$/;
  if (!grammar.test(value)) {
    return invalidScalar("integer text is not canonical decimal notation");
  }
  try {
    return BigInt(value);
  } catch {
    return invalidScalar("integer text is invalid");
  }
}

function normalizeDecimal(value: DecimalParts): DecimalParts {
  if (typeof value.negative !== "boolean" || typeof value.coefficient !== "bigint") {
    return invalidScalar("decimal parts have invalid types");
  }
  if (!Number.isInteger(value.scale) || value.scale < I32_MIN || value.scale > I32_MAX) {
    return invalidScalar("decimal scale is outside the i32 range");
  }
  if (value.coefficient < 0n || value.coefficient > U128_MAX) {
    return invalidScalar("decimal coefficient is outside the u128 range");
  }
  if (value.coefficient === 0n) {
    return { negative: false, coefficient: 0n, scale: 0 };
  }

  let coefficient = value.coefficient;
  let scale = value.scale;
  while (coefficient % 10n === 0n) {
    if (scale === I32_MIN) {
      return invalidScalar("decimal normalization exceeds the i32 scale range");
    }
    coefficient /= 10n;
    scale -= 1;
  }
  return { negative: value.negative, coefficient, scale };
}

export function decimalToCanonicalText(value: DecimalParts): string {
  const normalized = normalizeDecimal(value);
  if (normalized.coefficient === 0n) {
    return "0";
  }
  const digits = normalized.coefficient.toString(10);
  const signLength = normalized.negative ? 1 : 0;
  let outputLength: number;
  if (normalized.scale <= 0) {
    outputLength = digits.length - normalized.scale + signLength;
  } else if (normalized.scale >= digits.length) {
    outputLength = normalized.scale + 2 + signLength;
  } else {
    outputLength = digits.length + 1 + signLength;
  }
  if (outputLength > MAX_DECIMAL_TEXT_CHARS) {
    return invalidScalar("decimal text exceeds the transport resource policy");
  }

  let body: string;
  if (normalized.scale <= 0) {
    body = digits + "0".repeat(-normalized.scale);
  } else if (normalized.scale >= digits.length) {
    body = `0.${"0".repeat(normalized.scale - digits.length)}${digits}`;
  } else {
    const split = digits.length - normalized.scale;
    body = `${digits.slice(0, split)}.${digits.slice(split)}`;
  }
  return normalized.negative ? `-${body}` : body;
}

export function decimalFromCanonicalText(value: string): DecimalParts {
  if (value.length === 0 || value.length > MAX_DECIMAL_TEXT_CHARS) {
    return invalidScalar("decimal text is outside the transport resource policy");
  }
  if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/.test(value)) {
    return invalidScalar("decimal text is not plain canonical notation");
  }

  const negative = value.startsWith("-");
  const unsigned = negative ? value.slice(1) : value;
  const point = unsigned.indexOf(".");
  const integer = point < 0 ? unsigned : unsigned.slice(0, point);
  const fraction = point < 0 ? "" : unsigned.slice(point + 1);
  let digits = integer + fraction;
  const leadingZeroes = digits.match(/^0+/)?.[0].length ?? 0;
  digits = digits.slice(leadingZeroes);
  if (digits.length === 0) {
    const zero = { negative: false, coefficient: 0n, scale: 0 };
    if (decimalToCanonicalText(zero) !== value) {
      return invalidScalar("zero decimal text is not canonical");
    }
    return zero;
  }

  let trailingZeroes = 0;
  while (digits.endsWith("0")) {
    digits = digits.slice(0, -1);
    trailingZeroes += 1;
  }
  if (digits.length > 39) {
    return invalidScalar("decimal coefficient exceeds u128");
  }
  const coefficient = BigInt(digits);
  if (coefficient > U128_MAX) {
    return invalidScalar("decimal coefficient exceeds u128");
  }
  const scale = fraction.length - trailingZeroes;
  if (!Number.isInteger(scale) || scale < I32_MIN || scale > I32_MAX) {
    return invalidScalar("decimal scale is outside the i32 range");
  }
  const parsed = { negative, coefficient, scale };
  if (decimalToCanonicalText(parsed) !== value) {
    return invalidScalar("decimal text is not the unique canonical spelling");
  }
  return parsed;
}

export function canonicalUuid(value: string): string {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value)) {
    return invalidScalar("UUID text is not canonical RFC UUID text");
  }
  const compact = value.replaceAll("-", "");
  if (/^0{32}$/.test(compact) || /^f{32}$/.test(compact)) {
    return invalidScalar("UUID text uses a reserved sentinel");
  }
  return value;
}

const BASE64URL_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

export function encodeBase64Url(bytes: Uint8Array): string {
  if (bytes.length > MAX_BYTES_VALUE) {
    return invalidScalar("Bytes exceed the transport resource policy");
  }
  let output = "";
  for (let index = 0; index < bytes.length; index += 3) {
    const first = bytes[index] ?? 0;
    const hasSecond = index + 1 < bytes.length;
    const hasThird = index + 2 < bytes.length;
    const second = hasSecond ? bytes[index + 1] ?? 0 : 0;
    const third = hasThird ? bytes[index + 2] ?? 0 : 0;
    const combined = (first << 16) | (second << 8) | third;
    output += BASE64URL_ALPHABET[(combined >> 18) & 0x3f];
    output += BASE64URL_ALPHABET[(combined >> 12) & 0x3f];
    if (hasSecond) {
      output += BASE64URL_ALPHABET[(combined >> 6) & 0x3f];
    }
    if (hasThird) {
      output += BASE64URL_ALPHABET[combined & 0x3f];
    }
  }
  return output;
}

export function decodeBase64Url(value: string): Uint8Array {
  if (value.length > MAX_BASE64URL_CHARS) {
    return invalidScalar("Bytes exceed the transport resource policy");
  }
  if (value.includes("=") || value.length % 4 === 1 || !/^[A-Za-z0-9_-]*$/.test(value)) {
    return invalidScalar("Bytes are not unpadded Base64url");
  }
  const output = new Uint8Array(Math.floor((value.length * 3) / 4));
  let outputIndex = 0;
  for (let index = 0; index < value.length; index += 4) {
    const remaining = Math.min(4, value.length - index);
    let combined = 0;
    for (let offset = 0; offset < 4; offset += 1) {
      const char = offset < remaining ? value.charAt(index + offset) : "A";
      const digit = BASE64URL_ALPHABET.indexOf(char);
      combined = (combined << 6) | (digit < 0 ? 0 : digit);
    }
    if (outputIndex < output.length) output[outputIndex++] = (combined >> 16) & 0xff;
    if (remaining >= 3 && outputIndex < output.length) output[outputIndex++] = (combined >> 8) & 0xff;
    if (remaining >= 4 && outputIndex < output.length) output[outputIndex++] = combined & 0xff;
  }
  if (encodeBase64Url(output) !== value) {
    return invalidScalar("Bytes use a noncanonical Base64url spelling");
  }
  return output;
}

function scalarToDto(scalar: WireScalar): ScalarDto {
  switch (scalar.kind) {
    case "i128": {
      if (scalar.value < I128_MIN || scalar.value > I128_MAX) {
        return invalidScalar("i128 value is outside the signed 128-bit range");
      }
      return { type: "i128", data: { value: scalar.value.toString(10) } };
    }
    case "u128": {
      if (scalar.value < 0n || scalar.value > U128_MAX) {
        return invalidScalar("u128 value is outside the unsigned 128-bit range");
      }
      return { type: "u128", data: { value: scalar.value.toString(10) } };
    }
    case "decimal":
      return { type: "decimal", data: { value: decimalToCanonicalText(scalar.value) } };
    case "revision":
      if (scalar.value < 0n || scalar.value >= U64_MAX) {
        return invalidScalar("Revision is outside the publishable u64 range");
      }
      return { type: "revision", data: { value: scalar.value.toString(10) } };
    case "uuid":
      return { type: "uuid", data: { value: canonicalUuid(scalar.value) } };
    case "bytes":
      return { type: "bytes", data: { value: encodeBase64Url(scalar.value) } };
  }
}

function objectWithKeys(value: unknown, expected: readonly string[], path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return invalidRequest(`${path} must be an object`);
  }
  const object = value as Record<string, unknown>;
  const actual = Object.keys(object);
  if (actual.length !== expected.length || expected.some((key) => !Object.hasOwn(object, key))) {
    return invalidRequest(`${path} has missing or unknown fields`);
  }
  return object;
}

function requireString(value: unknown, path: string): string {
  if (typeof value !== "string") {
    return invalidScalar(`${path} must be a string`);
  }
  return value;
}

function dtoToScalar(value: unknown): WireScalar {
  const tagged = objectWithKeys(value, ["type", "data"], "scalar");
  const type = requireString(tagged.type, "scalar.type");
  const data = objectWithKeys(tagged.data, ["value"], "scalar.data");
  const text = requireString(data.value, "scalar.data.value");
  switch (type) {
    case "i128": {
      const parsed = requireCanonicalIntegerText(text, true, 40);
      if (parsed < I128_MIN || parsed > I128_MAX) return invalidScalar("i128 value is out of range");
      return { kind: "i128", value: parsed };
    }
    case "u128": {
      const parsed = requireCanonicalIntegerText(text, false, 39);
      if (parsed > U128_MAX) return invalidScalar("u128 value is out of range");
      return { kind: "u128", value: parsed };
    }
    case "decimal":
      return { kind: "decimal", value: decimalFromCanonicalText(text) };
    case "revision": {
      const parsed = requireCanonicalIntegerText(text, false, 20);
      if (parsed >= U64_MAX) return invalidScalar("Revision value is reserved or out of range");
      return { kind: "revision", value: parsed };
    }
    case "uuid":
      return { kind: "uuid", value: canonicalUuid(text) };
    case "bytes":
      return { kind: "bytes", value: decodeBase64Url(text) };
    default:
      return invalidScalar("scalar type tag is unknown");
  }
}

export function encodeEnvelope(envelope: RequestEnvelopeV1): string {
  if (envelope.protocol.major !== 1 || envelope.protocol.minor !== 0) {
    throw new TransportError("UnsupportedProtocolVersion", "protocol version is unsupported");
  }
  const requestId = canonicalUuid(envelope.request_id);
  const dto: RequestEnvelopeDtoV1 = {
    protocol: PROTOCOL_V1,
    request_id: requestId,
    operation: {
      type: "transport_probe",
      data: { scalar: scalarToDto(envelope.operation.data.scalar) },
    },
  };
  return JSON.stringify(dto);
}

function skipWhitespace(source: string, position: number): number {
  let cursor = position;
  while (/\s/.test(source.charAt(cursor)) && cursor < source.length) cursor += 1;
  return cursor;
}

function exceedsUtf8ByteLimit(value: string, limit: number): boolean {
  if (value.length > limit) return true;

  let byteLength = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code <= 0x7f) {
      byteLength += 1;
    } else if (code <= 0x7ff) {
      byteLength += 2;
    } else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        byteLength += 4;
        index += 1;
      } else {
        byteLength += 3;
      }
    } else {
      // TextEncoder replaces unpaired UTF-16 surrogates with U+FFFD (3 bytes).
      byteLength += 3;
    }
    if (byteLength > limit) return true;
  }
  return false;
}

function preflightJson(source: string): void {
  let cursor = 0;
  let objectCount = 0;
  let propertyCount = 0;
  let stringTokenCount = 0;

  const readString = (decode: boolean): string | undefined => {
    const start = cursor;
    if (source.charAt(cursor) !== '"') {
      throw new TransportError("MalformedJson", "JSON string is malformed");
    }
    cursor += 1;
    let decodedLength = 0;
    while (cursor < source.length) {
      const char = source.charAt(cursor);
      if (char === "\\") {
        cursor += source.charAt(cursor + 1) === "u" ? 6 : 2;
        decodedLength += 1;
      } else if (char === '"') {
        cursor += 1;
        stringTokenCount += 1;
        if (stringTokenCount > MAX_JSON_STRING_TOKENS) {
          return invalidRequest("JSON envelope contains too many string values");
        }
        if (!decode) return undefined;
        try {
          return JSON.parse(source.slice(start, cursor)) as string;
        } catch {
          throw new TransportError("MalformedJson", "JSON string is malformed");
        }
      } else {
        cursor += 1;
        decodedLength += 1;
      }
      const limit = decode ? MAX_JSON_KEY_CHARS : MAX_BASE64URL_CHARS;
      if (decodedLength > limit) {
        return invalidRequest(decode
          ? "JSON object key exceeds the transport resource policy"
          : "JSON string exceeds the transport resource policy");
      }
    }
    throw new TransportError("MalformedJson", "JSON string is unterminated");
  };

  const readValue = (depth: number): void => {
    if (depth > MAX_JSON_NESTING) {
      throw new TransportError("InvalidRequest", "JSON nesting exceeds the transport policy");
    }
    cursor = skipWhitespace(source, cursor);
    const char = source.charAt(cursor);
    if (char === '"') {
      readString(false);
      return;
    }
    if (char === "{") {
      objectCount += 1;
      if (objectCount > MAX_JSON_OBJECTS) {
        return invalidRequest("JSON envelope has too many objects");
      }
      cursor += 1;
      cursor = skipWhitespace(source, cursor);
      if (source.charAt(cursor) === "}") {
        cursor += 1;
        return;
      }
      const keys = new Set<string>();
      let objectFieldCount = 0;
      while (cursor < source.length) {
        cursor = skipWhitespace(source, cursor);
        if (source.charAt(cursor) !== '"') return;
        const key = readString(true);
        if (key === undefined) {
          throw new TransportError("MalformedJson", "JSON object key is malformed");
        }
        if (keys.has(key)) {
          throw new TransportError("DuplicateJsonKey", "JSON object contains a duplicate key");
        }
        keys.add(key);
        objectFieldCount += 1;
        propertyCount += 1;
        if (objectFieldCount > MAX_JSON_OBJECT_FIELDS || propertyCount > MAX_JSON_PROPERTIES) {
          return invalidRequest("JSON object has too many fields");
        }
        cursor = skipWhitespace(source, cursor);
        if (source.charAt(cursor) !== ":") return;
        cursor += 1;
        readValue(depth + 1);
        cursor = skipWhitespace(source, cursor);
        if (source.charAt(cursor) === "}") {
          cursor += 1;
          return;
        }
        if (source.charAt(cursor) !== ",") return;
        cursor += 1;
      }
      return;
    }
    if (char === "[") {
      return invalidRequest("JSON arrays are not part of the transport envelope");
    }
    const tokenStart = cursor;
    while (cursor < source.length && !/[\s,\]}]/.test(source.charAt(cursor))) cursor += 1;
    if (cursor - tokenStart > MAX_JSON_ATOM_CHARS) {
      return invalidRequest("JSON scalar token exceeds the transport resource policy");
    }
    if (cursor === 0) cursor += 1;
  };

  readValue(0);
}

export function decodeEnvelope(json: string): RequestEnvelopeV1 {
  if (exceedsUtf8ByteLimit(json, MAX_JSON_ENVELOPE_BYTES)) {
    throw new TransportError("InvalidRequest", "JSON envelope exceeds the transport resource policy");
  }
  preflightJson(json);
  let parsed: unknown;
  try {
    parsed = JSON.parse(json) as unknown;
  } catch {
    throw new TransportError("MalformedJson", "JSON envelope is malformed");
  }

  const envelope = objectWithKeys(parsed, ["protocol", "request_id", "operation"], "envelope");
  const protocol = objectWithKeys(envelope.protocol, ["major", "minor"], "protocol");
  if (protocol.major !== 1 || protocol.minor !== 0) {
    throw new TransportError("UnsupportedProtocolVersion", "protocol version is unsupported");
  }
  const requestId = canonicalUuid(requireString(envelope.request_id, "request_id"));
  const operation = objectWithKeys(envelope.operation, ["type", "data"], "operation");
  if (operation.type !== "transport_probe") {
    return invalidRequest("operation type is unknown");
  }
  const data = objectWithKeys(operation.data, ["scalar"], "operation.data");
  const scalar = dtoToScalar(data.scalar);
  return {
    protocol: PROTOCOL_V1,
    request_id: requestId,
    operation: { type: "transport_probe", data: { scalar } },
  };
}
