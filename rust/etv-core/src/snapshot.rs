use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::content::{hex_encode_32, ContentPack};
use crate::engine::StepEngine;
use crate::events::EVENT_ID_COOLDOWN_WINDOW;
use crate::ledger::trail_ledger;
use crate::model::{BackpackState, GamePhase, IntentAction, MapNode, PlayerIntent, RunState};
use crate::narrator::{
    build_narration_context_with_phase, evaluate_turn_matters, Narrator, NullNarrator,
    PreTurnObservation,
};
use crate::worldgen::create_new_run_with_content;

/// Mechanical projection of a [`crate::model::JournalEntry`], retaining `day`, `location`,
/// `event_id`, `choice_id` (the ID prefix before `':'` in `choice_made`), `deltas`, and `tags`
/// while excluding prose fields (`scene_title`, `narration`, `choice_label`, `outcome`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalMechanicsRecord {
    pub day: i32,
    pub location: String,
    pub event_id: String,
    pub choice_id: String,
    pub deltas: BTreeMap<String, i32>,
    pub tags: Vec<String>,
}

/// Mechanical projection of a [`crate::model::MemoryCard`], retaining `id`, `kind`, `tags`,
/// `day_created`, `day_last_seen`, `entities`, `salience_bits` (`f64::to_bits`), `cooldown_until`,
/// and `source` while excluding prose fields (`title`, `text`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCardMechanicsRecord {
    pub id: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub day_created: i32,
    pub day_last_seen: i32,
    pub entities: Vec<String>,
    pub salience_bits: u64,
    pub cooldown_until: i32,
    pub source: String,
}

/// Mechanics-only per-turn record covering every [`RunState`] field that can affect a later
/// mechanic while excluding all text-only fields:
/// - **IN (`RunState` fields):** `seed`, `day`, `time_of_day`, `location_id`, `destination_id`,
///   `distance_remaining`, `total_distance`, `distance_traveled`, `mode`, `weirdness_level`,
///   `uncanny_tokens`, `gm_profile`, `twists`, `game_over`, `victory`, `cause_of_death`,
///   `ending` (`tier` + `facts`, excluding prose `headline`), `party` (`morale`, `member_names`,
///   `member_health`, `member_conditions`, `member_traits`, `member_death_causes`), `wagon`
///   (`condition`, `pace`, `animals_health`, `capacity`), `supplies`, `journal`
///   ([`JournalMechanicsRecord`], excluding `scene_title`, `narration`, `choice_label`, `outcome`),
///   `map_nodes` (including `cache_supplies`), `rng_counter`, `rng_state` (`rng_state_hash`),
///   `recent_event_tags`, `recent_event_ids` (hydrated from `journal` when `None`), `memory_cards`
///   ([`MemoryCardMechanicsRecord`], excluding `title` and `text`), `resource_crises_seen`,
///   `doctrine`, `taboo`, `rationing_steps`, `escape_valve_cooldown`, `last_action`,
///   `maintained_turns_remaining`, `last_spoilage_day`, and `backpack`.
/// - **OUT (`RunState` fields):** `run_id` (contains random OS CSPRNG salt), `callout_level`
///   (UI presentation verbosity setting), `content_hash` (changes on any content text edit),
///   and prose strings on `ending`, `journal`, and `memory_cards`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MechanicsStateRecord {
    pub seed: String,
    pub day: i32,
    pub time_of_day: String,
    pub location_id: String,
    pub destination_id: String,
    pub distance_remaining: i32,
    pub distance_traveled: i32,
    pub total_distance: i32,
    pub mode: String,
    pub weirdness_level: i32,
    pub uncanny_tokens: i32,
    pub gm_profile: String,
    pub twists: Vec<String>,
    pub supplies: BTreeMap<String, i32>,
    pub wagon_condition: i32,
    pub wagon_pace: String,
    pub animals_health: i32,
    pub wagon_capacity: i32,
    pub party_morale: i32,
    pub member_names: Vec<String>,
    pub member_health: Vec<i32>,
    pub member_conditions: Vec<String>,
    pub member_traits: Vec<Vec<String>>,
    pub member_death_causes: Vec<String>,
    pub journal: Vec<JournalMechanicsRecord>,
    pub map_nodes: Vec<MapNode>,
    pub rng_counter: u64,
    pub rng_state_hash: String,
    pub recent_event_tags: Vec<String>,
    pub recent_event_ids: Vec<String>,
    pub memory_cards: Vec<MemoryCardMechanicsRecord>,
    pub resource_crises_seen: Vec<String>,
    pub doctrine: String,
    pub taboo: String,
    pub phase: String,
    pub event_id: String,
    pub choice_id: String,
    pub numerical_deltas: BTreeMap<String, i32>,
    pub rationing_steps: i32,
    pub escape_valve_cooldown: i32,
    pub last_action: String,
    pub maintained_turns_remaining: i32,
    pub last_spoilage_day: i32,
    pub backpack: BackpackState,
    pub game_over: bool,
    pub victory: bool,
    pub cause_of_death: String,
    pub ending_tier: Option<String>,
    pub ending_facts: Option<BTreeMap<String, Value>>,
}

/// Presentation-only per-turn record (title, narration, choice labels, outcome sentence,
/// narrator seam call count and text, and end-of-run Trail Ledger lines).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationRecord {
    pub event_title: String,
    pub narration: String,
    pub choice_labels: Vec<(String, String)>,
    pub outcome_sentence: String,
    pub lines: Vec<String>,
    pub turn_matters: bool,
    pub narrator_calls: usize,
    pub narrator_scene_text: Option<String>,
    pub narrator_text: Option<String>,
    pub displayed_line: String,
    pub trail_ledger_lines: Vec<String>,
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
    let recent_event_ids = match &state.recent_event_ids.0 {
        Some(ids) => ids.clone(),
        None => {
            let from_journal: Vec<String> = state
                .journal
                .iter()
                .filter(|j| !j.choice_made.is_empty() && !j.event_id.is_empty())
                .map(|j| j.event_id.clone())
                .collect();
            let start = from_journal.len().saturating_sub(EVENT_ID_COOLDOWN_WINDOW);
            from_journal[start..].to_vec()
        }
    };

    let journal = state
        .journal
        .iter()
        .map(|j| JournalMechanicsRecord {
            day: j.day,
            location: j.location.clone(),
            event_id: j.event_id.clone(),
            choice_id: j
                .choice_made
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_string(),
            deltas: j.deltas.clone(),
            tags: j.tags.clone(),
        })
        .collect();

    let memory_cards = state
        .memory_cards
        .iter()
        .map(|c| MemoryCardMechanicsRecord {
            id: c.id.clone(),
            kind: c.kind.clone(),
            tags: c.tags.clone(),
            day_created: c.day_created,
            day_last_seen: c.day_last_seen,
            entities: c.entities.clone(),
            salience_bits: c.salience.to_bits(),
            cooldown_until: c.cooldown_until,
            source: c.source.clone(),
        })
        .collect();

    let record = MechanicsStateRecord {
        seed: state.seed.to_string(),
        day: state.day,
        time_of_day: state.time_of_day.as_str().to_string(),
        location_id: state.location_id.clone(),
        destination_id: state.destination_id.clone(),
        distance_remaining: state.distance_remaining,
        distance_traveled: state.distance_traveled,
        total_distance: state.total_distance,
        mode: state.mode.clone(),
        weirdness_level: state.weirdness_level,
        uncanny_tokens: state.uncanny_tokens,
        gm_profile: state.gm_profile.as_str().to_string(),
        twists: state
            .twists
            .iter()
            .map(|t| t.as_str().to_string())
            .collect(),
        supplies: state.supplies.to_dict(),
        wagon_condition: state.wagon.condition,
        wagon_pace: state.wagon.pace.as_str().to_string(),
        animals_health: state.wagon.animals_health,
        wagon_capacity: state.wagon.capacity,
        party_morale: state.party.morale,
        member_names: state.party.members.iter().map(|m| m.name.clone()).collect(),
        member_health: state.party.members.iter().map(|m| m.health).collect(),
        member_conditions: state
            .party
            .members
            .iter()
            .map(|m| m.condition.as_str().to_string())
            .collect(),
        member_traits: state
            .party
            .members
            .iter()
            .map(|m| m.traits.iter().map(|t| t.as_str().to_string()).collect())
            .collect(),
        member_death_causes: state
            .party
            .members
            .iter()
            .map(|m| m.death_cause.clone())
            .collect(),
        journal,
        map_nodes: state.map_nodes.clone(),
        rng_counter: state.rng_counter,
        rng_state_hash: compute_rng_state_hash(state),
        recent_event_tags: state.recent_event_tags.clone(),
        recent_event_ids,
        memory_cards,
        resource_crises_seen: state.resource_crises_seen.clone(),
        doctrine: state.doctrine.clone(),
        taboo: state.taboo.clone(),
        phase: phase.as_str().to_string(),
        event_id: event_id.to_string(),
        choice_id: choice_id.to_string(),
        numerical_deltas: numerical_deltas.clone(),
        rationing_steps: state.rationing_steps,
        escape_valve_cooldown: state.escape_valve_cooldown,
        last_action: state.last_action.clone(),
        maintained_turns_remaining: state.maintained_turns_remaining,
        last_spoilage_day: state.last_spoilage_day,
        backpack: state.backpack.clone(),
        game_over: state.game_over,
        victory: state.victory,
        cause_of_death: state.cause_of_death.clone(),
        ending_tier: state.ending.as_ref().map(|e| e.tier.clone()),
        ending_facts: state.ending.as_ref().map(|e| e.facts.clone()),
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

/// Run a single seed under `content` for up to `max_turns` turns using the default [`NullNarrator`]
/// and return its [`Tier2SeedTrace`].
pub fn run_tier2_seed_trace(
    seed: i128,
    seed_idx: usize,
    content: &ContentPack,
    max_turns: usize,
) -> Tier2SeedTrace {
    run_tier2_seed_trace_with_narrator(seed, seed_idx, content, max_turns, &NullNarrator)
}

/// Run a single seed under `content` for up to `max_turns` turns with a plugged-in [`Narrator`]
/// and return its [`Tier2SeedTrace`].
///
/// The narrator is invoked once on every shown turn (every loop iteration that executes a turn
/// step and shows the player a turn result), receives an owned read-only
/// [`crate::narrator::NarrationContext`] built from the engine's facts, and its call count and
/// returned text are recorded in [`PresentationRecord`] only — never in [`MechanicsStateRecord`]
/// or `mechanics_state_hash`. When the run reaches `GameOver`, the Trail Ledger lines are also
/// recorded in `presentation_record.trail_ledger_lines` only.
pub fn run_tier2_seed_trace_with_narrator<N: Narrator + ?Sized>(
    seed: i128,
    seed_idx: usize,
    content: &ContentPack,
    max_turns: usize,
    narrator: &N,
) -> Tier2SeedTrace {
    let state = create_new_run_with_content(seed, content);
    let mut engine = StepEngine::with_content(state, content);
    let mut turns = Vec::with_capacity(max_turns);
    let mut narrator_calls: usize = 0;

    #[allow(clippy::explicit_counter_loop)]
    for turn in 1..=max_turns {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        let pre_obs = PreTurnObservation::capture(engine.phase, &engine.state);
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

        let flags = evaluate_turn_matters(&pre_obs, &intent, &msgs, &engine.state);
        let turn_matters = flags.any();
        narrator_calls += 1;
        let ctx = build_narration_context_with_phase(
            &engine.state,
            engine.phase,
            &post_event_id,
            &chosen_choice_id,
            &msgs,
            flags,
        );
        let turn_narration = narrator.narrate_turn(&ctx);
        let narrator_scene_text = turn_narration.scene;
        let narrator_text = turn_narration.outcome;
        let displayed_line = narrator_text
            .clone()
            .unwrap_or_else(|| ctx.fallback_line().to_string());

        let trail_ledger_lines = if engine.phase == GamePhase::GameOver || engine.state.game_over {
            trail_ledger(&engine.state)
        } else {
            Vec::new()
        };

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
            turn_matters,
            narrator_calls,
            narrator_scene_text,
            narrator_text,
            displayed_line,
            trail_ledger_lines,
        };

        turns.push(TurnSnapshot {
            turn,
            mechanics_state_hash,
            presentation_record,
        });
    }

    Tier2SeedTrace { seed, turns }
}

/// Run the Tier 2 snapshot generator across the 1,000-seed matrix under `content` using
/// [`NullNarrator`].
pub fn run_tier2_snapshot_matrix(
    content: &ContentPack,
    max_turns_per_seed: usize,
) -> Tier2SnapshotMatrix {
    run_tier2_snapshot_matrix_with_narrator(content, max_turns_per_seed, &NullNarrator)
}

/// Run the Tier 2 snapshot generator across the 1,000-seed matrix under `content` with a
/// plugged-in [`Narrator`].
pub fn run_tier2_snapshot_matrix_with_narrator<N: Narrator + ?Sized>(
    content: &ContentPack,
    max_turns_per_seed: usize,
    narrator: &N,
) -> Tier2SnapshotMatrix {
    let seeds = build_1000_seeds();
    let mut traces = Vec::with_capacity(seeds.len());
    let mut total_turns = 0usize;

    for (idx, &seed) in seeds.iter().enumerate() {
        let trace =
            run_tier2_seed_trace_with_narrator(seed, idx, content, max_turns_per_seed, narrator);
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
