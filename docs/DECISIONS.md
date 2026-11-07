# Decisions

Format: `D<n> — title · status (open | settled YYYY-MM-DD) · owner`. Claude Code: add a new entry instead of guessing when a spec is ambiguous.

## Open

- **D1 — Product name and brand** · open · founder. "PWM" is a placeholder.
  - **Visual direction: Calm** · settled 2026-11-05 · founder. Picked from three directions (Audit, Signal, Calm) on the design canvas "PWM app mockups": white and lavender surfaces, one indigo accent (`#4F3FD6` light, `#A79BFF` dark), radius 12 / 16 / 20, pill buttons. The values are in `design/tokens.json`, plus a new `accent-soft` color for secondary buttons and selected chips. Every text/background pair meets WCAG AA in both themes. The name stays open.
  - **Fonts: Manrope + Geist Mono** · settled 2026-11-05 · founder. Picked from three pairings on the canvas (Manrope + DM Mono, Geist + Geist Mono, Manrope + Geist Mono). Manrope is for all text. Geist Mono is for passwords, one-time codes and the Secret Key, because it keeps look-alike characters (`0 O`, `1 l I`) apart. Both are SIL OFL 1.1 (Manrope © The Manrope Project Authors; Geist © Vercel, with basement.studio).
    - **Licence:** `OFL-1.1` joins the S6 allow-list for **font files only, never code**, like CC-BY for the wordlist (D21). OFL allows bundling the fonts in a paid, closed-source product. Its conditions: ship the licence text with the files, don't sell the fonts on their own, and don't publish a modified font under a reserved name.
    - **How they ship:** the unmodified font files from the upstream releases sit next to a `NOTICE.md` (source, version, SHA-256, licence), as the wordlist does. The OFL text goes into `THIRD_PARTY_NOTICES` and the About screen. They don't come through cargo or npm, so `deny.toml` and `scripts/licenses.mjs` are unchanged.
    - **Tokens:** `font.family.sans` and `font.family.mono` now start with Manrope and Geist Mono, then the system fonts as fallbacks.
  - **Pill buttons** · done 2026-11-06:
    - **Shared screens and the inline frame** (the extension popup, the manage page, the desktop app): buttons use `radius.full`. Primary buttons sit on `accent`; a plain button is a secondary action, accent text on `accent-soft`.
    - **Apple apps:** `.buttonBorderShape(.capsule)` is set at the root and in the AutoFill extension.
    - **Android:** Material 3 buttons are already pills. `Secondary` is now a `FilledTonalButton` on `accent-soft`.
  - **Font files** · bundled 2026-11-06. The files are in `design/fonts/`: the variable fonts from the Google Fonts repository, unmodified and only renamed, with `NOTICE.md` giving the commit, sizes and SHA-256 hashes.
    - **Extension and desktop app:** the generated `tokens.css` declares both in `@font-face`. The builds copy the fonts, both OFL texts and `NOTICE.md` into `fonts/`.
    - **Apple:** `PWMDesign` carries them as package resources (`Fonts/`) and registers them for the process (`AppFonts`). Token text styles use them. `Font.secret(_:)` replaces the system monospaced font, and the root sets Manrope as the default.
    - **Android:** they are in `res/font` (`manrope`, `geist_mono`). The theme uses Manrope for every Material text style and Geist Mono for secrets.
    - **Copies:** `scripts/fonts.mjs` keeps the Apple and Android copies the same as `design/fonts`, and `just check` fails when they differ.
    - **Notices:** `just notices` writes `THIRD_PARTY_NOTICES/fonts.md` (the notice and both OFL texts), which the Apple and Android notice screens show.
  - **Not done yet:**
    - **Font names still hard-coded:** the family names are written in `AppFonts` (Swift), `Theme.kt` and `FONT_FILES` (`extension/scripts/tokens.mjs`) rather than generated from `tokens.json`. The extension's test checks that `tokens.json` and `FONT_FILES` agree.
    - **System controls:** on Apple, navigation titles and other system controls keep the system font.
- **D4 — Launch segment**: consumers first (this plan) vs SMB self-hosted first.
- **D5 — Company jurisdiction and hosting region**: EU (Greece/Switzerland/Germany hosting) is a selling point.
- **D6 — Entitlements/billing**: RevenueCat vs own entitlement service on Stripe + store receipts.
- **D7 — Bundled extras**: email aliases (planned Phase 3) yes; VPN no.
- **D8 — Final prices** (see `PRICING.md`) after user research.
- **D32 — Transactional email provider** · open · founder. Needed for M10.7: email verification at sign-up (so registration stops revealing which emails have accounts) and account notices. To choose: provider and region (EU hosting fits D5), and its licence and data-processing terms.
- **D9 — Licence-check scope** · proposed 2026-11-01 · Claude Code. Proposal: Rust checks every dependency including dev/build deps (`cargo deny`); npm checks only production dependencies of packages we ship (`scripts/licenses.mjs`, currently `extension/`), because dev tooling is never distributed. Today the npm dev tooling pulls in permissive licences that are not on the S6 allow-list (BlueOak-1.0.0, CC-BY-3.0 for SPDX data). Alternative: check dev deps too and add those licences to the allow-list.

## Settled

- **S1 — Shared Rust core, native UIs on Apple and Android** · settled 2026-10-31. Autofill extensions need native code; one core keeps crypto and features identical everywhere.
- **S2 — Zero-knowledge with Secret Key (2SKD) and OPAQUE auth** · settled 2026-10-31. See `SECURITY.md`.
- **S3 — User-initiated autofill by default; no clickable credential UI in page DOM** · settled 2026-10-31. Response to the 2025 clickjacking research.
- **S4 — Pricing commitments are product requirements** · settled 2026-10-31. See `PRICING.md`.
- **S5 — Password manager only**: the earlier idea of adding Vaultaire-style private photo/file vault features is out of scope · settled 2026-10-31.
- **S6 — Closed source (replaces former D2)** · settled 2026-10-31. All code (core, apps, extensions, server) is proprietary. Consequences:
  - Repo is private; `LICENSE` is an "All rights reserved" proprietary notice; EULA/Terms written by counsel.
  - Dependencies must use permissive licences only. `cargo deny` / `license-checker` allow-list: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, MPL-2.0 (unmodified files only). Data files only (never code): CC-BY-3.0 / CC-BY-3.0-US with attribution in the notices (D21). Font files only (never code): OFL-1.1 with the licence text in the notices (D1). **Deny GPL, AGPL, LGPL (static linking), SSPL, BSL and unknown licences.**
  - Never copy or adapt code from GPL/AGPL projects (Bitwarden, KeePassXC, KeePass, Proton Pass clients, etc.). Read their public docs/specs only; write our own implementation.
  - Ship third-party notices in every app and extension (generated by `cargo about` and `license-checker`).
  - Trust is earned without open code: published security whitepaper, independent audits with public summaries, public bug bounty, signed releases. Optional later: share core source with auditors or under NDA.
- **D3 — Windows/Linux UI framework** · settled 2026-11-06 · founder (approved as proposed).
  The plan defaults to Tauri ("Tauri (default in this plan, faster) vs native WinUI 3 + GTK (better platform feel, more work)"), and `ARCHITECTURE.md` already places a Tauri app in `apps/desktop`. M7 can't start until this is settled.

  Open:
  - which framework
  - how much of the extension's UI it reuses
  - what it means for licences (S6), security and tests
  - how M7 splits into tasks

  What the apps need:
  - **Screens:** the same ones as the Mac app (D42, D43, D48): sign-in, including "Sign in with another device" (D46), unlock, the vault, item editing, the generator, password health, import and export, devices and settings.
  - **Unlock:** Windows Hello on Windows, under D45's rules; the master password on Linux.
  - **Browser unlock:** a native-messaging helper (M7.2) with D48's protocol.
  - **Screen capture:** the window is left out of captures where the system allows it, as on macOS (D49).
  - **Installers:** signed (M7.3).
  - **A small team:** every UI toolkit is one more place where each feature is written and tested again.

  Proposal:
  1. **Tauri 2.** A Rust backend that links the core crates directly, with no FFI or WASM in between, and a web UI in the system's webview: WebView2 on Windows, WebKitGTK on Linux.
     - **Why:**
       - One UI for both platforms, in Preact and TypeScript like the extension, so the extension's screens are shared instead of written twice. That is three UI codebases in all (SwiftUI, Compose, Preact) instead of five.
       - The core's own Rust types, with no binding layer to keep in step.
       - Small installers: the system's webview, not a bundled Chromium.
       - Windows Hello, the helper, the tray and the lock rules need native code anyway, and the Rust backend is where they live.
     - **Licences, checked on crates.io on 2026-11-06:**
       - `tauri` 2.12, `wry`, the plugins and `tauri-driver`: Apache-2.0 or MIT. `tao`: Apache-2.0.
       - `webview2-com`, `webkit2gtk`, `gtk`: MIT. `windows`: MIT or Apache-2.0.
       - `cssparser` and `selectors`, which Tauri uses for its CSP: MPL-2.0, used unmodified.
       - All are on the S6 list.
       - **On Linux:** WebKitGTK, GTK and GLib are LGPL C libraries, linked dynamically from the system. S6 denies LGPL only for static linking.
       - **Before merging:** `cargo deny` and `just audit` run on the app's full dependency tree in M7.4.
  2. **Shared screens.** The extension's Preact screens (about 2,800 lines in `popup/` and `manage/`) move to a shared package used by the extension and the desktop app.
     - **The `api` each host supplies:** the extension's is its messages to the background, as now. The desktop app's is Tauri's `invoke`.
     - **The same design tokens and fonts (D1)** in both.
     - **Desktop-only screens:** the screens the popup lacks, such as health, devices, and the sidebar layout of a full window, are added to the shared package so the extension's manage page can use them later.
  3. **The webview's rules:**
     - **Content:** only the app's own bundled files. No remote URL is ever loaded.
     - **CSP:** strict: `default-src 'self'`, no `unsafe-eval`, no inline script. Navigation away and new windows are blocked.
     - **Commands:** Tauri's capability files allow only the app's own commands. The shell, file system and HTTP plugins are not exposed to the UI.
     - **Who does what:** the Rust backend does the networking, storage and core calls. The UI gets only what a screen shows, as the extension's popup does. Revealed fields cross into JavaScript as short-lived strings (D31).
     - **Isolation:** Tauri's isolation pattern is on, so every message from the UI is checked before it reaches the backend.
     - **Screen capture on Windows:** the window is left out of captures (`SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE`, which is Tauri's `content_protected`). Linux has no such API, which is already logged under SECURITY's threat model.
     - **TLS:** `rustls` with the system's verifier, plus certificate pinning as in `SECURITY.md`.
     - **Webview updates:**
       - **Windows:** WebView2 is updated by Windows.
       - **Linux:** the distribution's WebKitGTK is used. The oldest version we accept is set in M7.4, and CI tests on the oldest supported Ubuntu LTS.
  4. **What stays native, in the Rust backend:**
     - **Windows Hello:** through the `windows` crate. How the key is wrapped is decided in its own task, under D45's rules, with every crypto step in the core.
     - **The window and session:** the tray, a global shortcut, start at login, and locking when the computer sleeps or the session locks.
     - **Desktop feel:** native menus and title bar, the system's dark mode, and Ctrl+N and Ctrl+F as ⌘N and ⌘F on the Mac.
     - **The native-messaging helper (M7.2):** a second small binary in the same Rust workspace, speaking D48's protocol.
  5. **Tests:**
     - **Rust:** unit tests for the backend's commands.
     - **Vitest:** for the shared screens, run for both hosts.
     - **End to end:** through `tauri-driver` (WebDriver, which supports Windows and Linux) against a real server.
     - **In CI:** a `desktop-build` job, already listed in `QUALITY.md`, on `windows-latest` and `ubuntu-latest`.
     - **On this machine:** the app builds and runs locally on Windows 11, which ships WebView2.
  6. **Tasks** (M7.2 and M7.3 stay as they are):
     - **M7.1 The shared screens package.** The extension's screens move into it, with no change to how the extension behaves. Its current tests show it.
     - **M7.4 The Tauri app on Windows and Linux:**
       - the same screens
       - master-password unlock
       - the tray and the lock rules
       - screen-capture exclusion on Windows
     - **M7.5 Windows Hello unlock,** with its key-wrapping decision first.

  Risks:
  - **WebKitGTK:** it differs between distributions and is slower than WebView2. That's why there's a version floor and a CI run on the oldest supported LTS.
  - **The GTK 3 crates:** Tauri's Linux side uses them through WebKitGTK 4.1, and RustSec marks them unmaintained (RUSTSEC-2024-0415), although `gtk` 0.19 came out in 2026-09. If `cargo deny` still flags them in M7.4, they get an ignore with a reason and a recheck date, as `deny.toml` does for others.
  - **Platform feel:** a webview UI doesn't look fully native. The native menus, title bar and shortcuts above narrow the gap.
  - **Accessibility:** WebView2 exposes UI Automation and WebKitGTK exposes AT-SPI. Narrator and Orca are checked by hand before the beta (`QUALITY.md`).

  Alternatives:
  - **Native: WinUI 3 in C# on Windows, GTK 4 with libadwaita in Rust on Linux.** It gives the best platform feel, but adds two new UIs to SwiftUI, Compose and the extension, so each feature lands four times instead of three. C# also needs a third-party UniFFI generator and a .NET toolchain in CI.
  - **Electron:** it reuses the screens as Tauri does, but ships its own Chromium, whose security updates we would have to follow. Its Node backend would need the core through WASM or N-API instead of directly.
  - **Flutter:** BSD-licensed, but a new language and UI for only two platforms.
  - **Compose Multiplatform:** it would reuse Android's Kotlin screens, but ships a JVM, and OpenJDK is GPL-2.0 with the Classpath Exception, which is outside S6.
  - **Qt or Slint:** their licences (LGPL or commercial; GPL, royalty-free or commercial) are not on the S6 list.
  - **No desktop app, only the extension:** `PRODUCT.md` promises a native Linux app (P1, missing in Dashlane and NordPass), and Windows Hello unlock needs an app.
- **D10 — Envelope encoding** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. `SECURITY.md` names `{v, nonce, ct}` but no wire format. Proposal: fixed binary layout `v:u8 || nonce:24 bytes || ciphertext||tag:16 bytes`; `v = 1` means XChaCha20-Poly1305 and implies the layout, so nothing else is parsed. Base64url without padding when embedded in JSON. Decoder rejects unknown `v` and inputs shorter than 41 bytes. Alternative: CBOR map (self-describing, but a bigger parser to fuzz and audit).
- **D11 — AAD layout and ids** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. The spec gives `version || vault_id || item_id || revision` for items only. Proposal: every envelope binds a purpose label so ciphertexts cannot be swapped between contexts. AAD = `len(label):u8 || label || fields`, all integers big-endian, ids are 16-byte random UUIDv4. Labels and fields: item `"pwm/item"` `version:u8 || vault_id || item_id || revision:u64`; vault key under sym_key `"pwm/vault-key"` `version:u8 || vault_id`; account keyset under AUK `"pwm/keyset"` `version:u8 || account_id`. New contexts add a new label.
- **D12 — Password salt (`salt_pw`)** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. Source and length unspecified. Proposal: 16 random bytes generated at signup, stored server-side with the KDF params and returned by a pre-login endpoint. For unknown emails the server returns a deterministic fake salt and default params (`HMAC(server_secret, email)`) so the endpoint does not reveal which accounts exist. Alternative: derive the salt from the email (no lookup, but salt changes if the email changes).
- **D13 — HKDF inputs for `sk_key`, AUK and auth secret** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. Salts are unspecified. Proposal (HKDF-SHA256, 32-byte output, `account_id` = 16 bytes): `sk_key = HKDF(ikm = secret_key, salt = account_id, info = "pwm/sk/v1")`; `AUK = HKDF(ikm = pw_key XOR sk_key, salt = account_id, info = "pwm/auk/v1")`; `auth_secret = HKDF(ikm = pw_key XOR sk_key, salt = account_id, info = "pwm/auth/v1")`. Using `account_id` as salt separates accounts even if a Secret Key were ever reused.
- **D14 — Secret Key text format** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. The spec shows `A3-XXXXXX-...` without alphabet or length. Proposal: 128 random bits as 26 Crockford base32 characters (no I, L, O, U; case-insensitive; `0/O` and `1/I/L` mapped on input) plus 1 Crockford mod-37 check character to catch typos, grouped `A1-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXX`. The 2-character prefix is the format version (`A1` for v1; the spec's `A3` read as illustrative). The account id is not embedded; it is printed separately on the Emergency Kit.
- **D15 — Argon2id on WASM and mobile floor** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. WASM is single-threaded and the spec has a separate mobile floor. Proposal: KDF params are per account and identical on every client (they must be, to derive the same key). With `p = 4` on one thread the output is unchanged and the total work is the same, only not parallel; measure in M1.3 and, if the extension is too slow, unlock through the desktop app over native messaging. Params are chosen at signup: 64 MiB / t = 3 by default, the 32 MiB / t = 4 floor only when the signup device cannot allocate 64 MiB. Memory-limited contexts (iOS AutoFill extension) never run the KDF; they use the biometric-wrapped key.
  - Measured 2026-11-01 (M1.3, `examples/kdf_bench.rs`, one desktop CPU, single-threaded): native 58 ms default / 36 ms floor; WASM in V8 (Node WASI, no SIMD) 103 ms / 65 ms. The extension can run the KDF itself; native-messaging unlock stays for biometrics, not speed. Re-measure on a low-end laptop and phone before launch.
- **D16 — Master password normalization** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. The same password can reach the KDF as different bytes on different platforms (e.g. composed vs decomposed accents, non-breaking spaces), which would lock users out on one device. Proposal: before Argon2id, apply the RFC 8265 OpaqueString rules: map non-ASCII spaces to U+0020, normalize to Unicode NFC, keep case, never trim, reject empty passwords and control characters; then UTF-8 encode. Uses the `unicode-normalization` crate (MIT/Apache-2.0). Alternative: NFKC/NFKD (also folds full-width and compatibility characters; more forgiving, slightly fewer distinct passwords).
- **D17 — Vault share format and signatures** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. `SECURITY.md` says a shared vault key is "encrypted to the recipient X25519 key, signed with the sharer Ed25519 key" but not how. Proposal (ECIES-style, only crates already on the vetted list):
  1. **Signatures (all uses):** Ed25519 over `len(label):u8 || label || message`, so a signature made for one purpose (share, device approval, ...) can never verify for another. Verification uses strict mode (rejects malleable signatures and weak public keys).
  2. **Key agreement:** the sharer makes a fresh X25519 key pair per share; `dh = X25519(eph_secret, recipient_x25519_pub)`; an all-zero (non-contributory) result is rejected.
  3. **Share key:** `share_key = HKDF(ikm = dh, salt = eph_pub || recipient_x25519_pub, info = "pwm/share/v1")`.
  4. **Encryption:** the vault key is sealed as a normal v1 envelope under `share_key` with AAD label `pwm/share` and fields `version:u8 || vault_id || sender_ed25519_pub || recipient_x25519_pub`.
  5. **Signature:** label `pwm/share-sig` over `version:u8 || vault_id || recipient_x25519_pub || eph_pub || envelope`.
  6. **Wire format:** `v:u8 (= 1) || eph_pub:32 || signature:64 || envelope` (base64url in JSON). The recipient verifies the signature with the sender Ed25519 key (fingerprint check per `SECURITY.md`) before decrypting.
  Why: a fresh ephemeral key gives every share its own key; binding both public keys and the sender identity into the KDF, AAD and signature stops a share being re-targeted to another recipient, vault or sender. Alternative: HPKE (RFC 9180) via the `hpke` crate, which is standardized but adds a crate outside the vetted list and a second AEAD construction.
- **D18 — Item plaintext format, folders and tags** · settled 2026-11-01 · founder (approved as proposed). Record format in `SECURITY.md`. The specs fix how an item blob syncs but not what is inside it. Proposal:
  1. **Encoding:** UTF-8 JSON with a schema version `"v": 1` and a `"type"` field, parsed only in `pwm-vault`. Binary values (passkey private keys, credential ids) are base64url strings. JSON keeps imports, exports and debugging simple; size is not a concern for password items.
  2. **Forward compatibility:** unknown fields are kept and written back unchanged, so an older client editing an item created by a newer one never drops data. A client refuses to edit (but still shows read-only) an item whose `v` is newer than it understands.
  3. **Item types (v1):** `login` (username, password, URLs each with a match mode from `AUTOFILL.md`: domain, host, starts-with, regex, exact, never; optional TOTP as an `otpauth://` URI; passkeys with credential id, relying party id, user handle, user name, private key, sign counter), `note`, `card` (cardholder, number, expiry month/year, security code, brand), `identity` (name parts, email, phone, address lines, city, region, postal code, country, company). Every type also has title, notes, favorite flag, folder id, tags and custom fields (name, value, kind: text, hidden, url, date). Timestamps are inside the ciphertext as well as on the server record.
  4. **Folders:** a folder is an encrypted record of type `folder` (name, optional parent id) stored in the same item table, so the server cannot tell folders from items. Items reference folders by id. Deleting a folder moves its items to no folder.
  5. **Tags:** plain strings inside each item; there is no tag entity. The tag list is computed on the client.
  6. **Size padding:** before encryption the JSON is padded to the next multiple of 256 bytes (ISO/IEC 7816-4: one `0x80` byte, then zeros), so the server learns sizes only in coarse buckets and cannot tell a folder or short note from a login by length. Costs at most 255 bytes per item.
  Alternative to 1: CBOR (smaller, native binary), at the cost of harder debugging and a less common parser for importers/exporters.
- **D19 — Item history and trash storage** · settled 2026-11-01 · founder (approved as proposed). M2.2 needs "last 10 versions" and a 30-day trash; the specs do not say where they live. Proposal:
  1. **History inside the item:** each item carries `history`, its previous versions newest first, at most 10. A version is a full snapshot of the item fields (without its own history) plus `saved_at`. Saving pushes the previous version only when something other than `favorite` or `updated_at` changed. Sync conflicts (M3.4) push the losing version the same way, so no edit is ever silently lost.
  2. **Trash inside the item:** a `trashed_at` timestamp (ms) in the encrypted item. Trashed items are hidden from lists, search and autofill and shown in Trash, where they can be restored (clears `trashed_at`) or deleted now.
  3. **Purge on the client:** at each unlock a client permanently deletes items trashed more than 30 days ago, which sends the normal tombstone (`DELETE`) to the server. The server never learns which items are in the trash, only the final deletion.
  4. **Folders:** trashing a folder moves its items to no folder right away (as D18 does for deletion); restoring the folder does not move them back.
  5. **Schema:** both fields are added to item schema v1 rather than v2, because no v1 client has shipped yet; the v1 fixture is extended.
  Why: the server already sees every revision number, so keeping history inside the ciphertext reveals nothing new, and padding hides most of the size growth. Alternatives: separate encrypted history records (unbounded, but the server sees how often each item changes) and a server-visible trash flag with a server-side purge job (works when no client opens the vault for 30 days, but the server learns which items were deleted and when).
- **D20 — Local encrypted cache** · settled 2026-11-01 · founder (approved the plan in chat: bundled SQLite behind a storage trait, ciphertext-only tables, browser storage later). M2.3 says "SQLite storing ciphertext only + migrations" and nothing else. Decision:
  1. **Crate and backends:** a new `core/pwm-store` crate defines a `Store` trait. The `sqlite` feature (default) implements it with `rusqlite` and SQLite compiled in (`bundled`), so every native platform runs the same SQLite version. The extension (WASM) turns the feature off and gets an IndexedDB backend behind the same trait later (M3.7/M4). The store does no crypto; it persists envelopes produced by `pwm-crypto`/`pwm-vault`.
  2. **Contents:** only what the server also holds, plus local sync state: account id, password salt, KDF params, sealed keyset; per vault its id, wrapped vault key and sync cursor; per item its vault id, item id, revision, envelope and a sync state (synced, modified, deleted-pending-upload). No item names, URLs, usernames or timestamps in clear. The Secret Key and any unlocked key are never written here; they go to platform secure storage (Keychain, Keystore, Windows Hello/TPM) when the apps arrive.
  3. **Schema versioning:** `PRAGMA user_version` is the schema version; migrations are ordered SQL scripts applied on open in one transaction, so a crash leaves the old version intact. A database from a newer app version is refused, never downgraded or written. `PRAGMA application_id` marks the file as ours, so a foreign SQLite file is refused too. Each schema version has a frozen fixture (`tests/fixtures/store_v<n>.sql`) that later versions must keep opening.
  4. **Hygiene:** `secure_delete` on (deleted ciphertext is overwritten), foreign keys on, strict tables with length checks on ids. On Unix the file is created with mode 0600; app sandboxes cover the other platforms. Revisions are stored as SQLite integers, so values above `i64::MAX` are rejected.
  Why: the cache must never hold more than a server breach would reveal, so a stolen locked device gives an attacker nothing beyond what the zero-knowledge design already assumes they have. Alternative: SQLCipher (whole-file encryption), which needs a crypto backend outside the vetted list and adds little when every value is already ciphertext.
- **D21 — Generator defaults, randomness and the passphrase wordlist** · settled 2026-11-01 · founder (approved as proposed). The roadmap names "random password and EFF-wordlist passphrase; zxcvbn strength" and nothing more, and the EFF wordlist's licence (CC BY 3.0 US) is not on the S6 allow-list. Proposal:
  1. **Randomness:** every choice comes from the OS CSPRNG (`getrandom`) through one helper in `pwm-crypto` that picks a uniform index by rejection sampling (no modulo bias). No seeded or user-space RNG.
  2. **Password:** length 20 by default (8–128). Character classes: lowercase, uppercase, digits, symbols (`!#$%&*+-=?@^_`, which most sites accept), all on by default; at least one class must be on. Every selected class appears at least once: a candidate missing a class is thrown away and redrawn, which keeps the result uniform over all valid passwords. Option to leave out look-alikes (`0 O o 1 l I`).
  3. **Passphrase:** the EFF large wordlist (7,776 words, 12.9 bits per word), compiled into the binary. 5 words by default (64.6 bits; 3–20 allowed), separated by `-`. Options: other separators, capitalize each word, add one random digit to one word.
  4. **Wordlist licence:** add `CC-BY-3.0` (and the US port) to the allow-list **for data files only, never code**, with the attribution in `THIRD_PARTY_NOTICES` and the app's About screen. Alternatives: (a) build our own CC0 list of 7,776 common words (more work, and the EFF list was curated for memorability and to avoid offensive words); (b) a smaller public-domain list (weaker per word).
  5. **Strength:** the `zxcvbn` crate (Rust port, MIT) gives a 0–4 score, a crack-time estimate and feedback. The UI can pass context (email, item title) so a password built from them scores low. Inputs longer than 100 characters are scored on their first 100, so a pasted giant string cannot stall the UI. Generated values also report their exact entropy in bits. Signup keeps the existing rule (score ≥ 3).
  6. **Hygiene:** generated values are returned as `Secret<String>`; zxcvbn copies its input internally, so zeroizing it is best effort, as for item JSON in M2.1. Nothing is logged.
  7. **Settings:** the generator's options are a plain struct for now; when they sync (M5 settings), they get a versioned format like every other stored structure.
- **D22 — Health report: weak, reused and breached checks** · settled 2026-11-01 · founder (approved as proposed). The roadmap says "weak, reused (compare hashes in memory), breached via HIBP k-anonymity range API" and leaves open who talks to HIBP, which hash finds reuse, what counts as weak and whether the breach check is on by default. Proposal:
  1. **Scope:** the password of every login item that is not trashed and not empty. Read-only items (newer schema, unknown type) are checked too, since checking does not edit them. Card PINs and custom fields are out of scope for now.
  2. **Weak:** zxcvbn score below 3 (the signup threshold, D21), with the item's username, title and URL hosts as context, so `alice2024` for user `alice` counts as weak.
  3. **Reused:** two or more items with exactly the same password (case-sensitive). Passwords are compared as HMAC-SHA256 values under a random key made for each report run and dropped afterwards, so the in-memory index is useless after the run and is never written anywhere. The helper lives in `pwm-crypto` (it already has `hmac`/`sha2` through `hkdf`).
  4. **Breached (HIBP range API):**
     - The core computes SHA-1 of each password, sends only the first 5 hex characters, and matches the returned suffixes locally. SHA-1 is required by the HIBP protocol and used for nothing else; it comes from RustCrypto's `sha1` crate (MIT/Apache-2.0, same family as the vetted crates).
     - **The core does no networking.** `pwm-health` builds the prefix list and parses response bodies; each platform performs the HTTPS GET to `https://api.pwnedpasswords.com/range/{prefix}` through a small fetch trait. This keeps the core free of an HTTP stack (and WASM-friendly), and makes the parser testable offline.
     - Requests go **directly to HIBP, not through our server**, so our server never sees which prefixes belong to which account. Requests send `Add-Padding: true` (so response size does not hint at the prefix), no cookies and no account identifiers. Each unique prefix is requested once per run; padding rows (count 0) are ignored.
     - Results (breach counts) stay in memory for the session and are not stored in the vault or the cache in M2.5.
  5. **Default:** weak and reused checks are always on (fully local). The breach check is **on by default with a switch in settings**, explained in one plain sentence ("Checks your passwords against known leaks without sending them: only the first 5 characters of a hash leave your device"). Alternative: off by default (opt-in), which is the most conservative choice but means most users never see leaked passwords.
  Why: k-anonymity with padding is the standard way to check leaks without revealing passwords; keeping the request off our server avoids linking prefixes to accounts; the keyed in-memory hash meets the roadmap's "compare hashes" without creating an offline-crackable table.
- **D23 — TOTP inputs and code generation** · settled 2026-11-01 · founder (approved as proposed). D18 stores a login's TOTP as an `otpauth://` URI; the roadmap says only "TOTP (RFC 6238) code generation". Open: which URI variants and parameters are accepted, what happens to pasted bare secrets and non-standard codes (Steam), and where the code lives. Proposal:
  1. **Where:** the HMAC-based one-time-password step (RFC 4226 dynamic truncation over HMAC-SHA1/SHA-256/SHA-512) is a small function in `pwm-crypto`, using the `hmac`, `sha1` and `sha2` crates already there. Parsing `otpauth://` URIs and formatting codes live in `pwm-vault` next to the item model. The caller passes the current Unix time, so the core never reads the clock and every case is testable.
  2. **Accepted inputs:**
     - `otpauth://totp/...` with a base32 `secret` (case-insensitive; spaces and `=` padding ignored) and optional `algorithm` (SHA1, SHA256, SHA512), `digits` (6–8) and `period` (1–3600 s). Defaults: SHA1, 6 digits, 30 s, as in the Google Authenticator key-URI format.
     - A bare base32 secret, as many sites show under "can't scan the QR code?", using the defaults.
     - Steam Guard: `steam://<secret>` and `otpauth://...&encoder=steam` give 5-character Steam codes (HMAC-SHA1, 30 s, Steam's 26-letter alphabet). Steam is a common request and every major competitor supports it.
  3. **Stored form:** URIs are stored exactly as entered or imported, so unknown parameters (e.g. `image=`) survive. A pasted bare secret is turned into `otpauth://totp/<title>?secret=<SECRET>` when the item is saved, so every stored value is a URI.
  4. **Not supported (yet):** counter-based `otpauth://hotp` (it needs the counter written back on every use) and other proprietary schemes. Such a value is kept unchanged and shown as "unsupported code type", never deleted.
  5. **Output:** the code as `Secret<String>` plus the seconds left in the current period. Copying follows the existing clipboard rule (auto-clear after 30 s).
  6. **Vectors:** RFC 4226 Appendix D and RFC 6238 Appendix B (SHA-1, SHA-256, SHA-512) plus Steam cases go in `core/test-vectors/totp.json` and are checked by both the Rust tests and the independent Node verifier.
  7. **Plans:** TOTP is a Premium feature (`PRICING.md`); gating is done in M9.3, so the core generator does not check plans.
- **D24 — Export formats and re-authentication** · settled 2026-11-01 · founder (approved as proposed). Normative text in `SECURITY.md`. The roadmap says "encrypted export (`.pwmx`) and plaintext export (JSON/CSV) behind re-auth", done when export → import gives an identical vault. Open: what key protects `.pwmx`, the file layouts, what each format contains and how re-auth is enforced. Proposal:
  1. **Where:** a new `core/pwm-export` crate writes and reads our own three formats. It does no file I/O: it returns bytes, and each platform saves them through its own file picker. Competitor importers stay in `pwm-import` (M8), which will call these readers for our own files. There is no plan check: export works on every plan and after a subscription lapses (`PRICING.md` §7). The server has no export endpoint (`ARCHITECTURE.md`).
  2. **Re-auth for every export, enforced in the core:** both encrypted and plaintext exports ask for the master password again inside the export flow. Neither biometrics nor the unlocked session counts, because someone at an unlocked device who can choose an export password can read the result just as well. Export functions take an `ExportGrant`, which only `authorize_export(master password, …)` can create, after re-deriving the AUK and opening the sealed keyset (the same check as unlock). A grant works once and expires 5 minutes after it was made (the caller passes the time, as for TOTP).
  3. **`.pwmx` key:** a separate export password, chosen in the export dialog, so the file opens without the account: after the account is deleted, on another device, or if we disappear.
     - The password is normalized like the master password (D16) and must score ≥ 3 in zxcvbn (D21). The dialog offers a generated 5-word passphrase.
     - `key = HKDF(ikm = Argon2id(export password, salt, params), salt = salt, info = "pwm/export/v1")`, with a random 16-byte salt per file.
     - The params are the signup default (64 MiB, t = 3, p = 4) and are written in the file. The reader enforces the same bounds as the account KDF (`KdfParams::validate`), so a crafted file cannot exhaust memory or force weak params.
     - Alternative: encrypt to the account keys, which needs no extra password but is useless without the account, which is the main reason to keep an export.
  4. **`.pwmx` layout v1:** `"PWMX" (4 bytes) || v:u8 (= 1) || m:u32 || t:u32 || p:u32 || salt:16 || envelope`.
     - The envelope is a normal v1 envelope (D10) with the new AAD label `pwm/export` and fields `version:u8 || m || t || p || salt`, so changing any header byte makes decryption fail.
     - The plaintext is the JSON export below, padded to a multiple of 4 KiB (ISO/IEC 7816-4, as in D18), so the file size shows the vault size only in coarse steps.
     - One envelope holds the whole file; vaults are small (no attachments, S5).
  5. **JSON export (also the `.pwmx` payload):** UTF-8 `{"format": "pwm-export", "version": 1, "exported_at": <ms>, "vaults": [{"id": "<uuid>", "items": [{"id": "<uuid>", ...item JSON...}]}]}`.
     - Each item is its D18 JSON unchanged, including folders, trashed items, history, passkeys and unknown fields, so importing gives back exactly the same items with the same ids.
     - Vaults have no names yet. When names arrive, they become an optional field with a version bump and a migration test.
  6. **CSV export (for moving to another manager):** RFC 4180, UTF-8, with a header row and one row per login, note, card and identity that is not in the trash.
     - Columns: `type, title, folder, favorite, tags, notes, url, username, password, totp`, then `card_*` and `identity_*` columns, one per field. Multiple URLs or tags go in one quoted cell, one per line. The folder is its name, with nested folders written as `Parent/Child`.
     - Left out: history, trash, passkeys, custom fields, ids and timestamps. CSV cannot carry them well, and the JSON export is the complete format.
     - Uses `csv-core`, the allocation-free core of the `csv` project (licensed MIT OR Unlicense; we take MIT). It reads and writes through buffers we own, so they can be zeroized; the higher-level `csv` crate keeps its own buffers.
     - Cells starting with `=`, `+`, `-` or `@` are not escaped against spreadsheet formulas, because escaping would change passwords that start with those characters. The warning tells users not to open the file in a spreadsheet.
  7. **Warning and hygiene:** before a plaintext file is saved, one plain warning is shown: "This file is not encrypted. Anyone who gets it can read all your passwords. Delete it when you're done." Exported bytes are returned as `Secret<Vec<u8>>` so they are zeroized after saving; nothing is logged.
  8. **Tests:**
     - A round trip for every item type through all three formats; for CSV, the omissions above are listed and checked.
     - A frozen `.pwmx` v1 fixture that every later version must keep opening.
     - Vectors in `core/test-vectors/export.json`, checked by the independent Node verifier: KDF, HKDF, AAD and envelope.
     - A fuzz target for the `.pwmx`, JSON and CSV readers.
  Why: an export is mostly needed when the account is not: after deletion, during a move to another manager, or as an offline backup. A separate, strong export password keeps the file safe without the account. Requiring the master password closes the "unlocked laptop" route to a full copy of the vault. Keeping the export plaintext identical to the item JSON makes the round trip exact by construction.
- **D25 — Server schema, revisions and migrations** · settled 2026-11-01 · founder (approved as proposed). The roadmap names the stack (axum, Postgres, `sqlx` migrations) and the tables (accounts, devices, vaults, vault_keys, items, tombstones, shares, subscriptions) but not their columns. Open: what each table holds, who picks ids and revisions, how delta sync finds changes, and how migrations and DB tests run. Proposal:
  1. **Revisions vs change sequence:** these are two separate numbers.
     - **`revision`:** per item. The client chooses it (previous + 1, or 1 for a new item) because it is bound into the item AAD before encryption (D11). In M3.3 the server accepts a write only if `If-Match` equals the stored revision.
     - **`seq`:** a per-vault change counter, assigned by the server. Every write or delete bumps `vaults.seq` in the same transaction and stamps the row with it. Delta sync asks for `since=<seq>` and gets the items and tombstones with a higher `seq`.
     - Why: a server-assigned counter cannot be known before encrypting, and a per-item revision cannot order changes across a vault. Per vault, not global, so the counter shows nothing about other accounts' activity.
     - Both are `bigint` from 1 to 2^63 − 1 and go on the wire as strings (M1.6 finding). `ARCHITECTURE.md`'s `since=rev` becomes `since=seq`.
  2. **Ids:** client-generated UUIDv4, stored as `uuid`, because they are bound into AADs before upload. The server refuses an id that is already taken, including one that has a tombstone.
  3. **Tables (v1).** All times are `timestamptz` (UTC). Deleting an account cascades to everything it owns.
     - `accounts`:
       - `id`, `email` (unique, trimmed and lowercased), `kdf_m/t/p`, `salt_pw` (16 bytes), `sealed_keyset`, `x25519_pub` and `ed25519_pub` (32 bytes each, needed for sharing), `opaque_record` (null until M3.2 registration), `created_at`.
       - The email is the only personal plaintext. It is needed to find the account at login and to send the notices `PRICING.md` requires (the price-change and renewal emails).
     - `devices`:
       - `id`, `account_id`, `platform` (`ios`, `android`, `macos`, `windows`, `linux`, `extension`), `name_sealed` (the device name sealed by the client; its AAD label is defined with device registration in M3.2), `created_at`, `last_seen_at`, `revoked_at`.
       - No IP addresses or user agents.
     - `vaults`:
       - `id`, `seq`, `created_at`. No name; vault names come later, sealed by the client.
     - `vault_keys`:
       - Key `(vault_id, account_id)`: one row per member of a vault.
       - `role` (`owner`, `editor`, `viewer`), `wrapped_key`, `wrap` (`own` = sealed under the member's sym_key with AAD `pwm/vault-key`; `share` = the D17 wire from `shared_by`), `shared_by` (null for `own`), `created_at`.
       - Vault shares live here, not in `shares`.
     - `items`:
       - Key `(vault_id, id)`. `revision`, `seq`, `blob` (the D18 envelope), `created_at`, `updated_at`. Indexed on `(vault_id, seq)`.
       - Blobs stay in Postgres. The S3 store in `ARCHITECTURE.md` is for attachments, which come later; per-item size limits arrive with M3.5.
     - `tombstones`:
       - Key `(vault_id, item_id)`. `revision` (the one the delete was made against), `seq`, `deleted_at`. Indexed on `(vault_id, seq)`.
       - Kept indefinitely for now; compaction (with a "full resync" answer for an old `since`) is deferred.
     - `shares`: one-time links to people who are not users.
       - `id`, `account_id`, `blob` (sealed with the link key, which never reaches the server), `expires_at`, `max_views` (null = unlimited), `views`, `created_at`. Revoking deletes the row.
       - The blob format is defined when the feature ships.
     - `subscriptions`:
       - At most one row per account; no row means Free.
       - `plan` (`premium`, `family`, `teams`), `status` (`active`, `past_due`, `canceled`, `expired`), `provider` (`stripe`, `app_store`, `play`), `provider_ref`, `price_minor` and `currency`, `current_period_end`, `price_locked_until` (`PRICING.md` §1), `cancel_at_period_end`, `created_at`, `updated_at`.
       - Provider-neutral while D6 is open. Family and Teams membership come in a later migration.
  4. **No plaintext slips in:** a test lists every text, varchar and JSON column in the schema and fails on any column not on an allow-list. Today the list is `email`, `provider_ref` and the checked enum columns above. A new free-text column needs a reason in review.
  5. **Migrations:**
     - Plain SQL files in `server/migrations/`, embedded in the binary with `sqlx::migrate!` and applied at startup under sqlx's advisory lock, so several instances can start at once.
     - The server refuses a database with a migration it doesn't know (a newer version) or with an edited migration (checksum mismatch), as D20 does for the local cache.
     - Changes always add a new migration, and a checked-in schema snapshot (tables, columns, types, constraints) makes any change visible in the diff.
  6. **Queries** are runtime-checked (`sqlx::query`), with integration tests behind them. The compile-time macros need a live database or checked-in offline data at build time; reconsider them in M3.3, when real queries arrive.
  7. **Server binary:**
     - **Config:** `PWM_DATABASE_URL` (required) and `PWM_LISTEN` (default `127.0.0.1:8080`).
     - **`GET /healthz`:** 200 when Postgres answers, 503 otherwise, with no version or details.
     - **Shutdown:** graceful, on Ctrl-C or SIGTERM.
     - **Logs:** startup and migration lines only. Request logging waits for the M3.5 scrubber.
     - **TLS:** HTTPS is terminated by a reverse proxy (documented with M3.6). Postgres TLS (rustls with the OS roots, because `webpki-roots` is CDLA-Permissive-2.0, which is not on the allow-list) arrives with docker compose in M3.6.
     - **Crates:** `axum`, `tokio` and `sqlx` (postgres, runtime-tokio, migrate; no default features), all MIT or Apache-2.0.
  8. **Tests (real Postgres, as `QUALITY.md` asks):**
     - Each test creates a throwaway database from `PWM_TEST_DATABASE_URL`, migrates it and drops it afterwards. `just db` starts a disposable `postgres:17` container.
     - Without the variable, the DB tests print "skipped" and pass. A new blocking `server-test` CI job runs a Postgres service and sets `PWM_REQUIRE_DB=1`, which turns a skip into a failure.
     - Covered:
       - Migrations apply, and re-running them is a no-op.
       - An unknown or edited migration is refused.
       - The schema snapshot matches.
       - The plaintext allow-list holds.
       - Constraints work: revision ≥ 1, key and salt lengths, enum values, cascade on account delete.
       - `/healthz` answers correctly with the database up and down.
  Why: the per-item revision has to exist before encryption, and sync needs an order the server controls, so they must be two numbers. Storing only ciphertext, sizes, ids and times (plus the email) keeps the zero-knowledge rule checkable by a test, not just by review. Refusing unknown migrations stops an older binary from running against a newer schema after a rollback.
  Implementation note (M3.1): `vaults` also has `owner_id` (the owning account, `ON DELETE CASCADE`), so the rule "deleting an account cascades to everything it owns" covers vaults too; without it a deleted owner would leave member-less vaults behind. The server uses `sqlx` 0.9, which holds `pwm-store`'s `rusqlite` at 0.39 (Cargo allows only one crate linking `sqlite3` per workspace).
- **D26 — OPAQUE login, session tokens and device registration** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "OPAQUE register/login; session tokens (short-lived + refresh), device registration". `SECURITY.md` fixes the OPAQUE input (`auth_secret`) and the pre-login rule (D12), but not the OPAQUE settings, the token design, where the server's long-term keys live, or the device-name format. Proposal:
  1. **OPAQUE settings (RFC 9807):**
     - **Crate and suite:** `opaque-ke` 4.0.1, the latest stable release (4.1 is a pre-release). Suite: OPRF ristretto255-SHA512, key exchange 3DH on ristretto255 with SHA-512.
     - **No extra stretching (KSF = Identity):** the input is `auth_secret`, which already went through Argon2id and is mixed with the 128-bit Secret Key. Stretching it again would only slow down logins.
     - **Bindings:** context `"pwm/opaque/v1"`; client identifier = `account_id` (16 bytes); server identifier = the server's OPAQUE public key (the protocol's default); credential identifier = `account_id`.
     - **Placement:** both client and server sides live in `pwm-crypto` (module `opaque`), so the server calls the core and implements no crypto itself.
  2. **Server secrets:**
     - One versioned blob, `v:u8 (= 1) || OPAQUE ServerSetup || prelogin_key:32`, passed as base64url in `PWM_SERVER_SECRETS`. `pwm-server keygen` prints a new one. The server refuses to start without it.
     - `prelogin_key` is the HMAC key for D12's fake answers.
     - Losing or changing the ServerSetup locks every account out, so it must live in a secret manager with a backup. There is no rotation in v1.
  3. **Endpoints (JSON, bytes as base64url without padding, ids as UUID strings):**
     - `POST /v1/auth/prelogin {email}` → `{account_id, salt, kdf}`. For an unknown email the server returns a fake `account_id` and salt (`HMAC(prelogin_key, label || email)`, the id shaped as a UUIDv4) and the default params, so the answer looks like a real one (D12). The client needs `account_id` because it salts the key derivation.
     - `POST /v1/auth/register/start {account_id, registration_request}` → `{registration_response}`. This stores nothing on the server.
     - `POST /v1/auth/register/finish {email, account_id, registration_upload, salt, kdf, sealed_keyset, x25519_pub, ed25519_pub, device}` → 201 with tokens. The account, its first device and a session are created in one transaction. KDF params must pass `KdfParams::validate`. A taken email or id gets 409.
     - `POST /v1/auth/login/start {account_id, credential_request}` → `{login_id, credential_response}`. For an unknown id, `opaque-ke`'s fake record gives an answer that looks the same. The server's login state is kept for 60 s in a `login_attempts` table, so any instance can finish the login.
     - `POST /v1/auth/login/finish {login_id, credential_finalization, device}` → tokens, or 401 `invalid_credentials`. A wrong password, a wrong Secret Key, an unknown account and an expired attempt all get the same answer.
     - `POST /v1/auth/refresh {refresh_token}` → new tokens. `POST /v1/auth/logout` ends the session.
     - `GET /v1/account` → `{account_id, email, kdf, salt, sealed_keyset, x25519_pub, ed25519_pub, plan}`. `plan` is `free` when there is no subscription row.
     - `GET /v1/devices` and `DELETE /v1/devices/{id}`. Revoking a device sets `revoked_at` and ends its session.
     - Errors are `{"error": "<code>"}` and never include input values.
  4. **Tokens:**
     - **Format:** random 32-byte tokens, base64url with a prefix (`pwma_` for access, `pwmr_` for refresh) so secret scanners can spot leaked ones.
     - **Storage:** the server stores only their SHA-256 hashes. That is enough because the tokens are random, not passwords. Generating and hashing tokens lives in `pwm-crypto`.
     - **Access token:** lasts 15 minutes and is sent as `Authorization: Bearer`.
     - **Refresh token:** expires after 90 days unused and is replaced on every use. Presenting an already-replaced refresh token (a sign it was copied) ends that session.
     - **One session per device:** a new login on a device replaces its old session.
     - Why not JWTs: every request checks the session in the database anyway (revocation must be instant), so signed tokens would add a signing key and save nothing.
  5. **Devices:**
     - Each device is registered when it registers or logs in, with a client-generated `id`, its `platform` and a sealed name.
     - A revoked device id cannot log in again; the client makes a new id. A device id that belongs to another account gets 409.
     - Approving a new device from an existing one (which hands over the Secret Key) is a later milestone; until then a new device needs the master password plus the Secret Key.
  6. **Device name format v1 (normative, goes into `SECURITY.md`):**
     - The UTF-8 name, at most 63 bytes, is padded to exactly 64 bytes (ISO/IEC 7816-4), so every sealed name has the same size.
     - It is sealed under `sym_key` with the new AAD label `pwm/device-name` and fields `version:u8 || account_id || device_id`.
     - Vectors go in `core/test-vectors/device_name.json`, checked in Rust and Node.
  7. **Schema migration 0002:**
     - New table `sessions`: `id`, `account_id`, `device_id` (unique), `access_hash`, `access_expires_at`, `refresh_hash`, `prev_refresh_hash`, `refresh_expires_at`, `created_at`.
     - New table `login_attempts`: `id`, `account_id` (no foreign key, because it may be a fake id), `state`, `expires_at`.
     - `accounts.opaque_record` becomes NOT NULL.
     - A migration test checks that v1 data survives the upgrade.
  8. **Email:** trimmed and lowercased, at most 254 bytes, exactly one `@` with text on both sides, and no spaces or control characters. **Known gap:** registration answers 409 for a taken email, so it reveals which emails have accounts. The fix is email verification ("check your inbox" for every signup), which needs an email provider; it is deferred until notices and billing emails land. Rate limits (M3.5) slow down enumeration meanwhile. Alternative: build email verification now, before any email provider is chosen.
  9. **Tests:**
     - **OPAQUE vectors:** a frozen server setup, registration record and login transcript, generated with seeded randomness. They catch a changed setting or an `opaque-ke` upgrade that breaks existing accounts. These are checked in Rust only; the RFC 9807 vectors in `opaque-ke` cover the protocol itself.
     - **Server tests:** full register → login → refresh → logout runs against Postgres. Also: a wrong password and an unknown account answer identically; fake prelogin answers are the same each time; refresh-token reuse ends the session; expired access tokens and login attempts are refused; a revoked device is locked out.
     - **Logging:** a check that tokens and OPAQUE messages never appear in errors or logs.
  Implementation note (M3.2): fuzzing showed that `opaque-ke`'s server login state holds the expected client MAC in plain form, so anyone who could read a stored login attempt during its 60 s could finish that login. The state is now sealed before it is stored, under a key derived from the server secrets (AAD `pwm/login-state`). The secrets blob's 32-byte field became `server_key`, from which both `prelogin_key` and `login_state_key` are derived with HKDF (`SECURITY.md`). Not yet merged anywhere, so no stored data changes.
  Why: OPAQUE keeps the password, and even a password-equivalent, off the server. Opaque random tokens with hashes in the database give instant revocation without a token-signing key to protect. One versioned secrets blob makes the single unrecoverable server secret explicit, so it gets backed up.
- **D27 — Vault and item endpoints, conflicts and delta sync** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "item endpoints with revision checks (`If-Match`), delta sync". D25 fixes the two numbers (per-item `revision` chosen by the client, per-vault `seq` assigned by the server) and the tables. Open: how vaults are created and who may write, the exact HTTP rules for writes, conflicts and deletes, and the shape and paging of delta sync. Proposal:
  1. **Vaults:**
     - `POST /v1/vaults {id, wrapped_key}` creates a vault owned by the caller. `wrapped_key` is the vault key sealed under their `sym_key` (AAD `pwm/vault-key`). The client creates its first vault right after signing up.
     - `GET /v1/vaults` lists the vaults the caller belongs to: `{id, role, wrap, wrapped_key, shared_by, seq}`.
     - Sharing (adding members) comes with the sharing milestone. The schema already holds it.
  2. **Access:** a vault the caller is not a member of answers 404, the same as one that doesn't exist, so outsiders can't tell them apart. `owner` and `editor` may write; `viewer` may only read (403 `read_only`).
  3. **Writing an item:** `PUT /v1/vaults/{id}/items/{item_id}` with body `{revision: "<n>", blob}`. `blob` is the item envelope (D18) in base64url; revisions are decimal strings on the wire.
     - **Create:** header `If-None-Match: *`, and `revision` must be `"1"`.
     - **Update:** header `If-Match: "<current revision>"`, and `revision` must be current + 1, because the client sealed the blob with that revision in its AAD.
     - **No precondition header:** 428 `precondition_required`. The server never overwrites without one.
     - **Stale write:** a wrong `If-Match`, or a create when the item already exists, gets 412 `revision_conflict` with the current `{revision, seq, blob}` in the body. The client can then merge (keep both versions in history, M3.4) without another round trip.
     - **Deleted id:** an id that has a tombstone gets 409 `item_deleted`. Ids are never reused.
     - **Success:** 201 (create) or 200 (update) with `{revision, seq}` and `ETag: "<revision>"`.
  4. **Deleting an item:** `DELETE /v1/vaults/{id}/items/{item_id}` with `If-Match: "<current revision>"` (required, as for writes).
     - It removes the item and writes a tombstone with that revision and a new `seq`, in one transaction.
     - A stale `If-Match` gets 412 with the current item; an unknown id gets 404; a deleted id gets 409 `item_deleted`.
  5. **Change counter:** every write or delete bumps `vaults.seq` while holding that vault's row lock, in the same transaction. Within one vault, changes therefore commit in `seq` order, and a reader never sees a later change without the earlier ones.
  6. **Delta sync:** `GET /v1/vaults/{id}/items?since=<seq>&limit=<n>` (`since` defaults to 0; `limit` defaults to 500, at most 1000).
     - Answer: `{changes: [...], next: "<seq>", more: bool}`.
     - **`changes`:** items and tombstones mixed, in `seq` order. Each is `{id, revision, seq, blob}` for an item or `{id, revision, seq, deleted: true}` for a tombstone.
     - **`next`:** the `since` to use next time: the last `seq` returned, or the vault's current `seq` once nothing more is left.
     - It reads one snapshot, so a page never mixes before and after a concurrent write.
  7. **Validation:** ids are UUIDs and blobs must parse as v1 envelopes. Size limits come in M3.5. Errors follow D26 (`{"error": code}`, never input values).
  8. **Tests (real Postgres):**
     - Create, update, delete and sync, as owner and as viewer.
     - Every precondition case: missing, stale, already exists, deleted id.
     - Paging over items and tombstones.
     - Non-members get 404.
     - `seq` has no gaps and no repeats under concurrent writers to one vault, and concurrent writes with the same `If-Match` give exactly one winner.
  Why: per-item optimistic concurrency with standard HTTP preconditions makes silent overwrites impossible. Returning the current item on a conflict lets the client keep both versions at once, as the roadmap requires. A per-vault counter under the vault's row lock gives an order a client can resume from exactly.
- **D28 — Sync client: transport, offline queue, conflicts and retries** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "`pwm-sync` client: offline queue, retry with backoff, conflict → keep both in history". D19 says a losing version goes into history, D20 gives the cache a sync state per item, and D27 fixes the server rules. Open: how the core reaches the network on six platforms, what an unsent edit looks like in the cache, how two versions are merged, what is retried and how often, and what happens when the server is behind. Proposal:
  1. **No I/O in the core (sans-IO):**
     - `pwm-sync` is a state machine. It hands the platform a request to send (method, path, headers, JSON body) and takes back the status, headers and body. It never opens a socket, reads a clock or sleeps.
     - The platform sends with its own HTTP stack (URLSession, OkHttp, `fetch`, the desktop shell) and schedules the next run when the engine asks.
     - Why not a blocking trait like D22's `RangeFetcher`: the browser extension (WASM) cannot block on a request, and a state machine runs the same on every platform. It can also be tested without a network.
  2. **Tokens:**
     - The engine attaches the access token. On a 401 it sends `/v1/auth/refresh` once, then retries the request.
     - New tokens go back to the platform to keep in secure storage. Tokens are held as `Secret<String>` and never logged.
     - If the refresh fails, the engine reports "signed out" and keeps every unsent change.
  3. **Offline queue = the cache:** an unsent change is an item whose cache state is `Modified` or `Deleted` (D20), so it survives restarts. This needs no new cache format. Local saves go through `pwm-sync`, which keeps one rule:
     - **New item:** revision 1, `Modified`. It is uploaded with `If-None-Match: *`.
     - **Edit of a synced item at revision R:** sealed at R+1, `Modified`. It is uploaded with `If-Match: "R"`.
     - **Another edit before upload:** sealed again at the same revision, replacing the unsent one. Its base stays R.
     - **Delete:** `Deleted`, with the revision set to the base the server knows. A new item that was never uploaded is simply removed. A `Deleted` record's envelope is never read.
     - **Trash purge:** at unlock, items due for purge (D19) are marked `Deleted`. This completes the M2.2 deferral.
  4. **One sync run:**
     - **Vault list:** `GET /v1/vaults`. New vaults are added with cursor 0, keys are updated, and vaults no longer listed are removed. Creating a vault needs a connection; it is rare and happens at sign-up.
     - **Pull, then push:** each vault is pulled page by page until `more` is false. Then unsent changes are pushed. Most conflicts are therefore found while pulling, before anything is uploaded.
     - **Cursor:** saved after a page's items are stored, so a crash only repeats that page. Applying a page twice changes nothing.
     - **Locked app:** sync runs only while the account is unlocked, because merging needs the vault key. Pulling while locked comes later.
  5. **Our own write coming back:** after a lost response, the engine can't tell from the revision whether the item it pulls (or the item in a 412) is its own upload. It compares the ciphertext instead. Every envelope has a random nonce, so identical bytes mean it was ours: the item becomes `Synced` and is not a conflict.
  6. **Conflicts (both sides changed the item since revision R):**
     - **Which version stays current:** the one with the later `updated_at` (inside the ciphertext). A tie goes to the server's version.
     - **The other version** goes into the winner's history (`keep_version`, D19), so both are kept.
     - **Upload:** the result is sealed at S+1 and uploaded with `If-Match: "S"`, where S is the server's revision. The losing content is in history even when the server's version stays current.
     - **When the versions can't be merged:** either side may be read-only (a newer schema or unknown type, D18) or fail to decrypt. Then the server's version stays as it is, and the local version is saved as a new item (new id, revision 1). Nothing is dropped.
  7. **Edits versus deletes:**
     - **Local edit, deleted on the server** (tombstone, or 409 `item_deleted`): the edit is kept as a new item (new id), because ids are never reused (D27).
     - **Local delete, edited on the server** (412 on the `DELETE`): the newer edit wins. The local delete is dropped and the server's version comes back. A delete never removes changes the user hasn't seen.
     - **Deleted on both sides:** the item is removed locally.
  8. **Retries:**
     - **Network errors, 5xx and 429:** retried with exponential backoff and full jitter: 1 s doubling to at most 5 min, never sooner than `Retry-After`. The platform's timer runs the next attempt.
     - **Conflict loops:** a 412 merges and retries at once, at most 3 times per item per run.
     - **Other 4xx for one item** (for example the size limits coming in M3.5): that item stays queued and is reported as "couldn't sync", and the run carries on. It is never deleted.
     - **403 `device_revoked`:** the engine clears the cache (D20) and reports it.
     - **When sync runs:** at unlock, about 2 s after a local change, every 5 min while the app is open, and on demand. Push notifications come later.
  9. **Server behind the device:** if a vault's `next` is lower than the cursor sent (D27; for example after a restore from backup), the engine pulls that vault again from 0. Afterwards, `Synced` items the server no longer has and that have no tombstone are uploaded again as new items (revision 1). Edits made after the backup therefore survive. Open risk: if a restored server has already handed out new seqs beyond a device's cursor, the device can't detect the restore from seqs alone. The restore runbook (M3.6) must bump a per-vault epoch that clients check; that is logged as a follow-up.
  10. **Not in M3.4:**
      - Adding members, and the read-only role in the cache: the sharing milestone. Until then, every vault a client sees is its own.
      - Pulling while locked.
      - Push notifications.
      - The IndexedDB cache for the extension (M3.7/M4).
  11. **Tests:**
      - **Fake server:** an in-memory implementation of the D27 rules, driving the engine. Every rule above gets its own test: lost responses, crashes between pages, each conflict and delete case, signed out, revoked, backoff timing with a fixed clock and fixed jitter.
      - **Randomized multi-device runs:** several devices edit, delete and go offline in random orders, then all sync. They must end with identical vaults, and every edit's content must still exist, as the current version, in history or as a new item.
      - **Real server:** the engine runs against the real `pwm-server` on Postgres in-process, so the HTTP contract is checked end to end.
  Implementation notes (M3.4):
  - **Revoked devices now get 403 `device_revoked`.** The server used to delete a revoked device's session, so the device only ever saw a 401 and looked signed out, keeping its cache. The session row now stays, and both its access and refresh tokens are answered with 403 `device_revoked` (still refused). The engine clears the cache on that answer, including when it arrives in reply to a token refresh. D26's "ends its session at once" still holds.
  - **The cursor skips our own writes.** When a write's `seq` is exactly the cursor + 1, the cursor moves to it, because nothing can be missing in between (D27 commits in `seq` order). The next pull then doesn't download our own change again.
  - **History merge:** `Item::keep_conflicting` (`pwm-vault`) keeps the losing version and every version in its history that the winner lacks. History stays newest first and at most 10 long (D19), so edits made offline on the losing device survive as well.
  Why: keeping the network out of the core is the only design that works the same on native and WASM, and it makes every failure reproducible in tests. Reusing the cache states keeps the queue durable without a new format. "Later edit wins, the other goes to history" never loses content and needs no user prompt during autofill. Alternatives: a CRDT per field (merges more automatically, but much more complex and it leaks nothing more useful), or asking the user on every conflict (simple, but it interrupts and fails in background sync).
- **D29 — Server limits, rate limits and log scrubbing** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "rate limiting, request size limits, structured logs with no secrets (log scrubber test)". D27 left size limits to M3.5, D25 held back request logging until a scrubber exists, and D28 already treats a refused item as "couldn't sync, stays queued". Open: the numbers, what is limited per IP or per account, what a log line holds, and what the scrubber removes. Proposal:
  1. **Sizes:**
     - **Request bodies:** at most 16 KiB, except item writes, which may be up to 384 KiB.
     - **Item blob:** at most 256 KiB once decoded. That is room for a large item with its 10 history versions (D19).
     - **Errors:** anything larger gets 413 `too_large` before the body is read.
     - **The core checks first:** if a saved or merged item would seal above the limit, its oldest history versions are dropped until it fits. If it still doesn't fit, the save is refused with "item too large". A device therefore never queues an item the server will refuse.
  2. **Counts, until plans arrive (M9):** at most 100 vaults per account and 50,000 live items per vault (tombstones not counted). Above that, the server answers 403 `limit_reached`, which the engine treats as "couldn't sync".
  3. **Rate limits:** token buckets in server memory, per IP for unauthenticated endpoints and per account for the rest. Over the limit gets 429 with `Retry-After`, which the engine already respects.
     - `register/*`: 5 per hour per IP.
     - `prelogin` and `login/*`: 30 per minute per IP.
     - `refresh`: 10 per minute per IP.
     - Everything else: 600 per minute per account.
     - **No per-account login lockout.** The OPAQUE input includes the 128-bit Secret Key (D13), so guessing passwords online is hopeless anyway. A lockout would only let a stranger lock someone else out.
     - **Client IP:** the connection's address, or the last `X-Forwarded-For` entry only when the connection comes from an address in `PWM_TRUSTED_PROXIES` (empty by default).
     - **Per-instance limits:** the counters live in each server process. That is fine for one instance; revisit before running several (M10).
     - **No new dependency:** a small token bucket with an injectable clock instead of a crate, so it can be tested exactly.
  4. **Logs:**
     - **Format:** JSON lines (`PWM_LOG_FORMAT=json`, the default outside a terminal), using `tracing-subscriber`'s `json` feature (MIT).
     - **One line per request:** a random request id (also returned as `x-request-id`), the method, the route template (for example `/v1/vaults/{vault_id}/items/{item_id}`, never the real path or query), status and duration.
     - **Never logged:** IP addresses, account or device ids, headers or bodies. Ids would let someone with the logs link activity to an account; the request id is enough to match a support report.
     - **Panics:** a panic hook logs only where the panic happened, never its message, since that can quote values.
  5. **Scrubber:** as a second line of defence, every log line passes through a writer that replaces three kinds of text with `[REDACTED]`:
     - tokens (`pwma_`/`pwmr_` + 43 characters)
     - email addresses
     - runs of 40 or more base64url characters, which covers blobs, keys and OPAQUE messages.
     UUIDs are kept, so the session id in the refresh-reuse warning (D26) still helps operators.
  6. **Tests:**
     - limits at their edges (exactly at the limit and one byte or one item over)
     - token buckets with a fake clock, and `Retry-After` values
     - trusted and untrusted proxies
     - an item that is too large being trimmed and then refused in the core
     - scrubber unit tests
     - the log test extended to a full register → login → vault → item → sync run at TRACE level, checking that no token, email, blob or key appears in the output and that every request line has the expected fields.
  Implementation notes (M3.5):
  - **Exact limits:** buckets use GCRA (one "theoretical arrival time" per key) in whole nanoseconds, so limits and `Retry-After` values are exact rather than off by floating-point rounding.
  - **Registration counts requests, not sign-ups:** both `register/start` and `register/finish` count, so the limit is 10 requests an hour, which is 5 sign-ups.
  - **IPv6 addresses count per /64,** the block one subscriber usually holds whole.
  - **Requests without a peer address** share one bucket. Only in-process tests send those; their shared setup uses `Limits::unlimited()`, and the limit tests pass a peer address.
  - **Scrubber detail:** `=` counts only as trailing padding on a run, so `key=` labels stay readable.
  - **New dependency:** `tracing-serde` (MIT), pulled in by the `json` feature. axum gains its `matched-path` feature for route templates.
  - **The 256 KiB item limit** is `pwm_vault::MAX_SEALED_ITEM_BYTES` in the core and `MAX_ITEM_BLOB_BYTES` on the server. A test keeps the two equal.
  Why: limits protect the service without changing what a well-behaved client sees, because D28 already handles 413, 403 and 429. Keeping ids and IPs out of logs means a log leak reveals nothing about users. The scrubber catches what a future careless log line would leak.
- **D30 — Server image, local compose, Postgres TLS and restore detection** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "`docker compose` for local dev; same image is the future self-host image". D25 moved Postgres TLS and the reverse-proxy notes here, and D28 left one open risk for M3.6: devices can't detect a restored server once it has handed out new seqs. Open: what goes into the image, how TLS and secrets are set up, and how a restore is signalled. Proposal:
  1. **Image:**
     - **Build:** a multi-stage `Dockerfile` that builds a static `x86_64-unknown-linux-musl` binary. arm64 comes with the release pipeline (M10.1).
     - **Final stage:** `FROM scratch`, holding only the binary, the Mozilla CA bundle (MPL-2.0, used unmodified) and a passwd entry for a non-root user (uid 10001).
     - **Nothing else:** no shell and no package manager. That means no GPL software is shipped in the image (S6) and almost nothing to attack.
     - **Health check:** the image can't contain curl, so a `pwm-server healthcheck` subcommand calls `/healthz` on itself.
  2. **Postgres TLS:**
     - `sqlx` with rustls and the system CA bundle (`rustls-native-certs` reading the image's bundle), as D25 planned. `webpki-roots` stays out (CDLA licence).
     - Remote databases should use `sslmode=verify-full`.
     - The compose setup talks to its own Postgres over the private compose network without TLS.
     - `cargo deny` must pass the new crates (the crypto provider is `ring`, Apache-2.0 AND ISC).
  3. **`compose.yaml`:**
     - **Postgres:** `postgres:17-alpine` with a named volume and a health check.
     - **Server:** built from the `Dockerfile`, starts once Postgres is healthy, published on `127.0.0.1:8080` only, read-only root filesystem, all capabilities dropped, `no-new-privileges`.
     - **Secrets:** read from a gitignored `.env`. A new `just secrets` recipe writes it once using `pwm-server keygen`.
     - **Commands:** `just server` becomes `docker compose up --build`. `just db` stays as the throwaway database for tests.
  4. **Reverse proxy:** the server speaks plain HTTP. A short `docs/DEPLOYMENT.md` says what the TLS proxy in front must do (HTTPS only, HSTS, pass `X-Forwarded-For`, list itself in `PWM_TRUSTED_PROXIES`) and includes a Caddy example. Full self-hosting docs remain M11.
  5. **Restore detection (closes the D28 open risk):**
     - **Server:** migration 0003 adds a one-row `server_meta` table holding a random `epoch`. `pwm-server mark-restored` replaces it, and the backup/restore notes in `DEPLOYMENT.md` make that command a required step. `GET /v1/vaults` returns the epoch in a `pwm-epoch` header, so D27's JSON doesn't change.
     - **Client:** the cache stores the epoch it last saw (cache schema v2: a nullable `server_epoch` on the account row, with a migration, a frozen v1→v2 fixture and a migration test). When the epoch changes, the engine pulls every vault again from 0 and uploads what the server lost (the D28 §9 path).
  6. **CI:** a new job builds the image, starts compose, waits until it is healthy, checks `/healthz`, and runs the registration flow once against it. The image is not pushed anywhere yet; the registry and signing come with M10.1.
  Implementation notes (M3.6):
  - **Where the cache keeps the epoch:** a one-row `sync_meta` table (cache schema v2), not a column on the account row. The engine also runs with no account row, for example on a device that has only synced so far.
  - **When the epoch is recorded:** only once every vault has been pulled. A run cut short by a crash therefore resyncs again next time.
  - **The first epoch** a device sees is just recorded, without a resync.
  - **Health check:** `pwm-server healthcheck` speaks HTTP/1.1 over a socket to loopback, so the scratch image needs no curl.
  - **`keygen --env <file>`** creates the file (mode 0600 on Unix) and never overwrites an existing one.
  - **TLS test:** it uses a second Postgres started with `-c ssl=on` and the image's self-signed "snakeoil" certificate. `sslmode=require` must connect encrypted, and `verify-full` must refuse the certificate.
  - **Smoke test:** a plain-socket example client (`server/examples/smoke.rs`, no HTTP client crate) runs against the compose stack in CI before and after `mark-restored`.
  - **New crates for TLS:** rustls, ring, rustls-native-certs and their dependencies (`sqlx` feature `tls-rustls-ring-native-roots`). All pass `cargo deny`.
  Why: a scratch image is the smallest and safest thing to ship, and it is the self-host image too. Reading CA roots from a file keeps TLS within the licence allow-list. A server epoch closes the one restore case that change numbers alone can't catch.
- **D31 — Core bindings for apps (UniFFI) and the extension (WASM)** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "FFI (`pwm-ffi` via UniFFI) and WASM (`pwm-wasm`) builds of core exposing account, vault, sync, generator, autofill APIs". Open: the shape of that API, how sign-up and login (which need server round trips) are exposed, how items cross the boundary, where the extension keeps its cache (the `Store` trait is synchronous but IndexedDB is not), and how bindings are tested. Proposal:
  1. **One API, two thin wrappers:**
     - A new crate, `core/pwm-client`, is the only API apps and the extension see. `pwm-ffi` (UniFFI proc-macros; MPL-2.0, used unmodified) and `pwm-wasm` (`wasm-bindgen`, MIT/Apache-2.0) only translate types and errors. All logic, and its tests, stay in plain Rust.
     - **Autofill** is not exposed yet, because `pwm-autofill` is still empty. M4.1 adds it to this API.
  2. **Every server conversation works like sync:** sign-up, login, unlock (fetching the keyset) and sync all use the D28 pattern. The platform gets `Step::Send(request)`, sends it, and hands back the answer. Apps write one HTTP loop and never touch OPAQUE messages or tokens.
     - **Sign-up:** returns the Secret Key for the Emergency Kit once and starts the session.
     - **Login:** takes the email, master password and Secret Key, and runs pre-login, both OPAQUE steps and the keyset fetch.
     - **Lock:** drops every key from memory. Unlocking again on the same device needs the master password (biometric unlock comes in M5/M6).
  3. **Items cross as D18 JSON** with their id (`Item::to_json_with_id`), so neither binding mirrors the item model. The format is already versioned and keeps unknown fields. Generator and strength take and return plain options and values.
  4. **Secrets at the boundary:** the master password, Secret Key and revealed item fields become platform strings that the core can't wipe. The core wipes its own copies. `SECURITY.md` will say this plainly, and the apps keep such strings short-lived (M5/M6).
  5. **Caches:**
     - **Native:** `SqliteStore` at a path the app chooses.
     - **WASM:** a new `MemoryStore` in `pwm-store` (no feature flag), plus `snapshot()` / `load(snapshot)`. The snapshot is a versioned byte format (`v:1`, ciphertext and ids only, the same contents D20 allows), with a fixture and a round-trip test.
     - **Extension:** writes the snapshot to IndexedDB after each change and loads it at startup. The IndexedDB code itself comes with the extension shell (M4.2).
  6. **WASM build:**
     - `wasm32-unknown-unknown` with `getrandom`'s `wasm_js` backend, plus the `js` feature of `getrandom` 0.2 for OPAQUE (the M3.2 deferral).
     - `wasm-bindgen-cli` generates the package and TypeScript types into `extension/core/` (generated, not committed).
     - New recipes: `just wasm` builds it and `just bindings` generates Swift and Kotlin.
  7. **Errors:** one error enum with stable string codes (for example `wrong_password`, `item_too_large`, `signed_out`). Swift and Kotlin get typed errors; JS gets an `Error` carrying `code`. Messages never contain values.
  8. **Tests:**
     - **`pwm-client`:** flows against the in-memory fake server, and end to end against the real server (sign-up → login on a second device → item → sync).
     - **Bindings, in CI:**
       - **WASM:** built and exercised in Node (Vitest): create an account offline, generate passwords, save an item, snapshot and load.
       - **Kotlin:** bindings generated, compiled and run on Linux with JNA.
       - **Swift:** bindings generated and run in a tiny Swift package on a macOS runner. Each run costs about 10 minutes of macOS time.
     - All three bindings jobs block merging.
  Implementation notes (M3.7):
  - **Bindings tests run on Linux, not macOS:** Swift (`swiftc`) and Kotlin (JVM + JNA) both run on an ubuntu runner. That uses no macOS minutes and tests the same generated code. iOS and macOS builds come with M5.
  - **The WASM test is a Node script,** `core/pwm-wasm/smoke/smoke.mjs`, run by `just wasm` and CI. Vitest takes over when the extension shell uses the core (M4.2).
  - **Sign-up stops after registering.** Its outcome carries the Secret Key, and the app then calls `start_create_vault` for the first vault (D27). One step at a time, so a failure is clear about what exists.
  - **A new device's name:** it has no keyset before login finishes, so it can't seal its name yet. The name is sent at its next login; a rename endpoint can come with the apps.
  - **The platform keeps the device id** (`new_id()` once per install) and passes it to sign-up and login. A revoked id is refused (D26).
  - **The `uniffi-bindgen` tool** lives in its own crate, `core/pwm-ffi-bindgen`, so its CLI dependencies never reach the library the apps link.
  - **The `MemoryStore` snapshot** has a frozen hex fixture (`tests/fixtures/snapshot_v1.hex`) and rejects truncated, padded or out-of-range input.
  Why: one Rust API keeps the six platforms identical and testable in one place. Steps keep networking in each platform's native stack. JSON items avoid duplicating a large, versioned model in two binding languages. A snapshot store gets the extension a durable cache now without an async `Store` trait.
- **D33 — Autofill engine: matching, field classification, signed rules, multi-step** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "`pwm-autofill`: PSL origin matching, field classifier, multi-step state, rules file format + signature check", and `AUTOFILL.md` gives the behaviour. Items already carry one match mode per URL (D18: `domain`, `host`, `starts_with`, `regex`, `exact`, `never`). Open: the Public Suffix List source, the exact meaning of each mode, how fields are described and scored, the rules file format and its signing key, and what multi-step state holds. Proposal:
  1. **One engine for every platform:**
     - `pwm-autofill` is plain Rust with no I/O.
     - Each platform turns its page or screen into a **field description**: the DOM in the extension, `AssistStructure` on Android, the page and app info on iOS.
     - The core decides what matches and what each field is.
     - `pwm-client` exposes it, so the bindings get it too (the M3.7 deferral).
  2. **Public Suffix List:**
     - **The list:** a copy of `public_suffix_list.dat` is vendored in the crate (MPL-2.0, data, unmodified, credited in the notices) and compiled into the binary, with its date and SHA-256 recorded next to it. It is refreshed by a script before each release and reviewed as a normal PR; the apps never download it.
     - **Private section included:** separate customers under shared hosts (`github.io`, `blogspot.com` and so on) count as separate sites, so one user's page can't receive another's logins.
     - **Our own matcher:** about 150 lines handling wildcards and exceptions, tested against the list's official test cases and fuzzed. That avoids a crate that bundles its own, possibly stale, copy of the list.
  3. **URL parsing:** the `url` crate (WHATWG, MIT/Apache) normalizes the page URL and the item URLs. That includes lower-case hosts, IDN to punycode, and removing default ports.
  4. **Match modes (only `http` and `https` pages):**
     - **`domain` (the default):** the same registrable domain. IP addresses and single-label hosts such as `localhost` match only themselves.
     - **`host`:** the same host, and the same port if the item gives one.
     - **`starts_with`:** the normalized page URL starts with the normalized item URL.
     - **`exact`:** the same URL, ignoring the `#fragment`.
     - **`regex`:** the whole URL against the item's pattern, using the `regex` crate (linear time, no backtracking), with a size limit and a 1,000-character cap on the pattern. A pattern that doesn't compile never matches.
     - **`never`:** never matches.
     - **Ranking:** results come best first: `exact`, then `starts_with`, `host`, `domain`, `regex`; ties go to the most recently used item.
  5. **Fill decisions (`AUTOFILL.md` security rules):**
     - The core answers `fill`, `warn` or `refuse` for a page, its frame and an item.
     - **`warn`:** an `http://` page, or a page whose only match is a `regex`.
     - **`refuse`:**
       - a frame from another site, unless the frame's own URL matches the item
       - a `never` item
       - a site the signed rules mark "never fill".
     - **Always after an explicit user action:** cards, identities and passkeys always need a confirmation step, so the core reports them as `confirm`.
  6. **Field classification:**
     - **Input:** per field, the tag, `type`, `name`, `id`, `autocomplete`, label text, placeholder, ARIA label, `inputmode`, `maxlength`, whether it is visible, its form and its position. Never values.
     - **Output:** per field, a class (`username`, `email`, `current-password`, `new-password`, `totp`, `cc-*`, `address-*`, `ignore`) and a confidence from 0 to 1.
     - **Method: rules and weighted scores, not a learned model.**
       - A valid `autocomplete` token decides on its own.
       - Otherwise, points from `type`, keyword matches in name/id/label/placeholder (English, Greek, German, French, Spanish, Italian and Portuguese to start), the form's shape (one password field means login, two or three means sign-up or change), and position (the username field comes before the password).
       - Hidden fields, and fields that look like honeypots, are `ignore`.
       - **Why not a model:** rules are explainable, testable field by field, and fixable through the rules file without shipping a model. A learned model can be measured against them later (Phase 2), using the top-1,000 harness (M4.7).
     - **One-time codes:** a field counts as `totp` with high confidence (0.8 or more) only with `autocomplete="one-time-code"`, a rule match, or a numeric 6–8 digit field labelled as a code. Anything less is copied to the clipboard instead (`AUTOFILL.md`).
  7. **Signed rules file:**
     - **What it holds:** `autofill-rules.json` (UTF-8) with `"v": 1`, a `version` that only ever goes up, `issued_at`, `expires_at` (at most 90 days later) and a list of sites. Each site has a registrable domain, optional path prefixes and field overrides. An override matches on `id`, `name` or `autocomplete` (exact, or contains), never CSS, so every platform can apply it, and sets the class. Sites can also be marked "never fill", or given multi-step hints.
     - **Signature:** shipped as `v:u8 (= 1) || key_id:u8 || signature:64 || json`. The Ed25519 signature uses the new label `pwm/autofill-rules` (D17 style) over `key_id || json`.
     - **Keys:** two public keys are compiled in, the current one and the next, so the key can be rotated without breaking older apps. The signing key stays offline (a hardware token used by the release owner) and never reaches CI.
     - **Rollback and freeze protection:** a client keeps the highest `version` it has accepted and refuses anything lower. Once `expires_at` has passed, it falls back to the copy bundled with the app plus the built-in rules, rather than keep obeying an old file.
     - **Data only:** the parser has strict limits (1 MiB, 50,000 sites) and a fuzz target.
     - **Not yet:** downloading and updating the file is part of M4.2 and the apps. M4.1 ships the format, the check and a bundled copy.
  8. **Multi-step logins:**
     - **What is remembered:** per tab (the platform gives a tab key), the site's registrable domain, the chosen item id, the step reached (username sent, password sent) and when. **Never values.**
     - **When it is forgotten:** after 5 minutes, when the tab's registrable domain changes, or when the tab closes.
     - **What it does:** the next page of the same login is offered the same item first, and asked for the next missing field.
  9. **Tests:**
     - the official PSL test cases
     - a table of match cases per mode, including IDN, ports, IP addresses, `localhost`, `http` warnings, frames from other sites, and a catastrophic regex that must still finish in linear time
     - classifier fixtures (descriptions of real login, sign-up, password-change, multi-step and code forms), with an accuracy floor per class
     - rules file signature, rollback, expiry, tampering and size-limit tests, with a test vector
     - multi-step expiry and domain changes
     - fuzz targets for the rules parser and URL matching.
  Implementation notes (M4.1):
  - **No production release keys yet.** `RULES_KEYS` is empty until the release owner creates the current and next keys offline (with M4.2, when downloads begin), so for now every downloaded rules file is refused and the built-in heuristics alone apply. `pwm-crypto` gains `sign_with_seed` / `public_key_from_seed` for the offline signing tool and the tests.
  - **Item URL match modes from a newer app version** (`UrlMatch::Other`) never match.
  - **Keyword matching folds Greek final sigma** (ς to σ) and strips accents, so keywords are written naturally.
  - **On a keyword tie, the class listed first wins.** `type=email` always means Email.
  - **"Last used" for ranking** is the item's `updated_at` until the apps record real use.
  - **Fuzzing:** `autofill_rules` signs each input with a test key, so the parser behind the signature is fuzzed too; `url_match` covers every mode. Both are in CI.
  - **`just psl`** (`scripts/update-psl.mjs`) refreshes the list and its recorded version and hash.
  Why: keeping matching and classification in the core gives every platform the same behaviour and one place to fix it. Including the PSL private section and refusing other sites' frames closes the cross-site leaks that clickjacking and shared-hosting attacks rely on. A signed, versioned, expiring data file lets us fix sites quickly without remote code, and without letting an attacker replay old rules.
- **D34 — Browser extension shell: structure, session, storage, rules download** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "MV3 extension shell: popup (search, copy, generator), unlock (master password, or via desktop app native messaging + biometrics)". M3 and M4.1 left these here too: IndexedDB persistence of the `MemoryStore` snapshot, creating the first vault only if there is none, and downloading the rules file with release keys. Open: how the extension is built, how it stays unlocked when Chrome stops its service worker, where the Secret Key and tokens live, auto-lock, the server address, clipboard clearing, and the rules keys and download. Proposal:
  1. **Structure:**
     - **Background:** the MV3 service worker owns the WASM core (`pwm-wasm`, `--target web`), the cache and every server conversation. The manifest's CSP adds `'wasm-unsafe-eval'`, the only change MV3 allows for WebAssembly. No remote code.
     - **Popup:** asks the background for everything over `chrome.runtime` messages, and never holds keys.
     - **UI:** Preact (MIT, about 4 KB) with TypeScript strict and the design tokens from `design/tokens.json`.
     - **Bundling:** esbuild (MIT) bundles into `extension/dist/`.
     - **Content scripts** come with filling (M4.3); M4.2 has none.
  2. **Staying unlocked** (Chrome stops an idle service worker and wipes its memory):
     - **Session blob:** when unlocked, the core gives the extension a session blob: the account keyset sealed under a fresh random key (a new AAD label `pwm/session`, versioned). The blob and its key go into `chrome.storage.session`.
     - **Why that store:** it is memory-only, cleared when the browser closes, and readable only by the extension's own pages (not by content scripts, which is Chrome's default and is kept).
     - **When the worker restarts:** it restores the session from the blob without asking for the password.
     - **Exposure:** this is the same as keeping the worker alive, and nothing reaches disk.
     - **Alternative:** re-entering the master password after every idle stop, roughly every 30 seconds to 5 minutes, which is too disruptive.
  3. **What is stored on disk** (`chrome.storage.local`, the extension's private storage):
     - **The cache snapshot** (ciphertext and ids only, D31). It is written after each change and loaded at startup. IndexedDB holds the bytes; `chrome.storage.local` holds small settings.
     - **The Secret Key, as is.** It is designed to live on the device: an attacker still needs the master password (D13), and the browser has no secure store. The same as the 1Password extension.
     - **The refresh token, sealed** under a key derived from the account keyset (new label `pwm/device-tokens`). It can be used only after unlock, and a copied disk alone reveals no usable token.
     - **Never:** the master password, the keyset or any unlocked key.
  4. **Auto-lock:**
     - **Timer:** 15 minutes idle by default; 1, 5, 15, 60 minutes or 8 hours can be chosen, but not "never". It uses `chrome.alarms`.
     - **Lock at once** when the computer locks (`chrome.idle` reports "locked").
     - **Browser close:** closing the browser clears session storage, so the next start asks for the master password.
     - **Lock button** in the popup.
  5. **Server:**
     - **Hosted by default.** The address is a placeholder until D1 names the product.
     - **Self-hosting:** an options page accepts an `https://` address and asks for permission to that host only at that moment.
     - **Requests:** made with `fetch` from the service worker; the core's steps (D28) do the rest.
  6. **Clipboard:**
     - Copying a password, code or card number writes it through an offscreen document (MV3 has no clipboard in a service worker) and clears it **30 seconds later** with an alarm.
     - The clear empties the clipboard whatever is in it. Checking first would need the `clipboardRead` permission, which shows a frightening install warning. The popup says it will clear.
  7. **Permissions:** `storage`, `alarms`, `idle`, `offscreen` and `clipboardWrite`, plus the hosted API's origin.
     - **Later:** `activeTab` and `scripting` with filling (M4.3), and `nativeMessaging` with the desktop app (M5.6/M7.2).
     - **No broad host permissions.**
  8. **Popup features for M4.2:**
     - **Signed out:** sign up (email, password with a strength meter at least 3, the Secret Key shown once with copy and a text Emergency Kit download, then the first vault), or sign in (email, password, Secret Key).
     - **Signed in:** search, the item list and detail, copy (username, password, current code), the generator, sync status and the lock button.
     - **No editing yet.** Items are created by save prompts (M4.4) and the apps.
     - **Unlock:** the master password only. Biometric unlock through the desktop app waits for that app (M5.6/M7.2) and is logged as a parity gap.
     - **New core calls:**
       - `session_export` / `session_restore`
       - sealing and opening the refresh token
       - the current code for an item
       - "create the first vault only if there is none" (the M3 leftover).
  9. **Rules file:**
     - **Download:** fetched from the hosted origin (placeholder until D1) at startup and every 24 hours with random spread, using a conditional GET. A failure keeps the current rules, and the accepted version is stored as the next `min_version`.
     - **Release keys:** a new offline tool, `pwm-rules-sign`, has `keygen` (writes a seed file, prints the public key) and `sign`. **The founder** runs `keygen` twice (the current and the next key) on an offline machine and keeps the seeds on hardware tokens or offline backups. Only the two public keys come into `RULES_KEYS` through a PR, and **I never see the seeds**. Until then, downloads are refused as they are today.
  10. **Browsers:** one Chromium build (Chrome, Edge, Brave, Opera) now. Firefox (its MV3 differences) and Safari come in M4.8 and M5.6.
  11. **Tests:**
      - **Rust:** session export and restore (bound to the account, wrong key refused, versioned), token sealing, first-vault-if-none, and the current code.
      - **Vitest** with fake `chrome.*` APIs and fake timers:
        - the message routing and that the popup never receives keys
        - the auto-lock timer, lock on screen lock, and restoring after a worker restart
        - clearing the clipboard after 30 s
        - storage adapters (snapshot and token), and the rules download schedule and rollback refusal
        - the WASM core in Vitest (snapshot persistence round trip, the M3.7 deferral).
      - **A build check** that the bundled extension has a valid manifest with only the listed permissions and no remote URLs in its scripts.
      - Loading it in a real browser comes with the M4.7 Playwright harness.
  Implementation notes (M4.2):
  - **Core:** `pwm-crypto` gains `export_session` / `import_session` and `seal_device_secret` / `open_device_secret` (`SECURITY.md`, with test vectors in `core/test-vectors/session.json`). `pwm-client` gains `export_session` / `restore_session`, `seal_tokens` / `unlock_with_sealed_tokens`, `current_code` and `start_ensure_vault` (sync, then create a vault only if there is none). All are in WASM; `start_ensure_vault` and `current_code` are in UniFFI too.
  - **Tokens:** the access and the refresh token are sealed together, so a restarted worker can sync at once without a refresh round trip.
  - **Rules:** the last accepted rules file is kept on disk with its version and given to the core again at every start (the core holds rules in memory). The download goes to the hosted origin even when self-hosting, because the rules are signed by us, not by the server.
  - **One conversation at a time:** the background queues server conversations (the core runs one at a time) and shares one sync run between concurrent requests.
  - **Options page:** the popup page opened at `#settings`. The server can be changed only while signed out, because the account lives on one server.
  - **Copy:** the Secret Key (after sign-up) and generated passwords also go through the background, so every copy is cleared after 30 seconds.
  - **Build:** `scripts/build.mjs` (esbuild) bundles into `extension/dist/` and fails on any permission beyond the list, a host permission other than the hosted API, content scripts, web-accessible resources, a remote CSP source, or a remote URL in the scripts. CI uploads the build as an artifact.
  - **Parity gap:** unlocking with biometrics through the desktop app waits for that app (M5.6/M7.2); the extension unlocks with the master password only.
  Why: a background-only core with a memory-only session keeps keys off disk while the extension still survives Chrome's worker shutdowns. Sealing tokens under the account keyset and keeping the Secret Key local is the same trade-off native apps make with secure storage, scaled to what a browser offers. Keeping the signing seeds with the founder, offline, is the point of signing the rules at all.
- **D35 — Filling on user action: permissions, inline icon, checks before filling** · settled 2026-11-02 · founder (approved as proposed). The roadmap says "Fill on user action; inline icon in isolated iframe/closed shadow root; visibility/overlay checks before fill (`AUTOFILL.md`)". `AUTOFILL.md` gives the rules but leaves open:
  - how the extension reaches pages with minimal permissions, given that an inline icon must already be on the page before the user acts
  - what exactly counts as "visible and not covered"
  - what each kind of user action fills
  - how values travel to the page
  - how far M4.3 goes (logins, cards, identities)

  Proposal:
  1. **Reaching the page (permissions):**
     - **Default: `activeTab` + `scripting`.** Clicking the toolbar button or pressing the shortcut gives the extension the current tab for that moment only. Then it injects the content script into that tab's frames. There is no install warning about reading every site.
     - **The inline icon is opt-in.** It needs the content script on pages before the user acts, so it needs host access. A setting, "Show the icon in sign-in fields", asks for `https://*/*` (and `http://*/*`) as an optional permission at that moment. Once granted, it registers the content script with `chrome.scripting.registerContentScripts`. Taking the permission back turns the icon off. Sign-up offers this setting once, as a plain choice (no repeated prompts).
     - **Frames:** with `activeTab` alone, only frames from the tab's own origin can be filled. A login form in a frame from another origin of the same site (`login.example.com` inside `www.example.com`) needs the optional permission; the popup says so. The engine's frame rule (D33: a frame from another site is refused unless it matches the item) applies either way.
     - **The URLs the engine judges** come from the browser (`sender.url`, `sender.origin`, the tab's URL), never from what the page or content script reports.
  2. **What each action does:**
     - **Toolbar popup:** the logins for the current tab come first ("For example.com"), each with a Fill button. Fill goes to the best login form on the page.
     - **Shortcut** (`fill-login`, suggested Ctrl+Shift+L / ⌘⇧L, changeable at `chrome://extensions/shortcuts`):
       - If there is exactly one match whose decision is `fill`, the shortcut fills it.
       - Otherwise it opens the popup with the choices.
       - When locked, it opens the popup to unlock. After unlocking, it does not fill on its own: the user picks again.
     - **Inline icon:** a click opens a small menu inside our iframe listing the matching logins (title and username). Choosing one fills that login.
     - **Engine decisions (D33):**
       - `fill` fills.
       - `warn` (an `http://` page, a regex-only match) shows the reason with a "Fill anyway" button.
       - `confirm` always asks first.
       - `refuse` never fills, and says why.
     - **What is filled:** the username or email and the current password. The one-time code is filled only when the engine classifies a field as a code with confidence of 0.8 or more. Otherwise the code is copied (cleared after 30 s), and the popup says so.
     - **Multi-step logins:** after a fill, the step is recorded (D33). On the next page of the same login, the same item comes first.
  3. **The inline icon** (DEF CON 2025 extension clickjacking):
     - **What it is:** a positioned host element with a **closed shadow root** holding an **extension iframe** (`inline.html`). The page cannot read, restyle the inside of, or script that iframe.
     - **No fingerprinting:** `inline.html` is the only web-accessible resource, with `use_dynamic_url` so pages cannot detect the extension.
     - **No embedding by pages:** the content script passes the iframe a one-time token. The background ignores an inline frame without a valid token, so a page cannot embed the menu itself.
     - **Clicks:** taken only when all of these hold:
       - the click is trusted
       - the iframe has been on screen for at least 500 ms
       - an `IntersectionObserver` v2 with `trackVisibility` reported it visible: not covered, transparent, filtered or transformed, in any ancestor frame
     - **Two clicks:** opening the menu and choosing a login are separate clicks, each checked the same way.
     - **No layout shift:** the icon appears when the field is found, not on hover, and never moves the page's layout.
     - **Per-site switch:** "Don't show here".
     - **CLAUDE.md:** this iframe is the single exception to "never inject clickable credential UI into page DOM", as `AUTOFILL.md` already specifies. No other UI goes into pages.
  4. **Checks before every fill**, run in the target frame right before setting values. If any check fails, nothing is filled, and the popup says "The field is hidden or covered" and offers to copy instead:
     - **The field itself:** in the page, enabled, not read-only, and a visible field of the expected kind (never a hidden or honeypot field, D33).
     - **Size and viewport:** it has a size and lies inside the viewport. For a fill started from the popup or the shortcut, the field is scrolled into view once and then checked again.
     - **Style:** `visibility: visible`, opacity 1 on the field and every ancestor, and no `filter` or `clip-path` hiding it.
     - **Not covered:** `elementsFromPoint` at the centre and two inner corners finds the field (or its own label) on top, looking through open shadow roots. (D63: one of the field's ancestors on top counts as clear too.)
     - **Not covered across frames:** an `IntersectionObserver` v2 with `trackVisibility` on the field reports it visible. This also covers this frame being hidden or covered inside its parent page. Where v2 is missing (Firefox, M4.8), only fills in the top frame are allowed. (D63: v2 is required only in frames; in the top frame the other checks decide.)
  5. **Values in transit:**
     - **Where values go:** the background sends values only after a user action, only to the one frame chosen, by `frameId` and `documentId`, so a navigated frame never receives them.
     - **Checked again just before sending:** the frame's browser-reported URL still matches the item.
     - **No lists:** content scripts never receive the vault list or other items. They send field descriptions without values (D33), and they keep no values after setting them.
     - **How fields are filled:** values are set with the native value setter plus `input` and `change` events, so frameworks see them.
     - **Content scripts are untrusted:** the background treats their messages as untrusted (a compromised page process could forge them). They can only ask for classification, report results and ask for a fill that a user action started.
  6. **Scope of M4.3:**
     - **Included:** logins and one-time codes.
     - **Not included:** cards and identities. They need their own rules for payment frames from other sites (Stripe, Adyen) and items without URLs, and `AUTOFILL.md` requires a confirmation step for them. Proposed as their own task after M4.4 (later moved to M14, Phase 2, as PRODUCT.md places cards and identities).
     - **Not included:** save prompts (M4.4) and passkeys (M4.5).
  7. **Manifest and build check (D34):**
     - **Permissions:** adds `activeTab` and `scripting`, and a `commands` entry.
     - **Web-accessible resources:** exactly `inline.html` (with its script and style), `use_dynamic_url: true`.
     - **Optional host permissions:** `https://*/*` and `http://*/*`, used only for the icon.
     - **Still refused:** static `content_scripts`, other web-accessible resources, other permissions, and broad `host_permissions`.
  8. **Tests:**
     - **Vitest:**
       - choosing the login form and fields from the classification
       - the decision handling
       - the shortcut rules
       - messages to frames by `frameId`/`documentId`, and refusal of stale or mismatched frames
       - the inline token
       - the visibility checks, as pure functions with injected geometry and styles
       - that content scripts never receive other items
     - **Playwright (Apache-2.0) with the built extension in Chromium,** in a new CI job. This is the start of the M4.7 harness. Fixture pages:
       - a plain login
       - a multi-step login
       - a hidden honeypot field
       - a field covered by a transparent overlay
       - a field with an opacity-0 ancestor
       - an inline icon made transparent by the page
       - a cross-site iframe
       - an `http://` page
       Each must fill, warn or refuse as specified. The full fixture set and the top-1,000 runner stay in M4.7.

  Alternatives:
  - **Static content scripts on `<all_urls>` from install** (what most competitors do): the inline icon works everywhere at once, but every user gets the "read and change all your data" warning, against `AUTOFILL.md`'s minimal permissions.
  - **No inline icon at all:** simplest and safest, but it is the most used way to fill in other managers.
  - **Checks with `elementsFromPoint` only:** they miss covering in a parent frame, which `IntersectionObserver` v2 catches.

  Implementation notes (M4.3):
  - **Site access is not all.** Site access granted for a self-hosted server stays when the icon is turned off (the popup gives it back only on the hosted server). So the choice is stored too, and the icon shows only when it is on *and* access is granted.
  - **Shortcut and Chrome version:** `minimum_chrome_version` is 127, for `action.openPopup` (the shortcut's fallback) and `documentId` targeting.
  - **No `frame-ancestors` in the extension CSP:** the M4.2 manifest had `frame-ancestors 'none'`, which also blocked the icon frame (the browser tests caught it). Web pages cannot frame extension pages that are not web-accessible anyway, so the directive is gone and `inline.html` is the only page they can frame.
  - **Our own icon is hidden while a fill is checked:** it overlaps its field, and `IntersectionObserver` v2 counted it as covering the field, so with the icon on every fill was refused (the browser tests caught it). The content script takes its icons off the page while it measures and fills, then puts them back. When a fill from the menu is refused, the menu now says why. The other icons are hidden while a menu is open, because one lying over the menu made it count as covered.
  - **What pages may load:** `inline.html` loads `inline.js`, `inline.css` and `tokens.css`, so those four are the web-accessible resources, all with `use_dynamic_url`.
  - **The icon frame's visibility:** the frame watches its own root element with `IntersectionObserver` v2. Where v2 is missing it takes no clicks at all.
  - **Honeypots:** a field counts as visible only if it has a size above 1 px, lies inside the page (not parked at `left: -9999px`), is not `visibility: hidden` and has opacity above 0. The checks before filling are stricter still.
  - **Fixed in passing (M4.2 bug):** the background read login fields at the top level of an item, but the core lists them under `login` (D18). So the popup showed no usernames, and copying a password failed. The test fakes now use the core's real JSON, and the browser tests read items through the real core.
  - **`saveItem`** is on the background for the browser tests now, and for the save prompts (M4.4).
  - **The test build** (`build.mjs --test`, `extension/dist-test/`) has site access from install and a background hook for Playwright. The release build's check fails if the hook is in it.
- **D36 — Save and update prompts: detection, where the offer appears, what is saved** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "Save/update prompts after submit". `AUTOFILL.md` says "detect submit + navigation; offer save only when credentials changed". `PRODUCT.md` promises no nag prompts and no UI moving under the cursor, and CLAUDE.md forbids clickable credential UI in page DOM.
  Open:
  - how the extension sees a submit at all, since D35 keeps it off pages by default
  - what counts as a submit, and when a login "worked"
  - where the offer appears
  - what "changed" means
  - what an offer may hold, and for how long
  - which part lives in the core

  Proposal:
  1. **Seeing submits** (permissions as in D35):
     - **Automatic offers need site access.** The content script must be on the page before the user submits, so offers use the same optional site access as the inline icon. The setting "Show the icon in sign-in fields" becomes **"Fill and save on sign-in pages"**, one switch for both. The icon and the offer can each be turned off on a site.
     - **Without site access:** the popup gets **"Save login from this page"**. Clicking it injects the content script (`activeTab`), reads the sign-in fields the user has typed into, and shows the same offer in the popup.
  2. **What counts as a submit:**
     - **Events:** a form's `submit` event, Enter in a password field, or a trusted click on the form's submit-like button (`type=submit`, or a button in the form). This also covers single-page apps that never submit a form.
     - **What is read at that moment:** only the fields the engine classified (D33) as username or email, current password and new password (first and confirm).
     - **Kept out:** values are never read before a submit. Nothing is read from hidden fields, or when the password is empty or the two new passwords differ.
     - **The capture:** the content script sends the background `{username, password, newPassword}` and forgets them. The background keeps the capture **in memory only** (not session storage, not disk), per tab. It expires after 5 minutes, or when the tab closes or the vault locks.
  3. **When a login "worked"**, so a wrong password is not offered: the offer appears when either of these happens within 10 seconds of the submit:
     - **Navigation:** the tab navigates to another page of the same site.
     - **The form goes away:** the password field is no longer in the page or no longer visible.
     If the same form is still there with the field (a failed login usually stays), there is no offer. The capture stays in the popup until it expires, in case the site behaves differently.
  4. **What "changed" means** (decided by the core, so the apps' save flows in M5/M6 reuse it):
     - **No matching login for the site** (D33 matching) → **Save**.
     - **A matching login with the same username but another password** → **Update password**. On a change-password form, the new password is compared and offered as the update.
     - **Already saved as is** → no offer.
     - **Several logins match with that username** → **Update**, after the user picks which.
     - **Refused or blocked sites** (rules `never_fill`, a frame from another site, D33) → no offer.
  5. **Where the offer appears:**
     - **Card:** a small card fixed at the **top right of the page**, inside the same kind of frame as the inline icon: a closed shadow root around the extension's own `inline.html` with a one-time token. It shows the site and username (never the password), with **Save** (or **Update**), **Not now** and **Never for this site**.
     - **Checked clicks:** clicks count only as in D35: trusted, and after 500 ms visible according to `IntersectionObserver` v2. A page that hides or covers the card cannot make the user save.
     - **No layout shift:** the card is fixed and never moves the page. It appears at most once per submit and closes on navigation. It never appears while the user is filling or typing.
     - **"Never for this site"** is stored per host, like "Don't show here".
     - **Toolbar:** the button gets a badge while an offer is pending, and the popup shows the offer at the top. This is also how users without site access, and users who closed the card, still see it.
     - **Locked vault:** the card says "Unlock to save" and opens the popup; after unlocking, the offer is there.
  6. **What is saved:**
     - **Save:** a login with title = the site's host name, the username, the password, and the page's origin as URL (default `domain` matching), in the first vault. The popup's offer lets the user edit the title and username before saving; the card does not.
     - **Update:** the password changes and the previous version goes to history (D19). Nothing else changes.
     - **`http` pages:** the offer shows "This page is not secure", but saving is allowed.
  7. **Core (`pwm-client`, Rust, tested first), exposed in WASM and UniFFI:**
     - `save_offer(page_url, username, password, new_password)` → `none` | `save` | `update {item_ids}`, using the D33 matching.
     - `save_login(vault_id, page_url, title, username, password)` → the new item id.
     - `update_password(vault_id, item_id, password)`: history kept.
     - **No item JSON built in TypeScript:** the extension never builds item JSON itself. The test-only `saveItem` stays for the harness.
  8. **Not included:**
     - **Offering the generator on new-password fields:** a separate task.
     - **Saving cards and identities:** M14.
     - **Passkeys:** M4.5.
     - **Importing what the browser itself saved:** M8.
  9. **Tests:**
     - **Rust:** the offer decision (new site, same credentials, new password, change-password form, several matches, refused frame, blocked site), save and update (history kept, other fields untouched), and the URL saved from a page URL.
     - **Vitest:**
       - which fields are read, and that hidden ones and mismatched new passwords are not
       - that captures expire and stay out of storage
       - the navigation and form-gone rules
       - the badge, the card token and "never for this site"
       - that the card's messages and the popup never carry the password
     - **Playwright (harness):**
       - a successful login that navigates → card → Save → the login exists and fills next time
       - a failed login (the form stays) → no card
       - the same credentials again → no card
       - a change-password form → Update → history holds the old password
       - a single-page login without a submit → card
       - a page that makes the card transparent → clicks ignored
       - the popup's manual save

  Alternatives:
  - **Offer at once on submit**, like some managers: simpler, but it offers wrong passwords after failed logins.
  - **OS notifications** (`notifications` permission): outside the page, so they cannot be clickjacked, but they are intrusive, and people turn them off.
  - **Only the toolbar badge, no card:** the least intrusive, but easy to miss. Users without site access get exactly this.
  - **Building the item in TypeScript:** less work now, but the apps' save flows would need the same logic again, and item format code belongs in the core.

  Implementation notes (M4.4):
  - **Usernames** are compared trimmed and without ASCII case (`Ada@x.com` is `ada@x.com`), so a different case does not offer a second login.
  - **Multi-step logins:** a username submitted without a password (a multi-step login's first page) is kept, in memory, as a hint for the same site's next page in that tab.
  - **"Worked" by navigation** compares the last two host labels, enough for `accounts.example.com` → `www.example.com`. The core's matching still decides what is offered.
  - **Message order:** the submitting document is recorded, so the next page reporting before the submit message arrives still counts as navigation.
  - **The card** is shown only in the top frame, and hidden while a fill is checked, like the icons. Both stay hidden until the checks end, even when a frame report arrives meanwhile and would show them (the browser tests caught that race).
  - **One switch:** "Fill and save on sign-in pages" is the D35 icon setting renamed. Turned on, frame reports answer with the fields to read on submit.
  - **New core calls:** `save_offer`, `save_login` and `update_password` in `pwm-client`, WASM and UniFFI (`SaveOffer` enum). A new login gets the page's origin with `domain` matching, and a title from the host name without `www.`.
- **D37 — Passkeys in the browser extension: interception, crypto, confirmation, storage** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "Passkey support (WebAuthn interception where browser allows; else defer to platform provider)". `PRODUCT.md` makes passkeys P0 ("autofill incl. passkeys"). `AUTOFILL.md` requires a confirmation step for passkeys. D18 already stores passkeys in logins: credential id, relying party id, user handle, user name, PKCS#8 private key, sign counter.
  Open:
  - how the extension sees a site's passkey requests at all
  - where the key generation and signing happen
  - which crates
  - how the user confirms
  - what is stored and in which item
  - how the browser's own passkeys stay usable

  Proposal:
  1. **Seeing requests** (Chromium has no passkey-provider API for extensions; `webAuthenticationProxy` is for remote desktops):
     - **A page-world script:** a small script registered for the page's own world (`world: "MAIN"`, `runAt: "document_start"`, all frames) wraps `navigator.credentials.create` and `.get` for `publicKey` requests only. It passes the request to our isolated content script with `window.postMessage`, and the content script passes it to the background.
     - **Site access:** this needs site access before the page loads, so passkeys get their own switch, **"Use PWM for passkeys"**. It asks for the same optional site access as D35/D36, and nothing changes until it is on.
     - **Trust:** the page can call or spoof the wrapper, so everything in it is untrusted.
       - **The origin** comes from the browser (`sender.url`, `sender.origin`), never from the page.
       - **The relying party id** must be the origin's host or a registrable suffix of it (WebAuthn rules, with the PSL), checked in the core.
       - **Only HTTPS** (and `localhost`) origins.
     - **Frames from another site:** a request from a frame of another site needs the page to allow it (`publickey-credentials-get/create` permissions policy), which the browser enforces before our wrapper runs; we refuse it anyway unless the frame is the top-level site's.
  2. **The browser's own passkeys stay usable:**
     - **The choice:** every request first opens our confirmation (below), with **"Use another device or the browser"**. That hands the request back to the browser's original `navigator.credentials` call, unchanged.
     - **No passkeys for the site:** if the vault is locked or has no passkey for the site, a `get` goes straight back to the browser without asking.
     - **A `create` excluding one of ours:** it also goes to the browser (the site already has one of ours).
  3. **Crypto in the core** (`pwm-passkey`, a new crate, tests and vectors first):
     - **Keys:** ES256 only (ECDSA P-256 with SHA-256), the algorithm every relying party accepts. `create` with no ES256 in `pubKeyCredParams` goes to the browser.
     - **Crates:** RustCrypto `p256` (features `ecdsa`, `pkcs8`) and `sha2`, both MIT/Apache-2.0. These are added to the vetted list in CLAUDE.md (the same RustCrypto family). CBOR for the attestation object and the COSE key is written by hand for the few fixed structures (a decoder is not needed), and fuzzed.
     - **What the core builds:**
       - `clientDataJSON` from the browser-given origin, the challenge and the type, so it cannot be shaped by the page.
       - `authenticatorData`: the RP id hash, flags UP=1, UV=1, BE=1, BS=1, AT on create, and a sign count of 0 (synced passkeys, as iCloud Keychain does).
       - the attestation object with `fmt: "none"`, and the assertion signature.
     - **AAGUID:** a fixed random one for PWM, published in `SECURITY.md`.
     - **User verification:** UV=1 means the vault is unlocked and the user confirmed in our window. A request with `userVerification: "required"` while locked asks to unlock first.
     - **Discoverable credentials:** always (resident keys); the user handle is stored.
  4. **Confirmation** (never in page DOM):
     - **Where:** a small extension window (`chrome.windows.create`, type `popup`) showing the site, the account, and **Create passkey / Sign in** or **Use another device or the browser**. Clicks there cannot be clickjacked by the page.
     - **Several passkeys for the site:** the user picks one.
     - **`allowCredentials`:** only those listed are offered.
     - **Timeouts:** the request's timeout is honoured (default 5 minutes). Closing the window counts as a cancel (`NotAllowedError`).
     - **Locked vault:** the window unlocks first, with the master password.
  5. **Storage:**
     - **Which item:** a new passkey goes into the login for that site and username if there is one, else into a new login titled with the RP id, with its URL set to the origin.
     - **What is stored:** credential id (16 random bytes), RP id, user handle, user name, PKCS#8 private key and created time, as D18 already defines, so nothing in the format changes. The server sees only ciphertext.
     - **Other devices:** passkeys sync like any item. Other platforms use them when their providers land (M5.5 iOS, M6.3 Android, M5.6/M7 desktop).
     - **Export:** unchanged. The JSON export carries passkeys (D24); CSV does not.
  6. **Conditional UI** (`mediation: "conditional"`, passkeys offered in the username field's autofill list):
     - **For now:** answered as "no credential" (left to the browser).
     - **Later:** offering passkeys from the inline icon's menu is a follow-up, so nothing appears in pages beyond D35.
  7. **Not included:**
     - **Non-ES256 algorithms:** they go to the browser.
     - **Hybrid / cross-device (QR) flows:** left to the browser.
     - **Attestation other than `none`.**
     - **Passkey management in the popup** (list and delete): shown read-only on the item for now.
     - **Firefox and Safari:** M4.8 / M5.6.
  8. **Tests:**
     - **Rust:**
       - **Vectors:** fixed keys and challenges for create and get, with the exact `clientDataJSON`, `authenticatorData` and attestation bytes.
       - **Independent check:** the signature verified in Node with WebCrypto in `verify.mjs`.
       - **Edge cases:** RP id rules (suffix, PSL, IP addresses, `localhost`), no ES256 offered, the excluded and allowed lists.
       - **Fuzzing:** a fuzz target for the request parser.
     - **Vitest:**
       - the wrapper's message protocol and that it hands back to the browser
       - that the origin is the browser's
       - the window flow (confirm, cancel, timeout, locked)
       - that page messages are never trusted
     - **Playwright:**
       - **The fixture:** a relying party page that verifies attestation and assertions in the page with WebCrypto.
       - **Must work:** register, then sign in with our passkey.
       - **Must refuse:** a spoofed RP id.
       - **Back to the browser:** "Use another device or the browser" reaches the browser's own WebAuthn (a stub in the fixture).
       - **Cancel:** closing the window gives `NotAllowedError`.

  Alternatives:
  - **Defer entirely to the platform** (Windows Hello, iCloud Keychain, Google Password Manager, and our own apps as OS providers later): no page-world script at all, but no passkeys in the extension before the desktop apps, against P0.
  - **Prompt inside the page** (as some managers do): faster, but it is page DOM UI, which CLAUDE.md forbids, and it can be clickjacked.
  - **A real sign counter:** it breaks across synced devices; 0 is what synced passkey providers use.

  Implementation notes (M4.5):
  - **Crates:** `p256` 0.14 (features `ecdsa`, `pkcs8`, `alloc`; no `getrandom`): the core draws the 32 key bytes itself and retries the negligible invalid case. ECDSA signing is deterministic (RFC 6979), which makes the vectors in `core/test-vectors/passkey.json` exact; `verify.mjs` checks every byte layout and the signature with Node's own P-256.
  - **Locked vault:** a `get` while locked goes to the browser, as §2 says. A `create` while locked opens the window to unlock first (§4).
  - **An RP id that is not the page's** answers `SecurityError`, as the browser itself would. Other refusals hand the request back to the browser.
  - **Timeouts:** the window and the request end after the page's `timeout`, kept between 30 s and 10 minutes (5 minutes when none is given). Closing the window is a cancel (`NotAllowedError`).
  - **Which login:** a new passkey joins the site's login whose username matches (trimmed, ASCII case ignored), the same rule as D36.
  - **One site access:** "Use PWM for passkeys" uses the same optional site access as "Fill and save". The popup gives the access back only when neither switch is on and the server is the hosted one.
  - **The page-world wrapper** answers with objects whose own properties shadow the native prototypes' getters, so `instanceof PublicKeyCredential` and `response.getPublicKey()` both work.
  - **Fuzzing:** the `passkey_request` fuzz target registers and signs whatever parses, for any origin.
- **D38 — "Autofill didn't work here" reports: what is sent, where, and how it is kept** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "'Autofill didn't work here' report with form fingerprint (no values)". `AUTOFILL.md` says the report captures "URL origin and a form-structure fingerprint (tag, type, attributes, label text of inputs — never values), user confirms before sending", and that reports become tickets with a 7-day fix SLA for the top 1,000 sites. `PRODUCT.md` lists it as P0.
  The tension: CLAUDE.md says the server never sees plaintext (no URLs), and `QUALITY.md` says analytics never include URLs or form contents. A failure report is, by its nature, a site and its form.
  Open:
  - how a report stays apart from accounts
  - what exactly it holds
  - how label text that may contain personal data is cleaned
  - where it goes, and how long it is kept

  Proposal:
  1. **Reports are anonymous and apart from the vault:**
     - **The endpoint:** `POST /v1/reports/autofill`, sent without a token, cookies, account id or device id. The sync API never sees a report, and a report cannot be tied to an account.
     - **Explicit and one-off:** the user explicitly sends each report and sees exactly what goes. This is not analytics, so the `QUALITY.md` rule ("never URLs") applies to analytics, not to these reports. The CLAUDE.md "server never sees plaintext" rule is about vault data, and a report holds none.
     - **On the server:** no IP address stored, request bodies never logged, and a per-IP rate limit (GCRA, D29) of 10 reports per hour.
  2. **Where reports go:** always to the **hosted** service, even for self-hosted accounts, because we are the ones who fix sites. The confirmation names the destination. Self-hosters who never want this simply never send one. There is no automatic sending of any kind.
  3. **What a report holds** (JSON, `"v": 1`; built by the core, `pwm-autofill::report`, so the apps can send the same reports later):
     - **The site:** the page's **origin only** (scheme, host, port), never the path or query, which can hold tokens.
     - **What happened**, picked from a list: "Nothing was filled", "The wrong field was filled", "It didn't offer to save", "The icon was in the way", "Something else". No free text, since people paste passwords into free text.
     - **Per frame:**
       - **The frame:** whether it is the top frame, a same-site frame or another site's frame. Never the frame's URL.
       - **Per input:** the form index; tag, `type`, `autocomplete`, `inputmode`, `maxlength`, visibility; `name` and `id`; the label, placeholder and `aria-label` text, cleaned (§4).
       - **What the engine decided:** the class and confidence for each field, and which fields a fill would use.
     - **The software:** the extension, core and rules versions and the browser's major version.
     - **Whether a saved login matched the site** (yes/no, only when unlocked). Never which one, or anything from it.
     - **Never:** field values, item data, usernames, the account, the device, cookies, the page's text outside labels, or the full URL.
  4. **Cleaning text** (labels, placeholders, names, ids), in the core, before anything leaves the device:
     - **Normal form:** lower case, with whitespace collapsed and cut to 40 characters.
     - **Things that look like personal data are replaced:**
       - email addresses become `<email>`
       - runs of 3 or more digits become `#`
       - anything that looks like a token (20+ characters of letters, digits, `-` or `_` without spaces) becomes `<token>`
     - **Limits:** at most 200 inputs and 20 frames; a report over 32 KiB is cut to fit, or refused.
  5. **Sending:**
     - **Where:** the popup, on a web page, gets **"Autofill didn't work here?"**.
     - **What it shows first:** the user picks what happened and sees a plain summary (the site, the number of forms and fields, their kinds), with "Show everything" for the exact JSON.
     - **Send or Cancel:** nothing goes until **Send**.
     - **Reaching the page:** it works without site access, using `activeTab` to scan the page at that moment, the same as fill.
  6. **On the server:**
     - **Storage:** a new table `autofill_reports` (migration `0004`): id, received time, the site origin and the report JSON. It is validated against the v1 schema and the limits.
     - **Retention:** kept 90 days; older rows are deleted on each insert.
     - **Reading reports:** `pwm-server reports export --since <date>` writes them as JSON lines for triage.
     - **Tickets and the public dashboard** come with M4.7 / M10, which read this table.
  7. **Tests:**
     - **Rust:**
       - the cleaning rules (emails, digit runs, tokens, length, Unicode)
       - that no value or URL path can get in
       - the size and count limits
       - the report schema
     - **Server:**
       - the endpoint without auth
       - the rate limit
       - retention
       - that no IP is stored and no body is logged (the D29 log scrubber)
       - the export
     - **Vitest:**
       - that the popup's preview is exactly what is sent
       - that nothing is sent without Send
       - that the request has no credentials
     - **Playwright:** a fixture form with a personal label ("Hello Ada, ada@x.com") gives a report with `<email>`, without the name of the logged-in user or any value.

  Alternatives:
  - **A third-party crash/feedback service:** less work, but it sends data to another company and needs a DPA.
  - **Free-text descriptions:** more useful, but people type passwords and personal data into them. Maybe later with a strong warning and its own scrubbing.
  - **Reports to the user's own server:** self-hosters keep their data, but we never see the failures we are meant to fix.

  Implementation notes (M4.6):
  - **Cleaning keeps the punctuation** around a replaced address (`(ada@x.com)` becomes `(<email>)`). A token needs at least one digit, so long ordinary words such as German compounds stay.
  - **Versions are kept as given:** the extension and browser version strings come from the extension, not the page, so they are only cut to 40 characters, not cleaned.
  - **Size before count:** a report over 32 KiB loses frames from the end until it fits, before the 200-input limit is reached.
  - **The server's schema check refuses unknown keys** at every level, so nothing can ride along under another name. Text values are at most 160 bytes (40 characters).
  - **No SQL JSON feature:** the report is stored with `CAST($2 AS jsonb)` and read back as text, so `sqlx` needs no new feature.
  - **The plaintext column allow-list** (`server/tests/schema.rs`) gains `autofill_reports.site` and `.report`, with D38 as the reason.
  - **One look, one send:** the background keeps the previewed JSON per tab, and Send posts exactly that string once. A new Send needs a new look.
- **D39 — Autofill harness: the full fixture set, the top-sites runner, its report and the release gate** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "Autofill harness: fixture sites incl. clickjacking/overlay attacks that must not fill; top-1,000 runner + JSON report". M4 is done when the fixture suite passes 100%, top-1,000 classification reaches 90% (95% by launch), and overlay attacks never fill. `AUTOFILL.md` and `QUALITY.md` describe a nightly run against real login pages, "no real credentials; assert field classification and fill targets only", a release gate ("blocked if pass rate drops > 0.5 pts"), and a public dashboard. M4.3–M4.6 started the harness (24 browser tests).
  Open:
  - what "passes" means on a real site
  - where the site list comes from, and its licence
  - how to treat bot walls
  - what the runner may do on someone else's site
  - where results go

  Proposal:
  1. **The fixture suite becomes data-driven:**
     - **`fixtures/fixtures.json`** lists every fixture page with its expected outcome: filled (and which fields with which values), refused with a reason, copied, or no form. One Playwright test runs them all, and a fixture without an entry fails the suite.
     - **New fixtures:**
       - **Login forms:** an open shadow DOM login; a closed shadow root (expected: no form, documented); an SPA that renders its form after load; an SPA that re-renders the form on each keystroke.
       - **Codes and frames:** a split 6-box one-time code (expected: copied, not filled); a frame inside a frame (same site).
       - **Hidden-field attacks:** the field moved off-screen after load; hidden with `clip-path`; covered by an element with `pointer-events: none`.
       - **Covering in a parent page:** a frame covered by a transparent overlay in its parent page (`IntersectionObserver` v2 catches it).
       - **Icon attacks:** an overlay added on top of the inline icon after it appears (no click taken); the icon moved under the cursor right before a click (no click within 500 ms).
     - **Run:** on every extension PR, as now (`autofill-harness` job).
  2. **The top-sites runner:**
     - **The site list:** `sites/sites.json` holds `{site, login_url, username?, password?}`, with optional CSS selectors for the expected fields. Each entry is added by hand after checking its login page, because no public list gives login URLs.
       - **To start:** about 100 popular sites, with selectors for at least 50.
       - **Growing to 1,000:** over M10, ordered by the Tranco top-sites list (CC BY 4.0; attribution in `THIRD_PARTY_NOTICES`). It is used only to choose and order sites, and is not vendored.
     - **What a run does** on each site, one at a time:
       - make a throwaway login in the test vault, with username `pwm-check-user@example.invalid` and password `pwm-check-password`
       - open the login URL
       - ask the extension to fill, through the same path as the shortcut
       - read which inputs got the two values
       - **Never** submit, click anything, solve a challenge or type anything but those two values. A real account is never used.
     - **Result per site:**
       - **pass:** the username and password reached the expected inputs (or, without selectors, a visible text-like input and a visible password input).
       - **fail:** nothing filled, the wrong inputs, or the checks refused a visible field.
       - **blocked:** a CAPTCHA, a bot wall or a 4xx/5xx page. It is recorded and left out of the pass rate; we never try to get past it (CLAUDE.md, system safety rules).
       - **error:** a timeout or a load failure.
     - **Multi-step sites** (username first): pass when the username page fills. The password page needs a real submit, which the runner never does.
     - **Being polite:** one site at a time, 20 s per page, the normal Chromium user agent with `PWM-AutofillCheck/1` added, and no retries beyond one. The run is nightly, not on PRs (real sites are slow and change).
  3. **The report** (`autofill-report.json`, `"v": 1`):
     - **Run details:** date, extension, core and rules versions, browser version.
     - **Per site:** status, duration, the engine's fill targets as field descriptions (D38-cleaned, never values), and the refusal reason if any.
     - **Totals:** the pass rate (pass ÷ (pass + fail)), and the counts per status.
     - **Where it goes:** uploaded as a CI artifact (90 days). `scripts/compare.mjs old.json new.json` prints what changed and exits non-zero when the pass rate dropped more than 0.5 points, which is the release gate `QUALITY.md` asks for. (It became `runner/compare.ts`; since D64 it counts only the sites both runs counted.)
     - **Later:** the release workflow and the public dashboard read it (M10).
  4. **Not in M4.7:**
     - **Tickets from user reports (D38):** they need server access from CI. They go with the release and ops tooling (M10).
     - **The public dashboard:** M10.
     - **Reaching 1,000 curated sites, and the 90% / 95% targets:** M4.7 delivers the runner and the first list; the targets are measured each night from then on and gate the launch (M10). The M4 "done when" is updated to say so.
  5. **Tests:**
     - **The runner, against the local fixture server:** its statuses (pass, fail, blocked by a fake CAPTCHA page, error by a timeout page).
     - **The report schema.**
     - **`compare.mjs`:** a drop of 0.4 vs 0.6 points.
     - **That the runner never submits:** the fixture's form posts to a page that fails the test if reached.

  Alternatives:
  - **Discover login pages automatically** (follow "Sign in" links): no list to keep, but it clicks around other people's sites and misses many. It may come later as a helper for curating entries.
  - **Assert classification only, without filling:** less intrusive, but it would not test the visibility checks or the fill itself, which is what users meet.
  - **A commercial top-sites list:** better data, but a licence cost and terms for a closed-source product.

  Implementation notes (M4.7):
  - **The first list is not yet checked.** It has 102 sites, but only 18 carry selectors (forms that are well known), not the 50 §2 asks for. The others use the selector-free rule.
    - The list is marked unchecked until the first nightly runs confirm each page. Checking pages one by one in a browser was too slow for this task.
    - Growing the selector count to 50 and the list towards 1,000 goes with M10 (Tranco order).
  - **Split one-time codes:** a code split over several boxes, or a code field with `maxlength` under 6, is now copied, not filled (`plan.ts`). The page's own script spreads a paste; a fill into the first box would not.
  - **An icon the page moves** is put back at once. Script moves are seen through a `MutationObserver` on the icon, stylesheet moves by a 100 ms check. For 500 ms afterwards it lets clicks through to the page, and its frame restarts its visibility wait, so a click right after the slide never reaches it.
  - **Shared launch code:** `lib/extension.ts` holds the launch and sign-up code that the browser tests and the runner share. The single-page cases moved from `fill.spec.ts` to `fixtures.json`.
  - **The runner** waits 1.5 s after load for pages that draw their form with script, and takes its fill targets from the D38 report builder, so the report never holds values. A selector that matches nothing reads as empty at once, instead of waiting for the element (the browser tests caught that hang).
- **D40 — Extension builds: one Chromium build for Chrome, Edge, Brave and Opera, a Firefox build, packaging and tests** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "Builds for Chrome, Edge, Firefox, Brave, Opera; Safari via macOS app (M5)". D34 built Chromium only. Edge, Brave and Opera run Chromium and accept the Chrome package. Firefox's MV3 differs in ways that touch our design (D34–D37):
  - the background is an event page, not a service worker
  - there is no `offscreen` API
  - there is no `IntersectionObserver` v2 (`trackVisibility`), which the inline icon's click guard and the frame check rely on
  - host permissions are opt-in after install
  - the page-world content script and `runtime.getContexts` differ
  - AMO review wants readable code
  Open:
  - what Firefox users get
  - how one source tree builds both
  - how each build is tested
  - what the store packages are

  Proposal:
  1. **Two builds from one source tree** (`build.mjs --target chromium|firefox`), into `dist/chromium/` and `dist/firefox/`, each zipped as `pwm-<target>-<version>.zip`.
     - **What differs:** each build gets its own manifest from `static/manifest.<target>.json` and a small `platform` module chosen at build time (`src/background/targets/`). Everything else is shared.
     - **Chromium:**
       - **Packaged as now**, for the Chrome Web Store (also Brave), Edge Add-ons and Opera add-ons. It is the same zip for all.
       - **Store specifics:** none needed. Each store's listing and signing are done at release (M10).
     - **Firefox** (`browser_specific_settings.gecko`, `strict_min_version` 128; an add-on id placeholder until D1):
       - **Background:** `background.scripts` (module), an event page; the WASM core loads the same way.
       - **Clipboard:** written from the background page itself (`document.execCommand` with the copy event, as the offscreen page does now). No `offscreen` permission.
       - **Session store:** `storage.session` (Firefox 115+), unchanged.
       - **Hosted API access:** asked for at sign-in (Firefox treats host permissions as optional); the popup explains why.
       - **Reviewable code:** the build is not minified, and AMO reviewers get a source zip (closed source, but reviewers see it, as AMO requires for bundled code).
  2. **What Firefox users get, given no `IntersectionObserver` v2:**
     - **Filling from the popup and the shortcut:** yes, with every D35 check except v2. As D35 already says, fills only in the **top frame** (a frame cannot tell whether its parent covers it).
     - **The inline icon: off on Firefox.** Its click guard needs v2, and without it a page could make it transparent or cover it (D35 §3). The setting is hidden, and "Fill and save on sign-in pages" offers only the save prompts. The save card is likewise replaced by the toolbar badge and the popup (its clicks need the same guard).
     - **Passkeys:** the page-world wrapper is registered with `world: "MAIN"` (Firefox 128+). The confirmation window works as on Chromium. If Firefox refuses the registration, the switch says passkeys are not available on this browser.
     - **Logged as platform gaps** (CLAUDE.md parity rule): the inline icon, the save card and filling in frames on Firefox. Revisit when Firefox ships `IntersectionObserver` v2.
  3. **Code shape:**
     - **One switch for what a browser can do:** `capabilities` (`visibilityV2`, `offscreen`, `serviceWorker`), checked at run time where cheap, set at build time where not. The background and content code ask it instead of naming browsers.
     - **The build check (D34/D35) per target:** each manifest's permissions are an exact list (Firefox: no `offscreen`), no remote code, no extra web-accessible resources. Firefox also gets `web-ext lint` (Mozilla's linter, MPL-2.0, dev-only).
  4. **Tests:**
     - **Unit:** Vitest runs the background against both capability sets (a Firefox-shaped fake platform: no offscreen, no v2). This covers that the inline icon stays off, that fills in frames are refused without v2, and the background-page clipboard.
     - **Chromium in the browser:** as now (fixtures, flows, runner), plus loading the packaged zip once to check it.
     - **Firefox in the browser:** Playwright cannot load an extension into Firefox. CI runs `web-ext lint`, and a smoke test drives Firefox with `web-ext run` + geckodriver (Selenium, Apache-2.0): install the build, open the popup page, sign up against the local server, fill `plain.html` from the background. The full fixture suite on Firefox follows once that path is stable.
     - **Edge, Brave and Opera:** no separate automated run; they get the identical Chromium zip. A manual smoke checklist (load unpacked, sign in, fill, passkey) lives in `docs/QUALITY.md` for release candidates.
  5. **Not in M4.8:**
     - **Store accounts, listings, signing and publishing:** at release, M10, with the founder's accounts.
     - **Safari:** M5.6.
     - **Firefox for Android:** later; it needs its own UI work.
     - **Extension icons and names:** these wait for D1.

  Alternatives:
  - **One manifest for both** (both `background.scripts` and `service_worker`): Chrome now accepts that, but the build check and the permissions would blur. Two small manifests are clearer.
  - **A weaker click guard on Firefox** (`elementsFromPoint` plus timing): an inline icon for Firefox users, but it is exactly the DEF CON 2025 class of attack D35 closes. Off until v2 exists.
  - **Skip Firefox until it has v2:** simplest, but Firefox users would get no extension at all, and popup filling works fine there.

  Implementation notes (M4.8):
  - **Where the targets differ:**
    - **Manifests:** `extension/manifests/chromium.json` and `firefox.json`, not `static/manifest.<target>.json`.
    - **No `src/background/targets/` module:** the build defines `PWM_TARGET`, and `chrome.ts` picks the clipboard (offscreen document or the background page) and the capabilities from it.
    - **Capabilities:** a single flag, `inlineUi`, for the icon and the save card. The offscreen document and the service worker are settled by the target's manifest. `IntersectionObserver` v2 is still checked at run time in the content script, which already filled only in the top frame without it.
  - **Firefox build:** syntax is folded (so the test hook is dropped), but names and whitespace are kept for AMO review. `offscreen.html` and the inline icon files are left out.
  - **The switch on Firefox** is relabelled "Save logins when you sign in", rather than hidden. The sign-up offer says the same.
  - **Passkeys:** if registering the page script fails, the switch stays off and says "This browser cannot pass passkey requests to PWM yet."
  - **Server access on Firefox:** asked for when the user signs in or creates an account, with a line in the form that says why. A refusal stops there with a message.
  - **The Firefox smoke test** (`tests/autofill-harness/firefox/smoke.ts`) talks W3C WebDriver to geckodriver over HTTP, with no Selenium and no `web-ext run`, so it adds no dependency.
    - **What it does:** installs the zip as a temporary add-on with a fixed UUID, opens the popup and scores a password. The score is the background's WASM core answering.
    - **Not covered yet:** signing up and filling. Firefox asks for host access in a prompt that WebDriver cannot answer, so they wait for a test-only grant.
  - **`web-ext lint`** runs pinned (`web-ext@8.3.0`) through `npx` in CI only; it is not a dependency. It reports no errors and one warning: the `innerHTML` assignment in Preact's `dangerouslySetInnerHTML` support, which the popup never uses.
  - **The smoke test opens the popup from the browser's chrome context** (geckodriver's `--allow-system-access`), because WebDriver may not navigate a tab to an extension page.
  - **Deferred to M10 (store submission):**
    - **AMO:** the source zip for reviewers, and the `data_collection_permissions` declaration that AMO asks of new add-ons.
    - **Chromium:** loading the store zip itself in a browser test; the harness loads the test build.
  - **Parity gaps logged:** on Firefox, the inline icon, the save card and filling in frames. Revisit when Firefox ships `IntersectionObserver` v2.
- **D41 — The shared Swift package for iOS and macOS: layout, the core as an XCFramework, the async layer, secrets, design tokens and tests** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "Shared Swift package over UniFFI core; design tokens". D31 gives the core's API (`PwmClient`, the step pattern for every server conversation, items as D18 JSON), and its bindings already compile and run with `swiftc` on Linux in CI.
  Open:
  - how the package is laid out
  - how the Rust library reaches Xcode
  - what the Swift layer adds over the generated bindings
  - where the cache and keys live on the device
  - how secrets are handled in Swift
  - how design tokens become Swift
  - how all this is tested when development happens on Windows, without Xcode

  Proposal:
  1. **One SwiftPM package, `apps/apple/PWMKit`, with three targets.** The iOS app, its AutoFill extension (M5.5) and the macOS app (M5.6) all depend on it.
     - **`PWMCore`:** the generated UniFFI Swift file over a binary target, `PWMCoreFFI.xcframework`. It is generated, never edited.
     - **`PWMKit`:** the Swift layer apps use (§3). It is the only target apps import for core features.
     - **`PWMDesign`:** the design tokens as SwiftUI values (§5).
     - **Minimum versions:** iOS 17 and macOS 14, for SwiftUI's Observation. No third-party Swift dependencies.
  2. **The core as an XCFramework**, built by `just apple` on a Mac:
     - **Targets:** `pwm-ffi` for `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-apple-darwin` and `x86_64-apple-darwin`. The two macOS slices are merged with `lipo`, then joined with `xcodebuild -create-xcframework`.
     - **Release profile:** the workspace profile, with symbols stripped.
     - **Not committed:** the XCFramework and the generated Swift file go to `apps/apple/PWMKit/Generated/` (gitignored). The package fails to resolve until `just apple` has run, and the error says so.
     - **Intel simulators (`x86_64-apple-ios`):** left out; Apple Silicon Macs only for development.
  3. **What `PWMKit` adds**, so apps never touch steps, requests or JSON:
     - **`Session`, an actor around one `PwmClient`:** async methods `signUp`, `logIn`, `unlock`, `sync` and so on.
       - **The step loop:** each method drives the D28/D31 loop with `URLSession`: send the request, hand back the answer, then `network_failed` on a transport error. Task cancellation calls `cancel`.
       - **Events:** published as an `AsyncStream`.
       - **The HTTP stack:** `URLSession` with an ephemeral configuration (no cookies, no cache on disk) and TLS left to the system.
     - **Items:** `Codable` Swift types for the D18 item JSON. Unknown fields are kept, as D18 requires, by keeping the raw JSON beside the typed fields and merging on save.
     - **Errors:** a Swift `PWMError` that wraps the core's stable codes, with plain-language messages for the UI.
     - **The generator, strength and TOTP:** thin wrappers.
  4. **Storage and secrets:**
     - **The cache:** `PwmClient.open(path)` in the App Group container, so the AutoFill extension (M5.5) reads the same file. It holds ciphertext only (D20), with file protection `completeUntilFirstUserAuthentication` so background sync (M5.8) and AutoFill work after the first unlock.
     - **The device keys:** the Keychain, through a `KeyStore` protocol with an in-memory fake for tests. Items are this-device-only and after-first-unlock, in a shared access group for the extension. The core decides what is stored; Swift only keeps the bytes.
     - **Face ID / Touch ID** (Secure Enclave wrapping) is M5.4, not here.
     - **Secrets in Swift** (D31 §4): Swift strings cannot be wiped.
       - **Short-lived:** the master password, the Secret Key and revealed fields stay in local variables and are passed straight to the core, never stored in long-lived state.
       - **Redacted:** a `Redacted<T>` wrapper whose `description` and `debugDescription` are `"<redacted>"` is used for any value that must be held.
       - **Never logged:** `os.Logger` calls take only codes and counts. A unit test checks that `Redacted`'s descriptions and `PWMError` messages carry no values.
  5. **Design tokens:** `scripts/tokens-swift.mjs` turns `design/tokens.json` into `PWMDesign/Tokens.swift`. Colors pair light and dark, plus spacing, radius and type sizes.
     - **Committed,** since SwiftPM cannot run Node at build time.
     - **Kept current:** `just check` fails if the file is stale (the same tokens feed the extension's CSS, D34).
  6. **Tests and CI.** This repository is developed on Windows without Xcode, so every Apple check runs in CI:
     - **Linux (the existing `bindings` job, no macOS minutes):** `swift test` for `PWMKit` against the Linux build of `pwm-ffi`.
       - **Covered:** the step loop against the local server (in `docker compose`): sign up, log in on a second session, save an item, sync. Also the item JSON round trip, error codes and `Redacted`.
       - **Apple-only code** (Keychain, file protection, SwiftUI) sits behind `#if canImport(...)` and is tested on macOS.
     - **macOS (new job `apple`, on PRs touching `apps/apple` or `core/`):** `just apple`, then `swift test` on macOS, and an iOS simulator build of the package (`xcodebuild build -scheme PWMKit -destination 'generic/platform=iOS Simulator'`).
       - **Cost:** about 10–15 macOS minutes per run, which bill at 10× on a private repository. Nightly is skipped unless `core/` changed.
     - **Not in M5.1:** an app target, signing, bundle ids and team ids. They wait for D1 and the Apple developer account (M5.2).
  7. **What the founder needs for M5.2 onward:**
     - **A Mac with Xcode,** to run the apps; CI can build and test but not run the UI.
     - **An Apple Developer account,** for signing, App Groups and the AutoFill entitlement.
     - M5.1 itself needs neither.

  Alternatives:
  - **Commit the XCFramework (or publish it as a release asset):** Xcode would open the package without Rust, but binaries in git, or a download step, are worse than one `just apple`.
  - **A separate Swift package per app:** duplicates the session and item code that the app and its AutoFill extension must share.
  - **Generate Swift types for items from the Rust model:** D31 chose JSON so bindings don't mirror the item model; hand-written `Codable` with a round-trip test is enough.
  - **macOS CI only:** simpler, but every PR touching the core would pay macOS minutes; the Linux job catches most breakage first.

  Implementation notes (M5.1):
  - **Names:** the XCFramework is `pwm_ffiFFI.xcframework`, named after the C module the UniFFI Swift file imports, not `PWMCoreFFI`. SwiftPM ties a binary target's name to its module.
  - **Generated files:** `apps/apple/PWMKit/generate.sh` (`apple` from `just apple`, `linux` in CI) writes them to `Generated/`:
    - `PWMCore/pwm_ffi.swift`
    - on Linux, `linux/` (the header and a module map that links `libpwm_ffi`)
    - on a Mac, the XCFramework
  - **Swift 6 language mode**, with strict concurrency, for `PWMKit` and `PWMDesign`. The generated `PWMCore` builds in Swift 5 mode and is imported with `@preconcurrency`.
  - **Items:** rather than `Codable` structs, `Item`, `Login` and `LoginURL` wrap the item's JSON object, with typed accessors that read and write it in place.
    - Unknown fields at any depth are kept exactly, with no merge step.
    - Items of a newer schema or of an unknown type are read-only (`isEditable`), and `Session.save` refuses them.
    - `Item`, `Login` and `JSONValue` print as redacted.
  - **`Session` keeps four things in the `KeyStore`:** the Secret Key, the tokens, the device id and the email.
    - `signUp` also creates the first vault.
    - `logIn` syncs at once.
    - A `Wait` step ends a sync as `.retry(afterMs:)`.
    - The core's own requests and the transport carry only `Sendable` copies (`HTTPRequest`, `HTTPResponse`).
  - **Text styles** scale with Dynamic Type relative to the nearest system text style (`@ScaledMetric`). The token sizes are the starting point.
  - **The Linux job** runs `pwm-server` from cargo with a Postgres service, rather than the compose image, to save the image build.
  - **The macOS job** is skipped on the nightly schedule. On PRs and pushes, a small Linux job (`apple-changes`) decides from the changed paths whether it runs.
  - **Not verified on this machine:** it has no Swift toolchain, so the package's first compile and test runs are in CI.
- **D42 — iOS onboarding: the app target before D1, the Emergency Kit PDF in the core, signing in on a new device, and splitting out device approval** · settled 2026-11-02 · founder (approved as proposed).
  The roadmap says "Onboarding: create account, Emergency Kit PDF, sign in on new device (password + Secret Key, or approve from existing device)". `Session` (D41) already runs sign-up, login and unlock. This is the first task with an app target, and the following are open:
  - how to have an Xcode project when development happens on Windows, before D1 names the product and before an Apple Developer account exists
  - where the screens live, so the macOS app (M5.6) reuses them
  - who draws the Emergency Kit PDF, and what its QR code holds
  - how the app is tested
  - whether approving a new device from an existing one belongs in this task at all

  Proposal:
  1. **Split device approval into its own task, M5.9**, placed after M5.4, with its own decision.
     - **Why:** it is a new protocol: the Secret Key sent over an authenticated X25519 channel (`SECURITY.md`). It needs:
       - its own test vectors and a security review
       - relay endpoints on the server
       - an approving side on every platform that is already signed in, starting with the extension
     - **M5.2 keeps:** create account, the Emergency Kit, and sign-in with the master password plus the Secret Key, typed or scanned from the kit.
     - The camera scanner built here is reused by M5.9.
  2. **The Xcode project, generated by XcodeGen.**
     - **Committed:** `apps/apple/project.yml`. The generated `PWM.xcodeproj` is gitignored.
       - XcodeGen (MIT) is a build tool that is never shipped, like the npm dev tooling under D9.
       - `just apple` runs it after building the XCFramework.
     - **One iOS app target, `PWM`** (iOS 17), that depends on the package. It is a thin shell: the `App` struct, `Info.plist`, assets and the camera usage text.
     - **Placeholders until D1:**
       - display name `PWM`
       - bundle id `invalid.pwm.app` (the reverse of `pwm.invalid`, as with the Firefox add-on id)
       - a placeholder icon
     - **Signing:**
       - **Locally:** the team id goes in `apps/apple/Local.xcconfig`, which is gitignored. A committed `Local.example.xcconfig` shows the one setting.
       - **In CI:** simulator builds are signed to run locally (ad hoc), with no team.
     - **No entitlements yet.** The App Group and the shared Keychain group wait for M5.5, whose AutoFill extension is their first user.
       - **Until then:** the cache lives in the app's own Application Support folder and the Keychain items in the app's default group.
       - **Migration:** M5.5 moves the cache file and the Keychain items into the shared ones, with a migration test, so a TestFlight install keeps its account.
  3. **Screens in a new package target, `PWMUI`,** so the macOS app (M5.6) reuses them.
     - **View models:** `@Observable` classes with no SwiftUI. They are tested by `swift test` on Linux and macOS.
     - **Views:** SwiftUI, behind `#if canImport(SwiftUI)`. They use `PWMDesign` tokens.
     - **Parity gap:** onboarding on macOS ships with M5.6, and Android onboarding with M6. This is logged here as a parity gap.
  4. **The screens:**
     - **Welcome:**
       - "Create account" and "Sign in".
       - **The server:** the hosted one (a placeholder address until D1), or self-hosted behind "Other server", as in the extension.
     - **Create account:**
       - Fields: email, master password, and the password again.
       - **The strength meter:** the same as the extension. Continue needs a score of at least 3.
       - **One plain line:** "Nobody, including us, can reset your master password." (`SECURITY.md`, Recovery).
     - **Save your Secret Key** (shown once):
       - **The key** in large monospaced text.
       - **"Save Emergency Kit":** the share sheet (Save to Files, Print, AirDrop), with the PDF passed as data and no temporary file.
       - **"Copy":** a local-only pasteboard item that expires in 30 seconds, never synced to other devices through Handoff.
       - **"I saved it":** enabled after a save, a print or a copy, as in the extension.
     - **Sign in:**
       - Fields: email, master password and Secret Key.
       - **"Scan Emergency Kit":** VisionKit's `DataScannerViewController`. It fills the email, the Secret Key and a self-hosted server address, never the master password. The button is hidden on devices without scanner support.
     - **Unlock** on later launches: the master password, with "Sign out" beside it.
     - **After sign-in, a placeholder vault screen:** the item count, Lock and Sign out. The real vault list is M5.3.
     - **The device name sent at sign-in** is `UIDevice.model` ("iPhone", "iPad"), because the user's own device name needs an entitlement. Rename comes in M5.3.
     - **Copy:** English, in a String Catalog so translation can follow.
  5. **The Emergency Kit PDF, drawn in the core:** a new crate, `pwm-kit`, exposed through UniFFI and WASM, so every platform prints the same kit.
     - **On the page:**
       - the email
       - the Secret Key, in large monospaced text, grouped
       - a blank line to write the master password by hand
       - the server address, only when self-hosted
       - the account id, small (`SECURITY.md` D14)
       - the creation date
       - the QR code
       - three lines on keeping it offline, and that nobody can recover it
     - **Paper:** A4 or US Letter, which the platform chooses from the locale.
     - **The PDF itself:**
       - **Writer:** a small hand-written PDF 1.4 writer, one page, uncompressed, with the standard Helvetica and Courier fonts (no font embedding) and the QR code drawn as filled squares.
       - **Deterministic:** no metadata beyond the given date, so one input always gives the same bytes.
       - **Not crypto,** so the "no hand-rolled primitives" rule doesn't apply.
     - **The QR code:**
       - **Library:** the `qrcode` crate (MIT or Apache-2.0) with default features off, which leaves no other dependencies.
       - **Contents:** `pwm-kit:1?email=…&secret-key=…&server=…`, percent-encoded, with `server` only when self-hosted. `1` is the format version (versioned formats rule).
       - **Never the master password.**
       - **Not registered as a URL scheme,** so the system camera never hands it to an app or the OS. Only our scanner parses it, through the core's `parse_kit_qr`.
     - **Characters outside the fonts:** the standard fonts cover Latin-1 only (WinAnsi). Other characters in an email are printed as `?`, while the QR keeps the exact email. Embedding a font comes with translations.
     - **Secrets:** the bytes are returned as `Secret<Vec<u8>>` in Rust. Swift gets `Data` and drops it after the share sheet closes.
     - **The extension switches too:** "Download Emergency Kit" saves the same PDF through WASM instead of the text file, so the kits match. This is a small change in `SignedOut.tsx`.
  6. **Tests:**
     - **`pwm-kit` (tests first):**
       - golden PDFs for A4 and Letter, byte-identical
       - `qpdf --check` on them in CI (qpdf is Apache-2.0, from apt)
       - the QR payload's round trip, and refusal of a wrong version or a missing field
       - a test that the Secret Key never appears in `Debug` output
       - a new fuzz target, `kit_qr_parse`
     - **View models, in `swift test` on Linux and macOS:** the strength gate, the "I saved it" gate, kit QR scan results filling the right fields, and error messages.
     - **UI tests (XCUITest) in the macOS `apple` job,** against a real local server:
       - The job adds Postgres from Homebrew and `pwm-server` built from cargo, as the Linux job does.
       - The flow: create an account, see the Secret Key, continue to the vault screen, sign out, then sign in again with the email, password and that Secret Key.
       - **Cost:** about 5–8 more macOS minutes, only on PRs that touch `apps/apple` or `core/`.
     - **Extension:** the existing popup test downloads the kit and checks for a PDF header.
  7. **What the founder can do but doesn't need to for M5.2:**
     - On a Mac: `brew install xcodegen`, `just apple`, open `apps/apple/PWM.xcodeproj` and run on a simulator.
     - **A real iPhone needs the Apple Developer account** and the team id in `Local.xcconfig`. CI needs neither.

  Alternatives:
  - **Keep device approval in M5.2:** it doubles the task, and it puts a new protocol and new server endpoints in a PR that is mostly UI.
  - **A hand-written `.xcodeproj`:** no tool, but the project file can't be reviewed or merged by hand, and it can't be checked on Windows. **Tuist** works too, but it is heavier than needed for one app target.
  - **Draw the kit on each platform** (PDFKit on Apple, `PdfDocument` on Android, the browser's print view): native, but each kit would look different and need its own tests. The QR format would also be repeated four times.
  - **A PDF crate** (`printpdf`, MIT): fonts and layout come for free, but it pulls in many dependencies for one fixed page.
  - **Wait for D1 and the Apple account:** it blocks all of M5. The placeholders are a handful of settings, changed in one place later.

  Implementation notes (M5.2):
  - **Sign-out wipes the cache:** a new core call, `sign_out`, locks and clears the store. `Session.signOut` also deletes the tokens, the Secret Key and the email from the Keychain, and keeps the device id. This matches the extension, which starts a fresh core.
  - **New error codes:** `invalid_email`, `invalid_server`, `invalid_date`, `not_a_kit` and `kit_too_new`. A bad account id is `invalid_id`.
  - **The QR code's test:** `rqrr` (MIT or Apache-2.0, a dev-dependency only) redraws the PDF's own squares as an image and decodes them. This checks the drawing, not just the encoder.
  - **Self-hosted over plain HTTP:** only to `localhost`, `127.0.0.1` or `[::1]`, in the kit and the app alike, as in the extension. The app's `Info.plist` allows local networking for this.
  - **The server choice** sits on the welcome screen, under "Other server". A scanned kit sets it. It is kept in `UserDefaults`, and a `-server <url>` launch argument overrides it, which is how the UI test points the app at its server.
  - **The master password fields have no `newPassword` content type.** iOS would offer to generate a strong password and keep it in iCloud Keychain, and in the simulator it took over the fields. The master password must be one the user knows. Sign-in and unlock keep `password`, so a password the user saved themselves can still fill.
  - **Copy:** a String Catalog (`App/Localizable.xcstrings`) with English as the source. The screens' literals are localizable keys looked up in the app's bundle.
  - **The extension popup** has no UI tests, so the kit download is covered by a background test (the request and its signed-out error) and by the WASM test on the real build.
  - **Not verified on this machine:** there is no Xcode here. The package, its view models and the server tests run on Linux in the `swift` Docker image. The views, the scanner, the Keychain, the app target and the UI test run first in the macOS job.
  - **Recheck of the `cargo deny` ignores** (due at M5.1): unchanged upstream. The next recheck is at M5.5.
- **D43 — iOS vault screens: splitting M5.3, the screens, search, editing, the lock timeout, and the device, health and export work that is still missing** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says "Vault list, search, item detail/edit, generator, health report, settings, lock timeout", plus the device list with rename and revoke from M3. `Session` (D41) already lists, saves, trashes and deletes items, gives TOTP codes and generates passwords. The rest is missing:
  - **The health report** (D22) exists only inside `pwm-health`. It isn't in `pwm-client`, UniFFI or WASM, and no platform has the HIBP fetcher.
  - **The device list** exists only on the server (`GET /v1/devices`). No client opens the sealed names, and there is no rename endpoint.
  - **Export** (D24) exists only inside `pwm-export`. No platform has an export screen, including the extension, although `CLAUDE.md` says export is always available. M2.7 deferred the dialogs to "M4/M5", and M4 didn't do them.

  Open: how to cut this into reviewable PRs, what the screens are, where search lives, how editing handles item kinds and newer items, how the lock timeout works on iOS, and the rename endpoint.

  Proposal:
  1. **Split the task in two.**
     - **M5.3, the vault, mostly Swift over the existing `Session`:**
       - the vault list, search, filters and trash
       - item detail and edit for every item type
       - the generator
       - TOTP
       - settings with the lock timeout
     - **M5.10 (new, right after M5.3), the parts that need core and server work:**
       - the health report
       - the device list with rename and revoke
       - export
     - **Why:** the PR keeps to one task. Each part with a new protocol surface (a new endpoint, a network call to HIBP, a re-auth flow) is reviewed on its own, not inside a large UI PR.
     - **The extension's gaps:** health, the device list and export are added to the extension in a new task, M4.10, which comes right after M5.10 and uses the same WASM calls. Until then they are logged here as parity gaps. Export must be done before the beta (M10).
  2. **The screens (M5.3), in `PWMUI`,** so macOS (M5.6) reuses them:
     - **The layout:**
       - **iPhone:** three tabs: Vault, Generator and Settings.
       - **iPad, and macOS later:** a sidebar with the same sections.
     - **The vault list:**
       - **Sections:** All items, Favorites, Logins, Cards, Identities, Notes, each folder, and Trash.
       - **Rows:** title and username (or the card's last 4 digits), with an icon for the item type.
       - **No website icons:** fetching favicons would tell third parties which sites are in the vault.
       - **Sync:** pull to refresh syncs, and so does every save. Background sync is M5.8.
     - **Search:** see point 3.
     - **Item detail:**
       - **Fields:** every field of the type.
       - **Hidden values** (passwords, card number and security code, hidden custom fields) are masked until tapped, and masked again when the screen goes.
       - **Copy:** each field has Copy, through the existing local-only, 30-second pasteboard (D42).
       - **Logins:** URLs open in the browser. The TOTP code shows with its countdown.
       - **Passkeys** show as "Passkey for example.com", read-only.
       - **Password history:** the previous passwords from the item's history (D19), read-only, newest first.
     - **Edit and new item:**
       - **Fields:** all D18 fields per type: login, card, identity, note, plus custom fields of every kind.
       - **The folder** can be picked, or a new one made in the picker.
       - **Tags** are entered as text.
       - **TOTP setup:** a pasted `otpauth://` URI or a bare secret. "Scan QR code" reuses the M5.2 scanner, and the core's D23 parser checks the result.
       - **The password field** has a Generate button, which opens the generator as a sheet.
     - **Delete and trash:**
       - "Delete" moves an item to Trash.
       - **Trash** offers Restore and Delete forever, which asks first.
       - **Emptying old trash:** the core's existing `purge_trash` runs after each sync.
     - **The generator tab:**
       - **Options:** the same as the extension: password length and character sets, or a passphrase's word count and separator.
       - **The strength meter**, Copy, and Regenerate.
       - **Kept in memory only:** the generated values are never stored.
     - **Settings (M5.3 part):**
       - the account email and the server, read-only
       - the lock timeout (point 5)
       - "Lock now"
       - "Save Emergency Kit" again, the same share sheet as onboarding
       - "Sign out"
       - "Third-party notices", which shows the bundled `THIRD_PARTY_NOTICES` (S6 requires them in every app)
       - the app version
     - **Copy** goes in the String Catalog, as in M5.2.
  3. **Search, specified here so Android and desktop match the extension:**
     - **What it matches:** a case-insensitive substring of the title, the username, a URL, or a card's last 4 digits, after trimming spaces.
     - **What it skips:** trashed items and folders.
     - **Order:** by title, using the locale's ordering, then by id.
     - **Where it runs:** in the view model, over the items `Session` has already opened.
       - **Not in the core:** this is matching on fields the platform already reads (D41's `Item` accessors), not vault parsing.
       - **Nothing is indexed:** no search index is written anywhere.
     - **Scale:** 5,000 items should open and filter without a visible delay. A view-model test checks this with generated items.
  4. **Editing:**
     - **Swift accessors** for cards, identities and custom fields join `Login` in `Item.swift`. As now, they edit the D18 JSON in place, so fields a newer client wrote are kept.
     - **The core still validates every save** (`save_item`), and its error text is shown.
     - **Newer items stay read-only:** items with a newer schema or an unknown type show read-only with "Update the app to edit this item" (D18).
     - **Unsaved edits:** leaving the edit screen with changes asks to discard them.
     - **A sync during an edit:** the save still wins. The other version goes to the item's history, as the core's merge already does (D19), so neither edit is lost.
  5. **The lock timeout on iOS:**
     - **Choices:**
       - **Immediately:** lock as soon as the app leaves the screen.
       - **1, 5, 15 or 60 minutes, or 8 hours:** the extension's choices.
       - **Default:** 15 minutes, as in the extension.
       - **Where it's kept:** in `UserDefaults`. It isn't secret and isn't synced.
     - **What the time counts:** the time the app spends in the background, measured with `ContinuousClock`. That clock keeps running while the phone sleeps and ignores changes to the wall clock.
       - **When the app returns** after the timeout, it locks before it shows anything.
       - **Not counted in the foreground:** idle time while the app is open. Phones lock their own screens.
     - **Locking the device locks the vault too,** whatever the setting. The app uses `protectedDataWillBecomeUnavailableNotification` for this.
     - **If iOS ends the app,** the keys go with the process, so the next launch asks to unlock as now.
     - **Face ID** (M5.4) will shorten this; the timeout rules stay.
  6. **M5.10: devices.**
     - **Core:**
       - **`pwm-client` lists the devices:** it opens each sealed name under the D26 v1 format. A name that fails to open shows as "Unknown device", never an error.
       - **It seals new names:** the same format, with the AAD bound to the account id and the device id, so a sealed name can't be moved to another device.
       - **Through UniFFI and WASM.**
     - **The server, `PATCH /v1/devices/{id} {name_sealed}`:**
       - **Answers:**
         - **204** when the name is saved.
         - **404** for a device id that isn't one of the account's.
         - **409 `device_revoked`** for a revoked device.
         - **400** for a blob that isn't a v1 envelope of exactly the padded size.
       - **What it stores:** only the ciphertext, in the existing column. No migration is needed.
       - **No logging:** neither the blob nor the device id is logged.
     - **The device name after login:** the client now seals its name right after login, through the new endpoint. Today only sign-up sends a sealed name. Login can't, because the keys open only after login finishes, so a device that signs in has no name at all.
     - **The screen, in Settings:**
       - **Each device:** name, platform, when it was added and last seen, and "This device" for the current one.
       - **Rename** any of the account's devices.
       - **Revoke** any device but this one, after asking first. This device uses Sign out instead.
  7. **M5.10: the health report (D22, unchanged).**
     - **Core:** `pwm-client` runs the weak and reused checks over every vault. For the breach check, it builds the HIBP prefixes and parses the responses. All of it goes through UniFFI and WASM.
     - **iOS fetcher:**
       - **The request:** an ephemeral `URLSession` with no cookies and no cache. It sends `Add-Padding: true`, straight to `api.pwnedpasswords.com`.
       - **Once per prefix** in each run.
     - **The screen:**
       - counts for weak, reused and breached, each opening its list of items
       - each item opens its edit screen, with Generate
       - the breach switch in Settings, on by default with D22's sentence
     - **Results are kept in memory** for the session only.
  8. **M5.10: export (D24, unchanged).**
     - **Core:** `pwm-client` wraps `authorize_export` and the `.pwmx`, JSON and CSV writers, through UniFFI and WASM. The grant stays in the core and never crosses the FFI.
     - **The flow, in Settings:**
       1. Pick a format.
       2. Enter the master password: Face ID doesn't count (D24).
       3. **For `.pwmx`:** an export password, entered twice, with the strength meter.
       4. **For JSON and CSV:** a plain warning that the file isn't encrypted, and anyone who gets it can read every password.
       5. Save the file through the share sheet or the file exporter, passed as data with no temporary file left behind.
     - **Every plan:** no plan check, now or in M9.
  9. **Tests:**
     - **View models (`swift test` on Linux and macOS):**
       - search and order
       - section filters
       - editing per type, with unknown fields kept
       - the discard prompt
       - read-only newer items
       - Trash
       - generator options
       - the lock timeout with an injected clock
       - **M5.10:** the device list and rename, health counts, the export steps, and refusal of a wrong master password
     - **Core (tests first, M5.10):**
       - the device name round trip, including a name moved to another device id being refused
       - health through the client over several vaults
       - export through the client, including a reused grant being refused
     - **Server (M5.10):** `PATCH` against real Postgres, covering each answer above, with only ciphertext stored. The plaintext-column test stays green.
     - **UI test (XCUITest), extending the M5.2 flow:**
       - **Logins:** add one with a generated password, find it by search, edit it, copy the password, then trash it and restore it.
       - **Lock:** set "Immediately", send the app to the background, and see the unlock screen.
       - **M5.10:** rename this device and see the new name after a sync.
  10. **Logged parity gaps:**
      - **macOS:** all of this ships with M5.6, on the same `PWMUI` screens.
      - **Android:** M6.1.
      - **The extension:** health, devices and export come in M4.10. Editing items in the popup stays out of scope, as before.

  Alternatives:
  - **One PR for all of M5.3:** fewer steps, but a very large PR that mixes UI with a new endpoint, a network call and the export re-auth.
  - **Search in the core:** every platform would get identical matching for free. But each item would cross the FFI an extra time, and the rule is small enough to specify (point 3).
  - **Website icons in the list:** they look better, but fetching them leaks the vault's sites. If they come later, they'd be an opt-in through our own proxy.
  - **Counting idle time in the foreground:** it matches the extension more closely, but on a phone the screen lock already covers it, and it needs a global touch monitor.
  - **Leaving extension export until M10:** it meets the rule only on paper. M4.10 closes the gap sooner, with WASM calls M5.10 already makes.

  Implementation notes (M5.3):
  - **One-time-code setup is checked by the core:** a new `normalize_totp` in `pwm-client` (and UniFFI) runs D23's parser when an item is saved and refuses other text with the new code `invalid_totp`. The WASM gets it when the popup edits items.
    - **D23's rule accepts any letters as a bare secret:** `hello` is a valid base32 key. A minimum length would catch typos, but it changes D23, so it is left for a decision.
  - **Lock timeout:** from leaving the app until the timeout check is done, the vault is covered. The Secret Key screen never locks: it is shown once and stays until "I saved it". The app-switcher snapshot is M5.7.
  - **A refused refresh token** (`signed_out` from a sync) shows a notice; the user signs out and in again. A re-login screen that keeps unsent changes is M5.8.
  - **Websites keep their match mode**, with a picker (`AUTOFILL.md`). A mode from a newer app version shows as it is and is kept.
  - **The edit screen's lists** (websites, other fields) give each row an id, so adding and deleting rows is safe in SwiftUI.
  - **The scanner** is now `QRScanner`, shared by the kit and the one-time code. The camera text says both.
  - **Third-party notices:** the app bundles `THIRD_PARTY_NOTICES/rust.html`, `data.md` and `data-autofill.md` when `just notices` has made them; without them, Settings says so. The release pipeline (M10.1) must run `just notices` first.
  - **Not verified on this machine:** there is no Xcode here. The view models and 56 package tests run on Linux in the `swift` Docker image. The SwiftUI views and the UI test run first in the macOS job.

  Implementation notes (M5.10):
  - **Devices run in the sync engine:** list, rename and revoke are engine tasks, so they share its token refresh and the `device_revoked` handling. A 404 or 409 answer is `unknown_device`.
  - **The name after login** is the login flow's last step, a `PATCH` once the keyset is open. It is best-effort: its answer, or no answer, still ends in `LoggedIn`. `start_login` takes the device name on every platform; the extension sends the same name as at sign-up.
  - **The sealed size** is `SEALED_DEVICE_NAME_LEN` (105 bytes) in `pwm-crypto`. The server refuses any other size, and leaves a revoked device's name as it was.
  - **Export:** the grant lives in the client, is used up by one export and is dropped when locking. A weak `.pwmx` password is refused before the grant is used. New codes: `export_not_authorized`, `export_expired`, `weak_export_password`, `invalid_device_name`, `unknown_device`.
  - **Health:** the breach check is an object the platform drives: its prefixes, each range body, then the result. On iOS the requests go one at a time through an ephemeral `URLSession` (no cookies, no cache) with `Add-Padding: true`, straight to `api.pwnedpasswords.com`. The switch is in Settings, on by default.
  - **WASM** has every call for M4.10 (`startListDevices`, `startRenameDevice`, `startRevokeDevice`, `healthReport`, `breachCheck`, `authorizeExport`, `exportFile`, `breachRangeUrl`). The background uses none yet.
  - **The UI test** renames this device from Settings and sees the new name come back from the server. Health and export are covered by view-model tests and by `ServerTests` against the real server, with a fake HIBP.

- **D44 — The extension's health report, devices and export: a full page, the leak check's permission, and fixing passwords from the report** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says "Health report, device list and export in the extension (D43), with the WASM calls from M5.10". The calls exist and are tested (M5.10). Open:
  - **Where the screens go:** the popup is about 360 pixels wide and closes when it loses focus, which is wrong for a list of problems or a file save.
  - **The leak check's permission:** the background must reach `api.pwnedpasswords.com`, and the manifest's required hosts list only our server.
  - **Whether the leak check is on by default,** as on iOS (D22 §5).
  - **What "Change" does,** since the popup does not edit items (D34).
  - **How export works** with a service worker that can stop at any time.

  Proposal:
  1. **A full extension page, `manage.html`,** opened in a tab from the popup's Settings ("Password health, devices and export").
     - **Three tabs:** Health, Devices and Export.
     - **The same look as the popup:** the same design tokens, Preact and messages to the background. It never touches the core directly.
     - **When the vault is locked,** the page shows the popup's unlock form, so it works on its own.
     - **No new required permissions.** The page is an extension page, under the existing CSP.
  2. **The leak check's permission, asked once, when it is first needed.**
     - **The request:** `chrome.permissions.request({ origins: ["https://api.pwnedpasswords.com/*"] })` (`browser.permissions` on Firefox), from the click that turns the check on or runs it.
     - **No manifest change:** the manifest's optional hosts (`https://*/*`) already cover it. Self-hosted servers are asked for the same way today (D34), so existing installs see no new warning when they update.
     - **If the user says no,** the check stays off, and the page says one plain line: "Leak check is off. Turn it on to check without sending your passwords."
  3. **On by default, as on iOS, with the D22 sentence beside the switch.** Since turning it on asks for the permission, the first time the Health tab opens it shows the switch already on and a "Check for leaks" button, and runs only after that click.
     - **No prompt** pops up by itself.
     - **The background fetches the ranges** one at a time with `Add-Padding: true`, `credentials: "omit"` and `cache: "no-store"`, straight to HIBP.
     - **Results stay in the background's memory** until the vault locks.
  4. **"Change" opens the site.** It opens the login's first website in a new tab.
     - **Then the existing flow takes over:** the extension offers to update the saved password after the user changes it there (M4.4, D36).
     - **A login with no website** shows "Change it on the site, then save it here" and no button.
     - **Editing in the popup** stays out of scope, as before.
  5. **Devices:** the list (name or "Unknown device", platform, last used, "This device"), Rename, and "Sign out" for any device but this one, after a confirmation. This device leaves with Sign out in the popup.
  6. **Export, in one background request.** The page sends the master password, the format and the export password together, and the background checks the password and exports in one go.
     - **Why:** a service worker can stop between two requests, which would drop the grant.
     - **The file** comes back as bytes to the page, which saves it with a download link from a `Blob`, as the Emergency Kit does. No `downloads` permission is needed.
     - **The rest is as on iOS:** the encrypted `.pwmx` with its own export password (entered twice, with the strength meter), or JSON/CSV after "I understand that anyone who gets this file can read all my passwords." The master password is asked every time, and export works on every plan.
     - **The page drops** the file and the passwords as soon as the download starts.
  7. **Tests:**
     - **Background (Vitest, with the fake core):** the new requests and their errors, the permission checks, and the leak check's request shape (padding header, no credentials, one request per prefix). Also: export refuses a wrong master password and keeps no grant between requests.
     - **WASM test on the real build:** health, devices and an export round trip on an unlocked client.
     - **Page:** the Playwright harness (M4.7), which already loads the test build with an account on the local sync server, opens `manage.html`, renames this device and downloads a JSON export, then checks the file.
  8. **Parity:** the extension then matches iOS on these three features, on Chromium and Firefox. Safari comes with M5.6. The gaps M5.10 logged for the extension close here.

  Alternatives:
  - **Inside the popup:** no new page, but lists are cramped, and the popup closes as soon as the save dialog or the HIBP permission prompt takes focus.
  - **A required host permission for HIBP:** no prompt, but every existing install would see a new permission warning on update, and some browsers turn the extension off until it is accepted.
  - **Off by default in the extension:** no permission request ever appears unless the user goes looking, but then most users never see leaked passwords (the reason D22 chose "on").
  - **Editing items on the page:** "Change" would be done in one place, but it is the item editor D34 left out, with its own tests. It can come with the importers (M8), which need an item view anyway.

  Implementation notes (M4.10):
  - **The background side** is a `Manager` next to the `Saver` and `Reporter` (`src/background/manage.ts`). The manage page is a second trusted sender beside the popup: the background accepts `popup.html` and `manage.html` from this extension only.
  - **The leak check:** the page asks for `https://api.pwnedpasswords.com/*` from the click on "Check for leaks"; the background checks the permission before any request. The one string is on the build's remote-URL allow-list; the range URLs come from the core.
  - **Requests to HIBP** go one at a time with `Add-Padding: true`, `credentials: "omit"`, `cache: "no-store"`, `redirect: "error"` and no referrer. Leak counts are kept in memory and dropped on lock, or when the switch is turned off.
  - **Export:** a weak `.pwmx` password is refused before the master password is checked, so the core never holds an unused grant. The file name is `PWM export <date>.<format>`.
  - **This device** is never signed out from the page (`this_device`); the core cannot tell, so the background checks the device list first.
  - **Tests:** 7 background tests with the fake core; the WASM test checks the new calls on the real build; a Playwright test (`tests/manage.spec.ts`) renames this device on the local server and downloads a JSON export. The leak check is not run against the real HIBP in CI.
  - **Not run on this machine:** the Playwright test, since Playwright's Chromium is not installed here; it runs first in the `autofill-harness` CI job.

- **D45 — Face ID / Touch ID unlock: what is wrapped, how the Secure Enclave holds it, and when the master password is still asked** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says "Face ID / Touch ID unlock via Secure Enclave-wrapped key". `SECURITY.md` says "AUK (or sym_key) wrapped by a platform key (Secure Enclave …). Never store the master password." It also says the AutoFill extension (M5.5) never runs the KDF and uses this biometric-wrapped key. Open:
  - what exactly is wrapped
  - how a Secure Enclave key wraps it, given that enclave keys are P-256 only
  - which side does what, given that all crypto lives in the core
  - how it is turned on
  - when the master password is still needed
  - what happens when Face ID changes
  - how it is tested without a device

  Proposal:
  1. **What is wrapped: the opened keyset, sealed by the core under a random 32-byte unlock key.**
     - **Core:** `export_biometric_unlock()` returns a blob and its key. The blob is `v:u8 (= 1) || account_id:16 || envelope`. The envelope holds the keyset plaintext, sealed under the key with a new AAD label, `pwm/biometric-unlock`, and fields `version || account_id || device_id`.
     - **Core:** `unlock_biometric(blob, key, access, refresh)` opens it, as `restore_session` does for the extension (D34). It refuses another account's blob, another device's blob, and a wrong key.
     - **Why the keyset, not the AUK:** unlocking then needs no Argon2. That's what the AutoFill extension's memory limit asks for (`SECURITY.md`), and it's what the extension's session already does.
     - **Vectors:** new test vectors in `core/test-vectors/biometric_unlock.json`, checked by Rust and the Node verifier, plus a fuzz case for the blob parser.
  2. **The Secure Enclave holds the unlock key's protection, not the key.**
     - **The enclave key:** at enabling, the app creates a P-256 key in the Secure Enclave. Its access control is `.privateKeyUsage` and `.biometryCurrentSet`, and its protection class is `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly`. It never leaves the enclave and isn't backed up.
     - **Wrapping:** the 32-byte unlock key is encrypted to that key's public half with Apple's `SecKeyCreateEncryptedData` (`eciesEncryptionCofactorVariableIVX963SHA256AESGCM`). Only the ciphertext is kept, in the Keychain (`ThisDeviceOnly`), next to the blob.
     - **Unlocking:** `SecKeyCreateDecryptedData` needs Face ID or Touch ID in that moment. The enclave decrypts, the core gets the key, and the Swift copy is dropped at once.
     - **The crypto rule:** this is the platform-key wrapping `SECURITY.md` names, done by the operating system's keystore. No primitive is written in Swift, and the core still does all the vault crypto. Android Keystore (M6.2) and Windows Hello (M7) will fill the same slot.
  3. **Turning it on.**
     - **Where:** a "Unlock with Face ID" switch in Settings, and one offer on the unlock screen after the first unlock with the master password ("Use Face ID next time?" with Turn on and Not now). The offer is shown once, never again after Not now, and never during autofill.
     - **Confirming:** turning it on asks for Face ID once, so a face that doesn't work is found now, not at the next unlock.
     - **When it's offered:** only on devices with Face ID or Touch ID enrolled and a passcode set. Otherwise the switch explains why it's unavailable.
  4. **When the master password is still asked:**
     - **Every 14 days:** after that, the next unlock asks for the master password once, so it isn't forgotten. This is common practice, and the Emergency Kit can't recover a forgotten password.
     - **When Face ID changes:** a new face or finger enrolled makes `.biometryCurrentSet` refuse. The app says "Face ID changed. Enter your master password", deletes the old key and blob, and turns it back on after that unlock.
     - **When Face ID fails:** if the system's attempt limit is reached or the user cancels, the master password field shows.
     - **After signing in on a device:** Face ID starts off.
     - **On each master password unlock:** the blob is made again, so it always matches the current keyset.
  5. **Turning it off, and signing out,** delete the enclave key, the wrapped key and the blob. Locking keeps them. The lock timeout (D43 §5) is unchanged: Face ID only replaces typing the password.
  6. **For the AutoFill extension (M5.5):** the same items move to the shared Keychain group with M5.5's App Group work (D42), so the extension unlocks with Face ID without the KDF.
  7. **`Info.plist`:** `NSFaceIDUsageDescription`, "To unlock your vault with Face ID instead of your master password."
  8. **Tests:**
     - **Core (tests first):** round trip; another account's or device's blob refused; wrong key refused; wrong version refused; vectors; fuzz.
     - **Swift (Linux and macOS):** the enclave sits behind a small protocol, `BiometricKeyStore` (create, wrap, unwrap, delete), with a fake that can "change biometrics", be cancelled, or be missing. The tests cover the switch, the one-time offer, the 14-day rule, Face ID changing, signing out, and a real round trip through the core.
     - **Not testable in CI:** the Secure Enclave and Face ID. The simulator has no enclave (`SecureEnclave.isAvailable` is false), so the real `SecKey` code runs first on a device. On the simulator the switch says Face ID is unavailable, and the UI test checks that.
  9. **Parity:** Touch ID on macOS comes with M5.6, from the same `PWMKit` code. Android is M6.2, Windows Hello is M7.1, and the extension unlocks through the desktop app (M5.6/M7.2, D34). These gaps are logged here.

  Alternatives:
  - **A Keychain item with a biometry access control holding the 32-byte key directly:** simpler and also enforced by the enclave. But the key would exist as a Keychain item, not under a non-exportable enclave key as the roadmap asks. The enclave-key wrap keeps the key's protection bound to hardware.
  - **Wrapping the AUK instead of the keyset:** smaller, but every unlock would then open the sealed keyset with it. That rules out the AutoFill extension, which must not run Argon2, and gains nothing, since the keyset is what an attacker wants anyway.
  - **No periodic master password:** fewer prompts, but people forget a password they never type, and nothing can reset it.
  - **Turning it on by default after onboarding:** convenient, but the user should choose to tie their vault to their face.

  Implementation notes (M5.4):
  - **Core:** `AccountKeyset::export_biometric_unlock` / `import_biometric_unlock` in `pwm-crypto`, and `Client::export_biometric_unlock` / `unlock_biometric` in `pwm-client`, through UniFFI. Anything that does not open is the new code `biometric_unlock_failed`. Vectors: `core/test-vectors/biometric_unlock.json` (Rust and Node). The `keyset_plaintext` fuzz target also opens arbitrary bytes as a biometric blob. Not in WASM: the extension unlocks with biometrics through the desktop app (M5.6/M7.2).
  - **Swift:** `BiometricKeyStore` with `SecureEnclaveKeyStore` (the enclave key, ECIES wrap and unwrap) and `NoBiometrics`. `Session` keeps the blob, the wrapped key and when the master password was last typed in the Keychain (`biometric-blob`, `biometric-key`, `password-unlocked-at`).
  - **The one-time offer** is remembered in `UserDefaults` (`biometricOffered`). A Face ID change sets the unlock screen to turn it back on after the next master password unlock, without offering.
  - **The unlock screen** tries Face ID once by itself each time it shows; a cancel leaves the password field, with no error.
  - **Not verified anywhere yet:** the Secure Enclave and Face ID themselves. The simulator has none, so CI checks only that Settings says it is unavailable. `swift test` covers the flow with a fake enclave against the real core and server. The first device run is the first real check.

- **D46 — Approving a new device from a signed-in one: the exchange, the matching code, the relay on the server, and which apps do what** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says: "the Secret Key sent over an authenticated X25519 channel (`SECURITY.md`), relay endpoints on the server, and the approving side on every signed-in platform, starting with the extension. Needs its own decision, test vectors and security review; reuses the M5.2 kit scanner." `SECURITY.md` names only the channel. Open:
  - how the channel is authenticated when the server relays it and is not trusted (zero-knowledge, S2)
  - what is sent
  - the relay's endpoints and abuse limits
  - which apps ask and which approve
  - how it is tested

  Proposal:
  1. **What it does.** A new device ("N") gets the Secret Key from a signed-in, unlocked device ("E") after the user confirms on E. N still needs the master password: approval only replaces typing the Secret Key. The server relays public keys and one ciphertext, and never sees the Secret Key.
  2. **How it is authenticated: a 6-digit code both screens show, which the user compares.**
     - **Why not a QR code alone:** the first approver is the extension, which has no camera. A matching code works between any two devices.
     - **The commitment:** without one, a server in the middle could try many keys until the two codes match. So N first sends only a commitment to its key, and reveals the key after E has sent its own. Neither side can then be steered toward a chosen code, and an attacker has a one-in-a-million chance per attempt.
     - **The scanner:** not used. D42 expected M5.9 to reuse it, but the matching code works everywhere, which keeps a single path to review.
  3. **The exchange (v1, normative, goes into `SECURITY.md`):**
     ```
     N: n_secret, n_pub   = fresh X25519 key pair
        commitment        = SHA-256(len || "pwm/approval-commit" || n_pub)
        → server: email, commitment, platform               (unauthenticated)
     E: e_secret, e_pub   = fresh X25519 key pair            (after the user picks the request)
        → server: e_pub                                      (authenticated)
     N: → server: n_pub                                      (E and N both check it against the commitment)
     both: dh   = X25519(own secret, other pub)              # all-zero result rejected
           key  = HKDF(ikm = dh, salt = approval_id || n_pub || e_pub, info = "pwm/approval/v1")
           code = HKDF(…, info = "pwm/approval-code/v1") → u32 mod 1,000,000, shown as "123 456"
     E: the user sees the same code on both screens and taps "Codes match"; then
        envelope = seal(key, secret_key_text, AAD "pwm/approval": version:u8 || approval_id || n_pub || e_pub)
        → server: envelope
     N: opens it, then logs in with the master password and that Secret Key (the normal login).
     ```
     - **Keys:** every key is fresh and used once; nothing of the account keyset is involved.
     - **If the codes differ,** E's user taps "Codes don't match", which ends the request.
     - **Vectors:** `core/test-vectors/approval.json` covers the commitment, the code, the envelope and refusals, checked by Rust and the Node verifier. A new fuzz target, `approval_messages`, covers the parsers.
  4. **The relay on the server (new migration with a migration test):**
     - **The table:** `approvals` with id, account (null for an unknown email), platform, commitment, `e_pub`, `n_pub`, envelope, state, poll-token hash, and a creation time.
     - **The endpoints:**
       - `POST /v1/approvals {email, commitment, platform}` → 201 `{id, poll_token, expires_in}`. It is unauthenticated, with the same answer for an unknown email, so it reveals nothing about accounts.
       - `GET /v1/approvals` (authenticated) lists the account's pending requests: id, platform, creation time and state.
       - `POST /v1/approvals/{id}/accept {e_pub}` (authenticated, E).
       - `POST /v1/approvals/{id}/reveal {n_pub}` (N, with `Authorization: Approval <poll_token>`). The server checks it against the commitment.
       - `POST /v1/approvals/{id}/complete {envelope}` (authenticated, E).
       - `GET /v1/approvals/{id}` (N, with its poll token) returns the state and whatever N needs next. The envelope is handed out once, then deleted.
       - `DELETE /v1/approvals/{id}` (either side) ends it.
     - **Order:** each step is accepted once and in order. A request expires 10 minutes after it is made; expired rows are deleted on read and by housekeeping.
     - **Abuse limits:** at most 3 pending requests per account, and a new rate-limit class, `approval`, of 5 requests per IP per 10 minutes.
     - **Plaintext:** the server stores the platform and times, as for devices; never names or the email beyond looking up the account.
  5. **Which apps do what:**
     - **Asking (N):** "Sign in with another device" on the iOS sign-in screen and in the extension's sign-in. N shows the code, then asks for the master password.
     - **Approving (E):** the extension's manage page and iOS Settings get "Approve a new device".
       - **Where requests show:** the extension shows a badge and a notice in the popup when a request is pending, read at sync. iOS checks when the app comes to the front. Push comes with M5.8.
       - **The screen:** each request shows its platform and how long ago it was made, then the code and the two buttons.
     - **macOS (M5.6) and Android (M6):** they get both sides with their apps.
  6. **What the approver needs:** the vault unlocked. Approving asks for nothing more, as with 1Password and Bitwarden. A person at an unlocked device could approve a stranger's device, but that device would still need the master password.
  7. **Tests:**
     - **Core (tests first):** the full exchange between two clients; a broken commitment, swapped keys, a wrong code path, a reused message and an all-zero key all refused; vectors; fuzz.
     - **Server:** every endpoint, the order of steps, the single hand-out, expiry, the 3-pending cap, the rate limit, the same answer for unknown emails, and only ciphertext and public keys stored.
     - **End to end through `pwm-client`:** one client approves another against real Postgres, and a server swapping keys is caught by mismatched codes.
     - **Extension:** background tests, and the Playwright harness approving an iOS-like request made through the core.
     - **iOS:** view-model tests, plus `ServerTests` approving between two sessions.
  8. **Security review:** this decision and its implementation get a written review before the beta (M10.3's external audit includes it). A short threat note goes into `SECURITY.md` with the exchange.

  Alternatives:
  - **A QR code shown by N and scanned by E:** strong and simple when E has a camera, but not for the extension, and two methods double what has to be reviewed. A QR option can come later on top of the same relay.
  - **E signs the envelope with the account's Ed25519 key:** N can't check that key before it has logged in, so it adds nothing over the matching code.
  - **Sending the whole keyset, so N skips the master password:** a bigger prize for anyone who fools the user, and it breaks the rule that a new device needs the master password (`SECURITY.md`).
  - **Push instead of polling:** better, but it needs M5.8's APNs work. Polling every 2 seconds for the 10 minutes a request lives is cheap.

  Implementation notes (M5.9):
  - **Crypto:** `ApprovalKeys`, `ApprovalChannel`, `approval_commitment` and `check_approval_commitment` in `pwm-crypto`; vectors in `core/test-vectors/approval.json` (Rust and Node); the `approval_messages` fuzz target.
  - **Relay:** migration `0005_approvals.sql`. The new device polls with a `pwmp_` token (a new `TokenKind`). The signed-in device denies with `POST /v1/approvals/{id}/deny`, and the new device gives up with `DELETE /v1/approvals/{id}`: the two sides authenticate differently, so they don't share one endpoint. Expired requests are deleted whenever a new one is made. Rate limits now look at the method too, so only the new device's side is limited per IP.
  - **Client:** the new device's side is `ApprovalAsk`, a step machine of its own (no account, no `Client`). The signed-in side runs through the sync engine's new `start_call`, so token refresh and `device_revoked` work as for sync. A key that fails its commitment never gets a code. New codes: `unknown_approval`, `approval_failed`; `approval_ended` on the platforms.
  - **iOS:** "Sign in with another device" on the sign-in screen; "Approve a new device" in Settings, with the count on the Settings tab, read when the app comes to the front.
  - **Extension:** asking from the popup's sign-in (the popup polls; the Secret Key stays in the background until the password is typed); approving on the manage page's Devices tab; the count on the toolbar badge and as a notice in the popup, read after each sync.
  - **The scanner** is not used, as proposed; D42's note about reusing it no longer applies.
  - **Tests:** crypto (6 + vectors + fuzz), relay (every step, order, single hand-out, cap, expiry, unknown email), the whole exchange through `pwm-client` against Postgres including a relay that swaps keys, 5 background tests, the WASM build, a Playwright test where the extension approves a request made through the core in Node, 5 Swift view-model tests and a Swift test where one session approves another against the real server.
  - **Not run on this machine:** the Playwright test (no Playwright Chromium here) and the SwiftUI screens (no Xcode); both run first in CI.

- **D47 — The iOS AutoFill extension: shared storage, unlocking without the KDF, QuickType suggestions and passkeys** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says "iOS AutoFill Credential Provider + passkeys + QuickType identity sync". `AUTOFILL.md` asks for `ASCredentialProviderViewController` and `ASCredentialIdentityStore`, kept in sync after every vault change. `SECURITY.md` says the extension never runs the KDF and uses the biometric-wrapped key (D45). D42 left the App Group and shared Keychain group for this task. Open:
  - how the app and the extension share the cache and keys, and how existing installs move over
  - how the extension unlocks
  - what the extension shows, and what it fills
  - what goes into the system's identity store, which is outside our encryption
  - how passkeys work when iOS hands over only a hash of the client data
  - how it is tested without a device

  Proposal:
  1. **A new app extension target, `PWMAutoFill`,** a credential provider built on `ASCredentialProviderViewController`, iOS 17.
     - **Its code:** it uses `PWMKit` and the core like the app, plus a small `PWMAutoFillUI` part of `PWMUI` for its screens.
     - **Entitlements, with placeholders until D1:**
       - the AutoFill credential provider capability
       - an App Group, `group.invalid.pwm`
       - a shared Keychain group, `$(AppIdentifierPrefix)invalid.pwm.shared`
     - **Signing:** simulator builds in CI keep ad-hoc signing. A device needs the Apple Developer account, as today.
  2. **Shared storage, with a one-time migration:**
     - **The cache** moves from the app's Application Support folder into the App Group container, with its `-wal` and `-shm` files. This happens once, at the first launch of the new version, before the core opens it.
     - **The Keychain items** move into the shared group: the Secret Key, tokens, email, device id, and the D45 blob and wrapped key. Each is copied first, then the old item is deleted.
     - **The Secure Enclave key:** made from now on in the shared group too. A key made before the move is turned off and made again at the next master password unlock.
     - **Migration test:** fake stores in both places, and a run that is stopped halfway then started again.
  3. **Unlocking in the extension: Face ID or Touch ID only, never the KDF.**
     - **When Face ID unlock is on:** the extension asks for Face ID and opens the D45 blob, just as the app does.
     - **When it is off, or the 14-day master password is due:** the extension says so in one line ("Turn on Face ID in PWM to fill here", or "Open PWM and enter your master password"). It never shows a master password field.
     - **After the extension closes:** nothing stays unlocked. Keys live only for one request.
  4. **What the extension shows and fills:**
     - **The list:** after Face ID, the logins that match the page or app, using the core's matching (D33) on each service identifier iOS passes, with a search over all logins below.
     - **Filling:** a tap fills, through `ASPasswordCredential`. Nothing fills without a tap.
     - **One-time codes:** after a fill, the login's TOTP code is copied with the 30-second expiry (D42), as the extension's fill does.
     - **Saving new logins:** stays in the app and the browser extension.
  5. **QuickType suggestions (`ASCredentialIdentityStore`):**
     - **What iOS gets:** each login's domains and username, plus an id (vault id and item id). iOS keeps these on the device, unencrypted by us, so the bar above the keyboard can suggest them before unlock. Passwords never go there.
     - **The switch:** "Suggest logins above the keyboard" in Settings, on by default. Turning it off clears the store.
     - **Keeping it current:** the store is replaced after every vault change in the app (save, delete, trash, sync), and emptied when signing out.
     - **Choosing a suggestion:** the extension asks for Face ID, then fills. iOS's no-interaction path always answers "interaction required".
  6. **Passkeys (iOS 17 passkey provider):**
     - **What iOS hands over:** the relying party id, user, and a hash of the client data, not the client data JSON the browser extension signs (D37).
     - **New core calls in `pwm-passkey` and `pwm-client`, through UniFFI:**
       - `passkey_register_hashed(rp_id, user_name, user_handle, client_data_hash)` stores a new ES256 passkey in a login, as D37 does, and returns the credential id and attestation object.
       - `passkey_assert_hashed(vault_id, item_id, credential_id, rp_id, client_data_hash)` returns the authenticator data and signature.
     - **The relying party id:** iOS checks it against the app's or site's associated domains before it calls a provider, so the core takes it as given. The core still refuses an id that isn't a registrable domain.
     - **Showing passkeys:** they appear in the identity store as `ASPasskeyCredentialIdentity`, and are chosen after Face ID.
     - **Vectors:** new cases in `core/test-vectors/passkey.json` for the hashed calls, checked by Rust and the Node verifier.
  7. **Turning it on:** iOS needs the user to pick PWM in Settings, under General, AutoFill & Passwords.
     - **The card:** the app shows a card in Settings explaining this, with a button that opens the right place (`ASSettingsHelper`, iOS 17). The card shows until the provider is on.
     - **No nagging:** no prompt elsewhere.
  8. **Tests:**
     - **Core (tests first):** the hashed register and assert, a signature verified with an independent ES256 check, an unknown credential refused, and vectors.
     - **Swift (Linux and macOS):**
       - the migration
       - building the identity list from items, with the switch, sign-out and every kind of change
       - the extension's model with fakes (Face ID off, master password due, matching, search, a fill, a cancel)
     - **CI** builds the extension target and runs the app's UI test as before.
     - **Not testable in CI:** iOS calling the provider, because turning it on needs the Settings app. Like the Secure Enclave, it is first checked on a device.
  9. **Parity:** the macOS credential provider (Safari and other apps) comes with M5.6, using the same code. Android is M6.3. Both gaps are logged here.

  Alternatives:
  - **A master password field in the extension:** it works without Face ID, but Argon2 at 64 MiB in a memory-limited extension risks being killed, and `SECURITY.md` rules it out.
  - **QuickType off by default:** iOS holds no usernames or domains, but the bar above the keyboard would stay empty for most users, and that bar is what makes AutoFill fast.
  - **Recomputing the client data for passkeys:** impossible, since iOS passes only the hash. The hashed calls are the documented provider interface.
  - **Leaving the cache where it is and copying it for the extension:** no migration, but two caches drift apart, and the extension would fill stale passwords.

  Implementation notes (M5.5):
  - **Core:** `register_hashed`, `register_hashed_random` and `authenticate_hashed` in `pwm-passkey`, with `checked_rp_id` for an RP id given without a page (lowercased, the IDNA form, never a public suffix or an IP address). In `pwm-client`: `passkey_register_hashed` stores the passkey as `passkey_create` does (the site's login for that username, or a new one), `passkey_assert_hashed`, and `credential_identities` for the suggestion list. All through UniFFI.
  - **The registration call takes no client data hash:** with the `none` attestation nothing signs it, so the extension passes iOS's hash straight back in the registration credential.
  - **Vectors** are in a new file, `core/test-vectors/passkey_hashed.json`, instead of new cases in `passkey.json`: the generator never rewrites a published file. Rust and `verify.mjs` check it (8 new Node checks), and the `passkey_request` fuzz target covers the hashed calls.
  - **Storage:** `StorageMigration` moves the cache files (the main file last, so a stopped run is finished by the next) and copies then deletes each Keychain item. The wrapped Face ID key is not moved: the old enclave key is deleted, the blob stays, and Face ID answers `master_password_due` until the next master password unlock makes both again in the shared group. A build signed without the App Group or Keychain group keeps the app's own places, and the extension says to open the app.
  - **Suggestions:** `Suggestions` sends the latest list one at a time after every save, trash, delete, sync with changes, new passkey and unlock, and an empty one when the switch is off or at sign-out. iOS refuses the list while PWM is not the provider, so it is sent again when Settings sees PWM turned on. The switch is kept in the App Group's defaults.
  - **The extension** opens the shared cache without a server (it never syncs; the app does, M5.8 adds background sync). New passkeys go to the first vault. Only ES256 is offered; a site that asks for none is refused.
  - **Settings:** the card "Turn on PWM in Settings, under General, AutoFill & Passwords" with a button that opens it (`ASSettingsHelper`), shown while PWM is not the provider, and the switch "Suggest logins above the keyboard".
  - **Tests:** 4 new passkey crate tests and the vectors, 5 new `pwm-client` tests, 5 migration and suggestion tests, 9 extension model and Settings tests over fakes, and a Swift test through the real core against the server (suggestions, matching, filling, a passkey made and signed, the switch, Face ID after the move, trash and sign-out).
  - **Not run on this machine:** the extension target and the SwiftUI screens (no Xcode); CI builds them with the app. iOS calling the provider needs it turned on in the Settings app, so it is first checked on a device, like the Secure Enclave.
- **D48 — The macOS app, the Safari Web Extension, and Touch ID unlock for the other browsers** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says "macOS app + Safari Web Extension; native-messaging host for other browsers' biometric unlock". Earlier decisions leave pieces here:
  - the macOS screens (D42, D43)
  - Touch ID (D45 §9)
  - the macOS credential provider (D47 §9)
  - Safari (D34 §10, D40 §5)
  - the extension's Touch ID unlock, logged as a parity gap (D34 §8)

  Open:
  - how the task is split
  - how the app is distributed and signed
  - what the Safari build can do
  - what crosses the native-messaging channel, and who may use it
  - how each part is tested without Xcode here

  Proposal:
  1. **Three tasks, one PR each, in this order:**
     - **M5.6 The macOS app:**
       - the shared screens with a sidebar
       - Touch ID unlock
       - the credential provider (passwords and passkeys in Safari and other apps), from the M5.5 code
     - **M5.11 The Safari Web Extension:** a third extension build, inside the macOS app.
     - **M5.12 Touch ID unlock for the extension in other browsers:** a native-messaging helper in the macOS app. Windows and Linux follow in M7.2, using the same protocol.
  2. **The macOS app (M5.6):**
     - **A new target, `PWMMac`, macOS 14,** in `project.yml`. It uses `PWMKit` and `PWMUI` like iOS, with a placeholder bundle id until D1.
     - **Layout:** `NavigationSplitView` with the same sections (Vault, Generator, Settings), as D42 planned.
     - **Mac-specific behaviour:**
       - **Shortcuts:** ⌘N for a new item and ⌘F for search.
       - **Locking:** when the Mac sleeps or the screen locks, plus the lock timeout.
       - **Copying:** through the pasteboard that's already there, which clears itself (D42).
     - **Signing in on a Mac:** no QR scanner. The Secret Key is typed or pasted, and "Sign in with another device" (D46) is offered.
     - **Touch ID:** the D45 code. On macOS the Secure Enclave key needs `kSecUseDataProtectionKeychain`, which gets added.
     - **The credential provider:**
       - **The same `AutoFillModel` and screens.** The controller hosts them with `NSHostingController` instead of `UIHostingController`.
       - **The same shared places (D47 §2):** the App Group and the Keychain group. There is no migration, since no Mac install exists yet.
       - **Suggestions:** they use `ASCredentialIdentityStore` as on iOS.
  3. **Distribution: Developer ID and notarization, outside the Mac App Store at first.**
     - **Why:** the M5.12 helper must write the browsers' native-messaging manifests into their folders, and an App Store app can't. 1Password and Bitwarden do the same.
     - **The sandbox:** the app stays sandboxed with the hardened runtime. The one exception it gets is write access to those manifest folders (`temporary-exception.files.home-relative-path.read-write`), which Developer ID apps may use.
     - **Before release:** CI builds ad-hoc signed. Notarization and the founder's Developer ID come at release (M10), as with the store accounts.
     - **The Safari extension** ships inside this app. Safari accepts Developer ID extensions, and during development the "Allow unsigned extensions" setting.
  4. **The Safari Web Extension (M5.11):**
     - **A third target in `build.mjs`:** `--target safari` builds `dist/safari/` from the same source, with its own manifest.
     - **The containing target:** a Safari Web Extension target in the macOS app copies that folder in at build time.
     - **What Safari can't do, like Firefox (D40 §2):** it has no `offscreen` API and no `IntersectionObserver` v2. The `capabilities` switch from D40 covers both:
       - **Background:** an event page, as on Firefox.
       - **Clipboard:** written from the background page.
       - **Inline icon and save card:** off.
       - **Fills:** in the top frame only.
       - **Passkeys:** through the page-world wrapper, if Safari allows `world: "MAIN"`; otherwise the switch says passkeys aren't available here. On a Mac, the credential provider also offers passkeys in Safari.
     - **Logged as platform gaps:** the same three as Firefox.
     - **iOS Safari:** not in M5.6. iOS fills through the credential provider (M5.5). A Safari extension on iOS can come later if users ask.
  5. **Touch ID for the extension (M5.12): the browser keeps a sealed blob, the Mac keeps its key behind Touch ID.**
     - **Turning it on:**
       - **Where:** the extension's Settings, "Unlock with Touch ID (needs the PWM app)", while unlocked.
       - **The permission:** `nativeMessaging` is optional and requested on that click (D44), so nothing changes at install.
       - **The blob:** the core makes one as D45 does. It is the keyset sealed under a fresh 32-byte key K, with a new AAD label `pwm/browser-unlock`, version 1, bound to the account and this extension's device id.
       - **What's stored where:** the blob goes to the extension's storage. K goes once to the helper, which asks for Touch ID to confirm and keeps K wrapped by a Secure Enclave key (`.biometryCurrentSet`), exactly as D45 does. K is then dropped from the browser.
     - **Unlocking:** the popup offers "Unlock with Touch ID". The helper asks for Touch ID and returns K, and the core opens the blob. The KDF never runs.
     - **The same rules as D45:**
       - the master password every 14 days, and at each sign-in
       - a changed face or finger deletes the key on the Mac
       - turning it off, or signing out, deletes the blob and asks the helper to forget K
     - **The Mac app doesn't need to be signed in, or even running.** The browser starts the helper itself. The helper only keeps keys for the extensions that enrolled.
     - **The helper:** a small app bundle inside the macOS app, `Contents/Library/LoginItems/PWM Browser Helper.app`, signed with its own entitlements (the shared Keychain group).
       - **Why a bundle:** a bare command-line tool can't carry the Keychain entitlement.
       - **Its code:** a few files in `PWMKit` (the protocol and its handler) behind a protocol, so it is tested on Linux.
     - **The manifests:** the app writes the native-messaging manifests for Chrome, Edge, Brave, Opera and Firefox only when the user turns on "Allow browser extensions to unlock with Touch ID" in its Settings. It removes them when that is turned off.
       - **Who may use it:** each manifest's `allowed_origins` (or Firefox's `allowed_extensions`) names our extension ids only. These are placeholders until the stores give the real ids at M10.
     - **The protocol, versioned:**
       - **Framing:** the native-messaging framing (a 4-byte length, then JSON), with `"v": 1`.
       - **Requests:** `enroll {account_id, device_id, key}`, `unlock {account_id, device_id}` and `forget {account_id, device_id}`.
       - **Answers:** `ok`, `key`, or an error code (`cancelled`, `changed`, `unknown`, `unavailable`).
       - **Where keys are kept:** under the calling extension's origin and the account and device ids.
       - **Checking the caller:** the helper checks the origin the browser passes as its first argument (on Firefox, the extension id) against the same list, and refuses anything else.
     - **What this guards against, and what it doesn't** (to add to `SECURITY.md`):
       - **A copied browser profile** holds only the blob: it's useless without K, which needs Touch ID on this Mac.
       - **A stolen helper key** is useless without the blob.
       - **Every unlock shows a Touch ID prompt.**
       - **Not covered:** malware running as the user that starts the helper and fakes the origin argument would still need the user to pass Touch ID for it, and also the blob. A compromised unlocked device is out of scope, as the threat model says.
     - **Safari:** it uses the same handler. Safari's native messaging goes to the extension's own `SafariWebExtensionHandler`, so there are no manifests to write.
  6. **Tests:**
     - **Rust:** the browser-unlock blob (bound to account and device, wrong key refused, versioned), plus vectors (Rust and Node).
     - **Swift on Linux and macOS:**
       - the helper's handler with fake key stores: enroll, unlock, forget, a refused origin, a cancelled or changed Touch ID, an unknown account
       - the framing
       - the manifest writer (its JSON and folders)
       - the macOS view models (shared with iOS)
     - **Vitest:** the extension's side with a fake native port: turning it on, unlocking, the 14-day rule, the helper missing, cancel, and "changed" falling back to the password.
     - **Playwright (Chromium on Linux):** a fake native host written in Node, registered for the test profile, so the extension's whole Touch ID path runs in a real browser. If Chromium under Playwright won't load a native host, Vitest covers it instead and the gap is noted.
     - **CI, the `apple` job:** builds the macOS app, its credential provider, the Safari extension target and the helper, plus a short macOS UI test (sign up, lock, unlock, sign out). That adds about 5 macOS minutes.
     - **Not testable in CI:** Safari loading the extension (Safari's driver can't drive extensions), Touch ID itself, and the browsers starting the real helper. A manual checklist goes in `QUALITY.md` for release candidates. The first real check is on a Mac.
  7. **Parity:**
     - **Windows and Linux:** the same helper protocol, in M7.2.
     - **Android and iOS:** no native messaging is needed; they fill through their own providers.
     - **The gap closes:** the extension's biometric unlock (D34 §8) on macOS with M5.12.

  Alternatives:
  - **The helper hands the app's own keyset to the extension:** no enrolment step, but it needs the Mac app signed in to the same account and unlocked. A key would also travel to a different device of the account. Rejected.
  - **The Mac App Store:** easier updates and trust, but no browser manifests. Possible later for a version without M5.12, with Safari only.
  - **One PR for all of M5.6:** fewer steps, but a very large change mixing three platforms and a new security channel.
  - **An XPC service in the running app instead of a helper bundle:** it works only while the app runs, and needs a login item anyway.

  Implementation notes (M5.6):
  - **The app:** `apps/apple/Mac` (`PWMMac`, macOS 14), one window. `MacMainView` in `PWMUI` is the sidebar (the vault's sections, then Generator and Settings, with the approvals count), the item list and the item. The other screens are the iOS ones. ⌘N opens a new login (through `VaultModel.requestNew`, so it also works from Generator, Settings or the trash), ⇧⌘L locks, and ⌘, shows Settings.
  - **Locking:** at sleep, when the screens sleep, and when the screen locks; switching to another app counts as leaving it, for the lock timeout.
  - **⌘F** is left to the system's search field in the toolbar. SwiftUI on macOS 14 has no way to focus it from code; this is not checked yet.
  - **Touch ID:** the D45 code. On macOS the Secure Enclave key and its queries use the data-protection Keychain.
  - **The credential provider:** the iOS controller's file, hosted with `NSHostingController` on macOS, in a second extension target (`PWMMacAutoFill`). Suggestions and the Settings card (`ASSettingsHelper`) work on macOS too, with Mac wording.
  - **Signing:** a macOS App Group outside the App Store starts with the team id (`$(TeamIdentifierPrefix)`). A build signed to run locally (no team, as in CI) can't carry the Keychain group, App Group or AutoFill entitlements. CI builds it with `Mac/PWMMac-CI.entitlements`, which has none: a sandboxed app without a team id is refused by the Keychain, so the CI build is not sandboxed. It can't use the data-protection Keychain either, so `SharedStorage` falls back to the login keychain there (checked with a write, since reading can answer "not found" where writing is refused). A refused Keychain call logs its status only, never a value, under the `pwm` subsystem. A build signed with the team always uses the data-protection Keychain and the shared group.
  - **Tests:** a view-model test for ⌘N. In CI, the macOS app and its extension are built, and a macOS UI test signs up, adds a login with ⌘N, locks with ⇧⌘L, unlocks and signs out. A manual checklist for release candidates is in `QUALITY.md`.
  - **Not run on this machine:** all of the macOS code (no Xcode); CI builds it first. Touch ID, the Secure Enclave and the credential provider need a signed build on a Mac.

  Implementation notes (M5.11):
  - **The build:** `safari` is a third target in `scripts/build.mjs` (`dist/safari/` and `pwm-safari-<version>.zip`, minified, `safari17`). Its manifest (`manifests/safari.json`) differs from Firefox's in three places, and the build check enforces each:
    - the background is an event page Safari wants spelled out (`persistent: false`)
    - no `idle` permission, since Safari has no `idle` API
    - `browser_specific_settings.safari` (17.0) instead of the gecko id
  - **What Safari users get:** Firefox's capability set, so the inline icon and the save card are off, and fills are in the top frame only. Server access is asked at sign-in, as on Firefox. The clipboard is written from the background page. Passkeys follow D40 §2 (the switch says so if Safari refuses the page-world script).
  - **No lock when the Mac locks** in Safari: without `idle`, the vault locks on its timer and when Safari quits. Logged as a platform gap.
  - **The Xcode target:** `PWMSafari` (`apps/apple/Safari`), embedded in the macOS app. A build step copies `extension/dist/safari/` in. Without it the target still builds and warns, so CI's macOS job doesn't need Node or the WASM build; the Linux `wasm` job builds and checks `dist/safari/` itself. `SafariWebExtensionHandler` answers no native messages yet (M5.12).
  - **Not done:** a Settings card in the Mac app that opens Safari's extension settings (`SFSafariApplication`). Safari lists the extension under Settings, Extensions once the app has run.
  - **Tests:** the build check's Safari cases (Vitest). The extension's tests already run against Firefox's capability set, which Safari shares.
  - **Not testable in CI:** Safari loading the extension; Safari's WebDriver can't drive extensions. A checklist step for release candidates is in `QUALITY.md`.

  Implementation notes (M5.12):
  - **Core:** `AccountKeyset::export_browser_unlock` / `import_browser_unlock` (the Face ID export now shares their code), `pwm-client` `export_browser_unlock` and `unlock_browser` (which also opens the sealed tokens), the code `browser_unlock_failed`, and both in WASM. Vectors in `core/test-vectors/browser_unlock.json` (Rust and Node); the `keyset_plaintext` fuzz target covers the new blob and checks the two blobs never open each other.
  - **The helper:** `BrowserUnlockHelper` in `PWMKit`, a Secure Enclave key per enrolment (caller, account, device), the wrapped keys in the shared Keychain group under the service `pwm.browser`. `PWM Browser Helper.app` sits in the app's `Contents/Library/LoginItems` and reads framed messages on stdin; Safari's `SafariWebExtensionHandler` answers the same messages, with the caller `safari`.
  - **The manifests:** `BrowserHosts` writes them for Chrome, Edge, Brave, Chromium and Firefox, only where the browser's folder exists, through the sandbox exception for those folders. Opera is listed by D48, but where Opera reads its manifests on macOS is not checked yet.
  - **The extension ids are placeholders** (a 32-letter Chromium id, the Firefox id `pwm@pwm.invalid`) until the stores give the real ones (M10). Until then, a development build's id has to be put in `BrowserUnlock.chromiumOrigins` to try it in Chrome.
  - **The extension:** `TouchIdUnlock` in the background; the popup's switch shows on Macs only. The blob is not made again at each master password unlock, unlike D45: a new one would need a new enrolment and a Touch ID prompt each time. If the keyset changes, the blob stops opening, and that turns Touch ID off.
  - **Tests:** 4 crypto and 3 client tests, the vectors and fuzz, 8 Swift tests for the helper (enrol and unlock, who may ask, separate enrolments, cancel and change, a cancelled first prompt, malformed requests, the caller from the arguments, the framing, the manifests), a Settings switch test, and 7 Vitest tests through the background with a fake helper.
  - **Not done as proposed:** the Playwright test with a fake native host. Vitest covers the extension's whole path.
  - **"Sign Out…" in the Mac menu bar** (File), which asks the same question as the Settings button. CI's Mac UI test signs out through it: scrolling the Settings form to its button does nothing there. The window now opens 600 points tall and sizes from its content's minimum, so a long Settings form no longer makes it taller than the screen.
  - **Not run on this machine:** the Swift tests (Docker was not running) and the Mac targets (no Xcode); CI runs both first. Touch ID, the browsers starting the helper and Safari's messages are first checked on a Mac.
- **D49 — Screenshots, screen recording and the app switcher on iOS and macOS** · settled 2026-11-03 · founder (approved as proposed).
  The roadmap says "Screenshot/screen-recording protection on unlock and revealed secrets". M5.2 and M5.3 left the Secret Key screen and the app-switcher cover here.

  What each platform allows:
  - **iOS:** no public way to block a screenshot. iOS reports that one was taken only after the fact. It does report screen recording, AirPlay mirroring and capture over a cable while they last (`sceneCaptureState`, iOS 17).
  - **macOS:** a window can be left out of screenshots, recordings and screen sharing (`NSWindow.sharingType = .none`). The system's own tools respect it; a capture app may not, so it is best effort.
  - **Android** has `FLAG_SECURE` (M6.4); **Windows**, `WDA_EXCLUDEFROMCAPTURE` (M7). **The browser extension** has no way at all.

  Open:
  - which screens count as secret
  - what the app does on each platform
  - whether the user can turn it off

  Proposal:
  1. **What counts as secret:**
     - a revealed hidden field (password, card number and code, other hidden fields)
     - the one-time code
     - the Secret Key screen, and the Emergency Kit preview
     - the generator's password
     - **Not counted:** the unlock screen. The master password field is always masked and shows only the email.
  2. **iOS, while the screen is recorded, mirrored or captured:**
     - **What the user sees:** every secret above is replaced with one line, "Hidden while your screen is recorded or shared." Revealing is turned off; copying still works, since nothing then shows on screen.
     - **When it ends:** the values show again only when the user taps them again.
  3. **iOS screenshots:**
     - **What happens:** iOS gives no way to stop one, so a revealed value is masked again when a screenshot is taken. One line under it says "Screenshots go to your photo library. The password is hidden again." That line shows only then, never as a prompt.
     - **No undocumented tricks:** not drawing content inside a hidden secure text field's layer, which some apps do to blank out screenshots. It relies on iOS internals, can break with any update, and breaks VoiceOver for that content.
  4. **The app switcher (iOS):** the vault is covered as soon as the app becomes inactive, not only when it reaches the background. iOS takes the switcher's picture in between, and the current cover can come too late. The Secret Key screen is covered too.
  5. **macOS:** the PWM window is left out of screenshots, recordings and screen sharing while the vault is unlocked or the Secret Key shows.
     - **The setting:** "Hide PWM from screenshots and screen sharing" in Settings, on by default.
     - **Why a switch:** someone sharing their screen for help may need to show the app. Turning it off asks nothing; the footer says what it changes.
     - **The unlock screen** stays visible, so screen sharing still shows that the app is locked.
  6. **Code shape:**
     - **The capture state:** a small `ScreenCapture` value in `PWMUI` (recorded or not, plus screenshot events), fed by the system on Apple platforms and by a fake in tests.
     - **The models decide what's hidden:** the item detail, the Secret Key screen and the generator read it. So the rules are tested on Linux, and the views only show what the models say.
  7. **Tests:**
     - **Swift (Linux and macOS):**
       - recording hides each kind of secret and turns revealing off
       - when recording ends, nothing is revealed again by itself
       - a screenshot masks again, with its line
       - the macOS switch is kept, and on by default
     - **The UI test:** checks that the switcher cover is there once the app is inactive.
     - **Not testable in CI:** real recording, mirroring and screenshots. A manual step goes in `QUALITY.md`.
  8. **Parity, logged as gaps:**
     - Android in M6.4 (`FLAG_SECURE`), Windows and Linux in M7 (Linux has no reliable API).
     - The browser extension: no API exists. The popup shows secrets only when tapped, and they mask again when it closes.

  Alternatives:
  - **The secure-text-field layer trick on iOS:** it does blank screenshots, but it's undocumented, fragile, and costs VoiceOver.
  - **Warning before each reveal while recording:** a prompt every time, which `CLAUDE.md` rules out (no nags).
  - **macOS exclusion always on, with no switch:** simpler, but it blocks honest screen sharing for support with no way out.

  Implementation notes (M5.7):
  - **`ScreenCapture`** (in `PWMUI`) holds whether the screen is captured and tells the models that registered (weakly) when capture starts or a screenshot is taken. `RootView` feeds the shared one on iOS: `sceneCaptureState` through a trait-change registration on the window's scene (no `UIApplication.shared`, so the AutoFill extension still builds), and the screenshot notification.
  - **The models:** `ItemDetailModel` hides revealed fields and the one-time code, refuses to reveal while captured, masks again after a screenshot with its line, and never reveals again by itself. `SecretKeyModel` and `GeneratorModel` hide their value while captured. Copying still works everywhere.
  - **The Emergency Kit preview:** the app shows none of its own; the kit opens in the system's share sheet, which the app can't hide. Not covered.
  - **The switcher cover (iOS):** from inactive, over the vault and the Secret Key screen, lifted when active again and the lock-timeout check is done. On macOS it still starts only in the background, since an inactive Mac window stays on screen beside others.
  - **macOS:** `WindowSharing` sets `NSWindow.sharingType` to `.none` while the vault or the Secret Key shows and the switch is on (`hideFromCapture`, on by default).
  - **Tests:** 5 new package tests on Linux (recording, screenshots, the Secret Key and the generator, dropped listeners, the macOS switch).
  - **Not done as proposed:** a UI test for the switcher cover. XCUITest has no reliable way to hold the app in the inactive state. A manual step covers it in `QUALITY.md`, with recording, mirroring, screenshots and the macOS setting.
  - **Not run on this machine:** the views and the system feeds (no Xcode); CI builds them first.
- **D50 — Background sync: syncing while locked, background refresh, push over APNs, the delete edge case, and signing in again** · settled 2026-11-04 · founder (approved as proposed).
  The roadmap's M5.8 says: pull while locked, iOS background refresh and a macOS timer, a content-free push over APNs, and the `pwm-sync` delete edge case. Earlier tasks also left these here:
  - fresh data in the AutoFill extension while the app hasn't run (D47)
  - push for device approval requests (D46)
  - signing in again after a refused refresh token, keeping unsent changes (D43)

  What the code allows today:
  - **The engine needs the keyset for only a few cases:** two versions to merge, an unsent edit to move to a new id, or a server behind the device (D28 §6, §7, §9). Most pulled changes are stored as ciphertext without being opened.
  - **Uploading needs no key:** a queued change is already sealed (D28 §3).
  - **The Apple apps keep their tokens in the Keychain,** readable after the device's first unlock. The cache file can be opened then too (D41). So a locked app can reach the server. The extension's tokens are sealed under the keyset (D34), so it can't, and doesn't need to.
  - **Signing in again** with the same device id and account keeps the cache (the core clears it only for another account), and the server accepts a known device. Only the screens are missing.

  Open:
  - how a locked sync handles a change that needs the key
  - when background sync runs on each platform
  - what the push says, who gets one, and how the server sends it
  - the delete fix
  - the sign-in-again screens

  Proposal:
  1. **Syncing while locked (core):**
     - **The call:** `pwm-client` gets a locked sync that needs only the tokens. It runs the same steps as D28 §4: the vault list, then pull, then push. It goes through UniFFI. WASM doesn't need it.
     - **The vault list:** stored as now. A new vault's wrapped key is kept as it is, and opened at unlock.
     - **Pulling:** changes that need no key are stored as ciphertext: new items, newer versions of synced items, tombstones of synced items, and an edit that beats a local delete.
     - **A change that needs the key:** both sides changed the item, a tombstone over an unsent edit, or a server behind the device. That vault's pull stops just before it, and its cursor stays there. The next unlocked sync continues from that point, and applying a change twice changes nothing (D28 §4). This needs no change to the cache format.
     - **Pushing:** queued changes go up as they are. A 412 or a 409 needs the key to merge, so that item stays queued for the unlocked sync, without counting as a failure.
     - **Tokens:** refreshed tokens go to the Keychain as now. A refused refresh token or a revoked device works as when unlocked (D28 §2, §8).
     - **What it gives:** the AutoFill extension's list and passkeys see new items at once, and a passkey made in the extension uploads without opening the app. QuickType suggestions need the decrypted names, so they still update at the next unlock (D47).
  2. **When it runs:**
     - **iOS:** a background refresh task, `pwm.sync`, asked for no sooner than 15 minutes after each run. iOS decides when it actually runs. A silent push also starts one (point 3). If the vault is unlocked at that moment, it is a normal sync.
     - **macOS:** every 15 minutes while the app runs, through `NSBackgroundActivityScheduler` (the system may defer it on battery), and on a push. The app quits when its window closes, so a closed app doesn't sync. That gap is logged; a menu-bar mode could come later.
     - **Unchanged:** at unlock, after a local change, every 5 minutes while open, and on demand (D28 §8).
  3. **Push over APNs:**
     - **The payload:** `{"aps":{"content-available":1}}`, sent as a background push at low priority. No vault id, account id or count. Apple learns only that some device token had a change, and when. That goes in the threat model.
     - **Who gets one:** after an item write or delete commits, every other device of that vault's members that has a push token. The device that made the change gets none. Each device gets at most one push a minute; APNs throttles background pushes further on its own.
     - **Approval requests (D46):** a visible notification with fixed text: "A new device wants to sign in. Open PWM to compare the code." It names no email or device. iOS and macOS show it only if the user allowed notifications. The app asks for that permission only from a switch in Settings, "Notify me when a new device asks to sign in", which is off until the user turns it on: no prompt at launch. The app still checks approvals when it comes to the front, as now.
     - **Registering:** the apps register for remote notifications at each launch; silent pushes need no permission. They send the token when it changes and after a sign-in, with `PUT /v1/devices/self/push {app: "ios" | "macos", token, environment: "sandbox" | "production"}`. `DELETE` removes it.
     - **Storage (migration 0006):** a `push_tokens` table with one row per device: app, token, environment and when it was last updated. Signing out, revoking or removing a device deletes its row. The token is never logged.
     - **The sender:** a background task in the server, fed after each commit. It speaks HTTP/2 to APNs with token auth: an ES256 JWT from the `.p8` key, signed with `p256` (already vetted, D37) and made again every 50 minutes. It uses `hyper` with `rustls` (MIT/Apache, on the allow-list).
     - **Best effort:** a push lost to a restart or an APNs error isn't retried, since background refresh and opening the app still sync. A 410 or `BadDeviceToken` deletes the token. Logs keep only counts and statuses.
     - **Settings:** `PWM_APNS_KEY_FILE`, `PWM_APNS_KEY_ID`, `PWM_APNS_TEAM_ID`, and the two bundle ids as topics. With none set, push is off and registration still works. `PWM_APNS_URL` points tests at a fake. The founder makes the APNs key in the Apple developer account; `DEPLOYMENT.md` documents it.
     - **Entitlements:** `aps-environment` in the iOS and macOS apps. CI's ad-hoc builds leave it out, so registering fails there and the app carries on without push.
  4. **The delete edge case (`pwm-sync`):**
     - **The change:** deleting an item that was never confirmed (revision 1, `Modified`) marks it `Deleted` at revision 1, instead of removing it.
     - **The next push** sends `DELETE` with `If-Match: "1"`:
       - **404:** the server never had it; it is removed locally, as now.
       - **Success:** it was ours, and it's now a tombstone.
       - **412:** another device already edited it, so the edit wins (D28 §7).
     - **A pull meanwhile** that brings back our own upload doesn't revive it: it isn't newer than the delete.
     - **The cost:** one request for each such delete, since the engine can't tell whether the upload reached the server.
  5. **Signing in again after a refused refresh token:**
     - **iOS and macOS:** a line over the vault list, "Sign in again to sync", opens a sheet that asks for the master password only. The email and Secret Key are already on the device. It runs the normal login with the same device id. The cache and its unsent changes are kept, and the next sync uploads them. A locked sync that gets the refusal keeps it, so the line shows at the next open.
     - **Face ID / Touch ID stays on:** the account and keyset haven't changed, and the master password was just typed. This counts as a master password unlock for the 14-day rule. It differs from D45 §4, which is about a sign-in on a new or wiped device.
     - **The extension:** the same line and a password field in the popup, for parity. It also has the email and Secret Key (D34).
  6. **Tests:**
     - **Core:**
       - every locked-pull case
       - stopping at a change that needs the key, then continuing after unlock
       - locked push, with 412 and 409 left queued
       - a revoked device while locked
       - the delete edge case: a lost answer, then a delete, then a pull
       - randomized multi-device runs with some locked syncs, still converging with no edit lost
     - **Server:**
       - the push endpoints
       - a write pushes to the other devices only, and at most once a minute
       - the exact payload bytes
       - an approval sends the alert
       - a 410 deletes the token
       - nothing about tokens in the logs
       - against a fake APNs over HTTP/2
     - **Swift:**
       - the background runner and the push token, with fakes
       - signing in again keeps unsent changes and Face ID
       - the line and the notification switch
     - **Extension:** signing in again keeps unsent changes.
     - **Not testable in CI:** real APNs delivery and iOS's scheduling of background refresh. They go in a manual checklist in `QUALITY.md`.
  7. **Parity, logged as gaps:**
     - **Android:** FCM and WorkManager in M6.5.
     - **Windows and Linux:** M7.
     - **The browser extension:** no push, since Web Push would add another service. It already syncs every 5 minutes while the browser runs, and when the popup opens.
     - **macOS:** syncs only while the app runs (point 2).

  Alternatives:
  - **Storing the server's versions while locked, to merge at unlock:** a vault's pull would never stop. But it needs a new cache table and migration, for a case that only arises with unsent edits or a restored server.
  - **A push service from a third party:** one more processor that sees device tokens and timing, and no less work.
  - **A silent push for approvals:** iOS delays or drops silent pushes, and a request lasts only 10 minutes.
  - **Asking for notification permission at first launch:** more people would get approval alerts, but it's a prompt nobody asked for.

  Implementation notes (M5.8):
  - **The engine:** `SyncEngine::locked` has no keyset. A pulled change that needs the key keeps its vault's cursor where it was, and the rest of the vault is still stored, so the next unlocked sync pulls from there and applies again what it already has (a no-op). A restore (new epoch) is left entirely to the unlocked sync. `SyncEngine::unlock` and `drop_keys` change an engine during a run, so unlocking during a background sync, or locking during one, carries the run on: a token refresh in flight is never lost. Locking while a restore resync is under way abandons it, since that needs the key to finish.
  - **The client:** a `Syncing` state for a locked engine with a run. `start_locked_sync` is an ordinary sync when unlocked. `start_push_token` works the same way, locked or not. `keyset()` now returns `Locked` instead of assuming an unlocked engine.
  - **The delete fix:** a delete of an item never confirmed is `Deleted` at revision 1. The existing rules already handle the answers: 404 removes it, a 412 against another device's edit keeps the edit.
  - **The server:** the sender is one Tokio task fed by a bounded queue (1024 events, more are dropped); targets are read after commit. HTTP/2 with `hyper` and `hyper-rustls` (the system's roots, as `sqlx`), the provider token signed with `p256`. A 410, `BadDeviceToken` or `DeviceTokenNotForTopic` deletes the token. A token another device held moves to the one registering it. Local sign-out on the apps removes the token through the API first; logout and revoke delete it on the server.
  - **The apps:** `AppModel.backgroundSync` reads the vault and the approval count again when they show. The push token is kept in defaults with the one last sent, so it goes up when it changes, after each sign-in, and after signing in again. On iOS the app's delegate now owns the model, so pushes and background refresh reach it before a window exists.
  - **The approval notice switch:** its state is the system's permission. Turning it on asks once; turning it off, or on after an earlier refusal, opens the system's notification settings, since an app can't take the permission back.
  - **Tests:** 10 locked-sync engine tests, the randomized test with locked runs, 2 delete tests, 10 client tests, 4 push unit tests (one over HTTP/2 to a fake APNs), 6 server push tests against a fake APNs, an end-to-end client test (locked sync, the push token, signing in again keeping an unsent edit), 8 Swift view-model tests, a Swift test against the real server, and 4 extension tests.
  - **Not done as proposed:** none.
  - **Not run on this machine:** the server's database tests and the Swift tests (no Postgres or Docker running), and the app targets (no Xcode). CI runs them first.
- **D51 — Importers: formats, mapping, encrypted exports, KeePass, limits, and the import flow** · settled 2026-11-04 · founder (approved as proposed).
  The roadmap's M8 lists the formats:
  - Bitwarden JSON, including its encrypted export
  - 1Password `.1pux`
  - Keeper JSON
  - NordPass CSV
  - Proton Pass ZIP/JSON
  - Dashlane CSV/JSON
  - KeePass `.kdbx` 3.1 and 4
  - LastPass CSV
  - a generic CSV with a column mapper

  It also asks for importing in memory, an offer to delete the source export, fuzzing, and synthetic fixtures. `PRODUCT.md` promises "direct encrypted importers for 8 competitors, no plaintext CSV left on disk". D24 left importing our own export into an existing account to M8. `core/pwm-import` exists but is empty.

  Open:
  - how the task is split
  - what an importer produces, and how foreign fields map to D18 items
  - which encrypted exports are opened, and with which crates
  - how KeePass is read
  - the limits against hostile files
  - duplicates
  - what each app shows, and what "delete the source" can honestly mean

  Proposal:
  1. **Split into four tasks, one PR each:**
     - **M8.1:** the crate and every format except KeePass.
     - **M8.4 (new):** KeePass. It has its own crypto, KDF limits and fixtures.
     - **M8.2:** the import screens in the extension, iOS and macOS.
     - **M8.3:** fuzzing. It ships with each parser, and is ticked when both parser tasks are done.
  2. **What an importer produces (`pwm-import`, no I/O):**
     - **Formats:** `detect(file name, bytes)` suggests a format from the content, and the name only as a hint. The user can always pick another.
     - **Parsing:** `parse(format, bytes, secrets)` returns a `Parsed` list: D18 items (login, note, card, identity, folder), and a report.
     - **The report:** counts per type, and what was left out, each with a count and a plain reason. Examples: attachments, SSH keys, aliases, unknown types, an item too large to sync.
     - **Never:** an item is never half-imported silently.
     - **Inputs:** `Secret<Vec<u8>>`, zeroized after parsing. Values are `Secret` strings until they become items. Nothing is logged.
     - **Saving:** `pwm-client` saves the list into a chosen vault (default the first) with new ids. Folders are remapped and nested folders kept. Then it syncs as usual. Through UniFFI and WASM.
  3. **Mapping, the same rules for every format:**
     - **Title, notes, favorite, folder and tags:** taken as they are. Empty titles become the URL's host, or "Untitled".
     - **URLs:** each URL is kept. A source's match mode is mapped where one exists (Bitwarden's six map one to one onto `AUTOFILL.md`'s); otherwise the default.
     - **One-time codes:** they go through `normalize_totp` (D23). One that is refused becomes a hidden custom field, so it isn't lost.
     - **Fields with no D18 home** become custom fields of the matching kind (text, hidden, url, date). Examples: security questions, PINs, a second username, SSN, passport.
     - **Password history:** imported as D19 history where the source has it (Bitwarden, 1Password, KeePass), newest first, at most 10.
     - **Trashed or archived items:** they come in trashed (D19), so they purge 30 days after the import.
     - **Passkeys:** imported when the export includes the private key and it is ES256 (Bitwarden, Proton Pass). Otherwise they are counted as left out.
     - **Items over the sync size limit (D29):** history is trimmed first, as when saving; then left out and reported.
  4. **The formats:** implemented from each vendor's published format notes and sample exports we write ourselves. No code from GPL projects.
     - **Bitwarden JSON:** unencrypted, and password-protected encrypted (PBKDF2-SHA256 or Argon2id, then AES-256-CBC with HMAC-SHA256). The "account restricted" encrypted export can't be opened without the Bitwarden account key, so it is refused with a line saying to export as password-protected or unencrypted. Types: login, note, card, identity. SSH keys are reported.
     - **1Password `.1pux`:** the ZIP's `export.data`. Logins, passwords, notes, cards and identities. Other categories become notes holding their fields. Documents and attachments are reported. Archived items come in trashed.
     - **Keeper JSON:** records with login, password, URL, notes and custom fields. Its one-time code key and its payment-card and address fields map; the rest become custom fields. Shared and nested folders become folders.
     - **NordPass CSV:** its `type` column chooses login, card, identity, note or folder.
     - **Proton Pass:** the unencrypted ZIP's `data.json`. Each vault becomes a folder, since we import into one vault. Aliases are reported. The PGP-encrypted export is refused with a line saying to export without encryption, because opening it would mean adding OpenPGP. Its CSV goes through the generic CSV.
     - **Dashlane:** the CSV ZIP (credentials, secure notes, payments, IDs, personal info) and the older JSON export.
     - **LastPass CSV:** secure notes (`http://sn`) become notes. A structured note whose `NoteType` is a credit card or an address maps to a card or an identity.
     - **Generic CSV:** the core reads the header and suggests a column for each field (title, URL, username, password, notes, one-time code, folder, favorite, tags). The user can change any. Browser exports (Chrome, Edge, Brave, Opera, Firefox, Safari) are matched by their headers and need no mapping.
     - **Our own `.pwmx` and JSON (D24):** opened with `pwm-export`'s readers, with new ids, so an export goes into any account.
  5. **KeePass (M8.4):**
     - **Read with the `keepass` crate (MIT).** It handles KDBX 3.1 and 4: AES-256, ChaCha20 and Twofish; AES-KDF and Argon2d/id; key files; the inner stream; gzip. It uses RustCrypto primitives underneath. Writing a KDBX reader of our own is a lot of security-sensitive parsing for no gain.
     - **Our own header check before handing a file to it:** KDF bounds (Argon2 at most 1 GiB and 100 iterations, AES-KDF at most 100 million rounds) and a 50 MB file limit, so a crafted file can't stall or exhaust a device.
     - **Mapping:** groups become nested folders, and the recycle bin comes in trashed. Title, username, password, URL and notes map. The `otp` and `TimeOtp-*` fields become the one-time code. Other strings become custom fields (protected ones hidden). Entry history becomes D19 history. Attachments are reported.
     - **Fixtures:** KDBX 4 fixtures made in the tests with the same crate's writer. KDBX 3.1 fixtures, which it can't write, are tiny synthetic files you make once with KeePass or KeePassXC (a file made with a GPL app isn't GPL), or made with `kdbxweb` (MIT) if you approve that one npm download.
  6. **New crates (to the allow-list check, `cargo deny`):**
     - **Archives:** `zip` with only `deflate` (MIT).
     - **Bitwarden's cipher:** RustCrypto `aes`, `cbc` and `pbkdf2` (MIT or Apache-2.0), beside the vetted `hmac`, `hkdf`, `sha2` and `argon2`. They are used only to open Bitwarden's format, never for our own.
     - **KeePass:** `keepass` and what it brings in (all MIT or Apache-2.0), checked when M8.4 adds it.
     - **CSV:** the `csv-core` reader `pwm-export` already uses.
  7. **Limits against hostile files:**
     - **Size:** files at most 50 MB.
     - **ZIPs:** at most 10,000 entries and 200 MB unpacked. Only the expected entry is read.
     - **Structure:** JSON nesting and string lengths are bounded, and at most 50,000 items per import.
     - **XML:** never expands entities.
     - **Errors:** each refusal has its own code (`import_too_large`, `import_unreadable`, `import_wrong_password`, `import_unsupported`), and the message never contains file content.
  8. **Duplicates:** an incoming item identical to one already in the vault is skipped and counted as "already in your vault". Identical means the same type, title, username, password and URLs. Importing the same file twice therefore adds nothing. Anything that differs is imported.
  9. **The import screens (M8.2):**
     - **Where:**
       - the extension's manage page
       - iOS and macOS Settings
       - Android (M6) and desktop (M7) later, logged as gaps
     - **Steps:**
       1. Pick a file. It is read into memory and never copied.
       2. Check the detected format.
       3. Enter the export's password or key file if it has one.
       4. See what will be imported and left out.
       5. Import.
     - **No master password** is asked: importing only adds items, unlike export.
     - **After an import from a plaintext file**, one line says the file holds passwords in plain text, with a button to delete it.
       - **iOS and macOS** delete the picked file outright (it doesn't go to the Trash).
       - **The extension** can't delete files, so it says to delete it.
     - **Honest wording:** we don't claim a "secure" delete. On flash storage overwriting a file doesn't guarantee its old blocks are gone. The docs say so.
  10. **Tests:**
      - **Per format, hand-written synthetic fixtures** in `core/pwm-import/fixtures/`, covering every item type, odd and missing fields, Unicode, and trashed items.
      - **Tests on those fixtures:** one per mapping rule above; every refusal; the limits (a ZIP bomb, a huge file, deep JSON); duplicates.
      - **Bitwarden's encrypted export:** a vector made by an independent implementation (Node's `crypto`: PBKDF2, HKDF, AES-CBC, HMAC) and checked by Rust and Node. The Argon2id variant uses the vetted `argon2` with RFC 9106 parameters.
      - **Our own export:** a round trip (export, then import into another account) giving the same items with new ids.
      - **Fuzz targets** per family (CSV, JSON, ZIP, Bitwarden encrypted, KDBX header and parse).

  Alternatives:
  - **Our own KDBX reader:** no new dependency tree, but thousands of lines of crypto-adjacent parsing to write and audit.
  - **CSV only for every source:** simpler, but it loses folders, history, one-time codes and passkeys, and leaves plaintext on disk, which `PRODUCT.md` promises to avoid where the source offers better.
  - **Asking for the master password before importing:** consistent with export, but importing reveals nothing new, so it would be friction without protection.
  - **Opening Proton's PGP export:** possible with an OpenPGP crate, but that is a large dependency for one vendor's optional encryption.

  Implementation notes (M8.1):
  - **The crate:** one module per format, with shared helpers for the D51 §3 rules (empty titles, URLs, one-time codes through `normalize_totp`, custom fields, password history, the trash, nested folders and the report). JSON documents and CSV tables are wiped when dropped. ZIP entries are read up to the unpacked limit, so a ZIP bomb stops there.
  - **Formats as built:**
    - **Bitwarden:** folders named `Parent/Child` nest. Linked custom fields are skipped (they only point at another field). A GUID or `b64.` credential id both work for passkeys.
    - **1Password and Proton Pass:** with more than one vault, each becomes a folder; with one, items stay at the top. D51 said each Proton vault becomes a folder; one folder holding everything added nothing.
    - **Proton Pass passkeys:** left out and counted, not imported as D51 proposed, because Proton doesn't document their content format.
    - **Keeper:** folder paths use `\`, shared folders first. `$paymentCard`, `$address` and the cardholder text map; other typed fields keep their label.
    - **Our own CSV export** is detected by its exact header and read with `pwm-export`'s reader.
  - **Duplicates** are checked by the client against the target vault: logins by title, username, password and URLs; cards by title and number; identities by title, name and email; notes by title and text. Repeats within one file count too. Folders with the same name and parent are reused instead of made twice, so a second import of the same file adds nothing at all.
  - **Too large:** an item over the sync limit even without history is left out when saving and counted as `too_large`; items in a folder that couldn't be saved stay at the top.
  - **New crates:** `aes`, `cbc`, `pbkdf2`, `zip` (deflate through `flate2`'s pure-Rust backend). `cargo deny` passes.
  - **The vector:** `core/test-vectors/bitwarden_export.mjs` makes `bitwarden_export.json` with Node's crypto; `verify.mjs` checks it in Node and `pwm-import` reads it. The Argon2id variant is tested in Rust with the vetted `argon2`, since Node 22 has no Argon2.
  - **Not done as proposed:** Proton passkeys (above).
  - **Not run on this machine:** the fuzz targets (libFuzzer doesn't build on Windows); CI runs them.

  Implementation notes (M8.2):
  - **Extension:** an Import tab on the manage page; the background's `Importer` passes the file to the core as bytes and drops them after. The page's limit is 32 MB, lower than the core's 50 MB, because the file travels as base64 in one extension message (64 MiB in Chrome). The preview lives in the core and goes when the vault locks.
  - **iOS and macOS:** `ImportModel` in `PWMUI` with an `ImportFiles` seam (security-scoped reading and deleting), and `ImportView` in Settings, next to Export. The file's size is checked before it is read.
  - **What counts as plain text:** a file imported without a password. That covers every unencrypted export, our JSON and CSV included; Bitwarden's protected export and `.pwmx` don't get the delete line.
  - **The Mac sandbox** gets `com.apple.security.files.user-selected.read-write`, so the picked file can be read and then deleted. CI's ad-hoc build still runs with no entitlements.
  - **After importing,** the apps sync at once; a failed sync leaves the items queued like any change.
  - **Not run on this machine:** the Swift tests and app targets (no Xcode, Docker not running); CI runs them first.

  Implementation notes (M8.4):
  - **The crate:** `keepass` 0.17 (MIT), with no features in the app and `save_kdbx4` in the tests only. It also reads KeePass 1, which our header check refuses (`import_keepass_kdb`): D51 covers 3.1 and 4.
  - **The header check** reads the outer header's fields without decrypting anything: KDBX 3.1's AES-KDF rounds, and KDBX 4's KDF parameter dictionary (its id, rounds, memory, iterations, lanes). Over the bounds it says `import_keepass_too_costly`, with a line to lower them in KeePass; an unknown KDF or version says `import_keepass_version`.
  - **Mapping as built:** an entry with no username, password, URL or other fields becomes a note. KeePass 2's TOTP fields are read when the secret is in base32 (with its length, period and algorithm); the other encodings stay as hidden fields. Protected fields are hidden. Entries with nothing in them are counted as empty.
  - **The key file** is passed as bytes (at most 1 MB from the screens); a file with a key file counts as protected, so no delete line shows after it.
  - **Fuzzing:** `import_files` reaches the KeePass header check (no password, so no key derivation); a target past it would run Argon2 with up to 1 GiB per input, so there is none.
  - **KDBX 3.1 fixtures:** the crate can't write 3.1, so the test reads any `.kdbx` in `core/pwm-import/fixtures/keepass/`, made once in KeePass or KeePassXC as its README says, and skips while there are none.
  - **Not run on this machine:** the Swift tests and app targets; CI runs them first.

- **D52 — The Android app: split, project layout, the core as native libraries, the Kotlin layer, storage, tokens, dependencies and tests** · settled 2026-11-04 · founder (approved as proposed).
  The roadmap's M6.1 says "Compose app over UniFFI core, same screens as M5.3". On Apple that took five tasks (M5.1 package, M5.2 onboarding, M5.3 vault screens, M5.9 approvals, M5.10 health, devices and export), plus M8.2's import. The core's API and its Kotlin bindings already exist; CI compiles a Kotlin smoke test against them.

  What this machine has: Android Studio with JDK 21, the SDK (platforms 35 and 36), the NDK (28) and an emulator. Gradle and the Android and Maven libraries are downloaded on first build.

  Open:
  - how M6.1 is split
  - the project layout and minimum Android version
  - how the Rust library reaches Android
  - what the Kotlin layer adds over the generated bindings
  - where the cache and keys live
  - design tokens, dependencies, and tests

  Proposal:
  1. **Split M6.1 like M5,** one PR each:
     - **M6.1:** the project, the core as native libraries, the Kotlin layer, storage, tokens, and the tests and CI job. A launchable app with a placeholder screen.
     - **M6.6 (new):** onboarding, as M5.2: create an account, the Emergency Kit, sign in, and asking another device to approve (M5.9's new-device side).
     - **M6.7 (new):** the vault screens, as M5.3: list, search, sections, detail, edit every type, the generator, Settings, the lock timeout.
     - **M6.8 (new):** health, devices, approving a new device, export, import, signing in again: M5.9, M5.10, D50 §5 and M8.2.
     - **Unchanged:** M6.2 to M6.5.
  2. **One Gradle project, `apps/android`, with three modules:**
     - **`:core`:** the generated UniFFI Kotlin file and the native libraries. Generated, never edited.
     - **`:kit`:** the Kotlin layer apps use (§4), in plain Kotlin and coroutines, with no Android UI. It holds the view models too, so they run on the JVM in tests, as `PWMUI`'s do on Linux.
     - **`:app`:** the Compose screens, the Activity, and the platform pieces (Keystore, autofill later).
     - **Versions:**
       - minimum Android 9 (API 28), the first with BiometricPrompt; compile and target API 36
       - Kotlin 2 with Compose, through the Compose BOM; the Gradle wrapper pinned
       - one ABI set: `arm64-v8a`, `armeabi-v7a`, `x86_64` (emulators)
     - **Passkeys:** Credential Manager providers need Android 14, so on 9 to 13 only passwords autofill. M6.3 logs it.
  3. **The core as native libraries:**
     - **The build:** `just android` builds `pwm-ffi` for the three ABIs with `cargo-ndk` (MIT or Apache) and the installed NDK, then generates the Kotlin file. Both go to `apps/android/core/build/` (not committed). Gradle runs it before compiling `:core`, so one command builds the whole app.
     - **JNA:** the UniFFI Kotlin bindings call the library through JNA (Apache-2.0, dual-licensed with LGPL; we take Apache), its Android AAR.
     - **Release:** the workspace's release profile, stripped.
  4. **What `:kit` adds,** as `PWMKit` does on Apple, so screens never touch steps, requests or JSON:
     - **`Session`:** a class around one `PwmClient`, its methods `suspend` functions on one dispatcher, so calls never overlap. It drives the D28/D31 step loop over HTTP.
     - **HTTP:** `HttpsURLConnection`, with no cookies and no cache, and TLS left to the system. No HTTP library.
     - **Items:** Kotlin types for the D18 JSON, keeping unknown fields by merging into the raw JSON on save, as on Apple.
     - **Errors:** a `PwmException` carrying the core's stable codes, with the same plain messages as the Apple app (one shared list, §6).
     - **Secrets:** a `Redacted<T>` whose `toString` is `<redacted>`. Master passwords and revealed values stay in local values and go straight to the core. Logging uses codes and counts only.
  5. **Storage:**
     - **The cache:** `PwmClient.open(path)` in the app's private files, which Android encrypts at rest. It holds ciphertext only (D20).
     - **The device's keys** (Secret Key, tokens, device id): an AES-256-GCM key in the Android Keystore, made for this app and never exportable, seals them in a private file. The platform's cipher is used only to protect them on disk, as the Keychain does on Apple; no crypto of ours runs in Kotlin.
     - **Behind a `KeyStore` interface,** with an in-memory fake for tests.
     - **Biometric unlock** is M6.2.
  6. **Shared copy and design tokens:**
     - **Tokens:** `scripts/tokens-kotlin.mjs` turns `design/tokens.json` into a committed Kotlin file. `just check` fails when it's stale, as for Swift.
     - **Error messages and UI copy:** the Swift list of messages moves to `design/messages.json`, generated into both apps, so they say the same thing.
  7. **Dependencies,** all permissive and on the allow-list:
     - **Libraries:** AndroidX and Compose (Apache-2.0), Kotlin and coroutines (Apache-2.0), JNA (Apache-2.0), JUnit 4 (EPL-1.0, tests only, never shipped).
     - **The check:** a licence report plugin (Apache-2.0) in Gradle checks every shipped dependency against the allow-list. `just audit` runs it.
     - **Never:** no analytics, crash or ad SDKs.
  8. **Tests and CI:**
     - **This machine:** builds the app and runs the JVM tests and the emulator.
     - **JVM tests of `:kit`:**
       - **Against a real server:** the Session and view models, with the Linux and Windows build of `pwm-ffi` through JNA, as the Swift package does on Linux.
       - **Covered:** sign-up, sign-in on a second session, save, sync, item JSON round trips, error codes, and `Redacted`.
     - **A new `android` CI job** on PRs touching `apps/android` or `core/`: `just android`, the JVM tests, a debug build, and Compose UI tests on an emulator (KVM on the Linux runner).
     - **Not testable in CI:** a real device's Keystore hardware. Its manual check goes in `QUALITY.md`.
  9. **Downloads:** the first build fetches Gradle, the Android and Maven libraries and `cargo-ndk`, from Gradle's, Google's and Maven Central's servers and crates.io. The founder approves that here, once, instead of per command.

  Alternatives:
  - **Kotlin Multiplatform sharing code with iOS:** the screens are already native per platform, and the core is the shared part; KMP would add a second shared layer.
  - **OkHttp:** a common HTTP client, but `HttpsURLConnection` does what the step loop needs without a dependency.
  - **Jetpack Security's `EncryptedSharedPreferences`:** deprecated by Google; the Keystore directly is the supported path.
  - **Minimum Android 8 (API 26),** the first with autofill: about 2% more devices, but no BiometricPrompt and more code paths.

  Implementation notes (M6.1):
  - **Versions:** Gradle 9.3.1, the Android Gradle Plugin 9.1 (with its built-in Kotlin) and Kotlin 2.4; the Compose BOM 2024.12.01; JNA 5.17.0. Espresso is pinned to 3.7.0 in the tests: the older one Compose's test library pulls in fails on Android 16.
  - **`:core` and `:kit` are plain Kotlin/JVM modules,** so the JVM tests load the host build of the core (`pwm_ffi.dll`, `.so` or `.dylib`) through JNA's jar, while `:app` ships JNA's Android AAR. Both compile against JNA as `compileOnly`, so the app never gets two copies.
  - **The step loop** runs each `Session` call on its own single-thread dispatcher; a cancelled coroutine calls the core's `cancel`. A transport `IOException` becomes `networkFailed`.
  - **`KeystoreKeys`** seals each value with AES-256-GCM under one Keystore key (`pwm.device-keys`, made on first use, never exportable), with the key's name as associated data, in `noBackupFilesDir/keys/`. Writes go through a temporary file and a rename.
  - **The licence check** is the `com.github.jk1.dependency-license-report` plugin (Apache-2.0) on the release runtime classpath, with `apps/android/allowed-licenses.json`. It runs in `just android` and the `android` CI job rather than `just audit`, because it needs the Android SDK, which the `audit` job doesn't have.
  - **The messages:** `design/messages.json` was taken from the Apple app's list. The Apple app keeps its own list in `PWMError.swift` for now: M8's open PRs add messages there, and moving it now would conflict. It moves once they merge, and a check then keeps both the same.
    - **Moved after M8 merged:** `scripts/messages.mjs` (replacing `messages-kotlin.mjs`) writes both `PWMKit/Messages.swift` and `:kit`'s `Messages.kt`, and `just check` fails when either is stale. The file gained M8's 11 import messages.
    - **One app's own names:** where the Apple wording names Apple's system (Face ID or Touch ID, the keychain, Files or Finder: 4 messages), `messages` holds wording for any platform and `platforms.apple` keeps the Apple app's text. The Apple app shows exactly what it showed before.
  - **Not done here:** the server test of `:kit` (no Postgres running); the `android` CI job runs it.

- **D53 — Android onboarding: screens and view models, the Emergency Kit, scanning it, asking another device, and the UI test** · settled 2026-11-04 · founder (approved as proposed).
  M6.6 (D52 §1) is the Android version of M5.2 plus M5.9's new-device side: create an account, the Emergency Kit, sign in with the Secret Key, and asking a signed-in device to approve. The core already does the hard parts (`password_strength`, `emergency_kit`, `scan_kit`, `ApprovalAsk`), and `:kit`'s `Session` signs up, logs in and unlocks (M6.1). Open:
  - how screens are structured and tested
  - how the kit is saved, printed and copied on Android
  - how the kit's QR code is scanned, and with which library
  - what happens if the app is killed before the kit is saved
  - the device name, the server choice and plain HTTP to a local server
  - how the UI test reaches the test server from the emulator

  Proposal:
  1. **View models in `:kit`, screens in `:app`,** as `PWMUI` and its views on Apple (D42 §3):
     - **View models:** plain Kotlin classes exposing `StateFlow`, with no Android imports, so the JVM tests run them. One per screen, as on Apple: `CreateAccountModel`, `SecretKeyModel`, `SignInModel`, `UnlockModel`, plus an `AppModel` that holds the current screen.
     - **Navigation:** `AppModel`'s screen state drives one `Activity`, with no navigation library. The system back gesture goes through Compose's `BackHandler` to the model.
     - **Lifetime:** an `Application` subclass owns the `AppModel`, so rotating the phone keeps the screen and what was typed. Master passwords live only in the view models' state, never in saved instance state.
     - **Screens:** Compose with Material 3, styled from the design tokens. Copy in `strings.xml`, so translation can follow.
  2. **The screens,** as on Apple (D42 §4):
     - **Welcome:** "Create account" and "Sign in"; the server is the hosted one, or "Other server".
     - **Create account:**
       - Fields: email, master password, and the password again.
       - The strength meter. Continue needs a score of at least 3.
       - The line "Nobody, including us, can reset your master password."
     - **Save your Secret Key** (shown once): the key in large monospaced text, with:
       - **"Save Emergency Kit":** the system's "Save to" picker (`ACTION_CREATE_DOCUMENT`), so the user picks Downloads, a drive or a USB stick. The PDF bytes are written straight to the chosen file, with no temporary copy.
       - **"Print":** Android's print dialog, fed the same bytes, again with no temporary file.
       - **"Copy":** marked sensitive, so Android 13+ hides it in the clipboard preview, and cleared after 30 seconds.
       - **"I saved it":** enabled after a save, a print or a copy, as on Apple and in the extension.
       - **Paper:** A4, or Letter in the same regions as the Apple app.
     - **Sign in:**
       - Fields: email, master password and Secret Key.
       - **"Scan Emergency Kit"** (§3) fills the email, the Secret Key and a self-hosted server, never the master password.
       - **"Ask another device"** (D46): shows the matching code and waits, with Cancel. It replaces typing the Secret Key.
     - **Unlock** on later launches: the master password, with "Sign out" beside it. Fingerprint and face unlock come with M6.2.
     - **After sign-in, a placeholder vault screen:** the item count, Lock and Sign out. The vault is M6.7.
  3. **Scanning the kit: CameraX and ZXing,** both Apache-2.0:
     - **CameraX** (`camera-core`, `camera-camera2`, `camera-lifecycle`, `camera-view`) shows the camera and hands frames to the decoder.
     - **ZXing's `core` jar** decodes QR codes only, on the device, with no network. The text goes to the core's `scan_kit`, which accepts only `pwm-kit:1?…`.
     - **The camera permission** is asked when the user taps "Scan Emergency Kit", never earlier. If it's refused, the screen says how to allow it, and typing still works.
     - **Not Google's ML Kit or its code scanner:** their terms are Google's own, not a licence on the allow-list, and the code scanner needs Google Play services.
  4. **If the app is killed before the kit is saved:** the account exists, but the user may never have seen the Secret Key.
     - **A "kit pending" flag** is set at sign-up and cleared by "I saved it". While it's set, the app opens on the Secret Key screen after unlocking, so the kit can still be saved.
     - **The Apple app doesn't do this.** It's a small change there too, and it is logged here as a gap, to add with M5's next touch.
  5. **The details:**
     - **The device name** sent at sign-in: the phone's model (`Build.MODEL`, such as "Pixel 8"), which needs no permission. Rename comes with devices in M6.8.
     - **The server choice** is kept in the app's private preferences (it's not a secret). A test-only instrumentation argument sets it, as `-server` does on Apple.
     - **Plain HTTP:** only to `localhost` and `127.0.0.1`, as the core already enforces, through a network security config. Everything else is HTTPS.
     - **Screenshots:** the Secret Key screen sets `FLAG_SECURE` now, since it's one line and that screen shows the most sensitive value. The rest is M6.4.
  6. **Tests:**
     - **JVM tests of the view models** (tests first), with a fake account as on Apple:
       - the strength gate and the "I saved it" gate
       - a scanned kit filling the right fields and never the password
       - the kit-pending flag surviving a restart
       - asking another device: the code shown, cancel, and an ended request
       - error messages
     - **Against a real server** (`PWM_TEST_SERVER`, as in M6.1): sign up, read the kit, sign out, sign in again with the Secret Key.
     - **The scanner's decoding:** a JVM test that draws the core's kit QR text as an image with ZXing's encoder (tests only), decodes it with the app's decoder and checks the result.
     - **A Compose UI test on the CI emulator:**
       - The flow: create an account, see the Secret Key, copy it, continue to the vault screen, sign out, sign in again with the Secret Key, lock and unlock.
       - **Reaching the server:** `adb reverse tcp:8080 tcp:8080` makes the emulator's `localhost:8080` the runner's, so the plain-HTTP rule stays as it is.
     - **Manual, in `QUALITY.md`:** scanning a printed kit with a real camera, and the save and print pickers.
  7. **Downloads:** the first build fetches CameraX and ZXing from Google's and Maven Central's servers, as D52 §9 did for the rest. The founder approves that here.

  Alternatives:
  - **Jetpack Navigation for Compose:** a standard library, but with one Activity and a handful of screens, a state in the model is simpler and is the same shape as the Apple app.
  - **AndroidX `ViewModel`s in `:app`:** survive rotation for free, but tie the logic to Android, so the JVM tests couldn't run it.
  - **Google's code scanner:** no camera permission and no camera code, but it's under Google's own terms and needs Play services, so it fails on phones without them.
  - **zxing-android-embedded** (Apache-2.0): a ready-made scanner screen, but it uses the old camera API and is in maintenance mode.
  - **Share the kit through the share sheet:** familiar, but it needs a temporary file and a `FileProvider`, so the kit would sit on disk outside the user's choice.

  Implementation notes (M6.6):
  - **State:** each model exposes one `StateFlow` of an immutable state, with passwords and Secret Keys as `Redacted`, so a state never prints them. Its work runs in the app's scope (`MainScope` in `PwmApplication`), so turning the phone doesn't cancel a sign-up.
  - **`Account`:** the interface the models use; `Session` implements it, and the tests use a fake. `Session` adds `emergencyKit` (dated today on the device), `askForApproval` (the D46 loop, polling every 2 s; cancelling sends the core's cancel request), `secretKey` and `itemCount`.
  - **Servers are compared as text:** `java.net.URL.equals` looks the host up on the network.
  - **Versions:** CameraX 1.4.2 and ZXing `core` 3.5.3, both Apache-2.0. The licence check now covers 107 shipped libraries, all on the allow-list.
  - **The scanner** reads the camera's brightness plane, so the frame's rotation doesn't matter to the decoder; a code that isn't a kit shows its error and the camera keeps looking.
  - **Copy:** the clip is marked sensitive and cleared after 30 s. In the background Android hides the clipboard from apps, so the app can't check whether something else was copied since; it clears then anyway.
  - **Print** marks the kit as saved once the print system has the PDF, which includes its preview, as the Apple app marks it when the share sheet opens.
  - **Plain HTTP to `[::1]`:** the core accepts it, but a network security config can't name an IPv6 address, so Android refuses it. `localhost` and `127.0.0.1` work.
  - **A new message,** `kit_save_failed`, is in `design/messages.json` for both apps.
  - **Run on this machine:** the JVM tests, the build and licence check, and all 4 device tests on an Android 16 emulator, the full flow included, against a local server on a throwaway Postgres. Not run: a real camera, and the save and print pickers (the `QUALITY.md` checklist).

- **D54 — Android vault screens: layout, moving between screens, items in Kotlin, the lock timeout, the scanner for one-time codes, and third-party notices** · settled 2026-11-04 · founder (approved as proposed).
  M6.7 (D52 §1) is the Android version of M5.3: the vault list, search, sections, detail, editing every type, the generator, Settings and the lock timeout. D43 settled what these screens do for every platform, and the core already has every call they need: listing, saving, trash and purge, one-time codes, the generator, and checking a one-time-code setup (`normalize_totp`). Health, devices and export are M6.8. Open, for Android only:
  - the layout on phones and tablets, and how screens follow each other
  - which Kotlin pieces are missing
  - how the lock timeout works on Android
  - scanning a one-time-code QR code
  - showing the third-party notices

  Proposal:
  1. **The screens do what D43 §2-4 says,** with no change in behavior:
     - **Sections:** All items, Favorites, Logins, Cards, Identities, Notes, each folder, and Trash.
     - **Search:** D43 §3's rule, in the view model, with no index. 5,000 items filter without a visible delay.
     - **Detail:** hidden values masked until tapped, Copy on every field through the 30-second clipboard (D53), the one-time code with its countdown, passkeys read-only, the password history.
     - **Edit and new item:**
       - every D18 field for each type
       - a folder picker that can make a new folder
       - tags as text
       - a one-time-code setup the core checks
       - Generate next to the password
       - a "discard changes?" question when leaving with edits
       - items from a newer app version read-only
     - **Trash:** Restore, and Delete forever after asking. Old trash is purged after each sync.
     - **The generator tab:** the extension's options, the strength meter, Copy and Regenerate, kept in memory only.
     - **Settings:**
       - the email and server
       - the lock timeout
       - Lock now
       - Save Emergency Kit again (the same Save and Print as onboarding)
       - Sign out, after asking
       - Third-party notices
       - the app version
     - **No website icons:** fetching them would tell other sites what's in the vault.
     - **Pull to refresh syncs,** and so does every save.
  2. **The layout:**
     - **Phones:** a bottom bar with Vault, Generator and Settings (Material 3's navigation bar).
     - **Tablets and wide windows** (600 dp and up): the same three as a side rail, with the list and the open item side by side, as on the iPad.
     - **No new library:** both come from Material 3, which the app already uses.
  3. **Moving between screens,** as in M6.6, with no navigation library:
     - **A stack in `VaultModel`:** the list, then an item, then its edit screen. The back gesture pops it, through the edit screen's discard question.
     - **Each tab keeps its own place** when the user switches tabs.
     - **Turning the phone** keeps the stack and what was typed, because the models live in the app (D53 §1).
  4. **Kotlin pieces in `:kit`,** ported from the Apple app's `PWMKit`:
     - **`Item` accessors** for cards, identities, custom fields, folders, tags, favorites, the one-time code, passkeys and the password history. They edit the D18 JSON in place, so fields a newer app wrote are kept (D43 §4).
     - **`Session` gains** `item`, `setTrashed`, `delete`, `purgeTrash`, `currentCode`, `normalizeTotp` and the generator, all calling the core.
     - **The view models** in `pwm.kit.ui`: `VaultModel` (sections, search, the stack), `ItemDetailModel`, `ItemEditModel`, `GeneratorModel` and `SettingsModel`. They replace M6.6's placeholder `VaultModel`.
  5. **The lock timeout on Android:**
     - **The choices:** as D43 §5: Immediately, 1, 5, 15 or 60 minutes, or 8 hours. The default is 15 minutes. It's kept in the app's private preferences.
     - **What the time counts:** the time the app spends in the background.
       - **Background and foreground** are the whole app's, from `ProcessLifecycleOwner` (AndroidX `lifecycle-process`, Apache-2.0), so a picker or the print dialog doesn't count as leaving.
       - **The clock:** `SystemClock.elapsedRealtime`, which keeps running while the phone sleeps and ignores changes to the wall clock.
       - **On return after the timeout,** the app locks before it shows anything.
     - **Turning the screen off locks too,** whatever the setting, as the device lock does on Apple. This uses Android's screen-off broadcast while the app is running.
     - **If Android ends the app,** the keys go with the process, and the next launch asks to unlock, as now.
     - **Fingerprint and face unlock** (M6.2) will shorten this; the timeout rules stay.
  6. **Scanning a one-time-code QR code:** the M6.6 kit scanner becomes a general `QrScanner`, used for both. A code from a site goes to the core's `normalize_totp`, like a pasted one. The camera permission is asked on tap, as in M6.6.
  7. **Third-party notices** (S6 requires them in every app):
     - **What's bundled:**
       - the Rust crates' notices, `THIRD_PARTY_NOTICES/rust.html`, and the word-list and site-data notices, when `just notices` has made them, as on Apple
       - the Android libraries' notices, from the licence report the build already makes, written into the app as plain text
     - **The screen:** a Settings page. It shows the HTML in a `WebView` with JavaScript off and no network, loading only the app's own files.
     - **Without the files,** the page says so. The release pipeline (M10.1) runs `just notices` first, as on Apple.
  8. **Screenshots:** the Secret Key screen keeps `FLAG_SECURE` (M6.6). The detail and edit screens get it in M6.4, with the app-switcher preview.
  9. **Tests:**
     - **JVM view-model tests** (tests first), with the fake account:
       - search and order, including 5,000 generated items
       - the section filters and Trash
       - editing each type, with unknown fields kept
       - the discard question and read-only newer items
       - the folder picker making a folder
       - the generator's options
       - the lock timeout, with an injected clock
       - the back stack
     - **`Item` accessors** against D18 JSON, with fields from a newer version kept.
     - **The server test** adds a trash and restore round trip, and a one-time code.
     - **A Compose UI test on the emulator,** extending M6.6's flow:
       - add a login with a generated password
       - find it by search, edit it, copy the password
       - trash it and restore it
       - set Immediately, send the app to the background, and see the unlock screen
  10. **Downloads:** the first build fetches `lifecycle-process` from Google's servers. The founder approves that here.

  Alternatives:
  - **Jetpack Navigation:** standard, but the stack in the model is what M6.6 does, and the JVM tests can drive it.
  - **The adaptive list-detail library** (`material3-adaptive`): it gives the tablet layout for free, but it's another library, and a check on the window width does the same.
  - **Counting idle time in the foreground:** as D43 said, the phone's own screen lock covers it.
  - **Notices from Google's `oss-licenses` plugin:** it builds the list automatically, but it's tied to Play services and covers only the Android libraries, not the Rust ones.

  Implementation notes (M6.7):
  - **The app's own system screens:** Save to and Print belong to the system, so opening them puts the whole app in the background. The app marks when it opens one: that trip doesn't count as leaving, even with Immediately, unless it lasts over 10 minutes. Without this, Immediately would lock mid-save and drop the picked file. The screen turning off still locks.
  - **Locking** hides the vault at once (its screens close, the kit leaves memory), then locks the core. Unsaved edits are lost, as on Apple.
  - **The list's order** uses `java.text.Collator` for the device's language, at secondary strength (case aside), then the id.
  - **One edit model per screen:** the form holds text, and secrets (password, card number and code, one-time-code setup, other fields' values) as `Redacted`. Saving applies it to the item's JSON in place; opening and leaving an item unchanged isn't an edit (empty and absent fields count as the same).
  - **A sync notice** (couldn't sync, some changes not sent, signed out) is a code the screen words; a revoked device signs out.
  - **Notices:** the licence report also writes `android-libraries.txt`. A Gradle task copies it, with `rust.html` and the data notices when `just notices` made them, into the app's `assets/notices/`. `THIRD_PARTY_NOTICES/` is gitignored, so CI builds carry the Android list only.
  - **Versions:** `lifecycle-process` 2.8.7 (Apache-2.0). The licence check still covers 107 libraries: it was already a dependency of another library.
  - **Icons:** Material's core set only. The extended set is large, and no item type needs a site's logo.
  - **Run on this machine:** the JVM tests and the server test, the build and licence check, and all 5 device tests on an Android 16 emulator, the full vault flow included, against a local server on a throwaway Postgres. Not run: a real phone, a tablet, and a real camera (the `QUALITY.md` checklist).

- **D55 — Fingerprint and face unlock on Android: the Keystore key, the prompt, and what D45 carries over** · settled 2026-11-04 · founder (approved as proposed).
  M6.2 says "BiometricPrompt + Keystore-wrapped key". D45 settled biometric unlock for every platform: the core seals the opened keyset under a random 32-byte unlock key (`export_biometric_unlock`, `unlock_biometric`, already in UniFFI), and the platform's keystore protects that key. Open, for Android:
  - which Keystore key protects the unlock key
  - which biometrics count, and the prompt
  - re-making the blob after a master password unlock without asking for a finger each time
  - how it's tested, given that emulators in CI have no fingerprint enrolled

  Proposal:
  1. **The Keystore key: RSA-3072 with OAEP**, made in the Android Keystore when unlock is turned on, never exportable and not backed up. It's the same slot as the Secure Enclave key on Apple (D45 §2).
     - **Only the private half needs a finger or face:** each use needs a fresh strong biometric (Android 11+: per-use with `AUTH_BIOMETRIC_STRONG`; Android 9 and 10: the equivalent per-use setting).
     - **Wrapping needs no prompt:** the 32-byte unlock key is encrypted to the public half with `RSA/ECB/OAEPWithSHA-256AndMGF1Padding`, done by the platform. Only the ciphertext is kept.
     - **A new finger or face makes the key unusable:** it's invalidated by new enrollments, and Android then refuses it.
     - **Why not AES in the Keystore:** an AES key that needs a biometric needs one to encrypt too. The blob is re-made after every master password unlock (D45 §4), so each of those would also ask for a finger. A public key wraps without a prompt, as Apple's enclave key does.
     - **Why not elliptic curves:** the Android Keystore can't encrypt with them.
     - **The crypto rule:** this is the platform-key wrapping `SECURITY.md` names, done by the operating system. No primitive is written in Kotlin; the core still does all the vault crypto.
  2. **Which biometrics count: Android's "strong" class only** (Class 3): most fingerprint sensors, and face unlock on phones with secure face hardware.
     - **Not the weak class** (camera-only face unlock on many phones): the Keystore can't bind a key to it.
     - **Not the screen lock PIN:** as on Apple, only a biometric replaces the master password.
     - **The prompt:** AndroidX Biometric's `BiometricPrompt`, with the private key's cipher as its crypto object, so the Keystore opens the key only for that touch. Its title is "Unlock your vault", and its button is "Use master password".
  3. **What carries over from D45 unchanged:**
     - **Turning it on:**
       - a switch in Settings, and one offer after a master password unlock ("Use fingerprint or face unlock next time?")
       - turning it on asks for a finger or face once, so one that doesn't work is found now
     - **The master password** is still asked every 14 days, after a biometric change (then it turns back on by itself), and after signing in.
     - **After each master password unlock,** the blob is made again.
     - **Turning it off and signing out** delete the key, the wrapped key and the blob; locking keeps them.
     - **The unlock screen** tries the prompt once by itself, and a cancel leaves the password field with no error.
     - **The lock timeout (D54 §5) is unchanged:** a biometric only replaces typing the password.
  4. **Where things are kept:** the blob, the wrapped key, and when the master password was last typed, as three more names in the Keystore-sealed key files (D52 §5). The offer's "shown once" is in the app's preferences.
  5. **The Activity** becomes a `FragmentActivity`, which `BiometricPrompt` needs. Compose works the same on it.
  6. **When it isn't available,** Settings says why:
     - no sensor of the strong class
     - none enrolled
     - no screen lock set
  7. **Tests:**
     - **JVM view-model tests** with a fake key store (as D45 §8's `BiometricKeyStore`): the switch, the one-time offer, the 14-day rule with an injected clock, a biometric change, a cancel, and sign-out.
     - **The server test:** a real round trip through the core. Turn it on, lock, unlock with the fake key store, and check that another device's blob is refused.
     - **On the CI emulator,** which has no finger enrolled: Settings says unlock is unavailable, and the UI test checks that.
     - **On a real phone (`QUALITY.md`):**
       - turning it on, unlocking, and cancelling
       - enrolling a new finger: the next unlock asks for the master password and turns it back on
       - removing the screen lock
  8. **Downloads:** the first build fetches AndroidX Biometric (`androidx.biometric:biometric` 1.1.0, Apache-2.0, with `androidx.fragment`). The founder approves that here.
  9. **Parity:** the autofill service (M6.3) will unlock with the same key, without the KDF, as D47 does on Apple.

  Alternatives:
  - **AES in the Keystore:** simpler, but every master password unlock would also ask for a finger, to wrap the new blob (§1).
  - **StrongBox:** a separate secure chip on some phones. Its RSA keys are 2048-bit only, and many phones don't have one. The Keystore's trusted environment is hardware-backed on every certified phone.
  - **Allowing the screen lock PIN as well:** more devices could use it, but the PIN would then open the vault, and it's often 4 digits.

  Implementation notes (M6.2):
  - **OAEP's parameters:** SHA-256 for the message digest, with MGF1 over SHA-1, the MGF1 digest the Android Keystore takes on every version. The public half is used from a software copy of its encoded key, as Android recommends, so wrapping never asks.
  - **The prompt** is `BiometricPrompt` from AndroidX Biometric 1.1.0 (with `androidx.fragment`; the licence check covers 108 libraries, all allowed). It uses the Activity on screen, which the app tracks; in the background it's `biometric_cancelled`. Every prompt error (the master password button, a cancel, too many tries) is `biometric_cancelled`, so the password field shows.
  - **A sign-in starts with it off,** and `logIn` and `signUp` count as typing the master password for the 14-day rule.
  - **New messages:** `biometric_not_enrolled` and `biometric_no_screen_lock` in `design/messages.json`.
  - **Run on this machine:**
    - the JVM tests, the server test's round trip, the build and licence check, and the 5 device tests on an Android 16 emulator
    - **the real key and prompt, by hand on the emulator:** with a PIN set and a fingerprint enrolled through the emulator's simulated sensor, turning it on and unlocking with a finger both worked. The PIN and fingerprint were removed afterwards, so later runs match CI.
  - **Not run:** a real phone, and a new enrollment invalidating the key (the `QUALITY.md` checklist).

- **D56 — Screenshots, screen recording and the app switcher on Android** · settled 2026-11-04 · founder (approved as proposed).
  M6.4 says "FLAG_SECURE on sensitive screens". D49 settled what counts as secret and logged Android's `FLAG_SECURE` as the way to protect it. That flag works per window: while it's set, screenshots and recordings show the window black, screen sharing and casting show it black, and the app switcher shows no picture of it. Android can't apply it to part of a screen. M6.6 already sets it on the Secret Key screen. Open:
  - which screens get it, given it's all or nothing for the window
  - the app-switcher preview
  - whether the user can turn it off, as on the Mac (D49 §5)

  Proposal:
  1. **The whole window is secure while the vault is unlocked, and on the Secret Key screen.**
     - **Covers:** every tab, item and edit screen, the generator, and Settings.
     - **Why the whole vault, not only revealed values:** the flag can't cover part of a window. Setting it only while a value shows would leave the list, the one-time codes and the generator's password visible.
     - **Not covered:** welcome, create account, sign in and unlock. Their passwords are always masked and only the email shows, as D49 §1 says of the unlock screen, so screen sharing still shows that the app is locked.
  2. **A switch, as on the Mac:** "Hide PWM from screenshots and screen sharing" in Settings, on by default.
     - **Turning it off** asks nothing; its line says what it changes.
     - **Why a switch:** someone sharing their screen for help may need to show the app (D49 §5).
     - **The Secret Key screen stays secure** whatever the switch says: it shows before Settings exists, and its key is shown once.
     - **Kept in the app's preferences,** not synced.
  3. **The app switcher:** on Android 13 and later, the vault never shows in the switcher, even with the switch off (`setRecentsScreenshotEnabled(false)`, as the iOS cover does, D49 §4). On Android 9 to 12 only `FLAG_SECURE` hides it, so the switch turns that off too; Settings says so.
  4. **Where it's decided:** `AppModel` says whether the window should be secure, from the screen and the switch. The Activity applies it, in one place, and the Secret Key screen's own flag from M6.6 goes. So the rule is tested on the JVM.
  5. **Tests:**
     - **JVM:**
       - secure on the vault and the Secret Key screen
       - not secure on welcome, sign in and unlock
       - the switch's effect, and that it never applies to the Secret Key screen
       - it's kept, and on by default
     - **On the emulator:** the window's flags on the welcome and vault screens, and after turning the switch off. The UI test checks this.
     - **By hand (`QUALITY.md`):** a screenshot and a screen recording come out black on the vault, casting shows black, and the switcher shows no picture.
  6. **No new libraries.**

  Alternatives:
  - **Always on, no switch:** simpler, but honest screen sharing for support would be impossible, which D49 avoided on the Mac.
  - **Secure only while a value is revealed:** less in the way, but the flag can't cover part of a window, so codes, the generator and the list would show.
  - **Watching for screenshots instead** (Android 14's capture callback): it only reports after the fact, like iOS. `FLAG_SECURE` stops them.

  Implementation notes (M6.4):
  - **`AppModel.State`** has `secureWindow` and `hideFromRecents`, worked out from the screen and the switch. `MainActivity` applies them whenever they change. The Secret Key screen no longer sets the flag itself.
  - **The switch** is `hideFromCapture` in the app's preferences, on by default. Settings words its line for Android 13+ or older, since only 13+ keeps the switcher covered with the switch off.
  - **Run on this machine:** the JVM tests, the build, and the 4 device tests on an Android 16 emulator against a local server. Not run: a real phone's screenshots, recording and casting (the `QUALITY.md` checklist).

- **D57 — Background sync on Android, and whether push comes now** · settled 2026-11-04 · founder (approved as proposed).
  M6.5 says "Background sync and push (FCM) for Android, the same as M5.8 (WorkManager for periodic refresh)". D50 settled the shared parts: the core's locked sync (`start_locked_sync`, already in UniFFI), and a content-free push that only wakes the device. On Apple, push goes through APNs, which the operating system provides. On Android, push means Firebase Cloud Messaging (FCM), and that brings things the rest of the plan has avoided:
  - **Licences:** the FCM client needs Google Play services libraries (`play-services-basement`, `-base`, `-tasks`, `-cloud-messaging`). Their Maven files name the Android SDK licence, Google's own terms, which isn't on the allow-list (S6). The licence check would refuse them.
  - **Phones without Play services** (some Huawei phones, GrapheneOS and other privacy-minded setups) get no push at all.
  - **The founder's side:** a Firebase project and its config file in the app, plus a service account for the server.
  - **The server's side:** FCM's HTTP v1 API takes an OAuth token signed with RS256. The RustCrypto `rsa` crate has an open timing advisory for signing (RUSTSEC-2023-0071, the Marvin attack), and `ring` and `aws-lc-rs` aren't on the vetted list. So the server would need a new crypto dependency and its own decision.
  - **What push adds on Android before M6.3 and M6.8:** little. With no autofill service yet, fresh data in the background only saves a sync at the next unlock. Approval alerts (D46) come with M6.8's approving screen.

  Open: whether push is part of M6.5, and how background sync runs.

  Proposal:
  1. **M6.5 does background sync now, and push waits for its own decision.**
     - **The split:** push becomes a new task, **M6.9**, after M6.3 and M6.8, when there's an autofill service and an approving screen for it to serve.
     - **M6.9 then needs a decision on:** Play services' licence (an allow-list exception, or UnifiedPush below), the Firebase project, and the server's RS256 signer.
     - **Until then,** it's logged as a parity gap.
  2. **Background sync with WorkManager** (AndroidX `work-runtime`, Apache-2.0):
     - **What runs:** a periodic job every 15 minutes, the shortest Android allows, only with a network connection. Android may run it later, in Doze or to save battery, as iOS does with background refresh.
     - **What it does:** the core's locked sync with the stored tokens, so it needs no master password. As D50 §1 says, it stores what needs no key, uploads queued changes, and leaves the rest for the next unlocked sync. If the vault is unlocked at that moment, it's a normal sync.
     - **When it doesn't run:** with nobody signed in, the job is cancelled.
     - **A refused refresh token** is kept, so the vault shows "Sign in again to sync" at the next open (D50 §5). The sign-in-again screen itself is M6.8.
     - **A revoked device** is signed out, as when unlocked.
     - **Unchanged:** syncing at unlock, after each save, and with pull to refresh.
  3. **Storage:** none new. The tokens are already in the Keystore-sealed key files, readable without a fingerprint, and the cache is a normal file.
  4. **Tests:**
     - **JVM:**
       - a locked sync through `Session` against the real server: an item from another device arrives while locked, and a queued change uploads
       - the scheduler's rules with a fake: scheduled at sign-in, cancelled at sign-out
     - **On the emulator:** WorkManager's test driver runs the job once, and the server sees the sync.
     - **By hand (`QUALITY.md`):** leave a real phone locked for an hour, make a change on another device, and see it on unlock without waiting.
  5. **Downloads:** the first build fetches AndroidX `work-runtime` and its dependencies. The founder approves that here.

  Alternatives:
  - **FCM now, as the roadmap says:** push matches the Apple apps sooner. But it needs everything listed above: an allow-list exception for Google's libraries, a Firebase project, a new crypto dependency on the server, and no push on phones without Play services. All of that for little gain before M6.3 and M6.8.
  - **UnifiedPush:** an open protocol (its Android library is Apache-2.0), where the user picks a "distributor" app, such as ntfy, that delivers pushes. There's no Google dependency, and it works without Play services. But most users have no distributor installed, so it needs FCM beside it to reach everyone. The server would also need Web Push's encryption (P-256, HKDF, AES-GCM) and VAPID signing. Worth weighing in M6.9, as an addition to FCM.
  - **No background sync at all:** simpler, but changes from other devices wait for the next unlock, and queued changes wait for the app to open.

  Implementation notes (M6.5):
  - **The job:** `SyncWorker`, a `CoroutineWorker`, through WorkManager 2.10.0 as unique periodic work (`pwm.sync`, kept if already there). Every run reports success: an offline or failed run waits for the next period, rather than WorkManager's backoff.
  - **A run with the app's process gone** starts it: `PwmApplication` builds the model and opens the account as at launch, and the run uses it while the screen stays on unlock.
  - **Versions:** `work-runtime-ktx` 2.10.0 (Apache-2.0), which brings Room and SQLite for its own queue; the licence check covers 118 libraries, all allowed. `work-testing` is in the device tests only.
  - **Run on this machine:** the JVM tests, the server test's locked sync against a local server, the build and licence check, and the 4 device tests on an Android 16 emulator. Not run: a real phone's scheduling over hours (the `QUALITY.md` checklist).

- **D58 — Android autofill: the split, which callers to trust, matching apps, unlocking, and what is filled** · settled 2026-11-04 · founder (approved as proposed).
  M6.3 says "AutofillService + Credential Manager provider (passwords + passkeys), inline suggestions". `AUTOFILL.md` asks for inline suggestions on keyboards that support them, with the dataset picker as a fallback. These are two separate Android systems:
  - **The autofill service** (Android 8+) fills fields in apps and browsers.
  - **The Credential Manager provider** (Android 14+) answers apps' and browsers' requests for passwords and passkeys.

  The core already classifies fields (`classify_fields`) and matches web pages (`autofill_matches`, D33). It can't match a native app yet. Open:
  - how to cut the task
  - which callers' web domains to believe
  - how native apps are matched
  - how a locked vault fills
  - what is filled, and what isn't yet

  Proposal:
  1. **Split M6.3, as D52 split M6.1:**
     - **M6.3:** the autofill service for passwords, in apps and browsers, with inline suggestions and the dropdown.
     - **M6.10 (new): the Credential Manager provider** for passwords and passkeys on Android 14+. It needs its own decision:
       - the passkey request format for apps, where the provider builds the client data with an `android:apk-key-hash` origin
       - checking an app against the site's Digital Asset Links file
       - the browsers allowed to pass a client data hash
     - **Until then:** passkeys on Android are a logged gap.
  2. **The service:** `PwmAutofillService`, which the user turns on in the system's autofill settings.
     - **The card:** Settings shows "Turn on PWM for autofill" with a button that opens the system's choice (`ACTION_REQUEST_SET_AUTOFILL_SERVICE`), until it's on. No prompt anywhere else (D47 §7).
     - **What it reads from the screen:** each field's autofill hints, input type, resource id, hint text, and, for web pages, the HTML tag and attributes Android passes. These map to the core's field description and go to `classify_fields`. Only username, email, current password, new password and one-time code fields are used.
     - **Never read:** field values. Nothing about the screen is logged.
  3. **Which web domains to believe:** any app can claim a web domain in what it hands the service, so a fake "bank.example" would get the bank's password.
     - **Trusted:** the domain only from a list of known browsers, each a package name with its signing certificate's SHA-256: Chrome, Firefox, Edge, Brave, Samsung Internet, DuckDuckGo and Vivaldi. The list is in the app, and the signed rules file (`AUTOFILL.md`) can extend it.
     - **Everything else** is treated as the app it is (§4), including web pages shown inside other apps.
     - **Browsers that need Android's compatibility mode** are left out. That mode reads the screen through accessibility, which is fragile. A browser that doesn't pass web domains gets nothing.
  4. **Matching native apps (new in the core):**
     - **The link:** a login's website list can hold `android-app://<package>?sha256=<certificate digest>`.
     - **New core rule** (`autofill_app_matches`), tested first: the package must be equal, and so must the digest of the app's signing certificate, which the platform reads from Android. Linking the certificate means an app reinstalled from elsewhere under the same name doesn't get the login.
     - **Making a link:** PWM writes one when the user picks a login from search for an app ("Use for this app" saves the link, with the certificate).
     - **Links without a digest** (`androidapp://…` from other managers' exports) are offered only from search, never as a suggestion.
     - **No guessing from package names** (`com.bank.app` → `bank.com`): a wrong guess fills the wrong app.
     - **Digital Asset Links** (an app proving it belongs to a site) waits for M6.10, which needs it anyway.
  5. **When the vault is unlocked** (the app's process holds it open, as the lock timeout allows):
     - the matching logins are offered by title and username, and a tap fills
     - "Search PWM" is always there, for apps not linked yet
  6. **When the vault is locked:** one entry, "Unlock PWM to fill".
     - **A tap opens PWM's fill screen:** fingerprint or face when that's on, or the master password (the service runs in the app, so the key derivation is fine here, unlike the iOS extension, D47 §3). Then the matches, with search; a tap fills and the screen closes.
     - **Nothing is stored outside the vault's encryption for this:** no list of usernames or domains for before unlocking. iOS keeps one because QuickType needs it (D47 §5); Android doesn't.
  7. **What it fills:**
     - **The username and password,** after a tap, never by itself.
     - **The one-time code:**
       - filled when a field is a one-time code with high confidence (the core's 0.8)
       - otherwise copied for 30 seconds after the fill, as the other apps do
     - **Not yet, and logged as gaps:**
       - **saving new logins** from what the user typed, as on iOS (D47 §4)
       - **cards and identities,** which need a confirmation step (`AUTOFILL.md`)
  8. **Inline suggestions** (Android 11+, keyboards that support them) through AndroidX Autofill's `InlineSuggestionUi`, with the dropdown otherwise. Both show the same entries. There's no "Save" or "Upgrade" entry, and nothing a page can style (`AUTOFILL.md`).
  9. **Tests:**
     - **Core (tests first):**
       - app matching: equal, another package, another certificate, a link without a digest
       - a fuzz case for the new link format
     - **JVM:**
       - the screen-to-field mapping over fake nodes
       - the browser list, and refusing a web domain from another app
       - building the entries locked and unlocked
       - "Use for this app" writing the link
       - the one-time code rule
     - **On the emulator:**
       - the service is turned on with `adb` (`settings put secure autofill_service …`)
       - a test login screen in the test app is filled through the dropdown, driven by UiAutomator
       - a second test screen claiming a web domain gets nothing
     - **By hand (`QUALITY.md`):** Chrome and Firefox with a real site, Gboard's inline suggestions, and a bank-style app.
  10. **Downloads:** the first build fetches AndroidX Autofill (`androidx.autofill:autofill`, Apache-2.0), and UiAutomator for the device tests. The founder approves that here.

  Alternatives:
  - **Everything in M6.3, passkeys included:** one task, but the Credential Manager adds a second protocol, Digital Asset Links over the network, and the browser allowlist for client data hashes. Each deserves its own review.
  - **A list of usernames and domains for before unlocking,** as iOS has: suggestions could name the login while locked. But it puts vault metadata outside our encryption on a platform that doesn't need it.
  - **Believing every caller's web domain:** more apps would fill, and so would a fake one.
  - **Guessing apps from package names,** as some managers do: convenient, and sometimes wrong in a way that hands a password to another app.

  Implementation notes (M6.3):
  - **The browsers' certificates** are their release signing certificates' SHA-256, from Google's list of browsers privileged for Credential Manager (`gpm-passkeys-privileged-apps/apps.json`), checked against the raw file.
  - **Reading another app's certificate** needs package visibility: the manifest's `<queries>` names the 7 browsers and asks for apps with a launcher icon. No `QUERY_ALL_PACKAGES`.
  - **What a match fills:** while unlocked, an entry carries the login's values and fills at once; one with a one-time code and no code field goes through PWM's fill screen without showing it, so the code is copied after the fill. Locked, the entry opens the fill screen, which unlocks with the app's unlock screen (without the fingerprint offer, D45 §3).
  - **Never into PWM itself:** a request from PWM's own screens gets nothing.
  - **The device test and keyboards:** a keyboard that takes inline suggestions (Gboard on the emulator) shows PWM's entries as chips that UiAutomator can't read. Debug builds read a switch the test sets, `pwm.test`/`dropdownOnly`, to send the dropdown instead; a release build never reads it.
  - **Found while testing, fixed here:** signing out kept the device id, so a new account signed up on the same phone was refused by the server (`device_conflict`, shown as a server error). Android now gives a sign-up a new device id, and reuses the stored id only for the account it belongs to (kept as `device-account`). The Apple app keeps the device id the same way and has the same gap; it's logged here for M5's next touch. The desktop (M7.6) and the extension (2026-11-06) follow the same rule, the extension keeping the owner as `deviceAccount` beside `deviceId`.
  - **Run on this machine:** the core tests and clippy, the JVM tests and the server test, the build and licence check (118 libraries), and the 5 device tests on an Android 16 emulator against a local server. Not run: a real phone with Chrome, Firefox and Gboard (the `QUALITY.md` checklist).

- **D59 — Android account and data screens: devices, approving, signing in again, health, export and import** · settled 2026-11-04 · founder (approved as proposed).
  M6.8 is the rest of the Apple app's Settings on Android:
  - the device list (M5.10)
  - approving a new device (M5.9, D46)
  - signing in again after a refused refresh token (D50 §5)
  - the health report (D22, D44)
  - export (D24)
  - import (D51, M8.2)

  Every core call already exists through UniFFI (`startListDevices`, `startRenameDevice`, `startRevokeDevice`, the approval calls, `healthReport`, `breachCheck`, `authorizeExport`, `exportFile`, `prepareImport` and the rest), and the Apple screens show what each does. Open, for Android:
  - how to cut it
  - how approvals reach a phone with no push yet (D57)
  - the breach check's requests
  - how export and import pick files
  - deleting an imported plain-text file

  Proposal:
  1. **Two PRs, one decision:**
     - **M6.8, the account:**
       - **Devices:** the list, rename and revoke.
       - **Approving a new device.**
       - **Signing in again.**
     - **M6.11 (new), the data:**
       - **The health report,** with the breach check.
       - **Export.**
       - **Import.**
     - **Why:** each half is about the size of an earlier M6 task, and they touch different screens.
  2. **Devices:**
     - **The list:** each device's name, platform, when it was added and last used, and "This device" on this one.
     - **Rename** any device; the name is sealed as D43 §6 says.
     - **Revoke** any other device, after asking. This one signs out instead.
  3. **Approving a new device (D46, the signed-in side):**
     - **Where:** "Approve a new device" in Settings lists open requests.
     - **Accepting:** shows the matching code, and "The codes match" sends the Secret Key sealed for the new device. Deny ends the request.
     - **No push on Android yet (D57):** the screen checks for requests every 3 seconds while it shows, and when the app opens.
       - **The new device's own screen** says where to look ("open Settings, then Approve a new device"), as it does on Apple.
       - **A notice from the background sync** wouldn't help: requests last 10 minutes and the job runs every 15. Logged as a gap until M6.9.
  4. **Signing in again (D50 §5):**
     - **When sync is refused:** the vault's notice becomes "Sign in again to sync", which asks for the master password only. The email and Secret Key are on the device.
     - **The sign-in:** the normal login, with the same device id (the D58 fix keeps it for this account). The cache and unsent changes are kept, and fingerprint unlock stays on. It counts as typing the master password for the 14-day rule.
  5. **The health report (M6.11, as D44):**
     - **The screen:** counts of weak, reused and leaked passwords, each opening its list; each item opens its edit screen, with Generate.
     - **The breach check:**
       - **The switch:** "Check for leaked passwords" in Settings, on by default, with D22's sentence.
       - **The requests:** one per 5-character hash prefix, straight to `api.pwnedpasswords.com` with the core's padding header, through `HttpURLConnection` with no caching and no cookies.
       - **Results stay in memory** for the session only.
  6. **Export (M6.11, as D24):**
     - **The format:** `.pwmx` (encrypted), JSON or CSV.
     - **The master password first;** a fingerprint doesn't count.
     - **`.pwmx`:** an export password twice, with the strength meter.
     - **JSON and CSV:** the warning that anyone with the file can read every password.
     - **Saving:** through the system's "Save to" (as the Emergency Kit, D53 §2): the bytes go straight to the file picked, with no temporary copy. The lock timeout ignores that trip (D54 notes).
     - **Every plan,** with no plan check.
  7. **Import (M6.11, as D51 and M8.2):**
     - **The file:** through the system's file picker, read once into memory. Files over 50 MB are refused before reading, as the core's limit says.
     - **The steps, as the other apps:** the core guesses the format; there's a password for encrypted exports, a key file for KeePass, and column mapping for unknown CSVs; then a preview of what will come in, and Import.
     - **Deleting the file afterwards:** for a plain-text import, "Delete the file" asks Android to delete it (`DocumentsContract.deleteDocument`). Where the place it came from can't delete (some cloud drives), the screen says to delete it by hand (`delete_failed`).
  8. **Tests:**
     - **JVM view-model tests** with the fake account, for each screen's steps:
       - the device list, rename and revoke
       - the approval steps and a denied request
       - signing in again keeping unsent changes
       - health counts
       - export's checks
       - import's steps, the delete line and a refused large file
     - **The server test,** extended inside its one account:
       - devices and rename
       - an approval end to end between its two sessions
       - signing in again
       - health and export (M6.11), with a fake breach service
     - **A device test:** rename this device and see the new name come back from the server (M6.8). Import a small CSV from a file the test writes, and export JSON to a file (M6.11).
     - **By hand (`QUALITY.md`):** approving a phone from another phone, and the pickers on a real phone.
  9. **No new libraries.**

  Alternatives:
  - **One PR for all six:** fewer steps, but too large to review well, and the two halves share nothing but Settings.
  - **A notice for approvals from the background job:** too slow for a request that lasts 10 minutes; push (M6.9) is the real answer.
  - **Copying the imported file into the app first:** simpler to read, but the plain text would sit in app storage too.

  Implementation notes (M6.8):
  - **Device conversations** go through the step loop like sync; offline is `network`, a revoked device `device_revoked`, a refused session `signed_out`.
  - **"Sign in again to sync"** replaces the old "signed out" notice on the list. It's kept in the app's preferences, set by a refused sync in the app or the background job, and cleared by signing in again, signing in, or signing out.
  - **PATCH on the JVM:** renaming a device is a `PATCH`, which the JDK's `HttpURLConnection` refuses while Android's sends it. The JVM server test uses a `java.net.http` transport (`JdkTransport`, tests only); the device test checks the app's own transport renames.
  - **Run on this machine:** the JVM tests and the server test (devices, rename, an approval end to end, signing in again) against a local server, the build, and the 5 device tests on an Android 16 emulator. Not run: approving from a real second phone (the `QUALITY.md` checklist).

  Implementation notes (M6.11):
  - **The breach check** goes through its own transport (`breachTransport`, the same `HttpURLConnection` with no cache or cookies), never the sync server's. Each prefix is asked once with the core's padding header; a refusal or a dead network shows the leak check's error and keeps the weak and reused lists. Results live in the health screen's model only.
  - **"Change"** opens the item's edit screen on the Vault tab, so saving goes through the vault as any edit. The report is read again each time the page shows, as on Apple.
  - **Export:** the master password is cleared after each try. The file waits in memory for "Save to" and goes once written, when the page closes, or when the vault locks; a failed write keeps it for another try (`export_save_failed`, a new shared message). "Save to" counts as a system screen for the lock timeout (D54 notes).
  - **Import:** the picked file is read through the content resolver, at most 50 MB whatever its size said, and kept only in the model. "Delete export file" calls `DocumentsContract.deleteDocument`; a place that can't delete says `delete_failed`. Leaving the page or locking cancels a preview.
  - **The device test** passes the file's address to the import screen and writes the export where "Save to" would, since the system's pickers aren't the app's to drive; the pickers are on the `QUALITY.md` checklist.
  - **Run on this machine:** the JVM tests (5 new), the server test (health with a fake breach service, export, a CSV import seen on the second session) against a local server, the build and licence check, `just check`, and the 5 device tests on an Android 16 emulator.

- **D60 — Push for Android: FCM, the licence exception, and the server's RS256 signer** · settled 2026-11-05 · founder (approved as proposed).
  M6.9 is D50 §3's push on Android: a content-free "your vault changed" push, and the fixed-text notice for a new device's request (D46). D57 §1 left three things open: Google Play services' licence, the Firebase project, and how the server signs for FCM. What was checked:
  - **The client's licences:** `firebase-messaging` 26.0.1 (the latest) is Apache-2.0. It needs five Play services libraries: `play-services-base` 18.9.0, `-basement` 18.9.0, `-tasks` 18.4.0, `-cloud-messaging` 17.4.0 and `-stats` 17.0.2. Each one's Maven file names the "Android Software Development Kit License", which isn't on the allow-list (S6). Every Android app that uses FCM ships them under those terms. The rest of the tree (`firebase-*`, `transport-*`, AndroidX) is to be confirmed by the licence check at build time.
  - **FCM's send API (HTTP v1):** an OAuth access token with the `firebase.messaging` scope. Without Google's libraries, the server gets one by signing a JWT with the service account's RSA key (RS256) and exchanging it at Google's token endpoint. A token that's no longer valid answers `UNREGISTERED` (404). Data-only messages take a priority (normal or high), a TTL and a collapse key.
  - **RS256 on the server:** the RustCrypto `rsa` crate still has the open timing advisory for private-key operations (RUSTSEC-2023-0071). `ring` 0.17.14 is already compiled into the server through `rustls` and `sqlx` (Apache-2.0 AND ISC, both allowed), and signs RSA with BoringSSL's constant-time code. Using it adds no new download, but it isn't on the vetted list.
  - **UnifiedPush,** the open alternative: its spec requires RFC 8291 encryption for every message, so even a content-free push needs AES-GCM on the server, a new crate. It also needs a distributor app that most users don't have. Its fallback for phones without one, the embedded FCM distributor, needs Play services anyway.
  - **Android 13+** needs the `POST_NOTIFICATIONS` permission before the app shows a notification it builds itself.

  Open: whether to take the licence exception, which signer the server uses, what the pushes carry, and when the app asks to show notifications.

  Proposal:
  1. **FCM, with a narrow licence exception:**
     - **The exception:** "Android Software Development Kit License" is allowed for exactly the five `com.google.android.gms` libraries above, by name, in `allowed-licenses.json`. Anything else under that licence still fails the check. S6's allow-list in this file gets the same line, as D21 did for the wordlist.
     - **Why it's acceptable:** the app stays closed source and these are binary libraries Google licenses for distribution in apps. No GPL-style terms apply.
     - **Nothing else from Firebase:** no Analytics, Crashlytics or the `google-services` Gradle plugin. Firebase starts from code, with its settings from Gradle properties (`pwm.firebase.projectId`, `.appId`, `.apiKey`, `.senderId`). Without them, as in CI and local builds, push is off and the app behaves as now. Auto-init and delivery-metrics export stay off.
  2. **The server signs with `ring`:**
     - **Vetted for one use:** RS256 for FCM's OAuth token, and nothing else. `CLAUDE.md`'s vetted list says so.
     - **Why not `rsa`:** the open advisory. The exposure would be small, since the server signs on its own schedule at most once every 50 minutes. But `cargo deny` would need an ignore for it, and `ring` is already in the build.
     - **The settings:** `PWM_FCM_CREDENTIALS_FILE` (the service account's JSON file, read at start and never logged), plus optional `PWM_FCM_URL` and `PWM_FCM_TOKEN_URL` for tests. Neither set: FCM is off. APNs and FCM are each on or off by themselves.
     - **The access token** is cached until 5 minutes before it expires.
  3. **What the pushes carry:**
     - **A change:** a data-only message, `{"t":"sync"}`. Normal priority, a 1-hour TTL, collapse key `sync`. At most one a minute per device, as on Apple.
     - **A new device's request:** `{"t":"approval"}`, high priority, a 10-minute TTL (the request's lifetime). It holds no email, device or code.
     - **Google learns** that some token had a change, and when, as Apple does. That goes in the threat model beside APNs.
     - **A dead token** (`UNREGISTERED`) is deleted, as APNs' 410 is.
  4. **The app:**
     - **A change:** the push starts the background sync job at once (expedited work), the same locked sync as D57.
     - **A request:** shows "A new device wants to sign in. Open PWM to compare the code." The words come from the app, not the push. Tapping it opens Settings, then Approve a new device.
     - **Asking permission:** only from a switch in Settings, "Notify me when a new device asks to sign in", off until turned on, as on Apple (D50 §3). On Android 13+ turning it on asks for `POST_NOTIFICATIONS`. If that's refused, the switch stays off and says how to allow it in system settings. No prompt at launch. The pushes still arrive with the switch off; the app just shows nothing.
     - **The notification channel:** "New devices", for these notices only.
     - **Registering:** the app sends its FCM token after sign-in and when FCM gives it a new one, through the core's existing `start_push_token`, which also works locked. Signing out removes it, as now.
  5. **The server and core, with migrations:**
     - **Migration 0007:** `push_tokens` also takes `app = 'android'`, with tokens up to 4,096 bytes; FCM tokens are longer than APNs tokens. Its migration test checks existing APNs rows still pass.
     - **The core and the route:** `start_push_token` and `PUT /v1/devices/self/push` take `"android"` with the FCM token as its text (letters, digits, `_`, `-` and `:`), always `production`.
  6. **Phones without Play services** keep D57's 15-minute background sync and get no push. That's logged as a gap; UnifiedPush can be weighed again with its encryption later.
  7. **The founder's side,** before push reaches real phones:
     - a Firebase project with the Android app registered, under the final package name (D1)
     - its four settings in the release build
     - a service account allowed to send, whose file goes to the server
     - Until then everything ships off.
  8. **Tests:**
     - **Server:** a fake FCM and token endpoint. The JWT's signature verifies with the service account's public key; the bodies are exactly the two payloads; a 404 `UNREGISTERED` deletes the token; the one-a-minute limit holds.
     - **Migration 0007's test.**
     - **Core:** Android token validation, accepted and refused.
     - **JVM:** the app's push handling with a fake. A change runs the sync. A request shows the notice only with the switch on and permission given. The switch and its refused permission. The token is registered after sign-in and on a new token.
     - **On the emulator:** a message handed to the service starts the job, and the request notice shows with the permission granted. No real FCM in CI.
     - **By hand (`QUALITY.md`):** with a real Firebase project, a change on another device wakes a locked phone within a minute, and a request shows the notice.
  9. **Downloads:** the first build fetches `firebase-messaging` 26.0.1 and its dependencies, including the five Play services libraries. The founder approves that here. `ring` is already in the lockfile.

  Alternatives:
  - **UnifiedPush only:** no Google code. But most users have no distributor app, so most phones would get no push. The server also needs RFC 8291 encryption (a new AES-GCM crate) and VAPID.
  - **UnifiedPush and FCM together:** reaches everyone, and phones without Play services too when they have a distributor. But it's both systems' work at once. Better as a later addition once FCM works.
  - **The `rsa` crate for RS256:** pure Rust and from RustCrypto. But its timing advisory is still open, and `cargo deny` would need an ignore.
  - **No push on Android:** no Google libraries at all. The 15-minute sync and the approving screen's polling already work. But approval notices never reach a phone that's in a pocket, and changes wait up to 15 minutes or longer in Doze.

  Implementation notes (M6.9):
  - **Two more libraries in the exception,** approved by the founder on 2026-11-05: the licence check found `com.google.firebase:firebase-iid-interop` 17.1.0 and `firebase-measurement-connector` 19.0.0 under the same licence. They are small interface libraries `firebase-messaging` loads at start-up. Each of the seven is allowed by name; anything else under that licence still fails. "The 3-Clause BSD License" (protobuf's name for BSD-3-Clause) is added as a name the check reads.
  - **Tokens up to 1,024 bytes,** not 4,096: the unique index on `push_tokens.token` can't hold an entry much above 2.7 KB. FCM tokens are about 150 to 250 characters. The core, the route and the table all check 100 to 1,024.
  - **The sender:** one task for both providers. A device whose provider is off on this server is skipped, not counted as failed. A 401 drops the cached access token. A 404 deletes the device's token.
  - **The app:** Firebase's own start-up provider is removed from the manifest, so nothing of Firebase starts without the four settings. Delivery-metrics export and Analytics are switched off in the manifest. A `sync` push queues one network-bound sync (`pwm.sync.now`, kept if already waiting), not expedited work, which would need a foreground notice on older Android.
  - **Signing out** removes the token from the server first (5 seconds at most), as on Apple (D50 notes). At the time no app called the server's logout at sign-out, so the session itself stayed valid until it expired; D62 (M6.12) now ends it too, and logout also drops the push token, so this step only matters when the logout doesn't reach the server.
  - **The device test** calls the model the way the messaging service does, since CI has no Firebase project; a real phone is on the `QUALITY.md` checklist.
  - **Run on this machine:** the core's tests, the server's tests (the FCM tests against fakes, migration 0007's test) against a local Postgres, the JVM tests and the server test against a local server, the build and licence check (161 libraries), and the 5 device tests on an Android 16 emulator.

- **D61 — The Credential Manager provider on Android: callers, apps' client data, Digital Asset Links, and what is offered** · settled 2026-11-05 · founder (approved as proposed).
  M6.10 is D58 §1's second half: Android 14's Credential Manager asks providers for passwords and passkeys on behalf of apps and browsers, in the system's own sheet. The core already makes and signs passkeys two ways: from an origin, for the extension (`passkey_create`, `passkey_get`, D37), and from a client data hash, for iOS (`passkey_register_hashed`, `passkey_assert_hashed`, D47 §6). What was checked in Android's provider guide and the AndroidX release notes:
  - **The service:** a `CredentialProviderService` (`androidx.credentials.provider`) answers `onBeginGetCredentialRequest`, `onBeginCreateCredentialRequest` and `onClearCredentialStateRequest`. It's declared with `BIND_CREDENTIAL_PROVIDER_SERVICE` and an XML file naming the password and passkey capabilities.
  - **Two steps:** the first answer is only entries (`PasswordCredentialEntry`, `PublicKeyCredentialEntry`, `CreateEntry`, or `AuthenticationAction` when locked), each with a pending intent. The user's choice opens our activity, which reads the full request (`PendingIntentHandler.retrieveProvider…Request`) and returns the credential.
  - **Who is asking:** `CallingAppInfo` gives the caller's package and signing certificates. `getOrigin(allowlist)` returns a web origin only when the caller matches a privileged-browser list we pass in, by package and certificate.
  - **Browsers** pass a `clientDataHash` and build the client data themselves; the provider signs the hash and puts a placeholder where the response's `clientDataJSON` goes.
  - **Apps:** the provider builds the client data, with the origin `android:apk-key-hash:<SHA-256 of the app's signing certificate>`, and must check the app against the site's Digital Asset Links file. Android doesn't check it for us.
  - **The library:** `androidx.credentials:credentials` 1.6.0 (April 2026, the latest stable).

  Open:
  - which browsers may pass a hash, and what everything else is
  - how an app's client data is built, and where
  - the Digital Asset Links check: what it reads, where it runs, and caching
  - what is offered locked and unlocked, and how a passkey is confirmed
  - passwords through the Credential Manager, including saving
  - how it's turned on, next to D58's autofill service

  Proposal:
  1. **Two kinds of caller, nothing else:**
     - **A privileged browser:** D58 §3's seven (Chrome, Firefox, Edge, Brave, Samsung Internet, DuckDuckGo, Vivaldi), each a package and its signing certificate's SHA-256, built into the allowlist we give `getOrigin`. The signed rules file can extend it, as for D58. The origin from `getOrigin` is checked by the core against the RP id as for the extension (HTTPS, a registrable suffix, the PSL).
     - **Any other app:** treated as the app it is (§3), whatever it claims. A browser that isn't on the list is an app, so it gets passkeys only for sites that list it in their Digital Asset Links file, which in practice means none.
     - **PWM itself** gets nothing, as in D58's notes.
  2. **The browser path uses the hashed calls (D47 §6) as they are:** `passkey_register_hashed` and `passkey_assert_hashed`, after the core's origin check. The response JSON is built in the core with a placeholder `clientDataJSON`, as the guide says, so the app never assembles WebAuthn structures.
  3. **The app path, new in the core** (`pwm-passkey` and `pwm-client`, through UniFFI, tests and vectors first):
     - **`passkey_create_for_app(vault_id, package, cert_sha256, asset_links, request_json)`** and **`passkey_get_for_app(package, cert_sha256, asset_links, request_json, vault_id, item_id, credential_id)`** return the `RegistrationResponseJSON` and `AuthenticationResponseJSON`.
     - **The client data:** `{"type":…,"challenge":…,"origin":"android:apk-key-hash:<base64url, no padding>","androidPackageName":<package>}`. Google's own provider includes `androidPackageName`; the exact bytes are checked against a real Google Password Manager response during the build and pinned in vectors.
     - **The RP id is checked by the Digital Asset Links file** (§4), not by an origin, which an app doesn't have. A request whose file doesn't list the app is refused before the user is asked (`passkey_app_not_linked`).
     - **The platform passes the file's text, the core decides.** Parsing and matching it is security logic, so it lives with the rest in the core; the app only fetches.
     - **A new passkey also links the login to the app:** it gets `android-app://<package>?sha256=<digest>` (D58 §4), so the password autofill and §6 find it for that app too.
  4. **Digital Asset Links:**
     - **The fetch:** `https://<rp id>/.well-known/assetlinks.json` with the app's own `HttpURLConnection`, no cookies, no cache, no redirects (the Digital Asset Links rule), a 5-second timeout and at most 256 KB.
     - **What counts:** a statement whose target is `android_app` with the caller's package name and its certificate's SHA-256 among `sha256_cert_fingerprints`, and whose relations include `delegate_permission/common.get_login_creds` or `delegate_permission/common.handle_all_urls`. Many sites list only the second (for app links), and it is the site's owner vouching for the same app either way.
     - **`include` statements** (a file pointing at another site's file) are followed one level deep, HTTPS only, under the same limits.
     - **Caching:** results stay in memory for an hour, per RP id, and go at lock and sign-out. Nothing is written to disk. A failed fetch isn't cached.
     - **What a site learns:** that someone on this network asked for its file, which the app asking would show it anyway.
     - **Not used for the autofill service (D58):** there, apps still match only through the user's own links. A Digital Asset Links check for every login's domain on every app screen would be a stream of requests revealing which sites are in the vault.
  5. **What is offered:**
     - **Unlocked** (the app's process holds the vault, as the lock timeout allows): the matching passkeys and passwords, each with the login's title and username. For a browser, passwords match the origin as D33 matches pages; for an app, through `android-app://` links (§6).
     - **Locked:** one `AuthenticationAction`, "Unlock PWM". It opens PWM's unlock (fingerprint or face, or the master password), then the entries are sent back with `setBeginGetCredentialResponse`. As in D58 §6, nothing about the vault is kept outside its encryption to name entries before unlocking.
     - **`allowCredentials`** limits the passkeys offered to those listed; with none of ours listed, no passkey entry is offered.
     - **Only ES256** (D37 §3). A `create` that doesn't offer it gets no entry.
     - **`excludeCredentials`** listing one of ours gets no `CreateEntry` for that account.
  6. **Confirming:**
     - **Passkeys:** every use asks for a fingerprint or face (`BiometricPrompt`), or the master password when that's off, right before signing. The UV flag is always 1 (D37 §3), and this makes it true on Android, as Face ID does in D47. Coming from "Unlock PWM" a moment earlier counts.
     - **Passwords:** the tap on the entry is the confirmation, as in D58 §5.
     - **The sheet is Android's.** We draw nothing in the caller's window.
  7. **Passwords through the Credential Manager:**
     - **Getting one:** `GetPasswordOption` is answered with `PasswordCredentialEntry`s, matched as in §5.
     - **Saving one (`CreatePasswordRequest`):** a `CreateEntry`, "Save in PWM". Picking it opens PWM (unlocking first), which shows the username and the site or app, and the vault, then saves a new login: for a browser with the origin as its website, for an app with its `android-app://` link and the app's name as title. An existing login with the same username for that site or app is updated after asking. This closes D58 §7's "saving new logins" gap for apps that ask through the Credential Manager; saving from the autofill service is still a gap.
  8. **Turning it on:**
     - **Android 14+:** the system's "preferred service" choice (`ACTION_REQUEST_SET_AUTOFILL_SERVICE`, D58 §2's card) sets both the autofill service and the credential provider of one package, so D58's card turns on both. When autofill is on but the provider isn't (the user turned it off in the system's "Passwords, passkeys & accounts"), Settings shows "Turn on PWM for passkeys", which opens `CredentialManager.createSettingsPendingIntent()`.
     - **Android 13 and older:** no Credential Manager, so no card; passkeys stay a logged gap there.
     - **`onClearCredentialStateRequest`** (an app signing out) has nothing to clear; it answers success.
  9. **Not included, and logged as gaps:**
     - **Hybrid / cross-device (QR) sign-in** from another device: Android's, not a provider's.
     - **The single-tap flow** where the system shows the fingerprint prompt inside its own sheet (`BiometricPromptData`, AndroidX 1.5+): later, once the plain flow is proven on real phones.
     - **Credential Manager requests from the autofill framework** (Android 15's fields marked for credentials): they reach the autofill service as today.
     - **Saving from the autofill service** (D58 §7).
  10. **Tests:**
      - **Core (tests first):**
        - the app's client data bytes and the hashed-path response JSON, as new vectors (`core/test-vectors/passkey_android.json`), with signatures checked by Rust and `verify.mjs`
        - the asset links matcher: listed, another package, another certificate, another relation, a malformed file, a file over the limit, an `include`
        - a fuzz target for the asset links parser (`asset_links`)
        - `passkey_create_for_app` / `passkey_get_for_app` in `pwm-client`: refused when not linked, the `android-app://` link written, `allowCredentials` and `excludeCredentials`
      - **JVM:** the caller rules (a listed browser, a browser under another certificate, an app, PWM itself), the entries built locked and unlocked, the confirmation rule, "Save in PWM", and the asset links fetcher's limits over a fake transport.
      - **On the emulator:**
        - the provider turned on with `adb` (the `credential_service` secure settings)
        - the test app, as a caller, creates a passkey through `CredentialManager.createCredential` and signs in with it, picking PWM in the system sheet with UiAutomator. The test verifies both responses' signatures with the public key it got.
        - a password saved and then fetched the same way
        - debug builds only: the `pwm.test` switch (D58's notes) can supply the asset links file for the test's RP id, since CI can't serve HTTPS for one. A release build never reads it.
      - **By hand (`QUALITY.md`):** a real site in Chrome (webauthn.io), a real app that uses passkeys, and signing in from another phone's PWM after sync.
  11. **Downloads:** the first build fetches `androidx.credentials:credentials` 1.6.0 (AndroidX, Apache-2.0, to be confirmed by the licence check) and its dependencies. Not `credentials-play-services-auth`, which is for callers on older Android. The founder approves that here.

  Alternatives:
  - **Only the browser path,** leaving apps for later: no Digital Asset Links at all, but passkeys in apps are the main reason the Credential Manager exists.
  - **Asking Google's Digital Asset Links API** (`digitalassetlinks.googleapis.com`) instead of fetching the file: it handles `include` and caching, but tells Google every app and site a user signs in to with PWM.
  - **Requiring `get_login_creds` only:** stricter, but refuses sites that list their app for app links only, which Google Password Manager accepts.
  - **Building the app's client data on the platform:** simpler to wire, but it's vault crypto input, which `CLAUDE.md` keeps in the core.
  - **No fingerprint per passkey use:** one tap like passwords. But a passkey says "user verified" to the site, which should mean something more than an unlocked phone.

  Implementation notes (M6.10):
  - **The client data, checked against Google's provider sample:** an app's is `{"type","challenge","origin","androidPackageName"}` in that order, with no `crossOrigin`, and the origin's hash in base64url without padding, as Google's own sample provider (`android/identity-samples`, MyVault) writes it. It wasn't compared with a real Google Password Manager response: the emulator has no Google account. The bytes are pinned in `core/test-vectors/passkey_android.json`.
  - **The browser's placeholder** is `e30` (base64url of `{}`), as the provider guide asks for a placeholder. Google's sample leaves the field out instead; a browser replaces it either way.
  - **A browser that passes an origin but no hash** gets client data built in the core for that origin, as the extension does.
  - **The core reads the asset links files** (`read_asset_links`, `asset_links_includes`, with a fuzz target, `asset_links`); `passkey_create_android`, `passkey_candidates_android` and `passkey_get_android` take the texts and refuse an app none of them lists (`passkey_app_not_linked`). The app fetches them (`AssetLinks`, its own transport: HTTPS only, no cookies, cache or redirects, 5 seconds, 256 KB) and keeps them in memory.
  - **"Just confirmed" is one minute:** unlocking or confirming in PWM's screen confirms a passkey for 60 seconds, so "Unlock PWM" and then a passkey asks once. Locking forgets it.
  - **Locked, "Create a passkey in PWM" is offered** for a browser, or for an app with a site file. Whether the site's file lists the app, and whether the account already has a passkey there, are checked after unlocking, since the core needs the vault open for the second. A refusal then shows "This app's website doesn't list it…" or "PWM already has a passkey for this account." (two new shared messages).
  - **Password entries skip logins with no password** (a passkey's login may have none): Android refuses an empty one.
  - **"Save in PWM" goes to the first vault,** as the iOS extension's passkeys do (D47 notes). For an app, the login is titled with the app's name and linked to it (`android-app://`); for a browser, its website is the origin.
  - **The device test:** the test app's screen asks the Credential Manager through the platform's API (its process has no AndroidX), with AndroidX's bundle keys. Debug builds read the test's asset links file from the `pwm.test` switch, as D61 §10 says; a release build never reads it. The test makes a passkey, signs in with it (the signature is checked with the platform's P-256), then saves and uses a password, picking PWM in the system's sheet. With it, the Android job's server sees 5 sign-ups, which is its hourly limit from one address; a sixth test that signs up needs the limit raised for tests.
  - **Run on this machine:** the core's tests (11 passkey crate tests, 6 client tests, the vectors in Rust and Node), every Rust test against a local Postgres, the JVM tests (13 new), the build and licence check (163 libraries), `just check`, and the 6 device tests on an Android 16 emulator. Not run: a real site in Chrome, a real app, or the fuzz target (libFuzzer doesn't build on Windows; CI runs it).

- **D62 — Signing out ends the session on the server** · settled 2026-11-05 · founder (approved as proposed).
  The server has had `POST /v1/auth/logout` since M2 (it deletes the device's session and its push token), but no client calls it: signing out only wipes the device. The device's refresh token then stays valid for up to 90 days unused (D26), so a copy of it taken before sign-out (a backup, malware, a stolen disk image) keeps the account's ciphertext reachable, and the device keeps showing as signed in on the Devices list. Found while building Android push (M6.9). Open: what the core does, how long an app waits, what happens offline, and the extension, whose tokens are sealed while locked (D34).

  Proposal:
  1. **The core: `start_sign_out(tokens)`**, a step-loop call like `start_push_token` (D50 §3) that works locked or unlocked:
     - It abandons whatever is running first (a sync, a device call): signing out wins, and what the run would have saved is about to be wiped.
     - It sends `POST /v1/auth/logout` with the open engine's tokens when unlocked, or with `tokens` from secure storage when locked. A refused access token is refreshed once, as for any call.
     - It ends with a new outcome, `LoggedOut`, when the session is over: a 204, a refused refresh token (already over) or a revoked device (already over; the engine has cleared the cache, D28).
     - With no account cached (a revoked device's cache was cleared) it answers `locked` at once and sends nothing.
     - 429 and 5xx end it with a wait, which the apps do not retry; any other status is `server`.
     - It never clears anything. The app calls `sign_out` afterwards, whatever happened.
     - UniFFI: `startSignOut(access, refresh)`. WASM: `startSignOut()`, unlocked only, since the extension has no tokens while locked.
  2. **The apps: try it, then sign out anyway.** Each app's sign-out first tries `start_sign_out` for at most 5 seconds (as Android's push token removal, D60 §4), ignores any error, then wipes the device as today. Being offline, a dead server or a refused token never blocks signing out.
     - **Apple (`PWMKit`):** `Session.signOut()` becomes `async` and does both; `forgetPushToken` before it stays.
     - **Android (`:kit`):** `Session.signOut()` does both, with its own transport whose timeouts are 5 seconds, so a dead network cannot hold the sign-out past the limit while a request is in flight.
     - **Extension:** the background's sign-out does both. Signing out is only offered in Settings, which needs the vault unlocked, so the tokens are always there; if it were ever locked, it signs out locally only.
     - A revoked device's sign-out finds no account and sends nothing (1).
  3. **The server:** no change. Its test already checks that after logout the access token is refused, the refresh token is refused and the other device stays signed in; a new test runs the core's `start_sign_out` against it, locked, and checks the refresh token and the push token are gone.
  4. **Offline sign-out:** the session then stays on the server until its refresh token goes unused for 90 days, or until the device is signed out from another device's Devices list (M5.10), which ends it at once. `SECURITY.md`'s threat model says so. A later queue that retries the logout after the wipe was considered and left out: it would keep a token on a device the user believes is clean.
  5. **The push token removal before sign-out** (Apple's `forgetPushToken`, Android's D60 §4) stays as it is. Logout also drops the push token, so it is now redundant when online, but harmless; removing it is not this task.

  Alternative: end the session from the core's `sign_out` itself (one call for the apps). Left out: `sign_out` is synchronous and offline by design, and the step loop is how every other server call works.

  Implementation notes (M6.12):
  - **Core:** `start_sign_out` takes `Option<Tokens>` in Rust so the WASM build can pass none. `sign_out` also drops events not yet taken: an app that gave up waiting after a token refresh must not store the new tokens after the wipe.
  - **The 5 seconds:** Apple and Android give the sign-out its own transport whose request timeout is 5 seconds (`withTimeout` on the transport; a test fake ignores it) and cancel the run after 5 seconds, so a request in flight ends within the transport's timeout. The extension races the run against a 5-second timer and wipes the device when the timer wins; the abandoned run finishes on the replaced core.
  - **Run on this machine:** the core tests, the server tests against a local Postgres, the Android JVM tests and its server test against a local server, the extension's Vitest and WASM tests, and `just check`. The Swift tests were written on Windows and run in CI.

- **D63 — The checks before filling, on real sites: the top frame, rounded fields, and saying which check failed** · settled 2026-11-06 · founder (approved as proposed).
  The first top-sites run (D39) that worked, on 2026-11-05, filled 7 of the 54 sites it could reach (13%). M4's target is 90%. A second run logged each D35 §4 check on the fields the engine chose. On most failures the engine found the right fields, and a D35 check then refused them:
  - **IntersectionObserver v2 alone, 19 sites.** Every other check passed: Google, YouTube, Facebook, Instagram, Microsoft, Heroku, Dropbox, Box, Docker, Costco, IKEA, BBC, Figma, Kraken, ok.ru, Naver, Wells Fargo, Khan Academy, Stack Overflow. v2 reports a field hidden when anything at all is drawn over any part of it. On Heroku that is a small decorative icon at the left end of the email field. These sites are in the top frame, where nothing can cover the frame itself.
  - **The covering check, about 5 sites:** Twitch, Discord, DigitalOcean, Roblox, Coursera. On most of them, the topmost element at a sample point is one of the field's own wrappers, not something over the field. The likely reason is that the corner points, 2 px inside the field's box, fall outside its rounded corners.
  - **The rest** are harness and site-list problems (the runner waits 1.5 s, stale login URLs, a tab looked up by its exact URL) and two engine choices of a hidden decoy field (Booking, Airbnb). They are their own tasks, not this decision.

  D35 §4 already lets Firefox and Safari, which lack v2, fill in the top frame without it. D35 §4 uses v2 for a frame hidden or covered by its parent page, which nothing else can see from inside the frame.

  The catch is the fixture `cover-pointer-none.html`. It puts an opaque white layer over a top-frame login form with `pointer-events: none`, so hit testing still finds the field. Only v2 refuses it today, so relaxing v2 in the top frame decides what happens to that fixture.

  Open:
  - whether v2 is required in the top frame, and what replaces it
  - what counts as "covered" at a sample point
  - how the harness learns which check failed

  Proposal:
  1. **v2 only where it's needed:** a fill into a frame inside another page still needs v2 to report the field visible, as now (`frame-covered.html` keeps refusing). In the top frame, v2 is no longer required, as on Firefox and Safari today. The other D35 §4 checks stay: field state, size and viewport, style, and covering by hit testing.
     - **Why this is safe enough:** in the top frame, anything drawn over the field is put there by the matched site's own page, which can already read the field. The covering check there keeps us from filling a field hidden by a modal or banner; it doesn't protect the values. Cross-site content in the page sits in its own frames, and hit testing catches a frame drawn over the field.
     - **What changes:** `cover-pointer-none.html` fills instead of refusing. A new fixture, `frame-cover-pointer-none.html`, puts the same layer, from the parent page, over a login form in a frame, and must refuse.
     - **The inline icon (D35 §3) is unchanged:** its clicks still need v2 on our own iframe.
  2. **Rounded fields:** a sample point counts as clear when the topmost element there is the field, its own label, or one of the field's ancestors, looking through open shadow roots. An ancestor is only on top where the field doesn't paint, such as outside a rounded corner. It never covers the field. Anything else on top is still "covered". The sample points stay the centre and two corners 2 px in.
  3. **Saying which check failed:** the content script already returns the failing check (`covered`, `not_visible`, `transparent`, …), but the background drops it. It is kept on the error as a detail. The popup still shows "The field is hidden or covered". The top-sites report records `field_hidden:<check>`. The "Autofill didn't work here" report (D38) is unchanged.
  4. **Tests:**
     - **Unit tests** (`checks.test.ts`):
       - v2 false in the top frame passes, and in a frame is `not_visible`
       - an ancestor on top is clear, anything else is covered
     - **New fixtures that must fill:** `icon-in-field.html` (a decorative icon over the field's left end), `rounded-field.html` (a rounded field in a wrapper).
     - **Fixtures that must refuse:** `frame-cover-pointer-none.html` (new), and all the current attack fixtures except `cover-pointer-none.html`.
     - **The top-sites run:** compared before and after with `runner/compare.ts`. About 24 more sites should pass (the 19, plus about 5 rounded fields), taking it from 7 to about 31 of 54.
  5. **Task:** M4.11 in `ROADMAP.md`, extension only. The engine (`pwm-autofill`) is unchanged, and nothing is stored or sent differently.

  Alternatives:
  - **Keep v2 in the top frame, but accept "not visible" unless something big covers the field:** refuse when another element that paints (a background colour or image, an image, video, canvas or frame) covers at least half of the field. This keeps `cover-pointer-none.html` refusing. But telling whether that element is above or below the field needs its stacking order, which the page doesn't expose. A filled box drawn behind a transparent field, which some design systems use, would then be refused too. It also guards only against the matched site's own page.
  - **Keep D35 §4 as it is** and fix only the harness: about 7 of 54 stays the baseline, and the extension refuses to fill Google, Facebook, Microsoft and most of the sites above for real users too.
  - **Drop v2 everywhere:** it would let a parent page hide a cross-site login frame, which is the case D35 §4 added v2 for.

- **D64 — The top-sites release gate: compare the sites both runs could reach** · settled 2026-11-06 · founder (approved as proposed).
  The release gate (D39, `runner/compare.ts`) fails when the pass rate, pass ÷ (pass + fail), drops more than 0.5 points from the previous report. Two top-sites runs of the same code on 2026-11-06 (CI runs 42132920831 and 42141806371) gave 59.2% and 58.0%, so the gate would have stopped a release with nothing changed. The pass count was 29 both times. The rate moved only because three sites moved between the counted statuses and the others:
  - asana.com: pass → blocked (a challenge)
  - trello.com: blocked (a challenge) → fail (no form)
  - instagram.com: error → pass

  On the 48 sites counted in both runs, the rate was 58.3% both times, and no site went from pass to fail or back. The noise comes from sites that block or fail to load on some nights, not from fills that pass on one night and fail on the next.

  A second issue is scale. QUALITY.md's 0.5 points was written for 1,000 sites, where it is 5 sites. With about 50 counted sites, one site is about 2 points.

  Open:
  - which sites the gate compares
  - how much it lets through
  - what it does when a run reaches too few sites

  Proposal:
  1. **Compare like with like:** the gate's rate counts only the sites that are pass or fail in both reports. A site that is blocked or errored in either run, or is in only one report (added to or removed from the list), is listed and not counted. On the two runs above, the gate sees 58.3% → 58.3%.
  2. **The limit stays 0.5 points,** on that rate. Below about 200 common sites, one site that goes from pass to fail is more than 0.5 points, unless another site went the other way. In practice the gate then stops on any net loss, which is what a short, curated list should do: either the extension broke a site, or the site changed and its entry needs a look. At 1,000 sites it is 5 sites, as QUALITY.md meant.
  3. **Enough overlap:** the gate also fails, with "too few sites to compare", when fewer than 80% of the sites counted in the previous report (and still in the candidate's report) are counted in the candidate. Otherwise a night when the CI address is blocked almost everywhere would pass with whatever is left. On the runs so far, the overlap was 46 of 47 (2026-11-05 to the first 2026-11-06 run) and 48 of 49 (between the two 2026-11-06 runs).
  4. **No reruns for now:** no site has gone from pass to fail and back between runs of the same code. If consecutive nightly runs start showing that, a rerun of each such site becomes its own decision.
  5. **What it prints:**
     - the whole-list rate as today (it is the number the dashboard and the M4 target use)
     - the gate's rate on the common sites and the overlap
     - each site that went from pass to fail, then those that went from fail to pass
     - the sites left out of the count, with their statuses
  6. **Report format:** unchanged (v1). Only `compare.ts` and its `Comparison` change. The gate reads two reports, so both must be v1, as now.
  7. **Tests** (`runner.spec.ts`):
     - a site moving to blocked or error doesn't move the gate's rate
     - one pass → fail among 48 sites fails the gate; with one fail → pass as well, it passes
     - added and removed sites are left out
     - overlap under 80% fails
     - the 0.4 / 0.6 point cases, rewritten on the common sites
  8. **Docs:** QUALITY.md's CI table, AUTOFILL.md, the harness README and ROADMAP M4.7's gate line point to D64. Task: M4.12 in `ROADMAP.md`, harness only. The extension and core are unchanged.

  Alternatives:
  - **Gate on each site that goes from pass to fail, with no offset for gains:** stricter, and it names the broken sites outright. But at 1,000 sites, any one site's redesign would stop every release until its entry is fixed. The proposal lists those sites anyway.
  - **Rerun any site that changed, before counting it:** no pass → fail flips have been seen to justify it, and each rerun would use up the one retry per site that D39 §2 allows, to stay polite to real sites.
  - **The better of two runs:** doubles the load on every site, for noise that comparing like with like already removes.
  - **A wider limit, such as 3 points:** with about 50 sites, one site is still about 2 points, and at 1,000 sites, 3 points would hide 30 broken sites.

- **D65 — Autofill on real sites: decoy fields, a password that isn't shown yet, buttons inside a field, and "email address"** · settled 2026-11-06 · founder (approved as proposed).
  After D63 and the runner fixes (M4.7), the top-sites run of 2026-11-06 filled 29 of the 50 sites it could compare (58%). M4's target is 90%. The login pages of 11 failing sites were opened in the built-in browser and their fields read (no values typed, nothing submitted). Five causes cover 8 of them:
  - **A decoy password field, Booking and Notion.** Both ask for the email first. Their page also holds a password field that people can't see or reach, inside `aria-hidden="true"` with `tabindex="-1"`: on Booking under a `span`, on Notion in a wrapper with opacity 0 and no height. It is there for browsers' own password managers. The engine plans to fill it, D35 §4 refuses it (`covered`, `transparent`), and the whole fill is refused, so the visible email field gets nothing.
  - **A password that isn't shown yet, probably Apple.** Its form (in an `idmsa.apple.com` frame) has both fields, and the fill is refused as `transparent`. That fits a password row kept transparent until the username is sent. It couldn't be confirmed here, since the frame doesn't load on its own.
  - **A button inside the field, Twitch.** The show-password button sits inside the right end of the password field. One of D35 §4's sample points (2 px inside a corner) lands on it, so the field counts as covered.
  - **"Email address" read as a street address, DigitalOcean and ok.ru.** The email field's only clue is its placeholder ("Enter your email address", "Phone number or email address"), and the classifier takes the longest keyword it finds. "address" (7 letters, `address-line1`) beats "email" and "phone" (5), so the field isn't the username.
  - **The runner, not the extension, on Salesforce, Proton and Wells Fargo:**
    - Salesforce and Proton now ask for the username first, but their entries in `sites.json` still name a password field.
    - Wells Fargo's username field takes 14 characters, and the check login has 30, so the site cuts it.
    - Naver's password field (16 characters, against the check password's 18) may be the same. It can't be opened in the built-in browser.

  Left out of this decision:
  - **Airbnb:** its field is at opacity 0 by design, even when focused, and the site draws what is typed elsewhere. Filling it would mean filling transparent fields, which `opacity.html` refuses on purpose.
  - **The sites where no form was found:** Amazon, Deezer, EA, Nintendo, Baidu, CNN and Chase. They need their own look.
  - **Alibaba**, which refused.

  Open:
  - what makes a field one people can't reach
  - what a fill does when only some of its fields pass D35 §4
  - what may sit inside a field's box
  - how "email address" is read

  Proposal:
  1. **Fields people can't reach are not offered.** The content script reports a field as not visible when it, or an ancestor (looking through open shadow roots), has `aria-hidden="true"` or `inert`. The engine already ignores fields reported as not visible. They then get no fill, no inline icon (D35 §3) and no save prompt (D36). `tabindex="-1"` alone doesn't count, because some real fields use it.
  2. **The username alone, when the password can't be filled.** When a fill plans a username and a password, and the password field fails D35 §4 while the username field passes, only the username is filled. The multi-step state records the username step, as on a username-first page (D33). If the username fails, the fill is refused, as now. A field that fails is never filled.
  3. **Buttons inside a field.** A corner sample point counts as clear when the element on top there lies wholly inside the field's box and covers at most a quarter of it. That is how show-password and clear buttons sit. The centre point must still be the field, its label or one of its ancestors (D63 §2).
     - **Why this is safe enough:** in the top frame, anything over the field is the matched site's own (D63 §1). In a frame, v2 still has to report the field visible, which it doesn't with anything drawn over it. A layer larger than the field, or reaching outside it, is still `covered` (`overlay.html`).
  4. **"Email address" is an email.** New keywords: `emailaddress` and `mailaddress` (email), and `phonenumber` and `mobilenumber` (tel). The longest keyword still wins, so "email address" is an email and "phone number or email address" is an email. A field that names both an email and a phone is an email, whichever keyword is longer: the branch's top-sites run showed Zoom's "enter email, zoom mail or phone number" read as a phone once `phonenumber` outgrew `email`. Classes are unchanged, and no stored or sent format changes.
  5. **The runner:**
     - A field with a `maxlength` shorter than the check value passes when it holds the value cut to that length.
     - "A password field to fill" (the username-first rule) leaves out a field the user can't see: one under `aria-hidden` or `inert`, or with an ancestor at opacity 0.
     - Salesforce's and Proton's entries lose their password selector.
  6. **Tests:**
     - **Unit tests:**
       - `pwm-autofill`: DigitalOcean's and ok.ru's field texts
       - `checks.test.ts`: a button wholly inside the field passes; a layer over half of it, or reaching outside it, is covered
       - `fill.test.ts`: a failing password leaves the username filled alone; a failing username refuses
       - the content script: `aria-hidden` and `inert` fields are not visible
     - **New fixtures that must fill the username only:** `decoy-password.html` (Booking's pattern) and `password-later.html` (a password row at opacity 0).
     - **New fixtures that must fill both:** `show-password.html` (a button inside the field's right end).
     - **New attack fixtures that must refuse:** `cover-half.html` (a layer over the field's right half).
     - **Unchanged:** every current attack fixture still refuses.
     - **The top-sites run:** compared with the 2026-11-06 report by the gate (D64). Booking, Notion, Twitch, DigitalOcean, ok.ru, Salesforce, Proton and Wells Fargo should pass, taking it from 29 to about 37 of 50 (74%), plus Apple and Naver if they are what they seem.
  7. **Task:** M4.13 in `ROADMAP.md`. It covers the extension, `pwm-autofill`'s keywords and the harness. iOS and Android classify through the same core, so they get the keywords. Their own checks before filling are their own code; whether rules 1–3 apply there is a parity note for M5 and M6.

  Alternatives:
  - **Per-site rules in the signed rules file (D33):** they fix one site at a time, and the next site with the same pattern fails again. They stay for sites like Airbnb.
  - **Fill whichever planned fields pass, in any combination:** a password filled without its visible username is a strange result for the user. The username-first case is the one real sites show.
  - **Count `tabindex="-1"` as unreachable:** some sites use it on real fields that they focus from script.
  - **Let a transparent field fill when nothing covers it (Airbnb):** `opacity.html`'s invisible login form passes that test too.
- **D66 — The desktop app's backend: tasks, project layout, the protocol in Rust, storage, the network, locking and the window, and tests** · settled 2026-11-06 · founder (approved as proposed).
  D3 picks Tauri 2 and shares the screens (M7.1). The roadmap's M7.4 asks for the shared screens, master-password unlock, the tray and the lock rules, screen-capture exclusion on Windows, `cargo deny`, a WebKitGTK floor and a CI job. In the extension, the background service worker answers the screens' requests (about 3,000 lines of TypeScript over the WASM core). On the desktop, a Rust backend must answer the same requests.

  What this machine has: the Rust toolchain for MSVC, Visual Studio 2019 and WebView2. Tauri's crates and `@tauri-apps/api` are downloaded on first build.

  Open:
  - how M7.4 is split
  - the project layout
  - how the backend answers the screens' protocol, and what it doesn't answer
  - where the cache and the device's keys live
  - the HTTP stack, and certificate pinning
  - locking, the window and the tray
  - the clipboard
  - tests, CI and downloads

  Proposal:
  1. **Split M7.4 like M6.1, one PR each, in this order:**
     - **M7.4 The app:**
       - the project, the backend, storage and HTTP
       - onboarding: create an account, the Emergency Kit, sign in, and asking another device
       - unlock, lock and sign out
       - the vault list, copying, the generator and Settings
       - the tray, the lock rules and screen-capture exclusion
       - the CI job
     - **M7.6 (new) The manage page on the desktop:** health and the leak check, devices, approving a new device, export and import. These are the requests the extension's `manage.ts`, `approvals.ts` and `importer.ts` answer.
     - **M7.7 (new) Items in a full window:**
       - a sidebar layout
       - an item's detail, and editing every item type
       - these go in the shared screens, with new requests, so the extension's manage page can use them later
       - this brings the desktop to the Mac app's screens (D42, D43)
     - **Then:** M7.5 (Windows Hello), M7.2 and M7.3, as D3 orders them.
  2. **Project layout:** `apps/desktop`, with its own Cargo workspace and lock file, depending on the core crates by path.
     - **`src-tauri/`:** the `pwm-desktop` crate, the app. M7.2's helper will be a second crate in this workspace.
     - **`web/`:** `@pwm/desktop`, a pnpm package. Its `main.tsx` sets the desktop host and renders `@pwm/ui`'s screens. It is bundled with esbuild, as the extension is, into the files Tauri serves.
     - **Why a separate workspace:** the root workspace's jobs (`core-test`, the fuzzers, `bindings`) would otherwise compile Tauri and need GTK's headers on Linux. Cargo also allows only one crate linking `sqlite3` per workspace, which already ties `rusqlite` to the server's `sqlx`.
     - **Builds:** plain `cargo build`, with `tauri-build`. The Tauri CLI comes only with the installers (M7.3). New recipes: `just desktop` (build and run) and `just desktop-test`.
  3. **The protocol in Rust:**
     - **One Tauri command, `request`.** It takes a request and answers a reply, the same JSON as `ui/src/protocol.ts`: a `kind` tag, camelCase fields, and `{ok, value}` or `{ok: false, error}`.
     - **In `protocol.rs`:** serde types for every request and reply. One shared file of sample requests and replies, one per kind, is read by a Vitest test against the TypeScript types and by a Rust test that round-trips it, so the two sides can't drift.
     - **Behind it:** one `pwm_client::Client` over the SQLite cache, driven by the step loop (D28, D31) over HTTP. A single async mutex keeps calls from overlapping, as Android's dispatcher does.
     - **What it doesn't answer:** the browser's requests (fill, suggestions, the inline icon, save offers, passkeys, the autofill report, Touch ID). They answer `unsupported`, which the shared messages describe in plain words.
     - **The host:** a new host flag, `browser`, hides those parts of the screens. A second flag replaces the Settings screen's user-agent test for Touch ID, so Windows Hello can take its place in M7.5.
  4. **Storage,** in the app's local data folder: `%LOCALAPPDATA%` on Windows, `$XDG_DATA_HOME` on Linux, never the roaming profile, so a device's identity stays on that device.
     - **The cache:** `pwm-store`'s SQLite file. It holds ciphertext only (D20).
     - **The device's keys** (Secret Key, device id, access and refresh tokens), behind a `KeyStore` trait with an in-memory fake for tests:
       - **Windows:** each value is sealed with DPAPI for the current user, with the key's name as its extra entropy, in a file under `keys/`. As with Android's Keystore, the platform's cipher only protects them on disk. No crypto of ours runs outside the core.
       - **Linux:** the Secret Service (GNOME Keyring, KWallet) through `oo7`, which inside Flatpak uses the Secret portal.
       - **Linux without a Secret Service** (some minimal desktops): the extension's model (D34). The Secret Key goes in a file only the user can read (it's useless without the master password, D13). The tokens are sealed under `sym_key` by the core (`pwm/device-tokens`), so they work only after unlock. Settings says no keyring was found.
     - **`unsafe` code:** DPAPI's calls through the `windows` crate are `unsafe`. The crate is `#![deny(unsafe_code)]`, with one module allowed: `keys/windows.rs`.
     - **Settings** (server, lock timeout, start at login): a JSON file with `"v": 1`. It isn't secret and isn't synced.
     - **Never written:** the master password, the keyset or any unlocked key. They live only in the backend's memory, in `Secret<T>`.
  5. **The network:**
     - **The client:** `reqwest` 0.13, async on Tauri's tokio, with no cookies, no cache, and no redirects to another host.
     - **TLS:** `rustls` with its `ring` provider, as the server uses it. `ring` serves only as TLS's provider; our code never calls it.
     - **Trust:** `rustls-platform-verifier`, the system's trust store, so company root certificates work as in a browser. Never `webpki-roots`, whose licence (CDLA-Permissive-2.0) isn't on the S6 list.
     - **Pinning:** no app pins today. iOS and Android leave TLS to the system, and an extension can't pin. So `SECURITY.md`'s "certificate pinning on mobile/desktop" isn't true anywhere. Proposal: pin the hosted server in every app at once, when D1 gives it a hostname (M10). Until then, `SECURITY.md`'s threat-model row says so. This replaces D3 §3's TLS line.
     - **When it syncs,** as the extension does: on unlock, when the window comes back, and after each change, retrying after a network error.
  6. **Locking, the window and the tray:**
     - **The lock timeout:** the extension's choices (1, 5, 15 or 60 minutes, or 8 hours; 15 by default), from the shared Settings screen.
       - **When it starts:** when the window loses focus or is hidden. Any use in the window starts it again.
       - **Why:** as on the Mac (D48), switching to another app counts as leaving.
     - **Locking at once:**
       - **On sleep:** `PBT_APMSUSPEND` on Windows; logind's `PrepareForSleep` on Linux.
       - **When the session locks:** `WTS_SESSION_LOCK` on Windows; logind's `Lock` and `LockedHint` on Linux.
       - **On switching user and signing out of the computer.**
     - **What locking does:** the backend drops the unlocked keys (zeroized) and the window shows Unlock.
     - **Closing the window:**
       - **Windows:** closing keeps PWM in the tray, whose menu has Open, Lock and Quit. Quitting locks first.
       - **Linux:** closing quits, because GNOME shows no tray by default. The tray icon appears where the desktop has one.
     - **One copy only** (`tauri-plugin-single-instance`): a second launch brings the window forward, so two processes never share one cache.
     - **Start at login:** a setting, off by default (`tauri-plugin-autostart`).
     - **Screen capture:** the window is left out of captures on Windows (D3 §3).
     - **Not in M7.4:** a global shortcut. With no autofill on the desktop it has nothing to trigger; it can come with M7.2.
     - **Devtools:** off in release builds.
  7. **The clipboard:** copying goes through the backend (`arboard`), never the webview.
     - **The 30-second clear (D34):** the clipboard is cleared 30 seconds later, whatever it holds then.
     - **Windows:** the copy is left out of the clipboard history and the cloud clipboard.
     - **Linux:** the copy is marked with the password-manager hint (`x-kde-passwordManagerHint`), which the clipboard managers that know it skip.
  8. **Licences, checked on crates.io and npm on 2026-11-06:**
     - **MIT or Apache-2.0:** `reqwest`, `rustls-platform-verifier`, `arboard`, `tauri-build`, the plugins, and `@tauri-apps/api` (no dependencies of its own).
     - **`rustls`:** Apache-2.0, ISC or MIT.
     - **`oo7`:** MIT.
     - **The check:** `just audit` runs `cargo deny` on the desktop workspace too, with the root `deny.toml`. `scripts/licenses.mjs` adds `@pwm/desktop` to the shipped packages.
  9. **Tests and CI:**
     - **Rust:**
       - each request's handler against the fake key store and a fake HTTP transport
       - the protocol samples (§3)
       - the lock timer, with an injected clock
       - against a real server: sign-up, sign-in on a second client, save, sync, lock and unlock (in CI, with Postgres)
     - **Vitest:** the desktop host, and the protocol samples against `protocol.ts`.
     - **End to end with `tauri-driver`:** launch, create an account, lock, unlock and copy, on Linux under Xvfb. On Windows, CI builds and runs the Rust and Vitest tests; `tauri-driver` there needs an Edge driver matching WebView2's version, so it is added only if it's reliable.
     - **A new `desktop` CI job,** on PRs touching `apps/desktop`, `ui/` or `core/`, on `windows-latest` and `ubuntu-22.04`. Ubuntu 22.04 is the oldest supported LTS with WebKitGTK's 4.1 API, which Tauri 2 needs. That's the floor, written in `DEVELOPMENT.md`.
     - **This machine:** builds and runs the app on Windows 11.
     - **Not testable in CI:** sleep and session-lock events, and the tray on a real desktop. Their manual checks go in `QUALITY.md`.
  10. **Downloads:** the first build fetches Tauri's crates, `reqwest`, `oo7`, `arboard` and their dependencies from crates.io, and `@tauri-apps/api` from npm. CI also gets `tauri-driver` and WebKitGTK's packages. The founder approves that here, once, instead of per command, as in D52 §9.

  Alternatives:
  - **The app in the root Cargo workspace:** one lock file, but every core job would build Tauri and need GTK, and the SQLite limit binds the app to the server's `sqlx`.
  - **The `keyring` crate on both platforms:** one API, but its Windows backend is the Credential Manager, whose entries can roam with a domain profile and carry one device's identity to another. DPAPI files in the local folder stay put.
  - **The extension's model everywhere, with no system key store:** simpler, but weaker than iOS and Android against someone who copies the disk.
  - **A blocking HTTP client (`ureq`):** smaller, but Tauri's commands are async, and `reqwest` fits its tokio.
  - **Tauri's HTTP, file system or shell plugins for the UI:** D3 §3 rules them out; the backend does all of that.
  - **Implementing the protocol in TypeScript over WASM inside the webview, as the extension does:** it would reuse the background's code, but the keys would then live in the webview, where D3 §3 keeps them out, and Windows Hello and DPAPI would still need Rust.

  Implementation notes (M7.4):
  - **Versions:** Tauri 2.12, `tauri-plugin-single-instance` 2.5, `tauri-plugin-autostart` 2.7, `reqwest` 0.13, `oo7` 0.7 (zbus 6), `arboard` 3.7, `windows` 0.62, and `@tauri-apps/api` 2.12.4.
  - **Licences, approved by the founder for these crates only,** as exceptions in `apps/desktop/deny.toml`; the S6 allow-list is unchanged:
    - **`clipboard-win` and `error-code` (BSL-1.0):** arboard's clipboard on Windows. BSL-1.0 is the Boost licence, permissive and OSI-approved, not the Business Source License that `deny.toml`'s "BSL" means.
    - **`target-lexicon` (Apache-2.0 WITH LLVM-exception):** a build-time dependency of the GTK libraries on Linux; it never ships.
  - **`apps/desktop/deny.toml`:** the root's policy, checked only for the targets the app ships for (Windows and Linux, x86-64 and Arm). That leaves out `webpki-root-certs` (CDLA-Permissive-2.0), which `rustls-platform-verifier` uses only on wasm32. `just audit` and the `audit` job run it.
  - **Advisories ignored, with a recheck at M7.3:** proc-macro-error (RUSTSEC-2024-0370, through GTK 3's glib-macros for the tray) and zbus 5's zvariant, zbus_names and zvariant_derive (RUSTSEC-2026-0370 to 0372, through `tauri-plugin-single-instance`), all on Linux and unmaintained, not vulnerable.
  - **`unsafe` code:** two modules, not one. `keys/windows.rs` (DPAPI) and `system/windows.rs` (a message-only window for session notifications, and a suspend callback). The rest of the crate is `#![deny(unsafe_code)]`.
  - **The webview:**
    - **Prototype freezing is off:** Tauri's `freezePrototype` breaks Preact, which assigns `constructor` on its objects.
    - **One inline style is allowed by its hash:** Tauri's own `<style>` hiding the isolation frame. No other inline style or script is allowed.
    - **When the hook refuses:** a call it drops never answers. The screens only ever send `request`.
  - **Locking on the screen:** after the timeout, sleep or a session lock, the window reloads, so the page keeps nothing it showed unlocked.
  - **Sleep the system's event missed:** a wall-clock jump of more than 15 seconds beyond the time that passed also locks, so a clock change locks too.
  - **"Start at login"** is a check item in the tray's menu, not a setting in the screens, so the protocol didn't change.
  - **Downloads** go to the Downloads folder under a free name ("Emergency Kit (2).pdf"), never over a file.
  - **Shared-screen fixes found by running the app:**
    - **After the Secret Key:** sign-up went on to the browser's fill-and-save offer, which the desktop hides, and showed the Secret Key again. It now finishes there.
    - **The Touch ID setting:** its user-agent test became a host flag.
  - **The protocol samples:** `ui/src/protocol-samples.ts`, typed against `protocol.ts`, written to `protocol-samples.json` (Vitest checks it's current; `PWM_UPDATE_SAMPLES=1` rewrites it), and read by `tests/protocol_samples.rs`.
  - **Not testable here:** the Linux code (this machine is Windows). It compiles first in the `desktop-linux` job.

  Implementation notes (M7.6):
  - **The manage page shares the window:** `manage.html` sits next to `index.html` in the desktop's page. Settings opens it; "Back to the vault" comes from a new host method, `closeManage`, which the extension leaves null because its manage page is a tab of its own.
  - **The leak check's switch** is in `settings.json` (`leakCheck`). It's optional within version 1: a file without it reads as on, and a test reads an M7.4 file. The check itself goes to Have I Been Pwned through the same `reqwest` client, with padding and only 5-character prefixes (D22 §4).
  - **Site links:** the health report's "Change" opens http and https links in the default browser through the `open` crate (MIT). The app still never opens a window of its own.
  - **The device id, found while testing:** the server ties a device id to one account, so after signing out, a second account signed up on the same computer was refused (`device_conflict`, shown as a server error). As on Android (D59's notes), a sign-up gets a new id, and a sign-in reuses the stored id only for the account it belongs to (kept as `deviceAccount`). The extension had the same gap; it now follows the same rule.
  - **The server tests share accounts:** the server allows 5 sign-ups an hour from one address, and CI's end-to-end check needs one. So the server tests are one sequence with 3 sign-ups.
  - **Wording:** where the manage page named the extension (signing in from the toolbar button, signing out in its settings), the desktop says Settings.
- **D67 — Items in the desktop's window: what reaches the page, the requests, the layout, editing, and tests** · settled 2026-11-06 · founder (approved as proposed).
  M7.7 (D66 §1) asks for a full-window layout with a sidebar, an item's detail, and editing every item type, in the shared screens with new requests. That brings the desktop to the Mac app's screens (D43). Two things stand in the way:
  - **The shared screens never see a secret.** Copying goes through the backend, and the protocol's own header says the screens never receive passwords or one-time codes. Showing and editing an item needs them in the page.
  - **No such screens exist in TypeScript.** The extension's popup has the list, search and copying, but no item detail or editing.

  Open:
  - what reaches the page, and when
  - the new requests
  - the layout
  - editing: the item types, fields from newer versions, folders, tags, one-time codes, trash
  - where the code lives, and what the extension gets
  - tests

  Proposal:
  1. **What reaches the page, as D31 allows for revealed fields:**
     - **The list:** summaries only, never a hidden value. Each row has the type, title, a subtitle (the username, a card's last 4 digits, or a name), the folder, favorite, and whether a password or one-time code exists.
     - **An item's detail:** its fields with every hidden value left out. That covers a login's password, a card's number and security code, hidden custom fields, the one-time code's secret and the passwords in its history. Each is listed by its place in the item, so the screen shows it masked.
     - **Show:** clicking Show asks for that one value, which the screen holds until it hides it again, or closes the item.
     - **The one-time code:** the detail carries the current code and its seconds left, refreshed each period, as on the Mac (D43). Never the secret.
     - **Edit:** only when the user opens Edit does the page get the whole item (D18 JSON). It's dropped when the editor closes, saved or not.
     - **Copy:** still through the backend and its 30-second clear (D34). The `copy` request takes any field by its place, not just a login's three.
     - **Locking:** reloads the window (D66 §6), so nothing revealed outlives the lock.
     - **The extension's popup keeps its rule:** it never asks for these requests.
     - **The protocol's header and `SECURITY.md`'s D31 paragraph** say which requests return hidden values: `reveal` and `itemForEdit` only.
  2. **The requests (M7.7)**, answered by the desktop backend, with samples in `protocol-samples.ts`:
     - **`vault`:** the summaries and the folders, with trashed items marked.
     - **`item`:** the detail, as point 1 describes, plus whether it's read-only.
     - **`reveal`:** one hidden value of one item.
     - **`itemForEdit`:** the whole item, for the editor.
     - **`saveItem`:** a new or edited item, as D18 JSON. The core checks it (`save_item`) and keeps the previous version in its history (D19). It answers with the item's id, then syncs in the background.
     - **`trashItem`, `restoreItem` and `deleteItem`:** to the trash, back out, and forever, which the screen asks about first.
     - **`newFolder`:** a folder made from the editor's picker.
     - **`totpSetup`:** a pasted `otpauth://` URI or bare secret, read by the core's parser (D23), answering the URI to save, or why it isn't one.
  3. **The layout,** in `@pwm/ui` as a new `VaultWindow`, which the desktop shows instead of the popup's `Vault`:
     - **Three panes:**
       - **The sidebar:** All items, Favorites, Logins, Cards, Identities, Notes, each folder, and Trash, as D43 lists them. Below them: Generator, Settings, and "Password health, devices and export".
       - **The list.**
       - **The item:** its detail or its editor.
     - **Narrower than 720 pixels:** one pane at a time, with Back.
     - **Search** over the summaries, by D43 §3's rules (title, username, URL, a card's last 4 digits; trashed items and folders skipped; ordered by title).
     - **Shortcuts:** Ctrl+N for a new login, Ctrl+F for search, as ⌘N and ⌘F on the Mac (D66 §6).
     - **No website icons:** fetching them would tell third parties which sites are in the vault (D43).
  4. **Editing, by D43 §4's rules:**
     - **In place:** the editor changes the D18 JSON it was given, so fields a newer client wrote are kept. D41 and D52 do the same in Swift and Kotlin. This is the item's decrypted JSON, not vault parsing: the core decrypts, checks every save and keeps the history.
     - **Read-only items:** those with a newer schema or an unknown type show read-only, with "Update the app to edit this item".
     - **Fields:**
       - every D18 field of a login, card, identity and note
       - custom fields of every kind
       - tags as text
       - the folder from a picker, which can make a new folder
     - **The password field** has Generate, with the generator beside it.
     - **The one-time code:** pasted as a URI or a secret, through `totpSetup`. The desktop has no camera scanner.
     - **Passkeys** show read-only, as "Passkey for example.com".
     - **Password history:** read-only, newest first, each password masked with its own Show.
     - **Unsaved changes:** leaving the editor with changes asks whether to discard them. No other prompts.
     - **A sync during an edit:** the save still wins, and the other version goes to the history (D19).
  5. **Where the code lives:**
     - **`ui/src/items/`:** the screens and typed accessors for the D18 JSON, in TypeScript.
     - **`ui/src/vault-window/`:** the layout.
     - **The desktop backend:** a new `items.rs`.
     - **The extension:**
       - **Not in M7.7:** its popup is too small for these screens, and its manage page is the natural home.
       - **Parity:** iOS, Android and macOS can edit items; the extension can't. That gap is logged here, for a new task, M4.14 "Items in the extension's manage page", which reuses `VaultWindow`.
       - **Its background** answers the new kinds `unsupported` until then.
  6. **Tests:**
     - **Vitest:**
       - the item accessors, for every type: unknown fields kept, editing a card, custom fields
       - search and ordering over generated summaries, 5,000 within the time D43 allows
       - the masking rule: no summary or detail sample holds a value marked hidden
     - **Rust:**
       - each new request against a real server (save, edit with history, trash and restore, delete, folders, `totpSetup`)
       - a test that `vault` and `item` never return a hidden value
     - **Protocol samples:** for every new request and reply.
     - **End to end:**
       - create a login, edit it, Show and Copy its password
       - trash and restore it
       - a card with a custom field
     - **`QUALITY.md`:** the desktop checklist gains these steps.

  Alternatives:
  - **Send the whole item for the detail too:** simpler, but every hidden value would sit in the page whenever an item is open, not only after Show or Edit.
  - **Keep editing in Rust, sending the page a form to draw:** it keeps JSON out of TypeScript, but needs a second schema for the forms. Every app already edits D18 JSON in its own language.
  - **Native item screens in Rust:** that's against D3, which shares the screens.
  - **Item screens in the extension's popup:** a popup is too small, and closes when it loses focus.

  Implementation notes (M7.7):
  - **Passkeys never reach the page,** not even in the editor. The detail shows each one's site and user name. The editor's copy has no passkeys, and a save puts the stored ones back, so the page can't add, change or drop one. The core's edit replaces the whole login, which is why the backend does this.
  - **The history** isn't in the editor's copy either. The core keeps the previous version itself (D19), so the editor never holds old passwords.
  - **`reveal`** answers only a place the item's hidden places list, never another field. Copy takes any field by its JSON Pointer, so it shares the backend's 30-second clear.
  - **The one-time code setup** goes through `totpSetup` when the editor saves, so the core checks and normalizes it (D23).
  - **New items** come without an id. The backend gives one, along with `v` and the times, and puts the item in the first vault.
  - **The window** opens at 1080 × 720, so the three panes fit. Below 720 pixels it shows one pane at a time, as D67 §3 says.
  - **Found while running it:** the editor's changes now build on the latest item, so two changes in one tick both count.
  - **The extension's background** answers D67's kinds, and copying by place, `unsupported` until M4.14. The screens describe that code in plain words.
  - **Reused from the popup:** `Generator` and `SyncLine` are now exported from `Vault.tsx` and used in the window.

  Implementation notes (M4.14, the extension):
  - **The rules are the desktop's,** in `extension/src/background/items.ts`: the same hidden places, masking, read-only check and passkey handling as `items.rs`, with tests that mirror its own.
  - **The manage page** shows `VaultWindow` as its Items tab, at `#items`. "Password health, devices and export" switches to the other tabs in the same page, and Items comes back. The desktop's manage page has no Items tab: its window is the vault. The popup links to it with "Edit items in a tab".
  - **Settings** aren't in the extension's window: they stay in the popup, so `VaultWindow`'s Settings is shown only where the host has one.
  - **The popup can't ask:** the background answers D67's kinds, and copying by place, only from `manage.html`, as the browser names the sender. The popup gets `unsupported`.
  - **Locking:** the background tells the extension's pages, and the manage page reloads, as D67 §1 asks of the desktop's window.
  - **The trash empties at unlock** (D19) in the extension too, now that it can trash items.
- **D68 — Windows Hello unlock: the key, wrapping the unlock key, the PIN, and what D45 carries over** · settled 2026-11-06 · founder (approved as proposed).
  M7.5 says "Windows Hello unlock, under D45's rules, with its key-wrapping decision first". D45 settled biometric unlock for every platform: the core seals the opened keyset under a random 32-byte unlock key (`export_biometric_unlock`, `unlock_biometric`), and the platform's keystore protects that key. On Apple it's a Secure Enclave key that decrypts (D45 §2); on Android a Keystore key that decrypts (D55 §1).

  Windows doesn't offer that. Its documented API for apps, `KeyCredentialManager`, makes a key per user, kept in the TPM, behind Windows Hello. That key can only **sign**, after a Windows Hello prompt. It can't encrypt or decrypt. So the unlock key can't simply be encrypted to it.

  Open:
  - how a signing-only key protects the unlock key
  - whether the Windows Hello PIN counts, since the API can't tell it from a face or finger
  - re-making the blob after a master password unlock
  - how the screens show it, since today's requests and labels say Touch ID
  - tests, given that CI's Windows runner has no Windows Hello

  Proposal:
  1. **The key:** one `KeyCredential` named for the account and this device, made with `RequestCreateAsync` when the user turns Windows Hello on. Windows keeps it in the TPM where there is one, never exportable. The `windows` crate's WinRT calls need no `unsafe` code.
  2. **Wrapping the unlock key: a signature as the secret.**
     - **Turning it on:** the backend makes a random 32-byte challenge and asks Windows Hello to sign it, which prompts once. The signature goes to the core, never anywhere else. The core derives a wrapping key from it with HKDF-SHA256, and seals the unlock key under that wrapping key. Both use a new AAD label, `pwm/windows-hello`, bound to the account and the device.
     - **What's kept,** in the DPAPI key store (D66 §4): the challenge, the sealed unlock key, and the blob. Never the signature, never the wrapping key.
     - **Unlocking:** Windows Hello signs the same challenge, which prompts. The core derives the same wrapping key, opens the unlock key, and then `unlock_biometric` as on the other platforms. Rust's copies of the signature and keys are `Secret`, dropped at once.
     - **Why this holds:** RSA signatures with PKCS#1 v1.5 padding, which `KeyCredential` uses, are deterministic. The same key signing the same challenge gives the same signature, and making it needs the TPM key and a Windows Hello gesture. A copied disk has the challenge and the sealed key, but not the signature.
     - **A check before relying on it:** turning it on signs the challenge twice and compares. If the signatures differ, Windows Hello unlock says it's unavailable on this computer, instead of failing later.
     - **The crypto rule:** HKDF and the seal are in the core (`pwm-crypto`, with test vectors); the platform only asks Windows for a signature. That is the platform-key role `SECURITY.md` gives Windows Hello and TPM.
  3. **The Windows Hello PIN counts.**
     - **Why:** `KeyCredentialManager` accepts whatever Windows Hello gesture the user set up: face, fingerprint or PIN. The app can't ask for biometrics only.
     - **How it differs from D45 and D55,** which accept only a biometric: Windows Hello's PIN unlocks the TPM key on this device only, with the TPM's own lockout after failed tries. It isn't the account password, and isn't shared with other devices.
     - **What the screens call it:** "Windows Hello", never "fingerprint" or "face".
  4. **Re-making the blob,** unlike D45 §4 and D55:
     - **Not after a master password unlock:** that would need a Windows Hello prompt each time, because the wrapping key comes only from a signature.
     - **After each Windows Hello unlock instead:** the wrapping key is in hand then, so the blob is made again and stays current.
     - **If the keyset changes** (rare), the old blob no longer opens. The app asks for the master password, and Windows Hello then needs turning on again. Settings says so.
  5. **Carried over from D45 unchanged:**
     - a Settings switch, "Unlock with Windows Hello", and one offer after a master password unlock
     - turning it on prompts once
     - the master password every 14 days and after signing in
     - turning it off or signing out deletes the key credential, the challenge, the sealed key and the blob; locking keeps them
     - the unlock screen tries Windows Hello once by itself, and a cancel leaves the password field with no error
     - the lock timeout is unchanged
     - **When Windows Hello isn't set up** (`IsSupportedAsync` false), the switch says so.
  6. **The screens:**
     - **The requests keep their names:** `touchId`, `setTouchId` and `unlockTouchId` become "the platform's biometric unlock". The protocol header says so.
     - **The host's `touchId` flag becomes `biometricName`:** "Touch ID" in the extension on a Mac, "Windows Hello" on the desktop, and null elsewhere (Linux until polkit/fprintd). The screens build their labels from it.
     - **The extension's Touch ID** (D48) is unchanged.
  7. **Tests:**
     - **Core (tests first):** the seal's round trip; another account's, device's or signature's seal refused; test vectors checked by Rust and the Node verifier.
     - **The backend:** behind a `HelloKey` trait (create, sign, delete, supported), with a fake that signs deterministically, can be cancelled, can be missing, or can sign differently each time. Tests cover:
       - turning it on, unlocking, a cancel, and the 14-day rule with the test clock
       - the blob re-made after a Windows Hello unlock
       - a keyset change
       - sign-out
       - a real round trip through the core against the server
     - **CI's Windows runner has no Windows Hello:** the switch says it's unavailable, and the protocol samples cover the replies.
     - **On a real computer** (`QUALITY.md`'s desktop checklist):
       - turn it on and unlock, with a face, a finger and the PIN
       - cancel
       - change the PIN
       - turn it off
     - **Parity:** Linux unlock stays the master password (polkit/fprintd later, as the roadmap says). That gap is logged here.

  Alternatives:
  - **Windows Hello as a yes/no prompt (`UserConsentVerifier`), with the unlock key in DPAPI:** simpler, but the prompt only gates the screen. Malware running as the user could open the DPAPI file without Windows Hello. The signature ties the key to the TPM and the gesture.
  - **A key from the Microsoft Passport key storage provider through NCrypt, which might decrypt:** closer to D45 and D55, but it isn't the documented route for apps, and which uses it allows isn't documented clearly. `KeyCredentialManager` is.
  - **Re-making the blob after every master password unlock with one more prompt:** it matches D45 to the letter, but adds a second prompt to every password unlock, for a keyset that almost never changes.
  - **Biometrics only, refusing the PIN:** the API can't tell them apart, so the only way to refuse the PIN would be to refuse Windows Hello altogether.

  Implementation notes (M7.5):
  - **The key credential's name** is "PWM", then the account's id, so each account on the computer has its own. `ReplaceExisting` when turning it on means a new key each time.
  - **Prompts when turning it on:** Windows may ask up to three times: to make the key, then for the two signatures compared (D68 §2). Unlocking asks once.
  - **What's kept,** as base64 in the DPAPI key store: `helloBlob`, `helloSealedKey`, `helloChallenge`. Beside them: `passwordUnlockedAt`, for the 14 days, and `helloOffered`, for the one offer.
  - **Sign-out** deletes the key credential, the three values and the 14-day timestamp. The offer's flag stays with the computer, as D45's "once" does.
  - **A signature that doesn't open the sealed key,** a key Windows reports gone, or a keyset that changed: each turns it off and answers `hello_changed`. The screen says to enter the master password and turn it on again.
  - **The host** names it with `biometricName` instead of the `touchId` flag. The extension says "Touch ID" on a Mac. The desktop says "Windows Hello" on Windows, and none on Linux.
  - **Blocking calls:** the WinRT calls wait for their answer on the request's thread, while the backend is locked anyway. A Windows Hello prompt holds other requests until it closes.
  - **Not run anywhere yet:** a real Windows Hello prompt, which needs a person at the computer. This computer reports Windows Hello as set up, and the offer showed. The tests drive the rest through a fake key.
- **D69 — Browser unlock on Windows and Linux: the helper, Windows Hello from a browser, the manifests, and Linux** · settled 2026-11-06 · founder (approved as proposed).
  M7.2 asks for "a native-messaging host for browser unlock on Windows/Linux (D48's protocol, a second binary in the desktop workspace)". D48 §5 settled how it works on a Mac:
  - The extension keeps a blob (the keyset sealed under a random key K) in its own storage.
  - A helper the browser starts keeps K behind Touch ID.
  - The protocol: `enroll`, `unlock` and `forget`, with `"v": 1`, the native-messaging framing, and the helper checking the caller's origin.
  - The app doesn't need to be signed in or running.

  Three things don't carry over:
  - **Windows Hello keys only sign** (D68). The Secure Enclave wraps K under a biometric; Windows has no such call for apps.
  - **Chrome gives a window to the host only when a page calls it.** It passes `--parent-window` with the calling window's handle, but 0 when the caller is a service worker, which is where the extension calls the helper. A Windows Hello prompt needs a window to come up in front of.
  - **Linux has no biometric unlock yet** (D68 §7: polkit or fprintd, later).

  Open:
  - where the helper lives and how it's built
  - how K is kept on Windows
  - which window the Windows Hello prompt belongs to
  - the manifests: where, and who writes them
  - Linux
  - what the extension shows
  - tests

  Proposal:
  1. **The helper:** a second crate in the desktop workspace, `pwm-browser-helper`, building `pwm-browser-helper.exe` next to the app.
     - **Small:** no Tauri and no WebView. It reads one framed message, answers, and exits, as the browser expects.
     - **D48's protocol unchanged:** `enroll {account_id, device_id, key}`, `unlock {account_id, device_id}`, `forget {account_id, device_id}`, `"v": 1`, and the same replies (`ok`, `key`, `error`). The same errors: `refused`, `invalid`, `unknown`, `cancelled`, `changed`, `unavailable`. So the extension's `TouchIdUnlock` code works unchanged.
     - **Who may call:** the origin Chrome, Edge, Brave and Opera pass as the first argument, or the extension id Firefox passes, against the same list of our extension ids as the Mac helper (placeholders until M10). Anything else is `refused`.
     - **Limits:** a message over 64 KiB is `invalid`; the helper never reads more.
     - **The protocol handler** is plain Rust behind a trait for the key and the store, so it's tested on every OS.
     - **No new crates:** `serde_json`, `pwm-crypto` and the `windows` crate the app already uses, with two more features (`Win32_System_Registry`, `Win32_Security_Cryptography` for DPAPI).
  2. **Keeping K on Windows: D68's construction, with its own label.**
     - **Enrolling:** the helper makes a Windows Hello key credential for this enrolment, named "PWM browser" and a hash of the caller, account and device. It makes a random 32-byte challenge, asks Windows Hello to sign it twice, and compares, as D68 §2 does. The core derives a wrapping key from the signature and seals K under it.
     - **Its own label:** HKDF info `pwm/browser-unlock/windows-hello/v1` and AAD `pwm/browser-unlock/windows-hello`, so a desktop unlock key and a browser enrolment's key can never stand in for each other. A new pair in `pwm-crypto` with test vectors, next to D68's.
     - **What's kept:** the challenge and the sealed K, in a file per enrolment under `%LOCALAPPDATA%\PWM\browser-unlock\`, protected with DPAPI for the user. Never the signature, never K or the wrapping key.
     - **Unlocking:** Windows Hello signs the same challenge (one prompt), the core opens K, and the helper sends K to the extension, which opens its blob. The KDF never runs.
     - **Nothing is re-made:** K doesn't change while the blob stays, so there's no D68 §4 step here.
     - **When it can't open:** a key Windows reports gone, or a signature that doesn't open K, deletes the enrolment and answers `changed`. The extension then turns it off and asks for the master password, as D48 does.
     - **The PIN counts,** as D68 §3 explains; the screens say "Windows Hello".
     - **Forget** deletes the key credential and the file.
  3. **The prompt's window.**
     - **The plan:** the helper opens a small window of its own ("Unlock PWM in your browser"), brings it to the front, and asks for the signature with it in front. The user has just clicked Unlock in the popup, so the browser is in front and hands focus on.
     - **Checked first:** whether the Windows Hello prompt shows in front this way in Chrome, Edge and Firefox. It's the first step of M7.2: a test build the founder runs on a real computer, since the prompt needs a person.
     - **If it doesn't:** the helper asks the desktop app to show the prompt, over a named pipe only this user can open, and the app must be running. That would be logged here and in Settings' text ("Keep the PWM app running").
     - **Not the popup:** the popup could call the helper with its own window handle, but K would then reach a screen, which the protocol forbids: the screens never receive keys.
  4. **The manifests: written by the desktop app, for this user only.**
     - **A switch in the desktop's Settings:** "Allow browser extensions to unlock with Windows Hello", as D48's on the Mac. Off by default.
     - **On:** the app writes two manifests under `%LOCALAPPDATA%\PWM\NativeMessagingHosts\`, one for Chromium browsers (`allowed_origins`) and one for Firefox (`allowed_extensions`), naming the helper by its full path. It registers them under `HKEY_CURRENT_USER\Software\`:
       - `Google\Chrome\NativeMessagingHosts\<name>` (Chrome, and Brave and Opera, which read Chrome's key; checked on a real computer)
       - `Microsoft\Edge\NativeMessagingHosts\<name>`
       - `Mozilla\NativeMessagingHosts\<name>`
     - **No administrator rights:** `HKEY_CURRENT_USER` only.
     - **Off:** removes the keys and the manifests, and deletes every enrolment the helper keeps, since no extension can reach it any more.
     - **The name** stays the extension's `NATIVE_HOST`, a placeholder until the stores give real ids (M10).
     - **The installers (M7.3)** remove the keys on uninstall.
  5. **Linux waits for its own biometric unlock.**
     - **Why:** the helper would have nothing to ask the user for. K in the Secret Service with no prompt is readable by any process of this user, so it would only skip the master password.
     - **When:** with the desktop's polkit or fprintd unlock (D68 §7), after M7.3, since polkit needs an action file the package installs.
     - **For M7.2:** the protocol handler builds and is tested on Linux. The binary answers `unavailable`, and the app writes no manifests there.
     - **Logged as a parity gap**, with a new task, M7.8 "Browser unlock on Linux (polkit or fprintd)", after M7.3.
  6. **The extension:**
     - **`biometricName`:** "Windows Hello" on Windows, "Touch ID" on a Mac, none elsewhere.
     - **The same requests and code:** `touchId`, `setTouchId` and `unlockTouchId`, and `TouchIdUnlock`.
     - **Its wording** names the app per platform: "Windows Hello needs the PWM app for Windows, with browser unlock turned on in its Settings." The codes `touch_id_off` and `touch_id_unavailable` get texts built from the name.
     - **D45 and D48's rules stay:** the master password every 14 days and after signing in, no offer after unlocking in the popup, and turning it off or signing out forgets the enrolment.
  7. **Tests:**
     - **Rust:**
       - the handler with a fake key and store: enroll, unlock, forget, a refused caller, cancel, changed, malformed and oversized messages, two enrolments kept apart
       - the framing
       - the manifest writer against a temporary folder and a fake registry
     - **The core:** vectors for the new seal, checked in Rust and in `verify.mjs`.
     - **Vitest:** the Windows wording and `biometricName`.
     - **CI:** the `desktop-windows` job builds the helper and runs its tests.
     - **Manual, in `QUALITY.md`:** on a Windows computer with Windows Hello, in Chrome, Edge and Firefox: turn on the app's switch, turn on unlock in the extension, lock, unlock with Windows Hello, cancel, then turn the switch off.

  Alternatives:
  - **Always go through the running desktop app:** its window makes the prompt simple, but the app must be running, unlike D48. Kept as the fallback in point 3.
  - **`UserConsentVerifier` with K under DPAPI:** an easier prompt that can take a window, but it only checks presence. K would sit where any process of this user could read it without a prompt. D68 chose signatures for this reason.
  - **The popup calls the helper:** it has a window handle, but K would reach a screen.
  - **Linux with K in the Secret Service and no prompt:** no user presence at all.
  - **Machine-wide registration (`HKEY_LOCAL_MACHINE`) by the installer:** needs administrator rights, and would offer the helper to every user of the computer.

  Implementation notes (M7.2):
  - **Three crates in the desktop workspace now:**
    - **`pwm-hello`:** D68's Windows Hello key and the DPAPI calls, moved out of the app so the helper uses the same code.
    - **`pwm-browser-helper`:** the protocol handler, the framing, the enrolment store, the manifests and the registry keys in a library; the binary reads one message, answers and exits.
    - **`pwm-desktop`:** the Settings switch (`browserUnlock`, `setBrowserUnlock`), in `browser_hosts.rs`.
  - **The core:** `seal_browser_hello` and `open_browser_hello` in `pwm-crypto`, next to D68's. The AAD `pwm/browser-unlock/windows-hello` carries the version, the caller (`len:u16 || bytes`, at most 1,024), the account and the device. Vectors in `browser_hello.json`, checked in Rust and by `verify.mjs`.
  - **The enrolment's name:** 32 hex digits from HKDF over the caller, account and device, so the key credential ("PWM browser" and the name) and the file (`<name>.bin`) show none of them. The file holds a version byte, the challenge and the envelope, sealed with DPAPI with the name as entropy.
  - **The window:** "PWM: confirm with Windows Hello to unlock your browser", in front for each prompt (making the key, the two signatures, each unlock). The Windows Hello call waits on another thread while the window keeps answering.
  - **Registry keys:** the default value of each key under `HKEY_CURRENT_USER` names the manifest. Turning it off removes only keys that still name our manifests.
  - **Turning it on** needs Windows Hello set up (`hello_unavailable` otherwise) and the helper next to the app (`browser_helper_missing`).
  - **`windows-registry`:** Microsoft's registry crate (MIT or Apache-2.0), already in the tree through `tauri-plugin-autostart`, so the registry code has no `unsafe`.
  - **The extension:** `biometricName` is "Windows Hello" on Windows. The helper's error codes keep Touch ID's names; their texts name Windows Hello and the app for Windows there.
  - **Not run anywhere yet:** a browser starting the helper and a real Windows Hello prompt, which need a person at the computer. Whether the prompt shows in front (§3) is the first thing that check answers (`QUALITY.md`, desktop step 13).
  - **Not checked here:** the Linux build, since this computer has no Linux target; CI's `desktop` job builds it.
- **D70 — Desktop installers for Windows and Linux: the formats, signing, the browser helper, uninstalling, and tests** · settled 2026-11-07 · founder (approved as proposed).
  M7.3 asks for "signed installers (MSIX/EXE, AppImage, .deb, .rpm, Flatpak)". D69 §4 adds two duties: install `pwm-browser-helper.exe` next to the app, and remove the browser helper's registry keys on uninstall. D66 kept the Tauri CLI out until now ("it comes only with the installers").

  Open:
  - which formats, since some of the listed ones break D69 or carry licence duties
  - who signs, with what, and when
  - how the helper gets into the package
  - what uninstalling removes
  - updates
  - tests, and what runs on this computer versus CI

  Proposal:
  1. **Windows: one per-user `.exe` installer (NSIS), built by Tauri's bundler.**
     - **Per user, no administrator rights:** it installs under `%LOCALAPPDATA%\Programs\PWM`, the same scope as D69's `HKEY_CURRENT_USER` keys and D66's data folder.
     - **Not MSIX for now:** an MSIX app's writes to `HKEY_CURRENT_USER` and `%LOCALAPPDATA%` are redirected into the package's own copy, so Chrome, Edge and Firefox would never see the manifests D69 writes. MSIX comes back only with the Microsoft Store, with its own decision.
     - **Not MSI:** WiX installs per machine and needs administrator rights.
     - **Compression:** zlib. NSIS itself is zlib/libpng, on the S6 list. Its LZMA and bzip2 modules carry their own terms, so they stay out of the stub. Tauri's `nsis_tauri_utils` plugin is MIT or Apache-2.0.
  2. **Linux: `.deb`, `.rpm` and a Flatpak now. AppImage waits.**
     - **`.deb` and `.rpm`** from Tauri's bundler, built on Ubuntu 22.04 (the WebKitGTK floor, D66 §9). WebKitGTK, GTK and GLib come from the system as dependencies, linked dynamically, as D3 allows.
     - **The Flatpak** runs on the GNOME runtime, which carries WebKitGTK, so we ship none of it. The Secret Service works through the Secret portal (`oo7`, D66). It packages our release build. Publishing it (Flathub, or our own repository) comes at release (M10).
     - **AppImage waits:** it would bundle WebKitGTK, GTK and their LGPL libraries inside our file. That's allowed, but each release would then owe the exact sources of those libraries beside it. Logged as a gap until the founder wants that duty.
     - **No browser helper on Linux yet** (M7.8). Polkit's action file joins the `.deb`, `.rpm` and Flatpak with M7.8.
  3. **Signing: wired now, the founder's identity at release, as D48 §3 did for the Mac.**
     - **Windows:** Authenticode on `pwm-desktop.exe`, `pwm-browser-helper.exe` and the installer, through Tauri's `signCommand`, which runs one command per file.
       - **Recommended:** Microsoft's Artifact Signing (Trusted Signing). The keys stay in Microsoft's HSMs, with no token to keep, for about US$10 a month. It needs the founder's Azure account and an identity check of the company (open to EU organisations).
       - **Alternative:** an OV certificate from a CA on its cloud HSM, about €300 or more a year. EV no longer earns SmartScreen trust faster (since 2024), so it isn't worth its price.
     - **Linux:** a release GPG key, its primary key kept offline by the founder and a signing subkey in CI's secrets.
       - **`.rpm`:** signed inside the package (`rpmsign`).
       - **`.deb`:** signed through the repository, as Debian does, once there is one (M10.1).
       - **Every file:** a `SHA256SUMS` with a detached signature.
       - **Flatpak:** its repository is GPG-signed when it's published.
     - **Before release:** CI signs with a throwaway certificate and a throwaway GPG key it makes on each run, then verifies. So the signing path is tested on every run, and the real identity only changes the secrets. Release builds stay unsigned until the secrets exist.
  4. **The helper in the package.**
     - **Windows:** Tauri's `externalBin` copies `pwm-browser-helper.exe` next to `pwm-desktop.exe`, where the app looks for it (`current_exe().with_file_name`, D69).
     - **Only on Windows:** set in `tauri.windows.conf.json`, merged over the shared config.
     - **The build order:** a `just desktop-installer` recipe builds the helper in release, copies it in under the name Tauri expects (with the target triple), then runs `tauri build`.
  5. **Uninstalling.**
     - **The helper cleans up after itself:** the uninstaller first runs `pwm-browser-helper.exe --uninstall`. It does what the Settings switch does when turned off (D69 §4):
       - removes the three keys, only where they still name our manifests
       - removes the manifests
       - deletes every enrolment and its Windows Hello key, which needs no prompt
     - **One place for that code:** the cleanup moves from the app's `browser_hosts.rs` into the helper's library, so the switch and the uninstaller run the same code.
     - **A browser can't trigger it:** browsers pass an origin or a manifest path, never `--uninstall`.
     - **The app's own data stays** unless the user ticks Tauri's "Delete the application data" box, which removes `%LOCALAPPDATA%\invalid.pwm.desktop`. D68's Windows Hello key is useless without that data.
     - **A running app** is asked to close first (Tauri's check).
  6. **Updates:** none in M7.3. A new version installs over the old one and keeps the data. Automatic updates need their own signing key and the release channels, so they come with M10.1.
  7. **The Tauri CLI:** `@tauri-apps/cli` (MIT or Apache-2.0), pinned as a dev dependency of `@pwm/desktop`. As dev tooling it's outside D9's npm check, but its licence is on the list anyway.
  8. **Names:** "PWM" and `invalid.pwm.desktop` stay placeholders until D1, as everywhere else. The installer's publisher field reads "PWM".
  9. **Tests, all in CI:**
     - **Rust:** `--uninstall` against a fake registry and store: our keys and files go, while another program's key with our name stays. The switch's existing tests keep passing on the moved code.
     - **A new `desktop-installers` job on Windows:**
       - Build the installer, sign it with the throwaway certificate, and verify all three signatures.
       - Install it silently for the user, and check the app and the helper are side by side.
       - Write the three keys as the switch would, uninstall silently, and check the keys, the manifests and the install folder are gone.
     - **On Ubuntu 22.04:**
       - Build the `.deb` and `.rpm`. Sign the `.rpm` with the throwaway key and check it (`rpm -K`).
       - Install the `.deb` with `apt`, and check its dependencies resolve and the app starts under Xvfb.
       - Build the Flatpak with `flatpak-builder` and start it.
     - **When it runs:** on pull requests that touch the desktop app (the `desktop-changes` filter), like the other desktop jobs.
     - **The artifacts:** each run keeps the installers for 7 days, for a manual check on a real computer.
     - **Manual, in `QUALITY.md`:** on a real Windows computer, install, turn on browser unlock, uninstall, and check the browsers no longer find the helper. On Ubuntu and Fedora, install the package and use the app.
  10. **Downloads:**
      - **This computer:** `@tauri-apps/cli` from npm. Installers are built and tested only in CI, so this computer doesn't download NSIS or install anything.
      - **CI:** Tauri's bundler fetches NSIS and its plugin from Tauri's releases on GitHub. The Linux job gets `rpm`, `flatpak-builder` and the GNOME runtime from Ubuntu and Flathub.

  Alternatives:
  - **MSIX:** redirection breaks D69's manifests and keys. For the Store, later.
  - **MSI:** per machine, needs administrator rights.
  - **AppImage now:** it bundles LGPL libraries, and each release then owes their sources.
  - **Signing only at release, nothing before:** the first real signing would also be the first test of the signing path.
  - **Deleting the keys in the uninstaller's own NSIS script:** a second copy of D69's rules in another language, and NSIS can't delete Windows Hello keys.

  Implementation notes (M7.3):
  - **`just desktop-installer`** (`apps/desktop/web/scripts/installer.mjs`): builds the page, then on Windows the helper in release, copied in as `src-tauri/binaries/pwm-browser-helper-<target triple>.exe`, then `tauri build` with `installer.conf.json` (and `installer.windows.conf.json` on Windows). Extra arguments go to `tauri build`, such as `--config sign.json`.
  - **The settings stay out of `tauri.conf.json`:** `tauri-build` checks `externalBin` on every `cargo build`, so only the installer build passes them.
  - **The install folder is `%LOCALAPPDATA%\PWM`,** Tauri's per-user default, not `%LOCALAPPDATA%\Programs\PWM` as §1 said. The template has no setting for it. That's also where D69 keeps the manifests and enrolments. The uninstaller removes only the files it installed, then the folder if it's empty, and `--uninstall` has emptied its subfolders by then.
  - **Replacing a version keeps browser unlock:** Tauri's updater passes `/UPDATE`, and an installer of another version runs the old uninstaller in place, from the install folder. The hook (`installer-hooks.nsh`) runs `--uninstall` only when neither is the case. Uninstalling from Windows' Settings runs a copy from the temporary folder.
  - **The cleanup moved:** `BrowserHosts` is now in `pwm-browser-helper` (`hosts.rs`), with the registry and `for_this_user`; `browser_hosts.rs` in the app maps its errors to the screens' codes. Turning it off also removes the manifests' folder, and `--uninstall` the enrolments' folder, once each is empty.
  - **The Flatpak** (`apps/desktop/flatpak/`) takes the release build from the `.deb`, on GNOME 50. Permissions: network, Wayland or X11, the GPU, Downloads (the Emergency Kit), and logind on the system bus (locking).
  - **No tray in the Flatpak:** the GNOME runtime has no appindicator library, and Tauri's tray panics when it's missing, which CI's first Flatpak run caught. The app now treats that as no tray, as on GNOME. Bundling the library would bring LGPL code into our package (§2). The `.deb` depends on it.
  - **CI:** `desktop-installer-windows` and `desktop-installer-linux`, on the `desktop-changes` filter, keep the installers for 7 days.
  - **Not run on this computer:** the installers themselves, by §10. Building with the CLI (`--no-bundle`) checked the settings and the helper's copy.
- **D71 — Unlock on Linux with polkit, for the desktop app and the browser extensions: a root key holder, the construction, packaging, and tests** · proposed 2026-11-07 · awaiting founder.
  M7.8 asks for "browser unlock on Linux (D69 §5): the helper asks polkit or fprintd, with the desktop's own unlock (D68 §7)". D69 §5 explains why it waited: on Linux a key the helper keeps in the Secret Service or a file is readable by any process of this user without a prompt, so it would only skip the master password. The platforms so far rest on a key the user's processes can't read: the Secure Enclave (D45, D48) and the TPM behind Windows Hello (D68, D69).

  What Linux offers:
  - **polkit** asks the user to confirm with their login password, or a fingerprint where PAM is set up for it (`pam_fprintd`), in the desktop's own dialog (GNOME, KDE and the others ship an agent). It answers yes or no and holds no secret.
  - **fprintd** checks a finger and also holds no secret, and it isn't on every computer. polkit already uses it where it's set up.
  - **The boundary between users:** a file only root can read is out of reach of this user's processes.

  Open:
  - how a key gets a gesture in front of it
  - the construction, and whether D68 and D69 carry over
  - which computers, browsers and packages
  - the screens' wording
  - packaging and uninstalling
  - tests

  Proposal:
  1. **A root key holder behind polkit: `pwm-keyholder`.** A small binary installed at `/usr/libexec/pwm/pwm-keyholder`, run only through `pkexec`.
     - **A polkit action, `invalid.pwm.unlock`** (a placeholder until D1): `auth_self` for the active session, with no remembered answer, so each call shows the dialog ("Confirm it's you to unlock PWM"). Inactive and remote sessions are refused.
     - **What it keeps:** for each name, a random 32-byte secret S in `/var/lib/pwm/keys/<uid>/`, readable only by root, the user taken from `PKEXEC_UID`. It never returns S.
     - **Two commands:**
       - `create <name>` makes or replaces S.
       - `sign <name>` reads a 32-byte challenge on stdin and answers `HKDF-SHA256(ikm = S, salt = challenge, info = "pwm/linux-polkit/response/v1")`.
     - **So it works like a Windows Hello key:** a gesture, then a fixed answer for a fixed challenge, from a secret the user's processes can't read. The answer is computed in `pwm-crypto` (a new `linux_polkit_response`), so no crypto lives in the app.
     - **No delete command:** turning it off deletes what the user's side keeps (the challenge and the sealed key), after which S opens nothing. The packages remove `/var/lib/pwm` on purge. That saves a prompt on every sign-out.
     - **Checks:** it runs only as root with `PKEXEC_UID` set. A name is 32 hex digits. The folder is made 0700 and the files 0600, written atomically. Nothing is logged.
  2. **D68 and D69 carry over unchanged, with Linux's own labels.**
     - **The desktop's unlock (D68):** a `PolkitKey` implements the `HelloKey` trait through `pkexec`. Turning it on makes S and signs twice (three dialogs, as on Windows), and each unlock signs once.
     - **The browser helper (D69):** the same handler and protocol, with `PolkitKey` instead of Windows Hello.
     - **Its own labels,** so no key can stand in for another platform's: info `pwm/linux-polkit/v1` with AAD `pwm/linux-polkit`, and info `pwm/browser-unlock/linux-polkit/v1` with AAD `pwm/browser-unlock/linux-polkit`. The core's D68 and D69 functions take the platform as a parameter. Windows' labels and vectors don't change, and Linux gets new vectors.
     - **"Changed":** an answer that doesn't open the sealed key (S lost, say after a purge) deletes the enrolment and asks for the master password, as D68 and D69 do.
     - **D45's rules stay:** the master password every 14 days and at each sign-in.
  3. **What it guards against, written into `SECURITY.md`:**
     - **Guarded:** this user's processes can't read S. A copied home folder or browser profile holds only the challenge and the sealed key. Every unlock shows polkit's dialog.
     - **Not guarded:**
       - root
       - a disk image of a computer without full-disk encryption, which holds both halves
       - malware running as the user that makes the call, which still needs the user's gesture in the dialog (as D48 says for the Mac)
     - **No enrolment check:** polkit can't tell a new fingerprint was added, unlike `.biometryCurrentSet`.
     - **Weaker than the TPM, said plainly in the docs.** A TPM-sealed variant can come later where a TPM exists, with its own decision.
  4. **Where it works:**
     - **Packages:** the `.deb` and `.rpm` (D70), which install the key holder and the action.
     - **Not in the Flatpak:** a sandbox can't run `pkexec`. The switch doesn't show there, and the gap is logged.
     - **Browsers that are packages too:** Chrome, Chromium, Edge, Brave, Vivaldi and Firefox. The switch writes each one's manifest under the user's home, only for browsers whose profile folder exists:
       - `~/.config/google-chrome/`, `chromium/`, `microsoft-edge/`, `BraveSoftware/Brave-Browser/` and `vivaldi/`, each with `NativeMessagingHosts/`
       - `~/.mozilla/native-messaging-hosts/`
       - Opera's folder gets checked on a real computer.
     - **Not Snap or Flatpak browsers:** they can't start a helper on the computer. Firefox's portal for this is too young. Logged as a gap.
     - **Without a polkit agent** (a bare window manager), `pkexec` fails: the switch says "unavailable" and the master password stays.
  5. **The screens:**
     - **The name:** `biometricName` is "Linux login" on Linux (the desktop, and the extension when the user agent says Linux and not Android). Labels read "Unlock with Linux login".
     - **Linux's texts:** like Windows' (D69 §6), naming the PWM app for Linux. "Linux login" is the founder's to change.
     - **The desktop's switch:** "Allow browser extensions to unlock with Linux login", as on Windows.
  6. **Packaging (D70):**
     - **The `.deb` and `.rpm` add three files:**
       - `/usr/libexec/pwm/pwm-keyholder`
       - `/usr/share/polkit-1/actions/invalid.pwm.unlock.policy`
       - `pwm-browser-helper` next to the app in `/usr/bin`
     - **Removing** (`.deb` purge, `.rpm` erase) deletes `/var/lib/pwm`.
     - **Left behind:** the manifests in each home folder, which name a helper that's gone, so the extension says it needs the app.
  7. **No new crates:** `hkdf` and `getrandom` through `pwm-crypto`, and `std`. The key holder finds its user from `PKEXEC_UID` and checks it's root from `/proc/self`'s owner, so no `unsafe` and no `libc`.
  8. **Tests:**
     - **Core:** `linux_polkit_response` and the Linux labels, with new vectors in Rust and `verify.mjs`.
     - **`pwm-keyholder`:** over a temporary folder, with the root check injected: names, determinism, users kept apart, and replacing on `create`.
     - **`PolkitKey`:** with a fake `pkexec`.
     - **The Linux manifests:** written and removed against a temporary home.
     - **The extension and the screens:** the wording.
     - **CI (`desktop-linux`):**
       - Install the key holder and the action, plus a polkit rule that allows the runner without a dialog (a `.pkla` file, since Ubuntu 22.04 has polkit 0.105).
       - Through the real `pkexec`: create, sign twice and compare, then a browser enrolment and unlock through the helper's framing.
     - **CI (`desktop-installer-linux`):** the `.deb` has the three files, and removing it with purge deletes `/var/lib/pwm`.
     - **Manual, in `QUALITY.md`:** on Ubuntu (GNOME) and Fedora (KDE), with a password and with a fingerprint: the desktop's unlock and browser unlock in Chrome and Firefox, a cancelled dialog, and turning it off.

  Alternatives:
  - **The key in the Secret Service or a file, behind a polkit check in our own process:** any process of the user reads it without the dialog (D69 §5).
  - **fprintd directly:** no secret, not on every computer, and polkit already uses it.
  - **A system D-Bus service checking polkit itself:** the peer's user comes straight from the bus, but it needs a service file and a systemd unit. `pkexec` is enough for a few calls a day.
  - **TPM 2.0 sealing:** hardware-backed, but not on every computer, with no gesture of its own, and much more code. Kept for later.
  - **The same labels as Windows:** fewer core changes, but the names would lie about where a key came from.
