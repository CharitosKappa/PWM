# Pricing and trust commitments

Prices are proposals to validate; the **commitments are requirements** the billing code and UI must enforce.

| Plan | Proposed price | Includes |
| --- | --- | --- |
| Free (forever) | $0 | Unlimited items and devices, autofill, passkeys, generator, health report, 1 share link/day, local-file mode (Phase 2) |
| Premium | ~$24/yr | TOTP, attachments (1 GB), emergency access, unlimited share links, priority support |
| Family | ~$48/yr, 6 people | Premium for all + shared family vaults |
| Teams / Business | ~$4–6/user/mo | Admin console, SSO, SCIM, audit log, self-hosted option |

Billing (D6): Stripe Checkout in the browser for the Mac, Windows, Linux and the extensions; the App Store in the iOS app; Google Play in the Android app. Our own entitlement service on the server is the one source of the plan, whoever sold it. A server without billing settings (self-hosted) makes every account Premium.

## Commitments (implement and test)

1. **Price hold**: existing subscribers keep their price ≥ 12 months after any increase. Store `price_locked_until` per subscription.
2. **Notice**: price change emails sent ≥ 60 days before it applies; renewal reminder ≥ 14 days before every charge.
3. **Same intro and renewal price**: no first-term-only discounts in our own billing.
4. **One-click cancel** in app and web; cancellation keeps access to the end of the paid period.
5. **Refunds**: pro-rata refund on request within 30 days of a renewal charge.
6. **Feature removal**: nothing moves from free to paid, and no platform app is discontinued, without 12 months' notice.
7. **Export**: always free on every plan, including after a subscription lapses; downgrade never deletes data.

Tests must cover: renewal reminder scheduling, price-lock enforcement on plan price changes, refund eligibility window, downgrade keeps data read/write within free limits.

## Where a store limits a commitment (D6 §5)

With Stripe we run the subscription, so every commitment holds in full. A plan bought in the App Store or Google Play is run by the store, and these gaps are known:

- **§1 Price hold:** kept by choosing "preserve current price" (App Store) or a legacy price cohort (Play) at each price change, in the store's console, for at least 12 months. The server checks each renewal's price against `price_locked_until` and alerts on a mismatch.
- **§4 One-click cancel:** we can't cancel a store subscription. The app's button opens the store's own subscription page.
- **§2 Notices:** the server's outbox schedules every reminder and notice for every provider, but sends nothing until an email provider is chosen (D32). Until then, no price change applies to anyone (D72 §2).
- **§5 Refunds:** only Apple refunds App Store purchases. The iOS app opens Apple's request sheet, and the other apps point to Apple's Report a Problem page; if Apple declines, we can't refund. Play refunds pro-rata through its API, like Stripe (D72 §1).
- **Deleting an account** can't end a store subscription; the delete screen says so first, with the link to cancel it (M9.4).

## What Free does with paid items (D73)

A gate never stops exporting, reading, editing or deleting what the account already has (D72 §4).

- **One-time codes:** the login's secret (the `otpauth://` link) can be stored, shown, copied, edited, imported and exported on Free, so it can be moved to another authenticator at any time. The current code needs Premium (or Family, Teams): the item says "One-time codes are part of Premium", and autofill offers no code step, without a message. The plan comes with every sync; a device that hasn't heard yet keeps codes, and after the paid period it knew about it counts as Free until the next sync.
- **Share links and attachments** (M13, M14): the server's limits are 1 share link a day and no attachment storage on Free, unlimited links and 1 GiB per person on Premium, Family and Teams. A server without billing gives every account Premium's. Their routes answer `plan_required` past the limit.

## Changing a price (D72 §2)

1. Create the new price at the provider (Stripe price, App Store or Play price change keeping existing subscribers' price).
2. For Stripe, put it first in `PWM_STRIPE_PRICES`, keeping the old one after it (`premium=price_new,premium=price_old`), so new subscribers buy the new one and old subscriptions still read as Premium.
3. Run `pwm-server price-change <provider> <plan> <new price id> <amount in minor units> <currency>`. Subscribers paying another price keep it for 12 months, get a notice now and a reminder 60 days before the hold ends.
4. Stripe subscriptions move to the new price at their first renewal after the hold, and only if their reminder was delivered at least 60 days before. Store subscriptions follow the store's console.
