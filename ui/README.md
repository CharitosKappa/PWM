# @pwm/ui

The screens shared by the browser extension and the desktop app (D3 §2), in
Preact and TypeScript: sign-in, unlock, the vault, settings and the manage
page (health, devices, import and export).

- **`protocol.ts`:** the requests the screens send and the replies they get.
  The extension's background answers them (D34); the desktop app's Rust
  backend will answer the same ones (M7.4).
- **`host.ts`:** what the screens need from the app they run in. Each app
  calls `setHost` once before the first render. The screens never call a
  browser or Tauri API themselves; `tsconfig.json` has no `chrome` types, so
  a direct call doesn't compile.
- **`screens.css`:** the screens' styles, on top of the design tokens'
  `tokens.css`.

Import files by path, for example `@pwm/ui/App.tsx`.
