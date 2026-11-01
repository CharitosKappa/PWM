# Competitor context (why features exist)

Summary of the competitive analysis (as of 2026-10-31). Prices are reviewer-reported; verify before using in marketing.

| Product | Best at | Weakest at | Top bad-review themes |
| --- | --- | --- | --- |
| Bitwarden | Open source, audited, strong free plan, self-host, cheap premium ($19.80/yr since Jan 2026) | Dated UI, autofill misses (banking, iOS), mobile lacks web-vault features | Updates break autofill for weeks; wrong fields filled; price doubled without email notice |
| 1Password | Best UX, Secret Key, Watchtower, Travel Mode, family plan | Price (~$3.99/mo), no free plan, cloud/subscription only (local vaults dropped) | Price, support, removed standalone licences |
| Keeper | Enterprise admin, SSO, audit trails, personal + work vaults | Autofill friction, frequent re-login, complex admin, sharing limits | Broken updates, buggy extension, aggressive pricing changes |
| NordPass | Clean fast UX, XChaCha20, passkeys, email masking, 24/7 chat | Intro price then steep renewal, Linux/Windows bugs | Auto-renewal at higher price, refused refunds, class action over renewal disclosure |
| Passwork | Self-hosted, LDAP/AD, SSO, perpetual licence, granular permissions | Business-only, tricky setup, 1 year of updates on self-host | Convoluted setup, hard updates, inflexible user management |
| Proton Pass | Generous free tier, open source, encrypted metadata, aliases | Maturing features, weak business features, sharing needs paid | No biometric unlock in extension (until 2026), no QR scan for 2FA, nag pop-ups |
| Dashlane | Polished UI, VPN + dark-web monitoring | Most expensive, free plan removed (Sep 2025), web-only desktop, emergency access removed | Free plan killed at short notice, poor support, sync issues |
| KeePass | Free, open source, offline, plugins | No sync/sharing/recovery, dated UI, no official mobile app | Hard to sync, steep learning curve |

## Cross-market gaps we target

1. Autofill reliability — nobody measures or publishes it.
2. Regression-prone extension releases — no staged rollouts/rollback.
3. Storage choice without a UX penalty — polish vs control.
4. Pricing honesty — hikes, renewal traps, removed free tiers and licences.
5. Feature parity across platforms — mobile and Linux lag.
6. Extension security — DOM-injected autofill menus were clickjackable in 11 managers (DEF CON 2025).
7. Recovery and inheritance — lost vaults, removed emergency access.
8. Sharing outside the ecosystem — limited or paid.
9. Support quality.
10. Migration friction — plaintext CSV exports.

Full analysis: https://claude.ai/artifact/XhXpFKMZeUHsS9twUfbb9n
Development plan: https://claude.ai/artifact/8B92qxb91GRhWgePv4Eduu
