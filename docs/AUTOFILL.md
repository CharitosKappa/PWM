# Autofill

Autofill reliability is our top promise and the top complaint against every competitor. Treat it as a product with its own tests, metrics and dashboard.

## Engine (in `core/pwm-autofill`, shared by all platforms)

1. **Origin matching**: match on registrable domain (Public Suffix List) by default; per-item options: exact host, starts-with, regex (advanced), never. Never fill on `http://` without a warning. Never fill inside cross-origin iframes unless the iframe origin matches the item.
2. **Field classification**: score each input using `autocomplete`, `type`, `name`/`id`, labels, placeholder, ARIA, form structure and position. Classes: `username`, `email`, `current-password`, `new-password`, `totp`, `cc-*`, `address-*`, `ignore`.
3. **Per-site overrides**: a signed rules file (`autofill-rules.json`) shipped and updated separately from app releases, so site fixes do not wait for store review.
4. **Multi-step logins**: remember step state (username page → password page → TOTP page) per tab.
5. **Save/update detection**: detect submit + navigation; offer save only when credentials changed. The decision is in the core (`save_offer`, D36). In the extension, captured values stay in background memory for at most 5 minutes, and the offer card follows the inline icon's rules.

## Extension security rules (from the 2025 DEF CON clickjacking research)

- Default fill mode = **on user action**: toolbar click, keyboard shortcut, or our inline icon in a **closed shadow root rendered in the extension's own iframe** (`web_accessible_resources` page), never a plain DOM element the page can restyle or make invisible.
- Before filling, verify the target input is visible, not covered (`elementsFromPoint` check), opacity 1, and in the viewport. In a frame, IntersectionObserver v2 must also report it visible (D35 §4, D63).
  - Per D65:
    - **Fields people can't reach are never offered:** a field under `aria-hidden="true"` or `inert`, such as a decoy password field for browsers' own password managers.
    - **Buttons inside a field:** a button wholly inside the field's box, covering at most a quarter of it (show password, clear), may sit at a corner sample point; the centre must be the field.
    - **The username alone:** when the password beside a username fails these checks, as on a page that shows it only after the username, the username is filled alone and the password waits for the next step. A field that fails is never filled.
- Payment cards, identities and passkeys always need a confirmation step. Passkeys confirm in an extension window of their own, never in the page (D37).
- TOTP codes are copied to the clipboard (auto-clear 30s) unless the field is classified `totp` with high confidence.
- Minimal permissions: `activeTab`, `storage`, `scripting`; host permissions requested on demand. `nativeMessaging` is optional, asked on the click that turns on Touch ID through the Mac app (D48 §5); Safari has it in the manifest, since it asks nothing for it. Site access is asked for only when the user turns on the inline icon (D35); without it the content script is injected only on a user action.
- No remote code. Rules file is data only and signature-checked.

## Mobile

- iOS: AutoFill Credential Provider + passkey provider (ASCredentialProviderViewController). QuickType suggestions via `ASCredentialIdentityStore`, kept in sync after every vault change.
  - Per D47: the extension (`apps/apple/AutoFill`) unlocks with Face ID or Touch ID only; without it, it says in one line what to do in the app. It lists the logins the core matches (D33) for each service identifier iOS passes, with a search over the rest; a tap fills, and copies the login's one-time code for 30 seconds. A suggestion chosen above the keyboard fills after Face ID; iOS's no-interaction path always answers "interaction required".
  - Suggestions are each login's domains (the registrable domain for the default match, the host otherwise; regex and never URLs are left out) and username, plus passkeys by RP id. The switch "Suggest logins above the keyboard" is in Settings, on by default.
  - Passkeys use the core's hashed calls (`passkey_register_hashed`, `passkey_assert_hashed`), since iOS passes only the client data hash.
- Android: AutofillService + Credential Manager provider; inline suggestions on keyboards that support them; fall back to dataset picker.
  - Per D58: the service fills passwords after a tap. A web domain is believed only from 7 listed browsers, each checked by package and signing certificate; any other app is matched as itself, through links `android-app://<package>?sha256=<digest>` that the user makes once ("Use for this app"). Unlocked, the matches are offered by name; locked, one "Unlock PWM to fill" entry opens PWM. Nothing about the vault is kept outside its encryption for this.
  - Per D61, Android 14+: the Credential Manager provider offers passwords and passkeys in the system's sheet. A listed browser's origin is believed (checked by Android and again by the app); any other app gets a site's passkeys only when the site's Digital Asset Links file lists it, and its passwords only through its `android-app://` links. Every passkey use is confirmed with a fingerprint or the master password; "Save in PWM" saves passwords apps ask to save.

## Test harness (`tests/autofill-harness`)

- Fixture sites (static HTML) for every pattern: single page, multi-step, iframe login, shadow DOM, SPA re-render, OTP fields, honeypot fields, invisible overlay (clickjacking attempts that must fail).
- **Top-1,000 runner**: nightly Playwright run against real login pages (no real credentials; assert field classification and fill targets only). Output: pass/fail per site → JSON → public compatibility dashboard. Per D39: it fills a throwaway login and never submits; bot walls are recorded as blocked, never bypassed; `runner/compare.ts` is the release gate.
  - Per D77, the public dashboard is a prerendered page with no scripts, rebuilt each night on GitHub Pages from the report and the live `history.json` (90 nights). It shows each site's status, since when, and why, and marks sites that haven't worked for more than 7 days. A night where more than 20% of the sites couldn't load isn't published.
- Every extension PR runs fixture suite; release candidates run top-1,000 suite. Release blocked if pass rate drops > 0.5 pts vs previous release, counted on the sites both runs could reach (D64), or if the candidate reaches fewer than 80% of them.

## Failure reports

- In-extension "Autofill didn't work here" button: captures URL origin and a **form-structure fingerprint** (tag, type, attributes, label text of inputs — never values), user confirms before sending. Per D38: anonymous, to the hosted service only, label text cleaned in the core, kept 90 days.
- Reports create tickets automatically; fix SLA: 7 days for top-1,000 sites, published on the dashboard.
