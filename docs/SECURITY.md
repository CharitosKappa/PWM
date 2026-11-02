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
| Device name under sym_key (D26) | `pwm/device-name` | `version:u8 \|\| account_id \|\| device_id` |
| Server login state under the login-state key (D26) | `pwm/login-state` | `version:u8 \|\| account_id` |
| Unlocked keyset in the extension's session store (D34) | `pwm/session` | `version:u8 \|\| account_id` |
| Device tokens on disk under sym_key (D34) | `pwm/device-tokens` | `version:u8 \|\| account_id` |

A new kind of envelope gets a new label; labels are never reused.

**Extension session (D34).** Chrome stops an idle service worker and wipes its memory. So that the extension stays unlocked, the core seals the account keyset plaintext under a fresh random 32-byte key (AAD `pwm/session`, version 1, bound to the account). The blob `v:u8 (= 1) || account_id:16 || envelope` and its key go only into `chrome.storage.session`: memory-only, cleared when the browser closes, and not readable by content scripts. Locking, sign-out and a revoked device clear it. On disk the extension keeps the cache snapshot (ciphertext and ids), the Secret Key as is (D13: useless without the master password), and the access and refresh tokens as `v:u8 (= 1) || envelope(JSON {access, refresh})` sealed under `sym_key` (AAD `pwm/device-tokens`), so they are usable only after unlock. Never the master password, the keyset or any unlocked key.

**Password salt (D12).** `salt_pw` is 16 random bytes generated at signup and stored server-side with the KDF params. The pre-login endpoint returns them; for an unknown email it returns `HMAC-SHA256(server_secret, email)[..16]` and the default params, so it does not reveal which accounts exist.

**Master password (D16).** Before Argon2id the password is prepared with the RFC 8265 OpaqueString rules: non-ASCII space characters (U+00A0, U+1680, U+2000–U+200A, U+202F, U+205F, U+3000) become U+0020, the result is normalized to Unicode NFC, case is kept, nothing is trimmed, and empty passwords or passwords containing control characters are rejected. The KDF input is the UTF-8 encoding.

**Account keyset plaintext (v1).** `version:u8 (= 1) || sym_key:32 || x25519_secret:32 || ed25519_seed:32` (97 bytes), sealed under the AUK with AAD `pwm/keyset` (version 1, account_id). Public keys are derived from the secrets and are not stored inside it.

**Vault key plaintext (v1).** The 32 raw key bytes, sealed under `sym_key` with AAD `pwm/vault-key` (version 1, vault_id).

**Item record v1 (D18).** An item or folder is UTF-8 JSON (schema field `"v": 1`, see `core/pwm-vault`), padded to the next multiple of 256 bytes with one `0x80` byte then zeros (ISO/IEC 7816-4), and sealed under the vault key with AAD `pwm/item` (record version 1, vault_id, item_id, revision). The AAD version is the record format (padding + JSON container); the JSON `v` is the schema version inside it. Parser errors never carry input text, because they could quote secret values.

**History and trash (D19).** Previous versions (at most 10) and the `trashed_at` time live inside the encrypted item, so the server never learns how an item changed or what is in the trash. Clients purge items trashed for 30 days at unlock; the server sees only the resulting tombstone.

**Local cache (D20).** Each device caches only what the server also holds: account id, salt, KDF params, sealed keyset, wrapped vault keys, item envelopes with revisions, plus local sync state. The Secret Key and unlocked keys are never written to it (platform secure storage only). Deleted rows are overwritten (`secure_delete`); the schema is versioned and a newer or foreign database is refused.

**Health checks (D22).** Reuse is found by HMAC-SHA256 under a random key made per report run and dropped afterwards. The breach check sends only the first 5 hex characters of each password's SHA-1 straight from the device to the HIBP range API (`Add-Padding: true`, no cookies or account ids), never through our server; SHA-1 is used for nothing else. Results stay in memory.

**Export (D24).** Every export, encrypted or plaintext, needs the master password again: the core exports only with a single-use grant, valid for 5 minutes, made by re-deriving the AUK and opening the sealed keyset. A `.pwmx` file is `"PWMX" || v:u8 (= 1) || m:u32 || t:u32 || p:u32 || salt:16 || envelope`. Its key is `HKDF(ikm = Argon2id(export password, salt, m/t/p), salt = salt, info = "pwm/export/v1")`. The export password is normalized as in D16 and must score ≥ 3 in zxcvbn, and the params must pass the account KDF bounds. The envelope (AAD `pwm/export`, binding the header) holds the JSON export padded to a multiple of 4 KiB (ISO/IEC 7816-4). The JSON export holds each item's D18 JSON unchanged plus its id. The CSV export holds logins, notes, cards and identities that are not in the trash, without history, passkeys or custom fields. The server has no export endpoint.

**Server login (D26).** OPAQUE (RFC 9807) via `opaque-ke` 4.0: OPRF ristretto255-SHA512, 3DH on ristretto255 with SHA-512, KSF = Identity (the input `auth_secret` is already Argon2id-stretched and mixed with the Secret Key), context `"pwm/opaque/v1"`, client identifier and credential identifier = `account_id`, server identifier = the server's OPAQUE public key. Both protocol sides live in `pwm-crypto`. The server's long-term secrets are one blob `v:u8 (= 1) || ServerSetup || server_key:32` (base64url in `PWM_SERVER_SECRETS`); losing the ServerSetup locks every account out, so it is kept in a secret manager with a backup and is not rotated in v1. Two keys are derived from `server_key` with HKDF-SHA256 (empty salt): `prelogin_key` (info `pwm/prelogin/v1`) and `login_state_key` (info `pwm/login-state/v1`). D12's fake pre-login answers are `HMAC-SHA256(prelogin_key, len(label) || label || email)` with labels `pwm/prelogin/id` and `pwm/prelogin/salt`, the id shaped as UUIDv4, with the default params. The OPAQUE server state kept between login start and finish holds the MAC the client must send, so it is stored sealed under `login_state_key` (AAD `pwm/login-state`, bound to the account): reading the database does not let anyone finish a login in progress. Unknown accounts get `opaque-ke`'s fake credential response; every login failure returns the same error.

**Session tokens (D26).** 32 random bytes, base64url with a `pwma_` (access, 15 minutes) or `pwmr_` (refresh, 90 days unused, replaced on every use) prefix. The server stores only SHA-256 hashes. Presenting a replaced refresh token ends that session. One session per device; revoking a device ends its session at once: its access and refresh tokens are then answered with 403 `device_revoked`, and the sync engine clears the local cache (D28).

**Secrets at the binding boundary (D31).** The master password, Secret Key, tokens and revealed item fields cross into Swift, Kotlin and JavaScript as platform strings, which the core cannot wipe. The core wipes its own copies (`Secret<T>`). Apps must keep such strings short-lived, never log them, and store the Secret Key and tokens only in secure storage (Keychain, Keystore, Windows Hello/TPM, or for the extension the encrypted session store). Requests the core hands out can carry an access or refresh token, so the platform's HTTP code must not log headers or bodies.

**Device name (D26).** The UTF-8 name (at most 63 bytes, no control characters) padded to exactly 64 bytes (ISO/IEC 7816-4), sealed under `sym_key` with AAD `pwm/device-name` (version 1, account_id, device_id), so every sealed name has the same size.

**Secret Key text (D14).** `A1-` prefix (format version), then the 128-bit key as 26 Crockford base32 characters (most significant bit first, top two bits zero) plus one Crockford mod-37 check character, grouped `A1-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXX`. Input is case-insensitive, ignores `-` and spaces, and maps `O`→`0`, `I`/`L`→`1`. The account id is printed separately on the Emergency Kit.

**KDF parameters (D15).** Argon2id params (`m`, `t`, `p`, version) are stored per account and are identical on every client. Default at signup m = 64 MiB, t = 3, p = 4; the m = 32 MiB, t = 4 floor only when the signup device cannot allocate 64 MiB. Single-threaded clients (WASM) compute the same `p = 4` sequentially. Memory-limited contexts (iOS AutoFill extension) never run the KDF; they use the biometric-wrapped key.

## Recovery

- Emergency Kit (PDF/printable): account email, Secret Key, QR, blank for master password.
- Phase 2: emergency access — trusted contact receives vault keys wrapped to their X25519 key, released only after a user-set delay (1–30 days) with no veto.
- No vendor recovery backdoor. Lost master password + no emergency contact = lost vault; the UI says so clearly at signup.

## Sharing

- Vault share: vault_key encrypted to the recipient's X25519 public key and signed with the sharer's Ed25519 key (decision D17, below); the recipient verifies the sharer's key fingerprint on first share (optional out-of-band check).

**Passkeys (D37).** The extension (and later the apps) acts as a WebAuthn authenticator for ES256 only, through `pwm-passkey` (RustCrypto `p256`).

- **Relying party id:** it must be the calling origin's host or a registrable suffix of it, never a public suffix or an IP address. Only HTTPS and `localhost` origins qualify.
- **The origin** is the browser's report of the calling frame. `clientDataJSON` is built in the core from it, with `crossOrigin`/`topOrigin` for same-site frames. Frames of another site never get our passkeys.
- **`authenticatorData`:** flags UP, UV, BE and BS (plus AT on create), a sign count of 0 always (synced passkeys), and AAGUID `6c9e3d41-8a27-4f0b-9d52-e17730c458a6`.
- **Attestation:** `fmt: "none"`. The COSE key and the attestation object are hand-written deterministic CBOR.
- **User verification** means the vault is unlocked and the user confirmed in an extension window (never page DOM).
- **Keys:** private keys are PKCS#8 inside the login item (D18), so they are encrypted like any item, and the server sees only ciphertext.

**Signatures (D17).** Ed25519 over `len(label):u8 || label || message`, verified in strict mode (rejects malleable signatures and small-order public keys). Labels are never reused; current labels: `pwm/share-sig` (vault shares) and `pwm/autofill-rules` (the signed autofill rules file, D33: `v:u8 (= 1) || key_id:u8 || signature:64 || json`, signature over `key_id || json`, release keys pinned in the apps, the signing key kept offline).

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
