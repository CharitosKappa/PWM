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
    pwm-sync/               # sync engine (CRDT-lite, see below), conflict resolution
    pwm-autofill/           # URL matching, field classification, site overrides
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
- Sync model: each item is an encrypted blob with `(item_id, vault_id, revision, updated_at)`. Last-writer-wins per item with revision check; conflicting edits keep both versions as item history (never silent data loss).

## Server API (v1, minimum)

| Endpoint | Purpose |
| --- | --- |
| `POST /v1/auth/register/{start,finish}` | OPAQUE registration |
| `POST /v1/auth/login/{start,finish}` | OPAQUE login → session token |
| `GET /v1/account` | Encrypted account keyset, plan, devices |
| `GET /v1/vaults` / `POST /v1/vaults` | List/create vaults (encrypted vault keys) |
| `GET /v1/vaults/{id}/items?since=rev` | Delta sync |
| `PUT /v1/vaults/{id}/items/{item_id}` | Upsert with `If-Match: revision` |
| `DELETE /v1/vaults/{id}/items/{item_id}` | Tombstone |
| `POST /v1/shares` / `GET /v1/shares/{id}` | One-time encrypted share links |
| `GET /v1/devices` / `DELETE /v1/devices/{id}` | Device list, revoke |

Export is client-side only (decrypt locally, write file); the server has no export endpoint.
