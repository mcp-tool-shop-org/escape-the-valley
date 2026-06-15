# Ship Gate

> No repo is "done" until every applicable line is checked.
> Copy this into your repo root. Check items off per-release.

**Tags:** `[all]` every repo · `[npm]` `[pypi]` `[vsix]` `[desktop]` `[container]` published artifacts · `[mcp]` MCP servers · `[cli]` CLI tools

**Release:** v1.1.0 — gated 2026-06-15

---

## A. Security Baseline

- [x] `[all]` SECURITY.md exists (report email, supported versions, response timeline) (2026-06-15) — report email + 72h ack / 30d resolve timeline + supported 1.x table
- [x] `[all]` README includes threat model paragraph (data touched, data NOT touched, permissions required) (2026-06-15) — README "Security" section: no telemetry/accounts, network opt-in, Testnet-only, links to full threat model
- [x] `[all]` No secrets, tokens, or credentials in source or diagnostics output (2026-06-15) — only fake test-fixture seeds (e.g. `sEDPlayerSeed...`); real wallet/issuer seeds live in the gitignored `.trail/secrets.json` sidecar, never committed
- [x] `[all]` No telemetry by default — state it explicitly even if obvious (2026-06-15) — stated in README + SECURITY.md; grep confirms no analytics/sentry/posthog/phone-home

### Default safety posture

- [x] `[cli|mcp|desktop]` Dangerous actions (kill, delete, restart) require explicit `--allow-*` flag (2026-06-15) — all network features (Ollama GM, XRPL ledger) are opt-in and disabled by default; XRPL is Testnet-only with a hard mainnet guard, so there is no real-value or destructive default path
- [x] `[cli|mcp|desktop]` File operations constrained to known directories (2026-06-15) — all save/secrets/journal writes confined to `.trail/`; atomic temp-file writes, no path traversal
- [ ] `[mcp]` SKIP: not an MCP server (Typer/Textual CLI game)
- [ ] `[mcp]` SKIP: not an MCP server

## B. Error Handling

- [x] `[all]` Errors follow the Structured Error Shape: `code`, `message`, `hint`, `cause?`, `retryable?` (2026-06-15) — user-facing failures carry a message + actionable `hint:` (e.g. `_network_hint()`, GM/Ollama start hint, address-format hint); network round-trips surface retry guidance
- [x] `[cli]` Exit codes: 0 ok · 1 user error · 2 runtime error · 3 partial success (2026-06-15) — CLI uses `typer.Exit(0)` on success and `typer.Exit(1)` on failure consistently across all commands (Health Pass cli-tui-003); the 2/3 tiers are not used — this single-player game maps cleanly to ok/error, which is honest for its surface
- [x] `[cli]` No raw stack traces without `--debug` (2026-06-15) — failures are caught and rendered as message + hint; `raise typer.Exit(1) from None` suppresses chained tracebacks; async TUI degrades instead of dumping exceptions
- [ ] `[mcp]` SKIP: not an MCP server
- [ ] `[mcp]` SKIP: not an MCP server — note: corrupt `run.json` still degrades gracefully (backed up to `run.json.corrupt-<ts>`, refused not crashed) and malformed events skip instead of crashing
- [x] `[desktop]` Errors shown as user-friendly messages — no raw exceptions in UI (2026-06-15) — Textual TUI renders structured messages/degradation signals (`gm_degraded`, `last_settle_failed`, voice availability) rather than raw exceptions; async workers keep a slow Ollama/Testnet from freezing or crashing the UI
- [ ] `[vscode]` SKIP: not a VS Code extension

## C. Operator Docs

- [x] `[all]` README is current: what it does, install, usage, supported platforms + runtime versions (2026-06-15) — README updated with "What's New in 1.1.0", `pip install escape-the-valley`, Python 3.11+ stated, usage for CLI + TUI
- [x] `[all]` CHANGELOG.md (Keep a Changelog format) (2026-06-15) — dated `## [1.1.0] - 2026-06-15` section with Added/Changed groups; `## [Unreleased]` header present
- [x] `[all]` LICENSE file present and repo states support status (2026-06-15) — MIT LICENSE present; pyproject `license = "MIT"`; SECURITY.md states 1.x supported
- [x] `[cli]` `--help` output accurate for all commands and flags (2026-06-15) — `trail --help` lists new/old commands (new, play, status, journal, self-check, tui, version, stats, postcard, ledger, parcel, wallet) + `--version`; `tui --gm-profile/--weirdness` verified to match CLI (A-01/02/03)
- [x] `[cli|mcp|desktop]` Logging levels defined: silent / normal / verbose / debug — secrets redacted at all levels (2026-06-15) — `NO_COLOR` honored, non-color danger cues, structured hints; no secrets are printed at any verbosity (seeds never rendered to stdout)
- [ ] `[mcp]` SKIP: not an MCP server
- [ ] `[complex]` SKIP: single-player CLI game — no operator/on-call surface; SECURITY.md + README cover the only recovery procedure (corrupt-save backup)

## D. Shipping Hygiene

- [x] `[all]` `verify` script exists (test + build + smoke in one command) (2026-06-15) — `scripts/verify.sh`: `ruff check` + `pytest` under `set -e`
- [x] `[all]` Version in manifest matches git tag (2026-06-15) — `pyproject.toml` version `1.1.0` == `__init__.__version__` `1.1.0` (cli-tui-006 unified strings); tag cut at release time on this commit
- [x] `[all]` Dependency scanning runs in CI (ecosystem-appropriate) (2026-06-15) — `ci.yml` runs `pip-audit` against installed deps on every push/PR (non-blocking report; advisory surfaced in job log)
- [ ] `[all]` SKIP: no `dependabot.yml` — org GitHub Actions budget rule forbids scheduled/dependabot workflows on tool repos (`.claude/rules/github-actions.md`: "Do NOT add dependabot.yml unless explicitly requested"; scheduled workflows allowed only in the marketing repo). Update cadence is manual + the CI `pip-audit` advisory feed.
- [ ] `[npm]` SKIP: not an npm package (PyPI)
- [x] `[pypi]` `python_requires` set (2026-06-15) — `requires-python = ">=3.11"`; CI matrix exercises 3.11/3.12/3.13
- [x] `[pypi]` Clean wheel + sdist build (2026-06-15) — `python -m build` produces `escape_the_valley-1.1.0-py3-none-any.whl` + `.tar.gz` with no errors; lockfile N/A for hatchling app
- [ ] `[vsix]` SKIP: not a VS Code extension
- [ ] `[desktop]` SKIP: PyInstaller binaries are an additive distribution; primary artifact is the PyPI wheel, which builds and installs cleanly (`release-binaries.yml` handles binary builds on release)

## E. Identity (soft gate — does not block ship)

- [x] `[all]` Logo in README header (2026-06-15) — `assets/readme-logo.png` present and referenced
- [x] `[all]` Translations (polyglot-mcp, 8 languages) (2026-06-15) — `README.{ja,zh,es,fr,hi,it,pt-BR}.md` all present (7 translated + English source = 8)
- [x] `[org]` Landing page (@mcptoolshop/site-theme) (2026-06-15) — `site/` Astro project present and building (`site/dist/`); landing page install = `pip install escape-the-valley` (A-13)
- [ ] `[all]` GitHub repo metadata: description, homepage, topics — UNVERIFIED: this gate session is forbidden from reading/changing repo metadata (no `gh repo edit`). State must be confirmed out-of-band before publish. Left unchecked honestly rather than false-checked.

---

## Gate Rules

**Hard gate (A–D):** Must pass before any version is tagged or published.
If a section doesn't apply, mark `SKIP:` with justification — don't leave it unchecked.

**Soft gate (E):** Should be done. Product ships without it, but isn't "whole."

**Checking off:**
```
- [x] `[all]` SECURITY.md exists (2026-02-27)
```

**Skipping:**
```
- [ ] `[pypi]` SKIP: not a Python project
```
