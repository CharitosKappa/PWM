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
master_password ──Argon2id(salt_pw)──▶ pw_key (32B)
secret_key (128-bit, random, shown once as "Emergency Kit") ──HKDF──▶ sk_key (32B)
account_unlock_key (AUK) = HKDF(pw_key XOR sk_key, info="pwm/auk/v1")

AUK ──decrypts──▶ account_keyset {
    sym_key       (32B random)        # encrypts vault keys
    x25519 keypair                     # receives shared vault keys
    ed25519 keypair                    # signs shares, device approvals
}
sym_key ──decrypts──▶ vault_key[n] (32B random per vault)
vault_key ──decrypts──▶ item blobs (each item: own random nonce, AAD below)
```

- AAD for items: `version || vault_id || item_id || revision`.
- The OPAQUE password input is derived separately: `HKDF(pw_key XOR sk_key, info="pwm/auth/v1")`, so the server-auth secret and AUK are independent.
- New device login needs master password + Secret Key (or approval from an existing device that transfers the Secret Key over an authenticated X25519 channel).
- Biometric unlock: AUK (or sym_key) wrapped by a platform key (Secure Enclave / Android Keystore with `setUserAuthenticationRequired` / Windows Hello / TPM). Never store the master password.

## Recovery

- Emergency Kit (PDF/printable): account email, Secret Key, QR, blank for master password.
- Phase 2: emergency access — trusted contact receives vault keys wrapped to their X25519 key, released only after a user-set delay (1–30 days) with no veto.
- No vendor recovery backdoor. Lost master password + no emergency contact = lost vault; the UI says so clearly at signup.

## Sharing

- Vault share: vault_key encrypted to recipient's X25519 public key, signed with sharer's Ed25519 key; recipient verifies fingerprint on first share (optional out-of-band check).
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
