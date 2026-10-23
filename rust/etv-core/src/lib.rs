//! `etv-core`: Deterministic data layer for Escape the Valley 2.0 (Phase 1a).
//!
//! Modules:
//! - [`rng`]: Bit-exact CPython MT19937 (`random.Random` & `SeededRNG`), counter replay, and lenient state parsing.
//! - [`model`]: Core game state (`RunState`, party, wagon, supplies, map nodes, journal, memory cards, intents, resources, backpack data).
//! - [`worldgen`]: Seeded world generation (`create_new_run`, `generate_map`, `generate_weather`, `generate_party`, `pick_twists`) and compiled-in `event_skeletons.json` loader.
//! - [`save`]: Save/load of schema v3 (and v0/v1/v2 → v3 migration), corrupt-save quarantine, supply clamps, and atomic `.trail/secrets.json` sidecar.

pub mod model;
pub mod rng;
pub mod save;
pub mod worldgen;
