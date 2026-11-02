# Architecture

One Rust core holds all crypto, vault logic, sync, import/export and autofill field matching. Each platform is a thin native UI over it. This is how every feature ships on every platform at once.

## Monorepo layout

```
pwm/
  CLAUDE.md
  Cargo.toml                # Cargo workspace root: members are core/pwm-* and server/ (one lockfile)
  package.json              # pnpm workspace root: extension/ and tests/*
  justfile
  docs/                     # these specs
  design/
    tokens.json             # colors, type, spacing shared by all UIs
  core/                     # Rust workspace
    pwm-crypto/             # primitives wrappers, KDF, key hierarchy, Secret<T>
    pwm-vault/              # item model, vault/account structs, versioned formats
    pwm-store/              # local encrypted cache (SQLite, ciphertext only) behind a Store trait (D20)
    pwm-sync/               # sans-IO sync engine: offline queue, retries, conflict merge (D28)
    pwm-autofill/           # URL matching, field classification, site overrides
    pwm-export/             # our own export formats: .pwmx (encrypted), JSON, CSV, behind re-auth (D24)
    pwm-import/             # importers (Bitwarden, 1Password, Keeper, NordPass, Proton Pass, Dashlane, KeePass, LastPass, CSV)
    pwm-generator/          # password/passphrase generator, strength estimation (zxcvbn)
    pwm-health/             # weak/reused/breached checks (HIBP k-anonymity)
    pwm-ffi/                # UniFFI bindings (Swift, Kotlin) + C ABI
    pwm-wasm/               # wasm-bindgen build for the extension
    test-vectors/           # JSON vectors for every crypto/format operation
  server/                   # Rust (axum) sync API, PostgreSQL, same binary for hosted + self-hosted
  apps/
    apple/                  # Swift package shared by iOS + macOS
    ios/                    # SwiftUI app + AutoFill Credential Provider extension
    macos/                  # SwiftUI app + Safari Web Extension host
    android/                # Kotlin, Jetpack Compose, Autofill Service, Credential Manager
    desktop/                # Tauri app for Windows + Linux (decision D3)
  extension/                # TypeScript MV3: Chrome, Edge, Firefox, Brave, Opera (+ Safari via macOS host)
  tests/
    autofill-harness/       # Playwright + fixture sites + top-1,000 site runner
    e2e/                    # cross-platform end-to-end scenarios
```

## Platform stack

| Layer | Technology | Responsibility |
| --- | --- | --- |
| Core | Rust → native libs (UniFFI) and WASM | Crypto, vault model, sync, import/export, field classification |
| iOS | Swift / SwiftUI, AutoFill Credential Provider | App, autofill, passkeys, Face ID |
| macOS | SwiftUI, Safari Web Extension | Desktop app, Touch ID, biometric bridge for all browsers |
| Android | Kotlin / Compose, Autofill Service, Credential Manager | App, autofill, passkeys, biometrics |
| Windows / Linux | Tauri (Rust backend + web UI) — see D3 | Desktop app, Windows Hello, biometric bridge |
| Extensions | TypeScript, Manifest V3, core via WASM | Autofill, save prompts, unlock via desktop app (native messaging) |
| Server | Rust (axum), PostgreSQL, S3-compatible blob store | Encrypted blob sync, sharing, billing hooks, org admin |
| Local mode (Phase 2) | Encrypted vault file via iCloud Drive / Google Drive / Dropbox / WebDAV / LAN P2P | No-account storage |

## Data flow

```
UI (native) ──FFI──▶ core (decrypt/encrypt in memory) ──▶ local encrypted cache (SQLite, ciphertext only)
                                   │
                                   └──HTTPS──▶ server (ciphertext blobs + metadata-free index)
Extension ──native messaging──▶ desktop app (unlock, biometrics)   or   WASM core (standalone unlock)
```

- Clients keep a local encrypted cache so the app works offline.
- Server API is versioned (`/v1/...`), auth via OPAQUE (see `SECURITY.md`), JSON over HTTPS, ciphertext fields base64.
- Sync model: each item is an encrypted blob with `(item_id, vault_id, revision, seq, updated_at)`: `revision` is per item and chosen by the client (it is in the AAD), `seq` is a per-vault change counter assigned by the server (D25). Every write names the revision it replaces, so the server never overwrites silently; a stale write gets 412 with the current item, and the client keeps both versions as item history (never silent data loss). Non-members of a vault get 404, viewers 403 `read_only` on writes (D27).
- Sync client (D28): `pwm-sync` does no I/O. It hands the platform one `HttpRequest` at a time and takes back the `HttpResponse`, so native apps and the WASM extension run the same engine. Unsent changes are the cache's `Modified`/`Deleted` records. A run lists vaults, pulls each vault, then pushes. Conflicts keep the later edit current and the other in history; versions that cannot be merged, and edits of items deleted elsewhere, become new items.

## Server API (v1, minimum)

| Endpoint | Purpose |
| --- | --- |
| `POST /v1/auth/prelogin` | Account id, salt and KDF params for an email (fake but stable for unknown emails, D12) |
| `POST /v1/auth/register/{start,finish}` | OPAQUE registration (D26) |
| `POST /v1/auth/login/{start,finish}` | OPAQUE login → access + refresh tokens (D26) |
| `POST /v1/auth/refresh` / `POST /v1/auth/logout` | New token pair (refresh token replaced on use) / end the session |
| `GET /v1/account` | Encrypted account keyset, KDF params, salt, public keys, plan |
| `GET /v1/vaults` / `POST /v1/vaults` | List the caller's vaults with their copy of each vault key and the current `seq`, plus the server epoch in a `pwm-epoch` header (D30) / create a vault (D27) |
| `GET /v1/vaults/{id}/items?since=<seq>&limit=<n>` | Delta sync: items and tombstones after `since` in `seq` order, from one snapshot; `{changes, next, more}` (D27) |
| `PUT /v1/vaults/{id}/items/{item_id}` | Create (`If-None-Match: *`, revision 1) or update (`If-Match: "<revision>"`, revision + 1); 412 returns the current item (D27) |
| `DELETE /v1/vaults/{id}/items/{item_id}` | Delete with `If-Match`, leaving a tombstone; ids are never reused (D27) |
| `POST /v1/shares` / `GET /v1/shares/{id}` | One-time encrypted share links |
| `GET /v1/devices` / `DELETE /v1/devices/{id}` | Device list, revoke |

Limits (D29):
- **Sizes:** bodies up to 16 KiB; item writes up to 384 KiB, with the blob at most 256 KiB once decoded. Larger gets 413 `too_large`.
- **Counts:** at most 100 vaults per account and 50,000 live items per vault. More gets 403 `limit_reached`.
- **Rates:** per IP for registration, login and refresh; per account for everything else. Over the limit gets 429 `rate_limited` with `Retry-After`.
- **Request ids:** every response carries an `x-request-id` that matches its log line.

Export is client-side only (decrypt locally, write file); the server has no export endpoint.
