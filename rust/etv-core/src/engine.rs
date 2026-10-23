//! Step-based deterministic turn engine (`step_engine.py`).
//!
//! Exact port of `src/escape_the_valley/step_engine.py` at tag `v1.3.1` (`47b01a3`)
//! with the GM off (`gm_config.enabled == false`).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::{apply_outcome, build_event_library, resolve_event, select_event};
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
    check_night_travel_danger, check_spoilage, compute_daily_consumption, compute_travel_distance,
    current_node, desperate_repair, determine_cause_of_death, halve_consumption, hard_ration,
    rest_day, update_morale, HealthEffect,
};
use crate::rng::CpythonRng;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventChoiceInfo {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub risk_hint: String,
    #[serde(default)]
    pub cost_hint: String,
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
    autosave: bool,
    base_path: Option<PathBuf>,
    pending_event: Option<EventSkeleton>,
    pending_event_choices: Vec<EventChoiceInfo>,
    pending_event_title: String,
    pending_event_narration: String,
    pending_routes: Vec<RouteOption>,
}

impl StepEngine {
    /// Construct a `StepEngine` with `autosave = false` (pure in-memory).
    pub fn new(state: RunState) -> Self {
        Self::with_options(state, false, None)
    }

    /// Return the currently pending `EventSkeleton`, if the engine is in `GamePhase::Event`.
    pub fn pending_event(&self) -> Option<&EventSkeleton> {
        self.pending_event.as_ref()
    }

    /// Construct a `StepEngine` with explicit `autosave` and `base_path` options (`step_engine.py:161-230`).
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
            autosave,
            base_path,
            pending_event: None,
            pending_event_choices: Vec::new(),
            pending_event_title: String::new(),
            pending_event_narration: String::new(),
            pending_routes: Vec::new(),
        };
        engine.check_initial_phase();
        engine
    }

    /// Set phase based on current state (`step_engine.py:231-250`).
    fn check_initial_phase(&mut self) {
        if self.state.game_over {
            self.phase = GamePhase::GameOver;
            return;
        }

        if let Some(node) = current_node(&self.state).cloned() {
            if node.connections.len() > 1
                && self.state.distance_remaining <= 0
                && self.build_route_choices(&node)
            {
                self.phase = GamePhase::Route;
            }
        }
    }

    /// Process one player action (`step_engine.py:251-281`).
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

        match self.phase {
            GamePhase::Event => self.handle_event_choice(intent),
            GamePhase::Route => self.handle_route_choice(intent),
            GamePhase::Camp => self.handle_camp(intent),
            GamePhase::GameOver => {}
        }

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
            self.msgs.lines.push(format!(
                "Traveled {distance} miles. {} miles remaining.",
                self.state.distance_remaining
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
                HealthEffect::Starvation { .. } | HealthEffect::Dehydration { .. } => {}
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
        }

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

        let event = select_event(&mut self.state, &mut self.rng, &self.event_library, weather);
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
            .map(|c| EventChoiceInfo {
                id: c.choice_id.clone(),
                label: c.label.clone(),
                risk_hint: c.risk_hint.clone(),
                cost_hint: c.cost_hint.clone(),
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

    fn re_present_event(&mut self) {
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

        let outcome = resolve_event(&mut self.state, &event, choice_id, &mut self.rng);
        apply_outcome(&mut self.state, &outcome);

        for _ in 0..std::cmp::max(0, outcome.time_cost) {
            self.advance_time();
        }

        let choice_label = self
            .pending_event_choices
            .iter()
            .find(|c| &c.id == choice_id)
            .map(|c| c.label.clone())
            .unwrap_or_default();

        let outcome_title = "Outcome".to_string();
        let outcome_narration = build_fallback_callout(&outcome);

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
}
