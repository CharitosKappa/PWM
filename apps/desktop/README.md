# apps/desktop

The desktop app for Windows and Linux (D3, D66): Tauri 2, with the screens
shared with the browser extension (`@pwm/ui`) and a Rust backend that links
the core crates directly.

- **`src-tauri/`:** the `pwm-desktop` crate, in a Cargo workspace of its own
  (`Cargo.toml` here, with its own lock file and `deny.toml`).
  - `backend.rs`: answers the screens' requests (`ui/src/protocol.ts`) over
    one `pwm_client::Client`, as the extension's background does. The
    browser's requests answer `unsupported`.
  - `keys/`: the email, Secret Key, device id and tokens. DPAPI files on
    Windows; the Secret Service on Linux, or files only the user can read
    without one (the tokens then sealed by the core).
  - `http.rs`: `reqwest` over rustls (ring provider, the system's trust
    store), no cookies, no cache, no redirects.
  - `app.rs`: the window, the one `request` command, the tray, one copy only,
    the timers, and locking on sleep and session lock (`system/`).
- **`web/`:** `@pwm/desktop`, the page Tauri serves: the shared screens with
  the desktop host, built by esbuild into `web/dist/`. `e2e/smoke.mjs` drives
  the built app through `tauri-driver` (CI, Linux).
- **`isolation/`:** Tauri's isolation hook: only the `request` command
  passes.
- **`scripts/icon.mjs`:** the placeholder icon until the brand is chosen (D1).

`docs/DEVELOPMENT.md` has the setup and commands.
