# M1-19 verification: TypeScript transport precision

**Status:** Local verification passed 2026-09-30.\
**Scope:** Versioned JSON request envelopes and exact transport of `i128`, `u128`, `Decimal`, `Revision`, UUID text, and bytes.\
**Runtime boundary:** This package is an isolated transport probe. It does not choose or implement the later desktop IPC process model.

## Implementation

`bindings/typescript` defines a closed protocol 1.0 request DTO with adjacent `type` and `data` tags. Large integers and revisions use canonical decimal strings; decimals use exact normalized text without a floating-point conversion. UUIDs require canonical lowercase RFC text, and bytes use unpadded canonical Base64url with the 16 MiB decoded-value limit checked before encoding or output allocation. JSON envelopes enforce the 64 MiB frame limit by counting UTF-8 bytes before parsing. A schema-specific preflight also bounds nesting, object/field/string counts and sizes, and rejects arrays before `JSON.parse` can allocate an arbitrary object graph. The parser rejects duplicate JSON keys, unknown fields, malformed scalar text, and unsupported protocol versions.

The fixed corpus in `bindings/typescript/test/data/transport-v1.0-golden.tsv` contains nine exact envelopes: signed 128-bit minimum and maximum, unsigned 128-bit maximum, negative maximum Decimal coefficient, scale-38 Decimal, genesis and maximum publishable Revision, UUID text, and Base64url bytes.

## Verification

Command:

```powershell
pnpm --dir bindings/typescript install --frozen-lockfile
pnpm --dir bindings/typescript verify
```

Results:

- Frozen dependency installation completed from `pnpm-lock.yaml`.
- `tsc --noEmit`: PASS.
- `node --test ./test/transport.test.ts`: 8 passed, 0 failed.
- `node ./test/transport_fuzz.ts --smoke`: PASS; one registered JSON-envelope decoder, nine golden envelopes, and all 12 malformed/oversize seed strategies exercised.
- Golden envelopes roundtrip exactly. Negative tests reject noncanonical and out-of-range integers, malformed Decimal text, over-budget Bytes and UTF-8 frame values, oversized JSON structure before parsing, noncanonical UUID/Base64url, duplicate keys, unknown fields, and unsupported protocol versions.

The contract and test vectors are local evidence only. Desktop IPC integration remains in M8.
