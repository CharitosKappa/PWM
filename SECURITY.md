# Security policy

We take reports about the security of PWM seriously and thank everyone who reports issues responsibly. The crypto design is specified in [`docs/SECURITY.md`](docs/SECURITY.md).

## Reporting a vulnerability

- Email **security@pwm.example** (placeholder until the product name is chosen, decision D1). PGP key: to be published with the website.
- Do not open a public issue, post on social media or contact support for security reports.
- Include: affected product and version, steps to reproduce, impact, and any proof-of-concept. Never include another person's data.

## What to expect

| Step | Target |
| --- | --- |
| Acknowledgement | within 2 business days |
| Triage and severity (CVSS) | within 5 business days |
| Fix for critical / high issues | within 30 days |
| Fix for medium / low issues | within 90 days |
| Public advisory | after the fix ships, credited to you unless you prefer otherwise |

We keep you informed throughout and agree a disclosure date with you. If we miss these targets we tell you why.

## Scope

In scope: apps (iOS, Android, Windows, macOS, Linux), browser extensions, the sync server and API, the self-hosted server image, and the crypto core.

Out of scope: social engineering, physical attacks, denial of service by volume, findings that require a compromised unlocked device (see the threat model), and reports from automated scanners without a demonstrated impact.

## Safe harbour

If you act in good faith, follow this policy, avoid privacy violations and service disruption, and only use accounts you own, we will not pursue legal action against you for your research. A public bug bounty with rewards opens with the public beta (roadmap M10.5).
