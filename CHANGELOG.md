# Changelog

All notable changes to Escape the Valley are documented here.

## [Unreleased]

### Added

- **Multiplayer parcel trading** over XRPL: `trail parcel send <addr> <supply> <amount>`, `trail parcel list`, `trail parcel accept <id>`, `trail parcel sent`, and `trail wallet share` — send supply tokens to another traveler via a Testnet micropayment with an attached memo, and accept incoming parcels into your backpack.
- **Ledger reconciliation proof harness** (audit mode): an on-ledger reconciliation proof that replays settlement receipts and verifies them against the XRPL Testnet, so a run's supply history can be independently audited.
- `--gm-profile` and `--weirdness` options on `trail tui`, matching the classic `trail new` CLI, so the GM voice and weirdness band can be set when launching the TUI.

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
