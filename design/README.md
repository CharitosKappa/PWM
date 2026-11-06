# Design tokens

`tokens.json` is the single source for colors, type, spacing and radius in every UI (SwiftUI, Compose, Tauri web UI, extension). Values follow the **Calm** direction picked for D1 on 2026-11-05: white and lavender surfaces, one indigo accent, generous rounding, pill buttons. The product name is still open. The mockups are on the design canvas (PWM app mockups). Type is Manrope for text and Geist Mono for passwords, codes and the Secret Key (D1, SIL OFL 1.1, allowed for font files only). The font files are in `fonts/` with their `NOTICE.md` and licence texts; every app bundles them, and the system fonts later in each stack are only a fallback.

## Format

[W3C Design Tokens](https://www.designtokens.org/) (`$type`, `$value`, `$description`). Aliases use `{group.token}`. The top-level `version` is bumped on any rename or removal so platform generators can fail loudly instead of silently dropping a token.

## Groups

| Group | Tokens | Notes |
| --- | --- | --- |
| `color.light`, `color.dark` | `bg`, `surface`, `surface-raised`, `border`, `text`, `text-muted`, `accent`, `accent-text`, `accent-soft`, `focus`, `success`, `warning`, `danger` | Both themes define the same names. Text/background pairs must meet WCAG AA (4.5:1 body, 3:1 large text and UI). `border` is for decorative dividers only (below 3:1); outlines that identify a control use `text-muted`. `accent-soft` is a background for secondary buttons and selected chips; text on it uses `accent` (5.9:1 in both themes). |
| `font.family` | `sans` Manrope, `mono` Geist Mono | Each is followed by system fonts as fallbacks. `mono` is for passwords, Secret Key and TOTP codes. The font files (`fonts/`) ship unmodified, with a `NOTICE.md` and the OFL text in the notices; `scripts/fonts.mjs` copies them into the Apple and Android projects. |
| `font.weight` | `regular`, `medium`, `semibold` | |
| `type` | `caption` 12, `body` 14, `body-strong` 14, `title` 17, `headline` 22, `display` 28, `secret` 15 mono | Sizes are base values; platforms scale them with the user's text-size setting (Dynamic Type, Android font scale). |
| `space` | `0`–`7` = 0, 4, 8, 12, 16, 24, 32, 48 px | 4 px grid. |
| `radius` | `none`, `sm` 12, `md` 16, `lg` 20, `full` | |

## Rules

- Use tokens, not literal values, in UI code. Add a token here before using a new value.
- No layout shift: components must reserve space for states (errors, loading) so nothing moves under the cursor.
- Platform code generation (Swift, Kotlin, CSS variables) is added with the first app that consumes the tokens (M4.2 / M5.1).
