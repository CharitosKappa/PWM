# Task runner for the whole monorepo. Run `just` to list recipes.

set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# fmt + clippy + lint for all packages
check:
    cargo fmt --all --check
    cargo fmt --manifest-path core/pwm-crypto/fuzz/Cargo.toml --check
    cargo fmt --manifest-path apps/desktop/Cargo.toml --all --check
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    pnpm install --frozen-lockfile
    pnpm format:check
    pnpm lint
    pnpm typecheck
    node scripts/tokens-swift.mjs --check
    node scripts/tokens-kotlin.mjs --check
    node scripts/messages.mjs --check
    node scripts/fonts.mjs --check

# all unit tests (core, server, extension, shared screens)
test:
    cargo test --workspace --all-features --locked
    pnpm install --frozen-lockfile
    pnpm test

# crypto test vectors: Rust checker (tests named `vectors_*`) + independent Node check
test-vectors:
    cargo test --workspace --all-features --locked vectors
    node core/test-vectors/verify.mjs

# Playwright autofill harness (D35): the test build of the extension in Chromium against fixture sites; needs `just wasm` and `just server` running
autofill:
    pnpm install --frozen-lockfile
    pnpm --filter @pwm/extension build:test
    pnpm --filter @pwm/autofill-harness exec playwright install chromium
    pnpm --filter @pwm/autofill-harness harness

# the top-sites runner (D39) against real login pages; writes tests/autofill-harness/autofill-report.json. Same needs as `just autofill`
top-sites:
    pnpm install --frozen-lockfile
    pnpm --filter @pwm/extension build:test
    pnpm --filter @pwm/autofill-harness exec playwright install chromium
    pnpm --filter @pwm/autofill-harness top-sites

# run the sync server locally: docker compose (Postgres + the server image) on http://127.0.0.1:8080; needs `just secrets` once
server:
    docker compose up --build

# write .env with new server secrets for `just server` (an existing .env is kept)
secrets:
    cargo run -q -p pwm-server -- keygen --env .env

# register, log in and read the account against a running server (default: `just server`)
smoke url="http://127.0.0.1:8080":
    cargo run -q -p pwm-server --example smoke -- {{url}}

# back up the `just server` database to a new file (D76 §3)
backup file:
    sh scripts/backup.sh {{file}}

# restore a backup into the `just server` stack, then mark it restored (D76 §3)
restore file:
    sh scripts/restore.sh {{file}}

# Postgres 17 with TLS on (self-signed certificate) for the TLS test: PWM_TEST_TLS_DATABASE_URL=postgres://pwm:pwm@127.0.0.1:54330/postgres
db-tls:
    docker run --rm -d --name pwm-test-tls -p 127.0.0.1:54330:5432 -e POSTGRES_USER=pwm -e POSTGRES_PASSWORD=pwm postgres:17 -c ssl=on -c ssl_cert_file=/etc/ssl/certs/ssl-cert-snakeoil.pem -c ssl_key_file=/etc/ssl/private/ssl-cert-snakeoil.key

# disposable Postgres 17 for the server's database tests (`docker stop pwm-test-db` removes it)
db:
    docker run --rm -d --name pwm-test-db -p 127.0.0.1:54329:5432 -e POSTGRES_USER=pwm -e POSTGRES_PASSWORD=pwm postgres:17-alpine

# server tests against the `just db` container; fails instead of skipping when it is not running
test-server $PWM_TEST_DATABASE_URL="postgres://pwm:pwm@127.0.0.1:54329/postgres" $PWM_REQUIRE_DB="1":
    cargo test -p pwm-server --locked

# WASM build of the core (D31): extension/core for the extension, target/wasm/node for the Node smoke test and the extension's WASM tests
wasm:
    cargo build --locked --release -p pwm-wasm --target wasm32-unknown-unknown
    wasm-bindgen --target web --out-dir extension/core target/wasm32-unknown-unknown/release/pwm_wasm.wasm
    wasm-bindgen --target nodejs --out-dir target/wasm/node target/wasm32-unknown-unknown/release/pwm_wasm.wasm
    node core/pwm-wasm/smoke/smoke.mjs target/wasm/node/pwm_wasm.js
    pnpm install --frozen-lockfile
    pnpm --filter @pwm/extension test:wasm

# the browser extension (D34, D40) into extension/dist/chromium and dist/firefox, with a zip of each
extension: wasm
    pnpm --filter @pwm/extension build

# the desktop app for Windows and Linux (D66): its page, then the app, run
desktop:
    pnpm --filter @pwm/desktop build
    cargo run --manifest-path apps/desktop/Cargo.toml --locked -p pwm-desktop

# the desktop installers (D70): the NSIS installer on Windows, the .deb and .rpm on Linux,
# in apps/desktop/target/release/bundle; extra arguments go to `tauri build`
desktop-installer *ARGS:
    node apps/desktop/web/scripts/installer.mjs {{ARGS}}

# the desktop app's checks and tests (D66 §9); the server tests run with PWM_TEST_SERVER set
desktop-test:
    pnpm --filter @pwm/desktop build
    cargo clippy --manifest-path apps/desktop/Cargo.toml --all-targets --locked -- -D warnings
    cargo test --manifest-path apps/desktop/Cargo.toml --locked

# a rules-signing seed (D34): run on an offline machine; prints the public key for RULES_KEYS
rules-keygen seed:
    cargo run -q --locked -p pwm-rules-sign -- keygen {{seed}}

ffi_lib := if os() == "windows" { "target/debug/pwm_ffi.dll" } else if os() == "macos" { "target/debug/libpwm_ffi.dylib" } else { "target/debug/libpwm_ffi.so" }

# the Swift package (D41) on a Mac: the core as an XCFramework, its tests, then the Xcode project (D42; needs `brew install xcodegen`)
apple:
    bash apps/apple/PWMKit/generate.sh apple
    cd apps/apple/PWMKit && swift test
    cd apps/apple && xcodegen generate

# design tokens into apps/apple/PWMKit/Sources/PWMDesign/Tokens.swift (D41); `just check` fails when it is stale
tokens:
    node scripts/tokens-swift.mjs

# the app fonts (D1) from design/fonts into the Apple and Android projects; `just check` fails when a copy is stale
fonts:
    node scripts/fonts.mjs

# Swift and Kotlin bindings over the native library (D31) into target/bindings
bindings:
    cargo build --locked -p pwm-ffi
    cargo run --locked -q -p pwm-ffi-bindgen -- generate --library {{ffi_lib}} --language swift --out-dir target/bindings/swift --no-format
    cargo run --locked -q -p pwm-ffi-bindgen -- generate --library {{ffi_lib}} --language kotlin --out-dir target/bindings/kotlin --no-format

# the Android app (D52): the core for the device ABIs (cargo-ndk), the Kotlin layer's JVM tests, a debug build and the licence check
[working-directory('apps/android')]
android:
    ./gradlew :kit:test :app:assembleDebug :app:checkLicense

# the Android app's tests on a running emulator or a connected device
[working-directory('apps/android')]
android-device:
    ./gradlew :app:connectedDebugAndroidTest

# design tokens into the Android app (D52 §6); `just check` fails when stale
android-generated:
    node scripts/tokens-kotlin.mjs

# the error messages in design/messages.json into both apps (D52 §6); `just check` fails when stale
messages:
    node scripts/messages.mjs

# apply formatters
fmt:
    cargo fmt --all
    cargo fmt --manifest-path core/pwm-crypto/fuzz/Cargo.toml
    pnpm -r exec prettier --write .

# dependency audit: advisories, bans, sources and licence allow-list (decision S6)
audit:
    cargo deny --all-features check
    cargo deny --manifest-path apps/desktop/Cargo.toml check
    pnpm install --frozen-lockfile
    pnpm license-check
    pnpm audit --audit-level high

# generate THIRD_PARTY_NOTICES/ (rust.html + npm.txt) for shipping in apps and extensions
notices:
    cargo about generate --all-features --fail about.hbs -o THIRD_PARTY_NOTICES/rust.html
    pnpm notices
    cp core/pwm-generator/data/NOTICE.md THIRD_PARTY_NOTICES/data.md
    cp core/pwm-autofill/data/NOTICE.md THIRD_PARTY_NOTICES/data-autofill.md
    node scripts/fonts.mjs --notices

# refresh the vendored Public Suffix List (D33) before a release; review the diff
psl:
    node scripts/update-psl.mjs

# fuzz one decoder target (nightly + cargo-fuzz, Linux/macOS; on Windows see docs/DEVELOPMENT.md)
[working-directory('core/pwm-crypto/fuzz')]
fuzz target secs="60":
    cargo +nightly fuzz run {{target}} -- -max_total_time={{secs}}
