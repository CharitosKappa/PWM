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
| `core-test` + `test-vectors` | every PR | yes |
| `server-test` | PRs touching server/core | yes |
| `extension-test` + autofill fixtures | PRs touching extension/core | yes |
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
