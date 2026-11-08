# apps/desktop

The desktop app for Windows and Linux (D3, D66): Tauri 2, with the screens
shared with the browser extension (`@pwm/ui`) and a Rust backend that links
the core crates directly.

- **`src-tauri/`:** the `pwm-desktop` crate, the app, in a Cargo workspace of its own
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
  - `browser_hosts.rs`: "Allow browser extensions to unlock with Windows
    Hello" (D69), over the helper library's `hosts` module.
  - `installer.conf.json`, `installer.windows.conf.json` and
    `installer-hooks.nsh`: the installers' settings (D70), used only by
    `just desktop-installer`.
- **`hello/`:** `pwm-hello`, Windows Hello's signing key (D68) and DPAPI,
  and on Linux `PolkitKey` (D71), shared by the app and the browser helper.
- **`browser-helper/`:** `pwm-browser-helper` (D69), the native-messaging
  host the browser starts for "Unlock with Windows Hello": D48's protocol,
  the extension's key kept under a Windows Hello signature's key. Its
  `hosts` module registers it with the browsers and undoes that, for the
  app's switch and for the uninstaller (`--uninstall`, D70 §5).
- **`keyholder/`:** `pwm-keyholder` (D71), the root key holder behind
  polkit that Linux login and Linux's browser helper ask through `pkexec`.
  `hello/`'s `polkit::PolkitKey` calls it.
- **`linux/`:** polkit's action file and the packages' removal script (D71).
- **`web/`:** `@pwm/desktop`, the page Tauri serves: the shared screens with
  the desktop host, built by esbuild into `web/dist/`. `e2e/smoke.mjs` drives
  the built app through `tauri-driver` (CI, Linux). `scripts/installer.mjs`
  builds the installers.
- **`flatpak/`:** the Flatpak's manifest and desktop entry (D70 §2).
- **`isolation/`:** Tauri's isolation hook: only the `request` command
  passes.
- **`scripts/icon.mjs`:** the placeholder icon until the brand is chosen (D1).

`docs/DEVELOPMENT.md` has the setup and commands.
