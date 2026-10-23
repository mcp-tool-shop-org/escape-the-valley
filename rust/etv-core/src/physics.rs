//! Deterministic survival physics — consumption, health, breakdown (`physics.py`).
//!
//! Exact port of `src/escape_the_valley/physics.py` at tag `v1.3.1` (`47b01a3`).

use std::collections::BTreeMap;

use crate::model::{
    doctrine_modifiers, Biome, Condition, MapNode, Pace, PartyMember, RunState, TimeOfDay, Trait,
    TwistModifier,
};
use crate::rng::CpythonRng;

/// Base travel distance per step by pace (`physics.py:18`).
pub fn pace_distance(pace: Pace) -> i32 {
    match pace {
        Pace::Slow => 3,
        Pace::Steady => 5,
        Pace::Hard => 8,
    }
}

/// Consumption multiplier by pace (`physics.py:19`).
pub fn pace_consumption(pace: Pace) -> f64 {
    match pace {
        Pace::Slow => 0.8,
        Pace::Steady => 1.0,
        Pace::Hard => 1.3,
    }
}

/// Breakdown chance bonus (percentage points) by pace (`physics.py:20`).
pub fn pace_breakdown_bonus(pace: Pace) -> i32 {
    match pace {
        Pace::Slow => 0,
        Pace::Steady => 1,
        Pace::Hard => 3,
    }
}

/// How far through the journey: `0.0` (start) to `1.0` (destination) (`physics.py:23-30`).
pub fn journey_pressure(state: &RunState) -> f64 {
    if state.total_distance <= 0 {
        return 0.0;
    }
    ((state.distance_traveled as f64) / (state.total_distance as f64)).min(1.0)
}

/// Compute daily food/water/firewood/lantern_oil consumption (`physics.py:33-89`).
pub fn compute_daily_consumption(state: &RunState, is_travel: bool) -> BTreeMap<String, i32> {
    let alive = state.party.alive_count() as i32;
    if alive == 0 {
        return BTreeMap::from([("food".to_string(), 0), ("water".to_string(), 0)]);
    }

    let pace_mod = pace_consumption(state.wagon.pace);

    // Scarcity curve: late-game consumption scales up to 1.12x
    let pressure = journey_pressure(state);
    let consumption_scale = 1.0 + 0.12 * pressure;

    // Doctrine modifier
    let doc_mods = doctrine_modifiers(&state.doctrine);
    let doc_mult = doc_mods.consumption_mult.unwrap_or(1.0);

    // Base: 2 food, 2 water per person per day.
    // Each step is one quarter-day, so divide by 4.
    let raw_food = 2.0 * (alive as f64) * pace_mod * consumption_scale * doc_mult / 4.0;
    let raw_water = 2.0 * (alive as f64) * pace_mod * consumption_scale * doc_mult / 4.0;
    let mut food = std::cmp::max(1, raw_food as i32);
    let mut water = std::cmp::max(1, raw_water as i32);

    // Hard ration halves food/water
    if state.rationing_steps > 0 {
        food = std::cmp::max(1, food / 2);
        water = std::cmp::max(1, water / 2);
    }

    // Biome modifiers
    let node = current_node(state);
    if let Some(n) = node {
        if n.biome == Biome::Desert {
            water = ((water as f64) * 1.5) as i32;
        } else if n.biome == Biome::Alpine {
            food = ((food as f64) * 1.2) as i32;
        }
    }

    let mut deltas = BTreeMap::from([("food".to_string(), -food), ("water".to_string(), -water)]);

    // Firewood: 1 per night camp (evening/night time of day)
    if matches!(state.time_of_day, TimeOfDay::Evening | TimeOfDay::Night) {
        let mut fw = 1;
        if node.is_some_and(|n| n.biome == Biome::Alpine) {
            fw = 2;
        }
        deltas.insert("firewood".to_string(), -fw);
    }

    // Lantern oil: 1 per night travel
    if is_travel && matches!(state.time_of_day, TimeOfDay::Evening | TimeOfDay::Night) {
        deltas.insert("lantern_oil".to_string(), -1);
    }

    deltas
}

/// Halve a consumption-delta dict for a half-day action (`HUNT`/`REPAIR`), rounding toward zero (`physics.py:91-107`).
pub fn halve_consumption(deltas: &BTreeMap<String, i32>) -> BTreeMap<String, i32> {
    deltas.iter().map(|(k, &v)| (k.clone(), v / 2)).collect()
}

/// How far the party travels in one step based on pace and conditions (`physics.py:110-135`).
pub fn compute_travel_distance(state: &RunState) -> i32 {
    let mut base = pace_distance(state.wagon.pace);

    let doc_mods = doctrine_modifiers(&state.doctrine);
    let penalty = doc_mods.distance_penalty.unwrap_or(0);
    base = std::cmp::max(1, base - penalty);

    if state.wagon.condition < 30 {
        base = std::cmp::max(1, base - 2);
    } else if state.wagon.condition < 60 {
        base = std::cmp::max(1, base - 1);
    }

    if state.wagon.animals_health < 40 {
        base = std::cmp::max(1, base - 2);
    }

    if state.twists.contains(&TwistModifier::EarlyWinter) {
        base = std::cmp::max(1, base - 1);
    }

    base
}

/// Check if the wagon breaks down (`physics.py:137-169`). Returns `Some(wagon_damage)` or `None`.
pub fn check_breakdown(state: &RunState, rng: &mut CpythonRng) -> Option<i32> {
    let node = current_node(state);
    let hazard = node.map(|n| n.hazard).unwrap_or(5);

    let mut chance =
        (hazard as f64) / 100.0 + (pace_breakdown_bonus(state.wagon.pace) as f64) / 100.0;

    let doc_mods = doctrine_modifiers(&state.doctrine);
    chance += doc_mods.breakdown_bonus.unwrap_or(0.0);

    let cond = state.wagon.condition;
    if cond > 60 {
        chance *= 0.3;
    } else if cond < 30 {
        chance *= 1.8;
    }

    if state.maintained_turns_remaining > 0 {
        chance *= 0.3;
    }

    if rng.random() < chance {
        let mut damage = rng.randint(8, 16) as i32;
        if state.party.has_trait(Trait::Mechanic) {
            damage = std::cmp::max(5, damage - 8);
        }
        return Some(damage);
    }

    None
}

/// Apply wagon breakdown. Auto-spends parts if available (`physics.py:172-180`).
pub fn apply_breakdown(state: &mut RunState, damage: i32) -> BTreeMap<String, i32> {
    state.wagon.condition = std::cmp::max(0, state.wagon.condition - damage);
    if state.supplies.parts() > 0 {
        state.supplies.set_parts(state.supplies.parts() - 1);
        let repair = std::cmp::min(damage, 15);
        state.wagon.condition = std::cmp::min(100, state.wagon.condition + repair);
        return BTreeMap::from([("parts".to_string(), -1)]);
    }
    BTreeMap::new()
}

/// Single health effect entry emitted by [`check_health_effects`] (`physics.py:183-237`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthEffect {
    Starvation { member: String, health_delta: i32 },
    Dehydration { member: String, health_delta: i32 },
    FellSick { member: String },
    Healed { member: String },
    Died { member: String, cause: String },
}

/// Check for illness/injury effects on party members (`physics.py:183-237`).
pub fn check_health_effects(state: &mut RunState, rng: &mut CpythonRng) -> Vec<HealthEffect> {
    let mut effects = Vec::new();
    let is_swamp = current_node(state).is_some_and(|n| n.biome == Biome::Swamp);

    for i in 0..state.party.members.len() {
        if !state.party.members[i].is_alive() {
            continue;
        }

        // Starvation
        if state.supplies.food() <= 0 {
            let loss = rng.randint(5, 15) as i32;
            state.party.members[i].health = std::cmp::max(0, state.party.members[i].health - loss);
            effects.push(HealthEffect::Starvation {
                member: state.party.members[i].name.clone(),
                health_delta: -10,
            });
        }

        // Dehydration (worse than hunger)
        if state.supplies.water() <= 0 {
            let loss = rng.randint(8, 20) as i32;
            state.party.members[i].health = std::cmp::max(0, state.party.members[i].health - loss);
            effects.push(HealthEffect::Dehydration {
                member: state.party.members[i].name.clone(),
                health_delta: -15,
            });
        }

        // Random sickness chance
        let mut sick_chance = 0.03;
        if state.party.morale < 30 {
            sick_chance += 0.03;
        }
        if state.party.members[i].condition == Condition::Exhausted {
            sick_chance += 0.05;
        }
        if is_swamp {
            sick_chance += 0.04;
        }

        if state.party.members[i].condition == Condition::Healthy && rng.random() < sick_chance {
            state.party.members[i].condition = Condition::Sick;
            effects.push(HealthEffect::FellSick {
                member: state.party.members[i].name.clone(),
            });
        }

        // Sick members lose health unless treated
        if state.party.members[i].condition == Condition::Sick {
            let loss = rng.randint(3, 8) as i32;
            state.party.members[i].health = std::cmp::max(0, state.party.members[i].health - loss);

            // Healer trait helps (evaluated dynamically after health loss, matching Python)
            if state.party.has_trait(Trait::Healer) && state.supplies.meds() > 0 {
                state.party.members[i].condition = Condition::Healthy;
                state.party.members[i].health =
                    std::cmp::min(100, state.party.members[i].health + 10);
                state.supplies.set_meds(state.supplies.meds() - 1);
                effects.push(HealthEffect::Healed {
                    member: state.party.members[i].name.clone(),
                });
            }
        }

        // Death check
        if state.party.members[i].health <= 0 {
            let cause = proximate_death_cause(&state.party.members[i], state, false);
            state.party.members[i].death_cause = cause.clone();
            effects.push(HealthEffect::Died {
                member: state.party.members[i].name.clone(),
                cause,
            });
        }
    }

    effects
}

/// Result of [`attempt_hunt`] (`physics.py:240-296`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HuntResult {
    pub supplies_delta: BTreeMap<String, i32>,
    pub injured: Option<String>,
}

/// Attempt to hunt. Costs ammo, may yield food, may cause injury (`physics.py:240-296`).
pub fn attempt_hunt(state: &mut RunState, rng: &mut CpythonRng) -> HuntResult {
    if state.supplies.ammo() <= 0 {
        return HuntResult::default();
    }

    let mut deltas = BTreeMap::from([("ammo".to_string(), -1)]);

    let mut success_chance = 0.5;
    let biome = current_node(state).map(|n| n.biome);
    if let Some(b) = biome {
        if b == Biome::Forest {
            success_chance += 0.15;
        } else if b == Biome::Plains {
            success_chance += 0.10;
        } else if b == Biome::Desert {
            success_chance -= 0.20;
        }
    }

    if state.party.has_trait(Trait::Tracker) {
        success_chance += 0.15;
    }
    if state.party.has_trait(Trait::SharpEye) {
        success_chance += 0.10;
    }

    let doc_mods = doctrine_modifiers(&state.doctrine);
    success_chance += doc_mods.hunt_bonus.unwrap_or(0.0);

    let mut injured = None;
    if rng.random() < success_chance {
        let mut food_gained = if biome == Some(Biome::Forest) {
            rng.randint(8, 18) as i32
        } else if biome == Some(Biome::Plains) {
            rng.randint(7, 16) as i32
        } else {
            rng.randint(5, 12) as i32
        };

        if state.party.morale > 60 && rng.random() < 0.10 {
            food_gained *= 2;
        }

        deltas.insert("food".to_string(), food_gained);
    } else if rng.random() < 0.15 {
        let alive_indices: Vec<usize> = state
            .party
            .members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.is_alive())
            .map(|(i, _)| i)
            .collect();
        if !alive_indices.is_empty() {
            let &unlucky_idx = rng.choice(&alive_indices);
            let dmg = rng.randint(5, 15) as i32;
            state.party.members[unlucky_idx].health =
                std::cmp::max(0, state.party.members[unlucky_idx].health - dmg);
            state.party.members[unlucky_idx].condition = Condition::Injured;
            injured = Some(state.party.members[unlucky_idx].name.clone());
        }
    }

    HuntResult {
        supplies_delta: deltas,
        injured,
    }
}

/// Attempt wagon repair using parts (`physics.py:299-317`).
pub fn attempt_repair(state: &mut RunState) -> BTreeMap<String, i32> {
    if state.supplies.parts() <= 0 {
        return BTreeMap::new();
    }

    let mut repair_amount = if state.party.has_trait(Trait::Mechanic) {
        30
    } else {
        20
    };
    if state.supplies.get("tools") > 0 {
        repair_amount += 10;
    }

    let doc_mods = doctrine_modifiers(&state.doctrine);
    let repair_mult = doc_mods.repair_mult.unwrap_or(1.0);
    repair_amount = ((repair_amount as f64) * repair_mult) as i32;

    state.wagon.condition = std::cmp::min(100, state.wagon.condition + repair_amount);
    BTreeMap::from([("parts".to_string(), -1)])
}

/// Party rests for a day. Health recovers, morale improves (`physics.py:320-337`).
/// Returns list of member names who recovered from `Sick` or `Injured`.
pub fn rest_day(state: &mut RunState, rng: &mut CpythonRng) -> Vec<String> {
    let mut recovered = Vec::new();
    for member in &mut state.party.members {
        if !member.is_alive() {
            continue;
        }

        let heal = rng.randint(3, 10) as i32;
        member.health = std::cmp::min(100, member.health + heal);

        if matches!(member.condition, Condition::Sick | Condition::Injured) && rng.random() < 0.3 {
            member.condition = Condition::Healthy;
            recovered.push(member.name.clone());
        }
    }

    let morale_boost = rng.randint(3, 8) as i32;
    state.party.morale = std::cmp::min(100, state.party.morale + morale_boost);
    recovered
}

/// Update morale based on conditions (`physics.py:340-373`).
pub fn update_morale(state: &mut RunState, event_mood: i32) {
    let mut delta = event_mood;
    let alive = state.party.alive_count() as i32;

    if state.supplies.food() < alive * 3 {
        delta -= 3;
    }
    if state.supplies.water() < alive * 3 {
        delta -= 4;
    }

    let dead_count = state.party.members.iter().filter(|m| !m.is_alive()).count() as i32;
    delta -= dead_count * 8;

    if state.wagon.condition > 70 {
        delta += 1;
    }

    if state.wagon.pace == Pace::Hard {
        delta -= 2;
    }

    let doc_mods = doctrine_modifiers(&state.doctrine);
    let morale_floor = doc_mods.morale_floor.unwrap_or(0);

    state.party.morale =
        std::cmp::max(morale_floor, std::cmp::min(100, state.party.morale + delta));

    if state.party.morale < 15 {
        for m in &mut state.party.members {
            if m.is_alive() && m.condition == Condition::Healthy {
                m.condition = Condition::Exhausted;
            }
        }
    }
}

/// If `salt == 0` and `day % 3 == 0`, some food spoils — at most once per qualifying day (`physics.py:375-397`).
pub fn check_spoilage(state: &mut RunState, rng: &mut CpythonRng) -> BTreeMap<String, i32> {
    if state.supplies.get("salt") > 0 || state.day % 3 != 0 {
        return BTreeMap::new();
    }
    if state.day == state.last_spoilage_day {
        return BTreeMap::new();
    }

    state.last_spoilage_day = state.day;
    let loss = rng.randint(2, 4) as i32;
    let actual = std::cmp::min(loss, state.supplies.food());
    if actual > 0 {
        return BTreeMap::from([("food".to_string(), -actual)]);
    }
    BTreeMap::new()
}

/// Night travel without lantern oil: 15% extra wagon damage (`physics.py:400-416`).
pub fn check_night_travel_danger(state: &RunState, rng: &mut CpythonRng) -> Option<i32> {
    if !matches!(state.time_of_day, TimeOfDay::Evening | TimeOfDay::Night) {
        return None;
    }
    if state.supplies.get("lantern_oil") > 0 {
        return None;
    }

    if rng.random() < 0.15 {
        let damage = rng.randint(5, 15) as i32;
        return Some(damage);
    }
    None
}

/// Check if the game is over (`physics.py:419-433`). Returns cause or `"VICTORY"` or `None`.
pub fn check_game_over(state: &RunState) -> Option<String> {
    if state.party.alive_count() == 0 {
        return Some("All party members have perished.".to_string());
    }

    if state.wagon.condition <= 0 && state.supplies.parts() <= 0 {
        return Some("Wagon destroyed beyond repair with no parts remaining.".to_string());
    }

    if let Some(final_node) = state.map_nodes.last() {
        if state.location_id == final_node.node_id && state.distance_remaining <= 0 {
            return Some("VICTORY".to_string());
        }
    }

    None
}

/// Escape valve availability: wagon must be `< 30` and cooldown `<= 0` (`physics.py:439-441`).
pub fn can_abandon_cargo(state: &RunState) -> bool {
    state.wagon.condition < 30 && state.escape_valve_cooldown <= 0
}

/// Escape valve availability: wagon `< 30` and `parts <= 0` and cooldown `<= 0` (`physics.py:444-450`).
pub fn can_desperate_repair(state: &RunState) -> bool {
    state.wagon.condition < 30 && state.supplies.parts() <= 0 && state.escape_valve_cooldown <= 0
}

/// Escape valve availability: food `< alive * 3` and not already rationing and cooldown `<= 0` (`physics.py:453-461`).
pub fn can_hard_ration(state: &RunState) -> bool {
    let alive = state.party.alive_count() as i32;
    alive > 0
        && state.supplies.food() < alive * 3
        && state.rationing_steps <= 0
        && state.escape_valve_cooldown <= 0
}

/// Drop non-essential supplies to lighten the wagon (`physics.py:467-485`).
/// Returns dropped items in Python's exact `["salt", "cloth", "rope", "boots"]` insertion order.
pub fn abandon_cargo(state: &mut RunState) -> Vec<(String, i32)> {
    let mut deltas = Vec::new();
    let droppable = ["salt", "cloth", "rope", "boots"];

    for key in droppable {
        let current = state.supplies.get(key);
        if current > 0 {
            let drop = std::cmp::max(1, current / 2);
            deltas.push((key.to_string(), -drop));
            state.supplies.set(key, current - drop);
        }
    }

    state.wagon.condition = std::cmp::min(100, state.wagon.condition + 25);
    state.party.morale = std::cmp::max(0, state.party.morale - 8);

    deltas
}

/// Result of [`desperate_repair`] (`physics.py:488-527`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesperateRepairResult {
    pub success: bool,
    pub wagon_delta: i32,
    pub cloth_used: bool,
    pub injured: String,
}

/// Risky repair without parts (`physics.py:488-527`).
pub fn desperate_repair(state: &mut RunState, rng: &mut CpythonRng) -> DesperateRepairResult {
    let mut success_chance = 0.50;
    if state.party.has_trait(Trait::Mechanic) {
        success_chance += 0.20;
    }
    if state.supplies.get("tools") > 0 {
        success_chance += 0.10;
    }

    let mut used_cloth = false;
    if state.supplies.get("cloth") > 0 {
        let cur = state.supplies.get("cloth");
        state.supplies.set("cloth", cur - 1);
        used_cloth = true;
    }

    if rng.random() < success_chance {
        state.wagon.condition = std::cmp::min(100, state.wagon.condition + 15);
        return DesperateRepairResult {
            success: true,
            wagon_delta: 15,
            cloth_used: used_cloth,
            injured: String::new(),
        };
    }

    state.wagon.condition = std::cmp::max(0, state.wagon.condition - 10);
    let alive_indices: Vec<usize> = state
        .party
        .members
        .iter()
        .enumerate()
        .filter(|(_, m)| m.is_alive())
        .map(|(i, _)| i)
        .collect();
    let mut injured_name = String::new();
    if !alive_indices.is_empty() {
        let &unlucky_idx = rng.choice(&alive_indices);
        let dmg = rng.randint(5, 15) as i32;
        state.party.members[unlucky_idx].health =
            std::cmp::max(0, state.party.members[unlucky_idx].health - dmg);
        state.party.members[unlucky_idx].condition = Condition::Injured;
        injured_name = state.party.members[unlucky_idx].name.clone();
    }

    DesperateRepairResult {
        success: false,
        wagon_delta: -10,
        cloth_used: used_cloth,
        injured: injured_name,
    }
}

/// Declare hard rations — 2 travel steps at half consumption (`physics.py:530-539`).
pub fn hard_ration(state: &mut RunState) {
    state.rationing_steps = 2;
    state.party.morale = std::cmp::max(0, state.party.morale - 10);
    for member in &mut state.party.members {
        if member.is_alive() {
            member.health = std::cmp::max(0, member.health - 5);
        }
    }
}

/// Determine what killed a party member based on current state (`physics.py:541-572`).
pub fn proximate_death_cause(member: &PartyMember, state: &RunState, from_event: bool) -> String {
    if state.supplies.water() <= 0 {
        return "Dehydration".to_string();
    }
    if state.supplies.food() <= 0 {
        return "Starvation".to_string();
    }
    if member.condition == Condition::Sick {
        return "Disease".to_string();
    }
    if member.condition == Condition::Injured {
        return "Injury".to_string();
    }
    if member.condition == Condition::Exhausted {
        return "Exhaustion".to_string();
    }
    if from_event {
        return "Injury".to_string();
    }
    "Exposure".to_string()
}

/// Derive the canonical game-over cause from state (`physics.py:575-595`).
/// Preserves Python `Counter(causes).most_common(1)[0][0]` first-seen tie-breaking.
pub fn determine_cause_of_death(state: &RunState) -> String {
    if state.wagon.condition <= 0 && state.supplies.parts() <= 0 {
        return "Wagon failure".to_string();
    }

    let mut counts: Vec<(String, usize)> = Vec::new();
    for m in &state.party.members {
        if !m.is_alive() && !m.death_cause.is_empty() {
            if let Some(entry) = counts.iter_mut().find(|(c, _)| c == &m.death_cause) {
                entry.1 += 1;
            } else {
                counts.push((m.death_cause.clone(), 1));
            }
        }
    }

    if !counts.is_empty() {
        let mut best_cause = counts[0].0.clone();
        let mut best_count = counts[0].1;
        for (cause, count) in counts.into_iter().skip(1) {
            if count > best_count {
                best_count = count;
                best_cause = cause;
            }
        }
        return best_cause;
    }

    "The trail".to_string()
}

/// Get the current map node (`physics.py:598-603`).
pub fn current_node(state: &RunState) -> Option<&MapNode> {
    state
        .map_nodes
        .iter()
        .find(|n| n.node_id == state.location_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::create_new_run;

    // preservation: truncation toward zero in `halve_consumption` and first-seen
    // tie-breaking in `determine_cause_of_death`.
    #[test]
    fn test_halve_consumption_and_determine_cause_of_death_ties() {
        let mut full = BTreeMap::new();
        full.insert("food".to_string(), -5);
        full.insert("water".to_string(), -1);
        full.insert("ammo".to_string(), 0);
        let halved = halve_consumption(&full);
        // int(-5 / 2) == -2 in Python; int(-1 / 2) == 0
        assert_eq!(halved.get("food"), Some(&-2));
        assert_eq!(halved.get("water"), Some(&0));
        assert_eq!(halved.get("ammo"), Some(&0));

        let mut st = create_new_run(42);
        // Tie between "Dehydration" (first seen) and "Starvation" (second seen) -> "Dehydration"
        st.party.members[0].health = 0;
        st.party.members[0].death_cause = "Dehydration".to_string();
        st.party.members[1].health = 0;
        st.party.members[1].death_cause = "Starvation".to_string();
        assert_eq!(determine_cause_of_death(&st), "Dehydration");

        // When wagon condition <= 0 and parts <= 0, "Wagon failure" takes precedence
        st.wagon.condition = 0;
        st.supplies.set("parts", 0);
        assert_eq!(determine_cause_of_death(&st), "Wagon failure");
    }

    // preservation: escape valve eligibility, `abandon_cargo` item drop order, and `hard_ration`.
    #[test]
    fn test_escape_valves_and_abandon_cargo_order() {
        let mut st = create_new_run(42);
        st.wagon.condition = 25;
        st.supplies.set("salt", 5);
        st.supplies.set("cloth", 4);
        st.supplies.set("rope", 3);
        st.supplies.set("boots", 1);
        assert!(can_abandon_cargo(&st));

        let dropped = abandon_cargo(&mut st);
        let keys: Vec<&str> = dropped.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["salt", "cloth", "rope", "boots"]);
        assert_eq!(
            dropped,
            vec![
                ("salt".to_string(), -2),
                ("cloth".to_string(), -2),
                ("rope".to_string(), -1),
                ("boots".to_string(), -1),
            ]
        );
        assert_eq!(st.wagon.condition, 50);

        // Hard ration requires food < alive * 3 and cooldown == 0
        st.supplies.set("food", 2);
        st.escape_valve_cooldown = 0;
        assert!(can_hard_ration(&st));
        let prev_morale = st.party.morale;
        hard_ration(&mut st);
        assert_eq!(st.rationing_steps, 2);
        assert_eq!(st.party.morale, prev_morale - 10);
    }

    // preservation: `check_spoilage` (`physics.rs:434-449`) and `desperate_repair` (`physics.rs:536-587`)
    // active paths, guards, cloth usage, trait/tools bonuses, and failure injury.
    #[test]
    fn test_desperate_repair_and_check_spoilage() {
        let mut st = create_new_run(42);
        let mut rng = CpythonRng::from_seed(42);

        // 1. check_spoilage guards: salt > 0 or day % 3 != 0 returns empty
        st.day = 3;
        st.supplies.set("salt", 2);
        st.supplies.set("food", 20);
        assert!(check_spoilage(&mut st, &mut rng).is_empty());
        assert_eq!(st.last_spoilage_day, 0);

        st.supplies.set("salt", 0);
        st.day = 4;
        assert!(check_spoilage(&mut st, &mut rng).is_empty());
        assert_eq!(st.last_spoilage_day, 0);

        // Active spoilage on day % 3 == 0 with salt == 0
        st.day = 3;
        let spoil_delta = check_spoilage(&mut st, &mut rng);
        let food_loss = *spoil_delta.get("food").expect("food key in spoilage delta");
        assert!(
            (-4..=-2).contains(&food_loss),
            "expected -4..=-2 spoilage loss, got {food_loss}"
        );
        assert_eq!(st.last_spoilage_day, 3);

        // Calling check_spoilage again on the same day is a no-op (at most once per qualifying day)
        assert!(check_spoilage(&mut st, &mut rng).is_empty());

        // Food cap when food < randint(2, 4)
        st.day = 6;
        st.supplies.set("food", 1);
        let capped_delta = check_spoilage(&mut st, &mut rng);
        assert_eq!(capped_delta.get("food"), Some(&-1));
        assert_eq!(st.last_spoilage_day, 6);

        // 2. desperate_repair: eligibility, cloth consumption, success and failure branches
        st.wagon.condition = 20;
        st.supplies.set("parts", 1);
        st.escape_valve_cooldown = 0;
        assert!(!can_desperate_repair(&st));
        st.supplies.set("parts", 0);
        assert!(can_desperate_repair(&st));

        st.supplies.set("cloth", 1);
        st.supplies.set("tools", 1);
        st.party.members[0].traits = vec![Trait::Mechanic];

        let mut saw_success = false;
        let mut saw_failure = false;
        for seed in 0..30i128 {
            let mut run_st = st.clone();
            run_st.wagon.condition = 20;
            run_st.supplies.set("cloth", if seed == 0 { 1 } else { 0 });
            let mut trial_rng = CpythonRng::from_seed(seed);
            let res = desperate_repair(&mut run_st, &mut trial_rng);
            if seed == 0 {
                assert!(res.cloth_used);
                assert_eq!(run_st.supplies.get("cloth"), 0);
            } else {
                assert!(!res.cloth_used);
            }
            if res.success {
                saw_success = true;
                assert_eq!(res.wagon_delta, 15);
                assert_eq!(run_st.wagon.condition, 35);
                assert!(res.injured.is_empty());
            } else {
                saw_failure = true;
                assert_eq!(res.wagon_delta, -10);
                assert_eq!(run_st.wagon.condition, 10);
                assert!(!res.injured.is_empty());
                let injured_member = run_st
                    .party
                    .members
                    .iter()
                    .find(|m| m.name == res.injured)
                    .expect("injured member exists");
                assert_eq!(injured_member.condition, Condition::Injured);
            }
        }
        assert!(saw_success, "must exercise desperate_repair success branch");
        assert!(saw_failure, "must exercise desperate_repair failure branch");
    }
}
