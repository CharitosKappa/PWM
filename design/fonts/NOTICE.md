# Font files

The app fonts chosen for D1: Manrope for text, Geist Mono for passwords, one-time codes and the Secret Key. Both are licensed under the SIL Open Font License 1.1, which is allowed for font files only, never code (D1, S6). The files are unmodified; only their names changed, so build tools and Android resources accept them.

Source: the Google Fonts repository, https://github.com/google/fonts, commit `bbda7253fc363238b05d97515ca71f8ae49c4fa9`.

## Manrope.ttf

Manrope, variable weight 200–800. Copyright 2018 The Manrope Project Authors (https://github.com/googlefonts/manrope).
Upstream: https://github.com/aaronbell/manrope
File: `ofl/manrope/Manrope[wght].ttf`, 164,700 bytes, SHA-256 `3ae11c49db0455a3cc33e37d380f20fdb8c7f8b41dc07625c177e3d87a9d6ae6`.
Licence: SIL Open Font License 1.1, full text in `OFL-Manrope.txt` (SHA-256 `58172e0c0fac2cda8a37b348164bb55e44b0e69051e557e92b1d3f6910141f7b`).

## GeistMono.ttf

Geist Mono, variable weight 100–900. Copyright 2024 The Geist Project Authors (https://github.com/vercel/geist-font).
Upstream: https://github.com/vercel/geist-font
File: `ofl/geistmono/GeistMono[wght].ttf`, 171,948 bytes, SHA-256 `d00e590b8eb3a59acc329b2d044fd143ae935090b7da33199ebee27cc7de8196`.
Licence: SIL Open Font License 1.1, full text in `OFL-GeistMono.txt` (SHA-256 `1781d2806a07d91c4edf4740b88449fab7d0eadad53f7c351b94cd4d4eb8c00f`).

## Rules

- Ship the licence text with the fonts: `just notices` puts this file and both OFL texts in `THIRD_PARTY_NOTICES/fonts.md`, which every app shows; the extension and the desktop app also carry them next to the font files.
- Never sell the fonts on their own, and never modify them. A modified font must not use the name "Manrope" or "Geist Mono".
- `scripts/fonts.mjs` copies the two font files into the Apple and Android projects; `just check` fails when a copy differs.
