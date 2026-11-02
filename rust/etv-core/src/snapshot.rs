use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::content::{hex_encode_32, ContentPack};
use crate::engine::StepEngine;
use crate::model::{GamePhase, IntentAction, PlayerIntent, RunState};
use crate::worldgen::create_new_run_with_content;

/// Mechanics-only per-turn record (inventory, health, morale, day, distance, wagon,
/// RNG state, event ID, choice ID, numerical deltas — excluding all shown strings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MechanicsStateRecord {
    pub day: i32,
    pub time_of_day: String,
    pub location_id: String,
    pub destination_id: String,
    pub distance_remaining: i32,
    pub distance_traveled: i32,
    pub total_distance: i32,
    pub supplies: BTreeMap<String, i32>,
    pub wagon_condition: i32,
    pub wagon_pace: String,
    pub animals_health: i32,
    pub wagon_capacity: i32,
    pub party_morale: i32,
    pub member_health: Vec<i32>,
    pub member_conditions: Vec<String>,
    pub rng_counter: u64,
    pub rng_state_hash: String,
    pub phase: String,
    pub event_id: String,
    pub choice_id: String,
    pub numerical_deltas: BTreeMap<String, i32>,
    pub rationing_steps: i32,
    pub escape_valve_cooldown: i32,
    pub maintained_turns_remaining: i32,
    pub last_spoilage_day: i32,
    pub game_over: bool,
    pub victory: bool,
}

/// Presentation-only per-turn record (title, narration, choice labels, outcome sentence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationRecord {
    pub event_title: String,
    pub narration: String,
    pub choice_labels: Vec<(String, String)>,
    pub outcome_sentence: String,
    pub lines: Vec<String>,
}

/// Combined per-turn Tier 2 snapshot containing `mechanics_state_hash` and `presentation_record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnSnapshot {
    pub turn: usize,
    pub mechanics_state_hash: String,
    pub presentation_record: PresentationRecord,
}

/// Per-seed Tier 2 snapshot trace across up to `max_turns` turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tier2SeedTrace {
    pub seed: i128,
    pub turns: Vec<TurnSnapshot>,
}

/// Full Tier 2 snapshot matrix across the 1,000-seed matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tier2SnapshotMatrix {
    pub content_hash: String,
    pub seed_count: usize,
    pub total_turns: usize,
    pub traces: Vec<Tier2SeedTrace>,
}

/// Build the canonical 1,000-seed matrix (matching the 1,000 seeds in the oracle suite).
pub fn build_1000_seeds() -> Vec<i128> {
    let special: [i128; 14] = [
        0,
        1,
        42,
        -1,
        -42,
        -999_999_999,
        -98_765_432_101_234_567_890_i128,
        (1_i128 << 31) - 1,
        1_i128 << 31,
        (1_i128 << 32) + 7,
        (1_i128 << 62) + 1337,
        (1_i128 << 64) + 13,
        (1_i128 << 99) + 0x1234_5678_9abc_def0_1234_5678_i128,
        -(1_i128 << 80) - 99,
    ];
    let mut seeds = Vec::with_capacity(1000);
    seeds.extend_from_slice(&special);
    let mut cand: i128 = 2;
    while seeds.len() < 1000 {
        let val = if cand % 9 == 0 { -cand } else { cand };
        if !seeds.contains(&val) {
            seeds.push(val);
        }
        cand += 1;
    }
    seeds
}

fn sha256_str(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    hex_encode_32(&bytes)
}

fn compute_rng_state_hash(state: &RunState) -> String {
    let raw = serde_json::to_string(&state.rng_state).unwrap_or_default();
    sha256_str(&raw)
}

/// Compute the `mechanics_state_hash` for a single turn (excluding all shown strings).
pub fn compute_mechanics_state_hash(
    state: &RunState,
    phase: GamePhase,
    event_id: &str,
    choice_id: &str,
    numerical_deltas: &BTreeMap<String, i32>,
) -> String {
    let record = MechanicsStateRecord {
        day: state.day,
        time_of_day: state.time_of_day.as_str().to_string(),
        location_id: state.location_id.clone(),
        destination_id: state.destination_id.clone(),
        distance_remaining: state.distance_remaining,
        distance_traveled: state.distance_traveled,
        total_distance: state.total_distance,
        supplies: state.supplies.to_dict(),
        wagon_condition: state.wagon.condition,
        wagon_pace: state.wagon.pace.as_str().to_string(),
        animals_health: state.wagon.animals_health,
        wagon_capacity: state.wagon.capacity,
        party_morale: state.party.morale,
        member_health: state.party.members.iter().map(|m| m.health).collect(),
        member_conditions: state
            .party
            .members
            .iter()
            .map(|m| m.condition.as_str().to_string())
            .collect(),
        rng_counter: state.rng_counter,
        rng_state_hash: compute_rng_state_hash(state),
        phase: phase.as_str().to_string(),
        event_id: event_id.to_string(),
        choice_id: choice_id.to_string(),
        numerical_deltas: numerical_deltas.clone(),
        rationing_steps: state.rationing_steps,
        escape_valve_cooldown: state.escape_valve_cooldown,
        maintained_turns_remaining: state.maintained_turns_remaining,
        last_spoilage_day: state.last_spoilage_day,
        game_over: state.game_over,
        victory: state.victory,
    };
    let canonical = serde_json::to_string(&record).expect("MechanicsStateRecord must serialize");
    sha256_str(&canonical)
}

/// Deterministic policy step for Tier 2 snapshot runs (depends only on `seed_idx`, `turn`,
/// `phase`, and offered `choice_id`s — never on shown text or the game RNG).
fn pick_tier2_intent(engine: &StepEngine, seed_idx: usize, turn: usize) -> (PlayerIntent, String) {
    match engine.phase {
        GamePhase::Event => {
            let choices = engine.pending_event_choices();
            if choices.is_empty() {
                (
                    PlayerIntent::new(IntentAction::Choose).with_choice("A"),
                    "A".to_string(),
                )
            } else {
                let idx = (seed_idx.wrapping_mul(31).wrapping_add(turn)) % choices.len();
                let cid = choices[idx].id.clone();
                (
                    PlayerIntent::new(IntentAction::Choose).with_choice(&cid),
                    cid,
                )
            }
        }
        GamePhase::Route => {
            let routes = engine.pending_routes();
            let count = routes.len().clamp(1, 4);
            let idx = (seed_idx.wrapping_add(turn)) % count;
            let cid = ["A", "B", "C", "D"][idx].to_string();
            (
                PlayerIntent::new(IntentAction::Choose).with_choice(&cid),
                cid,
            )
        }
        GamePhase::Camp => {
            let selector = (seed_idx.wrapping_mul(17).wrapping_add(turn)) % 12;
            let intent = match selector {
                0 if turn == 1 => {
                    let pace = if seed_idx.is_multiple_of(2) {
                        "steady"
                    } else {
                        "hard"
                    };
                    PlayerIntent::new(IntentAction::ChangePace).with_pace(pace)
                }
                3 if engine.state.supplies.ammo() > 0 && engine.state.supplies.food() < 35 => {
                    PlayerIntent::new(IntentAction::Hunt)
                }
                6 if engine.state.wagon.condition < 75 && engine.state.supplies.parts() > 0 => {
                    PlayerIntent::new(IntentAction::Repair)
                }
                9 if engine.state.party.morale < 65 => PlayerIntent::new(IntentAction::Rest),
                _ => PlayerIntent::new(IntentAction::Travel),
            };
            (intent, String::new())
        }
        GamePhase::GameOver => (PlayerIntent::new(IntentAction::Travel), String::new()),
    }
}

/// Run a single seed under `content` for up to `max_turns` turns and return its `Tier2SeedTrace`.
pub fn run_tier2_seed_trace(
    seed: i128,
    seed_idx: usize,
    content: &ContentPack,
    max_turns: usize,
) -> Tier2SeedTrace {
    let state = create_new_run_with_content(seed, content);
    let mut engine = StepEngine::with_content(state, content);
    let mut turns = Vec::with_capacity(max_turns);

    for turn in 1..=max_turns {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        let pre_event_id = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or_default();
        let (intent, chosen_choice_id) = pick_tier2_intent(&engine, seed_idx, turn);
        let msgs = engine.step(&intent);
        let post_event_id = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or(pre_event_id);

        engine.state.rng_counter = engine.rng.counter;
        engine.state.rng_state = Some(engine.rng.getstate());

        let mechanics_state_hash = compute_mechanics_state_hash(
            &engine.state,
            engine.phase,
            &post_event_id,
            &chosen_choice_id,
            &msgs.outcome_deltas,
        );

        let narration = engine
            .pending_event()
            .map(|e| e.fallback_narration.clone())
            .unwrap_or_default();
        let choice_labels = msgs
            .event_choices
            .iter()
            .map(|c| (c.id.clone(), c.label.clone()))
            .collect();

        let presentation_record = PresentationRecord {
            event_title: msgs.event_title,
            narration,
            choice_labels,
            outcome_sentence: msgs.outcome_narration,
            lines: msgs.lines,
        };

        turns.push(TurnSnapshot {
            turn,
            mechanics_state_hash,
            presentation_record,
        });
    }

    Tier2SeedTrace { seed, turns }
}

/// Run the Tier 2 snapshot generator across the 1,000-seed matrix under `content`.
pub fn run_tier2_snapshot_matrix(
    content: &ContentPack,
    max_turns_per_seed: usize,
) -> Tier2SnapshotMatrix {
    let seeds = build_1000_seeds();
    let mut traces = Vec::with_capacity(seeds.len());
    let mut total_turns = 0usize;

    for (idx, &seed) in seeds.iter().enumerate() {
        let trace = run_tier2_seed_trace(seed, idx, content, max_turns_per_seed);
        total_turns += trace.turns.len();
        traces.push(trace);
    }

    Tier2SnapshotMatrix {
        content_hash: content.content_hash_hex.clone(),
        seed_count: seeds.len(),
        total_turns,
        traces,
    }
}
