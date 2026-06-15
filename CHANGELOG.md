# Changelog

All notable changes to Escape the Valley are documented here.

## [Unreleased]

### Added

- **Multiplayer parcel trading** over XRPL: `trail parcel send <addr> <supply> <amount>`, `trail parcel list`, `trail parcel accept <id>`, `trail parcel sent`, and `trail wallet share` — send supply tokens to another traveler via a Testnet micropayment with an attached memo, and accept incoming parcels into your backpack.
- **Ledger reconciliation proof harness** (audit mode): an on-ledger reconciliation proof that replays settlement receipts and verifies them against the XRPL Testnet, so a run's supply history can be independently audited.
- `--gm-profile` and `--weirdness` options on `trail tui`, matching the classic `trail new` CLI, so the GM voice and weirdness band can be set when launching the TUI.

### Changed (Stage-C humanization)

- **Async, non-freezing GM and ledger calls:** narration and XRPL round-trips run off the UI thread, so a slow Ollama warm-up or a flaky Testnet no longer freezes the TUI. The first narrated turn loads the model and can take 10-30s by design — this is now documented as expected, not a hang.
- **Reachable escape valves:** hard ration, desperate repair, and abandon cargo are reachable from the TUI as genuine last resorts, with cooldowns and side effects intact.
- **Degradation signals surfaced to the UI:** `StepMessages` carries `gm_degraded` / `gm_degraded_reason`, the engine tracks `gm_calls` / `gm_fallbacks`, `GMClient` exposes a `.stats` dict, `VoiceBridge` exposes availability/last-error status, and `BackpackState` carries `last_settle_failed` — so the player can see when the GM has fallen back, when voice is unavailable, and when a settlement failed, instead of guessing.
- **Corrupt-save backup:** on a corrupt or unreadable `run.json`, the engine renames it to `run.json.corrupt-<timestamp>` before refusing it, so the next save can't clobber the only evidence and the file is recoverable.
- **Robust event loading:** the event loader tolerates malformed event entries and validates resource keys, so a single bad event definition degrades to a skip instead of crashing the run.
- **Testnet-only guard:** ledger operations assert the XRPL Testnet endpoint with a request timeout, keeping the game off mainnet and resilient to a stalled network.
- **Troubleshooting docs:** a new Troubleshooting handbook page (plus a README section) keyed to the three real failure modes — no narration ⇒ start Ollama or use `--gm-off`; ledger pending ⇒ `trail ledger reconcile`; save won't resume ⇒ recover from the `run.json.corrupt-*` backup. `trail self-check` is documented as the first thing to run.

### Fixed (Stage-A hardening)

- **Determinism across save/load:** the full PRNG state is now persisted, so resuming a saved game reproduces the same world and event stream as an uninterrupted run.
- **Secrets sidecar:** wallet and issuer seeds are split into a local `.trail/secrets.json` sidecar instead of the main save file, keeping private key material out of shareable saves.
- **Weirdness `>= 2` gate:** the uncanny is only available once a run crosses into `weirdness_level >= 2` (with tokens remaining); weirdness 0-1 stays grounded survival with no uncanny-token spend.
- **Ledger idempotency:** `ledger enable` and `accept_parcel` are now idempotent — repeating them does not double-enable, double-credit, or corrupt balances; balances are parsed exactly without binary-float drift.
- **External memo verification:** settlement memo verification reads the memo back from the ledger rather than trusting local state, making the receipt check genuinely external.
- **TUI persistence:** the TUI persists ledger and parcel state across actions, and guards outcome rendering against non-numeric deltas and missing labels.
- **Tone-lint on outcomes:** outcome narration is tone-linted so the engine's plain consequences read in the GM's period voice; event-caused deaths are attributed to their cause instead of a generic "The trail."
- **CI action pinning:** all GitHub Actions are pinned to full commit SHAs (publish, release-binaries), and CI installs the `xrpl` extra so the mock-based XRPL tests run instead of silently skipping.

## [1.0.1] - 2026-03-25

### Added
- `--version` / `-V` flag on root CLI (Typer callback)
- `trail stats` command — run summary with `--json` output (party, distance, wagon, journal, outcome)
- 7 new tests (347 total)

## [1.0.0] - 2026-03-04

### Added

- Full-screen Textual TUI with camp actions, event choices, and route forks
- Three GM profiles: Chronicler (spare), Fireside (default), Lantern-Bearer (weird)
- Voice narration via local audio synthesis (optional)
- XRPL Testnet ledger backpack: supply tokens, settlement receipts, parcels (optional)
- Procedural world generation with seeded RNG for reproducible runs
- Party traits, twist deck, and doctrine/taboo system
- Maintenance windows (rest+repair combo) for breakdown resistance
- Escape valves: hard ration, desperate repair, abandon cargo
- Warning callouts toggle (`--callouts verbose|minimal`)
- Oregon Trail-style death causes in trail ledger
- Memory card system for GM narrative continuity
- Supply cache discovery at nodes
- Spoilage mechanic (food decays without salt)
- Night travel danger (needs lantern oil)
- Classic CLI mode (`trail new`, `trail play`)
- Save/load with full backward compatibility
- 316 tests, lint clean
- Survival guide (`docs/survival-guide.md`)
