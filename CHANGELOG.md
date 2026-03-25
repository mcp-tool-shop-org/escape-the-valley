# Changelog

All notable changes to Escape the Valley are documented here.

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
