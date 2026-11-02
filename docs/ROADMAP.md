# Roadmap and task list

Work top to bottom. Tick `[x]` when the acceptance criteria pass. Durations are planning estimates.

```
Phase 0  Foundation      months 1–3   M0–M3   → Gate: Private beta (core works on 2 platforms)
Phase 1  MVP + launch    months 4–9   M4–M10  → Gate: Public launch (audit published, 95% autofill)
Phase 2  Differentiate   months 10–15 M11–M15 → Gate: Teams beta (98% autofill, 4.6+ rating)
Phase 3  Teams           months 16–24 M16–M19
```

---

## Phase 0 — Foundation

### M0 — Repo and tooling
- [x] M0.1 Create monorepo layout from `ARCHITECTURE.md`; Rust workspace in `core/`, `server/`, `extension/` package.
- [x] M0.2 `justfile` with `check`, `test`, `test-vectors`, `autofill`, `server`.
- [x] M0.3 CI workflows per `QUALITY.md` (fmt-lint, core-test, audit) — green on empty crates.
- [x] M0.4 `design/tokens.json` (colors light/dark, type scale, spacing, radius) — placeholder values, documented.
- [x] M0.5 `SECURITY.md` (disclosure policy) at repo root, proprietary `LICENSE` notice, `cargo deny` + `license-checker` licence allow-list (decision S6) failing CI on any copyleft dependency, and `THIRD_PARTY_NOTICES` generation (`cargo about`, `license-checker`).

**Done when:** `just check && just test` pass locally and in CI.

### M1 — Crypto core (`pwm-crypto`)
- [x] M1.1 `Secret<T>` wrapper: zeroize on drop, redacted Debug/Display, no Serialize by default.
- [x] M1.2 AEAD wrapper (XChaCha20-Poly1305) with versioned envelope `{v, nonce, ct}` and AAD.
  - Deferred: base64url JSON form of the envelope (lands with the first JSON wire format, M2.3/M3.3); `getrandom` `wasm_js` backend (M3.7).
- [x] M1.3 Argon2id KDF with stored params; HKDF helpers; Secret Key generation + human format (`A3-XXXXXX-...`).
- [x] M1.4 Key hierarchy from `SECURITY.md`: AUK, auth secret, account keyset create/unlock, vault key wrap/unwrap.
- [x] M1.5 X25519 share wrap/unwrap + Ed25519 sign/verify.
  - Deferred: key fingerprint format for the out-of-band check (with the sharing UI, M13). Share-key bytes are pinned by `share.json`, re-derived independently in `verify.mjs` (M1.6).
- [x] M1.6 Test vectors in `core/test-vectors/*.json` for every operation; tamper tests (bit flips must fail).
  - Finding for M3.3: revisions above 2^53 lose precision as JSON numbers in JavaScript; send them as strings on the wire.
- [x] M1.7 Fuzz targets for envelope decode.
  - 5 targets (`envelope_decode`, `vault_share_decode`, `keyset_plaintext`, `secret_key_parse`, `password_normalize`) ran 10 min each clean on 2026-11-01 (6–76 M executions each). CI: 60 s per target on PRs, 10 min nightly.

**Done when:** 100% of vectors pass; fuzzers run 10 min clean; no `unsafe`.

### M2 — Vault model (`pwm-vault`, `pwm-generator`, `pwm-health`)
- [x] M2.1 Item types: login (multi-URL, TOTP secret, passkey), secure note, card, identity; custom fields; tags; folders. Versioned schema.
  - Note: zeroizing decrypted items is best effort; serde_json's internal parse buffers are not wiped.
- [x] M2.2 Item history (last 10 versions) and soft-delete (trash 30 days).
  - Purge runs where the client unlocks (M3.4 wires it to sync tombstones). Read-only items (newer schema or unknown type) cannot be trashed, only deleted.
- [x] M2.3 Local encrypted cache (SQLite storing ciphertext only) + migrations.
  - New crate `pwm-store` (D20): `Store` trait, bundled-SQLite backend, schema v1 with a frozen fixture. Deferred: IndexedDB backend for the extension (M3.7/M4); sync code that fills the cache (M3.4); concurrent access from app + autofill extension is only covered by SQLite locking and a 5 s busy timeout, revisit with the iOS/Android apps.
- [x] M2.4 Generator: random password and EFF-wordlist passphrase; zxcvbn strength.
  - Per D21. `random_index` (rejection sampling) added to `pwm-crypto`. Deferred: About-screen attribution for the EFF list (each app, M5+); generator settings sync with a versioned format (M5); zxcvbn on `wasm32` is checked with the WASM build (M3.7).
- [x] M2.5 Health: weak, reused (compare hashes in memory), breached via HIBP k-anonymity range API.
  - Per D22. `hibp_sha1` and `ReuseHasher` added to `pwm-crypto`; platforms implement `RangeFetcher` (HTTPS GET with `Add-Padding: true`). Deferred: the platform fetchers and the settings switch for the breach check (M4/M5); card PINs and custom fields are not checked yet.
- [x] M2.6 TOTP (RFC 6238) code generation.
  - Per D23. `hotp_value` in `pwm-crypto`; `Totp`/`normalize_totp` in `pwm-vault` (otpauth URIs, bare secrets, Steam). Vectors in `core/test-vectors/totp.json` (Rust + Node); new fuzz target `totp_parse`. Deferred: counter-based HOTP; QR scanning (apps); Premium gating (M9.3).
- [x] M2.7 Encrypted export (`.pwmx`) and plaintext export (JSON/CSV) behind re-auth.
  - Per D24. New crate `pwm-export`: `authorize_export` grant (master password, single use, 5 min), JSON, `.pwmx` and CSV writers and readers. `seal_export`/`open_export` in `pwm-crypto`; `Item::to_json_with_id`/`from_json_with_id` in `pwm-vault`. JSON and `.pwmx` round-trip every fixture item unchanged (frozen `export_v1` fixtures); CSV keeps what it can hold (it also drops URL match modes, beyond the D24 list). Vectors in `core/test-vectors/export.json` (Rust + Node); new fuzz target `export_read`. Deferred: export dialogs, warning text and file saving in each app (M4/M5); importing an export into an existing account (id clashes, merge) with the importers (M8).

**Done when:** round-trip tests for all item types; export → import yields identical vault.

### M3 — Sync server + client sync
- [x] M3.1 axum server, Postgres schema (accounts, devices, vaults, vault_keys, items, tombstones, shares, subscriptions); migrations with `sqlx`.
  - Per D25. `pwm-server` lib + binary: config from env (`PWM_DATABASE_URL`, `PWM_LISTEN`, `PWM_LOG`), migrations embedded and applied at startup (newer or edited migrations refused), `GET /healthz`, graceful shutdown. Schema v1 in `server/migrations/0001_initial.sql` with a checked-in snapshot; tests check the plaintext column allow-list, constraints and account-deletion cascade against real Postgres (`just db`, `just test-server`; new blocking CI job `server-test`). `vaults` gained `owner_id` for the cascade. Deferred: Postgres TLS (rustls + OS roots) and the reverse-proxy setup (M3.6); request logging (M3.5 scrubber); tombstone compaction; refusing an item id that has a tombstone (M3.3, in the write transaction).
- [x] M3.2 OPAQUE register/login; session tokens (short-lived + refresh), device registration.
  - Per D26. `pwm-crypto`: OPAQUE client and server (`opaque-ke` 4.0.1, ristretto255, 3DH, KSF Identity), `ServerSecrets` (versioned blob, D12 fake pre-login answers), session tokens (`pwma_`/`pwmr_`, SHA-256 hashes), device names sealed under `sym_key` (`pwm/device-name`), login state sealed at rest (`pwm/login-state`, found by fuzzing). Vectors: `device_name.json` (Rust + Node), `opaque.json` (Rust only: replays the server steps and logs in with the frozen record). Server: `/v1/auth/{prelogin,register,login,refresh,logout}`, `GET /v1/account`, `GET/DELETE /v1/devices`; migration 0002 (`sessions`, `login_attempts`, `opaque_record` NOT NULL) with a v1-data migration test; `pwm-server keygen`. New fuzz target `opaque_messages`. Deferred: email verification (registration answers 409 for a taken email, so it reveals which emails have accounts; fixed when an email provider lands); approving a new device from an existing one (later milestone); purging expired sessions in the background (expired rows are refused, not yet deleted); rate limits (M3.5); `getrandom` 0.2 `js` backend for OPAQUE on WASM (M3.7). `cargo deny` ignores RUSTSEC-2026-0387 (crypto-bigint 0.5 via opaque-ke 4.0, not reachable from the ristretto255 suite) until opaque-ke ships a stable release on elliptic-curve 0.14; recheck at M3.7.
- [x] M3.3 Item endpoints with revision checks (`If-Match`), delta sync `since=rev`.
  - Per D27. `POST/GET /v1/vaults`; `PUT/DELETE /v1/vaults/{id}/items/{item_id}` with `If-None-Match: *` / `If-Match: "<revision>"` (428 without, 412 with the current item, 409 `item_deleted` for a tombstoned id); `GET /v1/vaults/{id}/items?since=&limit=` reads one REPEATABLE READ snapshot. `seq` is bumped under the vault row lock. Revisions and seqs are decimal strings on the wire (canonical, no leading zeros). Path, query and body errors are plain `invalid_request` (`/v1/devices/{id}` too). Tests on Postgres cover every precondition case, viewers, editors and outsiders, paging over items and tombstones, and concurrency (gapless seqs, one winner per revision, a reader never sees a gap); removing the lock or the snapshot fails them. Deferred: adding members (sharing milestone); size limits and per-plan vault limits (M3.5); a client whose `since` is ahead of the server (e.g. after a restore) gets a lower `next` and must resync from 0, which `pwm-sync` handles in M3.4; tombstone compaction.
- [x] M3.4 `pwm-sync` client: offline queue, retry with backoff, conflict → keep both in history.
  - Per D28. A sans-IO `SyncEngine` over the `Store` trait:
    - **API:** `start_sync` / `start_create_vault` → `Step::Send` / `Wait` / `Done`; `receive` and `network_failed` feed answers back.
    - **Local changes:** `save_item` / `delete_item` follow the revision rules, and `purge_trash` wires up the M2.2 purge.
    - **Retries and tokens:** backoff with full jitter that respects `Retry-After`; one token refresh per request; signed out and revoked outcomes.
    - **Recovery:** detects its own write coming back by comparing ciphertext, and resyncs when the server is behind.
    - **Requests and responses** wipe on drop and print no headers or bodies.
    - **`pwm-vault`:** `Item::keep_conflicting` merges the loser's history.
    - **Server:** revoked devices get 403 `device_revoked` (implementation note in D28).
  - **Tests:**
    - 19 rule tests against an in-memory D27 server with fault injection.
    - A randomized 3-device test over 40 seeds that checks convergence and that no edit is lost.
    - The engine against the real server on Postgres.
    - Mutation checks: no history union, no own-write check, delete beating a newer edit, and a tie going to the local version each fail a test.
  - **Deferred:**
    - FFI/WASM bindings for the engine (M3.7 for WASM, M4/M5 for the apps).
    - Pulling while locked, and push notifications.
    - Shared vaults and the read-only role in the cache (sharing milestone).
    - A per-vault restore epoch, so devices can detect a restored server that has already handed out new seqs (M3.6 runbook).
    - Purging session rows of revoked devices with other expired sessions.
  - **Known edge cases** (no data lost):
    - A vault create whose answer is lost and is then retried makes a second, empty vault.
    - Deleting an item whose first upload's answer was lost brings it back on the next pull.
- [x] M3.5 Rate limiting, request size limits, structured logs with no secrets (log scrubber test).
  - Per D29:
    - **Sizes:** bodies up to 16 KiB, item writes up to 384 KiB, blobs up to 256 KiB (413 `too_large`).
    - **Counts:** at most 100 vaults per account and 50,000 live items per vault (403 `limit_reached`), counted under a lock so racing requests can't pass.
    - **Rates:** exact GCRA buckets per IP for registration, login and refresh, and per account for everything else (429 with `Retry-After`). The client IP comes from `PWM_TRUSTED_PROXIES` rules; IPv6 counts per /64.
    - **Logs:** JSON lines (`PWM_LOG_FORMAT`), one per request: request id (also `x-request-id`), method, route template, status and duration. No path, IP or ids. Panics log only where they happened.
    - **Scrubber:** removes tokens, emails and long base64 runs from every line.
    - **Core:** `encrypt_item_fitting` drops the oldest history versions until an item fits, otherwise "item too large". `pwm-sync` uses it for saves, merges and copies.
  - **Tests:** sizes at the limit and one byte over, both count caps, every rate class with a fake clock, trusted proxies and forged headers, request ids, scrubber unit tests, and the log test rewritten for the full flow through the real pipeline. Mutation checks: with the scrubber off, or the raw path logged instead of the route, the log test fails.
  - **Deferred:**
    - Shared limits across several server instances (M10).
    - Per-plan caps (M9).
    - A general per-IP limit on authenticated endpoints, for floods of invalid tokens (with M10 operations).
- [x] M3.6 `docker compose` for local dev; same image is the future self-host image.
  - Per D30:
    - **Image:** a `Dockerfile` building a static musl binary on `scratch`, with the CA bundle and a non-root user. No shell, so no GPL software ships.
    - **`compose.yaml`:** Postgres 17 plus the server on `127.0.0.1:8080`, read-only, all capabilities dropped, `no-new-privileges`, secrets from a gitignored `.env`.
    - **New `just` recipes:** `secrets`, `server`, `smoke` and `db-tls`.
    - **Postgres TLS:** rustls with the system roots (`verify-full` for remote databases), tested against a TLS Postgres in CI.
    - **Restore detection:** migration 0003 adds `server_meta.epoch`, `pwm-server mark-restored` replaces it, and `GET /v1/vaults` sends it as `pwm-epoch`. The cache moves to schema v2 (`sync_meta`, frozen fixture and migration test), and the engine pulls every vault again when the epoch changes. This closes the D28 open risk.
    - **`pwm-server healthcheck`** and **`keygen --env`**.
    - **`docs/DEPLOYMENT.md`:** settings, the TLS proxy (with a Caddy example), backups, and the required `mark-restored` step.
  - **Tests:**
    - cache v1→v2 and the v2 fixture
    - epoch round trip and `clear`
    - an engine test where the restored server has already handed out new seqs, so only the epoch reveals the restore (it fails with epoch detection off)
    - server epoch and health check tests, and the TLS test
    - a new CI `image` job: compose build, a check that the image has no shell, compose up and wait, then the smoke test before and after `mark-restored`.
  - **Deferred:**
    - An arm64 image, pushing to a registry, and signing (M10.1).
    - Full self-hosting docs (M11).
    - Running `mark-restored` automatically from restore tooling.
- [x] M3.7 FFI (`pwm-ffi` via UniFFI) and WASM (`pwm-wasm`) builds of core exposing account, vault, sync, generator, autofill APIs.
  - Per D31:
    - **New `pwm-client`:** sign-up, login (pre-login → OPAQUE → keyset), offline unlock and lock, items as D18 JSON (save with history, trash, delete, purge), sync and vault creation as steps, the generator and strength, and stable error codes.
    - **`pwm-store`:** `MemoryStore` with a versioned `PWMS` snapshot (frozen fixture).
    - **`pwm-ffi`:** UniFFI 0.32 proc-macros over a SQLite-backed client, with `pwm-ffi-bindgen` for generating the bindings.
    - **`pwm-wasm`:** wasm-bindgen over a `MemoryStore` client, with errors as JS `Error`s carrying `code`. `getrandom` uses `wasm_js` / `js` in every version in the tree, which closes the M3.2 deferral.
    - **New `just` recipes:** `wasm` and `bindings`.
  - **Tests:**
    - offline client tests
    - an end-to-end test against the real server: sign-up → first vault → item → sync, login on a second device, every login failure answering `invalid_credentials`, lock/unlock, and a snapshot reloaded offline
    - `MemoryStore` parity with SQLite, snapshot round trip and damage tests
    - **New CI jobs:** `wasm` (Node smoke test: generator, `zxcvbn` on wasm32 — the M2.4 deferral — sign-up with Argon2id in WASM, snapshots) and `bindings` (Swift and Kotlin smoke programs on Linux).
  - **Deferred:**
    - Autofill in the API (M4.1, when `pwm-autofill` has an engine).
    - The extension's IndexedDB persistence and its Vitest tests (M4.2).
    - Biometric unlock (M5/M6).
    - Device rename.
    - iOS/macOS and Android builds of the library (M5/M6).
  - **Audit:**
    - `cargo deny` ignores RUSTSEC-2026-0374 (`basic-toml` unmaintained). It is pulled in only by the never-shipped `pwm-ffi-bindgen` tool, through UniFFI's templates; the library the apps link does not contain it.
    - RUSTSEC-2026-0387 was rechecked: `opaque-ke` still has only a 4.1 pre-release, so its ignore stays; recheck at M4.

**Gate — Private beta:** create account on macOS app (M5 skeleton) and extension, items sync both ways.

---

## Phase 1 — MVP and launch

### M4 — Browser extension + autofill engine
- [ ] M4.1 `pwm-autofill`: PSL origin matching, field classifier, multi-step state, rules file format + signature check.
- [ ] M4.2 MV3 extension shell: popup (search, copy, generator), unlock (master password, or via desktop app native messaging + biometrics).
- [ ] M4.3 Fill on user action; inline icon in isolated iframe/closed shadow root; visibility/overlay checks before fill (`AUTOFILL.md`).
- [ ] M4.4 Save/update prompts after submit.
- [ ] M4.5 Passkey support (WebAuthn interception where browser allows; else defer to platform provider).
- [ ] M4.6 "Autofill didn't work here" report with form fingerprint (no values).
- [ ] M4.7 Autofill harness: fixture sites incl. clickjacking/overlay attacks that must not fill; top-1,000 runner + JSON report.
- [ ] M4.8 Builds for Chrome, Edge, Firefox, Brave, Opera; Safari via macOS app (M5).

**Done when:** fixture suite 100%; top-1,000 classification ≥ 90% (target 95% by launch); overlay attacks never fill.

### M5 — iOS + macOS
- [ ] M5.1 Shared Swift package over UniFFI core; design tokens.
- [ ] M5.2 Onboarding: create account, Emergency Kit PDF, sign in on new device (password + Secret Key, or approve from existing device).
- [ ] M5.3 Vault list, search, item detail/edit, generator, health report, settings, lock timeout.
- [ ] M5.4 Face ID / Touch ID unlock via Secure Enclave-wrapped key.
- [ ] M5.5 iOS AutoFill Credential Provider + passkeys + QuickType identity sync.
- [ ] M5.6 macOS app + Safari Web Extension; native-messaging host for other browsers' biometric unlock.
- [ ] M5.7 Screenshot/screen-recording protection on unlock and revealed secrets.

### M6 — Android
- [ ] M6.1 Compose app over UniFFI core, same screens as M5.3.
- [ ] M6.2 BiometricPrompt + Keystore-wrapped key.
- [ ] M6.3 AutofillService + Credential Manager provider (passwords + passkeys), inline suggestions.
- [ ] M6.4 FLAG_SECURE on sensitive screens.

### M7 — Windows + Linux desktop
- [ ] M7.1 Tauri app (pending D3) with same screens; Windows Hello unlock; Linux unlock via password (+ polkit/fprintd later).
- [ ] M7.2 Native-messaging host for browser unlock on Windows/Linux.
- [ ] M7.3 Signed installers (MSIX/EXE, AppImage, .deb, .rpm, Flatpak).

### M8 — Importers (`pwm-import`)
- [ ] M8.1 Bitwarden JSON (incl. encrypted export), 1Password `.1pux`, Keeper JSON, NordPass CSV, Proton Pass ZIP/JSON, Dashlane CSV/JSON, KeePass `.kdbx` (KDBX 3.1/4), LastPass CSV, generic CSV with column mapper.
- [ ] M8.2 Import in memory; if a file was picked, offer to securely delete the source export afterwards.
- [ ] M8.3 Fuzz every parser; sample files in `core/pwm-import/fixtures/` (synthetic data only).

### M9 — Plans and billing
- [ ] M9.1 Entitlements service (decision D6); Stripe web checkout; App Store / Play Billing.
- [ ] M9.2 Implement every commitment in `PRICING.md` with tests (price lock, reminders, cancel, refunds, export after lapse).
- [ ] M9.3 Plan gates in core (TOTP, attachments, share links) checked client-side and server-side.

### M10 — Launch readiness
- [ ] M10.1 Release pipeline: beta channels, staged rollout, auto-halt metrics, rollback (`QUALITY.md`).
- [ ] M10.2 Crash reporting with secret scrubbing; opt-in analytics.
- [ ] M10.3 Public status page, help center, in-app support entry.
- [ ] M10.4 Public autofill compatibility dashboard (static site fed by nightly JSON).
- [ ] M10.5 Bug bounty live; external audit fixes closed; audit reports published.
- [ ] M10.6 Private beta with ≥ 1,000 users; fix top issues.

**Gate — Public launch:** audit published, top-1,000 autofill ≥ 95%, crash-free ≥ 99.5% on all platforms.

---

## Phase 2 — Differentiate (outline; expand into tasks at Phase 1 end)

- M11 Storage modes: self-hosted server image + docs; local encrypted file mode with iCloud/Drive/Dropbox/WebDAV/LAN sync.
- M12 KeePass `.kdbx` live sync (read/write a user's KDBX file).
- M13 Family plan, shared vaults, one-time share links (free tier: 1/day).
- M14 Emergency access with time delay; attachments; cards/identities autofill.
- M15 Support tooling (in-app chat for paid); native Linux polish; health report extras.

## Phase 3 — Teams (outline)

- M16 Org model, admin console (web), policies, audit log.
- M17 SSO (SAML/OIDC), SCIM provisioning, LDAP/AD for self-hosted.
- M18 Email aliases, Travel Mode.
- M19 One-command self-host installer with auto-update channel.
