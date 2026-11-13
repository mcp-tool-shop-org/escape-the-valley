//! `etv-core`: Deterministic data layer and turn engine for Escape the Valley 2.0 (Phase 1a + 1b).
//!
//! Modules:
//! - [`rng`]: Bit-exact CPython MT19937 (`random.Random` & `SeededRNG`), counter replay, and lenient state parsing.
//! - [`model`]: Core game state (`RunState`, party, wagon, supplies, map nodes, journal, memory cards, intents, resources, backpack data).
//! - [`worldgen`]: Seeded world generation (`create_new_run`, `generate_map`, `generate_weather`, `generate_party`, `pick_twists`) and compiled-in `event_skeletons.json` loader.
//! - [`save`]: Save/load of schema v3 (and v0/v1/v2 → v3 migration), corrupt-save and corrupt-secrets quarantine, supply clamps, and atomic `.trail/secrets.json` sidecar.
//! - [`physics`]: Deterministic survival physics (consumption, travel distance, breakdowns, health effects, hunting, repairs, resting, morale, spoilage, night travel danger, escape valves, game-over, cause of death).
//! - [`memory`]: Memory store, budget eviction, pressures, themes, retrieval, GM brief builder, deterministic engine emitters, and GM card validation.
//! - [`events`]: Full 260-event library (60 hand-authored + 200 JSON), weighted selection, cooldowns, advertised inventory cost gating, outcome resolution, and state application.
//! - [`measure`]: Step 5 fallback-rate and latency measurement harness (no model call of its own).
//! - [`engine`]: Step-based deterministic turn engine (`StepEngine`), phase transitions (`Camp`, `Event`, `Route`, `GameOver`), route choices, town arrival trade, and graded endings (`compute_ending`).

pub mod content;
pub mod engine;
pub mod events;
pub mod events_builtin;
pub mod ledger;
pub mod measure;
pub mod memory;
pub mod model;
pub mod narrator;
pub mod notices;
pub mod physics;
pub mod play;
pub mod rng;
pub mod save;
pub mod screens;
pub mod settings;
pub mod snapshot;
pub mod tui;
pub mod validator;
pub mod worldgen;

pub use ledger::trail_ledger;
pub use narrator::{
    build_narration_context_from_turn, compute_applied_turn_deltas, CheckFailure, CheckReasonCode,
    CheckerContext, ComposedFallbackComposer, EndpointError, FallbackComposerError,
    FallbackPhraseTable, LiveNarrator, NarrationContext, Narrator, NarratorLength,
    NarratorSettings, NarratorStats, NarratorTransport, NullNarrator, OutcomeResponse,
    SceneResponse, SlotOutcome, SlotRecord, SourcePhraseEntry, SourcePhraseTable,
    StartupCheckStatus, TransportError, TurnMattersFlags, TurnNarration, DEFAULT_OLLAMA_ENDPOINT,
    RESOURCE_NOUN_TABLE, SHIPPED_FALLBACK_PHRASES_CANONICAL_SHA256,
    SHIPPED_FALLBACK_PHRASES_LF_SHA256, SOURCE_PHRASE_TABLE_SHA256,
};
