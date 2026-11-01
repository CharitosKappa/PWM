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
- [ ] M0.3 CI workflows per `QUALITY.md` (fmt-lint, core-test, audit) — green on empty crates.
- [ ] M0.4 `design/tokens.json` (colors light/dark, type scale, spacing, radius) — placeholder values, documented.
- [ ] M0.5 `SECURITY.md` (disclosure policy) at repo root, proprietary `LICENSE` notice, `cargo deny` + `license-checker` licence allow-list (decision S6) failing CI on any copyleft dependency, and `THIRD_PARTY_NOTICES` generation (`cargo about`, `license-checker`).

**Done when:** `just check && just test` pass locally and in CI.

### M1 — Crypto core (`pwm-crypto`)
- [ ] M1.1 `Secret<T>` wrapper: zeroize on drop, redacted Debug/Display, no Serialize by default.
- [ ] M1.2 AEAD wrapper (XChaCha20-Poly1305) with versioned envelope `{v, nonce, ct}` and AAD.
- [ ] M1.3 Argon2id KDF with stored params; HKDF helpers; Secret Key generation + human format (`A3-XXXXXX-...`).
- [ ] M1.4 Key hierarchy from `SECURITY.md`: AUK, auth secret, account keyset create/unlock, vault key wrap/unwrap.
- [ ] M1.5 X25519 share wrap/unwrap + Ed25519 sign/verify.
- [ ] M1.6 Test vectors in `core/test-vectors/*.json` for every operation; tamper tests (bit flips must fail).
- [ ] M1.7 Fuzz targets for envelope decode.

**Done when:** 100% of vectors pass; fuzzers run 10 min clean; no `unsafe`.

### M2 — Vault model (`pwm-vault`, `pwm-generator`, `pwm-health`)
- [ ] M2.1 Item types: login (multi-URL, TOTP secret, passkey), secure note, card, identity; custom fields; tags; folders. Versioned schema.
- [ ] M2.2 Item history (last 10 versions) and soft-delete (trash 30 days).
- [ ] M2.3 Local encrypted cache (SQLite storing ciphertext only) + migrations.
- [ ] M2.4 Generator: random password and EFF-wordlist passphrase; zxcvbn strength.
- [ ] M2.5 Health: weak, reused (compare hashes in memory), breached via HIBP k-anonymity range API.
- [ ] M2.6 TOTP (RFC 6238) code generation.
- [ ] M2.7 Encrypted export (`.pwmx`) and plaintext export (JSON/CSV) behind re-auth.

**Done when:** round-trip tests for all item types; export → import yields identical vault.

### M3 — Sync server + client sync
- [ ] M3.1 axum server, Postgres schema (accounts, devices, vaults, vault_keys, items, tombstones, shares, subscriptions); migrations with `sqlx`.
- [ ] M3.2 OPAQUE register/login; session tokens (short-lived + refresh), device registration.
- [ ] M3.3 Item endpoints with revision checks (`If-Match`), delta sync `since=rev`.
- [ ] M3.4 `pwm-sync` client: offline queue, retry with backoff, conflict → keep both in history.
- [ ] M3.5 Rate limiting, request size limits, structured logs with no secrets (log scrubber test).
- [ ] M3.6 `docker compose` for local dev; same image is the future self-host image.
- [ ] M3.7 FFI (`pwm-ffi` via UniFFI) and WASM (`pwm-wasm`) builds of core exposing account, vault, sync, generator, autofill APIs.

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
