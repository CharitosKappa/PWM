# Design tokens

`tokens.json` is the single source for colors, type, spacing and radius in every UI (SwiftUI, Compose, Tauri web UI, extension). Values are **placeholders** until the brand is chosen (decision D1); the names are meant to stay.

## Format

[W3C Design Tokens](https://www.designtokens.org/) (`$type`, `$value`, `$description`). Aliases use `{group.token}`. The top-level `version` is bumped on any rename or removal so platform generators can fail loudly instead of silently dropping a token.

## Groups

| Group | Tokens | Notes |
| --- | --- | --- |
| `color.light`, `color.dark` | `bg`, `surface`, `surface-raised`, `border`, `text`, `text-muted`, `accent`, `accent-text`, `focus`, `success`, `warning`, `danger` | Both themes define the same names. Text/background pairs must meet WCAG AA (4.5:1 body, 3:1 large text and UI). `border` is for decorative dividers only (below 3:1); outlines that identify a control use `text-muted`. |
| `font.family` | `sans`, `mono` | System fonts; `mono` is for passwords, Secret Key and TOTP codes. |
| `font.weight` | `regular`, `medium`, `semibold` | |
| `type` | `caption` 12, `body` 14, `body-strong` 14, `title` 17, `headline` 22, `display` 28, `secret` 15 mono | Sizes are base values; platforms scale them with the user's text-size setting (Dynamic Type, Android font scale). |
| `space` | `0`–`7` = 0, 4, 8, 12, 16, 24, 32, 48 px | 4 px grid. |
| `radius` | `none`, `sm` 4, `md` 8, `lg` 12, `full` | |

## Rules

- Use tokens, not literal values, in UI code. Add a token here before using a new value.
- No layout shift: components must reserve space for states (errors, loading) so nothing moves under the cursor.
- Platform code generation (Swift, Kotlin, CSS variables) is added with the first app that consumes the tokens (M4.2 / M5.1).
