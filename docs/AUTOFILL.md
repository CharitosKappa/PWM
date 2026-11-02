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
- Before filling, verify the target input is visible, not covered (`elementsFromPoint` check), opacity 1, and in the viewport.
- Payment cards, identities and passkeys always need a confirmation step.
- TOTP codes are copied to the clipboard (auto-clear 30s) unless the field is classified `totp` with high confidence.
- Minimal permissions: `activeTab`, `storage`, `nativeMessaging`, `scripting`; host permissions requested on demand. Site access is asked for only when the user turns on the inline icon (D35); without it the content script is injected only on a user action.
- No remote code. Rules file is data only and signature-checked.

## Mobile

- iOS: AutoFill Credential Provider + passkey provider (ASCredentialProviderViewController). QuickType suggestions via `ASCredentialIdentityStore`, kept in sync after every vault change.
- Android: AutofillService + Credential Manager provider; inline suggestions on keyboards that support them; fall back to dataset picker.

## Test harness (`tests/autofill-harness`)

- Fixture sites (static HTML) for every pattern: single page, multi-step, iframe login, shadow DOM, SPA re-render, OTP fields, honeypot fields, invisible overlay (clickjacking attempts that must fail).
- **Top-1,000 runner**: nightly Playwright run against real login pages (no real credentials; assert field classification and fill targets only). Output: pass/fail per site → JSON → public compatibility dashboard.
- Every extension PR runs fixture suite; release candidates run top-1,000 suite. Release blocked if pass rate drops > 0.5 pts vs previous release.

## Failure reports

- In-extension "Autofill didn't work here" button: captures URL origin and a **form-structure fingerprint** (tag, type, attributes, label text of inputs — never values), user confirms before sending.
- Reports create tickets automatically; fix SLA: 7 days for top-1,000 sites, published on the dashboard.
