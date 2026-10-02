# M7-07 storage-format fixtures

These fixed 72-byte `FORMAT` probe files are the first pre-release storage-format baseline. There is no published Alpha and therefore no genuine released N-1 database yet. The v1.0 probe becomes an N-1 input only after a later storage profile is released; it is not recorded as an N-1 compatibility pass.

| File | Meaning | Expected open behavior | SHA-256 |
|---|---|---|---|
| `format-v1.0.bin` | Current WorldDB frame 1.0 with no required or optional capabilities | Opens read-only; bytes stay unchanged | `ABA3763318D243BD554B3A8EB5EE88C646756DFB0CE0F09667995D7FB318C2C4` |
| `format-v2.0-unsupported.bin` | Validly checksummed future major version | Rejects with `UnsupportedVersion`; bytes stay unchanged | `68ED0C42EE9EC387CCFC848FD87B42BB33177DE677DF222362662BDB486EC207` |
| `format-v1.0-unknown-required-capability.bin` | Frame 1.0 declaring required capability bit 0 | Rejects with `UnsupportedRequiredCapabilities`; bytes stay unchanged | `874FBF1B7F675E6689FA3E0E83DCCC0E806AAB379F8217D6884FBA4CBD387D18` |

The current positive fixture pins the root probe only. Component-specific format and corruption tests remain the source for segment, manifest, WAL, audit, recovery-journal, and index encodings. A complete database captured from the first published Alpha must be added and exercised for open, upgrade, exact backup, restore, and logical export at M9-02.
