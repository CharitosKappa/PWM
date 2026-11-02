# Browser extension

TypeScript Manifest V3 extension (D34). The background service worker owns the core (`core/pwm-wasm`, built by `just wasm` into `core/`), the cache and every server conversation. The popup (Preact) talks to it with `chrome.runtime` messages and never receives keys. See `docs/AUTOFILL.md` for the security rules and `docs/DEVELOPMENT.md` for building and loading it.

| Path              | What                                                                                                                                   |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `src/background/` | Service worker: `service.ts` (the logic, tested with fakes), `chrome.ts` (storage, alarms, clipboard), `http.ts` (the core's requests) |
| `src/popup/`      | Popup and options page                                                                                                                 |
| `src/offscreen/`  | Writes and clears the clipboard                                                                                                        |
| `src/shared/`     | Messages and helpers used by both sides                                                                                                |
| `static/`         | Manifest, HTML and CSS                                                                                                                 |
| `scripts/`        | Build, manifest check, design tokens to CSS                                                                                            |

Chromium build only for now (M4.8 adds Firefox; Safari comes with the macOS app).
