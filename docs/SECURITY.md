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
- Biometric unlock: the opened keyset sealed under a random key, which only a platform key can unwrap (Secure Enclave / Android Keystore with `setUserAuthenticationRequired` / Windows Hello / TPM, or on Linux a key only root can read, behind polkit: D71). See **Biometric unlock (D45)** below. Never store the master password.

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
| Unlocked keyset for Face ID / Touch ID on one device (D45) | `pwm/biometric-unlock` | `version:u8 \|\| account_id \|\| device_id` |
| The browser extension's unlocked keyset for Touch ID through the Mac app (D48) | `pwm/browser-unlock` | `version:u8 \|\| account_id \|\| device_id` (the extension's device id) |
| The biometric unlock key on Windows, under a key derived from a Windows Hello signature (D68) | `pwm/windows-hello` | `version:u8 || account_id || device_id` |
| The browser extension's key K, kept by the Windows helper under a key derived from a Windows Hello signature (D69) | `pwm/browser-unlock/windows-hello` | `version:u8 \|\| caller_len:u16 \|\| caller \|\| account_id \|\| device_id` |
| The desktop's unlock key on Linux, under a key derived from the key holder's answer (D71) | `pwm/linux-polkit` | `version:u8 \|\| account_id \|\| device_id` |
| The browser extension's key K, kept by the Linux helper under a key derived from the key holder's answer (D71) | `pwm/browser-unlock/linux-polkit` | `version:u8 \|\| caller_len:u16 \|\| caller \|\| account_id \|\| device_id` |
| Secret Key sent to a new device that a signed-in one approved (D46) | `pwm/approval` | `version:u8 \|\| approval_id \|\| n_pub:32 \|\| e_pub:32` |

A new kind of envelope gets a new label; labels are never reused.

**Biometric unlock (D45).** The core seals the account keyset plaintext under a fresh random 32-byte key (AAD `pwm/biometric-unlock`, version 1, bound to the account and the device). The app keeps the blob `v:u8 (= 1) || account_id:16 || envelope`, and the key only encrypted to a P-256 key in the Secure Enclave (system ECIES, `eciesEncryptionCofactorVariableIVX963SHA256AESGCM`). That enclave key never leaves the hardware, is usable only after Face ID or Touch ID with the current enrolment (`.biometryCurrentSet`), and is bound to this device (`WhenPasscodeSetThisDeviceOnly`). A new face or finger makes it refuse: the app deletes it, and the master password unlocks. The master password is asked every 14 days, at each sign-in, and whenever biometrics fail; each master password unlock makes the blob again. Turning it off or signing out deletes the enclave key, the wrapped key and the blob. Unlocking this way never runs the KDF, which the AutoFill extension needs (M5.5).

**Windows Hello unlock v1 (D68).** Windows Hello's key can only sign, so D45's unlock key is sealed under `HKDF-SHA256(ikm = signature, salt = challenge, info = "pwm/windows-hello/v1")`, where the challenge is 32 random bytes kept on the computer and the signature is the TPM key's RSA PKCS#1 v1.5 signature over it, made after a Windows Hello gesture (face, fingerprint or PIN) and never stored. Turning it on checks that two signatures match. The sealed key is `v:u8 (= 1) || envelope`.

**Unlock on Linux v1 (D71).** Linux has no hardware key that releases a secret only after a gesture, so a root key holder stands in for one. `pwm-keyholder`, installed at `/usr/libexec/pwm/pwm-keyholder` by the `.deb` and `.rpm`, runs only through `pkexec`, behind the polkit action `invalid.pwm.unlock` (`auth_self` for the active session, no remembered answer): every call shows polkit's dialog, which takes the login password, or a fingerprint where PAM is set up for it. For each name it keeps a random 32-byte secret S in `/var/lib/pwm/keys/<uid>/`, readable only by root, and never returns it. It answers a 32-byte challenge with `HKDF-SHA256(ikm = S, salt = challenge, info = "pwm/linux-polkit/response/v1")`. That answer takes the Windows Hello signature's place in D68's construction (info `pwm/linux-polkit/v1`, AAD `pwm/linux-polkit`) and D69's (info `pwm/browser-unlock/linux-polkit/v1`, AAD `pwm/browser-unlock/linux-polkit`). Turning it off deletes the user's side (the challenge and the sealed key), after which S opens nothing; purging the package deletes `/var/lib/pwm`.
- **What it guards against:** this user's processes can't read S, so a copied home folder or browser profile holds only the challenge and the sealed key, and every unlock shows the dialog.
- **Not covered:** root; a disk image of a computer without full-disk encryption, which holds both halves; malware running as the user that makes the call, which still needs the user's gesture in the dialog. polkit can't tell that a fingerprint was added. It is weaker than a TPM-bound key; the Flatpak and Snap or Flatpak browsers can't use it.

**Browser unlock v1 (D48).** The extension in Chrome, Edge, Brave, Opera, Firefox or Safari can unlock with Touch ID through the PWM app for Mac.
- **The blob:** while unlocked, the core seals the keyset under a fresh 32-byte key K (AAD `pwm/browser-unlock`, version 1, bound to the account and the extension's device id). The extension keeps `v:u8 (= 1) || account_id:16 || envelope` on its disk and sends K once to the Mac's helper. K never reaches the browser's disk.
- **The helper** keeps K encrypted to a Secure Enclave key of its own for that enrolment (as D45: `.biometryCurrentSet`, this device only). It asks for Touch ID when it is turned on and at every unlock, then returns K; the extension opens the blob and its sealed tokens (D34). A new face or finger deletes the enrolment.
- **Who may ask:** browsers start the helper only for the extension ids in the manifests the app writes when the user turns on "Allow browser extensions to unlock with Touch ID". The helper also checks the caller the browser passes against the same ids. Safari's messages come from the extension in the app's own bundle.
- **Messages:** native-messaging framing (a 4-byte length, then JSON), `v: 1`: `enroll {account_id, device_id, key}`, `unlock {account_id, device_id}`, `forget {account_id, device_id}`. Answers are `ok`, `key`, or an error (`cancelled`, `changed`, `unknown`, `unavailable`, `refused`, `invalid`). Nothing is logged.
- **The same rules as D45:** the master password every 14 days and at each sign-in. Turning it off or signing out forgets K on the Mac and the blob in the browser.
- **What it guards against:** a copied browser profile holds only the blob; the helper's key alone opens nothing; every unlock shows Touch ID. Not covered: malware running as the user that fakes the caller still needs Touch ID and the blob.

**The iOS AutoFill extension (D47).** The app and the extension share one cache (in the App Group container) and one Keychain group; the enclave key is made in that group too. The extension unlocks only with Face ID or Touch ID as above, never with the master password, and drops every key once it has answered iOS. It fills only after a tap or a chosen suggestion. Outside our encryption, iOS keeps the suggestions it shows above the keyboard on the device: for each login with a password, its domains and username, and for each passkey, its RP id, user name, credential id and user handle, with an id of the form `vault_id/item_id`. Never a password, a title or a note. The switch "Suggest logins above the keyboard" (on by default) and signing out empty that list. For passkeys iOS builds `clientDataJSON` itself and hands over its SHA-256 hash; iOS has already checked the RP id against the app's or site's associated domains, and the core still refuses one that is not a registrable domain (or under one), a public suffix or an IP address.

**Extension session (D34).** Chrome stops an idle service worker and wipes its memory. So that the extension stays unlocked, the core seals the account keyset plaintext under a fresh random 32-byte key (AAD `pwm/session`, version 1, bound to the account). The blob `v:u8 (= 1) || account_id:16 || envelope` and its key go only into `chrome.storage.session`: memory-only, cleared when the browser closes, and not readable by content scripts. Locking, sign-out and a revoked device clear it. On disk the extension keeps the cache snapshot (ciphertext and ids), the Secret Key as is (D13: useless without the master password), and the access and refresh tokens as `v:u8 (= 1) || envelope(JSON {access, refresh})` sealed under `sym_key` (AAD `pwm/device-tokens`), so they are usable only after unlock. Never the master password, the keyset or any unlocked key.

**Password salt (D12).** `salt_pw` is 16 random bytes generated at signup and stored server-side with the KDF params. The pre-login endpoint returns them; for an unknown email it returns `HMAC-SHA256(server_secret, email)[..16]` and the default params, so it does not reveal which accounts exist.

**Master password (D16).** Before Argon2id the password is prepared with the RFC 8265 OpaqueString rules: non-ASCII space characters (U+00A0, U+1680, U+2000–U+200A, U+202F, U+205F, U+3000) become U+0020, the result is normalized to Unicode NFC, case is kept, nothing is trimmed, and empty passwords or passwords containing control characters are rejected. The KDF input is the UTF-8 encoding.

**Account keyset plaintext (v1).** `version:u8 (= 1) || sym_key:32 || x25519_secret:32 || ed25519_seed:32` (97 bytes), sealed under the AUK with AAD `pwm/keyset` (version 1, account_id). Public keys are derived from the secrets and are not stored inside it.

**Vault key plaintext (v1).** The 32 raw key bytes, sealed under `sym_key` with AAD `pwm/vault-key` (version 1, vault_id).

**Item record v1 (D18).** An item or folder is UTF-8 JSON (schema field `"v": 1`, see `core/pwm-vault`), padded to the next multiple of 256 bytes with one `0x80` byte then zeros (ISO/IEC 7816-4), and sealed under the vault key with AAD `pwm/item` (record version 1, vault_id, item_id, revision). The AAD version is the record format (padding + JSON container); the JSON `v` is the schema version inside it. Parser errors never carry input text, because they could quote secret values.

**History and trash (D19).** Previous versions (at most 10) and the `trashed_at` time live inside the encrypted item, so the server never learns how an item changed or what is in the trash. Clients purge items trashed for 30 days at unlock; the server sees only the resulting tombstone.

**Local cache (D20).** Each device caches only what the server also holds: account id, salt, KDF params, sealed keyset, wrapped vault keys, item envelopes with revisions, plus local sync state. The Secret Key and unlocked keys are never written to it (platform secure storage only). Deleted rows are overwritten (`secure_delete`); the schema is versioned and a newer or foreign database is refused.

**Syncing while locked and push (D50).** A locked app syncs with its tokens only: it stores pulled item envelopes as they are and uploads queued changes that are already sealed. A change that needs the vault key (two versions to merge, an unsent edit to move, a restored server) waits for the next unlocked sync, so no key is ever needed while locked. On iOS and macOS the tokens are in the Keychain, readable after the device's first unlock; the extension's tokens stay sealed (D34) and it syncs only while unlocked.
- **Push tokens:** one APNs or FCM token per device (`push_tokens`, as bytes, never logged), deleted at sign-out, revoke and logout, or when APNs or FCM calls it invalid.
- **What a push says:** a vault change sends `{"aps":{"content-available":1}}` over APNs, or the FCM data `{"t":"sync"}`, and nothing else, at most once a minute per device, never to the device that made the change. An approval request sends a fixed notice over APNs, or `{"t":"approval"}` over FCM, whose app has the words; neither names an email or device.
- **What Apple and Google learn:** that some change happened for a device token, and when. Nothing about the account, vault or item.
- **Provider tokens:** for APNs, ES256 over the `.p8` key (`p256`), made again every 50 minutes. For FCM (D60), an RS256 assertion over the service account's key (`ring`), exchanged at Google's token endpoint for an access token that is kept until 5 minutes before it expires. Both keys are server settings, like `PWM_SERVER_SECRETS`.
- **On Android,** Firebase starts only with the build's settings and brings messaging alone: no Analytics, no delivery data, no `google-services` plugin. The new-device notice shows only after the user turns on its switch and Android allows notifications.

**Importing (D51).** Imports run in memory in the core (`pwm-import`); the file is never copied, values are zeroized after parsing, nothing is logged, and errors never quote the file. Items get new ids and sync like any new item.
- **Hostile files:** at most 50 MB, ZIPs at most 10,000 entries and 200 MB unpacked (only the expected entries are read), JSON nesting stops at 128 levels, at most 50,000 items. Every refusal has its own code.
- **Bitwarden's password-protected export** opens with PBKDF2-SHA256 (5,000 to 2,000,000 iterations) or Argon2id (16 MiB to 1 GiB, 1 to 10 iterations, 1 to 16 lanes), HKDF-Expand into encryption and MAC keys, then AES-256-CBC with HMAC-SHA256 checked first. These primitives (RustCrypto `aes`, `cbc`, `pbkdf2`) open that format only; our own formats use the AEAD above. The account-restricted export and Proton's PGP export are refused.
- **KeePass** (`keepass` crate, MIT): before the crate runs, our own check of the outer header refuses KeePass 1 files and key derivation over 100 million AES rounds or Argon2 over 1 GiB, 100 iterations or 64 lanes.
- **The source file:** apps offer to delete a plaintext export after importing it. On flash storage that doesn't guarantee its old blocks are gone, so it is not called a secure delete.

**Health checks (D22).** Reuse is found by HMAC-SHA256 under a random key made per report run and dropped afterwards. The breach check sends only the first 5 hex characters of each password's SHA-1 straight from the device to the HIBP range API (`Add-Padding: true`, no cookies or account ids), never through our server; SHA-1 is used for nothing else. Results stay in memory.

**Export (D24).** Every export, encrypted or plaintext, needs the master password again: the core exports only with a single-use grant, valid for 5 minutes, made by re-deriving the AUK and opening the sealed keyset. A `.pwmx` file is `"PWMX" || v:u8 (= 1) || m:u32 || t:u32 || p:u32 || salt:16 || envelope`. Its key is `HKDF(ikm = Argon2id(export password, salt, m/t/p), salt = salt, info = "pwm/export/v1")`. The export password is normalized as in D16 and must score ≥ 3 in zxcvbn, and the params must pass the account KDF bounds. The envelope (AAD `pwm/export`, binding the header) holds the JSON export padded to a multiple of 4 KiB (ISO/IEC 7816-4). The JSON export holds each item's D18 JSON unchanged plus its id. The CSV export holds logins, notes, cards and identities that are not in the trash, without history, passkeys or custom fields. The server has no export endpoint.

**Server login (D26).** OPAQUE (RFC 9807) via `opaque-ke` 4.0: OPRF ristretto255-SHA512, 3DH on ristretto255 with SHA-512, KSF = Identity (the input `auth_secret` is already Argon2id-stretched and mixed with the Secret Key), context `"pwm/opaque/v1"`, client identifier and credential identifier = `account_id`, server identifier = the server's OPAQUE public key. Both protocol sides live in `pwm-crypto`. The server's long-term secrets are one blob `v:u8 (= 1) || ServerSetup || server_key:32` (base64url in `PWM_SERVER_SECRETS`); losing the ServerSetup locks every account out, so it is kept in a secret manager with a backup and is not rotated in v1. Two keys are derived from `server_key` with HKDF-SHA256 (empty salt): `prelogin_key` (info `pwm/prelogin/v1`) and `login_state_key` (info `pwm/login-state/v1`). D12's fake pre-login answers are `HMAC-SHA256(prelogin_key, len(label) || label || email)` with labels `pwm/prelogin/id` and `pwm/prelogin/salt`, the id shaped as UUIDv4, with the default params. The OPAQUE server state kept between login start and finish holds the MAC the client must send, so it is stored sealed under `login_state_key` (AAD `pwm/login-state`, bound to the account): reading the database does not let anyone finish a login in progress. Unknown accounts get `opaque-ke`'s fake credential response; every login failure returns the same error.

**Session tokens (D26).** 32 random bytes, base64url with a `pwma_` (access, 15 minutes) or `pwmr_` (refresh, 90 days unused, replaced on every use) prefix. The server stores only SHA-256 hashes. Presenting a replaced refresh token ends that session. One session per device; revoking a device ends its session at once: its access and refresh tokens are then answered with 403 `device_revoked`, and the sync engine clears the local cache (D28). Signing out ends the session too (D62): every client sends `POST /v1/auth/logout` (which also drops the device's push token) before wiping the device, waiting at most 5 seconds; offline, it wipes the device anyway and the session lasts until its refresh token goes 90 days unused or the device is signed out from another device.

**Billing (D6).** The server keeps, per subscription, its provider and the provider's reference, the plan, status, price, currency and dates, and per account a random `billing_token` (passed to the stores as Apple's `appAccountToken` and Play's `obfuscatedAccountId`) and Stripe's customer id. No card data (Stripe's page takes it) and no item data. A purchase counts only for the account whose token the provider reports; a device can't claim another's. Every provider notification is a hint: the server asks the provider's API over TLS before changing anything, so Apple's signed payloads are never trusted on their own. Stripe's webhooks are checked with HMAC-SHA256 under the endpoint secret (`pwm-crypto` `webhook_signature_valid`, constant time, five minutes' tolerance); Play's push URL carries a secret compared in constant time (`secret_matches`). Provider keys, the webhook secret and Play's purchase tokens are never logged.

**Secrets at the binding boundary (D31).** The master password, Secret Key, tokens and revealed item fields cross into Swift, Kotlin and JavaScript as platform strings, which the core cannot wipe. In the shared screens (D67), only two requests return hidden values: `reveal` (one value, after Show) and `itemForEdit` (the whole item, while the editor is open); lists and item details never do. In the extension, only the manage page may send these requests (the background checks the sender), and a lock reloads it so nothing revealed outlives the lock. The core wipes its own copies (`Secret<T>`). Apps must keep such strings short-lived, never log them, and store the Secret Key and tokens only in secure storage (Keychain, Keystore, Windows Hello/TPM, or for the extension the encrypted session store). Requests the core hands out can carry an access or refresh token, so the platform's HTTP code must not log headers or bodies.

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

- **Relying party id:** it must be the calling origin's host or a registrable suffix of it, never a public suffix or an IP address, and it must fit in DNS: at most 253 characters in its ASCII (IDNA) form and 63 in a label. Only HTTPS and `localhost` origins qualify.
- **The origin** is the browser's report of the calling frame. `clientDataJSON` is built in the core from it, with `crossOrigin`/`topOrigin` for same-site frames. Frames of another site never get our passkeys.
- **`authenticatorData`:** flags UP, UV, BE and BS (plus AT on create), a sign count of 0 always (synced passkeys), and AAGUID `6c9e3d41-8a27-4f0b-9d52-e17730c458a6`.
- **Attestation:** `fmt: "none"`. The COSE key and the attestation object are hand-written deterministic CBOR.
- **User verification** means the vault is unlocked and the user confirmed in an extension window (never page DOM).
- **Keys:** private keys are PKCS#8 inside the login item (D18), so they are encrypted like any item, and the server sees only ciphertext.

**Passkeys on Android (D61).** The Credential Manager provider answers two kinds of caller:

- **A privileged browser:** one of D58's seven, by package and signing certificate, checked by Android (`getOrigin` with our list) and again by the app. Its origin is checked against the RP id as for a page. It builds the client data and hands over the hash, which the core signs; the response carries the placeholder `e30` where `clientDataJSON` goes.
- **Any other app:** the core builds `clientDataJSON` with the origin `android:apk-key-hash:<base64url SHA-256 of the app's signing certificate>` and `androidPackageName`. The app has no origin, so the RP id must be listed for it by the site's Digital Asset Links file (`https://<rp id>/.well-known/assetlinks.json`): a statement for `android_app` with the package, the certificate's SHA-256, and `delegate_permission/common.get_login_creds` or `.handle_all_urls`. `include` statements are followed one level deep. The app fetches the files (HTTPS only, no cookies, cache or redirects, 5 seconds, 256 KB) and keeps them in memory for an hour, dropped at lock and sign-out; the core reads them. Fetching tells the site only that someone on this network asked for its file. The autofill service never fetches them.
- **User verification:** every passkey use asks for a fingerprint or face, or the master password, in PWM's own screen, right before signing; unlocking or confirming within the last minute counts. Nothing about the vault names the sheet's entries before unlocking.
- **Never PWM itself:** a request from PWM's own package gets nothing.

**Signatures (D17).** Ed25519 over `len(label):u8 || label || message`, verified in strict mode (rejects malleable signatures and small-order public keys). Labels are never reused; current labels: `pwm/share-sig` (vault shares) and `pwm/autofill-rules` (the signed autofill rules file, D33: `v:u8 (= 1) || key_id:u8 || signature:64 || json`, signature over `key_id || json`, release keys pinned in the apps, the signing key kept offline).

**Device approval v1 (D46).** A new device ("N") gets the Secret Key from a signed-in, unlocked one ("E") of the same account. N still needs the master password.

```
N: n_secret, n_pub = fresh X25519 key pair
   commitment      = SHA-256(len:u8 || "pwm/approval-commit" || n_pub)
   → relay: email, commitment, platform            (unauthenticated; same answer for any email)
E: e_secret, e_pub = fresh X25519 key pair          (after the user picks the request)
   → relay: e_pub                                    (authenticated)
N: → relay: n_pub                                    (the relay and E both check it against the commitment)
both: dh   = X25519(own secret, other pub)           # all-zero result rejected
      salt = approval_id || n_pub || e_pub
      key  = HKDF(dh, salt, info = "pwm/approval/v1")
      code = u32_be(HKDF(dh, salt, info = "pwm/approval-code/v1")[0..4]) mod 1,000,000, shown as "123 456"
E: after the user sees the same code on both screens and taps "Codes match":
   envelope = seal(key, secret_key_text, AAD "pwm/approval": version:u8 || approval_id || n_pub || e_pub)
   → relay: envelope                                  (handed to N once, then deleted)
```

- **Why the commitment:** a relay in the middle could otherwise try many keys until the two codes match. N commits to its key before E's key is known and reveals it only after, so neither code can be steered: a man in the middle succeeds with a chance of 1 in 1,000,000 per request, and the user sees two different codes otherwise.
- **The relay** stores only public keys, the commitment, the platform, times and the envelope until it is fetched; requests live 10 minutes, at most 3 are pending per account (more go nowhere), and asking is limited to 5 per IP per 10 minutes. The poll token is a `pwmp_` token stored as a SHA-256 hash.
- **Not covered:** a person at an unlocked E who approves a stranger's request; that device still needs the master password. A review of this exchange is part of the external audit before the beta (M10.3).

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
| Network attacker | Yes | TLS 1.3 with the system's trust store, OPAQUE (no password on wire). Pinning the hosted server comes in every app at once when D1 gives it a hostname (D66 §5); no app pins yet |
| Malicious website (phishing, clickjacking) | Yes | Exact-origin matching, user-initiated fill, no DOM-injected clickable UI (see `AUTOFILL.md`) |
| Malicious extension in same browser | Partial | Minimal permissions, isolated UI, no secrets in DOM longer than needed |
| Compromised unlocked device / malware | No (document it) | Auto-lock, clipboard clearing (30s), memory zeroize |
| Screen recording, screen sharing, screenshots, the app switcher | Partial | iOS hides secrets while the screen is captured and masks them again after a screenshot (iOS can't block one); the switcher is covered; macOS leaves the window out of captures (D49). No API in browsers or on Linux |
| Lost/stolen locked device | Yes | Full-disk + vault encryption, biometric bound to hardware keys, remote device revoke |
| Tokens copied off a device before it signed out (backup, malware, disk image) | Yes | Sign-out ends the session on the server, so a copied refresh token is refused (D62); a sign-out made offline leaves it valid until 90 days unused, so the Devices list on another device still shows that device and can sign it out |
| Push service (APNs, FCM) observing timing | Accepted | Pushes carry no content or ids; Apple and Google see only that a device token had a change, and when (D50, D60) |
| Billing providers (Stripe, Apple, Google) | Accepted | They see who pays and when, never vault data. Forged notifications only make the server read the provider's API again, within a rate limit (D6 §3) |
| Weak master password | Partial | Secret Key; zxcvbn ≥ 3 required at signup |

## Security program

- Bug bounty from public beta. `SECURITY.md` + `security.txt` in repo and website.
- External audits before public launch (crypto + core, server, apps, extensions); reports published.
- Closed source (S6), so trust comes from evidence: a public security whitepaper describing this spec, independent audit summaries published, and signed releases on every platform.
- Auditors get full source access under NDA.
- Dependency audit (`cargo audit`, `cargo deny`, `npm audit`) in CI, blocking on high severity.
