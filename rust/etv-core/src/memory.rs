//! Memory system — store, pressures, themes, retrieval, GM brief builder, and emitters
//! (`memory.py`, `memory_emitters.py`).
//!
//! Exact port of `src/escape_the_valley/memory.py` and
//! `src/escape_the_valley/memory_emitters.py` at tag `v1.3.1` (`47b01a3`).

use std::collections::HashSet;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::{Condition, MapNode, MemoryCard, RunState};
use crate::physics::{current_node, journey_pressure, HealthEffect};
use crate::worldgen::{EventCategory, EventSkeleton};

/// Max cards before eviction (`memory.py:23`).
pub const MEMORY_BUDGET: usize = 50;

const TITLE_MAX: usize = 40;
const TEXT_MAX: usize = 300;
const GM_MAX_PER_PROPOSAL: usize = 2;
const GM_ALLOWED_KINDS: [&str; 5] = ["npc", "omen", "place", "rumor", "promise"];

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Tag -> theme mapping (`memory.py:26-58`).
pub fn tag_to_theme(tag: &str) -> Option<&'static str> {
    match tag {
        "river" | "crossing" | "ford" => Some("river"),
        "folklore" | "weird" | "omen" | "ghost" => Some("uncanny"),
        "food" | "hunger" | "starvation" | "hunt" => Some("hunger"),
        "trade" | "merchant" | "town" => Some("commerce"),
        "bandit" | "attack" | "ambush" => Some("danger"),
        "sickness" | "disease" | "fever" => Some("plague"),
        "storm" | "rain" | "snow" | "cold" => Some("weather"),
        "wagon" | "breakdown" | "repair" => Some("breakdown"),
        "death" | "grave" => Some("loss"),
        "morale" | "campfire" => Some("spirit"),
        _ => None,
    }
}

/// Append a card, enforcing budget by evicting lowest-salience (`memory.py:63-88`).
pub fn add_card(state: &mut RunState, card: MemoryCard) {
    if state
        .memory_cards
        .iter()
        .any(|existing| existing.id == card.id)
    {
        return;
    }

    state.memory_cards.push(card);

    if state.memory_cards.len() > MEMORY_BUDGET {
        let count = state.memory_cards.len() - MEMORY_BUDGET;
        drop_lowest(state, count);
    }
}

/// Evict the `count` lowest-salience cards, preferring expired cooldowns first (`memory.py:98-121`).
/// Preserves the relative order of surviving cards in `state.memory_cards`.
pub fn drop_lowest(state: &mut RunState, count: usize) {
    if count == 0 || state.memory_cards.is_empty() {
        return;
    }

    let day = state.day;
    let mut indices: Vec<usize> = (0..state.memory_cards.len()).collect();
    indices.sort_by(|&a, &b| {
        let ca = &state.memory_cards[a];
        let cb = &state.memory_cards[b];
        let exp_a = if ca.cooldown_until <= day { 0 } else { 1 };
        let exp_b = if cb.cooldown_until <= day { 0 } else { 1 };
        exp_a
            .cmp(&exp_b)
            .then_with(|| ca.salience.total_cmp(&cb.salience))
    });

    let mut to_remove: HashSet<String> = HashSet::new();
    for idx in indices {
        if to_remove.len() >= count {
            break;
        }
        to_remove.insert(state.memory_cards[idx].id.clone());
    }

    state.memory_cards.retain(|c| !to_remove.contains(&c.id));
}

/// Scan state for pressure tags, sorted by severity descending (`memory.py:126-196`).
pub fn compute_pressures(state: &RunState) -> Vec<String> {
    let alive = state.party.alive_count() as i32;
    if alive == 0 {
        return vec!["PARTY_DEAD".to_string()];
    }

    let mut pressures: Vec<(i32, &'static str)> = Vec::new();

    // Food pressure
    let food = state.supplies.food();
    if food <= 0 {
        pressures.push((10, "FOOD_GONE"));
    } else if food <= alive * 2 {
        pressures.push((8, "FOOD_CRITICAL"));
    } else if food <= alive * 4 {
        pressures.push((4, "FOOD_LOW"));
    }

    // Water pressure
    let water = state.supplies.water();
    if water <= 0 {
        pressures.push((10, "WATER_GONE"));
    } else if water <= alive * 2 {
        pressures.push((9, "WATER_CRITICAL"));
    } else if water <= alive * 4 {
        pressures.push((5, "WATER_LOW"));
    }

    // Wagon
    if state.wagon.condition <= 0 {
        pressures.push((10, "WAGON_DESTROYED"));
    } else if state.wagon.condition <= 15 {
        pressures.push((7, "WAGON_FRAGILE"));
    } else if state.wagon.condition < 40 {
        pressures.push((4, "WAGON_WORN"));
    }

    // Parts
    if state.supplies.parts() <= 0 && state.wagon.condition < 50 {
        pressures.push((6, "NO_PARTS"));
    }

    // Party health
    let sick_count = state
        .party
        .members
        .iter()
        .filter(|m| m.is_alive() && matches!(m.condition, Condition::Sick | Condition::Injured))
        .count();
    if sick_count >= 2 {
        pressures.push((6, "PARTY_SICK"));
    } else if sick_count == 1 {
        pressures.push((3, "PARTY_HURT"));
    }

    let low_health = state
        .party
        .members
        .iter()
        .filter(|m| m.is_alive() && m.health < 30)
        .count();
    if low_health >= 2 {
        pressures.push((7, "PARTY_DYING"));
    }

    // Morale
    if state.party.morale < 15 {
        pressures.push((6, "MORALE_BROKEN"));
    } else if state.party.morale < 30 {
        pressures.push((3, "MORALE_LOW"));
    }

    // Journey
    let pressure = journey_pressure(state);
    if pressure > 0.8 {
        pressures.push((2, "LATE_JOURNEY"));
    } else if pressure > 0.5 {
        pressures.push((1, "MID_JOURNEY"));
    }

    pressures.sort_by_key(|&(sev, _)| -sev);
    pressures
        .into_iter()
        .map(|(_, label)| label.to_string())
        .collect()
}

fn lowered_str_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Derive 1–3 theme tags from recent journal entries + event tags (`memory.py:206-229`).
pub fn compute_themes(state: &RunState) -> Vec<String> {
    let mut raw_tags = Vec::new();
    let start = state.journal.len().saturating_sub(5);
    for entry in &state.journal[start..] {
        raw_tags.extend(lowered_str_tags(&entry.tags));
    }
    raw_tags.extend(lowered_str_tags(&state.recent_event_tags));

    // Preserve first-seen insertion order of themes, just like Python dict
    let mut theme_counts: Vec<(&'static str, i32)> = Vec::new();
    for tag in &raw_tags {
        if let Some(theme) = tag_to_theme(tag) {
            if let Some(entry) = theme_counts.iter_mut().find(|(t, _)| *t == theme) {
                entry.1 += 1;
            } else {
                theme_counts.push((theme, 1));
            }
        }
    }

    theme_counts.sort_by_key(|&(_, count)| -count);
    theme_counts
        .into_iter()
        .take(3)
        .map(|(theme, _)| theme.to_string())
        .collect()
}

/// Retrieve top-scoring memories using tag overlap + recency + salience (`memory.py:234-307`).
pub fn retrieve_memories(
    state: &mut RunState,
    max_results: usize,
    mark_retrieved: bool,
) -> Vec<MemoryCard> {
    if state.memory_cards.is_empty() {
        return Vec::new();
    }

    let mut context_tags: HashSet<String> = HashSet::new();
    let j_start = state.journal.len().saturating_sub(3);
    for entry in &state.journal[j_start..] {
        for t in lowered_str_tags(&entry.tags) {
            context_tags.insert(t);
        }
    }
    for t in lowered_str_tags(&state.recent_event_tags) {
        context_tags.insert(t);
    }

    for p in compute_pressures(state) {
        context_tags.insert(p.to_lowercase());
    }

    let day = state.day;
    let mut scored: Vec<(f64, usize)> = Vec::new();

    for (idx, card) in state.memory_cards.iter().enumerate() {
        if card.cooldown_until > day {
            continue;
        }

        let mut score = 0.0;
        let card_tags: HashSet<String> = lowered_str_tags(&card.tags).into_iter().collect();
        let overlap = card_tags.intersection(&context_tags).count();
        score += (overlap as f64) * 0.2;

        let days_ago = day - card.day_last_seen;
        if days_ago <= 3 {
            score += 0.3;
        } else if days_ago <= 5 {
            score += 0.15;
        }

        score *= card.salience;

        if card.source == "engine" {
            score += 0.05;
        }

        scored.push((score, idx));
    }

    scored.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut results = Vec::new();
    for (_, idx) in scored.into_iter().take(max_results) {
        if mark_retrieved {
            state.memory_cards[idx].day_last_seen = day;
            state.memory_cards[idx].cooldown_until = day + 3;
        }
        results.push(state.memory_cards[idx].clone());
    }

    results
}

/// Everything the GM needs to narrate intelligently (`memory.py:313-321`).
#[derive(Debug, Clone, PartialEq)]
pub struct GMBrief {
    pub situation: String,
    pub pressures: Vec<String>,
    pub themes: Vec<String>,
    pub callbacks: Vec<MemoryCard>,
    pub tone_profile: String,
    pub weirdness_allowance: String,
}

/// Compute the full GM brief from state (`memory.py:324-362`).
pub fn build_gm_brief(state: &mut RunState, mark_retrieved: bool) -> GMBrief {
    let pressures = compute_pressures(state);
    let themes = compute_themes(state);
    let callbacks = retrieve_memories(state, 6, mark_retrieved);

    let situation = build_situation(state, &pressures);
    let weirdness = compute_weirdness(state).to_string();

    GMBrief {
        situation,
        pressures: pressures.into_iter().take(3).collect(),
        themes,
        callbacks,
        tone_profile: state.gm_profile.as_str().to_string(),
        weirdness_allowance: weirdness,
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

fn build_situation(state: &RunState, pressures: &[String]) -> String {
    let mut parts = Vec::new();

    let biome = current_node(state)
        .map(|n| n.biome.as_str())
        .unwrap_or("unknown terrain");
    parts.push(format!(
        "Day {}, {}. {} terrain.",
        state.day,
        state.time_of_day.as_str(),
        capitalize_first(biome)
    ));

    let progress = journey_pressure(state);
    if progress < 0.25 {
        parts.push("Early in the journey.".to_string());
    } else if progress < 0.5 {
        parts.push("Nearing the halfway mark.".to_string());
    } else if progress < 0.75 {
        parts.push("Well past halfway.".to_string());
    } else {
        parts.push("The end of the trail is near.".to_string());
    }

    let alive = state.party.alive_count();
    let total = state.party.members.len();
    if alive < total {
        parts.push(format!("{alive} of {total} party members alive."));
    }

    if let Some(top) = pressures.first() {
        let flavor = match top.as_str() {
            "FOOD_GONE" => Some("They have no food."),
            "FOOD_CRITICAL" => Some("Rations are nearly gone."),
            "WATER_GONE" => Some("No water remains."),
            "WATER_CRITICAL" => Some("Water is dangerously low."),
            "WAGON_DESTROYED" => Some("The wagon is destroyed."),
            "WAGON_FRAGILE" => Some("The wagon may not survive another hit."),
            "PARTY_SICK" => Some("Multiple members are sick or injured."),
            "PARTY_DYING" => Some("Several members are in critical condition."),
            "MORALE_BROKEN" => Some("The party's spirit is shattered."),
            _ => None,
        };
        if let Some(f) = flavor {
            parts.push(f.to_string());
        }
    }

    match state.doctrine.as_str() {
        "travel_light" => {
            parts.push("The caravan travels light — less to eat, less to carry.".to_string())
        }
        "careful_hands" => {
            parts.push("Every repair is deliberate. The caravan moves carefully.".to_string())
        }
        "no_debts" => {
            parts.push("They carry no debts. Morale holds, but the wagon is lean.".to_string())
        }
        _ => {}
    }

    match state.taboo.as_str() {
        "never_night" => parts.push("They will not travel at night.".to_string()),
        "never_river" => parts.push("They refuse to drink river water.".to_string()),
        "leave_nothing" => parts.push("They leave nothing behind.".to_string()),
        _ => {}
    }

    parts.join(" ")
}

fn compute_weirdness(state: &RunState) -> &'static str {
    if state.uncanny_tokens <= 0 {
        return "none";
    }
    if state.weirdness_level >= 3 {
        return "strong";
    }
    if state.weirdness_level >= 2 {
        return "hint";
    }
    "none"
}

/// Format `GMBrief` as text for injection into GM prompt (`memory.py:452-478`).
pub fn format_brief_for_prompt(brief: &GMBrief) -> String {
    let mut lines = Vec::new();
    lines.push("GM BRIEF:".to_string());
    lines.push(format!("Situation: {}", brief.situation));

    if !brief.pressures.is_empty() {
        lines.push(format!("Pressures: {}", brief.pressures.join(", ")));
    }
    if !brief.themes.is_empty() {
        lines.push(format!("Themes: {}", brief.themes.join(", ")));
    }
    if brief.weirdness_allowance != "none" {
        lines.push(format!(
            "Weirdness allowance: {}",
            brief.weirdness_allowance
        ));
    }
    if !brief.callbacks.is_empty() {
        lines.push(String::new());
        lines.push("Relevant memories you may reference (optional):".to_string());
        for card in &brief.callbacks {
            lines.push(format!("  - [{}] {}: {}", card.kind, card.title, card.text));
        }
    }

    lines.join("\n")
}

// =========================================================================
// Deterministic memory emitters (`memory_emitters.py`)
// =========================================================================

/// Emit memory cards from health check effects (`memory_emitters.py:26-79`).
pub fn emit_health_cards(state: &mut RunState, effects: &[HealthEffect]) {
    let day = state.day;
    for eff in effects {
        match eff {
            HealthEffect::Died { member, cause } => {
                let cause_str = if cause.is_empty() {
                    "unknown"
                } else {
                    cause.as_str()
                };
                let card = MemoryCard {
                    id: format!("eng_death_{}_d{day}", member.to_lowercase()),
                    kind: "wound".to_string(),
                    title: format!("{member}'s Death"),
                    text: format!("{member} perished on day {day} ({cause_str})."),
                    tags: vec![
                        "death".to_string(),
                        "loss".to_string(),
                        cause_str.to_lowercase().replace(' ', "_"),
                    ],
                    day_created: day,
                    day_last_seen: day,
                    entities: vec![member.clone()],
                    salience: 0.7,
                    cooldown_until: 0,
                    source: "engine".to_string(),
                };
                add_card(state, card);
            }
            HealthEffect::FellSick { member } => {
                let card = MemoryCard {
                    id: format!("eng_sick_{}_d{day}", member.to_lowercase()),
                    kind: "wound".to_string(),
                    title: format!("{member}'s Illness"),
                    text: format!("{member} fell sick on day {day}."),
                    tags: vec!["sickness".to_string(), "disease".to_string()],
                    day_created: day,
                    day_last_seen: day,
                    entities: vec![member.clone()],
                    salience: 0.7,
                    cooldown_until: 0,
                    source: "engine".to_string(),
                };
                add_card(state, card);
            }
            HealthEffect::Healed { member } => {
                let card = MemoryCard {
                    id: format!("eng_healed_{}_d{day}", member.to_lowercase()),
                    kind: "event_callback".to_string(),
                    title: format!("{member} Recovered"),
                    text: format!("{member} recovered from their ailment on day {day}."),
                    tags: vec!["recovery".to_string()],
                    day_created: day,
                    day_last_seen: day,
                    entities: vec![member.clone()],
                    salience: 0.7,
                    cooldown_until: 0,
                    source: "engine".to_string(),
                };
                add_card(state, card);
            }
            HealthEffect::Starvation { .. } | HealthEffect::Dehydration { .. } => {}
        }
    }
}

/// Emit a crisis card the first time a resource hits 0 (`memory_emitters.py:82-112`).
pub fn emit_resource_crisis_card(state: &mut RunState, resource: &str) {
    let res_string = resource.to_string();
    if state.resource_crises_seen.contains(&res_string) {
        return;
    }

    state.resource_crises_seen.push(res_string.clone());

    let title = match resource {
        "food" => "Starvation Begins".to_string(),
        "water" => "The Water Ran Out".to_string(),
        "ammo" => "Out of Ammunition".to_string(),
        "meds" => "No Medicine Left".to_string(),
        "parts" => "Last Part Used".to_string(),
        other => format!("No {} Left", capitalize_first(other)),
    };

    let day = state.day;
    let card = MemoryCard {
        id: format!("eng_crisis_{resource}_d{day}"),
        kind: "crisis".to_string(),
        title,
        text: format!("The party ran out of {resource} on day {day}."),
        tags: vec![res_string, "crisis".to_string()],
        day_created: day,
        day_last_seen: day,
        entities: Vec::new(),
        salience: 0.7,
        cooldown_until: 0,
        source: "engine".to_string(),
    };
    add_card(state, card);
}

/// Emit a card for significant wagon events (`memory_emitters.py:115-152`).
pub fn emit_wagon_card(state: &mut RunState, damage: i32, had_parts: bool) {
    if damage < 15 && had_parts {
        return;
    }

    let day = state.day;
    let card = if !had_parts {
        MemoryCard {
            id: format!("eng_wagon_noparts_d{day}"),
            kind: "event_callback".to_string(),
            title: "Breakdown Without Parts".to_string(),
            text: format!(
                "The wagon took {damage} damage on day {day} with no parts to repair it."
            ),
            tags: vec![
                "wagon".to_string(),
                "breakdown".to_string(),
                "crisis".to_string(),
            ],
            day_created: day,
            day_last_seen: day,
            entities: Vec::new(),
            salience: 0.7,
            cooldown_until: 0,
            source: "engine".to_string(),
        }
    } else {
        MemoryCard {
            id: format!("eng_wagon_hit_d{day}"),
            kind: "event_callback".to_string(),
            title: "Heavy Wagon Damage".to_string(),
            text: format!("The wagon took {damage} damage on day {day}."),
            tags: vec!["wagon".to_string(), "breakdown".to_string()],
            day_created: day,
            day_last_seen: day,
            entities: Vec::new(),
            salience: 0.7,
            cooldown_until: 0,
            source: "engine".to_string(),
        }
    };
    add_card(state, card);
}

/// Emit a card when arriving at a notable location (`memory_emitters.py:155-176`).
pub fn emit_arrival_card(state: &mut RunState, node: &MapNode) {
    if !node.is_town {
        return;
    }

    let day = state.day;
    let biome_str = node.biome.as_str().to_string();
    let card = MemoryCard {
        id: format!("eng_arrival_{}_d{day}", node.node_id),
        kind: "landmark".to_string(),
        title: truncate_chars(&format!("Arrived at {}", node.name), TITLE_MAX),
        text: format!(
            "The party reached {}, a settlement in {biome_str} terrain, on day {day}.",
            node.name
        ),
        tags: vec!["town".to_string(), "landmark".to_string(), biome_str],
        day_created: day,
        day_last_seen: day,
        entities: vec![node.name.clone()],
        salience: 0.7,
        cooldown_until: 0,
        source: "engine".to_string(),
    };
    add_card(state, card);
}

/// Emit a card for folklore/uncanny events (`memory_emitters.py:179-202`).
pub fn emit_event_card(state: &mut RunState, event: &EventSkeleton) {
    if !event.tags.iter().any(|t| t == "folklore") && event.category != EventCategory::Folklore {
        return;
    }

    let day = state.day;
    let text = if !event.fallback_narration.is_empty() {
        truncate_chars(&event.fallback_narration, TEXT_MAX)
    } else {
        format!("A strange event on day {day}.")
    };

    let card = MemoryCard {
        id: format!("eng_event_{}_d{day}", event.event_id),
        kind: "omen".to_string(),
        title: truncate_chars(&event.title, TITLE_MAX),
        text,
        tags: event.tags.clone(),
        day_created: day,
        day_last_seen: day,
        entities: Vec::new(),
        salience: 0.7,
        cooldown_until: 0,
        source: "engine".to_string(),
    };
    add_card(state, card);
}

/// Emit a crisis card when an escape valve is used (`memory_emitters.py:208-230`).
pub fn emit_escape_valve_card(state: &mut RunState, valve_type: &str, detail: &str) {
    let title = match valve_type {
        "abandon_cargo" => "Cargo Abandoned".to_string(),
        "desperate_repair" => "Desperate Repair Attempt".to_string(),
        "hard_ration" => "Hard Rationing Imposed".to_string(),
        other => other
            .split('_')
            .map(capitalize_first)
            .collect::<Vec<_>>()
            .join(" "),
    };

    let day = state.day;
    let card = MemoryCard {
        id: format!("eng_valve_{valve_type}_d{day}"),
        kind: "crisis".to_string(),
        title,
        text: truncate_chars(&format!("Day {day}: {detail}"), TEXT_MAX),
        tags: vec!["escape_valve".to_string(), valve_type.to_string()],
        day_created: day,
        day_last_seen: day,
        entities: Vec::new(),
        salience: 0.7,
        cooldown_until: 0,
        source: "engine".to_string(),
    };
    add_card(state, card);
}

/// Check all critical resources, emit crisis cards for any newly at zero (`memory_emitters.py:236-241`).
pub fn check_resource_crises(state: &mut RunState) {
    for key in ["food", "water", "ammo", "meds", "parts"] {
        if state.supplies.get(key) <= 0 {
            emit_resource_crisis_card(state, key);
        }
    }
}

/// Validate GM-proposed memory cards (`memory_emitters.py:247-387`).
pub fn validate_gm_cards(state: &RunState, proposed: &Value) -> Vec<MemoryCard> {
    let mut accepted = Vec::new();
    let Some(arr) = proposed.as_array() else {
        return accepted;
    };

    for proposal in arr.iter().take(GM_MAX_PER_PROPOSAL) {
        let Some(obj) = proposal.as_object() else {
            continue;
        };

        let Some(kind) = obj.get("kind").and_then(|v| v.as_str()) else {
            continue;
        };
        if !GM_ALLOWED_KINDS.contains(&kind) {
            continue;
        }

        let raw_title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let raw_text = obj.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let title = truncate_chars(raw_title, TITLE_MAX);
        let text = truncate_chars(raw_text, TEXT_MAX);

        if title.is_empty() || text.is_empty() {
            continue;
        }

        if mentions_supply_numbers(&text) {
            continue;
        }

        let digest_src = format!("{kind}\x1f{}\x1f{title}\x1f{text}", state.day);
        let digest_bytes = Sha256::digest(digest_src.as_bytes());
        let digest = &crate::rng::hex_encode(&digest_bytes)[..12];
        let card_id = format!("gm_{kind}_{}_{digest}", state.day);

        let tags = obj
            .get("tags")
            .map(|v| nonempty_str_items(v, 5))
            .unwrap_or_default();
        let entities = obj
            .get("entities")
            .map(|v| nonempty_str_items(v, 5))
            .unwrap_or_default();

        accepted.push(MemoryCard {
            id: card_id,
            kind: kind.to_string(),
            title,
            text,
            tags,
            day_created: state.day,
            day_last_seen: state.day,
            entities,
            salience: 0.5,
            cooldown_until: 0,
            source: "gm".to_string(),
        });
    }

    accepted
}

fn nonempty_str_items(value: &Value, limit: usize) -> Vec<String> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .take(limit)
        .map(ToString::to_string)
        .collect()
}

fn mentions_supply_numbers(text: &str) -> bool {
    let lower = text.to_lowercase();
    let supply_words = [
        "food",
        "water",
        "ammo",
        "meds",
        "parts",
        "firewood",
        "salt",
        "rope",
        "tools",
        "lantern_oil",
        "cloth",
        "boots",
    ];
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':')
        .filter(|s| !s.is_empty())
        .collect();

    for i in 0..tokens.len() {
        let tok = tokens[i].trim_matches(':');
        if tok.chars().all(|c| c.is_ascii_digit()) && !tok.is_empty() {
            if let Some(next) = tokens.get(i + 1) {
                let next_clean = next.trim_matches(':');
                if supply_words.contains(&next_clean) {
                    return true;
                }
            }
        }
        if let Some((left, right)) = tokens[i].split_once(':') {
            if supply_words.contains(&left)
                && !right.is_empty()
                && right.chars().all(|c| c.is_ascii_digit())
            {
                return true;
            }
            if supply_words.contains(&left) && right.is_empty() {
                if let Some(next) = tokens.get(i + 1) {
                    if !next.is_empty() && next.chars().all(|c| c.is_ascii_digit()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::create_new_run;
    use serde_json::json;

    // preservation: duplicate ID rejection, `MEMORY_BUDGET` (`50`) eviction via `drop_lowest`
    // (evicting expired-cooldown lowest-salience cards while keeping active-cooldown and higher-salience cards),
    // retrieval cooldown stamping, and `validate_gm_cards` guardrails (including missing or non-list `"tags"`).
    #[test]
    fn test_memory_store_dedup_eviction_and_validate_gm_cards() {
        let mut st = create_new_run(42);
        st.day = 5;

        let card1 = MemoryCard {
            id: "dup_id_1".to_string(),
            kind: "omen".to_string(),
            title: "Strange Fog".to_string(),
            text: "Fog rolled over the ridge.".to_string(),
            tags: vec!["fog".to_string()],
            day_created: 5,
            day_last_seen: 5,
            entities: Vec::new(),
            salience: 0.7,
            cooldown_until: 0,
            source: "engine".to_string(),
        };
        add_card(&mut st, card1.clone());
        // Duplicate ID is dropped
        add_card(
            &mut st,
            MemoryCard {
                title: "Different Title".to_string(),
                ..card1
            },
        );
        assert_eq!(st.memory_cards.len(), 1);
        assert_eq!(st.memory_cards[0].title, "Strange Fog");

        // Retrieval stamps cooldown_until = day + 3 (= 8)
        st.recent_event_tags.push("fog".to_string());
        let retrieved = retrieve_memories(&mut st, 3, true);
        assert_eq!(retrieved.len(), 1);
        assert_eq!(st.memory_cards[0].cooldown_until, 8);

        // Fill store past MEMORY_BUDGET (50) and verify drop_lowest evicts expired-cooldown
        // lowest-salience cards first while keeping active-cooldown cards (like dup_id_1 with
        // cooldown_until == 8 > st.day == 5, and low_sal_active with salience 0.1 but cooldown_until == 9)
        add_card(
            &mut st,
            MemoryCard {
                id: "low_sal_active".to_string(),
                kind: "rumor".to_string(),
                title: "Protected by Cooldown".to_string(),
                text: "Low salience but active cooldown.".to_string(),
                tags: Vec::new(),
                day_created: 5,
                day_last_seen: 5,
                entities: Vec::new(),
                salience: 0.1,
                cooldown_until: 9,
                source: "engine".to_string(),
            },
        );
        add_card(
            &mut st,
            MemoryCard {
                id: "low_sal_expired_1".to_string(),
                kind: "rumor".to_string(),
                title: "Evict First".to_string(),
                text: "Lowest salience with expired cooldown.".to_string(),
                tags: Vec::new(),
                day_created: 1,
                day_last_seen: 1,
                entities: Vec::new(),
                salience: 0.15,
                cooldown_until: 2,
                source: "engine".to_string(),
            },
        );
        add_card(
            &mut st,
            MemoryCard {
                id: "low_sal_expired_2".to_string(),
                kind: "rumor".to_string(),
                title: "Evict Second".to_string(),
                text: "Second-lowest salience with expired cooldown.".to_string(),
                tags: Vec::new(),
                day_created: 1,
                day_last_seen: 1,
                entities: Vec::new(),
                salience: 0.20,
                cooldown_until: 0,
                source: "engine".to_string(),
            },
        );
        for i in 0..48 {
            add_card(
                &mut st,
                MemoryCard {
                    id: format!("filler_{i}"),
                    kind: "place".to_string(),
                    title: format!("Place {i}"),
                    text: "A landmark along the trail.".to_string(),
                    tags: Vec::new(),
                    day_created: 5,
                    day_last_seen: 5,
                    entities: Vec::new(),
                    salience: 0.6 + (i as f64) * 0.001,
                    cooldown_until: 0,
                    source: "engine".to_string(),
                },
            );
        }
        // 4 + 48 = 52 cards inserted; budget is enforced at MEMORY_BUDGET (50)
        assert_eq!(st.memory_cards.len(), MEMORY_BUDGET);
        let remaining_ids: Vec<&str> = st.memory_cards.iter().map(|c| c.id.as_str()).collect();
        assert!(
            !remaining_ids.contains(&"low_sal_expired_1"),
            "lowest-salience expired-cooldown card must be evicted first"
        );
        assert!(
            !remaining_ids.contains(&"low_sal_expired_2"),
            "second-lowest-salience expired-cooldown card must be evicted second"
        );
        assert!(
            remaining_ids.contains(&"low_sal_active"),
            "active-cooldown card (cooldown_until > day) must be protected over expired-cooldown cards"
        );
        assert!(remaining_ids.contains(&"dup_id_1"));

        // validate_gm_cards rejects invalid kinds, supply numbers, and accepts valid cards with engine-assigned IDs
        let proposals = json!([
            {
                "id": "attacker_id",
                "kind": "rumor",
                "title": "Whisper at the Well",
                "text": "A traveler spoke of a flooded pass ahead.",
                "tags": ["river", "rumor", null, "", 42],
                "entities": ["Millford"]
            },
            {
                "kind": "rumor",
                "title": "Supply Cheat",
                "text": "Found 10 food in the bushes."
            },
            {
                "kind": "crisis",
                "title": "Disallowed Kind",
                "text": "GM cannot emit crisis cards."
            }
        ]);
        let validated = validate_gm_cards(&st, &proposals);
        assert_eq!(validated.len(), 1);
        assert!(validated[0].id.starts_with("gm_rumor_5_"));
        assert_eq!(validated[0].source, "gm");
        assert_eq!(validated[0].salience, 0.5);
        assert_eq!(
            validated[0].tags,
            vec!["river".to_string(), "rumor".to_string()]
        );

        // Proposals with omitted `"tags"` or non-list `"tags"` are accepted with `tags == []`
        // matching Python `memory_emitters.py:337-369`.
        let proposals_missing_or_nonlist_tags = json!([
            {
                "kind": "omen",
                "title": "Silent Crow",
                "text": "A crow watched from the dead cedar."
            },
            {
                "kind": "place",
                "title": "Split Stone",
                "text": "A boulder cleft in two by frost.",
                "tags": "not-a-list",
                "entities": 123
            }
        ]);
        let validated_tags = validate_gm_cards(&st, &proposals_missing_or_nonlist_tags);
        assert_eq!(validated_tags.len(), 2);
        assert!(validated_tags[0].tags.is_empty());
        assert!(validated_tags[0].entities.is_empty());
        assert!(validated_tags[1].tags.is_empty());
        assert!(validated_tags[1].entities.is_empty());
    }
}
