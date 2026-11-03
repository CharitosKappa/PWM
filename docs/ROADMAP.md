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

**M3 leftovers, now scheduled:**
- M4.2: create the first vault only if there is none.
- M4.9: dependency advisories.
- M5.3: device rename.
- M5.8 / M6.5: background sync and push, and the delete edge case.
- M10.7: email verification.
- M10.8: housekeeping and automatic restore.

**Gate — Private beta:** create account on macOS app (M5 skeleton) and extension, items sync both ways.

---

## Phase 1 — MVP and launch

### M4 — Browser extension + autofill engine
- [x] M4.1 `pwm-autofill`: PSL origin matching, field classifier, multi-step state, rules file format + signature check.
  - Per D33:
    - **Public Suffix List:** vendored and unmodified (version 2026-10-31, private section included), with our own matcher, and `just psl` to refresh it.
    - **Match modes:** `domain`/`host`/`starts_with`/`exact`/`regex`/`never`, using WHATWG URL parsing and IDN. Regex runs in linear time and is size-limited.
    - **Ranking and fill decisions:** `fill`, `warn` (`http`, regex only), `refuse` (a frame from another site, a blocked site) and `confirm`.
    - **Field classification:** `autocomplete` tokens first, then keywords in 7 languages, form shape and position. One-time codes are filled only at 0.8 confidence or more.
    - **Signed rules file:** Ed25519 with the label `pwm/autofill-rules`, two pinned keys, rollback and expiry checks, 1 MiB / 50,000-site limits.
    - **Multi-step state:** per tab, no values, 5-minute expiry, forgotten when the site changes.
    - **Exposed through `pwm-client`, UniFFI and WASM:** matches, classification, rules and steps. This closes the M3.7 deferral.
  - **Tests:**
    - every case in the official PSL test file
    - a match-mode table: IDN both ways, ports, IP addresses, `localhost`, `github.io`, look-alike domains, a catastrophic regex under 500 ms
    - classifier fixtures: login, sign-up, change-password, multi-step, codes, cards, addresses, honeypots, and other languages
    - rules signature, key rotation, tampering, rollback, expiry, limits, and a frozen vector (`core/test-vectors/autofill_rules.json`)
    - multi-step expiry, site change and the tab cap
    - the client end to end, and the WASM smoke test
    - fuzz targets `autofill_rules` and `url_match`.
    - **Mutation checks:** without the private section, the cross-site frame check or the rollback check, a test fails.
  - **Deferred:**
    - Creating the release keys and the offline signing tool, and downloading and storing rules (M4.2).
    - Equivalent domains (one login for several domains), considered with the rules file.
    - Android app matching via Digital Asset Links (M6.3).
    - Measuring classification against real sites (M4.7 harness).
- [x] M4.2 MV3 extension shell: popup (search, copy, generator), unlock (master password, or via desktop app native messaging + biometrics).
  - Per D34:
    - **Background service worker:** owns the WASM core, the cache (IndexedDB snapshot) and every server conversation. The popup (Preact) only sends messages and never receives keys, tokens, item passwords or code secrets.
    - **Session:** the unlocked keyset sealed under a random key (`pwm/session`) in `chrome.storage.session`, so a restarted worker stays unlocked. Tokens are sealed under the keyset (`pwm/device-tokens`) on disk.
    - **Auto-lock:** 15 minutes by default (1, 5, 15, 60 minutes or 8 hours), at once when the computer locks, and on browser close.
    - **Popup:** sign up (strength meter, Secret Key shown once, text Emergency Kit, a PDF since M5.2), sign in, unlock, search, item detail, copy username/password/current code (cleared after 30 s through an offscreen document), generator, sync status, lock, settings (auto-lock, a self-hosted server with permission asked at that moment), sign out.
    - **First vault only if there is none** (`start_ensure_vault` in `pwm-client`), closing the M3 leftover.
    - **Rules download:** every 24 hours ± 2 hours with a conditional GET, rollback refused, the last good file reloaded at start. `pwm-rules-sign` (`keygen` / `public` / `sign`) for the offline release keys.
    - **Build:** esbuild into `extension/dist/` (`just extension`) with a manifest, permission and remote-code check; Chromium only.
  - **Tests:**
    - **Rust:** session export/restore (bound to the account, wrong key refused, versioned, vectors), token sealing, first-vault-if-none against the real server, the current code (RFC 6238), and `pwm-rules-sign`.
    - **Vitest with fakes:** the popup never receives secrets, sign-up/sign-in storage, auto-lock and screen lock, restore after a worker restart, clipboard clearing, sync outcomes (wait, revoked, errors), settings validation, the rules schedule and rollback refusal, storage adapters, the HTTP loop, the manifest check and the design tokens.
    - **Vitest over the real WASM core** (`pnpm test:wasm`, in `just wasm` and the CI `wasm` job): the typings against the build and the snapshot round trip. This closes the M3.7 deferral.
  - **Deferred:**
    - **Release keys:** the founder runs `just rules-keygen` twice offline and adds the public keys to `RULES_KEYS`; until then downloaded rules are refused.
    - **Biometric unlock** through the desktop app (M5.6/M7.2), logged as a parity gap in D34.
    - **Loading the built extension in a real browser** comes with the M4.7 Playwright harness.
    - **Editing items in the popup:** items come from save prompts (M4.4) and the apps.
    - **Firefox and Safari builds** (M4.8, M5.6).
    - **Store icons and the product name** wait for D1.
- [x] M4.3 Fill on user action; inline icon in isolated iframe/closed shadow root; visibility/overlay checks before fill (`AUTOFILL.md`).
  - Per D35:
    - **Reaching pages:** `activeTab` + `scripting`. The content script is injected only when the user clicks Fill or presses the shortcut. The inline icon is opt-in: it asks for site access when turned on, then registers the content script. Sign-up offers it once.
    - **Popup:** the logins for the current site come first, each with Fill ("Fill anyway" under a warning).
    - **Shortcut (Ctrl+Shift+L / ⌘⇧L):** fills the one sure match, or opens the popup.
    - **Inline icon:**
      - **What it is:** a closed shadow root around the extension's own frame, with a one-time token and a dynamic URL.
      - **Clicks:** taken only when trusted and after 500 ms visible (`IntersectionObserver` v2). There are two clicks, the icon and the login.
      - **Per site:** "Don't show here".
    - **Checks before every fill, in the target frame:**
      - the field is connected, enabled and writable
      - it has a size and is in the viewport (scrolled into view once from the popup or shortcut)
      - visibility and opacity 1 on every ancestor, no filter or clip-path
      - nothing on top at the centre and two corners
      - `IntersectionObserver` v2 reports it visible
      If any check fails, nothing is filled.
    - **Values:** sent only to the chosen frame (`frameId` + `documentId`). The engine judges the URLs the browser reports. Content scripts never get the vault list.
    - **Codes:** filled when the engine is sure (≥ 0.8), otherwise copied and cleared after 30 s. Multi-step state is recorded after each fill.
    - **Build check:** allows exactly `activeTab`, `scripting`, the command, the inline frame's resources and the optional site access, and still no declared content scripts.
  - **Tests:**
    - **Vitest (116):**
      - the fill plan (forms, honeypots, multi-step, codes)
      - the checks before filling
      - the icon's click guard
      - popup and shortcut fills
      - decisions, frames and documents
      - the inline token, menu, hide and turning off
      - that content scripts never receive other items
    - **Real WASM core:** the new core calls exist, and the field JSON classifies.
    - **Playwright** (`tests/autofill-harness`, `just autofill`, CI job `autofill-harness`): the test build in Chromium against a local server and HTTPS fixture pages.
      - Must fill: a plain form, honeypots, multi-step, a code field, a same-site frame, an `http` page after confirming.
      - Must refuse: a transparent overlay, a nearly transparent ancestor, a frame from another site.
      - Inline icon: two checked clicks fill; a page-made transparent icon takes no click.
  - **Fixed in passing:** the background now reads login fields where the core puts them (`login`). This was an M4.2 bug: no usernames in the popup, and password copy failed.
  - **Deferred:**
    - **Cards and identities:** M14 (Phase 2, per PRODUCT.md).
    - **Firefox:** fills only in the top frame without `IntersectionObserver` v2 (M4.8).
    - **The full fixture set and the top-1,000 runner:** M4.7.
    - **Save prompts:** M4.4.
    - **The popup UI:** not driven by Playwright yet (the tests call the background directly).
- [x] M4.4 Save/update prompts after submit.
  - Per D36:
    - **Seeing submits:** with "Fill and save on sign-in pages" on (the D35 icon setting renamed), the content script reads the username, current-password and new-password fields when the user submits: a submit event, Enter in a field, or a click on the form's button (single-page apps). Nothing is read before a submit or from hidden fields, and two new passwords must agree.
    - **Where captures live:** in background memory only, for 5 minutes, per tab. They are gone on lock or when the tab closes.
    - **No wrong passwords:** an offer comes only if the tab moved to another page of the site or the password field went away within 10 s.
    - **The core decides** (`save_offer`): nothing, save, or update (same username, or the old password on a change-password form). It also writes the result (`save_login`, `update_password`, history kept). Frames from another site and blocked sites get no offer.
    - **The card:**
      - **Where:** fixed top right, in the extension's own frame in a closed shadow root.
      - **What it shows:** the site and username, never the password.
      - **Actions:** Save or Update (a pick when several logins match), Not now, Never for this site.
      - **Clicks:** taken only when trusted and visible for 500 ms.
    - **Toolbar:** a badge while an offer is pending, and the popup shows the offer. There the title and username can be edited before saving.
    - **"Save login from this page"** in the popup reads the typed fields with `activeTab`, without site access.
  - **Tests:**
    - **Rust:** the offer decision (new site, saved as is, new password, other username, change-password by old password, sign-up form, several candidates, frames, non-web pages, a `never_fill` site), save (origin, title, `domain` match), update (history kept, nothing else changed), and errors.
    - **Vitest (141):**
      - save after navigation, and the popup's edits
      - update and a wrong candidate
      - form gone (single-page apps), the multi-step username
      - no offer when the login failed, the user went to another site, the login is already saved, the switch is off, the site is marked Never, after 5 minutes, after lock
      - captures never in storage, a sign-in while locked, the card's token and actions
      - the manual save, the watched fields, message order
      - reading typed values
    - **Playwright:**
      - save after a sign-in that works, and the login fills next time
      - no card after a failed sign-in, or when already saved
      - a single-page sign-in
      - a card the page made transparent takes no click
      - the popup's manual save
      - a change-password form updates, with history
  - **Deferred:**
    - **Offering the generator on new-password fields:** a separate task.
    - **Choosing the vault for a new login:** the first vault for now. Shared vaults come in M10.
    - **Popup offers in Playwright:** driven through the background, not the popup's UI.
- [x] M4.5 Passkey support (WebAuthn interception where browser allows; else defer to platform provider).
  - Per D37:
    - **`pwm-passkey` (new core crate):**
      - the WebAuthn RP id rules (PSL, no public suffixes or IP addresses, HTTPS or `localhost`)
      - `clientDataJSON` from the browser-given origin
      - `authenticatorData` (UP, UV, BE, BS, sign count 0, PWM's AAGUID)
      - hand-written CBOR for the COSE key and the `none` attestation
      - ES256 with RustCrypto `p256`
      - the request parser with limits
    - **`pwm-client`:** checking a create (excluded, unsupported), storing a new passkey in the site's login for that username or in a new login, the candidates for a sign-in, and signing. All are in WASM and UniFFI, so the iOS and Android providers (M5.5, M6.3) reuse them.
    - **Extension:**
      - **The switch:** "Use PWM for passkeys" asks for site access and registers a page-world wrapper and an isolated bridge at `document_start`.
      - **The window:** calls go to a confirmation window (an extension page in its own window): create, pick the passkey, unlock first, or "Use another device or the browser", which calls the browser's original WebAuthn.
      - **Handed back without asking:** a locked vault (sign-in), no passkey for the site, no ES256, an excluded passkey, conditional UI, and frames of another site.
  - **Tests:**
    - **Rust:** RP id table, exact registration and assertion bytes, signature verification, refusals, request limits, CBOR against RFC 8949, frozen vectors with an independent Node check (`verify.mjs`), the client's storage, exclusion and candidate rules, and the `passkey_request` fuzz target.
    - **Vitest (161):**
      - what goes back to the browser
      - the SecurityError for another site's RP id
      - create, unlock-first, pick and sign
      - the browser choice, closing the window, the timeout
      - the switch
      - the WebAuthn JSON conversion and credential objects
      - the message guards
    - **Playwright:**
      - **The fixture:** a relying party page that verifies the attestation and the assertion signature with WebCrypto.
      - register then sign in through the window (PWM's AAGUID)
      - another site's RP id refused without a window
      - closing the window gives `NotAllowedError`
      - "Use another device or the browser" reaches the browser's own authenticator (a CDP virtual authenticator)
  - **Deferred:**
    - Passkeys in the inline icon's menu (conditional UI).
    - Managing passkeys in the popup.
    - Algorithms other than ES256.
    - Firefox and Safari (M4.8, M5.6).
    - The apps' passkey providers (M5.5, M6.3, M5.6/M7).
- [x] M4.6 "Autofill didn't work here" report with form fingerprint (no values).
  - Per D38:
    - **Core (`pwm-autofill::report`, through `pwm-client`, WASM and UniFFI):**
      - the v1 report: the page's origin only, what happened from a fixed list, and per frame its relation (top, same site, other site) and each input's structure, cleaned labels and names, and the engine's class
      - whether a fill would use it, the software versions, and whether a saved login matched (only when unlocked)
      - **Cleaning:** lower case, emails become `<email>`, runs of 3+ digits become `#`, tokens become `<token>`, at most 40 characters
      - **Limits:** 20 frames, 200 inputs, 32 KiB
    - **Server:**
      - **The endpoint:** `POST /v1/reports/autofill`, without auth, at 10 per hour per IP, with a 32 KiB body limit.
      - **Checked:** against the v1 schema, unknown keys refused.
      - **Stored:** in `autofill_reports` (migration 0004) with no account or IP, kept 90 days.
      - **Exported:** with `pwm-server reports export [--since]`.
    - **Popup:** "Autofill didn't work here?", unlocked or locked.
      - **Before sending:** pick what happened, then a summary and "Show everything" (the exact JSON).
      - **Sending:** only Send posts, to the hosted service, without credentials.
  - **Tests:**
    - **Rust:** cleaning, what a report holds and never holds, frames, problems, limits, web pages only, the client (classes from the core, matched only when unlocked).
    - **Server:** the schema check, storing without account or IP, refusing values and oversized bodies, retention, the per-IP limit, and the export.
    - **Vitest (169):**
      - frames and fill targets to the core
      - nothing sent on preview
      - locked reports
      - Send posts exactly the preview, anonymously, to the hosted service
      - errors and the browser name
    - **Playwright:** a form with a name and an email in its labels, a typed username and password, and a session in the URL. The report has `<email>` and `#`, and none of the rest.
  - **Deferred:**
    - **Tickets from reports and the public dashboard:** M4.7 / M10.
    - **Reports from the apps:** M5/M6 (the core is ready).
    - **The popup's report screen in Playwright:** the tests drive the background.
- [x] M4.7 Autofill harness: fixture sites incl. clickjacking/overlay attacks that must not fill; top-1,000 runner + JSON report.
  - Per D39:
    - **Fixture suite, data-driven:** `tests/autofill-harness/fixtures/fixtures.json` says what must happen on each of the 37 pages, and a test fails if a page has no entry.
      - **New:** open shadow DOM, closed shadow DOM (no form), a form drawn after load, controlled inputs, a split one-time code (copied), nested same-site frames, a form moved off-screen, clipped, or under a cover that lets clicks through, a frame covered by its parent page, an icon covered after it appears, and an icon moved under the cursor.
    - **Extension:** split codes are copied rather than filled, and an icon the page moves lets clicks through for 500 ms and restarts its wait.
    - **Top-sites runner** (`runner/run.ts`, `just top-sites`, nightly CI job `autofill-top-sites`):
      - **What it does:** fills a throwaway login on each real login page and checks which inputs got it. It never submits, clicks or gets past a challenge.
      - **Statuses:** pass, fail, blocked (challenge or HTTP error, left out of the pass rate) and error.
      - **The report:** `autofill-report.json` (v1), uploaded for 90 days.
    - **Release gate:** `runner/compare.ts` fails when the pass rate drops more than 0.5 points.
    - **The first site list:** 102 sites, 18 with selectors, unchecked until the nightly runs confirm them.
  - **Tests:**
    - **Fixtures:** every entry in `fixtures.json`, and two new icon attack tests.
    - **Runner** (`runner.spec.ts`): pass with selectors, without them, and on a username-first page; fail; blocked by a challenge or a 503; error on timeout; that it never submits; the totals; the gate at 0.4 and 0.6 points.
    - **Unit:** the split code.
  - **Deferred:**
    - **Tickets from user reports and the public dashboard:** M10.
    - **Checking the site list, 50+ selectors and growing to 1,000:** M10.
    - **The 90% / 95% targets:** measured nightly from now on, and gating the launch (M10).
- [x] M4.8 Builds for Chrome, Edge, Firefox, Brave, Opera; Safari via macOS app (M5).
  - Per D40:
    - **Two builds from one tree:** `node scripts/build.mjs` builds `dist/chromium/` (Chrome, Edge, Brave and Opera) and `dist/firefox/`, with a zip of each. `--target` builds one.
    - **Firefox:**
      - an event page, and the clipboard written from it (no offscreen document)
      - add-on id `pwm@pwm.invalid` until D1, Firefox 128+
      - not minified, for review
      - server access asked for at sign-in
      - no inline icon or save card (no `IntersectionObserver` v2): saves are offered on the toolbar button, and fills happen only in the top frame
      - passkeys where the page script registers, and a clear message where it does not
    - **Build check per target:** exact permissions, background, web-accessible resources and gecko settings.
  - **Tests:**
    - **Unit:** both manifests pass and are refused when changed; no icon without the inline UI, while sign-ins are still watched; the save offer goes on the badge only; passkeys unsupported.
    - **CI:** both builds, `web-ext lint`, and a Firefox smoke test (install, popup, a score from the background's core). The zips are uploaded.
    - **Edge, Brave and Opera:** a manual checklist for release candidates in `docs/QUALITY.md`.
  - **Deferred:**
    - **The Firefox smoke test:** signing up and filling, which need a host-access grant that WebDriver cannot give.
    - **Store listings, signing, the AMO source zip and data-collection declaration:** M10.
    - **Safari:** M5.6.
    - **Firefox for Android:** later.
  - **Parity gaps (D40):** on Firefox, the inline icon, the save card and filling in frames.
- [x] M4.9 Dependency review of the `cargo deny` ignores, from M3:
  - **RUSTSEC-2026-0387:** drop it once `opaque-ke` has a stable release on `elliptic-curve` 0.14, after rerunning `opaque.json`.
  - **RUSTSEC-2026-0374:** drop it once UniFFI no longer pulls in `basic-toml`.
  - **Either way:** upgrade, or keep the ignore with its reason refreshed.
  - **Done 2026-11-02, both ignores kept, with their reasons refreshed in `deny.toml`. Recheck at M5.1:**
    - **RUSTSEC-2026-0387:** no upgrade exists. The latest `opaque-ke` (4.1.0-pre.2) and `voprf` (0.6.0-pre.1) are still on `elliptic-curve` 0.13. `opaque.json` was not rerun, since nothing changed.
      - Still not reachable from the ristretto255 suite.
      - The `crypto-bigint` 0.7 that `p256` brings (D37) is 0.7.10, past the fix.
    - **RUSTSEC-2026-0374:** `uniffi` 0.32.2, the latest, still pulls in `basic-toml` through `askama` 0.16.1.
      - It runs only inside a derive macro at build time.
      - `pwm-ffi` built on its own does not contain it.
- [x] M4.10 Health report, device list and export in the extension (D43, D44: a full page, `manage.html`), with the WASM calls from M5.10. Export must be done before the beta (M10).
  - Per D44: `manage.html`, opened from the popup's Settings, with Password health (leaked, weak, reused; "Change" opens the login's site, where the update offer saves the new password), Devices (rename, sign out another device) and Export (encrypted `.pwmx`, or JSON/CSV after the warning; the master password every time; a download).
  - **The leak check** asks for HIBP access from a click, with no manifest change, and runs straight from the background.
  - **Parity:** the extension now matches iOS on these three, on Chromium and Firefox (Safari with M5.6).

**Done when:** fixture suite 100%; top-1,000 classification ≥ 90% (target 95% by launch); overlay attacks never fill. Since D39, the classification target is measured each night by the top-sites runner on a growing list, and gates the launch in M10, not the end of M4.

### M5 — iOS + macOS
- [x] M5.1 Shared Swift package over UniFFI core; design tokens.
  - Per D41: `apps/apple/PWMKit`, a SwiftPM package for iOS 17 and macOS 14:
    - **`PWMCore`:** the generated UniFFI file over `pwm_ffiFFI.xcframework`, built by `just apple`.
    - **`PWMKit`:**
      - **`Session`:** an actor that runs sign-up, login, unlock, sync and item edits over `URLSession`, with the Secret Key and tokens in the Keychain.
      - **`Item`:** typed access to D18 JSON that keeps unknown fields.
      - **Also:** `PWMError`, `Redacted`, `Generator` and `CacheLocation` (the App Group container).
    - **`PWMDesign`:** the design tokens in Swift, generated by `just tokens` and checked by `just check`, with SwiftUI colors (light/dark) and Dynamic Type text styles.
  - **Tests:**
    - **Linux, in the `bindings` job:** the item fixture round trip with unknown fields, read-only newer items, redaction, errors, the generator and the tokens.
      - **Offline session:** a lost connection, a weak password and the device id.
      - **Against a real server:** sign-up, then login on a second device, and an item synced between them; lock and unlock with the wrong and the right password.
    - **macOS (new `apple` job, for PRs touching the package or the core):** the same tests and an iOS simulator build.
  - **Deferred:**
    - **To M5.2, with D1 and the Apple developer account:** app targets, signing, bundle and team ids, App Group and Keychain access group ids.
    - **Face ID / Touch ID:** M5.4.
    - **Background sync:** M5.8.
- [x] M5.2 Onboarding: create account, Emergency Kit PDF, sign in on new device (password + Secret Key). Approving from an existing device is M5.9 (D42).
  - **`pwm-kit` (new core crate):** the Emergency Kit as a one-page PDF (A4 or Letter, deterministic) and its QR code (`pwm-kit:1?…`, never the password), through UniFFI and WASM.
    - **The extension** now downloads the same PDF instead of a text file.
    - **`sign_out`:** a new core call that wipes the cache.
  - **iOS app:** `apps/apple/project.yml` (XcodeGen) with placeholder ids until D1, and no entitlements yet.
  - **`PWMUI` (new package target):** the screens.
    - welcome, with the server choice
    - create account (strength meter, at least 3)
    - the Secret Key and the kit (share sheet, a local-only copy that expires after 30 s)
    - sign in, typed or scanned from the kit with VisionKit
    - unlock
    - a placeholder vault screen
  - **Tests:**
    - golden PDFs, checked by `qpdf --check` in CI
    - a scan test of the printed QR code
    - a new fuzz target, `kit_qr_parse`
    - view-model tests on Linux and macOS
    - a sign-out and kit test against a real server
    - an XCUITest in the macOS job: create an account, then sign in again with the Secret Key, lock and unlock
  - **Deferred:**
    - **Onboarding on macOS:** M5.6, with the same `PWMUI` screens. Logged in D42.
    - **The App Group and the shared Keychain group,** with the move of the cache and keys into them: M5.5.
    - **Signing for devices:** waits for the Apple Developer account.
    - **The real name and icon:** D1.
    - **Approval from an existing device:** M5.9.
    - **Screenshot protection of the Secret Key screen:** M5.7.
- [x] M5.3 Vault list, search, item detail/edit, generator, settings, lock timeout. D43 moved the health report and the device list to M5.10.
  - Per D43, in `PWMUI`:
    - **Vault:** a split view (sections, items, detail) that starts at the item list on iPhone. Sections are All items, Favorites, each type, each folder and Trash. Search covers the title, username, websites and a card's last 4 digits.
    - **Items:** detail and edit for logins, cards, identities and notes, with other fields, folders (new ones made in the picker), tags, the one-time code with its countdown, passkeys and earlier passwords (read-only). Hidden values are masked until tapped. Leaving with changes asks first. Items from a newer app version are read-only.
    - **Trash:** Delete moves items there; Restore and Delete forever (asked first). Old trash is emptied after each sync.
    - **Generator tab and sheet:** the core's generator with password and passphrase options.
    - **Settings:** email and server, the lock timeout, Lock now, the Emergency Kit again, third-party notices, the version and Sign out (asked first).
    - **Lock timeout:** Immediately, 1, 5, 15 (default) or 60 minutes, or 8 hours in the background, on a clock that runs while the phone sleeps. Locking the device locks the vault.
  - **Core:** `normalize_totp` checks one-time-code setups (`invalid_totp`).
  - **`PWMKit`:** accessors for cards, identities, other fields and earlier passwords; `Session.purgeTrash`.
  - **Tests:** 23 new package tests on Linux (search, sections, sync notices, editing each type with unknown fields kept, read-only items, the lock timeout with a test clock, the generator). The UI test now adds a login with a generated password, finds, edits, trashes and restores it, locks on leaving, and finds it again after signing in as a new device.
  - **Deferred:**
    - **macOS:** M5.6, on the same screens.
    - **Re-login after a refused refresh token,** keeping unsent changes: M5.8.
    - **Screenshot protection and the app-switcher cover:** M5.7.
    - **A minimum length for bare one-time-code keys:** needs a decision (D43 notes).
- [x] M5.10 Health report, device list with rename and revoke, and export, on iOS (D43). Needs core and server work:
  - **Health:** the D22 checks through `pwm-client`, UniFFI and WASM, and the iOS HIBP fetcher.
  - **Devices:**
    - **Rename needs a new endpoint** (`PATCH /v1/devices/{id}` with a sealed name, D26 format).
    - **A newly signed-in device** sets its name right after login, because today only sign-up sends one.
  - **Export:** the D24 flows through `pwm-client`, with re-auth, in Settings.
  - Done per D43 (implementation notes there):
    - **Server:** `PATCH /v1/devices/{id}`: 204, 404 (not the account's), 409 `device_revoked`, 400 for anything but a 105-byte v1 envelope. Only ciphertext is stored; logs show the route, not the id.
    - **Core:** device list, rename and revoke as sync-engine tasks; the device name sent at the end of login; `health_report`, `breach_check`, `authorize_export` and `export` in `pwm-client`, through UniFFI and WASM.
    - **iOS (`PWMUI`, from Settings):** Password health (leaked, weak, reused; each opens the item to change it), the leak-check switch, Devices (rename, sign out another device), Export (encrypted with its own password, or plaintext after a warning; the master password every time).
    - **Tests:** server tests for every `PATCH` answer; end to end through the client (named at login, renamed, a name moved to another device does not open, revoked); offline client tests for health, breaches and export; 6 Swift view-model tests; 2 Swift tests against the real server; the UI test renames this device.
  - **Deferred:** the extension's screens (M4.10, done); macOS (M5.6); Android (M6.1).
- [x] M5.4 Face ID / Touch ID unlock via Secure Enclave-wrapped key.
  - Per D45:
    - **Core:** the keyset sealed under a random key (`pwm/biometric-unlock`, bound to the account and the device), through UniFFI, with vectors (Rust and Node) and fuzzing.
    - **iOS:** that key wrapped by a Secure Enclave P-256 key that needs Face ID or Touch ID with the current enrolment.
    - **Turning it on:** a Settings switch, and one offer after a master password unlock.
    - **The master password** is still asked every 14 days, after a sign-in, and after Face ID changes, which turns it off until the next password unlock.
  - **Tests:** core round trip, refusals and vectors; Swift with a fake enclave against the real server (the 14-day rule, a cancel, a changed face, signing out); 7 view-model tests; the UI test checks that the simulator says it is unavailable.
  - **Deferred:**
    - **The first real check** is on a device (no enclave in the simulator or CI).
    - **The AutoFill extension's use of it,** through the shared Keychain group: M5.5.
    - **Touch ID on macOS:** M5.6. **Android:** M6.2. **Windows Hello:** M7.1.
- [x] M5.9 Approve a new device from an existing one (D42 split it from M5.2): the Secret Key sent over an authenticated X25519 channel (`SECURITY.md`), relay endpoints on the server, and the approving side on every signed-in platform, starting with the extension. Needs its own decision, test vectors and security review; reuses the M5.2 kit scanner.
  - Per D46:
    - **The exchange:** fresh X25519 keys on both sides; a commitment from the new device; a 6-digit code compared on both screens; the Secret Key sealed for the new device. Normative text in `SECURITY.md`, with vectors (Rust and Node) and a fuzz target.
    - **The relay:** `/v1/approvals` (migration 0005), storing only public keys, the commitment and one ciphertext, handed out once; 10-minute lifetime; at most 3 pending per account; asking limited per IP; the same answer for any email.
    - **Asking:** "Sign in with another device" on iOS and in the extension. **Approving:** iOS Settings and the extension's manage page, with a count on the Settings tab and the toolbar badge.
  - **Deferred:**
    - **The security review** of the exchange: part of the external audit (M10.3).
    - **Push** instead of checking at sync or when the app comes to the front: M5.8.
    - **macOS:** M5.6. **Android:** M6.
    - **The scanner** is not used (D46 §2).
- [x] M5.5 iOS AutoFill Credential Provider + passkeys + QuickType identity sync.
  - Per D47:
    - **The extension** (`apps/apple/AutoFill`, iOS 17) shares the cache and Keychain group with the app; a one-time move brings existing installs over. It unlocks with Face ID or Touch ID only, lists the logins the core matches with a search, and fills on a tap (copying the one-time code for 30 seconds).
    - **Passkeys:** sign-in and creation through new hashed core calls, with vectors (`passkey_hashed.json`, Rust and Node).
    - **QuickType:** domains and usernames (and passkeys by RP id) in `ASCredentialIdentityStore`, replaced after every vault change, emptied by the Settings switch or at sign-out.
    - **Settings:** a card that opens AutoFill settings while PWM is not the provider.
  - **Deferred:**
    - **A check on a device:** iOS calling the provider, and the Secure Enclave key in the shared group. Not possible in CI.
    - **Fresh data in the extension** while the app hasn't run: background sync, M5.8.
    - **macOS:** M5.6. **Android:** M6.3.
- [ ] M5.6 macOS app + Safari Web Extension; native-messaging host for other browsers' biometric unlock.
- [ ] M5.7 Screenshot/screen-recording protection on unlock and revealed secrets.
- [ ] M5.8 Background sync, from M3:
  - **Pulling while locked:** store new ciphertext in the cache without keys, and merge conflicts at the next unlock. This keeps the AutoFill extension's data fresh.
  - **Background refresh:** iOS background refresh, and a timer on macOS.
  - **Server push:** a content-free "your vault changed" notification over APNs, which triggers a sync.
  - **Edge case in `pwm-sync`:** deleting an item whose first upload's answer was lost should not bring it back on the next pull. Keep a pending delete instead of dropping the record.

### M6 — Android
- [ ] M6.1 Compose app over UniFFI core, same screens as M5.3.
- [ ] M6.2 BiometricPrompt + Keystore-wrapped key.
- [ ] M6.3 AutofillService + Credential Manager provider (passwords + passkeys), inline suggestions.
- [ ] M6.4 FLAG_SECURE on sensitive screens.
- [ ] M6.5 Background sync and push (FCM) for Android, the same as M5.8 (WorkManager for periodic refresh).

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
- [ ] M10.7 Transactional email, from M3. It needs the provider decision in D32.
  - **Verify the email at sign-up.**
  - **Stop revealing existing accounts.** Today registration answers 409 for an email that already has an account. Instead, answer the same either way, and email the owner of an existing account.
  - **Account notices:** use email for these too, such as a new device signing in.
- [ ] M10.8 Server operations, from M3:
  - **Scheduled clean-up:** delete expired sessions and login attempts, and the session rows of revoked devices. They are already refused, just not deleted.
  - **Tombstone compaction:** decide the horizon first. A device whose cursor is older than the horizon must resync from 0.
  - **Backups with a tested restore:** the restore script runs `pwm-server mark-restored` automatically.

**Gate — Public launch:** audit published, top-1,000 autofill ≥ 95%, crash-free ≥ 99.5% on all platforms.

---

## Phase 2 — Differentiate (outline; expand into tasks at Phase 1 end)

- M11 Storage modes: self-hosted server image + docs; local encrypted file mode with iCloud/Drive/Dropbox/WebDAV/LAN sync.
- M12 KeePass `.kdbx` live sync (read/write a user's KDBX file).
- M13 Family plan, shared vaults, one-time share links (free tier: 1/day).
- M14 Emergency access with time delay; attachments; cards/identities autofill (moved here from M4.10, per PRODUCT.md: always with a confirmation step, rules for payment frames from other sites such as Stripe and Adyen, and for items without URLs; propose the rules in `DECISIONS.md` first).
- M15 Support tooling (in-app chat for paid); native Linux polish; health report extras.

## Phase 3 — Teams (outline)

- M16 Org model, admin console (web), policies, audit log.
- M17 SSO (SAML/OIDC), SCIM provisioning, LDAP/AD for self-hosted.
- M18 Email aliases, Travel Mode.
- M19 One-command self-host installer with auto-update channel.
