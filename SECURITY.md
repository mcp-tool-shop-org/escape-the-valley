# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.x     | Yes       |

## Reporting a Vulnerability

If you discover a security issue, please report it privately:

1. **Do not** open a public GitHub issue
2. Email: 64996768+mcp-tool-shop@users.noreply.github.com
3. Include: description, reproduction steps, and potential impact

We will acknowledge reports within 72 hours and aim to resolve confirmed vulnerabilities within 30 days.

## Threat Model

Escape the Valley is a single-player terminal game. Its threat surface is minimal:

**Network boundaries:**
- **Ollama GM** (on by default): `trail tui` / `trail new` / `trail play` issue HTTP to local Ollama at `OLLAMA_HOST` (default `http://localhost:11434`) on the first narrated turn. Pass `--gm-off` to disable. If Ollama is down, narration falls back to deterministic text; a missing GM never bricks a run. No data leaves the machine unless `OLLAMA_HOST` is explicitly pointed at a remote server.
- **XRPL Testnet** (off until enable): Connects to XRPL Testnet for ledger backpack features only after `trail ledger enable`. Wallet seeds are stored locally only, in a gitignored secrets sidecar (see Data at rest). No mainnet interaction.
- **Voice narration** (opt-in `--voice`): Local audio synthesis. No network calls.

**Data at rest:**
- Save files stored in `.trail/run.json` (local directory). Contains game state only — no wallet seeds.
- XRPL Testnet wallet/issuer seeds are kept out of `run.json` and persisted to a separate, gitignored secrets sidecar at `.trail/secrets.json`, keyed by run id. The whole `.trail/` directory is gitignored, so no save state or seeds are ever committed. The sidecar is written `0o600` where the platform supports it.
- No telemetry, analytics, or phoning home.
- No user accounts or authentication.

**Trust boundaries:**
- The game trusts Ollama responses for narrative text only; game mechanics are deterministic and cannot be influenced by GM output.
- XRPL operations are confined to Testnet with zero real-value tokens.
- No file operations outside `.trail/` and standard config paths.

## Security Design

- No secrets hardcoded in source
- No telemetry or tracking
- GM narration is on by default (HTTP to local Ollama); pass `--gm-off` to disable. XRPL is off until `trail ledger enable`. Voice is opt-in (`--voice`)
- XRPL wallet seeds are Testnet-only and stored locally in a gitignored secrets sidecar (`.trail/secrets.json`), never in `run.json` and never committed
- Error messages never expose stack traces to users
- CLI uses structured exit codes (0 success, 1 error)
