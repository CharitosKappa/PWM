# Test vectors

JSON vectors for every crypto and format operation in `docs/SECURITY.md`. They freeze the v1 byte formats: a change that breaks a vector is a format change and needs a decision, a version bump and a migration.

| File | Operation |
| --- | --- |
| `aad.json` | AAD byte layouts (D11, D17) |
| `aead.json` | XChaCha20-Poly1305 v1 envelopes (D10) |
| `hkdf.json` | HKDF-SHA256, 32-byte output (D13) |
| `kdf.json` | Argon2id password key (D12, D15) |
| `password.json` | Master password normalization (D16); `normalized: null` means rejected |
| `secret_key.json` | Secret Key text format (D14) |
| `account.json` | pw_key → sk_key → AUK / auth secret, sealed keyset v1 |
| `vault_key.json` | Vault key wrapped under sym_key |
| `signature.json` | Domain-separated Ed25519 signatures (D17) |
| `share.json` | X25519 vault share v1 (D17) |
| `export.json` | Password-protected `.pwmx` export file v1: header, export key, AAD and envelope (D24) |
| `device_name.json` | Device names padded to 64 bytes and sealed under sym_key v1 (D26) |
| `opaque.json` | OPAQUE registration and login transcript with seeded randomness (D26); Rust checker only, the protocol is covered by the RFC 9807 vectors in `opaque-ke` |
| `totp.json` | RFC 4226 HOTP, RFC 6238 TOTP and Steam Guard codes from stored URIs (D23); RFC values copied from the RFC appendices |

All byte strings are lowercase hex; revisions are decimal strings (JSON numbers above 2^53 lose precision in JavaScript).

## Checks (`just test-vectors`)

- `core/pwm-crypto/tests/vectors.rs` checks every vector against the Rust core and flips every bit of every ciphertext, key envelope, share and signature: each flip must be rejected.
- `verify.mjs` re-derives every vector except Argon2id with Node's built-in crypto, independently of the Rust code. It includes a small HChaCha20 (Node has only IETF ChaCha20-Poly1305), checked against the IETF draft vector first; it is verification tooling, never product code.

## Adding vectors

Add a generator to `core/pwm-crypto/examples/gen_vectors.rs` (`totp.json` is the exception: its values come from the RFCs) and run `cargo run -p pwm-crypto --example gen_vectors`. Existing files are never overwritten. Inputs are fixed; nonces and ephemeral keys are random and recorded.
