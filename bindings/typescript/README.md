# WorldDB JSON transport 1.0

This isolated TypeScript package defines precision-safe scalar DTOs inside the
versioned request-envelope shape from the transport contract. It does not choose
a desktop process model or add runtime dependencies.

The DTO uses a closed adjacent-tag form (`type` plus `data`), canonical UUID
text, canonical decimal strings for `i128`, `u128`, `Revision`, and `Decimal`,
and unpadded Base64url for `Bytes`. Decimal text and decoded Bytes values are
bounded by the 16 MiB resource policy; these are not persistent-format limits.
JSON envelopes are limited to 64 MiB measured as UTF-8 bytes.
The JSON parser rejects duplicate keys, unknown fields, malformed scalar text,
and unsupported protocol versions.

Run the isolated typecheck and golden/negative tests with Node.js 22.18 or newer:

```powershell
pnpm install --frozen-lockfile
pnpm verify
```

The exact JSON vectors live in `test/data/transport-v1.0-golden.tsv`.
