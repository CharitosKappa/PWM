# Autofill harness

Playwright runs the extension's test build in Chromium (D35, D39).

- **The fixture suite** (`tests/`) runs on every extension PR. It checks what must fill, warn or refuse.
- **The top-sites runner** (`runner/`) runs nightly against real login pages.

```
just wasm       # the core for the extension
just server     # the sync server on http://127.0.0.1:8080 (another terminal)
just autofill   # the fixture suite
just top-sites  # the runner, against real sites (network)
```

## Fixtures

- **What each page expects:** every page in `fixtures/` has an entry in `fixtures/fixtures.json`. It says one of:
  - **fill:** the fields and their values
  - **refuse:** and the reason
  - **confirm:** refused until confirmed
  - **copy:** the code is copied
  - **no_form**
  - **flow:** a named test covers it
- **The data-driven run:** `tests/fixtures.spec.ts` runs the entries, and fails if a page has none.
- **The named tests:** `tests/fill.spec.ts` holds the flows: the inline icon and its attacks, save offers, passkeys and failure reports.
- **The server:** `fixtures/serve.ts` serves them over HTTPS (8443, a throwaway self-signed certificate) and HTTP (8081) for any `*.test` host, which Chromium maps to 127.0.0.1.

## The runner

- **What it does:** `runner/run.ts` takes `sites/sites.json`. For each site it saves a throwaway login (`pwm-check-user@example.invalid` / `pwm-check-password`), opens the login page, fills through the extension, and checks which inputs got the values.
- **What it never does:** submit, click, solve a challenge or use a real account.
- **Results:**
  - **pass**
  - **fail**
  - **blocked:** a challenge page or an HTTP error, left out of the pass rate
  - **error**
- **The report:** `autofill-report.json` (v1), with fill targets as D38 report fields, never values.
- **Release gate:** `node runner/compare.ts previous.json candidate.json` fails when the pass rate drops more than 0.5 points.
- **Adding a site:** check its login page, then add `{site, login_url}` and, if you are sure of them, the CSS selectors of the username and password inputs.
- **Its own tests:** `tests/runner.spec.ts` checks the runner against local pages.
