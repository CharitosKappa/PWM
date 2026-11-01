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
| Docker | any recent | `just server` (from M3.6) |

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

## Fuzzing

Targets live in `core/pwm-crypto/fuzz/fuzz_targets/` (`envelope_decode`, `vault_share_decode`, `keyset_plaintext`, `secret_key_parse`, `password_normalize`). CI runs each for 60 s on every PR and 10 minutes nightly; crash inputs are uploaded as artifacts.

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
