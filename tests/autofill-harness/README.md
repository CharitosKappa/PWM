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

- **What it does:** `runner/run.ts` takes `sites/sites.json`. For each site it saves a throwaway login (`pwm-check-user@example.invalid` / `pwm-check-password`), opens the login page, waits for a field to appear (up to 10 s) and for any fade-in to finish, fills through the extension, and checks which inputs got the values.
- **What it never does:** submit, click, solve a challenge or use a real account.
- **Results:**
  - **pass**
  - **fail**
  - **blocked:** a challenge page, an HTTP error, or a bot wall that is only text (`bot_wall`, when no field ever appeared), left out of the pass rate
  - **error:** the page didn't load: `timeout`, or the browser's network error (`net::ERR_HTTP2_PROTOCOL_ERROR`)
- **The report:** `autofill-report.json` (v1), with fill targets as D38 report fields, never values. A refused fill names the failed check (`field_hidden:covered`, D63). A field holding the check value cut to its `maxlength` counts, and a password field people can't see (under `aria-hidden` or `inert`, or transparent) isn't one the page asks for (D65 §5). `final_host` says where a page ended up when it left the login URL's host: the entry may be stale.
- **Release gate (D64):** `node runner/compare.ts previous.json candidate.json` counts only the sites that pass or fail in both reports, so a site that is blocked one night doesn't move it. It fails when the pass rate on those sites drops more than 0.5 points, or when the candidate counts fewer than 80% of the sites the previous report counted. It lists each site lost and gained, and those left out. Its tests are in `tests/compare.spec.ts`, which needs no browser.
- **Adding a site:** check its login page, then add `{site, login_url}` and, if you are sure of them, the CSS selectors of the username and password inputs. Only password logins belong here. An entry taken out goes under `removed` with the reason.
- **Its own tests:** `tests/runner.spec.ts` checks the runner against local pages.

## The public dashboard

- **What it is (D77):** `dashboard/build.ts` turns a night's report and the live `history.json` into a static page. It is plain HTML and CSS with no scripts, cookies, fonts or requests to other hosts.
- **What it writes:** `index.html`, `style.css`, `report.json` (each site's status, reason and since when, without the fill targets) and `history.json` (the last 90 nights).
- **The nightly deploy:** CI's `autofill-dashboard` job runs after the runner, on `main` only. It downloads the live `history.json` and publishes to GitHub Pages.
  - It doesn't publish when the history can't be downloaded, when the report fails its checks, or when more than 20% of the sites ended as `error` (the runner broke). The last good page stays up.
  - It is off until the repository variable `PUBLISH_DASHBOARD` is `true`, so the nightly run isn't red before Pages exists. To turn it on:
    1. Turn on Pages with "GitHub Actions" as the source.
    2. Set the variable.
    3. Run the workflow once by hand with `first_dashboard` ticked, which starts the history empty.
- **Locally:**

  ```
  node dashboard/build.ts --report autofill-report.json --first --out site
  ```

  `--history <file>` instead of `--first` adds the night to a downloaded history. `--tranco` adds the Tranco attribution, once the site list is ordered by it.

- **Its tests:** `tests/dashboard.spec.ts`, which needs no browser.
