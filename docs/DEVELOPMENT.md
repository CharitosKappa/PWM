# Development setup

## Tools

| Tool | Version | Used for |
| --- | --- | --- |
| Rust (rustup) | stable (see `rust-toolchain.toml`), with rustfmt + clippy | core, server |
| just | 1.x | task runner (`just --list`) |
| Node.js | ≥ 22 | extension, scripts |
| pnpm | version in `package.json` `packageManager` | JS workspace |
| cargo-deny | 0.20+ | `just audit` |
| cargo-about | 0.9+ (install with `--features cli`) | `just notices` |
| Docker | any recent | `just db` (Postgres for server tests and `just server`) |
| wasm32 target | `rustup target add wasm32-unknown-unknown` | `just wasm` |
| wasm-bindgen-cli | the version in `Cargo.lock` (`cargo install wasm-bindgen-cli --version 0.2.130 --locked`) | `just wasm` |

## Windows

The local Windows setup uses the GNU Rust toolchain, which needs MinGW-w64 for `dlltool` and C dependencies:

```
winget install Rustlang.Rustup Casey.Just BrechtSanders.WinLibs.POSIX.MSVCRT
rustup set default-host x86_64-pc-windows-gnu
rustup toolchain install stable --component rustfmt clippy
cargo install --locked cargo-deny
cargo install --locked cargo-about --features cli
```

Restart the terminal afterwards so the new `PATH` entries (cargo, just, WinLibs `mingw64\bin`) are picked up. Windows release builds (Tauri desktop, FFI) will need the MSVC toolchain; add it when M7 starts.

## Everyday commands

```
just check         # fmt + clippy + eslint + prettier + tsc
just test          # all unit tests
just test-vectors  # crypto test vectors
just audit         # cargo deny (advisories, bans, sources, licences) + npm licences + pnpm audit
just notices       # THIRD_PARTY_NOTICES/ for shipping
just fmt           # apply rustfmt + prettier
```

The repo uses LF line endings everywhere (`.gitattributes`); format checks fail on CRLF.

## Server and database tests

The server's integration tests need Postgres (D25). Each test creates and drops its own database.

```
just db            # disposable postgres:17 container on 127.0.0.1:54329 (`docker stop pwm-test-db` removes it)
just test-server   # server tests against it; fails if it is not running
just db-tls        # optional: Postgres with TLS on 127.0.0.1:54330, for the TLS test (see the recipe)
```

To run the server itself, use docker compose: Postgres plus the server image, on `http://127.0.0.1:8080` (D30).

```
just secrets       # once: writes .env with new PWM_SERVER_SECRETS (gitignored; an existing .env is kept)
just server        # docker compose up --build
just smoke         # in another terminal: register, log in and read the account against it
```

`docker compose down -v` stops the stack and deletes its database. Deployment, TLS and backups are in `docs/DEPLOYMENT.md`.

`just test` also runs the server tests, but skips the database ones unless `PWM_TEST_DATABASE_URL` is set; CI's `server-test` job always runs them. After adding a migration, regenerate the schema snapshot with `PWM_UPDATE_SNAPSHOT=1 just test-server` and review the diff. Never edit a merged migration: the server refuses a database whose applied migrations differ.

Server settings: `PWM_DATABASE_URL` and `PWM_SERVER_SECRETS` (both required), `PWM_LISTEN` (default `127.0.0.1:8080`), `PWM_TRUSTED_PROXIES` (proxies whose `X-Forwarded-For` gives the client IP for rate limits, such as `10.0.0.0/8`; default none), `PWM_LOG` (log filter, default `info,sqlx=warn`) and `PWM_LOG_FORMAT` (`json` or `text`; JSON unless writing to a terminal). Every log line goes through the scrubber (D29).

`PWM_SERVER_SECRETS` holds the OPAQUE server setup and the pre-login key (D26). Create it once with `just secrets` (or `cargo run -p pwm-server -- keygen`) and keep it: accounts registered under one value cannot log in under another. In production it lives in a secret manager with a backup; losing it locks every account out.

## Core bindings

Apps and the extension use the core through `pwm-client` (D31): every server conversation is a series of steps the platform sends with its own HTTP stack.

```
just wasm          # WASM build into extension/core (gitignored) and the Node smoke test
just bindings      # Swift and Kotlin bindings into target/bindings
```

The autofill engine vendors the Public Suffix List (D33). Refresh it before a release with `just psl` (downloads the list and its tests, records the version and SHA-256) and review the diff.

## Browser extension

```
just extension     # just wasm, then build extension/dist/chromium/ and dist/firefox/ with a zip of each (checks each manifest and that no remote code is bundled)
pnpm --filter @pwm/extension test        # unit tests with fake chrome APIs (part of just test)
pnpm --filter @pwm/extension test:wasm   # tests over the real WASM core (part of just wasm)
```

The autofill harness (D35) runs the extension's test build in Chromium against fixture pages:

```
just server       # in another terminal: the sync server on http://127.0.0.1:8080
just autofill     # test build into extension/dist-test/, Playwright's Chromium, the fixtures
```

Load `extension/dist/chromium/` unpacked from `chrome://extensions` (developer mode); the same folder loads in Edge (`edge://extensions`), Brave and Opera. In Firefox, open `about:debugging#/runtime/this-firefox`, choose "Load Temporary Add-on" and pick `extension/dist/firefox/manifest.json`; Firefox asks for server access when you sign in. The Firefox smoke test needs geckodriver: `pnpm --filter @pwm/autofill-harness firefox-smoke extension/dist/pwm-firefox-<version>.zip`. The fill shortcut is Ctrl+Shift+L (⌘⇧L); change it at `chrome://extensions/shortcuts`. The hosted server address is a placeholder until D1. For local work, open the extension's options while signed out, choose "Self-hosted" and enter `http://localhost:8080` (run `just server`).

**Rules signing keys (D34).** The founder creates them on an offline machine and keeps the seeds offline; only the public keys enter the repo:

```
just rules-keygen /media/offline/rules-current.seed   # prints the public key (hex)
cargo run -p pwm-rules-sign -- sign <seed> <key-id> rules.json autofill-rules.bin
```

CI also compiles and runs small Swift and Kotlin programs against the bindings (`core/pwm-ffi/smoke/`). Locally, Swift can run in the `swift` Docker image on the Linux build of the library.

## Apple: the shared Swift package

`apps/apple/PWMKit` (D41) is the Swift package that the iOS app, its AutoFill extension and the macOS app share. On a Mac with Xcode and Apple Silicon:

```
brew install xcodegen
just apple     # the core as an XCFramework into apps/apple/PWMKit/Generated/, swift test, then apps/apple/PWM.xcodeproj
just tokens    # regenerate Sources/PWMDesign/Tokens.swift after changing design/tokens.json
```

- **The iOS app:** open `apps/apple/PWM.xcodeproj`, choose the `PWM` scheme and an iPhone simulator, then run.
  - The project is generated from `apps/apple/project.yml`; change that file, not the project.
  - **A self-hosted or local server:** choose "Other server" on the welcome screen. For a local server, enter `http://127.0.0.1:8080` after `just server`.
  - **A real device:** copy `apps/apple/Config/Local.example.xcconfig` to `Local.xcconfig` (gitignored) and set your team id. Simulators need no team.
- **The UI test:** start `just server`, then in `apps/apple` run `TEST_RUNNER_PWM_TEST_SERVER=http://127.0.0.1:8080 xcodebuild test -project PWM.xcodeproj -scheme PWM -destination 'platform=iOS Simulator,name=iPhone 16'`. It is skipped without the variable.
- **The macOS app:** the `PWMMac` scheme, run on "My Mac".
  - **Without a team** it is signed to run locally, which can't carry the Keychain group, App Group or AutoFill entitlements. Build it as CI does, with `CODE_SIGN_ENTITLEMENTS=Mac/PWMMac-CI.entitlements` (no entitlements, not sandboxed): it then keeps its own login-keychain items and cache, and Touch ID and AutoFill are off.
  - **With your team** in `Local.xcconfig` (it also sets the Mac signing identity), everything works, including the credential provider.
  - **The Safari extension:** run `just extension` first; the `PWMSafari` target copies `extension/dist/safari/` into the app. Without it the app still builds, with a warning, and Safari gets no extension. To try an unsigned build, allow unsigned extensions in Safari's Develop settings.
  - **Its UI test:** `TEST_RUNNER_PWM_TEST_SERVER=http://127.0.0.1:8080 xcodebuild test -project PWM.xcodeproj -scheme PWMMac -destination 'platform=macOS' CODE_SIGN_ENTITLEMENTS=Mac/PWMMac-CI.entitlements`.

- **Before Xcode opens the package:** run `just apple` first, after every core change. The package does not resolve without `Generated/`.
- **The server tests** run when `PWM_TEST_SERVER=http://127.0.0.1:8080` is set (run `just server`), and are skipped otherwise.
- **On Linux** (as in CI):
  1. `bash apps/apple/PWMKit/generate.sh linux`
  2. In `apps/apple/PWMKit`, run `swift test -Xlinker -L$PWD/../../../target/debug`, with `LD_LIBRARY_PATH` set to the same folder.
- **On Windows:** the package does not build natively. It does build in the `swift` Docker image, on the Linux build of the core (generate it in a `rust` container first). The app and the views build only on a Mac, so CI runs them.

## Android

`apps/android` (D52) is a Gradle project: `:core` (the generated bindings), `:kit` (the Kotlin layer, plain JVM) and `:app` (Compose). It builds and runs on Windows, macOS and Linux with Android Studio's JDK, the Android SDK with NDK 28, the Rust targets and `cargo-ndk`; `apps/android/README.md` has the setup.

```
just android          # :kit's JVM tests, a debug build, the licence check
just android-device   # the app's tests on a running emulator or device
```

`:kit`'s tests call the core through JNA on the host build of `pwm-ffi`; with `PWM_TEST_SERVER=http://127.0.0.1:8080` and `just server` running, its server test signs up, syncs and signs in on a second session.

## Fuzzing

Targets live in `core/pwm-crypto/fuzz/fuzz_targets/` (`envelope_decode`, `vault_share_decode`, `keyset_plaintext`, `secret_key_parse`, `password_normalize`, `totp_parse`, `export_read`, `opaque_messages`, `autofill_rules`, `url_match`, `kit_qr_parse`). CI runs each for 60 s on every PR and 10 minutes nightly; crash inputs are uploaded as artifacts.

Linux/macOS (nightly + `cargo install cargo-fuzz`):

```
just fuzz envelope_decode 600
```

Windows (libFuzzer does not support the GNU toolchain), from the repo root in Git Bash with Docker Desktop running:

```
MSYS_NO_PATHCONV=1 docker run --rm -v "$PWD:/src" -e CARGO_TARGET_DIR=/tmp/target rust:latest bash -c \
  'rustup toolchain install nightly --profile minimal && cargo install cargo-fuzz --locked &&
   cd /src/core/pwm-crypto/fuzz && cargo +nightly fuzz run envelope_decode -- -max_total_time=600'
```

The fuzz crate is its own workspace so libFuzzer (Apache-2.0 WITH LLVM-exception, test-only) stays out of the shipped dependency graph that `cargo deny` checks.
