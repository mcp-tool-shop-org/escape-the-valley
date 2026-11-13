//! Step-based deterministic turn engine (`step_engine.py`).
//!
//! Exact port of `src/escape_the_valley/step_engine.py` at `87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`
//! with the GM off (`gm_config.enabled == false`).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::{
    apply_outcome, build_event_library, missing_supplies_for_choice, resolve_event_with_ruleset,
    select_event_with_ruleset,
};
use crate::memory::{
    check_resource_crises, emit_arrival_card, emit_escape_valve_card, emit_event_card,
    emit_health_cards, emit_wagon_card,
};
use crate::model::{
    doctrine_modifiers, EndingResult, GamePhase, IntentAction, JournalEntry, MapNode, Pace,
    PlayerIntent, RunState, TimeOfDay,
};
use crate::physics::{
    abandon_cargo, apply_breakdown, attempt_hunt, attempt_repair, can_abandon_cargo,
    can_desperate_repair, can_hard_ration, check_breakdown, check_game_over, check_health_effects,
    check_night_travel_danger, check_spoilage, collect_new_death_effects,
    compute_daily_consumption, compute_travel_distance, current_node, desperate_repair,
    determine_cause_of_death, halve_consumption, hard_ration, rest_day, update_morale,
    HealthEffect,
};
use crate::rng::CpythonRng;
use crate::ruleset::Ruleset;
use crate::save::save_game;
use crate::worldgen::{generate_weather, EventOutcome, EventSkeleton};

const EVENT_CHOICE_LETTERS: [&str; 7] = ["A", "B", "C", "D", "E", "F", "G"];
const ROUTE_LETTERS: [&str; 4] = ["A", "B", "C", "D"];

const PAR_MILES_PER_DAY: i32 = 8;
const PAR_DAYS_FLOOR: i32 = 8;

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// A single choice the player can pick during an event (`step_engine.py:111-118`).
///
/// `affordable` and `missing` are Rust-only: under `Ruleset::V2` they say whether the party
/// holds the supplies the choice requires, and which supply keys it lacks. Under `Legacy` they
/// are always `true` / empty, and neither is serialised, so Legacy output is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventChoiceInfo {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub risk_hint: String,
    #[serde(default)]
    pub cost_hint: String,
    #[serde(default = "default_affordable", skip_serializing_if = "is_true")]
    pub affordable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

fn default_affordable() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

impl Default for EventChoiceInfo {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            risk_hint: String::new(),
            cost_hint: String::new(),
            affordable: true,
            missing: Vec::new(),
        }
    }
}

/// A fork option in the road (`step_engine.py:121-128`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteOption {
    pub node_id: String,
    pub name: String,
    pub distance: i32,
}

/// Everything that happened during one step (`step_engine.py:130-150`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StepMessages {
    pub lines: Vec<String>,
    #[serde(default)]
    pub effects: Vec<HealthEffect>,
    pub event_title: String,
    pub event_narration: String,
    pub event_choices: Vec<EventChoiceInfo>,
    pub route_options: Vec<RouteOption>,
    pub outcome_title: String,
    pub outcome_narration: String,
    pub outcome_deltas: BTreeMap<String, i32>,
    pub gm_degraded: bool,
    pub gm_degraded_reason: String,
    pub ending: Option<EndingResult>,
}

/// Return the `A`..`G` letters that `event.outcome_templates` defines (`step_engine.py:83-89`).
pub fn template_choice_letters(event: &EventSkeleton) -> Vec<String> {
    EVENT_CHOICE_LETTERS
        .iter()
        .filter(|&&letter| event.outcome_templates.contains_key(letter))
        .map(|&s| s.to_string())
        .collect()
}

/// Letters currently on offer for a `ROUTE` fork (`step_engine.py:92-94`).
pub fn route_offered_letters(pending: &[RouteOption]) -> Vec<String> {
    ROUTE_LETTERS
        .iter()
        .take(pending.len())
        .map(|&s| s.to_string())
        .collect()
}

/// Retry copy that names the letters the engine will honor (`step_engine.py:97-108`).
pub fn choose_one_of_line(offered: &[String]) -> String {
    if offered.is_empty() {
        return "Choose an option for the current event.".to_string();
    }
    format!(
        "That option isn't available -- choose one of: {}.",
        offered.join("/")
    )
}

/// Step-based deterministic game engine (`step_engine.py::StepEngine`).
pub struct StepEngine {
    pub state: RunState,
    pub rng: CpythonRng,
    pub event_library: Vec<EventSkeleton>,
    pub phase: GamePhase,
    pub msgs: StepMessages,
    pub diagnostics: BTreeMap<String, i32>,
    /// Which rules this engine plays by. Runtime only: never saved. Defaults to `Legacy`.
    pub ruleset: Ruleset,
    autosave: bool,
    base_path: Option<PathBuf>,
    pending_event: Option<EventSkeleton>,
    pending_event_choices: Vec<EventChoiceInfo>,
    pending_event_title: String,
    pending_event_narration: String,
    pending_routes: Vec<RouteOption>,
    alive_before_step: Vec<usize>,
    emitted_death_indices: Vec<usize>,
}

impl StepEngine {
    /// Construct a `StepEngine` with `autosave = false` (pure in-memory).
    pub fn new(state: RunState) -> Self {
        Self::with_options(state, false, None)
    }

    /// Construct a `StepEngine` using an explicit `ContentPack` (`autosave = false`).
    pub fn with_content(state: RunState, content: &crate::content::ContentPack) -> Self {
        let mut engine = Self::with_options(state, false, None);
        engine.event_library = content.all_events.clone();
        engine
    }

    /// Choose the ruleset this engine plays by (default `Legacy`). Not saved; a resumed run
    /// must be given the same ruleset it was played under.
    pub fn set_ruleset(&mut self, ruleset: Ruleset) {
        self.ruleset = ruleset;
    }

    /// Return the currently pending `EventSkeleton`, if the engine is in `GamePhase::Event`.
    pub fn pending_event(&self) -> Option<&EventSkeleton> {
        self.pending_event.as_ref()
    }

    /// Return the currently offered choices for a pending event.
    pub fn pending_event_choices(&self) -> &[EventChoiceInfo] {
        &self.pending_event_choices
    }

    /// Return the currently offered route forks when in `GamePhase::Route`.
    pub fn pending_routes(&self) -> &[RouteOption] {
        &self.pending_routes
    }

    /// Construct a `StepEngine` with explicit `autosave` and `base_path` options (`step_engine.py:161-235`).
    pub fn with_options(state: RunState, autosave: bool, base_path: Option<PathBuf>) -> Self {
        let rng = CpythonRng::from_seed_and_counter(
            state.seed,
            state.rng_counter,
            state.rng_state.as_ref(),
        );
        let event_library = build_event_library();
        let diagnostics = BTreeMap::from([
            ("wagon_breakdowns".to_string(), 0),
            ("events_high_sev".to_string(), 0),
            ("events_total".to_string(), 0),
            ("maintenance_windows".to_string(), 0),
            ("caches_found".to_string(), 0),
            ("escape_valves_used".to_string(), 0),
            ("gm_calls".to_string(), 0),
            ("gm_fallbacks".to_string(), 0),
            ("memory_card_failures".to_string(), 0),
        ]);

        let mut engine = Self {
            state,
            rng,
            event_library,
            phase: GamePhase::Camp,
            msgs: StepMessages::default(),
            diagnostics,
            ruleset: Ruleset::default(),
            autosave,
            base_path,
            pending_event: None,
            pending_event_choices: Vec::new(),
            pending_event_title: String::new(),
            pending_event_narration: String::new(),
            pending_routes: Vec::new(),
            alive_before_step: Vec::new(),
            emitted_death_indices: Vec::new(),
        };
        engine.check_initial_phase();
        engine
    }

    /// Set phase based on current state (`step_engine.py:237-240`).
    fn check_initial_phase(&mut self) {
        if self.state.game_over {
            self.phase = GamePhase::GameOver;
        }
    }

    /// Process one player action (`step_engine.py:242-277`).
    pub fn step(&mut self, intent: &PlayerIntent) -> StepMessages {
        self.msgs = StepMessages::default();

        if self.phase == GamePhase::GameOver {
            self.msgs.lines.push("This run is over.".to_string());
            return self.msgs.clone();
        }

        if self.state.party.alive_count() == 0 {
            self.check_game_over_transition();
            self.msgs.lines.push("This run is over.".to_string());
            self.save();
            return self.msgs.clone();
        }

        self.alive_before_step = self
            .state
            .party
            .members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.is_alive())
            .map(|(i, _)| i)
            .collect();
        self.emitted_death_indices.clear();

        match self.phase {
            GamePhase::Event => self.handle_event_choice(intent),
            GamePhase::Route => self.handle_route_choice(intent),
            GamePhase::Camp => self.handle_camp(intent),
            GamePhase::GameOver => {}
        }

        self.emit_unreported_deaths(false);
        self.check_game_over_transition();
        self.save();

        self.msgs.clone()
    }

    fn handle_camp(&mut self, intent: &PlayerIntent) {
        match intent.action {
            IntentAction::Travel => self.do_travel(),
            IntentAction::Rest => self.do_rest(),
            IntentAction::Hunt => self.do_hunt(),
            IntentAction::Repair => self.do_repair(),
            IntentAction::ChangePace => self.do_change_pace(&intent.pace),
            IntentAction::AbandonCargo => self.do_abandon_cargo(),
            IntentAction::DesperateRepair => self.do_desperate_repair(),
            IntentAction::HardRation => self.do_hard_ration(),
            _ => {}
        }
    }

    fn do_travel(&mut self) {
        self.state.last_action = "TRAVEL".to_string();

        if let Some(node) = current_node(&self.state).cloned() {
            if node.connections.len() > 1 && self.state.distance_remaining <= 0 {
                if self.build_route_choices(&node) {
                    self.phase = GamePhase::Route;
                    self.msgs
                        .lines
                        .push("The trail forks ahead. Choose your path.".to_string());
                }
                return;
            }

            if node.connections.len() == 1 && self.state.distance_remaining <= 0 {
                let conn_id = &node.connections[0];
                let valid = conn_id != &node.node_id
                    && self.state.map_nodes.iter().any(|n| &n.node_id == conn_id);
                if !valid {
                    self.msgs.lines.push(format!(
                        "The trail forks, but every route is broken or leads nowhere. The party can't press on from {} this way.",
                        node.name
                    ));
                    return;
                }
            }
        }

        if self.state.maintained_turns_remaining > 0 {
            self.state.maintained_turns_remaining -= 1;
        }

        if self.state.escape_valve_cooldown > 0 {
            self.state.escape_valve_cooldown -= 1;
        }

        let distance = compute_travel_distance(&self.state);

        self.state.distance_remaining -= distance;
        self.state.distance_traveled += distance;

        let consumption = compute_daily_consumption(&self.state, true);
        self.state.supplies.apply_delta(&consumption);

        if self.state.rationing_steps > 0 {
            self.state.rationing_steps -= 1;
            if self.state.rationing_steps <= 0 {
                self.msgs.lines.push("Rationing has ended.".to_string());
            }
        }

        if self.state.distance_remaining <= 0 {
            self.arrive_at_next_node();
        } else {
            let rem = self.state.distance_remaining;
            let unit = if rem == 1 { "mile" } else { "miles" };
            self.msgs.lines.push(format!(
                "Traveled {distance} miles. {rem} {unit} remaining."
            ));
        }

        self.advance_time();

        let spoilage = check_spoilage(&mut self.state, &mut self.rng);
        if !spoilage.is_empty() {
            self.state.supplies.apply_delta(&spoilage);
            let loss = spoilage.get("food").copied().unwrap_or(0).abs();
            self.msgs
                .lines
                .push(format!("Food spoiled! Lost {loss} food (no salt)."));
        }

        if let Some(damage) = check_breakdown(&self.state, &mut self.rng) {
            *self
                .diagnostics
                .entry("wagon_breakdowns".to_string())
                .or_insert(0) += 1;
            let had_parts = self.state.supplies.parts() > 0;
            let deltas = apply_breakdown(&mut self.state, damage);
            if !deltas.is_empty() {
                self.msgs
                    .lines
                    .push(format!("Wagon breakdown! Repaired. Damage: {damage}"));
            } else {
                self.msgs
                    .lines
                    .push(format!("Wagon breakdown! No parts. Damage: {damage}"));
            }
            emit_wagon_card(&mut self.state, damage, had_parts);
            let outcome_text = if had_parts {
                format!("The wagon broke down. Damage: {damage}.")
            } else {
                format!("The wagon broke down. Damage: {damage}. No spare parts on hand.")
            };
            self.record_milestone(
                "wagon:breakdown",
                "Wagon Breakdown",
                &outcome_text,
                vec!["wagon".to_string(), "breakdown".to_string()],
            );
        }

        if let Some(damage) = check_night_travel_danger(&self.state, &mut self.rng) {
            apply_breakdown(&mut self.state, damage);
            self.msgs
                .lines
                .push(format!("Dark travel mishap! Wagon damage: {damage}"));
        }

        let effects = check_health_effects(&mut self.state, &mut self.rng);
        self.msgs.effects.extend(effects.clone());
        for eff in &effects {
            match eff {
                HealthEffect::Died { member, cause } => {
                    let c = if cause.is_empty() {
                        "the trail"
                    } else {
                        cause.as_str()
                    };
                    self.msgs.lines.push(format!("{member} has died ({c})."));
                }
                HealthEffect::FellSick { member } => {
                    self.msgs.lines.push(format!("{member} has fallen ill."));
                }
                HealthEffect::Healed { member } => {
                    self.msgs.lines.push(format!("{member} is recovering."));
                }
                HealthEffect::Starvation { .. }
                | HealthEffect::Dehydration { .. }
                | HealthEffect::Recovered { .. } => {}
            }
        }
        for (i, m) in self.state.party.members.iter().enumerate() {
            if !m.is_alive() && !self.emitted_death_indices.contains(&i) {
                self.emitted_death_indices.push(i);
            }
        }
        self.record_death_milestones(&effects);
        emit_health_cards(&mut self.state, &effects);

        check_resource_crises(&mut self.state);

        self.maybe_trigger_event();

        update_morale(&mut self.state, 0);
    }

    fn do_rest(&mut self) {
        if self.state.last_action == "REPAIR" {
            let doc_mods = doctrine_modifiers(&self.state.doctrine);
            let duration = 2 + doc_mods.maintenance_bonus.unwrap_or(0);
            self.state.maintained_turns_remaining = duration;
            *self
                .diagnostics
                .entry("maintenance_windows".to_string())
                .or_insert(0) += 1;
            let new_water = std::cmp::max(0, self.state.supplies.water() - 3);
            self.state.supplies.set_water(new_water);
            self.msgs
                .lines
                .push("Maintenance window: the wagon rides steady. (-3 water)".to_string());
        }
        self.state.last_action = "REST".to_string();

        let consumption = compute_daily_consumption(&self.state, false);
        self.state.supplies.apply_delta(&consumption);

        let recovered = rest_day(&mut self.state, &mut self.rng);
        self.advance_time();

        self.msgs
            .lines
            .push("The party rests for a day.".to_string());
        for member in recovered {
            self.msgs.lines.push(format!("  {member} has recovered."));
            self.msgs.effects.push(HealthEffect::Recovered { member });
        }

        self.emit_unreported_deaths(false);
        update_morale(&mut self.state, 0);
    }

    fn do_hunt(&mut self) {
        self.state.last_action = "HUNT".to_string();
        if self.state.supplies.ammo() <= 0 {
            self.msgs
                .lines
                .push("No ammunition for hunting.".to_string());
            return;
        }

        let result = attempt_hunt(&mut self.state, &mut self.rng);
        self.state.supplies.apply_delta(&result.supplies_delta);

        let food_gain = result.supplies_delta.get("food").copied().unwrap_or(0);
        if food_gain > 0 {
            self.msgs
                .lines
                .push(format!("Hunt successful! +{food_gain} food. -1 ammo."));
        } else {
            self.msgs
                .lines
                .push("The hunt yielded nothing. -1 ammo.".to_string());
        }
        if let Some(injured) = result.injured {
            if !injured.is_empty() {
                self.msgs
                    .lines
                    .push(format!("{injured} was injured on the hunt."));
            }
        }
        self.emit_unreported_deaths(false);

        let half = halve_consumption(&compute_daily_consumption(&self.state, false));
        self.state.supplies.apply_delta(&half);
        update_morale(&mut self.state, 0);
    }

    fn do_repair(&mut self) {
        if self.state.supplies.parts() <= 0 {
            self.msgs.lines.push("No parts for repairs.".to_string());
            return;
        }

        if self.state.wagon.condition >= 90 {
            self.msgs
                .lines
                .push("Wagon is in good shape. No repair needed.".to_string());
            return;
        }

        if self.state.last_action == "REST" {
            let doc_mods = doctrine_modifiers(&self.state.doctrine);
            let duration = 2 + doc_mods.maintenance_bonus.unwrap_or(0);
            self.state.maintained_turns_remaining = duration;
            *self
                .diagnostics
                .entry("maintenance_windows".to_string())
                .or_insert(0) += 1;
            let new_water = std::cmp::max(0, self.state.supplies.water() - 3);
            self.state.supplies.set_water(new_water);
            self.msgs
                .lines
                .push("Maintenance window: the wagon rides steady. (-3 water)".to_string());
        }
        self.state.last_action = "REPAIR".to_string();

        let deltas = attempt_repair(&mut self.state);
        self.state.supplies.apply_delta(&deltas);
        self.msgs.lines.push(format!(
            "Wagon repaired to {}/100. -1 part.",
            self.state.wagon.condition
        ));
        self.emit_unreported_deaths(false);

        let half = halve_consumption(&compute_daily_consumption(&self.state, false));
        self.state.supplies.apply_delta(&half);
    }

    fn do_change_pace(&mut self, pace_str: &str) {
        let new_pace = match pace_str.to_lowercase().as_str() {
            "slow" => Pace::Slow,
            "steady" => Pace::Steady,
            "hard" => Pace::Hard,
            _ => {
                self.msgs.lines.push(format!(
                    "Unknown pace: {pace_str}. Use slow, steady, or hard."
                ));
                return;
            }
        };

        self.state.wagon.pace = new_pace;
        self.msgs
            .lines
            .push(format!("Pace set to {}.", new_pace.as_str()));
    }

    fn do_abandon_cargo(&mut self) {
        self.state.last_action = "ABANDON_CARGO".to_string();
        if !can_abandon_cargo(&self.state) {
            self.msgs
                .lines
                .push("Wagon is not damaged enough to justify abandoning cargo.".to_string());
            return;
        }

        *self
            .diagnostics
            .entry("escape_valves_used".to_string())
            .or_insert(0) += 1;
        self.state.escape_valve_cooldown = 3;
        let result = abandon_cargo(&mut self.state);
        let dropped_items: Vec<String> = result
            .iter()
            .filter(|(_, v)| *v < 0)
            .map(|(k, v)| format!("-{} {k}", v.abs()))
            .collect();
        let dropped_summary = if dropped_items.is_empty() {
            "nothing".to_string()
        } else {
            dropped_items.join(", ")
        };
        self.msgs.lines.push(format!(
            "Abandoned cargo to lighten the wagon. Wagon +25. Dropped: {dropped_summary}. Morale fell."
        ));
        emit_escape_valve_card(
            &mut self.state,
            "abandon_cargo",
            &format!("Abandoned cargo to save the wagon (dropped: {dropped_summary})."),
        );
    }

    fn do_desperate_repair(&mut self) {
        self.state.last_action = "DESPERATE_REPAIR".to_string();
        if !can_desperate_repair(&self.state) {
            self.msgs.lines.push(
                "Desperate repair requires a badly damaged wagon and no spare parts.".to_string(),
            );
            return;
        }

        *self
            .diagnostics
            .entry("escape_valves_used".to_string())
            .or_insert(0) += 1;
        self.state.escape_valve_cooldown = 3;
        let result = desperate_repair(&mut self.state, &mut self.rng);
        if result.success {
            self.msgs.lines.push(format!(
                "Desperate repair succeeded! Wagon +{}.",
                result.wagon_delta
            ));
        } else {
            self.msgs.lines.push(format!(
                "Desperate repair failed! Wagon {}. {} was injured in the attempt.",
                result.wagon_delta, result.injured
            ));
        }
        let detail = if result.success {
            "Repair succeeded.".to_string()
        } else {
            format!("Repair failed. {} injured.", result.injured)
        };
        emit_escape_valve_card(&mut self.state, "desperate_repair", &detail);
        self.emit_unreported_deaths(false);
    }

    fn hard_ration_refusal_line(&self) -> String {
        let alive = self.state.party.alive_count() as i32;
        if alive <= 0 || self.state.supplies.food() >= alive * 3 {
            return "Food is not low enough to ration.".to_string();
        }
        if self.state.escape_valve_cooldown > 0 {
            return format!(
                "Cannot ration again for {} more actions.",
                self.state.escape_valve_cooldown
            );
        }
        if self.state.rationing_steps > 0 {
            return "Already rationing.".to_string();
        }
        "Cannot ration further right now.".to_string()
    }

    fn do_hard_ration(&mut self) {
        self.state.last_action = "HARD_RATION".to_string();
        if !can_hard_ration(&self.state) {
            self.msgs.lines.push(self.hard_ration_refusal_line());
            return;
        }

        *self
            .diagnostics
            .entry("escape_valves_used".to_string())
            .or_insert(0) += 1;
        self.state.escape_valve_cooldown = 3;
        hard_ration(&mut self.state);
        self.msgs.lines.push(
            "Hard rationing imposed for 2 days. Food and water consumption halved. Morale -10, everyone weakened."
                .to_string(),
        );
        emit_escape_valve_card(
            &mut self.state,
            "hard_ration",
            "Hard rationing imposed — half rations for 2 days.",
        );
        self.emit_unreported_deaths(false);
    }

    /// Emit message, `died` effect, milestone, and memory card for any newly dead member (`step_engine.py:693-709`).
    fn emit_unreported_deaths(&mut self, from_event: bool) {
        let death_effects = collect_new_death_effects(
            &mut self.state,
            &self.alive_before_step,
            &mut self.emitted_death_indices,
            from_event,
        );
        if death_effects.is_empty() {
            return;
        }
        for eff in &death_effects {
            if let HealthEffect::Died { member, cause } = eff {
                let c = if cause.is_empty() {
                    "the trail"
                } else {
                    cause.as_str()
                };
                self.msgs.lines.push(format!("{member} has died ({c})."));
            }
        }
        self.msgs.effects.extend(death_effects.clone());
        self.record_death_milestones(&death_effects);
        emit_health_cards(&mut self.state, &death_effects);
    }

    fn record_milestone(
        &mut self,
        event_id: &str,
        scene_title: &str,
        outcome: &str,
        tags: Vec<String>,
    ) {
        let location = current_node(&self.state)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        self.state.journal.push(JournalEntry {
            day: self.state.day,
            location,
            event_id: event_id.to_string(),
            scene_title: scene_title.to_string(),
            narration: String::new(),
            choice_made: String::new(),
            outcome: truncate_chars(outcome, 300),
            deltas: BTreeMap::new(),
            tags,
        });
    }

    fn record_death_milestones(&mut self, effects: &[HealthEffect]) {
        let location = current_node(&self.state)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let day = self.state.day;
        for eff in effects {
            if let HealthEffect::Died { member, cause } = eff {
                let c = if cause.is_empty() {
                    "the trail"
                } else {
                    cause.as_str()
                };
                let c_lower = c.to_lowercase();
                self.record_milestone(
                    &format!("death:{c_lower}"),
                    &format!("Death: {member}"),
                    &format!("{member} died of {c_lower} on day {day} near {location}."),
                    vec!["death".to_string(), c_lower],
                );
            }
        }
    }

    fn maybe_trigger_event(&mut self) {
        if self.rng.random() > 0.6 {
            return;
        }

        let node_biome = current_node(&self.state).map(|n| n.biome);
        let weather =
            node_biome.map(|biome| generate_weather(&mut self.rng, biome, self.state.day));

        let Some(event) = select_event_with_ruleset(
            &mut self.state,
            &mut self.rng,
            &self.event_library,
            weather,
            self.ruleset,
        ) else {
            // Only V2 can return no event (every candidate excluded); no state changed.
            return;
        };
        *self
            .diagnostics
            .entry("events_total".to_string())
            .or_insert(0) += 1;
        if event.severity == "high" {
            *self
                .diagnostics
                .entry("events_high_sev".to_string())
                .or_insert(0) += 1;
        }

        let title = event.title.clone();
        let narration = event.fallback_narration.clone();
        let choices: Vec<EventChoiceInfo> = event
            .fallback_choices
            .iter()
            .map(|c| {
                let missing =
                    missing_supplies_for_choice(&self.state, &event, &c.choice_id, self.ruleset);
                EventChoiceInfo {
                    id: c.choice_id.clone(),
                    label: c.label.clone(),
                    risk_hint: c.risk_hint.clone(),
                    cost_hint: c.cost_hint.clone(),
                    affordable: missing.is_empty(),
                    missing,
                }
            })
            .collect();

        if choices.is_empty() {
            return;
        }

        self.pending_event = Some(event);
        self.pending_event_choices = choices.clone();
        self.pending_event_title = title.clone();
        self.pending_event_narration = narration.clone();
        self.phase = GamePhase::Event;

        self.msgs.event_title = title;
        self.msgs.event_narration = narration;
        self.msgs.event_choices = choices;
    }

    /// Recompute `affordable` / `missing` of the pending choices from the current state (V2).
    fn refresh_choice_affordability(&mut self) {
        let Some(event) = self.pending_event.as_ref() else {
            return;
        };
        for info in &mut self.pending_event_choices {
            let missing = missing_supplies_for_choice(&self.state, event, &info.id, self.ruleset);
            info.affordable = missing.is_empty();
            info.missing = missing;
        }
    }

    fn re_present_event(&mut self) {
        if self.ruleset == Ruleset::V2 {
            self.refresh_choice_affordability();
        }
        self.msgs.event_title = self.pending_event_title.clone();
        self.msgs.event_narration = self.pending_event_narration.clone();
        self.msgs.event_choices = self.pending_event_choices.clone();
    }

    fn handle_event_choice(&mut self, intent: &PlayerIntent) {
        let offered_ids: Vec<String> = self
            .pending_event_choices
            .iter()
            .map(|c| c.id.clone())
            .collect();
        if intent.action != IntentAction::Choose {
            self.msgs.lines.push(choose_one_of_line(&offered_ids));
            self.re_present_event();
            return;
        }

        let Some(event) = self.pending_event.clone() else {
            self.phase = GamePhase::Camp;
            return;
        };

        let choice_id = &intent.choice_id;
        if !offered_ids.contains(choice_id) {
            self.msgs.lines.push(choose_one_of_line(&offered_ids));
            self.re_present_event();
            return;
        }

        let outcome = resolve_event_with_ruleset(
            &mut self.state,
            &event,
            choice_id,
            &mut self.rng,
            self.ruleset,
        );
        apply_outcome(&mut self.state, &outcome);

        for _ in 0..std::cmp::max(0, outcome.time_cost) {
            self.advance_time();
        }

        let choice_template = event
            .fallback_choices
            .iter()
            .find(|c| &c.choice_id == choice_id);
        let choice_label = self
            .pending_event_choices
            .iter()
            .find(|c| &c.id == choice_id)
            .map(|c| c.label.clone())
            .unwrap_or_default();

        let outcome_title = "Outcome".to_string();
        let outcome_narration = if !outcome
            .special_flags
            .iter()
            .any(|f| f == "insufficient_supplies")
        {
            choice_template
                .and_then(|c| c.outcome_sentence.as_deref())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| build_fallback_callout(&outcome))
        } else {
            build_fallback_callout(&outcome)
        };

        self.msgs.outcome_title = outcome_title;
        self.msgs.outcome_narration = outcome_narration.clone();
        self.msgs.outcome_deltas = outcome.supplies_delta.clone();

        let location = current_node(&self.state)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let scene_title = if !self.msgs.event_title.is_empty() {
            self.msgs.event_title.clone()
        } else {
            event.title.clone()
        };
        let narration_src = if !self.msgs.event_narration.is_empty() {
            &self.msgs.event_narration
        } else {
            &event.fallback_narration
        };

        self.state.journal.push(JournalEntry {
            day: self.state.day,
            location,
            event_id: event.event_id.clone(),
            scene_title,
            narration: truncate_chars(narration_src, 300),
            choice_made: format!("{choice_id}: {choice_label}"),
            outcome: truncate_chars(&outcome_narration, 300),
            deltas: outcome.supplies_delta.clone(),
            tags: event.tags.clone(),
        });

        emit_event_card(&mut self.state, &event);
        self.emit_unreported_deaths(true);
        check_resource_crises(&mut self.state);

        self.pending_event = None;
        self.pending_event_choices.clear();
        self.pending_event_title.clear();
        self.pending_event_narration.clear();
        self.phase = GamePhase::Camp;

        self.state.rng_counter = self.rng.counter;
    }

    fn build_route_choices(&mut self, node: &MapNode) -> bool {
        let mut options = Vec::new();
        for conn_id in &node.connections {
            if conn_id == &node.node_id {
                continue;
            }
            for n in &self.state.map_nodes {
                if &n.node_id == conn_id {
                    let dist = node.distance_to.get(conn_id).copied().unwrap_or(15);
                    options.push(RouteOption {
                        node_id: conn_id.clone(),
                        name: n.name.clone(),
                        distance: dist,
                    });
                    break;
                }
            }
        }

        if options.is_empty() && !node.connections.is_empty() {
            self.msgs.lines.push(format!(
                "The trail forks, but every route is broken or leads nowhere. The party can't press on from {} this way.",
                node.name
            ));
            self.pending_routes.clear();
            self.msgs.route_options.clear();
            return false;
        }

        self.pending_routes = options.clone();
        self.msgs.route_options = options;
        true
    }

    fn handle_route_choice(&mut self, intent: &PlayerIntent) {
        let offered_letters = route_offered_letters(&self.pending_routes);
        if intent.action != IntentAction::Choose {
            self.msgs.lines.push(choose_one_of_line(&offered_letters));
            self.msgs.route_options = self.pending_routes.clone();
            return;
        }

        let idx = ROUTE_LETTERS
            .iter()
            .position(|&letter| letter == intent.choice_id);

        let Some(i) = idx.filter(|&i| i < self.pending_routes.len()) else {
            self.msgs.lines.push(choose_one_of_line(&offered_letters));
            self.msgs.route_options = self.pending_routes.clone();
            return;
        };

        let route = self.pending_routes[i].clone();
        let dist = current_node(&self.state)
            .and_then(|n| n.distance_to.get(&route.node_id).copied())
            .unwrap_or(15);
        self.state.destination_id = route.node_id.clone();
        self.state.distance_remaining = dist;
        self.msgs.lines.push(format!(
            "Heading toward {} ({} miles).",
            route.name, route.distance
        ));

        self.pending_routes.clear();
        self.phase = GamePhase::Camp;
    }

    fn arrive_at_next_node(&mut self) {
        let dest_idx = self
            .state
            .map_nodes
            .iter()
            .position(|n| n.node_id == self.state.destination_id);

        let Some(d_idx) = dest_idx else {
            self.msgs.lines.push(
                "The way ahead doesn't match the map. The party holds its ground rather than press on blind."
                    .to_string(),
            );
            self.state.distance_remaining = 0;
            self.state.destination_id = self.state.location_id.clone();
            return;
        };

        let dest = self.state.map_nodes[d_idx].clone();
        self.state.location_id = dest.node_id.clone();
        self.state.distance_remaining = 0;
        self.msgs.lines.push(format!("Arrived at {}!", dest.name));

        if dest.water_available {
            let old_water = self.state.supplies.water();
            let refill = std::cmp::min(20, 50 - old_water);
            if refill > 0 {
                self.state.supplies.set_water(old_water + refill);
                self.msgs
                    .lines
                    .push(format!("Found water. +{refill} water."));
            }
        }

        if let Some(cache) = dest.cache_supplies.clone() {
            if !cache.is_empty() {
                self.state.supplies.apply_delta(&cache);
                let cache_items = ordered_cache_items(&cache)
                    .into_iter()
                    .map(|(k, v)| format!("+{v} {k}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.msgs
                    .lines
                    .push(format!("Found a supply cache! {cache_items}"));
                *self
                    .diagnostics
                    .entry("caches_found".to_string())
                    .or_insert(0) += 1;
                self.state.map_nodes[d_idx].cache_supplies = None;
            }
        }

        let updated_dest = self.state.map_nodes[d_idx].clone();
        emit_arrival_card(&mut self.state, &updated_dest);

        if updated_dest.is_town {
            let day = self.state.day;
            self.record_milestone(
                "town:arrival",
                &format!("Arrived at {}", updated_dest.name),
                &format!("The party reached {} on day {day}.", updated_dest.name),
                vec!["town".to_string(), "arrival".to_string()],
            );

            let doc_mods = doctrine_modifiers(&self.state.doctrine);
            let trade_chance = 0.30 + doc_mods.trade_bonus.unwrap_or(0.0);
            if self.state.party.morale > 60 && self.rng.random() < trade_chance {
                let food_offer = self.rng.randint(3, 9) as i32;
                let cur_food = self.state.supplies.food();
                self.state.supplies.set_food(cur_food + food_offer);
                self.msgs
                    .lines
                    .push(format!("Traded at the settlement. +{food_offer} food."));
            } else {
                self.msgs
                    .lines
                    .push("This is a settlement. Supplies may be available.".to_string());
            }
        }

        if updated_dest.connections.len() == 1 {
            let next_id = &updated_dest.connections[0];
            if next_id != &updated_dest.node_id
                && self.state.map_nodes.iter().any(|n| &n.node_id == next_id)
            {
                self.state.destination_id = next_id.clone();
                self.state.distance_remaining =
                    updated_dest.distance_to.get(next_id).copied().unwrap_or(15);
            }
        }
    }

    fn advance_time(&mut self) {
        match self.state.time_of_day {
            TimeOfDay::Morning => self.state.time_of_day = TimeOfDay::Afternoon,
            TimeOfDay::Afternoon => self.state.time_of_day = TimeOfDay::Evening,
            TimeOfDay::Evening => self.state.time_of_day = TimeOfDay::Night,
            TimeOfDay::Night => {
                self.state.time_of_day = TimeOfDay::Morning;
                self.state.day += 1;
            }
        }
    }

    fn check_game_over_transition(&mut self) {
        if let Some(result) = check_game_over(&self.state) {
            if result == "VICTORY" {
                self.state.victory = true;
                self.state.game_over = true;
                self.msgs.lines.push("You made it! Victory!".to_string());
            } else {
                let cause = determine_cause_of_death(&self.state);
                self.state.cause_of_death = cause.clone();
                self.state.game_over = true;
                self.msgs
                    .lines
                    .push(format!("The journey ends. Cause: {cause}."));
            }
            self.phase = GamePhase::GameOver;
            let ending = compute_ending(&self.state);
            self.state.ending = Some(ending.clone());
            self.msgs.ending = Some(ending);
        }
    }

    /// Resolve a run that ended without a clean `GAME_OVER` transition (`step_engine.py:1420-1453`).
    pub fn finalize_run(&mut self, _reason: &str) -> EndingResult {
        if let Some(ref ending) = self.state.ending {
            return ending.clone();
        }

        if !self.state.game_over && self.phase != GamePhase::GameOver {
            self.state.game_over = true;
            if self.state.cause_of_death.is_empty() {
                self.state.cause_of_death =
                    if self.state.wagon.condition <= 0 && self.state.supplies.parts() <= 0 {
                        "Wagon failure".to_string()
                    } else {
                        "The trail".to_string()
                    };
            }
            self.phase = GamePhase::GameOver;
        }

        let ending = compute_ending(&self.state);
        self.state.ending = Some(ending.clone());
        ending
    }

    fn save(&mut self) {
        self.state.rng_counter = self.rng.counter;
        self.state.rng_state = Some(self.rng.getstate());
        if !self.autosave {
            return;
        }
        let base = self.base_path.clone().unwrap_or_else(|| PathBuf::from("."));
        let _ = save_game(&self.state, &base);
    }
}

fn ordered_cache_items(cache: &BTreeMap<String, i32>) -> Vec<(&str, i32)> {
    const TEMPLATE_ORDERS: [&[&str]; 5] = [
        &["food", "water"],
        &["parts", "cloth"],
        &["food", "meds", "ammo"],
        &["water", "salt"],
        &["food", "parts", "rope"],
    ];
    for order in TEMPLATE_ORDERS {
        if order.len() == cache.len() && order.iter().all(|k| cache.contains_key(*k)) {
            return order.iter().map(|&k| (k, cache[k])).collect();
        }
    }
    cache.iter().map(|(k, &v)| (k.as_str(), v)).collect()
}

/// Build fallback outcome narration callout (`step_engine.py:1468-1491`).
pub fn build_fallback_callout(outcome: &EventOutcome) -> String {
    let mut parts = Vec::new();
    for (key, val) in outcome.ordered_supplies_delta() {
        if val > 0 {
            parts.push(format!("+{val} {key}"));
        } else if val < 0 {
            parts.push(format!("{val} {key}"));
        }
    }

    if outcome.health_delta != 0 {
        let sign = if outcome.health_delta > 0 { "+" } else { "" };
        parts.push(format!("health {sign}{}", outcome.health_delta));
    }
    if outcome.wagon_delta != 0 {
        let sign = if outcome.wagon_delta > 0 { "+" } else { "" };
        parts.push(format!("wagon {sign}{}", outcome.wagon_delta));
    }
    if outcome.morale_delta != 0 {
        let sign = if outcome.morale_delta > 0 { "+" } else { "" };
        parts.push(format!("morale {sign}{}", outcome.morale_delta));
    }
    if outcome.time_cost != 0 {
        parts.push(format!("{} time lost", outcome.time_cost));
    }

    if parts.is_empty() {
        "No significant effect.".to_string()
    } else {
        parts.join(", ")
    }
}

/// Deterministic 'par' day count for a journey of `total_distance` miles (`step_engine.py:1505-1509`).
pub fn compute_par_days(total_distance: i32) -> i32 {
    if total_distance <= 0 {
        return PAR_DAYS_FLOOR;
    }
    let ceil_div = (total_distance + PAR_MILES_PER_DAY - 1) / PAR_MILES_PER_DAY;
    std::cmp::max(PAR_DAYS_FLOOR, ceil_div)
}

/// Best-effort deterministic read of whether the run's taboo held (`step_engine.py:1512-1548`).
pub fn taboo_kept(state: &RunState) -> bool {
    match state.taboo.as_str() {
        "" => true,
        "leave_nothing" => state.party.members.iter().all(|m| m.is_alive()),
        "never_river" => {
            for entry in &state.journal {
                let tagset: HashSet<String> = entry.tags.iter().map(|t| t.to_lowercase()).collect();
                let choice = entry.choice_made.to_lowercase();
                if (tagset.contains("river") || tagset.contains("ford")) && choice.contains("ford")
                {
                    return false;
                }
            }
            true
        }
        "never_night" => {
            for entry in &state.journal {
                if entry.tags.iter().any(|t| t.eq_ignore_ascii_case("night")) {
                    return false;
                }
            }
            true
        }
        _ => true,
    }
}

/// Count fallen members grouped by their attributed `death_cause` (`step_engine.py:1551-1558`).
pub fn deaths_by_cause(state: &RunState) -> BTreeMap<String, i32> {
    let mut counts = BTreeMap::new();
    for m in &state.party.members {
        if !m.is_alive() {
            let cause = if m.death_cause.is_empty() {
                "Unknown".to_string()
            } else {
                m.death_cause.clone()
            };
            *counts.entry(cause).or_insert(0) += 1;
        }
    }
    counts
}

/// Grade the run's ending from existing state (`step_engine.py:1561-1638`).
pub fn compute_ending(state: &RunState) -> EndingResult {
    let party_size = state.party.members.len() as i32;
    let survivors = state.party.alive_count() as i32;
    let days = state.day;
    let par_days = compute_par_days(state.total_distance);
    let kept = taboo_kept(state);
    let causes = deaths_by_cause(state);

    let causes_val: serde_json::Map<String, Value> =
        causes.into_iter().map(|(k, v)| (k, json!(v))).collect();

    let facts = BTreeMap::from([
        ("victory".to_string(), json!(state.victory)),
        ("survivors".to_string(), json!(survivors)),
        ("party_size".to_string(), json!(party_size)),
        ("days".to_string(), json!(days)),
        ("par_days".to_string(), json!(par_days)),
        ("taboo".to_string(), json!(state.taboo)),
        ("taboo_kept".to_string(), json!(kept)),
        (
            "uncanny_tokens_unspent".to_string(),
            json!(state.uncanny_tokens),
        ),
        ("deaths_by_cause".to_string(), Value::Object(causes_val)),
        ("distance".to_string(), json!(state.distance_traveled)),
        ("total_distance".to_string(), json!(state.total_distance)),
        ("cause_of_death".to_string(), json!(state.cause_of_death)),
    ]);

    let (tier, headline) = if !state.victory {
        let h = if survivors >= party_size {
            "The trail turned them back.".to_string()
        } else if survivors > 0 {
            format!("{survivors} of {party_size} were still standing when the journey failed.")
        } else {
            "None of them reached the valley.".to_string()
        };
        ("lost".to_string(), h)
    } else if survivors < party_size {
        let lost = party_size - survivors;
        (
            "pyrrhic".to_string(),
            format!("The valley was reached — but {lost} did not live to see it."),
        )
    } else if days > par_days {
        (
            "weathered".to_string(),
            format!(
                "All {party_size} reached the valley, weathered and late ({days} days against {par_days})."
            ),
        )
    } else {
        let h = if kept {
            format!("All {party_size} reached the valley intact, on time, vow unbroken.")
        } else {
            format!("All {party_size} reached the valley intact, on time.")
        };
        ("triumphant".to_string(), h)
    };

    EndingResult {
        tier,
        facts,
        headline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::create_new_run;

    // preservation: H-3 (fork prompt does not burn `maintained_turns_remaining` or `escape_valve_cooldown`)
    // and H-2 (`do_abandon_cargo` formats dropped supplies from the flat delta list).
    #[test]
    fn test_fork_does_not_burn_maintenance_or_cooldown_and_broken_route_refuses() {
        let mut st = create_new_run(42);
        // Set up a fork at node 0 with distance_remaining == 0
        let n1_id = st.map_nodes[1].node_id.clone();
        let n2_id = st.map_nodes[2].node_id.clone();
        st.map_nodes[0].connections = vec![n1_id.clone(), n2_id.clone()];
        st.map_nodes[0].distance_to.insert(n1_id.clone(), 12);
        st.map_nodes[0].distance_to.insert(n2_id.clone(), 15);
        st.distance_remaining = 10;
        st.maintained_turns_remaining = 2;
        st.escape_valve_cooldown = 3;

        let mut engine = StepEngine::new(st);
        assert_eq!(engine.phase, GamePhase::Camp);

        // Now place the party at the fork with distance_remaining == 0 while still in Camp phase
        engine.state.distance_remaining = 0;
        let msgs_fork = engine.step(&PlayerIntent {
            action: IntentAction::Travel,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_eq!(engine.phase, GamePhase::Route);
        assert_eq!(msgs_fork.route_options.len(), 2);
        // H-3: neither maintenance nor cooldown decremented on entering the fork prompt
        assert_eq!(engine.state.maintained_turns_remaining, 2);
        assert_eq!(engine.state.escape_valve_cooldown, 3);

        // Choose route A -> returns to Camp with distance_remaining > 0, still without decrementing
        engine.step(&PlayerIntent {
            action: IntentAction::Choose,
            choice_id: "A".to_string(),
            pace: String::new(),
        });
        assert_eq!(engine.phase, GamePhase::Camp);
        assert_eq!(engine.state.maintained_turns_remaining, 2);
        assert_eq!(engine.state.escape_valve_cooldown, 3);

        // Actual travel step decrements both by 1
        engine.step(&PlayerIntent {
            action: IntentAction::Travel,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_eq!(engine.state.maintained_turns_remaining, 1);
        assert_eq!(engine.state.escape_valve_cooldown, 2);

        // H-2: Abandon Cargo lists dropped supplies in narration and escape valve memory card
        let mut st_cargo = create_new_run(42);
        st_cargo.wagon.condition = 25;
        st_cargo.supplies.set("salt", 4);
        st_cargo.supplies.set("cloth", 0);
        st_cargo.supplies.set("rope", 2);
        st_cargo.supplies.set("boots", 0);
        let mut eng_cargo = StepEngine::new(st_cargo);
        let msgs_cargo = eng_cargo.step(&PlayerIntent {
            action: IntentAction::AbandonCargo,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert!(msgs_cargo.lines[0].contains("Dropped: -2 salt, -1 rope."));
        assert!(eng_cargo
            .state
            .memory_cards
            .last()
            .unwrap()
            .text
            .contains("-2 salt, -1 rope"));
    }

    // preservation: `compute_ending` tiers (`triumphant`, `weathered`, `pyrrhic`, `lost`) and `finalize_run`.
    #[test]
    fn test_compute_ending_tiers_and_finalize_run() {
        let mut st = create_new_run(42);
        st.victory = true;
        st.game_over = true;
        st.day = 10;
        st.taboo = "leave_nothing".to_string();
        let ending = compute_ending(&st);
        assert_eq!(ending.tier, "triumphant");
        assert!(ending.headline.contains("vow unbroken"));

        st.day = 99;
        let weathered = compute_ending(&st);
        assert_eq!(weathered.tier, "weathered");

        st.party.members[0].health = 0;
        st.party.members[0].death_cause = "Starvation".to_string();
        let pyrrhic = compute_ending(&st);
        assert_eq!(pyrrhic.tier, "pyrrhic");

        // finalize_run on a stalled run with broken wagon and 0 parts
        let mut st_stall = create_new_run(7);
        st_stall.wagon.condition = 0;
        st_stall.supplies.set("parts", 0);
        let mut eng_stall = StepEngine::new(st_stall);
        let final_ending = eng_stall.finalize_run("stalled");
        assert_eq!(final_ending.tier, "lost");
        assert_eq!(eng_stall.state.cause_of_death, "Wagon failure");
        assert!(eng_stall.state.game_over);
    }

    // preservation: `StepEngine::do_desperate_repair` (`engine.rs:533-565`) success, failure, and
    // refusal branches and `StepEngine::do_travel` spoilage active path (`engine.rs:295-302`).
    #[test]
    fn test_do_desperate_repair_and_travel_spoilage_active_paths() {
        // 1. Refusal when wagon >= 30 or parts > 0
        let st_refuse = create_new_run(42);
        let mut eng_refuse = StepEngine::new(st_refuse);
        let msgs_refuse = eng_refuse.step(&PlayerIntent {
            action: IntentAction::DesperateRepair,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_eq!(
            msgs_refuse.lines,
            vec!["Desperate repair requires a badly damaged wagon and no spare parts."]
        );

        // 2. Active success and failure branches of do_desperate_repair
        let mut saw_ok = false;
        let mut saw_fail = false;
        for seed in 0..30i128 {
            let mut st = create_new_run(seed);
            st.wagon.condition = 20;
            st.supplies.set("parts", 0);
            st.escape_valve_cooldown = 0;
            let mut eng = StepEngine::new(st);
            let msgs = eng.step(&PlayerIntent {
                action: IntentAction::DesperateRepair,
                choice_id: String::new(),
                pace: String::new(),
            });
            assert_eq!(eng.state.escape_valve_cooldown, 3);
            assert_eq!(eng.diagnostics.get("escape_valves_used"), Some(&1));
            let last_card = eng
                .state
                .memory_cards
                .last()
                .expect("escape valve card emitted");
            assert_eq!(last_card.id, "eng_valve_desperate_repair_d1");
            if msgs.lines[0].starts_with("Desperate repair succeeded! Wagon +15.") {
                saw_ok = true;
                assert_eq!(eng.state.wagon.condition, 35);
                assert!(last_card.text.contains("Repair succeeded."));
            } else if msgs.lines[0].starts_with("Desperate repair failed! Wagon -10.") {
                saw_fail = true;
                assert_eq!(eng.state.wagon.condition, 10);
                assert!(msgs.lines[0].ends_with(" was injured in the attempt."));
                assert!(last_card.text.contains("Repair failed."));
            } else {
                panic!("unexpected desperate_repair line: {:?}", msgs.lines);
            }
        }
        assert!(saw_ok, "must hit do_desperate_repair success line");
        assert!(saw_fail, "must hit do_desperate_repair failure line");

        // 3. Active spoilage path inside do_travel when advancing to day 3 with salt == 0
        let mut st_spoil = create_new_run(42);
        st_spoil.day = 2;
        st_spoil.time_of_day = TimeOfDay::Night;
        st_spoil.supplies.set("salt", 0);
        st_spoil.supplies.set("food", 30);
        st_spoil.distance_remaining = 50;
        let mut eng_spoil = StepEngine::new(st_spoil);
        let msgs_spoil = eng_spoil.step(&PlayerIntent {
            action: IntentAction::Travel,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_eq!(eng_spoil.state.day, 3);
        assert_eq!(eng_spoil.state.last_spoilage_day, 3);
        assert!(
            msgs_spoil
                .lines
                .iter()
                .any(|l| l.starts_with("Food spoiled! Lost ") && l.ends_with(" food (no salt).")),
            "expected spoilage line in travel messages, got {:?}",
            msgs_spoil.lines
        );
    }

    #[test]
    fn test_fork_reload_stays_in_camp_and_matches_uninterrupted_seed_2() {
        use crate::save::{load_game, save_game};

        fn fork_policy_intent(eng: &StepEngine, prng: &mut CpythonRng) -> PlayerIntent {
            if eng.phase == GamePhase::Event {
                let ids: Vec<String> = if eng.pending_event_choices.is_empty() {
                    vec!["A".to_string()]
                } else {
                    eng.pending_event_choices
                        .iter()
                        .map(|c| c.id.clone())
                        .collect()
                };
                let pick = prng.choice(&ids).clone();
                return PlayerIntent {
                    action: IntentAction::Choose,
                    choice_id: pick,
                    pace: String::new(),
                };
            }
            if eng.phase == GamePhase::Route {
                let count = std::cmp::max(1, eng.pending_routes.len());
                let opts: Vec<&str> = ["A", "B", "C", "D"][..count].to_vec();
                let pick = (*prng.choice(&opts)).to_string();
                return PlayerIntent {
                    action: IntentAction::Choose,
                    choice_id: pick,
                    pace: String::new(),
                };
            }
            if eng.state.supplies.food() < 12 && eng.state.supplies.ammo() > 0 {
                return PlayerIntent {
                    action: IntentAction::Hunt,
                    choice_id: String::new(),
                    pace: String::new(),
                };
            }
            if eng.state.wagon.condition < 50 && eng.state.supplies.parts() > 0 {
                return PlayerIntent {
                    action: IntentAction::Repair,
                    choice_id: String::new(),
                    pace: String::new(),
                };
            }
            if eng
                .state
                .party
                .members
                .iter()
                .any(|m| m.is_alive() && m.health < 45)
            {
                return PlayerIntent {
                    action: IntentAction::Rest,
                    choice_id: String::new(),
                    pace: String::new(),
                };
            }
            PlayerIntent {
                action: IntentAction::Travel,
                choice_id: String::new(),
                pace: String::new(),
            }
        }

        let mut st = create_new_run(2);
        st.run_id = format!("{}-00000000", RunState::seed_prefix(2));
        let mut eng = StepEngine::new(st);
        let mut prng = CpythonRng::from_seed(2 ^ 0xA5A5A5A5);

        for _ in 0..8 {
            let intent = fork_policy_intent(&eng, &mut prng);
            eng.step(&intent);
        }

        let node = current_node(&eng.state).expect("current node");
        assert!(node.connections.len() > 1);
        assert_eq!(eng.state.distance_remaining, 0);
        assert_eq!(eng.phase, GamePhase::Camp);

        let tmp = tempfile::tempdir().expect("tempdir");
        save_game(&eng.state, tmp.path()).expect("save at fork");
        let loaded = load_game(tmp.path()).expect("load at fork");
        let mut reloaded = StepEngine::new(loaded);

        assert_eq!(reloaded.phase, GamePhase::Camp);
        assert_eq!(reloaded.state.rng_counter, eng.state.rng_counter);

        let mut prng_rel = prng.clone();
        for step_idx in 1..=30 {
            let intent_u = fork_policy_intent(&eng, &mut prng);
            let intent_r = fork_policy_intent(&reloaded, &mut prng_rel);
            assert_eq!(intent_r, intent_u, "step {step_idx}: intent diverged");
            let msgs_u = eng.step(&intent_u);
            let msgs_r = reloaded.step(&intent_r);
            assert_eq!(reloaded.phase, eng.phase, "step {step_idx}: phase diverged");
            assert_eq!(
                msgs_r.lines, msgs_u.lines,
                "step {step_idx}: lines diverged"
            );
            assert_eq!(
                reloaded.state.rng_counter, eng.state.rng_counter,
                "step {step_idx}: rng_counter diverged"
            );
        }
    }

    #[test]
    fn test_non_travel_and_travel_deaths_emit_message_milestone_and_memory_card() {
        use crate::model::Condition;
        use crate::worldgen::{ChoiceTemplate, EventCategory};

        fn assert_single_death(
            eng: &StepEngine,
            msgs: &StepMessages,
            member_name: &str,
            expected_cause: &str,
        ) {
            let member = eng
                .state
                .party
                .members
                .iter()
                .find(|m| m.name == member_name)
                .expect("member found");
            assert!(!member.is_alive());
            assert_eq!(member.death_cause, expected_cause);

            let expected_line = format!("{member_name} has died ({expected_cause}).");
            let death_lines: Vec<&String> = msgs
                .lines
                .iter()
                .filter(|l| l.contains(" has died ("))
                .collect();
            assert_eq!(death_lines, vec![&expected_line]);

            let died_effects: Vec<&HealthEffect> = msgs
                .effects
                .iter()
                .filter(|e| matches!(e, HealthEffect::Died { .. }))
                .collect();
            assert_eq!(
                died_effects,
                vec![&HealthEffect::Died {
                    member: member_name.to_string(),
                    cause: expected_cause.to_string(),
                }]
            );

            let death_milestones: Vec<&JournalEntry> = eng
                .state
                .journal
                .iter()
                .filter(|j| j.event_id.starts_with("death:"))
                .collect();
            assert_eq!(death_milestones.len(), 1);
            assert_eq!(
                death_milestones[0].event_id,
                format!("death:{}", expected_cause.to_lowercase())
            );
            assert_eq!(
                death_milestones[0].scene_title,
                format!("Death: {member_name}")
            );

            let card_id = format!(
                "eng_death_{}_d{}",
                member_name.to_lowercase(),
                eng.state.day
            );
            let death_cards: Vec<_> = eng
                .state
                .memory_cards
                .iter()
                .filter(|c| c.id == card_id)
                .collect();
            assert_eq!(death_cards.len(), 1);
            assert_eq!(death_cards[0].title, format!("{member_name}'s Death"));
            assert_eq!(
                death_cards[0].text,
                format!(
                    "{member_name} perished on day {} ({expected_cause}).",
                    eng.state.day
                )
            );
        }

        // 1. EVENT death: lethal health_delta (-15) on Ruth (health 10) -> "Injury", +1 RNG draw
        let mut st_ev = create_new_run(42);
        assert_eq!(st_ev.party.members[0].name, "Ruth");
        st_ev.party.members[0].health = 10;
        let mut eng_ev = StepEngine::new(st_ev);
        let ev = EventSkeleton {
            event_id: "test_lethal_ambush".to_string(),
            title: "Rockfall".to_string(),
            category: EventCategory::Survival,
            tags: vec!["danger".to_string()],
            severity: "high".to_string(),
            family: String::new(),
            biome_filter: None,
            weather_filter: None,
            time_filter: None,
            preconditions: Vec::new(),
            base_weight: 1.0,
            folklore_type: None,
            costs_uncanny_token: false,
            oregon_nod: false,
            fallback_narration: "Boulders crash down the slope.".to_string(),
            fallback_choices: vec![ChoiceTemplate {
                choice_id: "A".to_string(),
                label: "Shield the wagon".to_string(),
                action: "CHOOSE".to_string(),
                style: "BOLD".to_string(),
                risk_hint: "Dangerous".to_string(),
                cost_hint: "Health".to_string(),
                outcome_weights: BTreeMap::new(),
                outcome_sentence: None,
            }],
            outcome_templates: BTreeMap::from([(
                "A".to_string(),
                EventOutcome {
                    health_delta: -15,
                    morale_delta: -5,
                    ..Default::default()
                },
            )]),
            gm_aside: String::new(),
        };
        eng_ev.phase = GamePhase::Event;
        eng_ev.pending_event = Some(ev.clone());
        eng_ev.pending_event_choices = vec![EventChoiceInfo {
            id: "A".to_string(),
            label: "Shield the wagon".to_string(),
            risk_hint: "Dangerous".to_string(),
            cost_hint: "Health".to_string(),
            ..Default::default()
        }];
        eng_ev.pending_event_title = ev.title.clone();
        eng_ev.pending_event_narration = ev.fallback_narration.clone();
        assert_eq!(eng_ev.rng.counter, 198);
        let msgs_ev = eng_ev.step(&PlayerIntent {
            action: IntentAction::Choose,
            choice_id: "A".to_string(),
            pace: String::new(),
        });
        assert_single_death(&eng_ev, &msgs_ev, "Ruth", "Injury");
        assert_eq!(eng_ev.rng.counter, 199);

        // 2. HUNT death: failed hunt injury on Ruth (health 5) -> "Injury", 4 RNG draws
        let mut st_hunt = create_new_run(42);
        st_hunt.party.members[0].health = 5;
        st_hunt.supplies.set("ammo", 10);
        let mut eng_hunt = StepEngine::new(st_hunt);
        eng_hunt.rng = CpythonRng::from_seed(15);
        let msgs_hunt = eng_hunt.step(&PlayerIntent {
            action: IntentAction::Hunt,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_single_death(&eng_hunt, &msgs_hunt, "Ruth", "Injury");
        assert_eq!(eng_hunt.rng.counter, 4);

        // 3. DESPERATE_REPAIR death: failed repair injury on Ruth (health 5) -> "Injury", 3 RNG draws
        let mut st_dr = create_new_run(42);
        st_dr.wagon.condition = 20;
        st_dr.supplies.set("parts", 0);
        st_dr.party.members[0].health = 5;
        let mut eng_dr = StepEngine::new(st_dr);
        eng_dr.rng = CpythonRng::from_seed(2);
        let msgs_dr = eng_dr.step(&PlayerIntent {
            action: IntentAction::DesperateRepair,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_single_death(&eng_dr, &msgs_dr, "Ruth", "Injury");
        assert_eq!(eng_dr.rng.counter, 3);

        // 4. HARD_RATION death: -5 HP cost on Ruth (health 5, Exhausted) -> "Exhaustion", 0 RNG draws
        let mut st_hr = create_new_run(42);
        st_hr.supplies.set("food", 4);
        st_hr.party.members[0].health = 5;
        st_hr.party.members[0].condition = Condition::Exhausted;
        let mut eng_hr = StepEngine::new(st_hr);
        assert_eq!(eng_hr.rng.counter, 198);
        let msgs_hr = eng_hr.step(&PlayerIntent {
            action: IntentAction::HardRation,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_single_death(&eng_hr, &msgs_hr, "Ruth", "Exhaustion");
        assert_eq!(eng_hr.rng.counter, 198);

        // 5. TRAVEL death: starvation on Ruth (health 5, food 0) -> "Starvation"
        let mut st_tr = create_new_run(42);
        st_tr.party.members[0].health = 5;
        st_tr.supplies.set("food", 0);
        st_tr.supplies.set("water", 50);
        let mut eng_tr = StepEngine::new(st_tr);
        let msgs_tr = eng_tr.step(&PlayerIntent {
            action: IntentAction::Travel,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_single_death(&eng_tr, &msgs_tr, "Ruth", "Starvation");

        // 6. REST and REPAIR death emission via emit_unreported_deaths (0 extra RNG draws)
        let mut st_rest = create_new_run(42);
        st_rest.party.members[0].health = 5;
        st_rest.party.members[0].condition = Condition::Sick;
        let mut eng_rest = StepEngine::new(st_rest);
        eng_rest.alive_before_step = vec![0, 1, 2, 3];
        eng_rest.emitted_death_indices.clear();
        eng_rest.state.party.members[0].health = 0;
        let counter_before_rest = eng_rest.rng.counter;
        eng_rest.emit_unreported_deaths(false);
        let msgs_rest = eng_rest.msgs.clone();
        assert_single_death(&eng_rest, &msgs_rest, "Ruth", "Disease");
        assert_eq!(eng_rest.rng.counter, counter_before_rest);

        let mut st_rep = create_new_run(42);
        st_rep.party.members[0].health = 5;
        st_rep.party.members[0].condition = Condition::Exhausted;
        let mut eng_rep = StepEngine::new(st_rep);
        eng_rep.alive_before_step = vec![0, 1, 2, 3];
        eng_rep.emitted_death_indices.clear();
        eng_rep.state.party.members[0].health = 0;
        let counter_before_rep = eng_rep.rng.counter;
        eng_rep.emit_unreported_deaths(false);
        let msgs_rep = eng_rep.msgs.clone();
        assert_single_death(&eng_rep, &msgs_rep, "Ruth", "Exhaustion");
        assert_eq!(eng_rep.rng.counter, counter_before_rep);
    }

    #[test]
    fn test_do_travel_singular_one_mile_remaining() {
        let mut st = create_new_run(42);
        st.distance_remaining = 6;
        let mut eng = StepEngine::new(st);
        let msgs = eng.step(&PlayerIntent {
            action: IntentAction::Travel,
            choice_id: String::new(),
            pace: String::new(),
        });
        assert_eq!(eng.state.distance_remaining, 1);
        assert_eq!(msgs.lines[0], "Traveled 5 miles. 1 mile remaining.");
    }
}
