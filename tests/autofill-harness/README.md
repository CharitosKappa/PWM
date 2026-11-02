# Autofill harness

Playwright runs the extension's test build in Chromium against fixture pages and checks what must fill, warn or refuse (D35). It started with M4.3. M4.7 adds the full fixture set (shadow DOM, SPA re-renders, more overlay attacks) and the top-1,000 site runner.

```
just wasm      # the core for the extension
just server    # the sync server on http://127.0.0.1:8080 (another terminal)
just autofill  # test build, Chromium, the fixtures
```

- **Test build** (`pnpm --filter @pwm/extension build:test`, into `extension/dist-test/`):
  - It has site access from install, because Playwright cannot click the toolbar button or answer a permission prompt.
  - It has a hook on the background (`globalThis.pwmTest`) that the tests drive from the service worker.
  - The release build's check fails if that hook is ever in it.
- **Fixture pages** (`fixtures/`) are served by `fixtures/serve.ts`:
  - over HTTPS on port 8443 (a throwaway self-signed certificate made with `openssl`)
  - and over HTTP on 8081
  - for any `*.test` host name; Chromium maps those to 127.0.0.1.
- **What is checked:**
  - a plain form
  - honeypots
  - a multi-step login
  - a code field
  - a transparent overlay
  - a nearly transparent ancestor
  - a frame from another site
  - a frame from the same site
  - an `http` page
  - the inline icon: two checked clicks, and a page that makes the icon transparent
