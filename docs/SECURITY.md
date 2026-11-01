# Security specification (normative)

This file is the source of truth for crypto. Any change needs a `DECISIONS.md` entry, new test vectors and a format version bump. Have it reviewed by the security lead and the external auditor before launch.

## Primitives

| Purpose | Algorithm | Crate |
| --- | --- | --- |
| Symmetric AEAD | XChaCha20-Poly1305 (24-byte random nonce) | `chacha20poly1305` |
| Password KDF | Argon2id, m = 64 MiB, t = 3, p = 4 (mobile floor m = 32 MiB, t = 4) | `argon2` |
| Key derivation | HKDF-SHA256 | `hkdf` |
| Key agreement (sharing) | X25519 | `x25519-dalek` |
| Signatures | Ed25519 | `ed25519-dalek` |
| Server auth | OPAQUE (RFC 9807), ristretto255 | `opaque-ke` |
| RNG | OS CSPRNG | `rand_core::OsRng` / `getrandom` |
| Memory hygiene | zeroize on drop | `zeroize`, `secrecy` |

KDF parameters are stored per account so they can be raised later without breaking old accounts.

## Key hierarchy

```
master_password ──Argon2id(salt_pw, account params)──▶ pw_key (32B)
secret_key (128-bit, random, shown once as "Emergency Kit")
    sk_key = HKDF(ikm=secret_key, salt=account_id, info="pwm/sk/v1")
account_unlock_key (AUK) = HKDF(ikm=pw_key XOR sk_key, salt=account_id, info="pwm/auk/v1")

AUK ──decrypts──▶ account_keyset {
    sym_key       (32B random)        # encrypts vault keys
    x25519 keypair                     # receives shared vault keys
    ed25519 keypair                    # signs shares, device approvals
}
sym_key ──decrypts──▶ vault_key[n] (32B random per vault)
vault_key ──decrypts──▶ item blobs (each item: own random nonce, AAD below)
```

- Every envelope is bound to its context by AAD (see Encodings below); for items the fields are `version || vault_id || item_id || revision`.
- The OPAQUE password input is derived separately: `HKDF(ikm=pw_key XOR sk_key, salt=account_id, info="pwm/auth/v1")`, so the server-auth secret and AUK are independent.
- New device login needs master password + Secret Key (or approval from an existing device that transfers the Secret Key over an authenticated X25519 channel).
- Biometric unlock: AUK (or sym_key) wrapped by a platform key (Secure Enclave / Android Keystore with `setUserAuthenticationRequired` / Windows Hello / TPM). Never store the master password.

## Encodings (format v1)

Decisions D10–D15. All HKDF calls are HKDF-SHA256 with 32-byte output. All integers are big-endian. Ids (`account_id`, `vault_id`, `item_id`) are 16 random bytes (UUIDv4).

**Envelope (D10).** `v:u8 || nonce:24 || ciphertext || tag:16`. `v = 1` is XChaCha20-Poly1305 with a fresh random nonce per encryption and fully determines the layout. Decoders reject any other `v` and any input shorter than 41 bytes. In JSON the envelope is base64url without padding.

**AAD (D11).** `len(label):u8 || label (ASCII) || fields`:

| Context | Label | Fields |
| --- | --- | --- |
| Item blob under vault_key | `pwm/item` | `version:u8 || vault_id || item_id || revision:u64` |
| Vault key under sym_key | `pwm/vault-key` | `version:u8 || vault_id` |
| Account keyset under AUK | `pwm/keyset` | `version:u8 || account_id` |
| Vault key shared to a recipient (D17) | `pwm/share` | `version:u8 \|\| vault_id \|\| sender_ed25519_pub:32 \|\| recipient_x25519_pub:32` |
| Export file `.pwmx` (D24) | `pwm/export` | `version:u8 \|\| m:u32 \|\| t:u32 \|\| p:u32 \|\| salt:16` |

A new kind of envelope gets a new label; labels are never reused.

**Password salt (D12).** `salt_pw` is 16 random bytes generated at signup and stored server-side with the KDF params. The pre-login endpoint returns them; for an unknown email it returns `HMAC-SHA256(server_secret, email)[..16]` and the default params, so it does not reveal which accounts exist.

**Master password (D16).** Before Argon2id the password is prepared with the RFC 8265 OpaqueString rules: non-ASCII space characters (U+00A0, U+1680, U+2000–U+200A, U+202F, U+205F, U+3000) become U+0020, the result is normalized to Unicode NFC, case is kept, nothing is trimmed, and empty passwords or passwords containing control characters are rejected. The KDF input is the UTF-8 encoding.

**Account keyset plaintext (v1).** `version:u8 (= 1) || sym_key:32 || x25519_secret:32 || ed25519_seed:32` (97 bytes), sealed under the AUK with AAD `pwm/keyset` (version 1, account_id). Public keys are derived from the secrets and are not stored inside it.

**Vault key plaintext (v1).** The 32 raw key bytes, sealed under `sym_key` with AAD `pwm/vault-key` (version 1, vault_id).

**Item record v1 (D18).** An item or folder is UTF-8 JSON (schema field `"v": 1`, see `core/pwm-vault`), padded to the next multiple of 256 bytes with one `0x80` byte then zeros (ISO/IEC 7816-4), and sealed under the vault key with AAD `pwm/item` (record version 1, vault_id, item_id, revision). The AAD version is the record format (padding + JSON container); the JSON `v` is the schema version inside it. Parser errors never carry input text, because they could quote secret values.

**History and trash (D19).** Previous versions (at most 10) and the `trashed_at` time live inside the encrypted item, so the server never learns how an item changed or what is in the trash. Clients purge items trashed for 30 days at unlock; the server sees only the resulting tombstone.

**Local cache (D20).** Each device caches only what the server also holds: account id, salt, KDF params, sealed keyset, wrapped vault keys, item envelopes with revisions, plus local sync state. The Secret Key and unlocked keys are never written to it (platform secure storage only). Deleted rows are overwritten (`secure_delete`); the schema is versioned and a newer or foreign database is refused.

**Health checks (D22).** Reuse is found by HMAC-SHA256 under a random key made per report run and dropped afterwards. The breach check sends only the first 5 hex characters of each password's SHA-1 straight from the device to the HIBP range API (`Add-Padding: true`, no cookies or account ids), never through our server; SHA-1 is used for nothing else. Results stay in memory.

**Export (D24).** Every export, encrypted or plaintext, needs the master password again: the core exports only with a single-use grant, valid for 5 minutes, made by re-deriving the AUK and opening the sealed keyset. A `.pwmx` file is `"PWMX" || v:u8 (= 1) || m:u32 || t:u32 || p:u32 || salt:16 || envelope`. Its key is `HKDF(ikm = Argon2id(export password, salt, m/t/p), salt = salt, info = "pwm/export/v1")`. The export password is normalized as in D16 and must score ≥ 3 in zxcvbn, and the params must pass the account KDF bounds. The envelope (AAD `pwm/export`, binding the header) holds the JSON export padded to a multiple of 4 KiB (ISO/IEC 7816-4). The JSON export holds each item's D18 JSON unchanged plus its id. The CSV export holds logins, notes, cards and identities that are not in the trash, without history, passkeys or custom fields. The server has no export endpoint.

**Secret Key text (D14).** `A1-` prefix (format version), then the 128-bit key as 26 Crockford base32 characters (most significant bit first, top two bits zero) plus one Crockford mod-37 check character, grouped `A1-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXX`. Input is case-insensitive, ignores `-` and spaces, and maps `O`→`0`, `I`/`L`→`1`. The account id is printed separately on the Emergency Kit.

**KDF parameters (D15).** Argon2id params (`m`, `t`, `p`, version) are stored per account and are identical on every client. Default at signup m = 64 MiB, t = 3, p = 4; the m = 32 MiB, t = 4 floor only when the signup device cannot allocate 64 MiB. Single-threaded clients (WASM) compute the same `p = 4` sequentially. Memory-limited contexts (iOS AutoFill extension) never run the KDF; they use the biometric-wrapped key.

## Recovery

- Emergency Kit (PDF/printable): account email, Secret Key, QR, blank for master password.
- Phase 2: emergency access — trusted contact receives vault keys wrapped to their X25519 key, released only after a user-set delay (1–30 days) with no veto.
- No vendor recovery backdoor. Lost master password + no emergency contact = lost vault; the UI says so clearly at signup.

## Sharing

- Vault share: vault_key encrypted to the recipient's X25519 public key and signed with the sharer's Ed25519 key (decision D17, below); the recipient verifies the sharer's key fingerprint on first share (optional out-of-band check).

**Signatures (D17).** Ed25519 over `len(label):u8 || label || message`, verified in strict mode (rejects malleable signatures and small-order public keys). Labels are never reused; current label: `pwm/share-sig`.

**Vault share v1 (D17).**

```
eph_secret, eph_pub = fresh X25519 key pair (one per share)
dh        = X25519(eph_secret, recipient_x25519_pub)        # all-zero result rejected
share_key = HKDF(ikm = dh, salt = eph_pub || recipient_x25519_pub, info = "pwm/share/v1")
envelope  = seal(share_key, vault_key, AAD "pwm/share": version:u8 || vault_id || sender_ed25519_pub || recipient_x25519_pub)
signature = Sign(sender, "pwm/share-sig", version:u8 || vault_id || recipient_x25519_pub || eph_pub || envelope)
wire      = v:u8 (= 1) || eph_pub:32 || signature:64 || envelope
```

The recipient verifies the signature with the sender's Ed25519 public key before computing `dh` or decrypting.
- One-time link to non-users: random 32B link key in the URL fragment (never sent to server), item encrypted with it; server enforces expiry and max views; link can be revoked.

## Threat model (summary)

| Threat | In scope | Mitigation |
| --- | --- | --- |
| Server breach / malicious insider | Yes | Zero-knowledge; Secret Key makes stolen blobs infeasible to brute-force |
| Network attacker | Yes | TLS 1.3, certificate pinning on mobile/desktop, OPAQUE (no password on wire) |
| Malicious website (phishing, clickjacking) | Yes | Exact-origin matching, user-initiated fill, no DOM-injected clickable UI (see `AUTOFILL.md`) |
| Malicious extension in same browser | Partial | Minimal permissions, isolated UI, no secrets in DOM longer than needed |
| Compromised unlocked device / malware | No (document it) | Auto-lock, clipboard clearing (30s), memory zeroize |
| Lost/stolen locked device | Yes | Full-disk + vault encryption, biometric bound to hardware keys, remote device revoke |
| Weak master password | Partial | Secret Key; zxcvbn ≥ 3 required at signup |

## Security program

- Bug bounty from public beta. `SECURITY.md` + `security.txt` in repo and website.
- External audits before public launch (crypto + core, server, apps, extensions); reports published.
- Closed source (S6), so trust comes from evidence: a public security whitepaper describing this spec, independent audit summaries published, and signed releases on every platform.
- Auditors get full source access under NDA.
- Dependency audit (`cargo audit`, `cargo deny`, `npm audit`) in CI, blocking on high severity.
