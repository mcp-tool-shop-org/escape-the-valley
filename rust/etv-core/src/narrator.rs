//! Narrator seam (`Narrator`, `NullNarrator`, `NarrationContext`, `PartyStatusFacts`, and
//! `TurnMattersFlags`).
//!
//! Provides a strictly read-only presentation hook for an AI narrator:
//! - [`NarrationContext`] is an owned, read-only snapshot of engine facts for a shown turn
//!   (time of day, game phase, biome, journey day, top pressures and themes, weirdness allowance,
//!   GM profile, nameless party counts/bands, event and choice IDs, shown effects/deltas/lines,
//!   place, journey band, flags, and the deterministic `fallback_line`). It holds **no** reference
//!   or handle to [`crate::rng::CpythonRng`], [`crate::model::RunState`], the save path, or any
//!   mutable engine state.
//! - [`TurnMattersFlags`] is derived by a pure read of effects and state fields the engine has
//!   already computed on that turn, with **zero RNG draws and zero state mutation**.
//! - [`Narrator::narrate`] is called once on every turn that shows the player a turn result.
//! - [`NullNarrator`] is the default implementation and always returns `None`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::content::{canonicalize_json_value_no_floats, hex_encode_32, ContentPack};
use crate::engine::StepMessages;
use crate::memory::build_gm_brief_readonly;
use crate::model::{
    Condition, GMProfile, GamePhase, IntentAction, PlayerIntent, RunState, TimeOfDay,
};
use crate::physics::{current_node, journey_pressure, HealthEffect};

/// Embedded approved fallback phrase table (`rust/etv-core/content/fallback_phrases.json`).
pub const EMBEDDED_FALLBACK_PHRASES_JSON: &str = include_str!("../content/fallback_phrases.json");

/// Expected SHA-256 digest of the approved source phrase table (`PHRASE-TABLE-DRAFT-2-2026-11-02.json`).
pub const SOURCE_PHRASE_TABLE_SHA256: &str =
    "ef29eecf13ba985708d21ad9f002ab4a2de5af4aa70dc914f8c1135f4bf12a22";

/// Expected SHA-256 digest of the canonical JSON form (`canonicalize_json_value_no_floats`) of
/// `rust/etv-core/content/fallback_phrases.json`.
pub const SHIPPED_FALLBACK_PHRASES_CANONICAL_SHA256: &str =
    "7d9c4eb4c86ac3cab2089041e180234ffb403abc7120a542e4fec8b948a6959f";

/// Expected SHA-256 digest of the LF-normalized bytes of `rust/etv-core/content/fallback_phrases.json`.
pub const SHIPPED_FALLBACK_PHRASES_LF_SHA256: &str =
    "42fa23cb0016971cbd7960693880854cc81296b6649af93186c2328ec2067e75";

/// Default local loopback Ollama endpoint (`src/escape_the_valley/gm.py:106`).
pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://127.0.0.1:11434";

/// Two-call per-turn narrator output (`scene` and `outcome`).
///
/// Either field is `None` when the narrator is off, unreachable, times out, returns invalid
/// JSON, or fails the hard-rule checker.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnNarration {
    pub scene: Option<String>,
    pub outcome: Option<String>,
}

/// Read-only trait for an optional turn narrator.
///
/// Implementations receive a frozen [`NarrationContext`] and may return presentation prose
/// (`Some(String)`) or `None` to fall back to [`NarrationContext::fallback_line`].
pub trait Narrator {
    fn narrate(&self, ctx: &NarrationContext) -> Option<String>;

    /// Perform full per-turn narration (both scene and outcome calls for the live narrator,
    /// or `scene: None` + `outcome: self.narrate(ctx)` for single-call test narrators).
    fn narrate_turn(&self, ctx: &NarrationContext) -> TurnNarration {
        TurnNarration {
            scene: None,
            outcome: self.narrate(ctx),
        }
    }
}

/// Default no-op narrator that always returns `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NullNarrator;

impl Narrator for NullNarrator {
    fn narrate(&self, _ctx: &NarrationContext) -> Option<String> {
        None
    }
}

/// Breakdown of the five "turn matters" rules derived by a pure read of post-step engine effects
/// and state fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnMattersFlags {
    /// A party member died on this turn (`StepMessages.effects` contains `HealthEffect::Died` or
    /// a member's `health` transitioned from `> 0` to `<= 0`).
    pub death: bool,
    /// A party member was injured on this turn (`PartyMember.health` fell below its pre-turn value).
    pub injury: bool,
    /// A supply key reached zero on this turn (`SuppliesState.items[k]` transitioned from `> 0`
    /// to `<= 0`).
    pub supply_zero: bool,
    /// The party arrived at a map node / landmark on this turn (`RunState.location_id` advanced to
    /// the destination node in `StepEngine::arrive_at_next_node`).
    pub landmark_arrival: bool,
    /// The party ended the turn camped at night (`pre.phase == GamePhase::Camp`,
    /// `!state.game_over`, `intent.action != IntentAction::Travel`, and post-step
    /// `state.time_of_day == TimeOfDay::Night`).
    pub night_at_camp: bool,
}

impl TurnMattersFlags {
    /// Returns `true` if any of the five "turn matters" rules fired.
    pub fn any(&self) -> bool {
        self.death || self.injury || self.supply_zero || self.landmark_arrival || self.night_at_camp
    }
}

/// Nameless party status facts (counts and coarse bands only — **never** member names).
///
/// The voice guide forbids member names in narration, so party status is represented strictly as
/// aggregate counts and bands derived from [`crate::model::PartyState`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyStatusFacts {
    alive_count: usize,
    total_count: usize,
    sick_or_injured_count: usize,
    critical_health_count: usize,
    exhausted_count: usize,
    health_band: String,
    morale_band: String,
}

impl PartyStatusFacts {
    /// Compute nameless party counts and bands from `state` (`memory.rs:137-164`, `model.rs:617-628`).
    pub fn from_state(state: &RunState) -> Self {
        let alive_count = state.party.alive_count();
        let total_count = state.party.members.len();
        let sick_or_injured_count = state
            .party
            .members
            .iter()
            .filter(|m| m.is_alive() && matches!(m.condition, Condition::Sick | Condition::Injured))
            .count();
        let critical_health_count = state
            .party
            .members
            .iter()
            .filter(|m| m.is_alive() && m.health < 30)
            .count();
        let exhausted_count = state
            .party
            .members
            .iter()
            .filter(|m| m.is_alive() && m.condition == Condition::Exhausted)
            .count();

        let avg_health = state.party.avg_health();
        let health_band = if alive_count == 0 {
            "dead"
        } else if critical_health_count >= 2 || avg_health < 30 {
            "critical"
        } else if sick_or_injured_count >= 1 || avg_health < 65 {
            "worn"
        } else {
            "steady"
        }
        .to_string();

        let morale = state.party.morale;
        let morale_band = if alive_count == 0 || morale < 15 {
            "broken"
        } else if morale < 30 {
            "low"
        } else if morale < 65 {
            "steady"
        } else {
            "high"
        }
        .to_string();

        Self {
            alive_count,
            total_count,
            sick_or_injured_count,
            critical_health_count,
            exhausted_count,
            health_band,
            morale_band,
        }
    }

    /// Number of living party members (`PartyState::alive_count`).
    pub fn alive_count(&self) -> usize {
        self.alive_count
    }

    /// Total number of party members at run start (`PartyState::members.len()`).
    pub fn total_count(&self) -> usize {
        self.total_count
    }

    /// Number of living party members currently `Sick` or `Injured`.
    pub fn sick_or_injured_count(&self) -> usize {
        self.sick_or_injured_count
    }

    /// Number of living party members with `health < 30`.
    pub fn critical_health_count(&self) -> usize {
        self.critical_health_count
    }

    /// Number of living party members currently `Exhausted`.
    pub fn exhausted_count(&self) -> usize {
        self.exhausted_count
    }

    /// Coarse party health band (`"steady"`, `"worn"`, `"critical"`, or `"dead"`).
    pub fn health_band(&self) -> &str {
        &self.health_band
    }

    /// Coarse party morale band (`"high"`, `"steady"`, `"low"`, or `"broken"`).
    pub fn morale_band(&self) -> &str {
        &self.morale_band
    }
}

/// Owned, read-only snapshot of engine facts for a single shown turn.
///
/// `NarrationContext` intentionally contains no reference or field for [`crate::model::RunState`],
/// [`crate::rng::CpythonRng`], or any save file handle.
///
/// # Compile-level isolation checks
///
/// `NarrationContext` cannot reach `&mut RunState` or `rng`:
///
/// ```compile_fail
/// use etv_core::narrator::{NarrationContext, Narrator};
///
/// struct BadNarrator;
/// impl Narrator for BadNarrator {
///     fn narrate(&self, ctx: &NarrationContext) -> Option<String> {
///         // ERROR: no field `state` or `rng` on `&NarrationContext`
///         let _ = &mut ctx.state;
///         let _ = ctx.rng.random();
///         None
///     }
/// }
/// ```
///
/// Even a narrator attempting to mutate fields on `NarrationContext` fails to compile because all
/// fields are private with shared-reference getters only:
///
/// ```compile_fail
/// use etv_core::narrator::NarrationContext;
///
/// fn try_mutate(ctx: &NarrationContext) {
///     ctx.fallback_line = String::from("tampered");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarrationContext {
    /// Plain deterministic line the game shows when [`Narrator::narrate`] returns `None`.
    /// This is the fallback line only — **not** prompt material for the narrator.
    fallback_line: String,
    event_id: String,
    choice_id: String,
    shown_effects: Vec<HealthEffect>,
    shown_deltas: BTreeMap<String, i32>,
    shown_lines: Vec<String>,
    place: String,
    band: String,
    flags: TurnMattersFlags,
    time_of_day: TimeOfDay,
    phase: GamePhase,
    biome: String,
    day: i32,
    pressures: Vec<String>,
    themes: Vec<String>,
    weirdness_allowance: String,
    gm_profile: GMProfile,
    situation: String,
    party_status: PartyStatusFacts,
    /// Party member names. Used **only** by the hard-rule checker to block names in prose; never
    /// prompt material (see [`format_turn_facts_for_prompt`]).
    #[serde(default)]
    check_party_names: Vec<String>,
    /// The event's `narration_seed` text. Checker-only: resource nouns and draft-animal species
    /// the event itself names are allowed in prose. Never prompt material.
    #[serde(default)]
    check_event_seed: String,
    /// The chosen option's label. Checker-only: used for the choice-parroting rule.
    #[serde(default)]
    check_choice_label: String,
}

impl NarrationContext {
    /// Attach the checker-only facts (party names, event seed text, chosen option label).
    ///
    /// These feed [`CheckerContext::from_narration_context`] and nothing else: they are not
    /// included in any prompt.
    pub fn with_check_facts(
        mut self,
        party_names: Vec<String>,
        event_seed: String,
        choice_label: String,
    ) -> Self {
        self.check_party_names = party_names;
        self.check_event_seed = event_seed;
        self.check_choice_label = choice_label;
        self
    }

    /// Party member names (checker-only; empty unless attached).
    pub fn check_party_names(&self) -> &[String] {
        &self.check_party_names
    }

    /// Event `narration_seed` text (checker-only; empty unless attached).
    pub fn check_event_seed(&self) -> &str {
        &self.check_event_seed
    }

    /// Chosen option label (checker-only; empty unless attached).
    pub fn check_choice_label(&self) -> &str {
        &self.check_choice_label
    }

    /// Construct an owned, read-only [`NarrationContext`] snapshot for a completed turn.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fallback_line: String,
        event_id: String,
        choice_id: String,
        shown_effects: Vec<HealthEffect>,
        shown_deltas: BTreeMap<String, i32>,
        shown_lines: Vec<String>,
        place: String,
        band: String,
        flags: TurnMattersFlags,
        time_of_day: TimeOfDay,
        phase: GamePhase,
        biome: String,
        day: i32,
        pressures: Vec<String>,
        themes: Vec<String>,
        weirdness_allowance: String,
        gm_profile: GMProfile,
        situation: String,
        party_status: PartyStatusFacts,
    ) -> Self {
        Self {
            fallback_line,
            event_id,
            choice_id,
            shown_effects,
            shown_deltas,
            shown_lines,
            place,
            band,
            flags,
            time_of_day,
            phase,
            biome,
            day,
            pressures,
            themes,
            weirdness_allowance,
            gm_profile,
            situation,
            party_status,
            check_party_names: Vec::new(),
            check_event_seed: String::new(),
            check_choice_label: String::new(),
        }
    }

    /// Plain fallback line the game displays when the narrator returns `None`.
    ///
    /// This is a fallback only, **not** prompt material for the narrator.
    pub fn fallback_line(&self) -> &str {
        &self.fallback_line
    }

    /// Event ID active or resolved on this turn (empty if none).
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    /// Choice ID selected on this turn (empty if none).
    pub fn choice_id(&self) -> &str {
        &self.choice_id
    }

    /// Health effects shown to the player on this turn.
    pub fn shown_effects(&self) -> &[HealthEffect] {
        &self.shown_effects
    }

    /// Numerical resource/stat deltas shown to the player on this turn.
    pub fn shown_deltas(&self) -> &BTreeMap<String, i32> {
        &self.shown_deltas
    }

    /// Output lines emitted by the engine on this turn.
    pub fn shown_lines(&self) -> &[String] {
        &self.shown_lines
    }

    /// Human-readable name (or ID) of the current map node / place.
    pub fn place(&self) -> &str {
        &self.place
    }

    /// Journey band label (`"early"`, `"mid"`, or `"late"`) derived from journey progress.
    pub fn band(&self) -> &str {
        &self.band
    }

    /// Which of the five "turn matters" rules triggered on this turn.
    pub fn flags(&self) -> &TurnMattersFlags {
        &self.flags
    }

    /// Time of day on this turn (`Morning`, `Afternoon`, `Evening`, or `Night`).
    pub fn time_of_day(&self) -> TimeOfDay {
        self.time_of_day
    }

    /// Game phase on this turn (`Camp`, `Event`, `Route`, or `GameOver`).
    pub fn phase(&self) -> GamePhase {
        self.phase
    }

    /// Biome / terrain string of the current map node (or `"unknown terrain"` if not found).
    pub fn biome(&self) -> &str {
        &self.biome
    }

    /// Current journey day (`1`-indexed).
    pub fn day(&self) -> i32 {
        self.day
    }

    /// Top active survival pressures (up to 3, sorted by severity descending) from the pure
    /// `GMBriefReadOnly` computation (`memory.rs:94-179, 322-335`).
    pub fn pressures(&self) -> &[String] {
        &self.pressures
    }

    /// Top active narrative themes (1–3 tags) from the pure `GMBriefReadOnly` computation
    /// (`memory.rs:189-215, 322-335`).
    pub fn themes(&self) -> &[String] {
        &self.themes
    }

    /// Weirdness allowance (`"none"`, `"hint"`, or `"strong"`) from the pure `GMBriefReadOnly`
    /// computation (`memory.rs:322-335, 408-419`).
    pub fn weirdness_allowance(&self) -> &str {
        &self.weirdness_allowance
    }

    /// Active GM voice / personality profile (`Chronicler`, `Fireside`, or `Lantern`).
    pub fn gm_profile(&self) -> GMProfile {
        self.gm_profile
    }

    /// Situation summary computed by the pure `GMBriefReadOnly` path (`memory.rs:322-335`).
    pub fn situation(&self) -> &str {
        &self.situation
    }

    /// Nameless party status counts and bands (contains **no** member names).
    pub fn party_status(&self) -> &PartyStatusFacts {
        &self.party_status
    }

    /// Resolve the display line for this turn: returns `narrator.narrate(self)` when `Some`,
    /// or [`Self::fallback_line`] when the narrator is off or returns `None`.
    pub fn resolve_display_line(&self, narrator: &dyn Narrator) -> String {
        narrator
            .narrate(self)
            .unwrap_or_else(|| self.fallback_line.clone())
    }
}

thread_local! {
    static LAST_PRE_TURN_OBSERVATION: RefCell<Option<PreTurnObservation>> = const { RefCell::new(None) };
}

fn time_of_day_slot(tod: TimeOfDay) -> i32 {
    match tod {
        TimeOfDay::Morning => 0,
        TimeOfDay::Afternoon => 1,
        TimeOfDay::Evening => 2,
        TimeOfDay::Night => 3,
    }
}

/// Read-only snapshot of pre-step values captured before `StepEngine::step` so post-step changes
/// (health drop, supply reaching zero, landmark arrival, night at camp, and applied deltas across
/// all 16 effect keys) can be detected purely by reading existing `RunState` fields and
/// `StepMessages` without any new RNG draw or state change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreTurnObservation {
    pub run_id: String,
    pub phase: GamePhase,
    pub day: i32,
    pub time_of_day: TimeOfDay,
    pub location_id: String,
    pub member_health: Vec<i32>,
    pub supplies: BTreeMap<String, i32>,
    pub animals_health: i32,
    pub wagon_condition: i32,
    pub morale: i32,
    pub distance_remaining: i32,
    pub distance_traveled: i32,
    pub journal_len: usize,
    pub rng_counter: u64,
}

impl PreTurnObservation {
    /// Capture pre-turn values from `phase` and `state` prior to `StepEngine::step`.
    pub fn capture(phase: GamePhase, state: &RunState) -> Self {
        let obs = Self {
            run_id: state.run_id.clone(),
            phase,
            day: state.day,
            time_of_day: state.time_of_day,
            location_id: state.location_id.clone(),
            member_health: state.party.members.iter().map(|m| m.health).collect(),
            supplies: state.supplies.items.clone(),
            animals_health: state.wagon.animals_health,
            wagon_condition: state.wagon.condition,
            morale: state.party.morale,
            distance_remaining: state.distance_remaining,
            distance_traveled: state.distance_traveled,
            journal_len: state.journal.len(),
            rng_counter: state.rng_counter,
        };
        LAST_PRE_TURN_OBSERVATION.with(|slot| {
            *slot.borrow_mut() = Some(obs.clone());
        });
        obs
    }
}

/// Compute the change actually applied on a turn for each of the 16 phrase-table keys by a pure
/// read of `pre` (before the turn) and `state` (after the turn), with zero RNG draws and zero
/// state writes:
///
/// - Supplies (`ammo`, `food`, `water`, `firewood`, `cloth`, `rope`, `oil`, `tools`, `parts`,
///   `boots`): `state.supplies.get(key) - pre.supplies.get(key)` (with `"oil"` reading
///   `"lantern_oil"` in `SuppliesState`, where `worldgen::resolve_resource_alias` maps `"oil"`).
/// - `health`: sum across party members of `after.health - before_health` (each member clamped to
///   `0..=100` in `events::apply_outcome`).
/// - `animals_health`: `state.wagon.animals_health - pre.animals_health` (clamped to `0..=100`).
/// - `wagon_condition`: `state.wagon.condition - pre.wagon_condition` (clamped to `0..=100`).
/// - `morale`: `state.party.morale - pre.morale` (clamped to `0..=100`).
/// - `distance`: `state.distance_remaining - pre.distance_remaining` on event choice turns
///   (`pre.phase == GamePhase::Event`), or `state.distance_traveled - pre.distance_traveled` on
///   non-event turns.
/// - `time_days`: quarter-day slot delta `(state.day * 4 + slot(state.time_of_day)) -
///   (pre.day * 4 + slot(pre.time_of_day))` produced by `StepEngine::advance_time`.
pub fn compute_applied_turn_deltas(
    pre: &PreTurnObservation,
    state: &RunState,
) -> BTreeMap<String, i32> {
    let mut applied = BTreeMap::new();

    for key in [
        "ammo", "food", "water", "firewood", "cloth", "rope", "tools", "parts", "boots",
    ] {
        let before_val = pre.supplies.get(key).copied().unwrap_or(0);
        let after_val = state.supplies.get(key);
        let delta = after_val - before_val;
        if delta != 0 {
            applied.insert(key.to_string(), delta);
        }
    }

    let before_lantern = pre.supplies.get("lantern_oil").copied().unwrap_or(0);
    let after_lantern = state.supplies.get("lantern_oil");
    let lantern_delta = after_lantern - before_lantern;
    let before_oil = pre.supplies.get("oil").copied().unwrap_or(0);
    let after_oil = state.supplies.get("oil");
    let raw_oil_delta = after_oil - before_oil;
    let oil_delta = if lantern_delta != 0 {
        lantern_delta
    } else {
        raw_oil_delta
    };
    if oil_delta != 0 {
        applied.insert("oil".to_string(), oil_delta);
    }

    let health_delta: i32 = pre
        .member_health
        .iter()
        .zip(state.party.members.iter())
        .map(|(&before, after)| after.health - before)
        .sum();
    if health_delta != 0 {
        applied.insert("health".to_string(), health_delta);
    }

    let animals_delta = state.wagon.animals_health - pre.animals_health;
    if animals_delta != 0 {
        applied.insert("animals_health".to_string(), animals_delta);
    }

    let wagon_delta = state.wagon.condition - pre.wagon_condition;
    if wagon_delta != 0 {
        applied.insert("wagon_condition".to_string(), wagon_delta);
    }

    let morale_delta = state.party.morale - pre.morale;
    if morale_delta != 0 {
        applied.insert("morale".to_string(), morale_delta);
    }

    let distance_delta = if pre.phase == GamePhase::Event {
        state.distance_remaining - pre.distance_remaining
    } else {
        state.distance_traveled - pre.distance_traveled
    };
    if distance_delta != 0 {
        applied.insert("distance".to_string(), distance_delta);
    }

    let pre_slots = pre.day * 4 + time_of_day_slot(pre.time_of_day);
    let post_slots = state.day * 4 + time_of_day_slot(state.time_of_day);
    let time_delta = post_slots - pre_slots;
    if time_delta != 0 {
        applied.insert("time_days".to_string(), time_delta);
    }

    applied
}

/// Evaluate the five "turn matters" rules by a pure read of `pre`, `intent`, `msgs`, and
/// post-step `state` (zero RNG draws, zero state mutation):
///
/// 1. `death`: reads `msgs.effects` (`StepMessages::effects` in `rust/etv-core/src/engine.rs:67`,
///    populated with `HealthEffect::Died` from `rust/etv-core/src/physics.rs:192,265,652` and
///    `rust/etv-core/src/events.rs:681`) and `state.party.members[i].health`
///    (`rust/etv-core/src/model.rs:580`).
/// 2. `injury`: reads `state.party.members[i].health` (`rust/etv-core/src/model.rs:580`, updated
///    at `rust/etv-core/src/physics.rs:208,218,247,339,591,615` and
///    `rust/etv-core/src/events.rs:674-675`) and checks `post_health < pre_health`.
/// 3. `supply_zero`: reads `state.supplies.items` (`rust/etv-core/src/model.rs:676`, updated at
///    `rust/etv-core/src/model.rs:701-710`) and checks if any key transitioned from `> 0` before
///    the turn to `<= 0` after the turn.
/// 4. `landmark_arrival`: reads `state.location_id` (`rust/etv-core/src/model.rs:1083`), updated
///    exclusively by `StepEngine::arrive_at_next_node` (`rust/etv-core/src/engine.rs:310-311,961`),
///    checking `state.location_id != pre.location_id`.
/// 5. `night_at_camp`: flags only when the turn ends at night while the party is camped — reads
///    `pre.phase` (`StepEngine::phase` at `rust/etv-core/src/engine.rs:146,234` / `GamePhase::Camp`
///    at `rust/etv-core/src/model.rs:501`), `state.game_over` (`RunState::game_over` at
///    `rust/etv-core/src/model.rs:1109`), `intent.action` (`PlayerIntent::action` at
///    `rust/etv-core/src/model.rs:956`, requiring `intent.action != IntentAction::Travel` so the
///    party remains camped in `GamePhase::Camp`), and post-step `state.time_of_day`
///    (`RunState::time_of_day` at `rust/etv-core/src/model.rs:1081`, compared against
///    `TimeOfDay::Night` at `rust/etv-core/src/model.rs:130`, advanced in
///    `StepEngine::advance_time` at `rust/etv-core/src/engine.rs:1035-1044`).
pub fn evaluate_turn_matters(
    pre: &PreTurnObservation,
    intent: &PlayerIntent,
    msgs: &StepMessages,
    state: &RunState,
) -> TurnMattersFlags {
    LAST_PRE_TURN_OBSERVATION.with(|slot| {
        *slot.borrow_mut() = Some(pre.clone());
    });

    let effect_death = msgs
        .effects
        .iter()
        .any(|eff| matches!(eff, HealthEffect::Died { .. }));
    let transition_death = pre
        .member_health
        .iter()
        .zip(state.party.members.iter())
        .any(|(&before, after)| before > 0 && after.health <= 0);
    let death = effect_death || transition_death;

    let injury = pre
        .member_health
        .iter()
        .zip(state.party.members.iter())
        .any(|(&before, after)| after.health < before);

    let supply_zero = pre.supplies.iter().any(|(key, &before_val)| {
        let after_val = state.supplies.items.get(key).copied().unwrap_or(0);
        before_val > 0 && after_val <= 0
    });

    let landmark_arrival = state.location_id != pre.location_id;

    let night_at_camp = pre.phase == GamePhase::Camp
        && !state.game_over
        && intent.action != IntentAction::Travel
        && state.time_of_day == TimeOfDay::Night;

    TurnMattersFlags {
        death,
        injury,
        supply_zero,
        landmark_arrival,
        night_at_camp,
    }
}

/// Compute the journey band label (`"early"`, `"mid"`, or `"late"`) from `journey_pressure(state)`
/// (`rust/etv-core/src/physics.rs:43-48`).
pub fn journey_band(state: &RunState) -> String {
    let p = journey_pressure(state);
    if p < 0.33 {
        "early".to_string()
    } else if p < 0.66 {
        "mid".to_string()
    } else {
        "late".to_string()
    }
}

/// Build an owned, read-only [`NarrationContext`] from a completed step.
///
/// Uses `GamePhase::GameOver` when `state.game_over` is `true`, otherwise `GamePhase::Camp`.
/// Prefer [`build_narration_context_with_phase`] or [`build_narration_context_from_turn`] when
/// the active `StepEngine::phase` is available.
pub fn build_narration_context(
    state: &RunState,
    event_id: &str,
    choice_id: &str,
    msgs: &StepMessages,
    flags: TurnMattersFlags,
) -> NarrationContext {
    let phase = if state.game_over {
        GamePhase::GameOver
    } else {
        GamePhase::Camp
    };
    build_narration_context_with_phase(state, phase, event_id, choice_id, msgs, flags)
}

/// Build an owned, read-only [`NarrationContext`] from an explicit pre-turn [`PreTurnObservation`],
/// post-turn [`RunState`], and [`StepMessages`], computing `shown_deltas` purely from the changes
/// actually applied on the turn ([`compute_applied_turn_deltas`]).
pub fn build_narration_context_from_turn(
    pre: &PreTurnObservation,
    state: &RunState,
    phase: GamePhase,
    event_id: &str,
    choice_id: &str,
    msgs: &StepMessages,
    flags: TurnMattersFlags,
) -> NarrationContext {
    let applied = compute_applied_turn_deltas(pre, state);
    build_narration_context_with_deltas(state, phase, event_id, choice_id, msgs, flags, applied)
}

fn clamp_static_supply_deltas_against_state(
    raw_deltas: &BTreeMap<String, i32>,
    supplies: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    let mut out = BTreeMap::new();
    for (k, &delta) in raw_deltas {
        if delta == 0 {
            continue;
        }
        let lookup_key = if k == "oil" {
            "lantern_oil"
        } else {
            k.as_str()
        };
        let current = supplies
            .get(lookup_key)
            .or_else(|| supplies.get(k.as_str()))
            .copied()
            .unwrap_or(0);
        let applied = (current + delta).max(0) - current;
        if applied != 0 {
            let norm_key = if k == "lantern_oil" {
                "oil"
            } else {
                k.as_str()
            };
            out.insert(norm_key.to_string(), applied);
        }
    }
    out
}

/// Build an owned, read-only [`NarrationContext`] from a completed step and its post-step
/// [`GamePhase`], reading engine facts purely via [`build_gm_brief_readonly`], [`current_node`],
/// [`journey_band`], [`PartyStatusFacts::from_state`], and [`compute_applied_turn_deltas`]
/// (zero RNG draws, zero state mutation).
pub fn build_narration_context_with_phase(
    state: &RunState,
    phase: GamePhase,
    event_id: &str,
    choice_id: &str,
    msgs: &StepMessages,
    flags: TurnMattersFlags,
) -> NarrationContext {
    let shown_deltas = LAST_PRE_TURN_OBSERVATION.with(|slot| {
        if let Some(pre) = slot.borrow().as_ref() {
            if pre.run_id == state.run_id {
                let applied = compute_applied_turn_deltas(pre, state);
                let turn_stepped = state.journal.len() != pre.journal_len
                    || state.rng_counter != pre.rng_counter
                    || state.location_id != pre.location_id
                    || !applied.is_empty();
                if turn_stepped {
                    return applied;
                }
                return clamp_static_supply_deltas_against_state(
                    &msgs.outcome_deltas,
                    &pre.supplies,
                );
            }
        }
        clamp_static_supply_deltas_against_state(&msgs.outcome_deltas, &state.supplies.items)
    });

    build_narration_context_with_deltas(
        state,
        phase,
        event_id,
        choice_id,
        msgs,
        flags,
        shown_deltas,
    )
}

fn build_narration_context_with_deltas(
    state: &RunState,
    phase: GamePhase,
    event_id: &str,
    choice_id: &str,
    msgs: &StepMessages,
    flags: TurnMattersFlags,
    shown_deltas: BTreeMap<String, i32>,
) -> NarrationContext {
    let fallback_line = if !msgs.outcome_narration.is_empty() {
        msgs.outcome_narration.clone()
    } else {
        msgs.lines.join(" ")
    };
    let node = current_node(state);
    let place = node
        .map(|n| n.name.clone())
        .unwrap_or_else(|| state.location_id.clone());
    let biome = node
        .map(|n| n.biome.as_str().to_string())
        .unwrap_or_else(|| "unknown terrain".to_string());
    let band = journey_band(state);
    let brief = build_gm_brief_readonly(state);
    let party_status = PartyStatusFacts::from_state(state);

    NarrationContext::new(
        fallback_line,
        event_id.to_string(),
        choice_id.to_string(),
        msgs.effects.clone(),
        shown_deltas,
        msgs.lines.clone(),
        place,
        band,
        flags,
        state.time_of_day,
        phase,
        biome,
        state.day,
        brief.pressures,
        brief.themes,
        brief.weirdness_allowance,
        state.gm_profile,
        brief.situation,
        party_status,
    )
    .with_check_facts(
        state.party.members.iter().map(|m| m.name.clone()).collect(),
        msgs.event_narration.clone(),
        msgs.event_choices
            .iter()
            .find(|c| c.id == choice_id)
            .map(|c| c.label.clone())
            .unwrap_or_default(),
    )
}

// ============================================================================
// Step 3: Live Narrator Port (Profiles, Prompts, Schemas, Checker, Fallbacks,
// Settings Stub, Loopback Endpoint Guard, Transport, and LiveNarrator)
// ============================================================================

/// Narrator output length cap (`Brief` = 1 sentence, `Standard` = 1–2 sentences,
/// `Rich` = 2–3 sentences).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NarratorLength {
    Brief,
    #[default]
    Standard,
    Rich,
}

impl NarratorLength {
    pub fn as_str(&self) -> &'static str {
        match self {
            NarratorLength::Brief => "Brief",
            NarratorLength::Standard => "Standard",
            NarratorLength::Rich => "Rich",
        }
    }

    /// Inclusive `(min_sentences, max_sentences)` allowed by this length setting.
    pub fn sentence_bounds(&self) -> (usize, usize) {
        match self {
            NarratorLength::Brief => (1, 1),
            NarratorLength::Standard => (1, 2),
            NarratorLength::Rich => (2, 3),
        }
    }
}

/// Human-facing interface name for each of the three narrator profiles (`Chronicler`,
/// `Fireside`, `Lantern-Bearer`).
pub fn profile_interface_name(profile: GMProfile) -> &'static str {
    match profile {
        GMProfile::Chronicler => "Chronicler",
        GMProfile::Fireside => "Fireside",
        GMProfile::Lantern => "Lantern-Bearer",
    }
}

/// Per-profile voice header ported from `src/escape_the_valley/gm.py:20-56` (`PROFILE_HEADERS`),
/// with prompt hygiene on `GMProfile::Lantern` so the literal word `"lantern"` never appears in
/// the system prompt when `lantern_oil` is not in the turn's effects.
pub fn profile_header(profile: GMProfile) -> &'static str {
    match profile {
        GMProfile::Chronicler => {
            "You are \"The Chronicler,\" a grounded, terse narrator for a survival trail game.\n\
Voice rules:\n\
- Write like a weary 19th-century diarist. Short, declarative sentences.\n\
- Focus on weather, terrain, exhaustion, supplies, and practical dilemmas.\n\
- Keep the uncanny subtle — a strange track, an odd silence, a tool that moved.\n\
- Never explain the weird. Let it sit alongside the mud and the hunger.\n\
- Tone: dry, observant, unsentimental, occasionally wry.\n\
- Maximum 3 sentences for scene narration, 2 for outcomes."
        }
        GMProfile::Fireside => {
            "You are \"The Fireside Storyteller,\" a warm, ominous folk narrator for a survival trail game.\n\
Voice rules:\n\
- Write like someone telling a story by camp light — vivid, personal, slightly haunted.\n\
- Notice the human details: how people hold their shoulders, what they don't say, small kindnesses.\n\
- When the uncanny appears, treat it like old country knowledge — folk warnings, grandmother's rules.\n\
- Balance warmth and dread. The trail is hard, the people are real, something watches.\n\
- Tone: empathetic, atmospheric, quietly foreboding.\n\
- Maximum 4 sentences for scene narration, 2 for outcomes."
        }
        GMProfile::Lantern => {
            "You are \"The Uncanny Bearer,\" a serious but uncanny storyteller for a survival trail game.\n\
Voice rules:\n\
- The valley is wrong and you know it. Not horror — something older and stranger.\n\
- Describe normal things in ways that feel slightly off: geometry that doesn't add up, echoes that arrive early.\n\
- Still respect the survival game: supplies matter, injuries hurt, distances are real.\n\
- When uncanny tokens are spent, let the weirdness breathe — a full surreal moment before snapping back.\n\
- Tone: precise, eerie, deadpan when describing the impossible.\n\
- Maximum 4 sentences for scene narration, 2 for outcomes."
        }
    }
}

/// Map `NarrationContext::weirdness_allowance` (`"none"`, `"hint"`, `"strong"`) to numeric
/// weirdness band `0..=3` of the voice guide (section 5): `"none"` is band 0 (grounded),
/// `"hint"` is band 2 ("Uncanny hint") and `"strong"` is band 3 ("Strong uncanny"). Band 1
/// (weather and rumour) has no engine allowance, so it is reachable only as the digit `"1"`.
/// Anything unrecognised is band 0, the strictest.
pub fn weirdness_band_from_allowance(allowance: &str) -> u8 {
    match allowance.trim().to_ascii_lowercase().as_str() {
        "none" | "0" => 0,
        "1" => 1,
        "hint" | "2" => 2,
        "strong" | "surreal" | "tokens" | "3" => 3,
        _ => 0,
    }
}

/// Shared voice hard rules from the Prose Voice Guide (`VOICE-GUIDE-v1-DRAFT-2026-11-02.md`).
pub fn shared_voice_hard_rules(length: NarratorLength, weirdness_band: u8) -> String {
    let (min_s, max_s) = length.sentence_bounds();
    format!(
        "SHARED VOICE HARD RULES:\n\
- Present tense, third-person impersonal camera at wheel-height in mud and weather.\n\
- Never state digits, counting words of two or more, or measurement/time/currency units.\n\
- Allowed quantifiers for wagon, team, or body only: one, both, pair (never before a supply noun). Never use half.\n\
- Never state or imply death or total ruin.\n\
- Never name a party member or invent proper nouns. No mid-sentence capital letters except the current place name.\n\
- Never mention a supply or resource noun unless that resource key was spent or gained on this turn.\n\
- Weirdness band ceiling: band {weirdness_band}. Never use horror cliches, magic, monsters, or game/meta terms.\n\
- Punctuation: periods and commas only. No semicolons, exclamation marks, question marks, colons, quotes, or ellipses.\n\
- LENGTH CAP: {} ({min_s} to {max_s} sentences).",
        length.as_str()
    )
}

/// Format turn facts strictly from [`NarrationContext`] (never including `fallback_line`,
/// `shown_lines`, party member names, or unspent resource nouns).
pub fn format_turn_facts_for_prompt(ctx: &NarrationContext) -> String {
    let mut delta_directions = Vec::new();
    for (k, &v) in ctx.shown_deltas() {
        if v < 0 {
            delta_directions.push(format!("{k}:spent"));
        } else if v > 0 {
            delta_directions.push(format!("{k}:gained"));
        }
    }
    let effects_summary: Vec<&'static str> = ctx
        .shown_effects()
        .iter()
        .map(|eff| match eff {
            HealthEffect::Starvation { .. } => "starvation",
            HealthEffect::Dehydration { .. } => "dehydration",
            HealthEffect::FellSick { .. } => "fell_sick",
            HealthEffect::Healed { .. } => "healed",
            HealthEffect::Recovered { .. } => "recovered",
            HealthEffect::Died { .. } => "death",
        })
        .collect();

    format!(
        "TURN FACTS:\n\
- event_id: {}\n\
- choice_id: {}\n\
- place: {}\n\
- journey_band: {}\n\
- biome: {}\n\
- time_of_day: {}\n\
- phase: {}\n\
- weirdness_allowance: {}\n\
- pressures: {}\n\
- themes: {}\n\
- situation: {}\n\
- party_health_band: {}\n\
- party_morale_band: {}\n\
- resource_directions: {}\n\
- health_effects: {}\n\
- turn_matters: death={}, injury={}, supply_zero={}, landmark_arrival={}, night_at_camp={}",
        ctx.event_id(),
        ctx.choice_id(),
        ctx.place(),
        ctx.band(),
        ctx.biome(),
        ctx.time_of_day().as_str(),
        ctx.phase().as_str(),
        ctx.weirdness_allowance(),
        ctx.pressures().join(", "),
        ctx.themes().join(", "),
        ctx.situation(),
        ctx.party_status().health_band(),
        ctx.party_status().morale_band(),
        if delta_directions.is_empty() {
            "none".to_string()
        } else {
            delta_directions.join(", ")
        },
        if effects_summary.is_empty() {
            "none".to_string()
        } else {
            effects_summary.join(", ")
        },
        ctx.flags().death,
        ctx.flags().injury,
        ctx.flags().supply_zero,
        ctx.flags().landmark_arrival,
        ctx.flags().night_at_camp,
    )
}

/// Build the full narrator system prompt from:
/// (a) the shared hard rules from the voice guide,
/// (b) the per-profile voice header (`PROFILE_HEADERS`), and
/// (c) the turn's facts from [`NarrationContext`] (never `fallback_line`).
pub fn build_system_prompt(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
) -> String {
    let band = weirdness_band_from_allowance(ctx.weirdness_allowance());
    let rules = shared_voice_hard_rules(length, band);
    let header = profile_header(profile);
    let facts = format_turn_facts_for_prompt(ctx);
    format!("{header}\n\n{rules}\n\n{facts}")
}

/// JSON schema instructions for the scene call (`src/escape_the_valley/gm.py:70-80`).
pub const SCENE_SCHEMA: &str = r#"{
  "scene_id": "string",
  "title": "string (short, evocative, 2-5 words)",
  "narration": "string (vivid scene description following all hard rules)",
  "profile": "chronicler | fireside | lantern",
  "uncanny_intensity": "none | hint | strong",
  "choices": [
    {"id": "A", "label": "string", "intent": {"action": "TRAVEL|REST|HUNT|REPAIR|TRADE|DETOUR|FORD|WAIT|PARLEY|SEARCH", "style": "CAUTIOUS|BOLD|CLEVER|DESPERATE|NEUTRAL", "notes": "string"}, "risk_hint": "string", "cost_hint": "string"}
  ],
  "tags": ["string"],
  "gm_aside": "string (1 short eerie or wry observation, or empty)",
  "memory_proposals": []
}"#;

/// JSON schema instructions for the outcome call (`src/escape_the_valley/gm.py:82-88`).
pub const OUTCOME_SCHEMA: &str = r#"{
  "scene_id": "string",
  "outcome_title": "string (2-4 words)",
  "outcome_narration": "string (vivid outcome description following all hard rules)",
  "callout": "string (1 punchy sentence summarizing what changed)",
  "oregon_nod": "string (optional 1-line wry trail observation, or empty)",
  "memory_proposals": []
}"#;

/// Build `(system_prompt, user_prompt)` for the per-turn scene call (`SCENE_SCHEMA`).
pub fn build_scene_prompts(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
) -> (String, String) {
    let sys = build_system_prompt(profile, length, ctx);
    let user = format!(
        "Generate a scene response strictly as valid JSON matching SCENE_SCHEMA:\n{SCENE_SCHEMA}"
    );
    (sys, user)
}

/// Build `(system_prompt, user_prompt)` for the per-turn outcome call (`OUTCOME_SCHEMA`).
pub fn build_outcome_prompts(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
) -> (String, String) {
    let sys = build_system_prompt(profile, length, ctx);
    let user = format!(
        "Generate an outcome response strictly as valid JSON matching OUTCOME_SCHEMA:\n{OUTCOME_SCHEMA}"
    );
    (sys, user)
}

/// Parsed response for the scene call (`src/escape_the_valley/gm.py:122-168`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneResponse {
    #[serde(default)]
    pub scene_id: String,
    #[serde(default)]
    pub title: String,
    pub narration: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub uncanny_intensity: String,
    #[serde(default)]
    pub choices: Vec<Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub gm_aside: String,
    #[serde(default)]
    pub memory_proposals: Vec<Value>,
}

/// Parsed response for the outcome call (`src/escape_the_valley/gm.py:172-196`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeResponse {
    #[serde(default)]
    pub scene_id: String,
    #[serde(default)]
    pub outcome_title: String,
    pub outcome_narration: String,
    #[serde(default)]
    pub callout: String,
    #[serde(default)]
    pub oregon_nod: String,
    #[serde(default)]
    pub memory_proposals: Vec<Value>,
}

/// Strict JSON parser for [`SceneResponse`] — no markdown fence stripping or regex salvage.
/// Malformed JSON or a missing/empty `narration` field returns `Err`.
pub fn parse_scene_response_strict(raw: &str) -> Result<SceneResponse, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err("scene response is not a raw JSON object".to_string());
    }
    let val: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid scene JSON: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "scene JSON root must be an object".to_string())?;
    if !obj.get("narration").map(|v| v.is_string()).unwrap_or(false) {
        return Err("scene JSON missing string field 'narration'".to_string());
    }
    let parsed: SceneResponse =
        serde_json::from_value(val).map_err(|e| format!("scene schema mismatch: {e}"))?;
    if parsed.narration.trim().is_empty() {
        return Err("scene narration is empty".to_string());
    }
    Ok(parsed)
}

/// Strict JSON parser for [`OutcomeResponse`] — no markdown fence stripping or regex salvage.
/// Malformed JSON or a missing/empty `outcome_narration` field returns `Err`.
pub fn parse_outcome_response_strict(raw: &str) -> Result<OutcomeResponse, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err("outcome response is not a raw JSON object".to_string());
    }
    let val: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid outcome JSON: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "outcome JSON root must be an object".to_string())?;
    if !obj
        .get("outcome_narration")
        .map(|v| v.is_string())
        .unwrap_or(false)
    {
        return Err("outcome JSON missing string field 'outcome_narration'".to_string());
    }
    let parsed: OutcomeResponse =
        serde_json::from_value(val).map_err(|e| format!("outcome schema mismatch: {e}"))?;
    if parsed.outcome_narration.trim().is_empty() {
        return Err("outcome_narration is empty".to_string());
    }
    Ok(parsed)
}

// ============================================================================
// Hard-Rule Checker (`VOICE-GUIDE-v1-DRAFT-2026-11-02.md` §3–§8 + `gm.py:96-99`)
// ============================================================================

/// Reviewed table mapping each engine supply/resource key to its prose surface nouns.
pub const RESOURCE_NOUN_TABLE: &[(&str, &[&str])] = &[
    (
        "food",
        &[
            "food",
            "foods",
            "flour",
            "bacon",
            "meat",
            "jerky",
            "hardtack",
            "provisions",
            "provision",
            "loaf",
            "loaves",
            "bread",
            "beans",
            "grain",
            "venison",
            "saltpork",
            "sack",
            "sacks",
        ],
    ),
    (
        "water",
        &[
            "canteen",
            "canteens",
            "waterskin",
            "waterskins",
            "barrel",
            "barrels",
            "cask",
            "casks",
        ],
    ),
    (
        "meds",
        &[
            "meds",
            "medicine",
            "medicines",
            "poultice",
            "poultices",
            "tincture",
            "tinctures",
            "salve",
            "salves",
            "remedy",
            "remedies",
            "herb",
            "herbs",
            "quinine",
            "laudanum",
            "tonic",
            "tonics",
        ],
    ),
    (
        "ammo",
        &[
            "ammo",
            "ammunition",
            "bullet",
            "bullets",
            "powder",
            "gunpowder",
            "cartridge",
            "cartridges",
            "shell",
            "shells",
            "rifle",
            "rifles",
            "musket",
            "muskets",
            "shot",
            "shots",
        ],
    ),
    (
        "parts",
        &[
            "parts",
            "spare",
            "spares",
            "linchpin",
            "linchpins",
            "grease",
            "tar",
            "splint",
            "splints",
            "tool",
            "tools",
        ],
    ),
    (
        "cloth",
        &[
            "cloth", "cloths", "linen", "linens", "bandage", "bandages", "rag", "rags", "blanket",
            "blankets", "wool", "patch", "patches", "thread", "needle", "needles",
        ],
    ),
    ("salt", &["salt", "salts", "brine"]),
    (
        "boots",
        &[
            "boot", "boots", "shoe", "shoes", "footwear", "sole", "soles",
        ],
    ),
    (
        "rope",
        &[
            "rope", "ropes", "cord", "cords", "cordage", "hemp", "tether", "tethers",
        ],
    ),
    (
        "lantern_oil",
        &[
            "lantern", "lanterns", "oil", "lamp", "lamps", "wick", "wicks", "tallow", "kerosene",
        ],
    ),
    (
        "firewood",
        &["firewood", "kindling", "charcoal", "tinder", "woodpile"],
    ),
    (
        "coin",
        &[
            "coin",
            "coins",
            "silver",
            "gold",
            "copper",
            "money",
            "currency",
            "dollar",
            "dollars",
            "pence",
            "shilling",
            "shillings",
        ],
    ),
];

const BANNED_NUMBER_OR_UNIT_WORDS: &[&str] = &[
    // Counting words of two or more ( plus zero)
    "zero",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "hundreds",
    "thousand",
    "thousands",
    "dozen",
    "dozens",
    "score",
    "scores",
    "couple",
    "triple",
    "twice",
    "thrice",
    // Measurement, duration, distance, and count units
    "day",
    "days",
    "mile",
    "miles",
    "hour",
    "hours",
    "minute",
    "minutes",
    "second",
    "seconds",
    "week",
    "weeks",
    "month",
    "months",
    "year",
    "years",
    "pound",
    "pounds",
    "ounce",
    "ounces",
    "gallon",
    "gallons",
    "quart",
    "quarts",
    "pint",
    "pints",
    "inch",
    "inches",
    "foot",
    "feet",
    "yard",
    "yards",
    "meter",
    "meters",
    "ration",
    "rations",
    "coin",
    "coins",
    "dollar",
    "dollars",
    "percent",
    "degree",
    "degrees",
    "step",
    "steps",
    "turn",
    "turns",
];

const BANNED_DEATH_WORDS: &[&str] = &[
    "dead",
    "died",
    "dies",
    "dying",
    "death",
    "deaths",
    "corpse",
    "corpses",
    "grave",
    "graves",
    "buried",
    "burial",
    "perish",
    "perished",
    "perishes",
    "fatal",
    "killed",
    "kills",
    "killing",
    "slain",
    "lifeless",
    "deceased",
    "destroyed",
];

/// Words banned across ALL weirdness bands (including Band 3 + legacy `gm.py` `BANNED_WORDS`).
const BANNED_WORDS_ALL_BANDS: &[&str] = &[
    "eldritch",
    "lovecraftian",
    "cosmic horror",
    "cthulhu",
    "sanity",
    "hit points",
    "hp",
    "game over",
    "npc",
    "player",
    "dice",
    "roll",
    "stat",
    "stats",
    "level up",
    "quest",
    "xp",
    "spawn",
    "simulation",
    "algorithm",
    "llm",
    "prompt",
    "monster",
    "monsters",
    "magic",
    "spell",
    "spells",
    "wizard",
    "sorcery",
    "fireball",
    "demon",
    "demons",
    "zombie",
    "zombies",
    "vampire",
];

/// Words additionally banned at Band <= 2 (overt ghosts/supernatural/explaining the mystery).
const BANNED_WORDS_BAND_2_AND_BELOW: &[&str] = &[
    "ghost",
    "ghosts",
    "phantom",
    "phantoms",
    "specter",
    "spectre",
    "apparition",
    "haunted",
    "supernatural",
    "poltergeist",
    "curse",
    "cursed",
    "hex",
    "warlock",
    "undead",
];

/// Words additionally banned at Band <= 1 (disembodied voices, impossible geometry).
const BANNED_WORDS_BAND_1_AND_BELOW: &[&str] = &[
    "disembodied",
    "impossible geometry",
    "non-euclidean",
    "telepathy",
    "telepathic",
    "levitate",
    "levitates",
    "levitating",
    "portal",
    "portals",
    "dimensional",
    "time loop",
    "hallucination",
    "mirage",
];

/// Words additionally banned at Band 0 (Grounded — any hint that the valley, wind, or river has
/// agency, memory, or intent).
const BANNED_WORDS_BAND_0_ONLY: &[&str] = &[
    "whispers",
    "whisper",
    "whispering",
    "malice",
    "malevolent",
    "sentient",
    "watching",
    "watches",
    "remembering",
    "remembers",
    "intent",
    "hungrily",
    "stalks",
    "stalking",
    "beckons",
    "beckoning",
    "murmurs",
    "omen",
    "omens",
];

/// Reason codes emitted by [`check_generated_line`] when a generated line violates the voice guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckReasonCode {
    /// Digits (`0..9`), counting words of two or more, or measurement/time/currency units.
    NumbersDigitsOrUnits,
    /// `"one"`, `"both"`, or `"pair"` before a resource noun, or `"half"` anywhere.
    QuantifierBeforeResourceOrHalf,
    /// Death or total-ruin vocabulary (`died`, `dead`, `corpse`, `grave`, etc.).
    DeathWords,
    /// Mid-sentence capital letter not matching an allowed landmark/place name.
    MidSentenceCapital,
    /// Supply or resource noun not in the turn's allowed resource list (or in sentences 2–3).
    UnauthorisedResourceNoun,
    /// Vocabulary banned at the turn's weirdness band (0–3) or in `BANNED_WORDS`.
    BandBannedVocabulary,
    /// Punctuation outside periods and commas (e.g. `;`, `!`, `?`, `:`, quotes, ellipses).
    InvalidPunctuation,
    /// Sentence count outside the configured [`NarratorLength`] cap (`Brief`/`Standard`/`Rich`).
    LengthCapExceeded,
    /// First or second person ("you", "we", "I" and their forms): prose is third-person
    /// impersonal (voice guide section 1). Added in step 4.
    PersonalPronoun,
    /// A party member's name appears in the line (voice guide L1c), including at the start of a
    /// sentence where the mid-sentence capital rule cannot see it. Added in step 4.
    NamedPartyMember,
    /// A draft-animal species ("mules", "oxen", "horses") the event itself does not name (voice
    /// guide L1d). Added in step 4.
    DraftAnimalSpecies,
    /// The line repeats the verb and a noun of the chosen option's label (voice guide L3).
    /// Added in step 4.
    ChoiceParroting,
}

impl CheckReasonCode {
    /// Every reason code, in declaration order.
    pub const ALL: [CheckReasonCode; 12] = [
        CheckReasonCode::NumbersDigitsOrUnits,
        CheckReasonCode::QuantifierBeforeResourceOrHalf,
        CheckReasonCode::DeathWords,
        CheckReasonCode::MidSentenceCapital,
        CheckReasonCode::UnauthorisedResourceNoun,
        CheckReasonCode::BandBannedVocabulary,
        CheckReasonCode::InvalidPunctuation,
        CheckReasonCode::LengthCapExceeded,
        CheckReasonCode::PersonalPronoun,
        CheckReasonCode::NamedPartyMember,
        CheckReasonCode::DraftAnimalSpecies,
        CheckReasonCode::ChoiceParroting,
    ];

    /// Stable snake_case name used in measurement reports.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NumbersDigitsOrUnits => "numbers_digits_or_units",
            Self::QuantifierBeforeResourceOrHalf => "quantifier_before_resource_or_half",
            Self::DeathWords => "death_words",
            Self::MidSentenceCapital => "mid_sentence_capital",
            Self::UnauthorisedResourceNoun => "unauthorised_resource_noun",
            Self::BandBannedVocabulary => "band_banned_vocabulary",
            Self::InvalidPunctuation => "invalid_punctuation",
            Self::LengthCapExceeded => "length_cap_exceeded",
            Self::PersonalPronoun => "personal_pronoun",
            Self::NamedPartyMember => "named_party_member",
            Self::DraftAnimalSpecies => "draft_animal_species",
            Self::ChoiceParroting => "choice_parroting",
        }
    }
}

/// Detailed failure returned by [`check_generated_line`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckFailure {
    pub code: CheckReasonCode,
    pub detail: String,
}

/// Turn-specific context for [`check_generated_line`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckerContext {
    pub allowed_resources: Vec<String>,
    pub allowed_landmarks: Vec<String>,
    pub weirdness_band: u8,
    pub length: NarratorLength,
    /// Party member names that must not appear in prose (empty = rule inactive).
    #[serde(default)]
    pub party_names: Vec<String>,
    /// The event's `narration_seed` text. Resource nouns and draft-animal species it names are
    /// allowed in prose (empty = only the turn's effects authorise a resource noun).
    #[serde(default)]
    pub event_seed: String,
    /// The chosen option's label, for the choice-parroting rule (empty = rule inactive).
    #[serde(default)]
    pub choice_label: String,
}

impl Default for CheckerContext {
    /// No allowed resources or landmarks, band 0, `Standard` length, and the step 4 rules
    /// inactive (no names, no seed text, no label).
    fn default() -> Self {
        Self {
            allowed_resources: Vec::new(),
            allowed_landmarks: Vec::new(),
            weirdness_band: 0,
            length: NarratorLength::Standard,
            party_names: Vec::new(),
            event_seed: String::new(),
            choice_label: String::new(),
        }
    }
}

impl CheckerContext {
    /// Build a [`CheckerContext`] from a [`NarrationContext`] and [`NarratorLength`].
    pub fn from_narration_context(ctx: &NarrationContext, length: NarratorLength) -> Self {
        let allowed_resources: Vec<String> = ctx
            .shown_deltas()
            .iter()
            .filter(|(_, &delta)| delta != 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut allowed_landmarks = Vec::new();
        if !ctx.place().is_empty() {
            allowed_landmarks.push(ctx.place().to_string());
        }
        Self {
            allowed_resources,
            allowed_landmarks,
            weirdness_band: weirdness_band_from_allowance(ctx.weirdness_allowance()),
            length,
            party_names: ctx.check_party_names().to_vec(),
            event_seed: ctx.check_event_seed().to_string(),
            choice_label: ctx.check_choice_label().to_string(),
        }
    }
}

/// First- and second-person words. Prose is third-person impersonal (voice guide section 1).
/// "mine" is left out on purpose: it is also a noun.
const PERSONAL_PRONOUN_WORDS: &[&str] = &[
    "i",
    "me",
    "my",
    "myself",
    "we",
    "us",
    "our",
    "ours",
    "ourselves",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
];

/// Draft-animal species names (voice guide L1d). Allowed only when the event seed names them.
const DRAFT_ANIMAL_SPECIES_WORDS: &[&str] = &[
    "mule",
    "mules",
    "ox",
    "oxen",
    "horse",
    "horses",
    "donkey",
    "donkeys",
    "burro",
    "burros",
    "cattle",
    "cow",
    "cows",
    "bull",
    "bulls",
    "steer",
    "steers",
    "mare",
    "mares",
    "stallion",
    "stallions",
    "pony",
    "ponies",
    "colt",
    "colts",
];

/// Function words ignored when reading the content words of a choice label.
const LABEL_STOP_WORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "and", "or", "with", "for", "in", "on", "at", "by", "from", "it",
    "its", "up", "down", "out", "off", "over", "into", "through", "across", "your", "our", "their",
    "his", "her", "them", "this", "that", "these", "those", "as", "is", "be", "all", "any", "some",
];

/// Crude stem used only to compare a prose word with a label word: strips one of `ing`, `ed`,
/// `es` or `s`, then a trailing `e`. `"bridges"` and `"bridge"` both give `"bridg"`.
fn crude_stem(word: &str) -> String {
    let w = word.to_ascii_lowercase();
    let mut s = w.as_str();
    if s.ends_with("ing") && s.len() > 5 {
        s = &s[..s.len() - 3];
    } else if (s.ends_with("ed") || s.ends_with("es")) && s.len() > 4 {
        s = &s[..s.len() - 2];
    } else if s.ends_with('s') && !s.ends_with("ss") && s.len() > 3 {
        s = &s[..s.len() - 1];
    }
    if s.ends_with('e') && s.len() > 3 {
        s = &s[..s.len() - 1];
    }
    s.to_string()
}

/// Content words of an option label: lowercase alphabetic tokens of three or more letters that
/// are not in [`LABEL_STOP_WORDS`].
fn label_content_words(label: &str) -> Vec<String> {
    label
        .split_whitespace()
        .map(clean_word_token)
        .filter(|w| w.len() >= 3 && !LABEL_STOP_WORDS.contains(&w.as_str()))
        .collect()
}

fn lookup_resource_key_for_noun(word_lower: &str) -> Option<&'static str> {
    for &(res_key, nouns) in RESOURCE_NOUN_TABLE {
        if nouns.contains(&word_lower) {
            return Some(res_key);
        }
    }
    None
}

fn clean_word_token(raw: &str) -> String {
    raw.trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

fn contains_phrase_or_word(line_lower: &str, tokens_lower: &[String], banned: &str) -> bool {
    if banned.contains(' ') || banned.contains('-') {
        line_lower.contains(banned)
    } else {
        tokens_lower.iter().any(|t| t == banned)
    }
}

/// Validate a single narrator-generated prose string (`scene` or `outcome`) against all hard
/// rules in the Prose Voice Guide (`VOICE-GUIDE-v1-DRAFT-2026-11-02.md`).
pub fn check_generated_line(line: &str, ctx: &CheckerContext) -> Result<(), CheckFailure> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "line is empty".to_string(),
        });
    }

    // 1. Punctuation check: periods and commas only (plus intra-word apostrophes/hyphens).
    if trimmed.contains("..")
        || trimmed.contains('…')
        || trimmed.contains(';')
        || trimmed.contains('!')
        || trimmed.contains('?')
        || trimmed.contains(':')
        || trimmed.contains('"')
        || trimmed.contains('(')
        || trimmed.contains(')')
        || trimmed.contains('[')
        || trimmed.contains(']')
        || !trimmed.ends_with('.')
    {
        return Err(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "disallowed punctuation or missing terminal period".to_string(),
        });
    }
    for ch in trimmed.chars() {
        if !(ch.is_ascii_alphanumeric()
            || ch == ' '
            || ch == '.'
            || ch == ','
            || ch == '\''
            || ch == '-')
        {
            return Err(CheckFailure {
                code: CheckReasonCode::InvalidPunctuation,
                detail: format!("disallowed character {ch:?}"),
            });
        }
    }

    // 2. Sentence count check against NarratorLength cap (Brief: 1, Standard: 1-2, Rich: 2-3).
    let raw_sentences: Vec<&str> = trimmed.split('.').collect();
    // Since trimmed ends with '.', the last element after split('.') must be empty, and all prior
    // elements must be non-empty sentences.
    if raw_sentences.last().map(|s| !s.is_empty()).unwrap_or(true) {
        return Err(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "trailing text after final period".to_string(),
        });
    }
    let sentences: Vec<&str> = raw_sentences[..raw_sentences.len() - 1]
        .iter()
        .map(|s| s.trim())
        .collect();
    if sentences.is_empty() || sentences.iter().any(|s| s.is_empty()) {
        return Err(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "empty sentence segment".to_string(),
        });
    }
    let (min_s, max_s) = ctx.length.sentence_bounds();
    if sentences.len() < min_s || sentences.len() > max_s {
        return Err(CheckFailure {
            code: CheckReasonCode::LengthCapExceeded,
            detail: format!(
                "expected {min_s}..={max_s} sentences for {:?}, found {}",
                ctx.length,
                sentences.len()
            ),
        });
    }

    // Collect cleaned lowercased word tokens across the line.
    let tokens_lower: Vec<String> = trimmed
        .split_whitespace()
        .map(clean_word_token)
        .filter(|w| !w.is_empty())
        .collect();

    // 3. QuantifierBeforeResourceOrHalf: "half" anywhere, or "one"/"both"/"pair" before a resource noun.
    for (idx, word) in tokens_lower.iter().enumerate() {
        if word == "half" {
            return Err(CheckFailure {
                code: CheckReasonCode::QuantifierBeforeResourceOrHalf,
                detail: "banned quantifier 'half'".to_string(),
            });
        }
        if word == "one" || word == "both" || word == "pair" {
            if let Some(next_word) = tokens_lower.get(idx + 1) {
                if lookup_resource_key_for_noun(next_word).is_some() {
                    return Err(CheckFailure {
                        code: CheckReasonCode::QuantifierBeforeResourceOrHalf,
                        detail: format!(
                            "quantifier '{word}' directly before resource '{next_word}'"
                        ),
                    });
                }
                if next_word == "of" {
                    if let Some(third_word) = tokens_lower.get(idx + 2) {
                        if lookup_resource_key_for_noun(third_word).is_some() {
                            return Err(CheckFailure {
                                code: CheckReasonCode::QuantifierBeforeResourceOrHalf,
                                detail: format!(
                                    "quantifier '{word} of' before resource '{third_word}'"
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    // 4. NumbersDigitsOrUnits: ASCII digits, counting words >= 2 (or zero), and unit words.
    // Note: "second" is allowed only when followed by "clause" or "sentence" as an ordinal;
    // standalone unit "second"/"seconds" is rejected.
    if trimmed.chars().any(|c| c.is_ascii_digit()) {
        return Err(CheckFailure {
            code: CheckReasonCode::NumbersDigitsOrUnits,
            detail: "contains ASCII digit".to_string(),
        });
    }
    for (idx, word) in tokens_lower.iter().enumerate() {
        if word == "second" {
            let next_is_ordinal_noun = tokens_lower
                .get(idx + 1)
                .map(|w| w == "clause" || w == "sentence")
                .unwrap_or(false);
            if next_is_ordinal_noun {
                continue;
            }
        }
        if BANNED_NUMBER_OR_UNIT_WORDS.contains(&word.as_str()) {
            return Err(CheckFailure {
                code: CheckReasonCode::NumbersDigitsOrUnits,
                detail: format!("banned number or unit word '{word}'"),
            });
        }
    }

    // 5. DeathWords
    for word in &tokens_lower {
        if BANNED_DEATH_WORDS.contains(&word.as_str()) {
            return Err(CheckFailure {
                code: CheckReasonCode::DeathWords,
                detail: format!("banned death word '{word}'"),
            });
        }
    }

    // 6. MidSentenceCapital: check each sentence after masking out allowed landmarks.
    let mut sorted_landmarks: Vec<&str> = ctx
        .allowed_landmarks
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    sorted_landmarks.sort_by_key(|b| std::cmp::Reverse(b.len()));

    for sentence in &sentences {
        let mut masked = (*sentence).to_string();
        for lm in &sorted_landmarks {
            if masked.contains(lm) {
                masked = masked.replace(lm, "landmark");
            }
        }
        for (w_idx, raw_token) in masked.split_whitespace().enumerate() {
            let clean_chars: Vec<char> = raw_token
                .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .chars()
                .collect();
            if clean_chars.is_empty() {
                continue;
            }
            if w_idx == 0 {
                if clean_chars.iter().skip(1).any(|c| c.is_ascii_uppercase()) {
                    return Err(CheckFailure {
                        code: CheckReasonCode::MidSentenceCapital,
                        detail: format!("unexpected uppercase inside first word '{raw_token}'"),
                    });
                }
            } else if clean_chars.iter().any(|c| c.is_ascii_uppercase()) {
                return Err(CheckFailure {
                    code: CheckReasonCode::MidSentenceCapital,
                    detail: format!("mid-sentence capital in '{raw_token}'"),
                });
            }
        }
    }

    // Words the event itself names (seed text and chosen label). A resource noun or draft-animal
    // species that the event names is not an invention (voice guide L1d, L1e).
    let event_named: Vec<String> = ctx
        .event_seed
        .split_whitespace()
        .chain(ctx.choice_label.split_whitespace())
        .map(clean_word_token)
        .filter(|w| !w.is_empty())
        .collect();

    // 7. UnauthorisedResourceNoun: resource nouns must belong to `ctx.allowed_resources` (or be
    // named by the event's seed or label) and may only appear in sentence 1 (index 0).
    for (s_idx, sentence) in sentences.iter().enumerate() {
        for raw_token in sentence.split_whitespace() {
            let word = clean_word_token(raw_token);
            if word.is_empty() {
                continue;
            }
            if let Some(res_key) = lookup_resource_key_for_noun(&word) {
                let is_allowed = ctx.allowed_resources.iter().any(|allowed| {
                    allowed == res_key
                        || allowed == &word
                        || (res_key == "lantern_oil" && allowed == "oil")
                }) || event_named.contains(&word);
                if !is_allowed {
                    return Err(CheckFailure {
                        code: CheckReasonCode::UnauthorisedResourceNoun,
                        detail: format!(
                            "resource noun '{word}' (key '{res_key}') not in allowed_resources {:?}",
                            ctx.allowed_resources
                        ),
                    });
                }
                if s_idx > 0 {
                    return Err(CheckFailure {
                        code: CheckReasonCode::UnauthorisedResourceNoun,
                        detail: format!(
                            "resource noun '{word}' appeared in sentence {} (only allowed in sentence 1)",
                            s_idx + 1
                        ),
                    });
                }
            }
        }
    }

    // 8. BandBannedVocabulary (cumulative from Band 3 down to Band 0)
    let line_lower = trimmed.to_ascii_lowercase();
    for &banned in BANNED_WORDS_ALL_BANDS {
        if contains_phrase_or_word(&line_lower, &tokens_lower, banned) {
            return Err(CheckFailure {
                code: CheckReasonCode::BandBannedVocabulary,
                detail: format!("word '{banned}' is banned across all bands"),
            });
        }
    }
    if ctx.weirdness_band <= 2 {
        for &banned in BANNED_WORDS_BAND_2_AND_BELOW {
            if contains_phrase_or_word(&line_lower, &tokens_lower, banned) {
                return Err(CheckFailure {
                    code: CheckReasonCode::BandBannedVocabulary,
                    detail: format!(
                        "word '{banned}' is banned at weirdness band {}",
                        ctx.weirdness_band
                    ),
                });
            }
        }
    }
    if ctx.weirdness_band <= 1 {
        for &banned in BANNED_WORDS_BAND_1_AND_BELOW {
            if contains_phrase_or_word(&line_lower, &tokens_lower, banned) {
                return Err(CheckFailure {
                    code: CheckReasonCode::BandBannedVocabulary,
                    detail: format!(
                        "word '{banned}' is banned at weirdness band {}",
                        ctx.weirdness_band
                    ),
                });
            }
        }
    }
    if ctx.weirdness_band == 0 {
        for &banned in BANNED_WORDS_BAND_0_ONLY {
            if contains_phrase_or_word(&line_lower, &tokens_lower, banned) {
                return Err(CheckFailure {
                    code: CheckReasonCode::BandBannedVocabulary,
                    detail: format!("word '{banned}' is banned at grounded band 0"),
                });
            }
        }
    }

    // 9. PersonalPronoun: first and second person, including contractions such as "you'll".
    for word in &tokens_lower {
        let stem = word.split('\'').next().unwrap_or(word);
        if PERSONAL_PRONOUN_WORDS.contains(&stem) {
            return Err(CheckFailure {
                code: CheckReasonCode::PersonalPronoun,
                detail: format!("first or second person word '{word}'"),
            });
        }
    }

    // 10. NamedPartyMember: any capitalised word that is a party member's name (or one of the
    // words of it), at any position in a sentence.
    for raw_token in trimmed.split_whitespace() {
        let cleaned = raw_token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if !cleaned
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
        {
            continue;
        }
        for name in &ctx.party_names {
            let hit = name.split_whitespace().any(|part| {
                let part = part.trim_matches(|c: char| !c.is_ascii_alphanumeric());
                part.len() >= 2 && part.eq_ignore_ascii_case(cleaned)
            });
            if hit {
                return Err(CheckFailure {
                    code: CheckReasonCode::NamedPartyMember,
                    detail: format!("party member name '{cleaned}'"),
                });
            }
        }
    }

    // 11. DraftAnimalSpecies, unless the event's own seed or label names it.
    for word in &tokens_lower {
        if DRAFT_ANIMAL_SPECIES_WORDS.contains(&word.as_str()) && !event_named.contains(word) {
            return Err(CheckFailure {
                code: CheckReasonCode::DraftAnimalSpecies,
                detail: format!("draft-animal species '{word}' not named by the event"),
            });
        }
    }

    // 12. ChoiceParroting: the label's first content word (its verb) and at least one of its
    // other content words (a noun) both reappear in the line, compared by crude stem.
    let label_words = label_content_words(&ctx.choice_label);
    if label_words.len() >= 2 {
        let line_stems: Vec<String> = tokens_lower.iter().map(|w| crude_stem(w)).collect();
        let verb_stem = crude_stem(&label_words[0]);
        if line_stems.contains(&verb_stem) {
            for noun in &label_words[1..] {
                if line_stems.contains(&crude_stem(noun)) {
                    return Err(CheckFailure {
                        code: CheckReasonCode::ChoiceParroting,
                        detail: format!(
                            "line repeats label verb '{}' and noun '{noun}'",
                            label_words[0]
                        ),
                    });
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Fallback Phrase Table & 464-Choice Composer + Release-Build Refusal
// ============================================================================

fn default_fallback_subject() -> String {
    "The party".to_string()
}

/// Source phrase entry (`{key, dir: "+"|"-", phrase}`) in the approved source table format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePhraseEntry {
    pub key: String,
    pub dir: String,
    pub phrase: String,
}

/// Source phrase table format (`PHRASE-TABLE-DRAFT-2-2026-11-02.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePhraseTable {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    pub subject: String,
    #[serde(default)]
    pub compose: String,
    pub none_phrase: String,
    pub phrases: Vec<SourcePhraseEntry>,
    #[serde(default)]
    pub note: String,
}

/// Shipped fallback phrase table (`rust/etv-core/content/fallback_phrases.json`), mapping
/// `(effect_key, direction)` to its approved clause, with `none.neutral` for "no effect" and
/// `key_order` preserving the approved source table's key ordering (`ammo` first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackPhraseTable {
    pub schema_version: u32,
    #[serde(default = "default_fallback_subject")]
    pub subject: String,
    #[serde(default)]
    pub key_order: Vec<String>,
    pub phrases: BTreeMap<String, BTreeMap<String, String>>,
}

impl FallbackPhraseTable {
    /// Load the compile-time embedded `fallback_phrases.json` table.
    pub fn embedded() -> Self {
        Self::from_json_str(EMBEDDED_FALLBACK_PHRASES_JSON)
            .expect("embedded fallback_phrases.json must be valid JSON")
    }

    /// Parse a `FallbackPhraseTable` from a JSON string.
    pub fn from_json_str(raw: &str) -> Result<Self, String> {
        let table: Self =
            serde_json::from_str(raw).map_err(|e| format!("invalid fallback_phrases.json: {e}"))?;
        if table.schema_version != 1 {
            return Err(format!(
                "unsupported fallback_phrases.json schema_version {} (expected 1)",
                table.schema_version
            ));
        }
        Ok(table)
    }

    /// Convert a source phrase table JSON string (`PHRASE-TABLE-DRAFT-2-2026-11-02.json` format)
    /// into a `FallbackPhraseTable`, mapping `"+"` -> `"pos"`, `"-"` -> `"neg"`, and
    /// `none_phrase` -> `phrases["none"]["neutral"]`, while preserving `subject` and `key_order`.
    pub fn from_source_table_json(raw: &str) -> Result<Self, String> {
        let src: SourcePhraseTable =
            serde_json::from_str(raw).map_err(|e| format!("invalid source phrase table: {e}"))?;
        let mut key_order = Vec::new();
        let mut phrases: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for entry in src.phrases {
            if !key_order.contains(&entry.key) {
                key_order.push(entry.key.clone());
            }
            let dir = match entry.dir.as_str() {
                "+" => "pos",
                "-" => "neg",
                other => {
                    return Err(format!(
                        "unexpected direction {other:?} for key {:?}",
                        entry.key
                    ))
                }
            };
            phrases
                .entry(entry.key)
                .or_default()
                .insert(dir.to_string(), entry.phrase);
        }
        let mut none_map = BTreeMap::new();
        none_map.insert("neutral".to_string(), src.none_phrase);
        phrases.insert("none".to_string(), none_map);

        Ok(Self {
            schema_version: 1,
            subject: src.subject,
            key_order,
            phrases,
        })
    }

    /// Extract the ordered `SourcePhraseEntry` list (`"+"` then `"-"`) and `none_phrase` in
    /// `self.key_order`.
    pub fn to_source_entries(&self) -> Result<(String, Vec<SourcePhraseEntry>), String> {
        let none_phrase = self
            .phrases
            .get("none")
            .and_then(|m| m.get("neutral"))
            .cloned()
            .ok_or_else(|| "missing none.neutral entry".to_string())?;
        let mut entries = Vec::new();
        for key in self.ordered_keys() {
            let dirs = self
                .phrases
                .get(key)
                .ok_or_else(|| format!("missing key {key:?} in phrases"))?;
            let pos = dirs
                .get("pos")
                .ok_or_else(|| format!("missing pos phrase for key {key:?}"))?;
            let neg = dirs
                .get("neg")
                .ok_or_else(|| format!("missing neg phrase for key {key:?}"))?;
            entries.push(SourcePhraseEntry {
                key: key.to_string(),
                dir: "+".to_string(),
                phrase: pos.clone(),
            });
            entries.push(SourcePhraseEntry {
                key: key.to_string(),
                dir: "-".to_string(),
                phrase: neg.clone(),
            });
        }
        Ok((none_phrase, entries))
    }

    /// Return the ordered effect keys (`self.key_order` when populated, or `self.phrases.keys()`
    /// excluding `"none"` as a fallback for minimal test tables).
    pub fn ordered_keys(&self) -> Vec<&str> {
        if !self.key_order.is_empty() {
            self.key_order.iter().map(String::as_str).collect()
        } else {
            self.phrases
                .keys()
                .filter(|k| k.as_str() != "none")
                .map(String::as_str)
                .collect()
        }
    }

    /// Compute the SHA-256 hex digest of the LF-normalized embedded `fallback_phrases.json` bytes.
    pub fn embedded_lf_sha256_hex() -> String {
        let lf_normalized = EMBEDDED_FALLBACK_PHRASES_JSON.replace("\r\n", "\n");
        let mut hasher = Sha256::new();
        hasher.update(lf_normalized.as_bytes());
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        hex_encode_32(&out)
    }

    /// Compute the SHA-256 hex digest of this table's canonical JSON representation
    /// (`canonicalize_json_value_no_floats`: compact UTF-8, object keys sorted recursively via
    /// `BTreeMap`, array order preserved for `key_order`, trailing `\n`).
    pub fn canonical_sha256_hex(&self) -> Result<String, String> {
        let val = serde_json::to_value(self)
            .map_err(|e| format!("failed to serialize FallbackPhraseTable: {e}"))?;
        let canonical = canonicalize_json_value_no_floats(&val)?;
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        Ok(hex_encode_32(&out))
    }

    /// Return all `(key, dir, value)` entries whose value starts with `"PLACEHOLDER-"`.
    pub fn placeholder_entries(&self) -> Vec<(String, String, String)> {
        let mut found = Vec::new();
        for (key, dirs) in &self.phrases {
            for (dir, val) in dirs {
                if val.starts_with("PLACEHOLDER-") {
                    found.push((key.clone(), dir.clone(), val.clone()));
                }
            }
        }
        found
    }

    /// Return all `(key, dir, value)` entries whose value does not equal
    /// `"PLACEHOLDER-<key>-<dir>"`.
    pub fn non_placeholder_entries(&self) -> Vec<(String, String, String)> {
        let mut bad = Vec::new();
        for (key, dirs) in &self.phrases {
            for (dir, val) in dirs {
                let expected = format!("PLACEHOLDER-{key}-{dir}");
                if val != &expected {
                    bad.push((key.clone(), dir.clone(), val.clone()));
                }
            }
        }
        bad
    }

    /// Returns `true` if every entry in the table is strictly `"PLACEHOLDER-<key>-<dir>"`.
    pub fn all_placeholders(&self) -> bool {
        !self.phrases.is_empty() && self.non_placeholder_entries().is_empty()
    }

    /// Returns `true` if any entry in the table starts with `"PLACEHOLDER-"`.
    pub fn has_placeholders(&self) -> bool {
        !self.placeholder_entries().is_empty()
    }

    /// Look up the phrase for `(key, dir)`, falling back to `"PLACEHOLDER-<key>-<dir>"`.
    pub fn lookup(&self, key: &str, dir: &str) -> String {
        self.phrases
            .get(key)
            .and_then(|dirs| dirs.get(dir))
            .cloned()
            .unwrap_or_else(|| format!("PLACEHOLDER-{key}-{dir}"))
    }
}

/// Errors returned when starting [`ComposedFallbackComposer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackComposerError {
    TableMissing,
    PlaceholdersInReleaseBuild,
}

/// Composes fallback outcome sentences from effect keys and directions using a
/// [`FallbackPhraseTable`].
///
/// In a release build (`cfg(not(debug_assertions))`), [`ComposedFallbackComposer::try_start`]
/// refuses to start if the phrase table is missing or contains any `"PLACEHOLDER-"` entry.
#[derive(Debug, Clone)]
pub struct ComposedFallbackComposer<'a> {
    table: &'a FallbackPhraseTable,
}

impl<'a> ComposedFallbackComposer<'a> {
    /// Start the composed-fallback path using the current build profile
    /// (`cfg!(not(debug_assertions))`).
    pub fn try_start(
        table: Option<&'a FallbackPhraseTable>,
    ) -> Result<Self, FallbackComposerError> {
        Self::try_start_with_mode(table, cfg!(not(debug_assertions)))
    }

    /// Start the composed-fallback path with an explicit `is_release_build` flag.
    pub fn try_start_with_mode(
        table: Option<&'a FallbackPhraseTable>,
        is_release_build: bool,
    ) -> Result<Self, FallbackComposerError> {
        let tbl = table.ok_or(FallbackComposerError::TableMissing)?;
        if is_release_build && tbl.has_placeholders() {
            return Err(FallbackComposerError::PlaceholdersInReleaseBuild);
        }
        Ok(Self { table: tbl })
    }

    /// Compose a fallback sentence from a slice of `(effect_key, delta)` pairs, ordering active
    /// non-zero keys according to `self.table.ordered_keys()` (`ammo` first) and formatting with
    /// `"<subject> <p1>, <p2> and <pN>."`. A key whose delta is `0` contributes no phrase.
    pub fn compose_from_effects(&self, effects: &[(&str, i32)]) -> String {
        let mut delta_by_key: BTreeMap<String, i32> = BTreeMap::new();
        for &(raw_key, delta) in effects {
            if delta == 0 {
                continue;
            }
            let key = if raw_key == "lantern_oil" {
                "oil"
            } else {
                raw_key
            };
            delta_by_key.insert(key.to_string(), delta);
        }
        self.compose_from_delta_map(&delta_by_key)
    }

    /// Compose a fallback sentence from a map of `effect_key -> delta`, ordering active non-zero
    /// keys according to `self.table.ordered_keys()`.
    pub fn compose_from_delta_map(&self, delta_by_key: &BTreeMap<String, i32>) -> String {
        let mut normalized: BTreeMap<String, i32> = BTreeMap::new();
        for (raw_key, &delta) in delta_by_key {
            if delta == 0 {
                continue;
            }
            let key = if raw_key == "lantern_oil" {
                "oil"
            } else {
                raw_key.as_str()
            };
            normalized.insert(key.to_string(), delta);
        }

        let mut clauses = Vec::new();
        let ordered = self.table.ordered_keys();
        for &key in &ordered {
            if let Some(&delta) = normalized.get(key) {
                if delta != 0 {
                    let dir = if delta < 0 { "neg" } else { "pos" };
                    clauses.push(self.table.lookup(key, dir));
                }
            }
        }
        for (key, &delta) in &normalized {
            if delta != 0 && !ordered.contains(&key.as_str()) {
                let dir = if delta < 0 { "neg" } else { "pos" };
                clauses.push(self.table.lookup(key, dir));
            }
        }
        if clauses.is_empty() {
            clauses.push(self.table.lookup("none", "neutral"));
        }

        let body = match clauses.len() {
            0 => String::new(),
            1 => clauses[0].clone(),
            2 => format!("{} and {}", clauses[0], clauses[1]),
            n => format!("{} and {}", clauses[..n - 1].join(", "), clauses[n - 1]),
        };
        let subject = self.table.subject.trim();
        let sentence = if subject.is_empty() {
            body
        } else {
            format!("{subject} {body}")
        };
        if sentence.ends_with('.') {
            sentence
        } else {
            format!("{sentence}.")
        }
    }

    /// Compose a fallback sentence directly from the post-clamp changes actually applied on a turn
    /// between `pre` (before the turn) and `state` (after the turn).
    pub fn compose_from_turn(&self, pre: &PreTurnObservation, state: &RunState) -> String {
        let applied = compute_applied_turn_deltas(pre, state);
        self.compose_from_delta_map(&applied)
    }
}

/// Compose one fallback sentence for each of the 464 choices across the 200 JSON events in
/// `pack.json_events` from their static effect profiles.
pub fn compose_all_464_choice_fallbacks(
    pack: &ContentPack,
    table: &FallbackPhraseTable,
) -> BTreeMap<(String, String), String> {
    let composer = ComposedFallbackComposer { table };
    let mut out = BTreeMap::new();
    for event in &pack.json_events {
        for choice in &event.fallback_choices {
            let mut effects: Vec<(&str, i32)> = Vec::new();
            if let Some(outcome) = event.outcome_templates.get(&choice.choice_id) {
                for (k, v) in outcome.ordered_supplies_delta() {
                    if v != 0 {
                        effects.push((k, v));
                    }
                }
                if outcome.health_delta != 0 {
                    effects.push(("health", outcome.health_delta));
                }
                if outcome.wagon_delta != 0 {
                    effects.push(("wagon_condition", outcome.wagon_delta));
                }
                if outcome.animals_health_delta != 0 {
                    effects.push(("animals_health", outcome.animals_health_delta));
                }
                if outcome.morale_delta != 0 {
                    effects.push(("morale", outcome.morale_delta));
                }
                if outcome.time_cost != 0 {
                    effects.push(("time_days", outcome.time_cost));
                }
                if outcome.distance_delta != 0 {
                    effects.push(("distance", outcome.distance_delta));
                }
            }
            let sentence = composer.compose_from_effects(&effects);
            out.insert((event.event_id.clone(), choice.choice_id.clone()), sentence);
        }
    }
    out
}

/// Resolve a turn's fallback line using [`ComposedFallbackComposer`] when allowed, or
/// returning `ctx.fallback_line()` (the engine's own plain line) when the phrase table is missing
/// or refused in a release build (`cfg(not(debug_assertions))`).
pub fn resolve_fallback_line(
    ctx: &NarrationContext,
    table: Option<&FallbackPhraseTable>,
) -> String {
    resolve_fallback_line_with_mode(ctx, table, cfg!(not(debug_assertions)))
}

/// Mode-parameterized helper for [`resolve_fallback_line`] so tests can verify release-build
/// refusal regardless of whether `cargo test` runs in debug or release profile.
pub fn resolve_fallback_line_with_mode(
    ctx: &NarrationContext,
    table: Option<&FallbackPhraseTable>,
    is_release_build: bool,
) -> String {
    match ComposedFallbackComposer::try_start_with_mode(table, is_release_build) {
        Ok(composer) => composer.compose_from_delta_map(ctx.shown_deltas()),
        Err(_) => ctx.fallback_line().to_string(),
    }
}

// ============================================================================
// Endpoint Validation & Settings Stub (Player Settings File Only)
// ============================================================================

/// Errors returned when validating an Ollama HTTP endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    InvalidScheme(String),
    InvalidAuthority(String),
    NonLoopbackRefused(String),
}

fn parse_endpoint_host(endpoint: &str) -> Result<String, EndpointError> {
    let trimmed = endpoint.trim();
    let rest = if let Some(r) = trimmed.strip_prefix("http://") {
        r
    } else if let Some(r) = trimmed.strip_prefix("https://") {
        r
    } else {
        return Err(EndpointError::InvalidScheme(endpoint.to_string()));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.chars().any(char::is_whitespace)
    {
        return Err(EndpointError::InvalidAuthority(endpoint.to_string()));
    }
    let host = if authority.starts_with('[') {
        if let Some(end_bracket) = authority.find(']') {
            &authority[..=end_bracket]
        } else {
            return Err(EndpointError::InvalidAuthority(endpoint.to_string()));
        }
    } else {
        authority.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        return Err(EndpointError::InvalidAuthority(endpoint.to_string()));
    }
    Ok(host.to_ascii_lowercase())
}

/// Returns `true` if `endpoint` points strictly to a loopback host (`127.0.0.1`, `localhost`,
/// or `[::1]`) with `http://` or `https://` and no userinfo (`@`).
pub fn is_loopback_endpoint(endpoint: &str) -> bool {
    match parse_endpoint_host(endpoint) {
        Ok(host) => host == "127.0.0.1" || host == "localhost" || host == "[::1]",
        Err(_) => false,
    }
}

/// Validate `endpoint`, refusing any non-loopback endpoint unless `allow_non_loopback` is
/// explicitly `true`.
pub fn validate_endpoint(
    endpoint: &str,
    allow_non_loopback: bool,
) -> Result<String, EndpointError> {
    if !allow_non_loopback && !is_loopback_endpoint(endpoint) {
        return Err(EndpointError::NonLoopbackRefused(endpoint.to_string()));
    }
    let _host = parse_endpoint_host(endpoint)?;
    Ok(endpoint.trim().trim_end_matches('/').to_string())
}

/// Player narrator settings (`profile`, `endpoint`, `model`, `length`, and timeouts).
///
/// Lives in the player's settings file only — **never** in the save file and **never** in
/// `content_hash` or `MechanicsStateRecord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NarratorSettings {
    pub enabled: bool,
    pub profile: GMProfile,
    pub endpoint: String,
    pub allow_non_loopback: bool,
    pub model: String,
    pub length: NarratorLength,
    /// Separate warm-up timeout allowance on start (30,000 ms = 30s default).
    pub warmup_timeout_ms: u64,
    /// Per-call timeout during play (10,000 ms = 10s default).
    pub per_call_timeout_ms: u64,
}

impl Default for NarratorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            profile: GMProfile::Fireside,
            endpoint: DEFAULT_OLLAMA_ENDPOINT.to_string(),
            allow_non_loopback: false,
            model: "llama3.2".to_string(),
            length: NarratorLength::Standard,
            warmup_timeout_ms: 30_000,
            per_call_timeout_ms: 10_000,
        }
    }
}

impl NarratorSettings {
    /// Load and validate [`NarratorSettings`] from a JSON string.
    pub fn from_json_str(raw: &str) -> Result<Self, String> {
        let mut settings: Self =
            serde_json::from_str(raw).map_err(|e| format!("invalid settings JSON: {e}"))?;
        let normalized = validate_endpoint(&settings.endpoint, settings.allow_non_loopback)
            .map_err(|e| format!("endpoint rejected: {e:?}"))?;
        settings.endpoint = normalized;
        Ok(settings)
    }

    /// Load and validate [`NarratorSettings`] from the player's settings file on disk.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read settings file {}: {e}", path.display()))?;
        Self::from_json_str(&raw)
    }
}

// ============================================================================
// Startup Messages (`src/escape_the_valley/gm.py:198-250`)
// ============================================================================

/// Check whether `target` matches any model name in `names` (`gm.py:198-210`).
pub fn model_listed(target: &str, names: &[String]) -> bool {
    let t = target.trim().to_ascii_lowercase();
    if t.is_empty() {
        return false;
    }
    let has_tag = t.contains(':');
    for raw in names {
        let name = raw.trim().to_ascii_lowercase();
        if name == t {
            return true;
        }
        if !has_tag && name == format!("{t}:latest") {
            return true;
        }
    }
    false
}

/// Extract model tag names from Ollama `/api/tags` JSON (`gm.py:213-232`).
pub fn extract_tag_names(tags_json: &str) -> Vec<String> {
    let Ok(val) = serde_json::from_str::<Value>(tags_json) else {
        return Vec::new();
    };
    let Some(models) = val.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in models {
        if let Some(obj) = entry.as_object() {
            if let Some(name) = obj
                .get("name")
                .or_else(|| obj.get("model"))
                .and_then(|v| v.as_str())
            {
                if !name.trim().is_empty() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// Plain-words missing-model startup message (`gm.py:235-243`), offering to play with the
/// narrator off (`trail tui --gm-off`).
pub fn model_missing_error(model: &str, detail: &str) -> String {
    let cleaned = detail.trim();
    let suffix = if cleaned.is_empty() {
        String::new()
    } else {
        format!(" ({cleaned})")
    };
    format!(
        "Storyteller model '{model}' is not installed in Ollama{suffix}. Run: ollama pull {model}, or trail tui --gm-off"
    )
}

/// Plain-words connection-error startup message (`gm.py:246-250`), offering to play with the
/// narrator off (`trail tui --gm-off`).
pub fn connect_error_message() -> String {
    "The storyteller is not running. Start it with: ollama serve, or play without it: trail tui --gm-off".to_string()
}

/// Result of checking Ollama and model readiness at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupCheckStatus {
    pub installed: bool,
    pub available: bool,
    pub offer_narrator_off: bool,
    pub message: Option<String>,
}

// ============================================================================
// HTTP Transport Trait, Optional `UreqTransport`, and `LiveNarrator`
// ============================================================================

/// Transport-layer error for narrator HTTP calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Timeout,
    ConnectionFailed(String),
    HttpStatus(u16, String),
    EndpointRejected(EndpointError),
}

/// Synchronous HTTP transport seam for [`LiveNarrator`].
///
/// Tests inject fake implementations (never touching the network); production builds with the
/// `narrator-ollama` Cargo feature use [`UreqTransport`].
pub trait NarratorTransport: Send + Sync {
    fn get_tags(&self, endpoint: &str, timeout: Duration) -> Result<(u16, String), TransportError>;
    fn post_generate(
        &self,
        endpoint: &str,
        body_json: &str,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError>;
}

/// Offline no-op transport used when [`LiveNarrator::new_offline`] is constructed without a
/// network transport.
#[derive(Debug, Default)]
pub struct OfflineTransport;

impl NarratorTransport for OfflineTransport {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Err(TransportError::ConnectionFailed(
            "offline transport".to_string(),
        ))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        _body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Err(TransportError::ConnectionFailed(
            "offline transport".to_string(),
        ))
    }
}

/// Synchronous `ureq` 3.4.2 transport compiled only when the `narrator-ollama` feature is enabled.
#[cfg(feature = "narrator-ollama")]
#[derive(Debug, Default, Clone, Copy)]
pub struct UreqTransport;

#[cfg(feature = "narrator-ollama")]
impl NarratorTransport for UreqTransport {
    fn get_tags(&self, endpoint: &str, timeout: Duration) -> Result<(u16, String), TransportError> {
        let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut resp = agent.get(&url).call().map_err(|e| match e {
            ureq::Error::Timeout(_) => TransportError::Timeout,
            other => TransportError::ConnectionFailed(other.to_string()),
        })?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok((status, body))
    }

    fn post_generate(
        &self,
        endpoint: &str,
        body_json: &str,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        let url = format!("{}/api/generate", endpoint.trim_end_matches('/'));
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut resp = agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send(body_json)
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => TransportError::Timeout,
                other => TransportError::ConnectionFailed(other.to_string()),
            })?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok((status, body))
    }
}

/// Diagnostic counters tracked by [`LiveNarrator`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarratorStats {
    pub calls: usize,
    pub successes: usize,
    pub timeouts: usize,
    pub json_errors: usize,
    pub rule_rejects: usize,
    pub fallbacks: usize,
}

/// Live local-Ollama narrator implementing [`Narrator`].
///
/// - Makes two calls per turn (`SCENE_SCHEMA` and `OUTCOME_SCHEMA`), strictly parses the JSON,
///   and checks every generated string with [`check_generated_line`].
/// - Any timeout, connection error, non-200 status, malformed JSON, or hard-rule failure returns
///   `None` for that slot so the engine displays its deterministic fallback line.
pub struct LiveNarrator {
    settings: NarratorSettings,
    transport: Arc<dyn NarratorTransport>,
    warmed_up: AtomicBool,
    calls: AtomicUsize,
    successes: AtomicUsize,
    timeouts: AtomicUsize,
    json_errors: AtomicUsize,
    rule_rejects: AtomicUsize,
    fallbacks: AtomicUsize,
    reject_codes: Mutex<BTreeMap<&'static str, usize>>,
    record_slots: AtomicBool,
    slot_log: Mutex<Vec<SlotRecord>>,
}

/// How one narrator call (scene or outcome) ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotOutcome {
    /// The line parsed and passed [`check_generated_line`].
    Accepted,
    /// The per-call timeout expired.
    Timeout,
    /// A connection or transport error other than a timeout.
    TransportError,
    /// A non-200 HTTP status.
    HttpStatus { status: u16 },
    /// The Ollama envelope was not JSON or had no `response` string.
    BadEnvelope,
    /// The model's JSON did not match the scene or outcome schema.
    BadSlotJson,
    /// The line parsed but failed the hard-rule checker.
    RuleReject {
        code: CheckReasonCode,
        detail: String,
    },
}

/// One recorded narrator call. Recording is off by default and is turned on by measurement
/// harnesses with [`LiveNarrator::enable_slot_recording`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRecord {
    pub is_scene: bool,
    pub outcome: SlotOutcome,
    /// The model's whole `response` string, when one came back (kept for every outcome).
    pub raw_response: Option<String>,
    /// The prose the model wrote for the slot, when the JSON parsed (accepted or rejected).
    pub candidate: Option<String>,
    /// Wall-clock time of the call in milliseconds.
    pub latency_ms: u64,
}

impl LiveNarrator {
    /// Construct a [`LiveNarrator`] backed by [`OfflineTransport`].
    pub fn new_offline(settings: NarratorSettings) -> Self {
        Self::with_transport(settings, Arc::new(OfflineTransport))
    }

    /// Construct a [`LiveNarrator`] with a custom [`NarratorTransport`].
    pub fn with_transport(
        settings: NarratorSettings,
        transport: Arc<dyn NarratorTransport>,
    ) -> Self {
        Self {
            settings,
            transport,
            warmed_up: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
            timeouts: AtomicUsize::new(0),
            json_errors: AtomicUsize::new(0),
            rule_rejects: AtomicUsize::new(0),
            fallbacks: AtomicUsize::new(0),
            reject_codes: Mutex::new(BTreeMap::new()),
            record_slots: AtomicBool::new(false),
            slot_log: Mutex::new(Vec::new()),
        }
    }

    /// Start keeping a [`SlotRecord`] for every call (off by default).
    pub fn enable_slot_recording(&self) {
        self.record_slots.store(true, Ordering::SeqCst);
    }

    /// Take and clear the recorded calls, oldest first.
    pub fn take_slot_records(&self) -> Vec<SlotRecord> {
        match self.slot_log.lock() {
            Ok(mut log) => std::mem::take(&mut *log),
            Err(_) => Vec::new(),
        }
    }

    /// Hard-rule rejects so far, by [`CheckReasonCode::as_str`] name.
    pub fn reject_code_counts(&self) -> BTreeMap<&'static str, usize> {
        self.reject_codes
            .lock()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    fn push_record(
        &self,
        is_scene: bool,
        outcome: SlotOutcome,
        raw_response: Option<String>,
        candidate: Option<String>,
        started: Instant,
    ) {
        if !self.record_slots.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut log) = self.slot_log.lock() {
            log.push(SlotRecord {
                is_scene,
                outcome,
                raw_response,
                candidate,
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            });
        }
    }

    /// Construct a [`LiveNarrator`] backed by [`UreqTransport`] (`narrator-ollama` feature).
    #[cfg(feature = "narrator-ollama")]
    pub fn new_ureq(settings: NarratorSettings) -> Self {
        Self::with_transport(settings, Arc::new(UreqTransport))
    }

    /// Active [`NarratorSettings`].
    pub fn settings(&self) -> &NarratorSettings {
        &self.settings
    }

    /// Snapshot of diagnostic counters.
    pub fn stats(&self) -> NarratorStats {
        NarratorStats {
            calls: self.calls.load(Ordering::SeqCst),
            successes: self.successes.load(Ordering::SeqCst),
            timeouts: self.timeouts.load(Ordering::SeqCst),
            json_errors: self.json_errors.load(Ordering::SeqCst),
            rule_rejects: self.rule_rejects.load(Ordering::SeqCst),
            fallbacks: self.fallbacks.load(Ordering::SeqCst),
        }
    }

    /// Returns `true` if [`Self::warmup`] has completed cleanly.
    pub fn is_warmed_up(&self) -> bool {
        self.warmed_up.load(Ordering::SeqCst)
    }

    /// Check Ollama connectivity and model installation at startup, returning plain-words
    /// guidance and `offer_narrator_off = true` if the service is down or the model is missing.
    pub fn check_startup(&self) -> StartupCheckStatus {
        if !self.settings.enabled {
            return StartupCheckStatus {
                installed: false,
                available: false,
                offer_narrator_off: false,
                message: None,
            };
        }
        let endpoint =
            match validate_endpoint(&self.settings.endpoint, self.settings.allow_non_loopback) {
                Ok(ep) => ep,
                Err(e) => {
                    return StartupCheckStatus {
                        installed: false,
                        available: false,
                        offer_narrator_off: true,
                        message: Some(format!("Endpoint refused ({e:?}). Use trail tui --gm-off")),
                    };
                }
            };
        let timeout = Duration::from_millis(self.settings.per_call_timeout_ms);
        match self.transport.get_tags(&endpoint, timeout) {
            Ok((200, body)) => {
                let names = extract_tag_names(&body);
                if model_listed(&self.settings.model, &names) {
                    StartupCheckStatus {
                        installed: true,
                        available: true,
                        offer_narrator_off: false,
                        message: None,
                    }
                } else {
                    StartupCheckStatus {
                        installed: true,
                        available: false,
                        offer_narrator_off: true,
                        message: Some(model_missing_error(&self.settings.model, "")),
                    }
                }
            }
            Ok((status, _)) => StartupCheckStatus {
                installed: false,
                available: false,
                offer_narrator_off: true,
                message: Some(format!("{} (HTTP {status})", connect_error_message())),
            },
            Err(_) => StartupCheckStatus {
                installed: false,
                available: false,
                offer_narrator_off: true,
                message: Some(connect_error_message()),
            },
        }
    }

    /// Perform the one-time warm-up request on start using the separate warm-up timeout
    /// allowance (`settings.warmup_timeout_ms`, default 30s).
    pub fn warmup(&self) -> Result<(), String> {
        if !self.settings.enabled {
            return Ok(());
        }
        let endpoint = validate_endpoint(&self.settings.endpoint, self.settings.allow_non_loopback)
            .map_err(|e| format!("endpoint rejected: {e:?}"))?;
        let warmup_timeout = Duration::from_millis(self.settings.warmup_timeout_ms);
        let payload = serde_json::json!({
            "model": self.settings.model,
            "prompt": "Warm up.",
            "stream": false,
            "options": {
                "num_predict": 1
            }
        })
        .to_string();
        match self
            .transport
            .post_generate(&endpoint, &payload, warmup_timeout)
        {
            Ok((200, _)) => {
                self.warmed_up.store(true, Ordering::SeqCst);
                Ok(())
            }
            Ok((status, _)) => Err(format!("warm-up HTTP {status}")),
            Err(TransportError::Timeout) => Err("warm-up timed out".to_string()),
            Err(e) => Err(format!("warm-up failed: {e:?}")),
        }
    }

    fn call_ollama_slot(
        &self,
        endpoint: &str,
        system_prompt: &str,
        user_prompt: &str,
        is_scene: bool,
        checker_ctx: &CheckerContext,
    ) -> Option<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = Instant::now();
        let timeout = Duration::from_millis(self.settings.per_call_timeout_ms);
        let payload = serde_json::json!({
            "model": self.settings.model,
            "system": system_prompt,
            "prompt": user_prompt,
            "format": "json",
            "stream": false,
            "options": {
                "temperature": 0.7,
                "num_predict": 512
            }
        })
        .to_string();

        let (status, raw_outer) = match self.transport.post_generate(endpoint, &payload, timeout) {
            Ok(pair) => pair,
            Err(TransportError::Timeout) => {
                self.timeouts.fetch_add(1, Ordering::SeqCst);
                self.fallbacks.fetch_add(1, Ordering::SeqCst);
                self.push_record(is_scene, SlotOutcome::Timeout, None, None, started);
                return None;
            }
            Err(_) => {
                self.fallbacks.fetch_add(1, Ordering::SeqCst);
                self.push_record(is_scene, SlotOutcome::TransportError, None, None, started);
                return None;
            }
        };
        if status != 200 {
            self.fallbacks.fetch_add(1, Ordering::SeqCst);
            self.push_record(
                is_scene,
                SlotOutcome::HttpStatus { status },
                None,
                None,
                started,
            );
            return None;
        }

        let outer_val: Value = match serde_json::from_str(&raw_outer) {
            Ok(v) => v,
            Err(_) => {
                self.json_errors.fetch_add(1, Ordering::SeqCst);
                self.fallbacks.fetch_add(1, Ordering::SeqCst);
                self.push_record(is_scene, SlotOutcome::BadEnvelope, None, None, started);
                return None;
            }
        };
        let Some(inner_str) = outer_val.get("response").and_then(|v| v.as_str()) else {
            self.json_errors.fetch_add(1, Ordering::SeqCst);
            self.fallbacks.fetch_add(1, Ordering::SeqCst);
            self.push_record(is_scene, SlotOutcome::BadEnvelope, None, None, started);
            return None;
        };

        let parsed = if is_scene {
            parse_scene_response_strict(inner_str).map(|scene| scene.narration)
        } else {
            parse_outcome_response_strict(inner_str).map(|outcome| outcome.outcome_narration)
        };
        let candidate_prose = match parsed {
            Ok(prose) => prose,
            Err(_) => {
                self.json_errors.fetch_add(1, Ordering::SeqCst);
                self.fallbacks.fetch_add(1, Ordering::SeqCst);
                self.push_record(
                    is_scene,
                    SlotOutcome::BadSlotJson,
                    Some(inner_str.to_string()),
                    None,
                    started,
                );
                return None;
            }
        };

        if let Err(failure) = check_generated_line(&candidate_prose, checker_ctx) {
            self.rule_rejects.fetch_add(1, Ordering::SeqCst);
            self.fallbacks.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut codes) = self.reject_codes.lock() {
                *codes.entry(failure.code.as_str()).or_insert(0) += 1;
            }
            self.push_record(
                is_scene,
                SlotOutcome::RuleReject {
                    code: failure.code,
                    detail: failure.detail,
                },
                Some(inner_str.to_string()),
                Some(candidate_prose),
                started,
            );
            return None;
        }

        self.successes.fetch_add(1, Ordering::SeqCst);
        self.push_record(
            is_scene,
            SlotOutcome::Accepted,
            Some(inner_str.to_string()),
            Some(candidate_prose.clone()),
            started,
        );
        Some(candidate_prose)
    }
}

impl Narrator for LiveNarrator {
    fn narrate(&self, ctx: &NarrationContext) -> Option<String> {
        self.narrate_turn(ctx).outcome
    }

    fn narrate_turn(&self, ctx: &NarrationContext) -> TurnNarration {
        if !self.settings.enabled {
            return TurnNarration::default();
        }
        let Ok(endpoint) =
            validate_endpoint(&self.settings.endpoint, self.settings.allow_non_loopback)
        else {
            self.fallbacks.fetch_add(2, Ordering::SeqCst);
            return TurnNarration::default();
        };

        let profile = self.settings.profile;
        let length = self.settings.length;
        let checker_ctx = CheckerContext::from_narration_context(ctx, length);

        let (scene_sys, scene_user) = build_scene_prompts(profile, length, ctx);
        let scene = self.call_ollama_slot(&endpoint, &scene_sys, &scene_user, true, &checker_ctx);

        let (out_sys, out_user) = build_outcome_prompts(profile, length, ctx);
        let outcome = self.call_ollama_slot(&endpoint, &out_sys, &out_user, false, &checker_ctx);

        TurnNarration { scene, outcome }
    }
}
