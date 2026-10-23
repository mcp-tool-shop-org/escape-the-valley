//! Event system — skeleton library, weighted selection, cost gates, and resolution (`events.py`).
//!
//! Exact port of `src/escape_the_valley/events.py` at tag `v1.3.1` (`47b01a3`).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::events_builtin::builtin_hand_authored_events;
use crate::model::{Biome, GMProfile, RunState, TimeOfDay, TwistModifier, Weather};
use crate::physics::{current_node, journey_pressure, proximate_death_cause};
use crate::rng::CpythonRng;
use crate::worldgen::{
    load_json_events, ChoiceTemplate, EventCategory, EventOutcome, EventSkeleton, FolkloreType,
};

const TAG_FAMILIES: [&str; 7] = [
    "river", "weather", "human", "folklore", "survival", "ford", "bridge",
];
const COOLDOWN_WINDOW: usize = 6;
const COOLDOWN_PENALTY: f64 = 0.25;

const INVENTORY_COST_KEYS: [&str; 5] = ["rope", "parts", "food", "tools", "meds"];
const DURABLE_INVENTORY: [&str; 1] = ["tools"];

static EVENT_LIBRARY_CACHE: OnceLock<Vec<EventSkeleton>> = OnceLock::new();

/// Return a shared slice of the full 260-event skeleton library (`events.py::build_event_library`).
pub fn event_library_slice() -> &'static [EventSkeleton] {
    EVENT_LIBRARY_CACHE.get_or_init(|| {
        let mut events = builtin_hand_authored_events();
        events.extend(load_json_events());
        events
    })
}

/// Build the full event skeleton library (60 hand-authored + 200 JSON events = 260 events)
/// (`events.py:86-1670`).
pub fn build_event_library() -> Vec<EventSkeleton> {
    event_library_slice().to_vec()
}

/// Profile-specific folklore weight multiplier (`events.py:1676-1680`).
pub fn profile_folklore_weight(profile: GMProfile) -> f64 {
    match profile {
        GMProfile::Chronicler => 0.5,
        GMProfile::Fireside => 1.0,
        GMProfile::Lantern => 1.6,
    }
}

/// Profile-specific uncanny weight multiplier (`events.py:1682-1686`).
pub fn profile_uncanny_weight(profile: GMProfile) -> f64 {
    match profile {
        GMProfile::Chronicler => 0.3,
        GMProfile::Fireside => 1.0,
        GMProfile::Lantern => 2.0,
    }
}

fn event_category_str(cat: EventCategory) -> &'static str {
    match cat {
        EventCategory::Survival => "survival",
        EventCategory::Human => "human",
        EventCategory::Folklore => "folklore",
        EventCategory::Big => "big",
    }
}

/// Check if an uncanny token can be spent based on profile rules (`events.py:1689-1726`).
pub fn can_spend_uncanny_token(state: &RunState, event: &EventSkeleton) -> bool {
    if state.weirdness_level < 2 {
        return false;
    }
    if state.uncanny_tokens <= 0 {
        return false;
    }
    if !event.costs_uncanny_token {
        return true;
    }

    let has_folklore_uncanny = event.tags.iter().any(|t| t == "folklore:uncanny");
    let has_fog = event.tags.iter().any(|t| t == "fog");

    match state.gm_profile {
        GMProfile::Chronicler => {
            matches!(event.severity.as_str(), "medium" | "high") && has_folklore_uncanny
        }
        GMProfile::Fireside => {
            if has_folklore_uncanny {
                return true;
            }
            if let Some(node) = current_node(state) {
                if state.time_of_day == TimeOfDay::Night && state.party.morale < 40 {
                    return has_fog || matches!(node.biome, Biome::Forest | Biome::Swamp);
                }
            }
            false
        }
        GMProfile::Lantern => {
            if has_folklore_uncanny {
                return true;
            }
            if let Some(node) = current_node(state) {
                if matches!(state.time_of_day, TimeOfDay::Night | TimeOfDay::Evening) {
                    return matches!(node.biome, Biome::Forest | Biome::Swamp) || has_fog;
                }
            }
            false
        }
    }
}

/// Record an event's primary tags in the cooldown buffer (`events.py:1739-1755`).
pub fn record_event_tags(state: &mut RunState, event: &EventSkeleton) {
    let first_family = event
        .tags
        .iter()
        .find(|t| TAG_FAMILIES.contains(&t.as_str()));
    if let Some(ft) = first_family {
        state.recent_event_tags.push(ft.clone());
    } else {
        state
            .recent_event_tags
            .push(event_category_str(event.category).to_string());
    }

    if event.severity == "high" {
        state.recent_event_tags.push("_high_sev".to_string());
    }

    let max_window = COOLDOWN_WINDOW * 2;
    if state.recent_event_tags.len() > max_window {
        let trim = state.recent_event_tags.len() - max_window;
        state.recent_event_tags.drain(..trim);
    }
}

/// Compute cooldown multiplier based on recent tag history (`events.py:1757-1767`).
pub fn cooldown_factor(state: &RunState, event: &EventSkeleton) -> f64 {
    if state.recent_event_tags.is_empty() {
        return 1.0;
    }
    let mut family_tags: Vec<&str> = event
        .tags
        .iter()
        .map(String::as_str)
        .filter(|t| TAG_FAMILIES.contains(t))
        .collect();
    if family_tags.is_empty() {
        family_tags.push(event_category_str(event.category));
    }
    let hits = state
        .recent_event_tags
        .iter()
        .filter(|t| family_tags.contains(&t.as_str()))
        .count();
    if hits == 0 {
        1.0
    } else {
        COOLDOWN_PENALTY.powi(hits as i32)
    }
}

fn twist_boosts_tag(twist: TwistModifier, tag: &str) -> bool {
    match twist {
        TwistModifier::BanditYear => matches!(tag, "human" | "bandits"),
        TwistModifier::SickSeason => matches!(tag, "sickness" | "survival"),
        TwistModifier::FloodYear => matches!(tag, "river" | "ford" | "bridge" | "weather" | "rain"),
        TwistModifier::EarlyWinter => matches!(tag, "weather" | "snow" | "wind"),
        TwistModifier::GoodHunting => matches!(tag, "hunt" | "forest"),
    }
}

/// Select an event using weighted probabilities with variety guards (`events.py:1782-1876`).
pub fn select_event(
    state: &mut RunState,
    rng: &mut CpythonRng,
    library: &[EventSkeleton],
    weather: Option<Weather>,
) -> EventSkeleton {
    let node_biome = current_node(state).map(|n| n.biome);
    let mut candidates: Vec<&EventSkeleton> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();

    for event in library {
        if let (Some(bf), Some(nb)) = (&event.biome_filter, node_biome) {
            if !bf.is_empty() && !bf.contains(&nb) {
                continue;
            }
        }

        if let (Some(wf), Some(w)) = (&event.weather_filter, weather) {
            if !wf.is_empty() && !wf.contains(&w) {
                continue;
            }
        }

        if let Some(tf) = &event.time_filter {
            if !tf.is_empty() && !tf.contains(&state.time_of_day) {
                continue;
            }
        }

        if event.costs_uncanny_token && !can_spend_uncanny_token(state, event) {
            continue;
        }

        let mut weight = event.base_weight;

        match event.category {
            EventCategory::Survival => {
                weight *= 1.0;
            }
            EventCategory::Human => {
                weight *= 0.8;
            }
            EventCategory::Folklore => {
                weight *= profile_folklore_weight(state.gm_profile);
                if event.folklore_type == Some(FolkloreType::Uncanny) {
                    weight *= profile_uncanny_weight(state.gm_profile);
                }
            }
            EventCategory::Big => {
                weight *= 0.4;
            }
        }

        for twist in &state.twists {
            if event.tags.iter().any(|t| twist_boosts_tag(*twist, t)) {
                weight *= 1.5;
            }
        }

        let pressure = journey_pressure(state);
        if event.severity == "high" {
            weight *= 1.0 + 0.25 * pressure;
            let start = state.recent_event_tags.len().saturating_sub(6);
            let recent_high = state.recent_event_tags[start..]
                .iter()
                .filter(|t| *t == "_high_sev")
                .count();
            if recent_high >= 2 {
                weight *= 0.7;
            }
        } else if event.severity == "low" {
            weight *= 1.0 - 0.25 * pressure;
        }

        if state.supplies.food() < 10 && event.tags.iter().any(|t| t == "food") {
            weight *= 1.3;
        }
        if state.supplies.water() < 10 && event.tags.iter().any(|t| t == "water") {
            weight *= 1.3;
        }

        weight *= cooldown_factor(state, event);

        candidates.append(&mut vec![event]);
        weights.push(weight);
    }

    if candidates.is_empty() {
        let survival: Vec<&EventSkeleton> = library
            .iter()
            .filter(|e| e.category == EventCategory::Survival)
            .collect();
        return if !survival.is_empty() {
            (**rng.choice(&survival)).clone()
        } else {
            library[0].clone()
        };
    }

    let selected = (**rng.weighted_choice(&candidates, &weights)).clone();
    record_event_tags(state, &selected);
    selected
}

// =========================================================================
// Advertised inventory costs & event resolution (`events.py:1879-2180`)
// =========================================================================

fn canonical_supply(word: &str) -> Option<&'static str> {
    match word.to_lowercase().as_str() {
        "part" | "parts" => Some("parts"),
        "tool" | "tools" => Some("tools"),
        "med" | "meds" | "medicine" => Some("meds"),
        "rope" => Some("rope"),
        "food" => Some("food"),
        _ => None,
    }
}

fn choice_for<'a>(event: &'a EventSkeleton, choice_id: &str) -> Option<&'a ChoiceTemplate> {
    event
        .fallback_choices
        .iter()
        .find(|c| c.choice_id == choice_id)
}

fn split_words(s: &str) -> Vec<&str> {
    s.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .collect()
}

fn has_cost_negate(clause: &str) -> bool {
    let lower = clause.to_lowercase();
    if lower.starts_with("no ")
        || lower.contains(" no ")
        || lower.contains("\tno ")
        || lower.contains("(no ")
    {
        return true;
    }
    split_words(&lower)
        .iter()
        .any(|&w| matches!(w, "loss" | "lost" | "gained" | "reward" | "without" | "or"))
}

fn has_non_cost_word(clause: &str) -> bool {
    split_words(&clause.to_lowercase()).iter().any(|&w| {
        matches!(
            w,
            "time"
                | "speed"
                | "distance"
                | "health"
                | "morale"
                | "wagon"
                | "nothing"
                | "injury"
                | "danger"
                | "anxiety"
                | "unknown"
                | "unpredictable"
                | "uncertain"
        )
    })
}

fn find_qty_supply(clause: &str) -> Option<(&'static str, i32)> {
    let words = split_words(clause);
    for i in 0..words.len().saturating_sub(1) {
        if let Ok(qty) = words[i].parse::<i32>() {
            if let Some(canon) = canonical_supply(words[i + 1]) {
                return Some((canon, qty));
            }
        }
    }
    None
}

fn find_bare_supply(clause: &str) -> Option<&'static str> {
    for w in split_words(clause) {
        if let Some(canon) = canonical_supply(w) {
            return Some(canon);
        }
    }
    None
}

fn upsert_ordered_cost(costs: &mut Vec<(String, i32)>, key: &str, qty: i32) {
    if let Some(entry) = costs.iter_mut().find(|(k, _)| k == key) {
        if qty > entry.1 {
            entry.1 = qty;
        }
    } else {
        costs.push((key.to_string(), qty));
    }
}

fn parse_cost_hint_spend_ordered(hint: &str) -> Vec<(String, i32)> {
    let mut costs = Vec::new();
    if hint.trim().is_empty() {
        return costs;
    }
    let head = hint.split(" for ").next().unwrap_or("");
    for raw_clause in head.split(',') {
        let clause = raw_clause.trim().trim_end_matches('.');
        if clause.is_empty() {
            continue;
        }
        if has_cost_negate(clause) {
            continue;
        }
        let bare = find_bare_supply(clause);
        if has_non_cost_word(clause) && bare.is_none() {
            continue;
        }
        if let Some((key, qty)) = find_qty_supply(clause) {
            if INVENTORY_COST_KEYS.contains(&key) {
                upsert_ordered_cost(&mut costs, key, qty);
            }
            continue;
        }
        if let Some(key) = bare {
            if INVENTORY_COST_KEYS.contains(&key) {
                upsert_ordered_cost(&mut costs, key, 1);
            }
        }
    }
    costs
}

fn parse_risk_hint_spend_ordered(hint: &str) -> Vec<(String, i32)> {
    let mut costs = Vec::new();
    if hint.is_empty() {
        return costs;
    }
    let words: Vec<String> = split_words(hint)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect();
    for i in 0..words.len() {
        if matches!(words[i].as_str(), "use" | "uses" | "need" | "needs") {
            if let Some(next) = words.get(i + 1) {
                if let Some(key) = canonical_supply(next) {
                    if INVENTORY_COST_KEYS.contains(&key) {
                        upsert_ordered_cost(&mut costs, key, 1);
                    }
                }
            }
        } else if i + 4 < words.len()
            && words[i] == "safe"
            && words[i + 1] == "if"
            && words[i + 2] == "you"
            && words[i + 3] == "have"
        {
            if let Some(key) = canonical_supply(&words[i + 4]) {
                if INVENTORY_COST_KEYS.contains(&key) {
                    upsert_ordered_cost(&mut costs, key, 1);
                }
            }
        }
    }
    costs
}

fn advertised_inventory_cost_ordered(
    choice: Option<&ChoiceTemplate>,
    outcome: &EventOutcome,
) -> Vec<(String, i32)> {
    let mut costs = Vec::new();
    if let Some(ch) = choice {
        for (k, q) in parse_cost_hint_spend_ordered(&ch.cost_hint) {
            upsert_ordered_cost(&mut costs, &k, q);
        }
        for (k, q) in parse_risk_hint_spend_ordered(&ch.risk_hint) {
            upsert_ordered_cost(&mut costs, &k, q);
        }
    }
    for (key, val) in outcome.ordered_supplies_delta() {
        if let Some(pos) = costs.iter().position(|(k, _)| k == key) {
            if val < 0 {
                if -val > costs[pos].1 {
                    costs[pos].1 = -val;
                }
            } else if val > 0 {
                costs.remove(pos);
            }
        }
    }
    costs.retain(|(k, v)| INVENTORY_COST_KEYS.contains(&k.as_str()) && *v > 0);
    costs
}

/// Return `{supply: qty}` the player must have to honor advertised costs (`events.py:1977-1998`).
pub fn advertised_inventory_cost(
    choice: Option<&ChoiceTemplate>,
    outcome: &EventOutcome,
) -> BTreeMap<String, i32> {
    advertised_inventory_cost_ordered(choice, outcome)
        .into_iter()
        .collect()
}

fn debit_advertised_costs(outcome: &mut EventOutcome, costs: &[(String, i32)]) {
    for (key, qty) in costs {
        if DURABLE_INVENTORY.contains(&key.as_str()) {
            continue;
        }
        let current = outcome.supplies_delta.get(key).copied().unwrap_or(0);
        if current >= 0 {
            outcome.supplies_delta.insert(key.clone(), -(*qty));
            if !outcome.supplies_delta_order.contains(key) {
                outcome.supplies_delta_order.push(key.clone());
            }
        }
    }
}

fn unaffordable_costs(state: &RunState, costs: &[(String, i32)]) -> Vec<String> {
    costs
        .iter()
        .filter(|(key, qty)| state.supplies.get(key) < *qty)
        .map(|(key, _)| key.clone())
        .collect()
}

fn risk_uses_meds(risk: &str) -> bool {
    let words: Vec<String> = split_words(risk)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect();
    for i in 0..words.len().saturating_sub(1) {
        if matches!(words[i].as_str(), "use" | "uses" | "need" | "needs")
            && matches!(words[i + 1].as_str(), "med" | "meds" | "medicine")
        {
            return true;
        }
    }
    false
}

fn risk_mentions_safe(risk: &str) -> bool {
    split_words(risk)
        .iter()
        .any(|w| w.eq_ignore_ascii_case("safe"))
}

fn insufficient_supplies_outcome(
    template: &EventOutcome,
    choice: Option<&ChoiceTemplate>,
) -> EventOutcome {
    let mut flags = template.special_flags.clone();
    if !flags.iter().any(|f| f == "insufficient_supplies") {
        flags.push("insufficient_supplies".to_string());
    }

    let action = choice.map(|c| c.action.as_str()).unwrap_or("");
    let risk = choice.map(|c| c.risk_hint.as_str()).unwrap_or("");

    let time_cost = std::cmp::max(template.time_cost, 1);
    let morale = std::cmp::min(template.morale_delta, -2);
    let mut health = if template.health_delta < 0 {
        template.health_delta
    } else {
        0
    };
    let mut wagon = if template.wagon_delta > 0 {
        0
    } else {
        template.wagon_delta
    };
    let mut distance = template.distance_delta;

    if action == "REPAIR" && wagon < 0 {
        wagon -= 5;
    } else if action == "PAY" {
        distance = std::cmp::max(distance, 5);
    } else if risk_uses_meds(risk) {
        health = std::cmp::min(health, -8);
    } else if risk_mentions_safe(risk)
        && template.health_delta >= 0
        && template.wagon_delta >= 0
        && action != "PAY"
    {
        health = -8;
    }

    EventOutcome {
        supplies_delta: BTreeMap::new(),
        supplies_delta_order: Vec::new(),
        health_delta: health,
        wagon_delta: wagon,
        animals_health_delta: 0,
        morale_delta: morale,
        time_cost,
        distance_delta: distance,
        special_flags: flags,
    }
}

/// Resolve an event choice into a concrete outcome (`events.py:2062-2142`).
pub fn resolve_event(
    state: &mut RunState,
    event: &EventSkeleton,
    choice_id: &str,
    rng: &mut CpythonRng,
) -> EventOutcome {
    let Some(template) = event.outcome_templates.get(choice_id) else {
        return EventOutcome {
            time_cost: 1,
            special_flags: vec!["undefined_choice_miss".to_string()],
            ..Default::default()
        };
    };

    let mut outcome = template.clone();
    let choice = choice_for(event, choice_id);
    let costs = advertised_inventory_cost_ordered(choice, &outcome);
    debit_advertised_costs(&mut outcome, &costs);
    let missing = unaffordable_costs(state, &costs);
    if !missing.is_empty() {
        let denied = insufficient_supplies_outcome(template, choice);
        if denied
            .special_flags
            .iter()
            .any(|f| f == "uncanny_token_spent")
            && state.uncanny_tokens > 0
        {
            state.uncanny_tokens -= 1;
        }
        return denied;
    }

    if outcome.health_delta != 0 {
        outcome.health_delta += rng.randint(-3, 3) as i32;
    }

    if outcome
        .special_flags
        .iter()
        .any(|f| f == "uncanny_token_spent")
        && state.uncanny_tokens > 0
    {
        state.uncanny_tokens -= 1;
    }

    outcome
}

/// Apply an event outcome to game state (`events.py:2145-2179`).
pub fn apply_outcome(state: &mut RunState, outcome: &EventOutcome) {
    state.supplies.apply_delta(&outcome.supplies_delta);

    if outcome.health_delta != 0 {
        for i in 0..state.party.members.len() {
            if state.party.members[i].is_alive() {
                state.party.members[i].health =
                    (state.party.members[i].health + outcome.health_delta).clamp(0, 100);
                if state.party.members[i].health <= 0
                    && state.party.members[i].death_cause.is_empty()
                {
                    let cause = proximate_death_cause(&state.party.members[i], state, true);
                    state.party.members[i].death_cause = cause;
                }
            }
        }
    }

    if outcome.wagon_delta != 0 {
        state.wagon.condition = (state.wagon.condition + outcome.wagon_delta).clamp(0, 100);
    }

    if outcome.animals_health_delta != 0 {
        state.wagon.animals_health =
            (state.wagon.animals_health + outcome.animals_health_delta).clamp(0, 100);
    }

    if outcome.morale_delta != 0 {
        state.party.morale = (state.party.morale + outcome.morale_delta).clamp(0, 100);
    }

    if outcome.distance_delta != 0 {
        state.distance_remaining += outcome.distance_delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::create_new_run;

    // Fails on 02532e17 because `events` module did not exist on 02532e17:
    // tests 260-event library construction (60 hand-authored + 200 JSON) and
    // advertised inventory cost denial without advancing RNG when supplies are missing.
    #[test]
    fn test_build_event_library_260_and_insufficient_supplies_denial() {
        let lib = build_event_library();
        assert_eq!(lib.len(), 260);
        assert_eq!(lib[0].event_id, "storm_sudden");

        let broken_axle = lib
            .iter()
            .find(|e| e.event_id == "broken_axle")
            .expect("broken_axle event");
        let choice_a = &broken_axle.fallback_choices[0];
        let tmpl_a = broken_axle.outcome_templates.get("A").unwrap();
        let cost = advertised_inventory_cost(Some(choice_a), tmpl_a);
        assert_eq!(cost.get("parts"), Some(&1));

        let mut st = create_new_run(42);
        st.supplies.set("parts", 0);
        let mut rng = CpythonRng::from_seed(42);
        let counter_before = rng.counter;

        let denied = resolve_event(&mut st, broken_axle, "A", &mut rng);
        assert!(denied.supplies_delta.is_empty());
        assert_eq!(denied.morale_delta, -2);
        assert!(denied
            .special_flags
            .iter()
            .any(|f| f == "insufficient_supplies"));
        assert_eq!(rng.counter, counter_before);
    }
}
