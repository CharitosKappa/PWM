# Task runner for the whole monorepo. Run `just` to list recipes.

set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# fmt + clippy + lint for all packages
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    pnpm install --frozen-lockfile
    pnpm format:check
    pnpm lint
    pnpm typecheck

# all unit tests (core, server, extension)
test:
    cargo test --workspace --all-features --locked
    pnpm install --frozen-lockfile
    pnpm test

# crypto test vectors (tests named `*vectors*`, see core/test-vectors/)
test-vectors:
    cargo test --workspace --all-features --locked vectors

# Playwright autofill harness against fixture sites (built in M4.7)
autofill:
    @echo "autofill harness not built yet (M4.7): see tests/autofill-harness/README.md"

# run the sync server locally (docker compose arrives in M3.6)
server:
    cargo run -p pwm-server

# apply formatters
fmt:
    cargo fmt --all
    pnpm -r exec prettier --write .

# dependency audit: advisories, bans, sources and licence allow-list (decision S6)
audit:
    cargo deny --all-features check
    pnpm install --frozen-lockfile
    pnpm license-check
    pnpm audit --audit-level high

# generate THIRD_PARTY_NOTICES/ (rust.html + npm.txt) for shipping in apps and extensions
notices:
    cargo about generate --all-features --fail about.hbs -o THIRD_PARTY_NOTICES/rust.html
    pnpm notices
