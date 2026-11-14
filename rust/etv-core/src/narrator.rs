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
use std::collections::{BTreeMap, BTreeSet};
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
///
/// The shipped table extends draft 2 by four phrases that draft 2 does not hold: meds and salt,
/// each spent and gained (approved by the Director on 2026-11-14). This stays the draft-2 hash.
pub const SOURCE_PHRASE_TABLE_SHA256: &str =
    "ef29eecf13ba985708d21ad9f002ab4a2de5af4aa70dc914f8c1135f4bf12a22";

/// Expected SHA-256 digest of the canonical JSON form (`canonicalize_json_value_no_floats`) of
/// `rust/etv-core/content/fallback_phrases.json`.
pub const SHIPPED_FALLBACK_PHRASES_CANONICAL_SHA256: &str =
    "f330779e72b7c41b6f008d5c92be4212b3d6de9e7e3f2142c156176db0fb4418";

/// Expected SHA-256 digest of the LF-normalized bytes of `rust/etv-core/content/fallback_phrases.json`.
pub const SHIPPED_FALLBACK_PHRASES_LF_SHA256: &str =
    "ebc9e9a05ac4e3e964f241a8cde9f7ed37ddd6c1197001b634b400f0e705ea57";

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
    /// The event's `narration_seed` text: the game's own description of the situation. It is
    /// prompt material (the narrator is told what is happening), and the checker also uses it:
    /// resource nouns and draft-animal species the event itself names are allowed in prose.
    #[serde(default)]
    check_event_seed: String,
    /// The chosen option's label. Prompt material (the narrator is told what the party chose),
    /// and used by the checker's choice-parroting rule, which stops it being copied.
    #[serde(default)]
    check_choice_label: String,
    /// The event's title, when the turn has an event on screen or has just resolved one.
    /// Prompt material: the narrator is told what is happening.
    #[serde(default)]
    event_title: String,
    /// The labels of the choices on offer, when the turn ends with an event on screen.
    /// Prompt material.
    #[serde(default)]
    offered_labels: Vec<String>,
    /// What the party did at camp on this turn (`travel`, `rest`, `hunt`, ...), or empty on a
    /// turn that resolved an event choice or took a fork. Prompt material.
    #[serde(default)]
    action: String,
    /// How the run ended, when this turn ended it: the engine's ending grade (`triumphant`,
    /// `weathered`, `pyrrhic` or `lost`). Empty on every other turn. Prompt material.
    #[serde(default)]
    ending: String,
    /// The run's seed number. Used **only** to derive the sampling seed sent with a narrator
    /// call, so the same run and turn ask for the same sample. It is a plain number: it gives
    /// no access to the engine's RNG. Never prompt material.
    #[serde(default)]
    run_seed: i128,
    /// The run's step counter after this turn ([`crate::model::RunState::turns_stepped`]): it
    /// advances on every engine step, choice and fork turns included. Used **only** in the
    /// sampling seed, so two turns with the same facts (a rest repeated on the same day, or two
    /// choice turns that leave the journal unchanged) ask for different samples. Never prompt
    /// material. Turn sets written before it existed read it as 0.
    #[serde(default)]
    turn_index: u64,
    /// Supply keys that stand at zero after the turn, sorted. Not prompt material: the
    /// "says what happened" checks use it to tell "no water" on an empty cask from "no water"
    /// on a turn that only spent some.
    #[serde(default)]
    supplies_at_zero: Vec<String>,
}

impl NarrationContext {
    /// Attach the party names, the event's seed text and the chosen option's label.
    ///
    /// The party names feed the checker only and are never in a prompt. The seed text and the
    /// label are both prompt material and checker facts.
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

    /// The same context with `gm_profile` set to `profile`. For a recorded context that is
    /// replayed under each profile in turn.
    pub fn with_gm_profile(mut self, profile: GMProfile) -> Self {
        self.gm_profile = profile;
        self
    }

    /// Attach what else the narrator is told about the turn: the event's title, the labels on
    /// offer, and the camp action taken.
    pub fn with_event_facts(
        mut self,
        event_title: String,
        offered_labels: Vec<String>,
        action: String,
    ) -> Self {
        self.event_title = event_title;
        self.offered_labels = offered_labels;
        self.action = action;
        self
    }

    /// Attach the ending grade of a run that this turn ended.
    pub fn with_ending(mut self, ending: String) -> Self {
        self.ending = ending;
        self
    }

    /// Attach the supply keys that stand at zero after the turn.
    pub fn with_supplies_at_zero(mut self, mut keys: Vec<String>) -> Self {
        keys.sort();
        keys.dedup();
        self.supplies_at_zero = keys;
        self
    }

    /// Supply keys that stand at zero after the turn, sorted.
    pub fn supplies_at_zero(&self) -> &[String] {
        &self.supplies_at_zero
    }

    /// The ending grade, when this turn ended the run (empty otherwise).
    pub fn ending(&self) -> &str {
        &self.ending
    }

    /// The event's title (empty when the turn has no event).
    pub fn event_title(&self) -> &str {
        &self.event_title
    }

    /// Labels of the choices on offer (empty unless an event is on screen).
    pub fn offered_labels(&self) -> &[String] {
        &self.offered_labels
    }

    /// The camp action taken on this turn (empty on a choice or fork turn).
    pub fn action(&self) -> &str {
        &self.action
    }

    /// Attach the run's seed number (see [`narration_call_seed`]).
    pub fn with_run_seed(mut self, run_seed: i128) -> Self {
        self.run_seed = run_seed;
        self
    }

    /// The run's seed number (0 unless attached).
    pub fn run_seed(&self) -> i128 {
        self.run_seed
    }

    /// Attach the turn's index (the run's step counter after the turn; see
    /// [`narration_call_seed`]).
    pub fn with_turn_index(mut self, turn_index: u64) -> Self {
        self.turn_index = turn_index;
        self
    }

    /// The turn's index (0 unless attached).
    pub fn turn_index(&self) -> u64 {
        self.turn_index
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
            event_title: String::new(),
            offered_labels: Vec::new(),
            action: String::new(),
            ending: String::new(),
            run_seed: 0,
            turn_index: 0,
            supplies_at_zero: Vec::new(),
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

/// Compute the change actually applied on a turn for every supply the run holds and the six
/// non-supply keys, by a pure read of `pre` (before the turn) and `state` (after the turn), with
/// zero RNG draws and zero state writes:
///
/// - Supplies: every key in `pre.supplies` or `state.supplies.items` (so `meds` and `salt` too,
///   and any key the engine adds later): `state.supplies.get(key) - pre.supplies.get(key)`, with
///   `"oil"` reading `"lantern_oil"` in `SuppliesState`, where `worldgen::resolve_resource_alias`
///   maps `"oil"`.
/// - `health`: sum across party members of `after.health - before_health` (each member clamped to
///   `0..=100` in `events::apply_outcome`).
/// - `animals_health`: `state.wagon.animals_health - pre.animals_health` (clamped to `0..=100`).
/// - `wagon_condition`: `state.wagon.condition - pre.wagon_condition` (clamped to `0..=100`).
/// - `morale`: `state.party.morale - pre.morale` (clamped to `0..=100`).
/// - `distance`: progress along the road, positive for miles covered. On event choice turns
///   (`pre.phase == GamePhase::Event`) it is `pre.distance_remaining -
///   state.distance_remaining`, so a choice that lengthened the road reads as ground lost;
///   on other turns it is `state.distance_traveled - pre.distance_traveled`.
/// - `time_days`: quarter-day slot delta `(state.day * 4 + slot(state.time_of_day)) -
///   (pre.day * 4 + slot(pre.time_of_day))` produced by `StepEngine::advance_time`.
pub fn compute_applied_turn_deltas(
    pre: &PreTurnObservation,
    state: &RunState,
) -> BTreeMap<String, i32> {
    let mut applied = BTreeMap::new();

    // Every supply the run holds before or after the turn, read off the engine's own maps rather
    // than a hand-written list (a hand-written list once left out meds and salt). The lamp oil
    // keys are handled below, because the phrase table names them `oil`.
    let supply_keys: BTreeSet<&str> = pre
        .supplies
        .keys()
        .chain(state.supplies.items.keys())
        .map(String::as_str)
        .filter(|k| *k != "lantern_oil" && *k != "oil")
        .collect();
    for key in supply_keys {
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
        pre.distance_remaining - state.distance_remaining
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

/// Build a [`NarrationContext`] whose shown deltas are exactly `shown_deltas`, the changes a
/// turn applied (all effect keys, not only supplies). For callers that hold a recorded turn
/// rather than a live pre-turn observation.
pub fn build_narration_context_with_deltas(
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

    // What the event is and what was chosen. While an event is on screen the step's messages
    // carry its title, text and choices. On the turn a choice resolves, the messages have moved
    // on to the outcome, and the journal entry the engine has just written holds them.
    let resolved_entry = if msgs.outcome_title.is_empty() {
        None
    } else {
        state
            .journal
            .last()
            .filter(|j| j.event_id == event_id && !j.choice_made.is_empty())
    };
    let event_title = if !msgs.event_title.is_empty() {
        msgs.event_title.clone()
    } else {
        resolved_entry
            .map(|j| j.scene_title.clone())
            .unwrap_or_default()
    };
    let event_seed = if !msgs.event_narration.is_empty() {
        msgs.event_narration.clone()
    } else {
        resolved_entry
            .map(|j| j.narration.clone())
            .unwrap_or_default()
    };
    let choice_label = msgs
        .event_choices
        .iter()
        .find(|c| c.id == choice_id)
        .map(|c| c.label.clone())
        .or_else(|| {
            resolved_entry.and_then(|j| {
                j.choice_made
                    .split_once(": ")
                    .map(|(_, label)| label.to_string())
            })
        })
        .unwrap_or_default();
    let offered_labels: Vec<String> = if phase == GamePhase::Event {
        msgs.event_choices.iter().map(|c| c.label.clone()).collect()
    } else {
        Vec::new()
    };
    // The camp action, from the engine's own record of it. Empty when the step resolved a
    // choice or took a fork, where the action is the choice itself.
    let action = if choice_id.is_empty() {
        state.last_action.to_ascii_lowercase()
    } else {
        String::new()
    };

    // The ending grade, on the turn the run ends.
    let ending = if state.game_over {
        state
            .ending
            .as_ref()
            .map(|e| e.tier.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

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
        situation_for_narrator(&brief.situation),
        party_status,
    )
    .with_check_facts(
        state.party.members.iter().map(|m| m.name.clone()).collect(),
        event_seed,
        choice_label,
    )
    .with_event_facts(event_title, offered_labels, action)
    .with_ending(ending)
    .with_run_seed(state.seed)
    .with_turn_index(state.turns_stepped)
    .with_supplies_at_zero(
        state
            .supplies
            .items
            .iter()
            .filter(|(_, amount)| **amount <= 0)
            .map(|(key, _)| key.clone())
            .collect(),
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

    /// Most words a narration line may run to at this length. The cap is on words, not on
    /// sentences, so a voice that writes short sentences is not cut off for it.
    pub fn word_cap(&self) -> usize {
        match self {
            NarratorLength::Brief => 25,
            NarratorLength::Standard => 45,
            NarratorLength::Rich => 70,
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

/// The headers state no sentence limit: the length setting is the only authority on sentence
/// count (the Python headers each carried their own maximum, which contradicted it).
///
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
- Tone: dry, observant, unsentimental, occasionally wry."
        }
        GMProfile::Fireside => {
            "You are \"The Fireside Storyteller,\" a warm, ominous folk narrator for a survival trail game.\n\
Voice rules:\n\
- Write like someone telling a story by camp light — vivid, personal, slightly haunted.\n\
- Notice the human details: how people hold their shoulders, what they don't say, small kindnesses.\n\
- When the uncanny appears, treat it like old country knowledge — folk warnings, grandmother's rules.\n\
- Balance warmth and dread. The trail is hard, the people are real, something watches.\n\
- Tone: empathetic, atmospheric, quietly foreboding."
        }
        GMProfile::Lantern => {
            "You are \"The Uncanny Bearer,\" a serious but uncanny storyteller for a survival trail game.\n\
Voice rules:\n\
- The valley is wrong and you know it. Not horror — something older and stranger.\n\
- Describe normal things in ways that feel slightly off: geometry that doesn't add up, echoes that arrive early.\n\
- Still respect the survival game: supplies matter, injuries hurt, distances are real.\n\
- When uncanny tokens are spent, let the weirdness breathe — a full surreal moment before snapping back.\n\
- Tone: precise, eerie, deadpan when describing the impossible."
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
    shared_voice_hard_rules_for(length, weirdness_band, false)
}

/// Most `sentences` items an answer may hold. The length cap is on words; this only keeps the
/// array bounded.
pub const NARRATION_MAX_SENTENCE_ITEMS: usize = 8;

/// The shared rule on death, as the prompt states it. On a turn where a death happened, or a run
/// ended lost or pyrrhic, the death may be stated plainly; everywhere else the ban stands.
pub const DEATH_RULE_BANNED: &str = "- Never state or imply death or total ruin.";

/// The death rule on a turn where the facts report a death or a failed journey.
pub const DEATH_RULE_ALLOWED: &str = "- A death or the journey's failure reported in the turn facts may be stated plainly: without sentiment, without a name, without gore. Never state or imply any other death or ruin.";

/// [`shared_voice_hard_rules`] with the death rule chosen for the turn (see
/// [`death_may_be_stated`]).
pub fn shared_voice_hard_rules_for(
    length: NarratorLength,
    weirdness_band: u8,
    death_allowed: bool,
) -> String {
    let cap = length.word_cap();
    let death_rule = if death_allowed {
        DEATH_RULE_ALLOWED
    } else {
        DEATH_RULE_BANNED
    };
    format!(
        "SHARED VOICE HARD RULES:\n\
- Present tense, third-person impersonal camera at wheel-height in mud and weather.\n\
- Never state digits, counting words of two or more, or measurement/time/currency units.\n\
- Allowed quantifiers for wagon, team, or body only: one, both, pair (never before a supply noun). Never use half.\n\
{death_rule}\n\
- Never name a party member or invent proper nouns. No mid-sentence capital letters except the current place name.\n\
- Never mention a supply or resource noun unless that resource key was spent or gained on this turn, or the situation names it.\n\
- Weirdness band ceiling: band {weirdness_band}. Never use horror cliches, magic, monsters, or game/meta terms.\n\
- Punctuation: periods and commas only. No semicolons, exclamation marks, question marks, colons, quotes, or ellipses.\n\
- LENGTH CAP: {} (at most {cap} words in all).",
        length.as_str()
    )
}

/// Which wording of the shared rules goes into the system prompt.
///
/// The game always sends `Shipped`. `Positive` exists for the measurement harness only: it says
/// what to do without naming any banned word, so the two can be compared on the same turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptRules {
    #[default]
    Shipped,
    Positive,
}

impl PromptRules {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromptRules::Shipped => "shipped",
            PromptRules::Positive => "positive",
        }
    }
}

/// The shared rules worded as instructions only: no banned word or banned mark is named.
/// Measurement variant; see [`PromptRules`].
pub fn shared_voice_rules_positive(length: NarratorLength, weirdness_band: u8) -> String {
    shared_voice_rules_positive_for(length, weirdness_band, false)
}

/// [`shared_voice_rules_positive`] with the death rule chosen for the turn.
pub fn shared_voice_rules_positive_for(
    length: NarratorLength,
    weirdness_band: u8,
    death_allowed: bool,
) -> String {
    let cap = length.word_cap();
    let fate_rule = if death_allowed {
        "- Tell only what the turn facts support. Where they report a death or a failed journey, say so plainly and briefly."
    } else {
        "- Tell only what the turn facts support, and leave every traveller as the facts give them."
    };
    format!(
        "SHARED VOICE RULES:\n\
- Present tense, third-person impersonal camera at wheel-height in mud and weather.\n\
- Speak of amounts in general terms only.\n\
{fate_rule}\n\
- Refer to people as the party or the travellers. The current place name is the only proper noun.\n\
- Mention only the supplies listed under resource_directions or named in the situation.\n\
- Keep to plain physical detail within weirdness band {weirdness_band}.\n\
- End each sentence with a period, and use commas inside sentences.\n\
- LENGTH CAP: {} (at most {cap} words in all).",
        length.as_str()
    )
}

/// The engine's situation text states the caravan's doctrine and taboo as plain sentences, one
/// of which ("Morale holds, but the wagon is lean.") reads as the party's present morale and can
/// contradict it. For the narrator they are rewritten as what they are: a standing doctrine and
/// a standing taboo. The engine's own text is left alone (the Python comparison checks it).
pub const SITUATION_RELABELS: [(&str, &str); 6] = [
    (
        "The caravan travels light — less to eat, less to carry.",
        "Their doctrine: travel light.",
    ),
    (
        "Every repair is deliberate. The caravan moves carefully.",
        "Their doctrine: careful hands.",
    ),
    (
        "They carry no debts. Morale holds, but the wagon is lean.",
        "Their doctrine: no debts.",
    ),
    (
        "They will not travel at night.",
        "Their taboo: no travel at night.",
    ),
    (
        "They refuse to drink river water.",
        "Their taboo: no river water.",
    ),
    (
        "They leave nothing behind.",
        "Their taboo: leave nothing behind.",
    ),
];

/// The supply the situation text names, if any. The engine's situation speaks of the top
/// pressure only, and of the supplies only food and water have a pressure line.
pub fn situation_supply_key(pressures: &[String]) -> Option<&'static str> {
    match pressures.first().map(String::as_str) {
        Some("FOOD_GONE" | "FOOD_CRITICAL") => Some("food"),
        Some("WATER_GONE" | "WATER_CRITICAL") => Some("water"),
        _ => None,
    }
}

/// How many of the party are left, in words with no number the narrator would have to avoid.
/// `None` when all are alive (the engine says nothing then either).
pub fn party_loss_phrase(alive: usize, total: usize) -> Option<&'static str> {
    if alive >= total {
        None
    } else if alive == 0 {
        Some("None of the party is left.")
    } else if alive == 1 {
        Some("Only one of the party is left.")
    } else if total - alive == 1 {
        Some("One of the party is lost.")
    } else {
        Some("Several of the party are lost.")
    }
}

/// The situation text as the narrator is given it.
///
/// The narrator may not state a number, so it is not handed one: the engine's opening
/// "Day N, <time of day>." is dropped (the journey band and the time of day are facts of their
/// own), and "N of M party members alive." becomes a phrase from [`party_loss_phrase`]. The
/// doctrine and taboo sentences are relabelled (see [`SITUATION_RELABELS`]).
pub fn situation_for_narrator(situation: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, sentence) in situation.split(". ").enumerate() {
        let sentence = sentence.trim().trim_end_matches('.');
        if sentence.is_empty() {
            continue;
        }
        if i == 0 && day_and_time(sentence) {
            continue;
        }
        if let Some((alive, total)) = alive_of_total(sentence) {
            if let Some(phrase) = party_loss_phrase(alive, total) {
                parts.push(phrase.to_string());
            }
            continue;
        }
        parts.push(format!("{sentence}."));
    }
    let mut out = parts.join(" ");
    for (from, to) in SITUATION_RELABELS {
        out = out.replace(from, to);
    }
    out
}

/// `true` for the engine's opening "Day N, <time of day>" sentence.
fn day_and_time(sentence: &str) -> bool {
    let Some(rest) = sentence.strip_prefix("Day ") else {
        return false;
    };
    let Some((day, _time)) = rest.split_once(", ") else {
        return false;
    };
    !day.is_empty() && day.chars().all(|c| c.is_ascii_digit())
}

/// Reads the engine's "N of M party members alive" sentence.
fn alive_of_total(sentence: &str) -> Option<(usize, usize)> {
    let rest = sentence.strip_suffix(" party members alive")?;
    let (alive, total) = rest.split_once(" of ")?;
    Some((alive.parse().ok()?, total.parse().ok()?))
}

/// Which of a turn's two calls a prompt is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSlot {
    /// The scene: where the party is and what it faces.
    Scene,
    /// The outcome: the scene's facts, plus what resulted.
    Outcome,
}

/// The word for one applied change in `resource_directions`.
///
/// Supplies are `spent` or `gained`. Health, morale, the wagon and the animals `fell` or `rose`.
/// Time is `days passed` (or `time saved`). Distance is progress on every turn: a positive
/// change is miles covered (`travelled`) and a negative one is `lost ground`.
pub fn change_word(key: &str, delta: i32) -> &'static str {
    let up = delta > 0;
    match key {
        "distance" => {
            if up {
                "travelled"
            } else {
                "lost ground"
            }
        }
        "time_days" => {
            if up {
                "days passed"
            } else {
                "time saved"
            }
        }
        "health" | "morale" | "wagon_condition" | "animals_health" => {
            if up {
                "rose"
            } else {
                "fell"
            }
        }
        _ => {
            if up {
                "gained"
            } else {
                "spent"
            }
        }
    }
}

fn or_none(text: &str) -> &str {
    if text.trim().is_empty() {
        "none"
    } else {
        text
    }
}

/// The turn's facts for the outcome call: everything the scene call gets, plus what resulted.
/// Same as [`format_turn_facts_for_slot`] with [`PromptSlot::Outcome`].
pub fn format_turn_facts_for_prompt(ctx: &NarrationContext) -> String {
    format_turn_facts_for_slot(ctx, PromptSlot::Outcome)
}

/// Format the turn's facts for one call, strictly from [`NarrationContext`].
///
/// Both calls are told what is happening: the event's title and its own text, the choices on
/// offer or the choice taken, the camp action, the place, the journey, the clock and the party's
/// condition. Only the outcome call is also told what resulted: which resources moved, the
/// health effects, and whether there was a death, an injury or a supply running out.
///
/// Never included: `fallback_line`, `shown_lines`, party member names, any number.
pub fn format_turn_facts_for_slot(ctx: &NarrationContext, slot: PromptSlot) -> String {
    let offered = ctx.offered_labels().join(" | ");
    let mut out = format!(
        "TURN FACTS:\n\
- event_id: {}\n\
- event_title: {}\n\
- event_text: {}\n\
- choices_offered: {}\n\
- choice_id: {}\n\
- choice_taken: {}\n\
- action: {}\n\
- place: {}\n\
- journey_band: {}\n\
- biome: {}\n\
- time_of_day: {}\n\
- phase: {}\n\
- ending: {}\n\
- weirdness_allowance: {}\n\
- pressures: {}\n\
- themes: {}\n\
- situation: {}\n\
- party_health_band: {}\n\
- party_morale_band: {}\n\
- arrived_at_landmark: {}\n\
- night_at_camp: {}",
        ctx.event_id(),
        or_none(ctx.event_title()),
        or_none(ctx.check_event_seed()),
        or_none(&offered),
        ctx.choice_id(),
        or_none(ctx.check_choice_label()),
        or_none(ctx.action()),
        ctx.place(),
        ctx.band(),
        ctx.biome(),
        ctx.time_of_day().as_str(),
        ctx.phase().as_str(),
        or_none(ctx.ending()),
        ctx.weirdness_allowance(),
        ctx.pressures().join(", "),
        ctx.themes().join(", "),
        ctx.situation(),
        ctx.party_status().health_band(),
        ctx.party_status().morale_band(),
        ctx.flags().landmark_arrival,
        ctx.flags().night_at_camp,
    );
    if slot == PromptSlot::Scene {
        return out;
    }

    let mut delta_directions = Vec::new();
    for (k, &v) in ctx.shown_deltas() {
        if v != 0 {
            delta_directions.push(format!("{k}:{}", change_word(k, v)));
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
    out.push_str(&format!(
        "\nWHAT RESULTED:\n\
- resource_directions: {}\n\
- health_effects: {}\n\
- turn_matters: death={}, injury={}, supply_zero={}, landmark_arrival={}, night_at_camp={}",
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
    ));
    out
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
    build_system_prompt_with_rules(profile, length, ctx, PromptRules::Shipped)
}

/// [`build_system_prompt`] with an explicit [`PromptRules`] wording (the outcome call's prompt).
pub fn build_system_prompt_with_rules(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
    rules: PromptRules,
) -> String {
    build_system_prompt_for_slot(profile, length, ctx, rules, PromptSlot::Outcome)
}

/// Whether this turn's narration may state a death: a member died on the turn, or the run ended
/// lost or pyrrhic. On every other turn, and on a triumphant or weathered ending, it may not.
pub fn death_may_be_stated(ctx: &NarrationContext) -> bool {
    ctx.flags().death || matches!(ctx.ending(), "lost" | "pyrrhic")
}

/// The system prompt for one of the turn's two calls.
pub fn build_system_prompt_for_slot(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
    rules: PromptRules,
    slot: PromptSlot,
) -> String {
    let band = weirdness_band_from_allowance(ctx.weirdness_allowance());
    let death_allowed = death_may_be_stated(ctx);
    let rules = match rules {
        PromptRules::Shipped => shared_voice_hard_rules_for(length, band, death_allowed),
        PromptRules::Positive => shared_voice_rules_positive_for(length, band, death_allowed),
    };
    let header = profile_header(profile);
    let facts = format_turn_facts_for_slot(ctx, slot);
    format!("{header}\n\n{rules}\n\n{facts}")
}

/// The legacy scene answer shape (`src/escape_the_valley/gm.py:70-80`). No longer sent in the
/// prompt; kept because [`parse_scene_response_strict`] still reads answers in this shape.
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

/// The legacy outcome answer shape (`src/escape_the_valley/gm.py:82-88`). No longer sent in the
/// prompt; kept because [`parse_outcome_response_strict`] still reads answers in this shape.
pub const OUTCOME_SCHEMA: &str = r#"{
  "scene_id": "string",
  "outcome_title": "string (2-4 words)",
  "outcome_narration": "string (vivid outcome description following all hard rules)",
  "callout": "string (1 punchy sentence summarizing what changed)",
  "oregon_nod": "string (optional 1-line wry trail observation, or empty)",
  "memory_proposals": []
}"#;

/// Sampling temperature sent with every narrator call. Low, so a telling is as repeatable as
/// the model allows.
pub const NARRATOR_TEMPERATURE: f64 = 0.15;

/// Context window asked for on every narrator call, in tokens. A narrator call is one
/// stateless request (no history is carried between calls), so a small window is enough.
pub const NARRATOR_NUM_CTX: u32 = 2048;

/// Most tokens a narrator answer may run to with the plain schema, at Brief and Standard.
pub const NARRATOR_NUM_PREDICT: u32 = 110;

/// The same at Rich. A 70-word answer with its JSON wrapper runs past 110 tokens, and an
/// answer cut off by the budget is unreadable JSON, so Rich gets a budget that covers its cap.
pub const NARRATOR_NUM_PREDICT_RICH: u32 = 150;

/// Extra tokens the cited-facts schema gets, for the list of fact keys.
pub const NARRATOR_NUM_PREDICT_CITED_EXTRA: u32 = 50;

/// Most tokens a narrator answer may run to with the cited-facts schema, which also has to
/// list fact keys, at Brief and Standard.
pub const NARRATOR_NUM_PREDICT_CITED: u32 = 160;

/// Failed calls in a row (timeouts, connection failures, server errors) after which the
/// narrator is paused for the rest of the session.
pub const TIMEOUT_BREAKER_STRIKES: usize = 2;

/// Most bytes read from one narrator server answer. A narrator line is a few hundred bytes; a
/// model list a few kilobytes. A longer body is cut off and fails as unreadable.
pub const NARRATOR_MAX_BODY_BYTES: u64 = 1_000_000;

/// Most slot records kept between two reads by the measurement harness. Recording is off in
/// the game; the harness reads them after every turn, so the cap is never reached there.
pub const MAX_SLOT_RECORDS: usize = 64;

/// Which answer shape the request's `format` schema asks for.
///
/// The game always sends `Plain`. `CitedFacts` exists for the measurement harness only: it asks
/// the model to list the turn-fact keys it used before it writes the sentences, so the two
/// shapes can be compared on the same turns. The listed keys are not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatSchema {
    #[default]
    Plain,
    #[serde(rename = "cited")]
    CitedFacts,
    /// No schema at all: the request carries no `format` field and asks for the sentences
    /// as plain text; the answer is checked as it stands. Measurement variant, to see
    /// whether binding the answer to a schema costs anything.
    #[serde(rename = "text")]
    PlainText,
}

impl FormatSchema {
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatSchema::Plain => "plain",
            FormatSchema::CitedFacts => "cited",
            FormatSchema::PlainText => "text",
        }
    }

    /// The `num_predict` cap sent with this shape at `length`.
    pub fn num_predict(&self, length: NarratorLength) -> u32 {
        let base = match length {
            NarratorLength::Rich => NARRATOR_NUM_PREDICT_RICH,
            NarratorLength::Brief | NarratorLength::Standard => NARRATOR_NUM_PREDICT,
        };
        match self {
            FormatSchema::Plain | FormatSchema::PlainText => base,
            FormatSchema::CitedFacts => base + NARRATOR_NUM_PREDICT_CITED_EXTRA,
        }
    }
}

/// The cited-facts variant of [`narration_format_schema`]: a `cited_fact_keys` string array
/// that comes before the `sentences` array. Measurement variant; see [`FormatSchema`].
pub fn narration_format_schema_cited(_length: NarratorLength) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "cited_fact_keys": {
                "type": "array",
                "items": { "type": "string" }
            },
            "sentences": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": NARRATION_MAX_SENTENCE_ITEMS
            }
        },
        "required": ["cited_fact_keys", "sentences"],
        "additionalProperties": false
    })
}

/// The schema for `schema` and `length`, or `None` for the plain-text variant, which sends no
/// `format` field.
pub fn narration_format_schema_for(schema: FormatSchema, length: NarratorLength) -> Option<Value> {
    match schema {
        FormatSchema::Plain => Some(narration_format_schema(length)),
        FormatSchema::CitedFacts => Some(narration_format_schema_cited(length)),
        FormatSchema::PlainText => None,
    }
}

/// The JSON Schema sent in the request's `format` field: an object holding one array of
/// sentence strings. The length setting caps the words (see [`NarratorLength::word_cap`]); a
/// schema cannot count words, so the cap is stated in the prompt and enforced by the checker.
/// A server that supports structured output can then only return that shape.
pub fn narration_format_schema(_length: NarratorLength) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "sentences": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": NARRATION_MAX_SENTENCE_ITEMS
            }
        },
        "required": ["sentences"],
        "additionalProperties": false
    })
}

/// The response shape as the user prompt shows it.
fn sentences_shape_text(length: NarratorLength, schema: FormatSchema) -> String {
    let cap = length.word_cap();
    if schema == FormatSchema::CitedFacts {
        return format!(
            "{{\"cited_fact_keys\": [\"...\"], \"sentences\": [\"...\"]}}\nFirst list in cited_fact_keys the names of the turn facts the sentences rest on. Then the sentences array holds complete sentences, one per item, at most {cap} words in all, each following all the rules."
        );
    }
    format!(
        "{{\"sentences\": [\"...\"]}}\nThe sentences array holds complete sentences, one per item, at most {cap} words in all, each following all the rules."
    )
}

/// The sampling seed sent with one narrator call: a number derived from the run's seed, the
/// turn's index (the run's step counter after it) and the turn's own facts (day, time of day, game
/// phase, action, event, choice, place and shown changes), the profile, the length and the slot. The same run,
/// turn and settings always give the same seed, so a replay asks for the same samples; two turns
/// with the same facts but different places in the run do not. It never touches the engine's
/// RNG. The value fits in 31 bits.
pub fn narration_call_seed(
    ctx: &NarrationContext,
    profile: GMProfile,
    length: NarratorLength,
    is_scene: bool,
) -> u32 {
    let mut deltas = String::new();
    for (k, v) in ctx.shown_deltas() {
        deltas.push_str(&format!("{k}={v};"));
    }
    let material = format!(
        "etv-narration-seed-2|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ctx.run_seed(),
        ctx.turn_index(),
        ctx.day(),
        ctx.time_of_day().as_str(),
        ctx.phase().as_str(),
        ctx.action(),
        ctx.event_id(),
        ctx.choice_id(),
        ctx.place(),
        deltas,
        profile.as_str(),
        length.as_str(),
        if is_scene { "scene" } else { "outcome" }
    );
    let digest: [u8; 32] = Sha256::digest(material.as_bytes()).into();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) & 0x7fff_ffff
}

/// Build `(system_prompt, user_prompt)` for the per-turn scene call.
pub fn build_scene_prompts(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
) -> (String, String) {
    build_scene_prompts_with_rules(profile, length, ctx, PromptRules::Shipped)
}

/// [`build_scene_prompts`] with an explicit [`PromptRules`] wording.
pub fn build_scene_prompts_with_rules(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
    rules: PromptRules,
) -> (String, String) {
    build_scene_prompts_for(profile, length, ctx, rules, FormatSchema::Plain)
}

/// [`build_scene_prompts`] with an explicit rules wording and answer shape.
pub fn build_scene_prompts_for(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
    rules: PromptRules,
    schema: FormatSchema,
) -> (String, String) {
    let sys = build_system_prompt_for_slot(profile, length, ctx, rules, PromptSlot::Scene);
    if schema == FormatSchema::PlainText {
        let cap = length.word_cap();
        let user = format!(
            "Generate a scene response as plain text: complete sentences, at most {cap} words in all, each following all the rules. Answer with the sentences only."
        );
        return (sys, user);
    }
    let user = format!(
        "Generate a scene response strictly as valid JSON matching SCENE_SCHEMA:\n{}",
        sentences_shape_text(length, schema)
    );
    (sys, user)
}

/// Build `(system_prompt, user_prompt)` for the per-turn outcome call.
pub fn build_outcome_prompts(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
) -> (String, String) {
    build_outcome_prompts_with_rules(profile, length, ctx, PromptRules::Shipped)
}

/// [`build_outcome_prompts`] with an explicit [`PromptRules`] wording.
pub fn build_outcome_prompts_with_rules(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
    rules: PromptRules,
) -> (String, String) {
    build_outcome_prompts_for(profile, length, ctx, rules, FormatSchema::Plain)
}

/// [`build_outcome_prompts`] with an explicit rules wording and answer shape.
pub fn build_outcome_prompts_for(
    profile: GMProfile,
    length: NarratorLength,
    ctx: &NarrationContext,
    rules: PromptRules,
    schema: FormatSchema,
) -> (String, String) {
    let sys = build_system_prompt_for_slot(profile, length, ctx, rules, PromptSlot::Outcome);
    if schema == FormatSchema::PlainText {
        let cap = length.word_cap();
        let user = format!(
            "Generate an outcome response as plain text: complete sentences, at most {cap} words in all, each following all the rules. Answer with the sentences only."
        );
        return (sys, user);
    }
    let user = format!(
        "Generate an outcome response strictly as valid JSON matching OUTCOME_SCHEMA:\n{}",
        sentences_shape_text(length, schema)
    );
    (sys, user)
}

/// Read the prose of one narrator answer.
///
/// The requested shape is `{"sentences": ["...", "..."]}`: every item must be a non-empty string,
/// and the sentences are joined with single spaces. An answer in the older shape (a `narration`
/// string for a scene, an `outcome_narration` string for an outcome) is still read, through the
/// strict parsers. Anything else is an error, and the turn falls back.
pub fn parse_slot_prose(raw: &str, is_scene: bool) -> Result<String, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err("response is not a raw JSON object".to_string());
    }
    let val: Value = serde_json::from_str(trimmed).map_err(|e| format!("invalid JSON: {e}"))?;
    if let Some(sentences) = val.get("sentences") {
        let items = sentences
            .as_array()
            .ok_or_else(|| "'sentences' is not an array".to_string())?;
        if items.is_empty() {
            return Err("'sentences' is empty".to_string());
        }
        let mut parts = Vec::with_capacity(items.len());
        for item in items {
            let text = item
                .as_str()
                .ok_or_else(|| "'sentences' holds a non-string item".to_string())?
                .trim();
            if text.is_empty() {
                return Err("'sentences' holds an empty sentence".to_string());
            }
            parts.push(text);
        }
        return Ok(parts.join(" "));
    }
    if is_scene {
        parse_scene_response_strict(trimmed).map(|scene| scene.narration)
    } else {
        parse_outcome_response_strict(trimmed).map(|outcome| outcome.outcome_narration)
    }
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

/// Counting words of two or more (plus zero). Banned wherever they appear.
const BANNED_NUMBER_WORDS: &[&str] = &[
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
];

/// Measurement, duration, distance and count units. A unit word is rejected only when a number
/// or quantity word comes straight before it ("several days", "one step", "a mile"). On its
/// own it is an ordinary word ("figures step from the brush", "a turn in the road").
const UNIT_WORDS: &[&str] = &[
    "day", "days", "mile", "miles", "hour", "hours", "minute", "minutes", "second", "seconds",
    "week", "weeks", "month", "months", "year", "years", "pound", "pounds", "ounce", "ounces",
    "gallon", "gallons", "quart", "quarts", "pint", "pints", "inch", "inches", "foot", "feet",
    "yard", "yards", "meter", "meters", "ration", "rations", "coin", "coins", "dollar", "dollars",
    "percent", "degree", "degrees", "step", "steps", "turn", "turns",
];

/// Quantity words that are not numbers of two or more. One of these straight before a unit
/// word states a measurement.
const QUANTITY_WORDS: &[&str] = &[
    "one",
    "single",
    "several",
    "many",
    "few",
    "some",
    "half",
    "both",
    "pair",
    "countless",
];

/// Unit words that are also everyday nouns and verbs. "A step" or "a turn" is not a
/// measurement; "a mile" or "an hour" is.
const EVERYDAY_UNIT_WORDS: &[&str] = &[
    "step", "steps", "turn", "turns", "foot", "feet", "second", "seconds", "yard", "yards", "coin",
    "coins", "ration", "rations", "degree", "degrees", "pound", "pounds",
];

/// Death words that may be used, plainly, on a turn where a death may be stated. The rest of
/// [`BANNED_DEATH_WORDS`] (the violent ones, and words of ruin) stay banned on every turn.
const PLAIN_DEATH_WORDS: &[&str] = &[
    "dead", "died", "dies", "dying", "death", "deaths", "grave", "graves", "buried", "burial",
    "perish", "perished", "perishes",
];

/// Gore. Checked on a turn where a death may be stated: the death is told without it.
const GORE_WORDS: &[&str] = &[
    "blood",
    "bloody",
    "bloodied",
    "bleeding",
    "gore",
    "gory",
    "guts",
    "entrails",
    "mangled",
    "severed",
    "rotting",
    "rotted",
    "maggots",
    "carcass",
    "carcasses",
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
    /// "Says what happened" rules (off in the game; see `narrator_truth`). The line says a
    /// supply is gone that is not at zero, or lost that was gained.
    SupplyStateContradiction,
    /// The run ended on this turn and the outcome line does not say so.
    EndingNotConveyed,
    /// A traveller died on this turn and the outcome line does not say so.
    DeathNotConveyed,
    /// The party arrived on this turn and the outcome line does not say so.
    ArrivalNotConveyed,
    /// No one is left alive and the line shows the party acting.
    ActionWithNoOneLeft,
    /// The line opens with the event's own text.
    EventTextCopied,
    /// The outcome line opens with the same two words as the previous accepted line.
    RepeatedOpening,
    /// The run ended pyrrhic and the outcome line carries no word of what it cost (the
    /// `pyrrhic-loss-word` arm).
    PyrrhicLossNotConveyed,
    /// On a choice turn the outcome line carries neither the choice's verb nor a supply the
    /// choice changed (the `choice-conveyed` arm).
    ChoiceNotConveyed,
    /// The outcome line adds too few words of its own to the lead it was given (the `lead-guard`
    /// arm: one at Brief, two at Standard and Rich).
    LeadOnly,
    /// The line names a time of day that contradicts the turn's (the `lead-guard` arm).
    TimeOfDayContradiction,
    /// The line names a thing the trail's world does not have: tires, engines, phones (the
    /// `no-anachronism` arm).
    Anachronism,
    /// The line shows the party doing what its taboo forbids, or going against its doctrine
    /// (the `taboo-rules` arm).
    TabooBroken,
    /// The line states the party's taboo as filler on a turn that did not involve it (the
    /// `taboo-rules` arm).
    TabooAsFiller,
    /// The line names a water source (river, stream, lake) the place, the biome and the turn's
    /// facts do not provide (the `no-water-source` arm).
    InventedWaterSource,
}

impl CheckReasonCode {
    /// Every reason code, in declaration order.
    pub const ALL: [CheckReasonCode; 27] = [
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
        CheckReasonCode::SupplyStateContradiction,
        CheckReasonCode::EndingNotConveyed,
        CheckReasonCode::DeathNotConveyed,
        CheckReasonCode::ArrivalNotConveyed,
        CheckReasonCode::ActionWithNoOneLeft,
        CheckReasonCode::EventTextCopied,
        CheckReasonCode::RepeatedOpening,
        CheckReasonCode::PyrrhicLossNotConveyed,
        CheckReasonCode::ChoiceNotConveyed,
        CheckReasonCode::LeadOnly,
        CheckReasonCode::TimeOfDayContradiction,
        CheckReasonCode::Anachronism,
        CheckReasonCode::TabooBroken,
        CheckReasonCode::TabooAsFiller,
        CheckReasonCode::InventedWaterSource,
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
            Self::SupplyStateContradiction => "supply_state_contradiction",
            Self::EndingNotConveyed => "ending_not_conveyed",
            Self::DeathNotConveyed => "death_not_conveyed",
            Self::ArrivalNotConveyed => "arrival_not_conveyed",
            Self::ActionWithNoOneLeft => "action_with_no_one_left",
            Self::EventTextCopied => "event_text_copied",
            Self::RepeatedOpening => "repeated_opening",
            Self::PyrrhicLossNotConveyed => "pyrrhic_loss_not_conveyed",
            Self::ChoiceNotConveyed => "choice_not_conveyed",
            Self::LeadOnly => "lead_only",
            Self::TimeOfDayContradiction => "time_of_day_contradiction",
            Self::Anachronism => "anachronism",
            Self::TabooBroken => "taboo_broken",
            Self::TabooAsFiller => "taboo_as_filler",
            Self::InventedWaterSource => "invented_water_source",
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
    /// `true` on a turn where a death may be stated (see [`death_may_be_stated`]). The plain
    /// death words are then allowed; violent and gory words are not.
    #[serde(default)]
    pub death_allowed: bool,
    /// Draft-animal species words this voice has been told the team is, and so may use. Empty
    /// in the game; set only by an experiment arm (see `narrator_experiment`).
    #[serde(default)]
    pub team_species: Vec<String>,
    /// The turn's facts for the "says what happened" rules. `None` in the game, where those
    /// rules do not run; set only by an experiment arm.
    #[serde(default)]
    pub truth: Option<crate::narrator_truth::TruthFacts>,
    /// Choice parroting as changed by the `parrot-verbatim` arm: only a copy of the whole
    /// label (two words or more) in a row, or of [`PARROT_RUN_WORDS`] or more of its words in a
    /// row, is rejected. `false` in the game.
    #[serde(default)]
    pub parrot_verbatim: bool,
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
            death_allowed: false,
            team_species: Vec::new(),
            truth: None,
            parrot_verbatim: false,
        }
    }
}

impl CheckerContext {
    /// Build a [`CheckerContext`] from a [`NarrationContext`] and [`NarratorLength`].
    pub fn from_narration_context(ctx: &NarrationContext, length: NarratorLength) -> Self {
        let mut allowed_resources: Vec<String> = ctx
            .shown_deltas()
            .iter()
            .filter(|(_, &delta)| delta != 0)
            .map(|(k, _)| k.clone())
            .collect();
        // A supply the situation text names ("They have no food.") may be mentioned too.
        if let Some(key) = situation_supply_key(ctx.pressures()) {
            if !allowed_resources.iter().any(|k| k == key) {
                allowed_resources.push(key.to_string());
            }
        }
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
            death_allowed: death_may_be_stated(ctx),
            team_species: Vec::new(),
            truth: None,
            parrot_verbatim: false,
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

/// With the `parrot-verbatim` arm, this many of the label's words in a row are a copy even
/// when the label is longer.
pub const PARROT_RUN_WORDS: usize = 5;

/// The words of `label` that `line_tokens` copies in a row, when it copies the whole label (of
/// two words or more) or [`PARROT_RUN_WORDS`] or more of its words in a row. Words are compared
/// lowercased and trimmed of punctuation, exactly (no stemming). A one-word label is never a
/// copy: naming what the party did passes.
pub fn verbatim_label_copy(line_tokens: &[String], label: &str) -> Option<String> {
    let label_tokens: Vec<String> = label
        .split_whitespace()
        .map(clean_word_token)
        .filter(|w| !w.is_empty())
        .collect();
    let line_tokens: Vec<String> = line_tokens
        .iter()
        .map(|w| clean_word_token(w))
        .filter(|w| !w.is_empty())
        .collect();
    let run = label_tokens.len().min(PARROT_RUN_WORDS);
    if run < 2 || line_tokens.len() < run {
        return None;
    }
    for start in 0..=(label_tokens.len() - run) {
        let want = &label_tokens[start..start + run];
        if line_tokens.windows(run).any(|w| w == want) {
            return Some(want.join(" "));
        }
    }
    None
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

pub(crate) fn lookup_resource_key_for_noun(word_lower: &str) -> Option<&'static str> {
    for &(res_key, nouns) in RESOURCE_NOUN_TABLE {
        if nouns.contains(&word_lower) {
            return Some(res_key);
        }
    }
    None
}

/// A model often writes the typographic apostrophe (U+2019, U+2018) where the plain one is
/// meant. The plain apostrophe is allowed punctuation, so the line is read with plain ones.
pub fn plain_apostrophes(text: &str) -> String {
    text.replace(['\u{2019}', '\u{2018}'], "'")
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
    match run_checks(line, ctx, false).into_iter().next() {
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

/// Every rule `line` breaks, at most one failure per rule, in the order the rules are checked.
/// The first entry is what [`check_generated_line`] returns. For measurement: it shows how
/// often a rule is broken at all, where the first-failure count hides every rule after the
/// first one a line breaks. The game does not call this.
pub fn check_generated_line_all(line: &str, ctx: &CheckerContext) -> Vec<CheckFailure> {
    let mut seen: Vec<CheckReasonCode> = Vec::new();
    let mut out = Vec::new();
    for failure in run_checks(line, ctx, true) {
        if !seen.contains(&failure.code) {
            seen.push(failure.code);
            out.push(failure);
        }
    }
    out
}

/// The rules, in order. With `all` false it stops at the first failure; with `all` true it
/// carries on and returns every failure it meets.
fn run_checks(line: &str, ctx: &CheckerContext, all: bool) -> Vec<CheckFailure> {
    let mut out: Vec<CheckFailure> = Vec::new();
    macro_rules! fail {
        ($failure:expr) => {{
            out.push($failure);
            if !all {
                return out;
            }
        }};
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        // Nothing else can be said about an empty line.
        out.push(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "line is empty".to_string(),
        });
        return out;
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
        fail!(CheckFailure {
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
            fail!(CheckFailure {
                code: CheckReasonCode::InvalidPunctuation,
                detail: format!("disallowed character {ch:?}"),
            });
        }
    }

    // 2. Whole sentences, and the word cap of the length setting (Brief 25, Standard 45, Rich 70).
    let raw_sentences: Vec<&str> = trimmed.split('.').collect();
    // Since trimmed ends with '.', the last element after split('.') must be empty, and all prior
    // elements must be non-empty sentences.
    if raw_sentences.last().map(|s| !s.is_empty()).unwrap_or(true) {
        fail!(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "trailing text after final period".to_string(),
        });
    }
    let sentences: Vec<&str> = raw_sentences[..raw_sentences.len() - 1]
        .iter()
        .map(|s| s.trim())
        .collect();
    if sentences.is_empty() || sentences.iter().any(|s| s.is_empty()) {
        fail!(CheckFailure {
            code: CheckReasonCode::InvalidPunctuation,
            detail: "empty sentence segment".to_string(),
        });
    }
    let cap = ctx.length.word_cap();
    let words = trimmed.split_whitespace().count();
    if words > cap {
        fail!(CheckFailure {
            code: CheckReasonCode::LengthCapExceeded,
            detail: format!(
                "expected at most {cap} words for {:?}, found {words}",
                ctx.length
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
            fail!(CheckFailure {
                code: CheckReasonCode::QuantifierBeforeResourceOrHalf,
                detail: "banned quantifier 'half'".to_string(),
            });
        }
        if word == "one" || word == "both" || word == "pair" {
            if let Some(next_word) = tokens_lower.get(idx + 1) {
                if lookup_resource_key_for_noun(next_word).is_some() {
                    fail!(CheckFailure {
                        code: CheckReasonCode::QuantifierBeforeResourceOrHalf,
                        detail: format!(
                            "quantifier '{word}' directly before resource '{next_word}'"
                        ),
                    });
                }
                if next_word == "of" {
                    if let Some(third_word) = tokens_lower.get(idx + 2) {
                        if lookup_resource_key_for_noun(third_word).is_some() {
                            fail!(CheckFailure {
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

    // 4. NumbersDigitsOrUnits: ASCII digits, counting words >= 2 (or zero), and a unit word
    // that has a number or quantity word straight before it.
    if trimmed.chars().any(|c| c.is_ascii_digit()) {
        fail!(CheckFailure {
            code: CheckReasonCode::NumbersDigitsOrUnits,
            detail: "contains ASCII digit".to_string(),
        });
    }
    for (idx, word) in tokens_lower.iter().enumerate() {
        if BANNED_NUMBER_WORDS.contains(&word.as_str()) {
            fail!(CheckFailure {
                code: CheckReasonCode::NumbersDigitsOrUnits,
                detail: format!("banned number word '{word}'"),
            });
        }
        if UNIT_WORDS.contains(&word.as_str()) && idx > 0 {
            let prev = tokens_lower[idx - 1].as_str();
            let article =
                (prev == "a" || prev == "an") && !EVERYDAY_UNIT_WORDS.contains(&word.as_str());
            if article || QUANTITY_WORDS.contains(&prev) {
                fail!(CheckFailure {
                    code: CheckReasonCode::NumbersDigitsOrUnits,
                    detail: format!("unit word '{word}' after quantity word '{prev}'"),
                });
            }
        }
    }

    // 5. DeathWords. On a turn where a death may be stated, the plain words pass and gore does
    // not; on every other turn the whole list is banned.
    for word in &tokens_lower {
        if BANNED_DEATH_WORDS.contains(&word.as_str())
            && !(ctx.death_allowed && PLAIN_DEATH_WORDS.contains(&word.as_str()))
        {
            fail!(CheckFailure {
                code: CheckReasonCode::DeathWords,
                detail: format!("banned death word '{word}'"),
            });
        }
        if ctx.death_allowed && GORE_WORDS.contains(&word.as_str()) {
            fail!(CheckFailure {
                code: CheckReasonCode::DeathWords,
                detail: format!("gore word '{word}' on a death turn"),
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
                    fail!(CheckFailure {
                        code: CheckReasonCode::MidSentenceCapital,
                        detail: format!("unexpected uppercase inside first word '{raw_token}'"),
                    });
                }
            } else if clean_chars.iter().any(|c| c.is_ascii_uppercase()) {
                fail!(CheckFailure {
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
    // named by the event's seed or label). Where in the line they appear does not matter.
    for sentence in sentences.iter() {
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
                    fail!(CheckFailure {
                        code: CheckReasonCode::UnauthorisedResourceNoun,
                        detail: format!(
                            "resource noun '{word}' (key '{res_key}') not in allowed_resources {:?}",
                            ctx.allowed_resources
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
            fail!(CheckFailure {
                code: CheckReasonCode::BandBannedVocabulary,
                detail: format!("word '{banned}' is banned across all bands"),
            });
        }
    }
    if ctx.weirdness_band <= 2 {
        for &banned in BANNED_WORDS_BAND_2_AND_BELOW {
            if contains_phrase_or_word(&line_lower, &tokens_lower, banned) {
                fail!(CheckFailure {
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
                fail!(CheckFailure {
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
                fail!(CheckFailure {
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
            fail!(CheckFailure {
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
                fail!(CheckFailure {
                    code: CheckReasonCode::NamedPartyMember,
                    detail: format!("party member name '{cleaned}'"),
                });
            }
        }
    }

    // 11. DraftAnimalSpecies, unless the event's own seed or label names it.
    for word in &tokens_lower {
        if DRAFT_ANIMAL_SPECIES_WORDS.contains(&word.as_str())
            && !event_named.contains(word)
            && !ctx.team_species.contains(word)
        {
            fail!(CheckFailure {
                code: CheckReasonCode::DraftAnimalSpecies,
                detail: format!("draft-animal species '{word}' not named by the event"),
            });
        }
    }

    // 12. ChoiceParroting: the label's first content word (its verb) and at least one of its
    // other content words (a noun) both reappear in the line, compared by crude stem.
    let label_words = label_content_words(&ctx.choice_label);
    if ctx.parrot_verbatim {
        if let Some(copied) = verbatim_label_copy(&tokens_lower, &ctx.choice_label) {
            fail!(CheckFailure {
                code: CheckReasonCode::ChoiceParroting,
                detail: format!("line copies the choice label's words '{copied}'"),
            });
        }
    } else if label_words.len() >= 2 {
        let line_stems: Vec<String> = tokens_lower.iter().map(|w| crude_stem(w)).collect();
        let verb_stem = crude_stem(&label_words[0]);
        if line_stems.contains(&verb_stem) {
            for noun in &label_words[1..] {
                if line_stems.contains(&crude_stem(noun)) {
                    fail!(CheckFailure {
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

    // 13 on. "Says what happened", event copying and the repeated opening: only when the
    // turn's facts were given (an experiment arm; never in the game).
    if let Some(facts) = &ctx.truth {
        for failure in crate::narrator_truth::truth_failures(trimmed, facts) {
            fail!(failure);
        }
    }
    out
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

    /// Return all `(key, dir, value)` entries whose value contains `"PLACEHOLDER"` anywhere
    /// (`"PLACEHOLDER-food-neg"`, and also `"uses PLACEHOLDER food"`).
    pub fn placeholder_entries(&self) -> Vec<(String, String, String)> {
        let mut found = Vec::new();
        for (key, dirs) in &self.phrases {
            for (dir, val) in dirs {
                if val.contains("PLACEHOLDER") {
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

    /// Returns `true` if the subject or any entry in the table contains `"PLACEHOLDER"`.
    pub fn has_placeholders(&self) -> bool {
        self.subject.contains("PLACEHOLDER") || !self.placeholder_entries().is_empty()
    }

    /// The phrase for `key` in direction `dir`, or `None` when the table has none, or holds only
    /// a stand-in for it: text containing `"PLACEHOLDER"`, or nothing but whitespace.
    pub fn phrase(&self, key: &str, dir: &str) -> Option<String> {
        self.phrases
            .get(key)
            .and_then(|dirs| dirs.get(dir))
            .filter(|p| !p.contains("PLACEHOLDER") && !p.trim().is_empty())
            .cloned()
    }

    /// Look up the phrase for `(key, dir)`, falling back to `"PLACEHOLDER-<key>-<dir>"`.
    /// Never used to compose a line (see [`Self::phrase`]).
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
    /// The table has no real `"none"."neutral"` phrase (missing, or only a `PLACEHOLDER-`
    /// stand-in), so a turn with no phrased change would have nothing to say. Refused in every
    /// build: the caller falls back to the engine's plain line.
    NeutralPhraseMissing,
    /// The table's subject (`"The party"`) contains `"PLACEHOLDER"`. Every composed line starts
    /// with it, so it is refused in every build.
    PlaceholderSubject,
}

/// Composes fallback outcome sentences from effect keys and directions using a
/// [`FallbackPhraseTable`].
///
/// In a release build (`cfg(not(debug_assertions))`), [`ComposedFallbackComposer::try_start`]
/// refuses to start if the phrase table is missing or contains any `"PLACEHOLDER-"` entry. In
/// every build it refuses a table without a real `"none"."neutral"` phrase, so a composed line
/// never needs a stand-in.
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
        if tbl.subject.contains("PLACEHOLDER") {
            return Err(FallbackComposerError::PlaceholderSubject);
        }
        // `phrase` also turns away an empty or whitespace-only neutral phrase.
        if tbl.phrase("none", "neutral").is_none() {
            return Err(FallbackComposerError::NeutralPhraseMissing);
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

        // A key the table has no phrase for adds no clause: the composed line never carries a
        // `PLACEHOLDER-` stand-in, in any build.
        let mut clauses = Vec::new();
        let ordered = self.table.ordered_keys();
        for &key in &ordered {
            if let Some(&delta) = normalized.get(key) {
                if delta != 0 {
                    let dir = if delta < 0 { "neg" } else { "pos" };
                    clauses.extend(self.table.phrase(key, dir));
                }
            }
        }
        for (key, &delta) in &normalized {
            if delta != 0 && !ordered.contains(&key.as_str()) {
                let dir = if delta < 0 { "neg" } else { "pos" };
                clauses.extend(self.table.phrase(key, dir));
            }
        }
        if clauses.is_empty() {
            // `try_start_with_mode` refused any table without a real neutral phrase, so this is
            // always the table's own text, never a `PLACEHOLDER-` stand-in.
            clauses.extend(self.table.phrase("none", "neutral"));
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
///
/// The table is checked as a release build checks it ([`ComposedFallbackComposer::try_start_with_mode`]
/// with `is_release_build = true`), whatever this build is: the lines are an audit of what a
/// player could be shown, so a table a release build would refuse gives an error, not lines.
pub fn compose_all_464_choice_fallbacks(
    pack: &ContentPack,
    table: &FallbackPhraseTable,
) -> Result<BTreeMap<(String, String), String>, FallbackComposerError> {
    let composer = ComposedFallbackComposer::try_start_with_mode(Some(table), true)?;
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
    Ok(out)
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
    let bad = || EndpointError::InvalidAuthority(endpoint.to_string());
    // An endpoint is a scheme, a host, an optional numeric port and an optional path. A query, a
    // fragment or userinfo has no place in it, and each could carry anything (a key included).
    if rest.contains(['?', '#']) {
        return Err(bad());
    }
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty()
        || authority.contains('@')
        || authority
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(bad());
    }
    let (host, port) = if authority.starts_with('[') {
        let end_bracket = authority.find(']').ok_or_else(bad)?;
        let inner = &authority[1..end_bracket];
        if inner.is_empty()
            || !inner
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return Err(bad());
        }
        let after = &authority[end_bracket + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return Err(bad()),
        };
        (&authority[..=end_bracket], port)
    } else {
        let (host, port) = match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        };
        if host.is_empty()
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return Err(bad());
        }
        (host, port)
    };
    if let Some(port) = port {
        let digits_only =
            !port.is_empty() && port.len() <= 5 && port.chars().all(|c| c.is_ascii_digit());
        if !digits_only || !(1..=65535).contains(&port.parse::<u32>().unwrap_or(0)) {
            return Err(bad());
        }
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
    // The shape first (scheme, host, numeric port, no userinfo, query or fragment), then where
    // it points.
    let _host = parse_endpoint_host(endpoint)?;
    if !allow_non_loopback && !is_loopback_endpoint(endpoint) {
        return Err(EndpointError::NonLoopbackRefused(endpoint.to_string()));
    }
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
    /// Which service writes the narration: the local Ollama server by default. Written to the
    /// settings file only when it is not the default, so a local player's file keeps its shape.
    #[serde(skip_serializing_if = "crate::narrator_provider::NarratorProvider::is_local")]
    pub provider: crate::narrator_provider::NarratorProvider,
    /// The base URL of an OpenAI-compatible provider (OpenRouter, a LAN server). Unused by the
    /// other providers.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub provider_url: String,
    /// Where a hosted provider's key is read from: an environment variable's name or a key
    /// file's path. The key itself is never stored here.
    #[serde(skip_serializing_if = "crate::narrator_provider::KeySource::is_empty")]
    pub key: crate::narrator_provider::KeySource,
}

/// The model the game narrates with by default: small and local.
pub const DEFAULT_NARRATOR_MODEL: &str = "qwen3:4b-instruct-2507-q4_K_M";

impl Default for NarratorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            profile: GMProfile::Fireside,
            endpoint: DEFAULT_OLLAMA_ENDPOINT.to_string(),
            allow_non_loopback: false,
            model: DEFAULT_NARRATOR_MODEL.to_string(),
            length: NarratorLength::Brief,
            warmup_timeout_ms: 30_000,
            per_call_timeout_ms: 10_000,
            provider: crate::narrator_provider::NarratorProvider::Ollama,
            provider_url: String::new(),
            key: crate::narrator_provider::KeySource::default(),
        }
    }
}

impl NarratorSettings {
    /// These settings with the cloud narrator chosen: Claude is preselected, on its default
    /// model. The voice, the length and the key source are kept.
    pub fn with_cloud(&self) -> Self {
        Self {
            provider: crate::narrator_provider::CLOUD_DEFAULT_PROVIDER,
            model: crate::narrator_provider::CLOUD_DEFAULT_MODEL.to_string(),
            ..self.clone()
        }
    }
}

impl NarratorSettings {
    /// Load and validate [`NarratorSettings`] from a JSON string.
    ///
    /// No error carries text from the file: a settings file can hold a key pasted in the wrong
    /// place, and these messages are shown to the player. A JSON error says only what kind it is
    /// and where; an endpoint error says only what is wrong with it.
    pub fn from_json_str(raw: &str) -> Result<Self, String> {
        let mut settings: Self = serde_json::from_str(raw).map_err(|e| {
            let what = match e.classify() {
                serde_json::error::Category::Data => "a setting has the wrong type or value",
                serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                    "the file is not valid JSON"
                }
                serde_json::error::Category::Io => "the file could not be read",
            };
            format!("{what} (at line {}, column {})", e.line(), e.column())
        })?;
        let normalized = validate_endpoint(&settings.endpoint, settings.allow_non_loopback)
            .map_err(|e| {
                match e {
                    EndpointError::NonLoopbackRefused(_) => {
                        "the endpoint is not on this computer (set allow_non_loopback to use another host)"
                    }
                    EndpointError::InvalidScheme(_) => {
                        "the endpoint must start with http:// or https://"
                    }
                    EndpointError::InvalidAuthority(_) => {
                        "the endpoint is not valid (a host, an optional numeric port and an optional path only; no user name, query or fragment)"
                    }
                }
                .to_string()
            })?;
        settings.endpoint = normalized;
        crate::narrator_provider::validate_provider(&settings)?;
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
            .with_config()
            .limit(NARRATOR_MAX_BODY_BYTES)
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
            .with_config()
            .limit(NARRATOR_MAX_BODY_BYTES)
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
    /// The seed and body of the call in flight, kept only while slots are being recorded.
    last_request: Mutex<Option<(u32, String)>>,
    positive_rules: AtomicBool,
    format_schema: AtomicUsize,
    /// Experiment arms; all off unless the measurement harness sets them.
    experiment: Mutex<crate::narrator_experiment::ExperimentArms>,
    /// The first two words of the run's previous accepted line (for the repeated-opening
    /// arm; kept always, used only when that arm is on).
    previous_opening: Mutex<Option<String>>,
    breaker_enabled: AtomicBool,
    /// Failed calls in a row (timeouts, connection failures, server errors).
    consecutive_timeouts: AtomicUsize,
    /// A call of the current turn timed out: the turn's other calls are skipped.
    turn_timed_out: AtomicBool,
    paused: AtomicBool,
}

/// What one call asks for beyond the prompts: the answer shape and, for experiment arms only,
/// a sentence-item limit, a stop at the first full stop, the model card's sampling settings,
/// and trimming to the word cap.
#[derive(Debug, Clone, Copy)]
struct SlotCall {
    /// The sampling seed sent with the call.
    seed: u32,
    schema: FormatSchema,
    max_items: Option<usize>,
    stop_at_period: bool,
    model_card_sampling: bool,
    trim_to_cap: bool,
    num_gpu: Option<u32>,
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
    /// The sampling seed sent with the call.
    pub seed: Option<u32>,
    /// The request body exactly as it was handed to the transport.
    pub request: Option<String>,
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
            last_request: Mutex::new(None),
            experiment: Mutex::new(crate::narrator_experiment::ExperimentArms::default()),
            previous_opening: Mutex::new(None),
            positive_rules: AtomicBool::new(false),
            format_schema: AtomicUsize::new(0),
            breaker_enabled: AtomicBool::new(true),
            consecutive_timeouts: AtomicUsize::new(0),
            turn_timed_out: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        }
    }

    /// Switch experiment arms on or off. The game never calls this; the measurement harness
    /// does. `candidates` is clamped to `1..=MAX_CANDIDATES`.
    pub fn set_experiment(&self, mut arms: crate::narrator_experiment::ExperimentArms) {
        arms.candidates = arms
            .candidates
            .clamp(1, crate::narrator_experiment::MAX_CANDIDATES);
        if let Ok(mut slot) = self.experiment.lock() {
            *slot = arms;
        }
    }

    /// Forget the previous accepted line. The measurement harness calls this when it moves
    /// from one run's turns to another's, so a line is compared only with its own run.
    pub fn reset_run_memory(&self) {
        if let Ok(mut previous) = self.previous_opening.lock() {
            *previous = None;
        }
    }

    /// The experiment arms in force (all off in the game).
    pub fn experiment(&self) -> crate::narrator_experiment::ExperimentArms {
        self.experiment.lock().map(|a| *a).unwrap_or_default()
    }

    /// Choose the answer shape asked for in `format`. The game never calls this; the
    /// measurement harness does, to compare the two shapes.
    pub fn set_format_schema(&self, schema: FormatSchema) {
        let code = match schema {
            FormatSchema::Plain => 0,
            FormatSchema::CitedFacts => 1,
            FormatSchema::PlainText => 2,
        };
        self.format_schema.store(code, Ordering::SeqCst);
    }

    /// The answer shape this narrator asks for.
    pub fn format_schema(&self) -> FormatSchema {
        match self.format_schema.load(Ordering::SeqCst) {
            1 => FormatSchema::CitedFacts,
            2 => FormatSchema::PlainText,
            _ => FormatSchema::Plain,
        }
    }

    /// Turn the timeout breaker on or off (on by default). With it on,
    /// [`TIMEOUT_BREAKER_STRIKES`] per-call timeouts in a row pause the narrator for the rest
    /// of the session: no further call is made, and every later line is the plain fallback.
    /// The measurement harness turns it off so every turn of a set is asked for.
    pub fn set_timeout_breaker(&self, enabled: bool) {
        self.breaker_enabled.store(enabled, Ordering::SeqCst);
    }

    /// `true` once the timeout breaker has paused this narrator.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Choose the wording of the shared rules in the system prompt. The game never calls
    /// this; the measurement harness does, to compare the two wordings.
    pub fn set_prompt_rules(&self, rules: PromptRules) {
        self.positive_rules
            .store(rules == PromptRules::Positive, Ordering::SeqCst);
    }

    /// The wording of the shared rules this narrator sends.
    pub fn prompt_rules(&self) -> PromptRules {
        if self.positive_rules.load(Ordering::SeqCst) {
            PromptRules::Positive
        } else {
            PromptRules::Shipped
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

    /// One failed call: with the breaker on, [`TIMEOUT_BREAKER_STRIKES`] in a row pause the
    /// narrator.
    fn strike(&self) {
        let strikes = self.consecutive_timeouts.fetch_add(1, Ordering::SeqCst) + 1;
        if self.breaker_enabled.load(Ordering::SeqCst) && strikes >= TIMEOUT_BREAKER_STRIKES {
            self.paused.store(true, Ordering::SeqCst);
        }
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
        let sent = self.last_request.lock().ok().and_then(|mut r| r.take());
        let (seed, request) = match sent {
            Some((seed, body)) => (Some(seed), Some(body)),
            None => (None, None),
        };
        if let Ok(mut log) = self.slot_log.lock() {
            if log.len() >= MAX_SLOT_RECORDS {
                // Nobody is reading them: keep the newest.
                log.remove(0);
            }
            log.push(SlotRecord {
                is_scene,
                outcome,
                raw_response,
                candidate,
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                seed,
                request,
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
            "think": false,
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
        call: SlotCall,
    ) -> Option<String> {
        let seed = call.seed;
        if self.paused.load(Ordering::SeqCst) || self.turn_timed_out.load(Ordering::SeqCst) {
            // Paused by the breaker, or a call of this turn already timed out: no call, the
            // plain line shows. So one turn waits at most one per-call timeout.
            self.fallbacks.fetch_add(1, Ordering::SeqCst);
            return None;
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = Instant::now();
        let timeout = Duration::from_millis(self.settings.per_call_timeout_ms);
        let schema = call.schema;
        let mut payload = serde_json::json!({
            "model": self.settings.model,
            "system": system_prompt,
            "prompt": user_prompt,
            "stream": false,
            "think": false,
            "options": {
                "temperature": NARRATOR_TEMPERATURE,
                "seed": seed,
                "num_ctx": NARRATOR_NUM_CTX,
                "num_predict": schema.num_predict(self.settings.length)
            }
        });
        if let Some(mut format) = narration_format_schema_for(schema, self.settings.length) {
            if let Some(limit) = call.max_items {
                format["properties"]["sentences"]["maxItems"] = serde_json::json!(limit);
            }
            payload["format"] = format;
        }
        if let Some(layers) = call.num_gpu {
            payload["options"]["num_gpu"] = serde_json::json!(layers);
        }
        if call.stop_at_period {
            payload["options"]["stop"] = serde_json::json!(["."]);
        }
        if call.model_card_sampling {
            use crate::narrator_experiment::{
                MODEL_CARD_TEMPERATURE, MODEL_CARD_TOP_K, MODEL_CARD_TOP_P,
            };
            payload["options"]["temperature"] = serde_json::json!(MODEL_CARD_TEMPERATURE);
            payload["options"]["top_p"] = serde_json::json!(MODEL_CARD_TOP_P);
            payload["options"]["top_k"] = serde_json::json!(MODEL_CARD_TOP_K);
        }
        let payload = payload.to_string();
        if self.record_slots.load(Ordering::SeqCst) {
            if let Ok(mut last) = self.last_request.lock() {
                *last = Some((seed, payload.clone()));
            }
        }

        let (status, raw_outer) = match self.transport.post_generate(endpoint, &payload, timeout) {
            Ok(pair) => pair,
            Err(TransportError::Timeout) => {
                self.timeouts.fetch_add(1, Ordering::SeqCst);
                self.fallbacks.fetch_add(1, Ordering::SeqCst);
                if self.breaker_enabled.load(Ordering::SeqCst) {
                    self.turn_timed_out.store(true, Ordering::SeqCst);
                }
                self.strike();
                self.push_record(is_scene, SlotOutcome::Timeout, None, None, started);
                return None;
            }
            Err(_) => {
                self.fallbacks.fetch_add(1, Ordering::SeqCst);
                self.strike();
                self.push_record(is_scene, SlotOutcome::TransportError, None, None, started);
                return None;
            }
        };
        if status >= 500 {
            // A server error counts as a strike, as a timeout does, and does not end a run of
            // them.
            self.strike();
        } else {
            // Any other answer, good or bad, ends a run of failures.
            self.consecutive_timeouts.store(0, Ordering::SeqCst);
        }
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

        let parsed = if schema == FormatSchema::PlainText {
            // No schema was asked for: the answer is the prose, checked as it stands.
            let text = inner_str.trim();
            if text.is_empty() {
                Err("empty answer".to_string())
            } else if call.stop_at_period {
                // Generation stopped at the first full stop; the period is put back here.
                Ok(crate::narrator_experiment::restore_stopped_period(text))
            } else {
                Ok(text.to_string())
            }
        } else {
            parse_slot_prose(inner_str, is_scene)
        };
        let candidate_prose = match parsed {
            Ok(prose) => {
                let prose = plain_apostrophes(&prose);
                if call.trim_to_cap {
                    crate::narrator_experiment::trim_to_word_cap(
                        &prose,
                        self.settings.length.word_cap(),
                    )
                } else {
                    prose
                }
            }
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
        if let Ok(mut previous) = self.previous_opening.lock() {
            *previous = Some(crate::narrator_truth::opening_words(&candidate_prose, 3));
        }
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
        self.turn_timed_out.store(false, Ordering::SeqCst);
        let Ok(endpoint) =
            validate_endpoint(&self.settings.endpoint, self.settings.allow_non_loopback)
        else {
            self.fallbacks.fetch_add(2, Ordering::SeqCst);
            return TurnNarration::default();
        };

        let profile = self.settings.profile;
        let length = self.settings.length;
        let arms = self.experiment();

        let rules = self.prompt_rules();

        // In the brief-by-stop arm a Brief line is plain text stopped at its first full stop,
        // and the other lengths keep the schema with a sentence-item limit.
        let stop_brief = arms.brief_by_stop && length == NarratorLength::Brief;
        let schema = if stop_brief {
            FormatSchema::PlainText
        } else {
            self.format_schema()
        };
        let call = SlotCall {
            seed: 0,
            schema,
            max_items: if arms.brief_by_stop && !stop_brief {
                Some(crate::narrator_experiment::schema_item_limit(length))
            } else {
                None
            },
            stop_at_period: stop_brief,
            model_card_sampling: arms.model_card_sampling,
            trim_to_cap: arms.trim_to_cap,
            num_gpu: arms.num_gpu,
        };
        let for_arm = |(sys, user): (String, String)| -> (String, String) {
            if arms.rulings_prompt {
                (
                    crate::narrator_experiment::rulings_system_prompt(&sys, profile, length),
                    crate::narrator_experiment::rulings_user_prompt(&user, length),
                )
            } else {
                (sys, user)
            }
        };
        // Up to `arms.candidates` calls for a line, one after another, each with its own seed
        // derived from the turn's; the first accepted line is used. One candidate is the
        // shipped single call with the turn's own seed.
        let attempt = |sys: &str, user: &str, is_scene: bool| -> Option<String> {
            // The line's own checker: the shipped one, plus what the arms add. The opening of
            // the run's previous accepted line is read now, so an outcome line is compared
            // with its own turn's scene line when that was accepted.
            let previous = self.previous_opening.lock().ok().and_then(|p| p.clone());
            let checker_ctx = crate::narrator_experiment::checker_context_for(
                ctx,
                length,
                profile,
                arms,
                !is_scene,
                previous.as_deref(),
            );
            let base = narration_call_seed(ctx, profile, length, is_scene);
            for k in 0..usize::from(arms.candidates.max(1)) {
                let seed = crate::narrator_experiment::candidate_seed(base, k);
                if let Some(line) = self.call_ollama_slot(
                    &endpoint,
                    sys,
                    user,
                    is_scene,
                    &checker_ctx,
                    SlotCall { seed, ..call },
                ) {
                    return Some(line);
                }
                if self.paused.load(Ordering::SeqCst) {
                    break;
                }
            }
            None
        };

        let (scene_sys, scene_user) =
            for_arm(build_scene_prompts_for(profile, length, ctx, rules, schema));
        let scene = attempt(&scene_sys, &scene_user, true);

        let (out_sys, out_user) = for_arm(build_outcome_prompts_for(
            profile, length, ctx, rules, schema,
        ));
        let out_sys = if arms.outcome_lead {
            crate::narrator_experiment::lead_outcome_system_prompt_for(&out_sys, ctx, arms)
        } else {
            out_sys
        };
        let outcome = attempt(&out_sys, &out_user, false);

        TurnNarration { scene, outcome }
    }
}
