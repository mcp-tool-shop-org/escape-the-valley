//! Seeded world generation and JSON event skeleton loader (`worldgen.py`, `event_loader.py`).

use crate::model::{
    doctrine_modifiers, is_valid_resource_key, Biome, Condition, Doctrine, GMProfile, MapNode,
    PartyMember, PartyState, RunState, SuppliesState, Taboo, TimeOfDay, Trait, TwistModifier,
    WagonState, Weather,
};
use crate::rng::CpythonRng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// Compiled-in JSON event skeletons from `src/escape_the_valley/data/event_skeletons.json`.
pub const EVENT_SKELETONS_JSON: &str =
    include_str!("../../../src/escape_the_valley/data/event_skeletons.json");

// ── Worldgen constants (worldgen.py:27-55) ──────────────────────────────────

pub const FIRST_NAMES: [&str; 18] = [
    "Elias",
    "Martha",
    "Thomas",
    "Agnes",
    "Willem",
    "Ruth",
    "Josiah",
    "Clara",
    "Silas",
    "Mercy",
    "Caleb",
    "Hannah",
    "Ezra",
    "Patience",
    "Amos",
    "Lydia",
    "Nathaniel",
    "Esther",
];

pub const TOWN_NAMES: [&str; 15] = [
    "Millford",
    "Redwater",
    "Stonecross",
    "Ashwell",
    "Blackmere",
    "Thornhaven",
    "Dryreach",
    "Coldspring",
    "Ironbrook",
    "Fellgate",
    "Whitmarsh",
    "Copperhill",
    "Ravenmoor",
    "Hazelwick",
    "Dunhollow",
];

pub const LANDMARK_NAMES: [&str; 20] = [
    "The Narrows",
    "Devil's Bend",
    "Widow's Pass",
    "Three Oaks Fork",
    "Salt Flat",
    "Bone Ridge",
    "Fog Hollow",
    "The Long Descent",
    "Sunken Meadow",
    "Crow's Perch",
    "Blind Canyon",
    "The Switchback",
    "Stillwater Crossing",
    "Whitefall",
    "The Gulch",
    "Ashen Bluffs",
    "Broken Bridge",
    "The Scar",
    "Shepherd's Rest",
    "Moss Landing",
];

pub const BIOME_PATTERNS: [[Biome; 5]; 5] = [
    [
        Biome::Plains,
        Biome::Forest,
        Biome::Alpine,
        Biome::Forest,
        Biome::Plains,
    ],
    [
        Biome::Plains,
        Biome::Swamp,
        Biome::Forest,
        Biome::Alpine,
        Biome::Forest,
    ],
    [
        Biome::Forest,
        Biome::Plains,
        Biome::Desert,
        Biome::Alpine,
        Biome::Plains,
    ],
    [
        Biome::Plains,
        Biome::Forest,
        Biome::Swamp,
        Biome::Forest,
        Biome::Alpine,
    ],
    [
        Biome::Forest,
        Biome::Alpine,
        Biome::Plains,
        Biome::Swamp,
        Biome::Forest,
    ],
];

/// Generate a branching route map with towns, landmarks, and caches (`worldgen.py::generate_map`).
pub fn generate_map(rng: &mut CpythonRng, num_nodes: usize) -> Vec<MapNode> {
    let pattern = *rng.choice(&BIOME_PATTERNS);
    let mut town_names: Vec<&str> = TOWN_NAMES.to_vec();
    rng.shuffle(&mut town_names);
    let mut landmark_names: Vec<&str> = LANDMARK_NAMES.to_vec();
    rng.shuffle(&mut landmark_names);

    let mut nodes: Vec<MapNode> = Vec::with_capacity(num_nodes);
    let mut town_idx = 0usize;
    let mut landmark_idx = 0usize;

    for i in 0..num_nodes {
        let progress = (i as f64) / ((num_nodes - 1) as f64);
        let biome_idx = (progress * ((pattern.len() - 1) as f64)) as usize;
        let biome = pattern[biome_idx.min(pattern.len() - 1)];

        // Note: Python evaluates `(i == 0) or (i == num_nodes - 1) or (i % rng.randint(4, 6) == 0)`
        // with short-circuiting `or`, so `rng.randint(4, 6)` is NOT called when `i == 0` or `i == num_nodes - 1`!
        let is_town = (i == 0) || (i == num_nodes - 1) || ((i as i128) % rng.randint(4, 6) == 0);

        let name = if is_town && town_idx < town_names.len() {
            let n = town_names[town_idx].to_string();
            town_idx += 1;
            n
        } else if landmark_idx < landmark_names.len() {
            let n = landmark_names[landmark_idx].to_string();
            landmark_idx += 1;
            n
        } else {
            format!("Mile {}", i * 10)
        };

        let base_temp = match biome {
            Biome::Plains => 20,
            Biome::Forest => 15,
            Biome::Desert => 35,
            Biome::Swamp => 25,
            Biome::Alpine => 5,
        };
        let temp = base_temp + (rng.randint(-5, 5) as i32);

        let hazard = (rng.randint(1, 8) as i32)
            + if matches!(biome, Biome::Alpine | Biome::Swamp) {
                2
            } else {
                0
            };

        // Note: Python short-circuits `biome != Biome.DESERT or rng.random() < 0.3`
        let water_available = biome != Biome::Desert || rng.random() < 0.3;

        nodes.push(MapNode {
            node_id: format!("node_{i:02}"),
            name,
            biome,
            hazard,
            water_available,
            temperature: temp,
            is_town,
            connections: Vec::new(),
            distance_to: BTreeMap::new(),
            cache_supplies: None,
        });
    }

    let len = nodes.len();
    for i in 0..len.saturating_sub(1) {
        let dist = rng.randint(8, 25) as i32;
        let next_id = nodes[i + 1].node_id.clone();
        nodes[i].connections.push(next_id.clone());
        nodes[i].distance_to.insert(next_id, dist);

        // Note: Python short-circuits `if i + 2 < len(nodes) and rng.random() < 0.2:`
        if i + 2 < len && rng.random() < 0.2 {
            let branch_dist = rng.randint(15, 35) as i32;
            let branch_id = nodes[i + 2].node_id.clone();
            nodes[i].connections.push(branch_id.clone());
            nodes[i].distance_to.insert(branch_id, branch_dist);
        }
    }

    place_caches(rng, &mut nodes, 2);
    nodes
}

/// Place supply caches on non-town interior nodes (`worldgen.py::_place_caches`).
pub fn place_caches(rng: &mut CpythonRng, nodes: &mut [MapNode], mut count: usize) {
    if nodes.len() <= 2 {
        return;
    }
    let mut eligible_indices: Vec<usize> = (1..(nodes.len() - 1))
        .filter(|&idx| !nodes[idx].is_town)
        .collect();

    if eligible_indices.len() < count {
        count = eligible_indices.len();
    }
    if count == 0 {
        return;
    }

    let mut weights: Vec<f64> = eligible_indices
        .iter()
        .map(|&idx| (nodes[idx].hazard + 1) as f64)
        .collect();

    let mut cache_node_indices: Vec<usize> = Vec::with_capacity(count);
    for _ in 0..count {
        if eligible_indices.is_empty() {
            break;
        }
        let picked_node_idx = *rng.weighted_choice(&eligible_indices, &weights);
        let pos = eligible_indices
            .iter()
            .position(|&idx| idx == picked_node_idx)
            .unwrap();
        cache_node_indices.push(picked_node_idx);
        eligible_indices.remove(pos);
        weights.remove(pos);
    }

    let cache_templates: [&[(&str, i32)]; 5] = [
        &[("food", 6), ("water", 5)],
        &[("parts", 1), ("cloth", 2)],
        &[("food", 4), ("meds", 1), ("ammo", 3)],
        &[("water", 6), ("salt", 2)],
        &[("food", 5), ("parts", 1), ("rope", 1)],
    ];

    for node_idx in cache_node_indices {
        let template = *rng.choice(&cache_templates);
        let mut map = BTreeMap::new();
        for &(k, v) in template {
            map.insert(k.to_string(), v);
        }
        nodes[node_idx].cache_supplies = Some(map);
    }
}

/// Generate weather for a given biome and day (`worldgen.py::generate_weather`).
/// Preserves Python dict insertion order (`Clear, Overcast, Rain, Storm, Fog, Snow, Hot`).
pub fn generate_weather(rng: &mut CpythonRng, biome: Biome, day: i32) -> Weather {
    let items: [Weather; 7] = [
        Weather::Clear,
        Weather::Overcast,
        Weather::Rain,
        Weather::Storm,
        Weather::Fog,
        Weather::Snow,
        Weather::Hot,
    ];
    let mut weights: [f64; 7] = [
        3.0, // Clear
        2.0, // Overcast
        1.5, // Rain
        0.5, // Storm
        1.0, // Fog
        0.0, // Snow
        0.0, // Hot
    ];

    match biome {
        Biome::Desert => {
            weights[6] = 4.0; // Hot
            weights[2] = 0.2; // Rain
            weights[4] = 0.1; // Fog
        }
        Biome::Swamp => {
            weights[4] = 3.0; // Fog
            weights[2] = 2.5; // Rain
        }
        Biome::Alpine => {
            weights[5] = 2.0; // Snow
            weights[3] = 1.5; // Storm
            weights[0] = 1.5; // Clear
        }
        Biome::Forest => {
            weights[4] = 2.0; // Fog
            weights[2] = 2.0; // Rain
        }
        Biome::Plains => {}
    }

    if day > 20 {
        weights[3] += 1.0; // Storm
        weights[5] += 0.5; // Snow
    }

    *rng.weighted_choice(&items, &weights)
}

/// Generate starting party with random traits (`worldgen.py::generate_party`).
pub fn generate_party(rng: &mut CpythonRng, size: usize) -> PartyState {
    let mut names: Vec<&str> = FIRST_NAMES.to_vec();
    rng.shuffle(&mut names);
    let all_traits = Trait::ALL;

    let mut members = Vec::with_capacity(size);
    for name in names.iter().take(size) {
        let num_traits = rng.randint(1, 2) as usize;
        let traits = rng.sample(&all_traits, num_traits);
        let health = rng.randint(80, 100) as i32;
        members.push(PartyMember {
            name: (*name).to_string(),
            health,
            condition: Condition::Healthy,
            traits,
            death_cause: String::new(),
        });
    }

    PartyState {
        members,
        morale: 70,
    }
}

/// Pick 1–2 twist modifiers for the run (`worldgen.py::pick_twists`).
pub fn pick_twists(rng: &mut CpythonRng) -> Vec<TwistModifier> {
    let count = rng.randint(1, 2) as usize;
    rng.sample(&TwistModifier::ALL, count)
}

/// Create a complete new game run (`worldgen.py::create_new_run`) and return both the
/// `RunState` and the `CpythonRng` positioned immediately after world generation.
pub fn create_new_run_with_rng(
    seed: i128,
    gm_profile: GMProfile,
    weirdness_level: i32,
) -> (RunState, CpythonRng) {
    let mut rng = CpythonRng::from_seed(seed);
    let nodes = generate_map(&mut rng, 25);
    let party = generate_party(&mut rng, 4);
    let twists = pick_twists(&mut rng);

    let doctrine = *rng.choice(&Doctrine::ALL);
    let taboo = *rng.choice(&Taboo::ALL);

    let first_dist = if nodes.len() > 1 {
        nodes[0]
            .distance_to
            .get(&nodes[1].node_id)
            .copied()
            .unwrap_or(15)
    } else {
        0
    };

    let mut total = 0;
    for node in &nodes[..nodes.len().saturating_sub(1)] {
        if let Some(first_conn) = node.connections.first() {
            total += node.distance_to.get(first_conn).copied().unwrap_or(15);
        }
    }

    let mut wagon = WagonState::default();
    let mods = doctrine_modifiers(doctrine.as_str());
    if let Some(cap_mult) = mods.capacity_mult {
        wagon.capacity = ((wagon.capacity as f64) * cap_mult) as i32;
    }

    let location_id = nodes[0].node_id.clone();
    let destination_id = if nodes.len() > 1 {
        nodes[1].node_id.clone()
    } else {
        nodes[0].node_id.clone()
    };

    let run = RunState {
        run_id: RunState::generate_run_id(seed),
        seed,
        day: 1,
        time_of_day: TimeOfDay::Morning,
        location_id,
        destination_id,
        distance_remaining: first_dist,
        total_distance: total,
        distance_traveled: 0,
        mode: "offline".to_string(),
        weirdness_level,
        uncanny_tokens: 2,
        gm_profile,
        twists,
        game_over: false,
        victory: false,
        cause_of_death: String::new(),
        ending: None,
        party,
        wagon,
        supplies: SuppliesState::with_defaults(),
        journal: Vec::new(),
        map_nodes: nodes,
        rng_counter: rng.counter,
        rng_state: None,
        recent_event_tags: Vec::new(),
        memory_cards: Vec::new(),
        resource_crises_seen: Vec::new(),
        doctrine: doctrine.as_str().to_string(),
        taboo: taboo.as_str().to_string(),
        rationing_steps: 0,
        escape_valve_cooldown: 0,
        last_action: String::new(),
        maintained_turns_remaining: 0,
        last_spoilage_day: 0,
        backpack: Default::default(),
        callout_level: "verbose".to_string(),
    };

    (run, rng)
}

/// Create a complete new game run with default `GMProfile::Fireside` and `weirdness_level = 2`.
pub fn create_new_run(seed: i128) -> RunState {
    create_new_run_with_rng(seed, GMProfile::Fireside, 2).0
}

// ── Event Loader (`event_loader.py` & `events.py` types) ─────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    Survival,
    Human,
    Folklore,
    Big,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolkloreType {
    Misinterpretation,
    Social,
    NaturalOddity,
    Uncanny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventOutcome {
    pub supplies_delta: BTreeMap<String, i32>,
    pub health_delta: i32,
    pub wagon_delta: i32,
    pub animals_health_delta: i32,
    pub morale_delta: i32,
    pub time_cost: i32,
    pub distance_delta: i32,
    pub special_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceTemplate {
    pub choice_id: String,
    pub label: String,
    pub action: String,
    pub style: String,
    pub risk_hint: String,
    pub cost_hint: String,
    #[serde(default)]
    pub outcome_weights: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSkeleton {
    pub event_id: String,
    pub title: String,
    pub category: EventCategory,
    pub tags: Vec<String>,
    pub severity: String,
    pub family: String,
    pub biome_filter: Option<Vec<Biome>>,
    pub weather_filter: Option<Vec<Weather>>,
    pub time_filter: Option<Vec<TimeOfDay>>,
    pub preconditions: Vec<String>,
    pub base_weight: f64,
    pub folklore_type: Option<FolkloreType>,
    pub costs_uncanny_token: bool,
    pub oregon_nod: bool,
    pub fallback_narration: String,
    pub fallback_choices: Vec<ChoiceTemplate>,
    pub outcome_templates: BTreeMap<String, EventOutcome>,
    pub gm_aside: String,
}

const MAX_CHOICES: usize = 4;

fn classify_category(tags: &[String]) -> EventCategory {
    if tags.iter().any(|t| t == "folklore") {
        EventCategory::Folklore
    } else if tags.iter().any(|t| t == "human") {
        EventCategory::Human
    } else {
        EventCategory::Survival
    }
}

fn classify_weirdness(band: i64) -> (bool, Option<FolkloreType>) {
    if band >= 3 {
        (true, Some(FolkloreType::Uncanny))
    } else if band == 2 {
        (false, Some(FolkloreType::NaturalOddity))
    } else {
        (false, None)
    }
}

fn infer_style(action: &str) -> &'static str {
    match action {
        "FORD" | "TRAVEL" | "HUNT" => "BOLD",
        "WAIT" | "REST" | "DETOUR" => "CAUTIOUS",
        _ => "NEUTRAL",
    }
}

fn resolve_resource_alias(key: &str) -> &str {
    match key {
        "oil" => "lantern_oil",
        other => other,
    }
}

fn as_i32_lenient(val: &Value) -> Option<i32> {
    if let Some(i) = val.as_i64() {
        i32::try_from(i).ok()
    } else {
        val.as_f64().map(|f| f as i32)
    }
}

/// Convert `engine_effect_profile` JSON object to `EventOutcome` (`event_loader.py::_convert_profile`).
pub fn convert_profile(
    profile: &serde_json::Map<String, Value>,
    costs_uncanny_token: bool,
) -> EventOutcome {
    let mut supplies = BTreeMap::new();
    let mut health = 0;
    let mut wagon = 0;
    let mut animals_health = 0;
    let mut morale = 0;
    let mut time_cost = 0;
    let mut distance = 0;

    for (key, val) in profile {
        match key.as_str() {
            "time_days" => {
                if let Some(v) = as_i32_lenient(val) {
                    time_cost = v;
                }
            }
            "distance" => {
                if let Some(v) = as_i32_lenient(val) {
                    distance = v;
                }
            }
            "morale" => {
                if let Some(v) = as_i32_lenient(val) {
                    morale = v;
                }
            }
            "health" => {
                if let Some(v) = as_i32_lenient(val) {
                    health = v;
                }
            }
            "condition" => {
                // Benign no-op flag (`event_loader.py:101-105`)
                continue;
            }
            "animals_health" => {
                if let Some(v) = as_i32_lenient(val) {
                    animals_health = v;
                }
            }
            "wagon_condition" => {
                if let Some(v) = as_i32_lenient(val) {
                    wagon = v;
                }
            }
            other => {
                let res_key = resolve_resource_alias(other);
                if !is_valid_resource_key(res_key) {
                    continue;
                }
                if let Some(v) = as_i32_lenient(val) {
                    supplies.insert(res_key.to_string(), v);
                }
            }
        }
    }

    let special_flags = if costs_uncanny_token {
        vec!["uncanny_token_spent".to_string()]
    } else {
        Vec::new()
    };

    EventOutcome {
        supplies_delta: supplies,
        health_delta: health,
        wagon_delta: wagon,
        animals_health_delta: animals_health,
        morale_delta: morale,
        time_cost,
        distance_delta: distance,
        special_flags,
    }
}

/// Convert a raw JSON event object to an `EventSkeleton` (`event_loader.py::_convert_event`).
pub fn convert_event(raw: &Value) -> Result<EventSkeleton, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| "event entry is not a JSON object".to_string())?;

    let event_id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "event entry missing a valid 'id'".to_string())?
        .to_string();

    let mut tags: Vec<String> = obj
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();

    let band = obj
        .get("weirdness_band")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let (costs_token, folklore_type) = classify_weirdness(band);
    let mut category = classify_category(&tags);

    if folklore_type == Some(FolkloreType::Uncanny) && !tags.iter().any(|t| t == "folklore:uncanny")
    {
        tags.push("folklore:uncanny".to_string());
    }

    if band >= 2 && category != EventCategory::Folklore {
        category = EventCategory::Folklore;
    }

    let raw_choices = obj
        .get("choices")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let capped_choices = if raw_choices.len() > MAX_CHOICES {
        &raw_choices[..MAX_CHOICES]
    } else {
        raw_choices
    };

    let mut choices = Vec::with_capacity(capped_choices.len());
    let mut outcomes = BTreeMap::new();

    for (i, ch_val) in capped_choices.iter().enumerate() {
        let ch = ch_val
            .as_object()
            .ok_or_else(|| format!("choice {i} is not an object"))?;
        let cid = char::from(b'A' + (i as u8)).to_string();
        let action = ch
            .get("intent_action")
            .and_then(|v| v.as_str())
            .unwrap_or("INVESTIGATE")
            .to_string();
        let style = infer_style(&action).to_string();
        let empty_map = serde_json::Map::new();
        let profile = ch
            .get("engine_effect_profile")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty_map);
        let outcome = convert_profile(profile, costs_token);
        let label = ch
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        choices.push(ChoiceTemplate {
            choice_id: cid.clone(),
            label,
            action,
            style,
            risk_hint: String::new(),
            cost_hint: String::new(),
            outcome_weights: BTreeMap::new(),
        });
        outcomes.insert(cid, outcome);
    }

    let raw_severity = obj
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let severity = match raw_severity {
        "low" | "medium" | "high" => raw_severity.to_string(),
        _ => "medium".to_string(),
    };

    let has_explicit_biome_filter = obj
        .get("biome_filter")
        .map(|v| match v {
            Value::Null => false,
            Value::Array(a) => !a.is_empty(),
            _ => true,
        })
        .unwrap_or(false);

    let biome_filter = if !has_explicit_biome_filter {
        let tagset: HashSet<String> = tags.iter().map(|t| t.to_lowercase()).collect();
        if tagset.contains("river") || tagset.contains("ford") {
            Some(
                Biome::ALL
                    .into_iter()
                    .filter(|&b| b != Biome::Desert)
                    .collect(),
            )
        } else {
            None
        }
    } else {
        None
    };

    let title = obj
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let fallback_narration = obj
        .get("narration_seed")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(EventSkeleton {
        event_id,
        title,
        category,
        tags,
        severity,
        family: String::new(),
        biome_filter,
        weather_filter: None,
        time_filter: None,
        preconditions: Vec::new(),
        base_weight: 1.0,
        folklore_type,
        costs_uncanny_token: costs_token,
        oregon_nod: false,
        fallback_narration,
        fallback_choices: choices,
        outcome_templates: outcomes,
        gm_aside: String::new(),
    })
}

/// Parse event skeletons from a JSON string (`event_loader.py::load_json_events`).
/// Malformed top-level JSON returns an empty `Vec`, and individual malformed entries are skipped.
pub fn load_json_events_from_str(json_str: &str) -> Vec<EventSkeleton> {
    let Ok(Value::Array(raw_list)) = serde_json::from_str::<Value>(json_str) else {
        return Vec::new();
    };
    let mut events = Vec::with_capacity(raw_list.len());
    for raw in &raw_list {
        if let Ok(ev) = convert_event(raw) {
            events.push(ev);
        }
    }
    events
}

/// Load the 200 compiled-in event skeletons from `event_skeletons.json`.
pub fn load_json_events() -> Vec<EventSkeleton> {
    load_json_events_from_str(EVENT_SKELETONS_JSON)
}
