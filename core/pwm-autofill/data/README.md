# Public Suffix List (decision D33)

Vendored, unmodified, compiled into `pwm-autofill`. Refresh before each release with `just psl` and review the diff like any other change; the apps never download it.

| File | Source | Licence | Version | SHA-256 |
| --- | --- | --- | --- | --- |
| `public_suffix_list.dat` | https://publicsuffix.org/list/public_suffix_list.dat | MPL-2.0 (data, unmodified; credited in the third-party notices) | 2026-10-31_14-17-28_UTC, commit 73f8cc701c084fe07d888ec552b2f0b53281a674 | `617d1b276660e5a01209d7b58159450e400cf636634a7a9c2cfd50a1c6675d01` |
| `test_psl.txt` | https://raw.githubusercontent.com/publicsuffix/list/main/tests/test_psl.txt | CC0-1.0 | same date | `8f50ad958916d6a8f79fba2363501475571acce752757f9126fe9d2f17dd920d` |

A test checks that the list's `VERSION` line matches this table, so a refresh must update it.
