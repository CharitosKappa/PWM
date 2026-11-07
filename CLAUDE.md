# CLAUDE.md — Password Manager (codename: PWM)

You are building a cross-platform, zero-knowledge password manager: iOS, Android, Windows, macOS, Linux and browser extensions (Chrome, Edge, Firefox, Safari, Brave, Opera).
"PWM" is a placeholder codename; rename when the brand is chosen.

Read these before writing code:

| File | What it holds |
| --- | --- |
| `docs/PRODUCT.md` | Positioning, promises, feature scope with priorities |
| `docs/ARCHITECTURE.md` | Monorepo layout, stack per platform, data flow |
| `docs/SECURITY.md` | Crypto spec, key hierarchy, threat model — **normative** |
| `docs/AUTOFILL.md` | Autofill engine, extension security rules, site test harness |
| `docs/QUALITY.md` | Testing, CI, release safety, metrics |
| `docs/PRICING.md` | Plans and trust commitments the billing code must enforce |
| `docs/ROADMAP.md` | Milestones and tasks with acceptance criteria — **work from here** |
| `docs/DECISIONS.md` | Open and settled decisions; log new ones here |
| `docs/COMPETITORS.md` | Why each feature exists (competitor strengths, gaps, bad reviews) |
| `docs/DEVELOPMENT.md` | Local toolchain setup and everyday commands |
| `docs/DEPLOYMENT.md` | Server image, settings, TLS proxy, backups and restore |

## How to work

1. Pick the first unchecked task in `docs/ROADMAP.md` whose dependencies are done.
2. Read the docs it references. If a spec is ambiguous, propose the decision in `docs/DECISIONS.md` and ask before building on it.
3. Write tests first for anything in `core/` (unit + test vectors).
4. Implement, run the checks below, then tick the task and note anything deferred.
5. One task per commit/PR. Commit message: `M<n>.<task>: <summary>`.

## Non-negotiable rules

- **All crypto lives in `core/` (Rust).** Platform code never implements crypto, key derivation or vault parsing; it calls the core through FFI/WASM.
- **The server never sees plaintext**: no item names, URLs, usernames, folder names or tags. Only ciphertext, sizes, ids and timestamps.
- **Never log secrets.** No passwords, keys, vault contents or form values in logs, crash reports, analytics or test snapshots. Use the `Secret<T>` wrapper (zeroize on drop, redacted `Debug`).
- **Closed source** (decision S6). Only add dependencies with permissive licences on the allow-list in `docs/DECISIONS.md`; `cargo deny` / `license-checker` must pass. Never copy or adapt code from GPL/AGPL projects (Bitwarden, KeePassXC, Proton Pass, KeePass) — implement from public specs.
- **Use vetted crates only** for primitives (RustCrypto `chacha20poly1305`, `argon2`, `hkdf`, `x25519-dalek`, `ed25519-dalek`, `opaque-ke`, `zeroize`, `p256` + `sha2` for passkeys (D37), and `ring` for the server's RS256 tokens to Google only: FCM and Play (D60, D6)). Never hand-roll primitives.
- **Autofill fills only after an explicit user action** (click/tap/shortcut) by default. Never inject clickable credential UI into page DOM (see `docs/AUTOFILL.md`).
- **No feature removal and no price change logic** without the notice rules in `docs/PRICING.md`.
- **Export is always available on every plan**, encrypted or (with re-auth + warning) plaintext.
- **Formats are versioned.** Every on-disk/on-wire structure carries a version; every change ships with a migration and a migration test.
- Platform parity: a user-visible feature is not "done" until it works on all platforms in its milestone, or the gap is logged in `docs/DECISIONS.md`.

## Commands (create these in M0)

```
just check        # fmt + clippy + lint for all packages
just test         # all unit tests (core, server, extension)
just test-vectors # crypto test vectors
just autofill     # Playwright autofill harness against fixture sites
just server       # run sync server locally (docker compose: postgres + server)
just audit        # cargo deny + npm licence allow-list + pnpm audit
just notices      # generate THIRD_PARTY_NOTICES/
```

## Style

- Rust: stable toolchain, `#![forbid(unsafe_code)]` in all core crates except `pwm-ffi`, `clippy -D warnings`, errors via `thiserror`.
- TypeScript: strict mode, no `any`, ESLint + Prettier.
- Swift/Kotlin: platform style guides; UI follows the shared design tokens in `design/tokens.json`.
- Keep UI copy plain and short. No dark patterns: no nag prompts during autofill, no UI that shifts under the cursor.
