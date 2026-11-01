# Pricing and trust commitments

Prices are proposals to validate; the **commitments are requirements** the billing code and UI must enforce.

| Plan | Proposed price | Includes |
| --- | --- | --- |
| Free (forever) | $0 | Unlimited items and devices, autofill, passkeys, generator, health report, 1 share link/day, local-file mode (Phase 2) |
| Premium | ~$24/yr | TOTP, attachments (1 GB), emergency access, unlimited share links, priority support |
| Family | ~$48/yr, 6 people | Premium for all + shared family vaults |
| Teams / Business | ~$4–6/user/mo | Admin console, SSO, SCIM, audit log, self-hosted option |

Billing: Stripe on web, App Store / Play Billing in apps, entitlements unified server-side (RevenueCat or own service — decision D6).

## Commitments (implement and test)

1. **Price hold**: existing subscribers keep their price ≥ 12 months after any increase. Store `price_locked_until` per subscription.
2. **Notice**: price change emails sent ≥ 60 days before it applies; renewal reminder ≥ 14 days before every charge.
3. **Same intro and renewal price**: no first-term-only discounts in our own billing.
4. **One-click cancel** in app and web; cancellation keeps access to the end of the paid period.
5. **Refunds**: pro-rata refund on request within 30 days of a renewal charge.
6. **Feature removal**: nothing moves from free to paid, and no platform app is discontinued, without 12 months' notice.
7. **Export**: always free on every plan, including after a subscription lapses; downgrade never deletes data.

Tests must cover: renewal reminder scheduling, price-lock enforcement on plan price changes, refund eligibility window, downgrade keeps data read/write within free limits.
