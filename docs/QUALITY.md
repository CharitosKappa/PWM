# Quality, CI and releases

Most one-star reviews in this market come from broken updates. Release safety is a feature.

## Testing

| Layer | Tests |
| --- | --- |
| core | Unit tests, property tests (`proptest`) for parsers/formats, crypto test vectors, fuzzing (`cargo fuzz`) on importers and decryptors, migration tests for every format version |
| server | Integration tests with Postgres in Docker; auth flow; revision conflicts; rate limits |
| extension | Unit (Vitest), autofill fixture suite (Playwright), top-1,000 nightly |
| iOS / macOS | XCTest + XCUITest for unlock, autofill provider, sync |
| Android | JUnit + Compose UI tests + autofill service instrumentation tests |
| desktop | Tauri integration tests, native messaging tests |
| e2e | Create account on device A → item on B → share to C → revoke |

## CI (GitHub Actions)

| Job | Trigger | Blocking |
| --- | --- | --- |
| `fmt-lint` (rustfmt, clippy, eslint, swiftlint, ktlint) | every PR | yes |
| `core-test` + `test-vectors`, and `qpdf --check` on the Emergency Kit's golden PDFs (D42) | every PR | yes |
| `server-test` | PRs touching server/core | yes |
| `extension-test` + autofill fixtures | PRs touching extension/core | yes |
| `wasm`: both extension builds, `web-ext lint`, Firefox smoke (D40) | PRs touching extension/core | yes |
| `bindings`: Swift/Kotlin smoke, the Swift package on Linux against a server (D41) | every PR | yes |
| `apple`: the Swift package on macOS against a server (D41), and the iOS app's UI test in the simulator (D42) | PRs touching `apps/apple`, `core/` or the tokens | yes |
| `ios-build`, `android-build`, `desktop-build` | PRs touching that app/core | yes |
| `audit` (cargo audit/deny, npm audit) | every PR + nightly | high severity |
| `autofill-top-sites` (D39) | nightly + release candidates | release only (`runner/compare.ts`) |

## Release process

1. Release candidate from `main`, version `YYYY.M.patch`, same core version on every platform.
2. Beta channel (TestFlight, Play internal/beta, extension beta listing, desktop beta) for ≥ 3 days.
3. Staged rollout: 1% → 10% → 50% → 100% over ~7 days.
4. Automatic halt if, vs previous release: crash-free sessions drop > 0.2 pts, autofill failure reports rise > 25%, or sync error rate doubles.
5. Rollback: extension re-publishes previous version; server feature flags; apps use phased-release pause.
6. Every release note lists fixes and any changed behavior in plain language.

### Extension smoke checklist (D40)

Edge, Brave and Opera get the same Chromium zip as Chrome and have no automated run. For each release candidate, in each of them:

1. Load the candidate's `pwm-chromium-<version>.zip` (unpacked) and open the popup.
2. Sign in to a test account, unlock and lock.
3. Fill a login from the popup and with the shortcut, then copy a password and check that the clipboard clears.
4. Turn on "Fill and save on sign-in pages": the icon fills, and a new login is offered after signing in.
5. Turn on passkeys: create one on a WebAuthn test page and sign in with it.

Firefox: the same steps with `pwm-firefox-<version>.zip` from `about:debugging`. Step 4 offers saves on the toolbar button only, with no icon or card.

Safari (D48): run the macOS app once (built after `just extension`), then turn on PWM in Safari, under Settings, Extensions (for an unsigned build, first allow unsigned extensions in Safari's Develop settings). Then the same steps, as for Firefox. Also check that signing in asks for access to the server.

### macOS app checklist (D48)

For each release candidate, signed with the founder's Developer ID and notarized:

1. Sign in, then turn on Touch ID unlock; lock with ⇧⌘L and unlock with Touch ID.
2. Put the Mac to sleep, or lock the screen: the vault is locked on return.
3. Turn on PWM in System Settings, under AutoFill & Passwords. In Safari and in another app, fill a login from the suggestions and from the list, and check that its one-time code is copied.
4. Create a passkey on a WebAuthn test page in Safari, then sign in with it.
5. Turn off "Suggest logins above the keyboard": no suggestions are left. Sign out: none either.

### Browser Touch ID checklist (D48)

On a Mac, with a signed build of the app and the extension's store id in `BrowserUnlock.chromiumOrigins`:

1. In the app's Settings, turn on "Allow browser extensions to unlock with Touch ID".
2. In Chrome, and then in Firefox and Safari: unlock the extension with the master password, turn on "Unlock with Touch ID" (Touch ID asks once), lock, then unlock with Touch ID.
3. Cancel Touch ID: the master password still unlocks, and Touch ID stays on. Add a finger in System Settings: the next unlock turns Touch ID off.
4. Turn the app's switch off: the extension says the Mac app is needed.

### Background sync and push checklist (D50)

On a device with a signed build, and a server with the `PWM_APNS_*` settings:

1. Lock the app on the phone. On another device, add a login. Within a minute the phone gets a push (the server log counts it); open AutoFill in Safari: the new login is listed.
2. With the server's push off, leave the phone locked for an hour: background refresh brings the login in (iOS decides when).
3. On the Mac, with the window open and locked, the same within 15 minutes.
4. Ask to sign in from a new device: with "Notify me when a new device asks to sign in" on, the notice shows on the phone and the Mac, with no email or device name.
5. End the phone's session on the server, edit an item on the phone, and pull to refresh: "Sign in again to sync" shows. Sign in with the master password: the edit reaches the other device, and Face ID still unlocks.
6. Sign out on the phone: the server's `push_tokens` has no row for it.

### Screen capture checklist (D49)

On an iPhone and a Mac, for each release candidate:

1. Reveal a password, then start a screen recording (Control Center): it shows "Hidden while your screen is recorded or shared", and Show does nothing. Stop: the password stays masked until tapped.
2. Reveal a password and take a screenshot: it is masked again, with the line about the photo library.
3. Open the app switcher on the vault and on the Secret Key screen: both are covered.
4. On the Mac, take a screenshot and share the screen while unlocked: the PWM window is left out. Turn off "Hide PWM from screenshots and screen sharing": it shows.

## Success metrics

| Metric | Phase 1 | Phase 2 | Phase 3 |
| --- | --- | --- | --- |
| Autofill success, top 1,000 sites | 95% | 98% | 99% |
| Median fix time for reported site failures | 14 d | 7 d | 3 d |
| Crash-free sessions | 99.5% | 99.8% | 99.9% |
| Store rating (iOS, Android, Chrome) | 4.5+ | 4.6+ | 4.7+ |
| Free → paid conversion | 3% | 5% | 6% |
| Paid support first reply | < 24 h | < 8 h | < 4 h |

Privacy: product analytics are opt-in, aggregate, and never include URLs, item data or form contents. Autofill failure reports are not analytics. The user sends each one after seeing it, and it holds the site's origin and the form's structure only (D38).
