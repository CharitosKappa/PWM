# PWM — plan files for Claude Code

## How to start

1. Create an empty folder (or Git repo) for the project and copy these files into its root, keeping the `docs/` folder.
2. Open the folder in Claude Code. It reads `CLAUDE.md` automatically.
3. First prompt:

   > Read CLAUDE.md and all files in docs/. Then start docs/ROADMAP.md at M0.1. Work one task at a time, tick each task when its acceptance criteria pass, and stop to ask me when you hit an open decision in docs/DECISIONS.md.

4. After M0, continue with: `Continue with the next unchecked task in docs/ROADMAP.md.`

## Before you go far

- The project is closed source (decision S6): keep the repo private and have a lawyer write the EULA/Terms.
- Settle D3 (desktop UI framework) in `docs/DECISIONS.md` before M7.
- Get the crypto spec (`docs/SECURITY.md`) reviewed by a cryptographer before M1 is marked done.
- iOS/macOS work (M5) needs a Mac with Xcode and an Apple Developer account; Android (M6) needs Android Studio.

## Files

| File | Purpose |
| --- | --- |
| `CLAUDE.md` | Rules and workflow for Claude Code |
| `docs/PRODUCT.md` | Positioning and feature scope |
| `docs/ARCHITECTURE.md` | Repo layout, stack, API |
| `docs/SECURITY.md` | Crypto and threat model |
| `docs/AUTOFILL.md` | Autofill engine and extension security |
| `docs/QUALITY.md` | Tests, CI, releases, metrics |
| `docs/PRICING.md` | Plans and commitments |
| `docs/ROADMAP.md` | Milestone task list |
| `docs/DECISIONS.md` | Decision log |
| `docs/COMPETITORS.md` | Competitor context |
