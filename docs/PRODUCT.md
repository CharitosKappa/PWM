# Product

## Positioning

1Password-level polish, KeePass-level control, and security proven by published independent audits (closed source, see decision S6). We win on the two things every competitor is reviewed down for: **autofill reliability** and **pricing honesty**.

## Promises (each maps to engineering work)

| Promise | What it means in the product |
| --- | --- |
| Autofill that works | Public per-site compatibility dashboard; in-extension "this site failed" report; fix SLA for top 1,000 sites |
| Prices that don't ambush you | Price lock, renewal reminders, one-click cancel, permanent free tier (see `PRICING.md`) |
| Your vault, your storage | Hosted cloud, self-hosted server, or local encrypted file — same apps |
| Same features everywhere | iOS, Android, Windows, macOS, Linux and extensions ship together |

## Target users

- Launch: individuals and families leaving Bitwarden, Dashlane, LastPass, 1Password after 2025–26 price and plan changes.
- Phase 3: teams and self-hosted businesses (Passwork / Keeper territory).

## Feature scope

Priority: P0 = launch blocker · P1 = within 6 months of launch · P2 = later. Phase numbers match `ROADMAP.md`.

### Table stakes (match the best competitors)

| Feature | Learned from | Priority | Phase |
| --- | --- | --- | --- |
| Zero-knowledge E2E encryption (XChaCha20-Poly1305, Argon2id) | Bitwarden, NordPass | P0 | 1 |
| Secret Key dual-secret login | 1Password | P0 | 1 |
| Autofill on web, iOS, Android incl. passkeys and TOTP | All | P0 | 1 |
| Password generator + health report (weak, reused, breached) | 1Password Watchtower | P0 | 1 |
| Biometric unlock everywhere, incl. extensions | Proton Pass gap | P0 | 1 |
| Unlimited devices and items on free | Bitwarden, Proton Pass | P0 | 1 |
| Secure notes, cards, identities, attachments | All | P1 | 2 |
| Family vaults and sharing | 1Password, Bitwarden | P1 | 2 |
| Email aliases | Proton Pass | P2 | 3 |
| Travel Mode | 1Password | P2 | 3 |
| Admin console, SSO, SCIM, audit log | Keeper, Passwork | P1 | 3 |
| Self-hosted server with LDAP/AD | Passwork, Bitwarden | P1 | 3 |

### Differentiators (gaps in the market)

| Feature | Gap closed | Priority | Phase |
| --- | --- | --- | --- |
| Public autofill dashboard + "this site failed" report | No vendor publishes reliability | P0 | 1 |
| Direct encrypted importers for 8 competitors, no plaintext CSV left on disk | CSV-only migration | P0 | 1 |
| Full mobile parity (health report, sharing, settings) | Bitwarden mobile lag | P0 | 1 |
| Three storage modes: cloud, self-hosted, local file + sync | Polish vs control trade-off | P1 | 2 |
| KeePass .kdbx import/export and live file sync | KeePass users lack good apps | P1 | 2 |
| Encrypted one-time share links to non-users, on free | Dashlane / Proton sharing limits | P1 | 2 |
| Emergency access with time delay + recovery kit | Dashlane removed it; KeePass has none | P1 | 2 |
| Native Linux desktop app | Missing in Dashlane, NordPass | P1 | 2 |

### Review complaints we design out

| Complaint | Seen at | Our fix | Phase |
| --- | --- | --- | --- |
| Updates break autofill for weeks | Bitwarden, Keeper | Staged rollouts, auto-rollback, top-1,000 regression suite | 1 |
| Wrong fields filled (email/password/TOTP) | Bitwarden | Field classifier + per-site overrides, tested in CI | 1 |
| Clunky, dated UI | Bitwarden, KeePass | Shared design system; usability test each release | 1 |
| Frequent forced re-login | Keeper | Configurable lock timeout, biometric re-unlock | 1 |
| Surprise renewals, refused refunds | NordPass | Price lock, 14-day notice, pro-rata refunds | 1 |
| Price doubled without notice | Bitwarden | 60-day email notice, 12-month price hold | 1 |
| Features removed (free tier, desktop app, local vaults) | Dashlane, 1Password | 12-month removal notice; export always free | 1 |
| Support "nonexistent" | 1Password, Dashlane | In-app chat (paid), 24h email SLA (free), status page | 2 |
| Review nag pop-ups, UI moving under cursor | Proton Pass | No prompts during autofill; no layout shift | 1 |
| Hard self-host setup | Passwork | One-command Docker install, auto-update channel | 3 |
