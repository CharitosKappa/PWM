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

### Android device checklist (D52)

On a real phone (not an emulator), with a debug build:

1. Install it and open it: the welcome screen shows.
2. Run `just android-device` with the phone connected: the Keystore test passes, so keys are sealed by the phone's hardware-backed Keystore.
3. Clear the app's data in system settings, then open it again: it starts clean, with no error.

Onboarding (D53), with the same build:

1. Create an account. On the Secret Key screen, a screenshot comes out black.
2. Save Emergency Kit: the system's "Save to" opens; save to Downloads and open the PDF. Print: the print dialog shows the kit; "Save as PDF" works.
3. Kill the app on the Secret Key screen before "I saved it", open it and unlock: the Secret Key screen comes back.
4. Sign out, then Sign in, Scan Emergency Kit: the camera permission is asked only now. Scan the printed kit: the email and Secret Key fill in, never the password. Refuse the permission once: the screen says how to allow it, and typing still works.
5. Sign in with another device, approving from the extension: the codes match on both.

The vault (D54), with the same build:

1. Add a login, a card, an identity and a note; find each by search; open a login's website in the browser.
2. Set up a one-time code by scanning a site's QR code: the code and its countdown show.
3. On a tablet, or with the window wider than 600 dp: a side rail, and the list beside the open item.
4. Lock timeout 1 minute: leave the app for 2 minutes and come back: it asks to unlock. Turn the screen off and on: it asks to unlock, whatever the setting.
5. With Immediately set, Save Emergency Kit in Settings: the picker opens and the file saves without the vault locking.
6. Settings, Third-party notices: each file opens and nothing loads from the network.

Fingerprint and face unlock (D55), with the same build:

1. With no finger enrolled, Settings says why it can't be used. Enroll one: the switch shows.
2. Unlock with the master password: the offer shows once. Turn on: the prompt asks once. Lock: the unlock screen asks by itself; Use master password leaves the password field, with no error.
3. Enroll another finger, then lock: the next unlock asks for the master password, says why, and turns it back on.
4. Remove the screen lock: Settings says to set one, and the unlock screen asks for the master password.
5. Sign out and sign in again: it starts off.

Screenshots and screen sharing (D56), with the same build:

1. On the vault and the Secret Key screen: a screenshot and a screen recording come out black, and casting shows black. On welcome and unlock they show.
2. The app switcher shows no picture of the vault.
3. Settings, turn off "Hide PWM from screenshots and screen sharing": the vault shows in a screenshot; on Android 13+ the switcher still shows no picture. The Secret Key screen stays black.

Background sync (D57), with the same build:

1. Sign in, lock the phone and leave it for an hour. On another device, add an item. Unlock: it's there without waiting for a sync.
2. With the phone offline, edit an item, lock, and go back online: within the next half hour the other device has it, with the app closed.

Push (D60), with a build that has the Firebase project's settings and a server with `PWM_FCM_CREDENTIALS_FILE`:

1. Sign in and lock the phone. On another device, add an item: within a minute the phone has it, without opening the app.
2. Settings, "Notify me when a new device asks to sign in": on Android 13+ the permission is asked now, and only now. Refuse it: the switch stays off and says where to allow it. Allow it in system settings, then turn the switch on.
3. Ask to sign in from a new device: the phone shows "A new device wants to sign in. Open PWM to compare the code." Tap it while locked: after unlocking, Approve a new device shows the request.
4. Sign out: the server's push_tokens row for the phone goes, and no more pushes arrive.

Autofill (D58), with the same build:

1. Settings shows "Turn on PWM for autofill"; its button opens the system's choice. Pick PWM: the card goes.
2. Chrome and Firefox on a site with a saved login: the login is offered; a tap fills. With Gboard, it shows as a chip above the keyboard.
3. A bank-style app: "Search PWM", pick the login with "Use for this app": it fills, and is offered by name next time.
4. Locked: "Unlock PWM to fill" asks for the fingerprint or the master password, then fills.
5. A login with a one-time code: on a screen with no code field, the code is on the clipboard after the fill.

Passkeys and the Credential Manager (D61), on an Android 14+ phone with the same build:

1. With PWM chosen for autofill, Settings shows no "Turn on PWM for passkeys". Turn PWM off under "Passwords, passkeys & accounts": the card shows, and its button opens that screen.
2. Chrome, webauthn.io: register a passkey with PWM (the fingerprint or master password is asked), then sign in with it. Locked, the sheet shows "Unlock PWM", then the passkey.
3. A real app that uses passkeys and lists itself in its site's `assetlinks.json`: create and sign in. An app its site doesn't list gets no PWM entry for passkeys.
4. An app that asks the Credential Manager to save a password: "Save in PWM" shows the username and the app; the login is saved, linked to the app, and offered next time. Saving again with the same username updates it.
5. Sync, then sign in on another phone's PWM or the extension with the passkey made here.

Devices and approving (D59), with the same build:

1. Settings, Devices: every device shows, this one marked. Rename one: another device sees the new name after a sync. Sign out another device: it is asked to sign in again.
2. On a second phone or the extension, sign in with "Sign in with another device". On this phone, Settings, Approve a new device: the request shows within a few seconds; the codes match on both; the new device signs in.
3. Revoke this phone's session from another device's list: the next sync here shows "Sign in again to sync"; the master password brings it back, with unsent changes kept.

Health, export and import (D59 §5-7), with the same build:

1. With a weak and a reused password, and one known to be leaked (such as `password123` on a synthetic login): Password health lists each; Change opens its edit screen on the Vault tab. Turn off "Check for leaked passwords": the leak check doesn't run.
2. Export each format: the master password is asked every time and a fingerprint doesn't count. Save export opens "Save to"; save to Downloads and to Google Drive; the file opens with the export password (`.pwmx`) or as text. With Immediately set, the vault doesn't lock on the trip.
3. Import a CSV from Downloads with the system's picker: the preview's counts match; after Import, Delete export file removes it. From Google Drive, where the file can't be deleted, the screen says to delete it by hand.

### Import checklist (D51)

Before the beta, with a real export from each manager (made with a throwaway account holding synthetic items, never real ones):

1. Bitwarden (unencrypted, and password-protected with PBKDF2 and with Argon2id), KeePass `.kdbx` from KeePass 2 and KeePassXC (3.1 and 4, with a password, a key file and both), 1Password `.1pux`, Keeper JSON, NordPass CSV, Proton Pass ZIP, Dashlane CSV ZIP, LastPass CSV, and Chrome, Firefox and Safari CSVs.
2. Each one: the detected format is right; the preview's counts match the source; logins, cards, identities, notes, folders, one-time codes and custom fields land where expected; what was left out is listed.
3. Import the same file twice: the second adds nothing.
4. On iOS (from Files and iCloud Drive), macOS (from Finder) and Android (from Downloads and Google Drive): after a plaintext import, Delete export file removes it, or says to delete it by hand where the place can't; after an encrypted one, no button shows. In the extension, the line says to delete it.
5. Delete the throwaway accounts and the export files afterwards.

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
