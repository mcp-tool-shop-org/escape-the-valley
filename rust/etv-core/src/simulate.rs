//! Balance simulation harness: drives the real [`StepEngine`] through many seeded runs under a
//! fixed play policy and aggregates what happened (win rate, days, deaths, event coverage).
//!
//! This module only reads engine state and calls `StepEngine::step`; it changes no mechanics,
//! draws no random numbers of its own, and every policy is a pure function of the engine's
//! visible state, the run's index in the batch and the turn number. Reports use integers only
//! (percentages are formatted from integer counts) and `BTreeMap`/`Vec` only, so the same
//! inputs always give byte-identical output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::content::ContentPack;
use crate::engine::{compute_ending, deaths_by_cause, StepEngine};
use crate::model::{Condition, GamePhase, IntentAction, Pace, PlayerIntent};
use crate::ruleset::Ruleset;
use crate::snapshot::compute_mechanics_state_hash;
use crate::difficulty::{apply_starting_levers, Difficulty, DifficultyLevers};
use crate::worldgen::create_new_run_with_content;

/// Default turn cap per run (one turn is one `StepEngine::step` call).
pub const DEFAULT_MAX_TURNS: usize = 500;
/// Default number of seeds when `--seeds` is not given.
pub const DEFAULT_SEEDS: u64 = 100;
/// How many most / least frequent events the report lists.
pub const TOP_EVENTS: usize = 15;

/// `cautious` rests when any living member's health is below this value ("clearly hurt";
/// chosen up front, not tuned against results).
pub const CAUTIOUS_REST_HEALTH_BELOW: i32 = 40;
/// `cautious` repairs when the wagon condition is below this value (60 is the physics threshold
/// where the wagon starts to slow down, see `compute_travel_distance`).
pub const CAUTIOUS_REPAIR_CONDITION_BELOW: i32 = 60;
/// `cautious` hunts when food is below this value and ammo is on hand.
pub const CAUTIOUS_HUNT_FOOD_BELOW: i32 = 40;

/// Thresholds used by `rotating`; these copy the Tier 2 snapshot driver so the two agree.
const ROTATING_HUNT_FOOD_BELOW: i32 = 35;
const ROTATING_REPAIR_CONDITION_BELOW: i32 = 75;
const ROTATING_REST_MORALE_BELOW: i32 = 65;

/// A play policy. Each is a pure function of visible state and turn number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    /// Always the first offered choice; always travel.
    First,
    /// Always the last offered choice; always travel.
    Last,
    /// Rest when hurt or exhausted, repair a damaged wagon, hunt when food is low, else travel;
    /// prefers a choice with no advertised cost.
    Cautious,
    /// Hard pace, always travel, last choice.
    Reckless,
    /// The Tier 2 snapshot style mix of choices and camp actions.
    Rotating,
}

impl Policy {
    /// Every policy, in report order.
    pub const ALL: [Policy; 5] = [
        Policy::First,
        Policy::Last,
        Policy::Cautious,
        Policy::Reckless,
        Policy::Rotating,
    ];

    /// Lower-case name used on the command line and in reports.
    pub fn name(self) -> &'static str {
        match self {
            Policy::First => "first",
            Policy::Last => "last",
            Policy::Cautious => "cautious",
            Policy::Reckless => "reckless",
            Policy::Rotating => "rotating",
        }
    }

    /// Parse a policy name (exact, lower-case).
    pub fn parse(s: &str) -> Option<Policy> {
        Policy::ALL.into_iter().find(|p| p.name() == s)
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Reached the valley.
    Victory,
    /// Game over without victory (the whole party died, or the wagon was lost).
    Died,
    /// Still running when the turn cap was reached.
    TurnCap,
}

/// Everything recorded about one simulated run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub seed: String,
    pub outcome: Outcome,
    pub turns: u64,
    pub day: i64,
    pub distance_traveled: i64,
    pub total_distance: i64,
    pub party_size: u64,
    pub survivors: u64,
    /// `compute_ending` tier for finished runs, `"unfinished"` for a turn-capped run.
    pub ending_tier: String,
    pub deaths_by_cause: BTreeMap<String, i64>,
    /// Events presented to the party, by `event_id`.
    pub events_seen: BTreeMap<String, u64>,
    /// Choices taken, keyed `"<event_id>:<choice_id>"`.
    pub choices_taken: BTreeMap<String, u64>,
    /// Times an event was presented on the travel step right after the same event.
    pub repeats: BTreeMap<String, u64>,
    pub final_supplies: BTreeMap<String, i64>,
    /// Mechanics state hash of the final state (see `snapshot::compute_mechanics_state_hash`).
    pub final_state_hash: String,
}

/// Aggregate over many runs of one policy. Integers and ordered containers only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimReport {
    pub policy: String,
    pub max_turns: u64,
    pub runs: u64,
    pub wins: u64,
    pub deaths: u64,
    pub turn_caps: u64,
    /// Lower median of the days reached (element `(n - 1) / 2` of the sorted list); 0 if no runs.
    pub median_days: i64,
    pub min_days: i64,
    pub max_days: i64,
    pub ending_tiers: BTreeMap<String, u64>,
    /// Party members lost, summed over runs, by attributed cause.
    pub deaths_by_cause: BTreeMap<String, i64>,
    /// Every event seen with its total count.
    pub events_seen: BTreeMap<String, u64>,
    /// The most frequent events: count descending, then id ascending.
    pub top_events: Vec<(String, u64)>,
    /// The least frequent events among those seen: count ascending, then id ascending.
    pub bottom_events: Vec<(String, u64)>,
    /// Events in the content pack never presented in any run.
    pub events_never_seen: Vec<String>,
    /// Per-event count of back-to-back presentations on consecutive travel steps.
    pub repeats_by_event: BTreeMap<String, u64>,
    pub repeats_total: u64,
    /// Choices taken, keyed `"<event_id>:<choice_id>"`.
    pub choices_taken: BTreeMap<String, u64>,
    /// `"<event_id>:<choice_id>"` pairs in the content pack never taken.
    pub choices_never_taken: Vec<String>,
}

/// Percentage of `wins` in `runs` to one decimal place, e.g. `"12.3%"`, from integer maths
/// (rounded half up). `runs == 0` gives `"n/a"`.
pub fn percent_string(wins: u64, runs: u64) -> String {
    if runs == 0 {
        return "n/a".to_string();
    }
    let tenths = (u128::from(wins) * 1000 + u128::from(runs) / 2) / u128::from(runs);
    format!("{}.{}%", tenths / 10, tenths % 10)
}

/// Choose the next intent. Returns the intent and the chosen choice id (empty when the action is
/// not a choice). Pure in `engine`'s visible state, `seed_idx` (the run's position in the batch;
/// only `rotating` reads it) and `turn`.
pub fn choose_intent(
    policy: Policy,
    engine: &StepEngine,
    seed_idx: usize,
    turn: usize,
) -> (PlayerIntent, String) {
    match policy {
        Policy::Rotating => rotating_intent(engine, seed_idx, turn),
        Policy::First | Policy::Last | Policy::Cautious | Policy::Reckless => {
            simple_intent(policy, engine)
        }
    }
}

fn pick_choice(policy: Policy, engine: &StepEngine) -> String {
    let choices = engine.pending_event_choices();
    let Some(first) = choices.first() else {
        return "A".to_string();
    };
    let chosen = match policy {
        Policy::Last | Policy::Reckless => choices.last().unwrap_or(first),
        Policy::Cautious => choices
            .iter()
            .find(|c| c.cost_hint.trim().is_empty())
            .unwrap_or(first),
        Policy::First | Policy::Rotating => first,
    };
    chosen.id.clone()
}

/// Under the V2 rules the engine refuses a choice the party cannot afford while an affordable
/// one is offered. A script that asked for such a choice picks again, as a player would:
/// `last` and `reckless` take the last affordable choice; `cautious` takes the first affordable
/// choice with no advertised cost, else the first affordable; the others take the first
/// affordable. A choice that is affordable, or an event with no affordable choice shown, is
/// left as picked.
pub fn repick_if_unaffordable(policy: Policy, engine: &StepEngine, choice_id: &str) -> String {
    let choices = engine.pending_event_choices();
    let picked_ok = choices
        .iter()
        .find(|c| c.id == choice_id)
        .is_none_or(|c| c.affordable);
    let mut affordable = choices.iter().filter(|c| c.affordable);
    if picked_ok {
        return choice_id.to_string();
    }
    let repick = match policy {
        Policy::Last | Policy::Reckless => affordable.next_back(),
        Policy::Cautious => choices
            .iter()
            .find(|c| c.affordable && c.cost_hint.trim().is_empty())
            .or_else(|| affordable.next()),
        Policy::First | Policy::Rotating => affordable.next(),
    };
    repick
        .map(|c| c.id.clone())
        .unwrap_or_else(|| choice_id.to_string())
}

fn simple_intent(policy: Policy, engine: &StepEngine) -> (PlayerIntent, String) {
    match engine.phase {
        GamePhase::Event => {
            let cid = pick_choice(policy, engine);
            (
                PlayerIntent::new(IntentAction::Choose).with_choice(&cid),
                cid,
            )
        }
        GamePhase::Route => {
            let count = engine.pending_routes().len().clamp(1, 4);
            let idx = match policy {
                Policy::Last | Policy::Reckless => count - 1,
                _ => 0,
            };
            let cid = ["A", "B", "C", "D"][idx].to_string();
            (
                PlayerIntent::new(IntentAction::Choose).with_choice(&cid),
                cid,
            )
        }
        GamePhase::Camp => (camp_intent(policy, engine), String::new()),
        GamePhase::GameOver => (PlayerIntent::new(IntentAction::Travel), String::new()),
    }
}

fn camp_intent(policy: Policy, engine: &StepEngine) -> PlayerIntent {
    let st = &engine.state;
    match policy {
        Policy::Reckless if st.wagon.pace != Pace::Hard => {
            PlayerIntent::new(IntentAction::ChangePace).with_pace("hard")
        }
        Policy::Cautious => {
            let hurt = st.party.members.iter().any(|m| {
                m.is_alive()
                    && (m.health < CAUTIOUS_REST_HEALTH_BELOW
                        || m.condition == Condition::Exhausted)
            });
            // Resting may not cure exhaustion, so never rest twice in a row: that would spin
            // until the turn cap.
            if hurt && st.last_action != "REST" {
                PlayerIntent::new(IntentAction::Rest)
            } else if st.wagon.condition < CAUTIOUS_REPAIR_CONDITION_BELOW
                && st.supplies.parts() > 0
            {
                PlayerIntent::new(IntentAction::Repair)
            } else if st.supplies.food() < CAUTIOUS_HUNT_FOOD_BELOW && st.supplies.ammo() > 0 {
                PlayerIntent::new(IntentAction::Hunt)
            } else {
                PlayerIntent::new(IntentAction::Travel)
            }
        }
        _ => PlayerIntent::new(IntentAction::Travel),
    }
}

/// The Tier 2 snapshot driver's mix (`snapshot.rs::pick_tier2_intent`), re-stated here because
/// that function is private. A test checks the two give identical mechanics hashes.
fn rotating_intent(engine: &StepEngine, seed_idx: usize, turn: usize) -> (PlayerIntent, String) {
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
            let count = engine.pending_routes().len().clamp(1, 4);
            let idx = (seed_idx.wrapping_add(turn)) % count;
            let cid = ["A", "B", "C", "D"][idx].to_string();
            (
                PlayerIntent::new(IntentAction::Choose).with_choice(&cid),
                cid,
            )
        }
        GamePhase::Camp => {
            let st = &engine.state;
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
                3 if st.supplies.ammo() > 0 && st.supplies.food() < ROTATING_HUNT_FOOD_BELOW => {
                    PlayerIntent::new(IntentAction::Hunt)
                }
                6 if st.wagon.condition < ROTATING_REPAIR_CONDITION_BELOW
                    && st.supplies.parts() > 0 =>
                {
                    PlayerIntent::new(IntentAction::Repair)
                }
                9 if st.party.morale < ROTATING_REST_MORALE_BELOW => {
                    PlayerIntent::new(IntentAction::Rest)
                }
                _ => PlayerIntent::new(IntentAction::Travel),
            };
            (intent, String::new())
        }
        GamePhase::GameOver => (PlayerIntent::new(IntentAction::Travel), String::new()),
    }
}

/// Run one seed under `policy` for up to `max_turns` turns and record what happened.
/// `seed_idx` is the run's position in its batch (used by `rotating` only).
pub fn run_one(
    seed: i128,
    seed_idx: usize,
    policy: Policy,
    max_turns: usize,
    content: &ContentPack,
) -> RunRecord {
    run_one_with_ruleset(seed, seed_idx, policy, max_turns, content, Ruleset::Legacy)
}

/// [`run_one`] under an explicit [`Ruleset`]. `Legacy` is exactly `run_one`.
pub fn run_one_with_ruleset(
    seed: i128,
    seed_idx: usize,
    policy: Policy,
    max_turns: usize,
    content: &ContentPack,
    ruleset: Ruleset,
) -> RunRecord {
    run_one_with_levers(
        seed,
        seed_idx,
        policy,
        max_turns,
        content,
        ruleset,
        DifficultyLevers::NONE,
    )
}

/// [`run_one_with_ruleset`] with a difficulty mode's starting levers applied to the new run
/// before its first turn. [`DifficultyLevers::NONE`] applies nothing, so it is exactly
/// `run_one_with_ruleset`.
pub fn run_one_with_levers(
    seed: i128,
    seed_idx: usize,
    policy: Policy,
    max_turns: usize,
    content: &ContentPack,
    ruleset: Ruleset,
    levers: DifficultyLevers,
) -> RunRecord {
    let mut state = create_new_run_with_content(seed, content);
    if levers != DifficultyLevers::NONE {
        apply_starting_levers(&mut state, levers);
    }
    let mut engine = StepEngine::with_content(state, content);
    engine.set_ruleset(ruleset);

    let mut events_seen: BTreeMap<String, u64> = BTreeMap::new();
    let mut choices_taken: BTreeMap<String, u64> = BTreeMap::new();
    let mut repeats: BTreeMap<String, u64> = BTreeMap::new();
    // Event presented by the most recent travel step (None when that step showed no event).
    let mut last_travel_event: Option<String> = None;
    let mut turns_run: u64 = 0;
    // Inputs of the final mechanics hash, mirroring the Tier 2 driver.
    let mut last_event_id = String::new();
    let mut last_choice_id = String::new();
    let mut last_deltas: BTreeMap<String, i32> = BTreeMap::new();

    for turn in 1..=max_turns {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        let phase_before = engine.phase;
        let pre_event_id = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or_default();
        let (mut intent, mut chosen_choice_id) = choose_intent(policy, &engine, seed_idx, turn);
        if ruleset == Ruleset::V2
            && phase_before == GamePhase::Event
            && intent.action == IntentAction::Choose
        {
            // The V2 engine refuses a choice the party cannot afford; the script picks again.
            let repick = repick_if_unaffordable(policy, &engine, &chosen_choice_id);
            if repick != chosen_choice_id {
                intent = PlayerIntent::new(IntentAction::Choose).with_choice(&repick);
                chosen_choice_id = repick;
            }
        }
        let msgs = engine.step(&intent);
        turns_run += 1;
        let post_event_id = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or_else(|| pre_event_id.clone());

        if phase_before == GamePhase::Event && intent.action == IntentAction::Choose {
            *choices_taken
                .entry(format!("{pre_event_id}:{chosen_choice_id}"))
                .or_insert(0) += 1;
        }
        if phase_before == GamePhase::Camp && intent.action == IntentAction::Travel {
            let presented = if engine.phase == GamePhase::Event {
                engine.pending_event().map(|e| e.event_id.clone())
            } else {
                None
            };
            if let Some(id) = &presented {
                *events_seen.entry(id.clone()).or_insert(0) += 1;
                if last_travel_event.as_ref() == Some(id) {
                    *repeats.entry(id.clone()).or_insert(0) += 1;
                }
            }
            last_travel_event = presented;
        }

        last_event_id = post_event_id;
        last_choice_id = chosen_choice_id;
        last_deltas = msgs.outcome_deltas;
    }

    engine.state.rng_counter = engine.rng.counter;
    engine.state.rng_state = Some(engine.rng.getstate());
    let final_state_hash = compute_mechanics_state_hash(
        &engine.state,
        engine.phase,
        &last_event_id,
        &last_choice_id,
        &last_deltas,
    );

    let state = &engine.state;
    let finished = engine.phase == GamePhase::GameOver || state.game_over;
    let outcome = if state.victory {
        Outcome::Victory
    } else if finished {
        Outcome::Died
    } else {
        Outcome::TurnCap
    };
    let ending_tier = if finished {
        compute_ending(state).tier
    } else {
        "unfinished".to_string()
    };

    RunRecord {
        seed: seed.to_string(),
        outcome,
        turns: turns_run,
        day: i64::from(state.day),
        distance_traveled: i64::from(state.distance_traveled),
        total_distance: i64::from(state.total_distance),
        party_size: state.party.members.len() as u64,
        survivors: state.party.alive_count() as u64,
        ending_tier,
        deaths_by_cause: deaths_by_cause(state)
            .into_iter()
            .map(|(k, v)| (k, i64::from(v)))
            .collect(),
        events_seen,
        choices_taken,
        repeats,
        final_supplies: state
            .supplies
            .to_dict()
            .into_iter()
            .map(|(k, v)| (k, i64::from(v)))
            .collect(),
        final_state_hash,
    }
}

/// Aggregate run records into a [`SimReport`]. Pure: `all_events` and `all_choices` are the
/// universes (event ids; `"<event_id>:<choice_id>"` pairs) used for the "never seen / never
/// taken" lists.
pub fn aggregate(
    policy: &str,
    max_turns: u64,
    records: &[RunRecord],
    all_events: &[String],
    all_choices: &[String],
) -> SimReport {
    let mut wins = 0u64;
    let mut deaths = 0u64;
    let mut turn_caps = 0u64;
    let mut days: Vec<i64> = Vec::with_capacity(records.len());
    let mut ending_tiers: BTreeMap<String, u64> = BTreeMap::new();
    let mut deaths_by_cause: BTreeMap<String, i64> = BTreeMap::new();
    let mut events_seen: BTreeMap<String, u64> = BTreeMap::new();
    let mut repeats_by_event: BTreeMap<String, u64> = BTreeMap::new();
    let mut choices_taken: BTreeMap<String, u64> = BTreeMap::new();

    for r in records {
        match r.outcome {
            Outcome::Victory => wins += 1,
            Outcome::Died => deaths += 1,
            Outcome::TurnCap => turn_caps += 1,
        }
        days.push(r.day);
        *ending_tiers.entry(r.ending_tier.clone()).or_insert(0) += 1;
        for (k, v) in &r.deaths_by_cause {
            *deaths_by_cause.entry(k.clone()).or_insert(0) += v;
        }
        for (k, v) in &r.events_seen {
            *events_seen.entry(k.clone()).or_insert(0) += v;
        }
        for (k, v) in &r.repeats {
            *repeats_by_event.entry(k.clone()).or_insert(0) += v;
        }
        for (k, v) in &r.choices_taken {
            *choices_taken.entry(k.clone()).or_insert(0) += v;
        }
    }
    days.sort_unstable();
    let median_days = if days.is_empty() {
        0
    } else {
        days[(days.len() - 1) / 2]
    };

    let mut by_count: Vec<(String, u64)> =
        events_seen.iter().map(|(k, v)| (k.clone(), *v)).collect();
    by_count.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_events: Vec<(String, u64)> = by_count.iter().take(TOP_EVENTS).cloned().collect();
    by_count.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let bottom_events: Vec<(String, u64)> = by_count.iter().take(TOP_EVENTS).cloned().collect();

    let mut events_never_seen: Vec<String> = all_events
        .iter()
        .filter(|id| !events_seen.contains_key(*id))
        .cloned()
        .collect();
    events_never_seen.sort();
    events_never_seen.dedup();
    let mut choices_never_taken: Vec<String> = all_choices
        .iter()
        .filter(|id| !choices_taken.contains_key(*id))
        .cloned()
        .collect();
    choices_never_taken.sort();
    choices_never_taken.dedup();

    let repeats_total = repeats_by_event.values().sum();

    SimReport {
        policy: policy.to_string(),
        max_turns,
        runs: records.len() as u64,
        wins,
        deaths,
        turn_caps,
        median_days,
        min_days: days.first().copied().unwrap_or(0),
        max_days: days.last().copied().unwrap_or(0),
        ending_tiers,
        deaths_by_cause,
        events_seen,
        top_events,
        bottom_events,
        events_never_seen,
        repeats_by_event,
        repeats_total,
        choices_taken,
        choices_never_taken,
    }
}

/// Every event id in the content pack.
pub fn content_event_ids(content: &ContentPack) -> Vec<String> {
    content
        .all_events
        .iter()
        .map(|e| e.event_id.clone())
        .collect()
}

/// Every `"<event_id>:<choice_id>"` pair in the content pack.
pub fn content_choice_keys(content: &ContentPack) -> Vec<String> {
    content
        .all_events
        .iter()
        .flat_map(|e| {
            e.fallback_choices
                .iter()
                .map(move |c| format!("{}:{}", e.event_id, c.choice_id))
        })
        .collect()
}

/// Simulate `seeds` under `policy`. The run at position `i` of `seeds` is run index `i`.
pub fn simulate(
    seeds: &[i128],
    policy: Policy,
    max_turns: usize,
    content: &ContentPack,
) -> SimReport {
    simulate_with_ruleset(seeds, policy, max_turns, content, Ruleset::Legacy)
}

/// [`simulate`] under an explicit [`Ruleset`]. `Legacy` is exactly `simulate`.
pub fn simulate_with_ruleset(
    seeds: &[i128],
    policy: Policy,
    max_turns: usize,
    content: &ContentPack,
    ruleset: Ruleset,
) -> SimReport {
    simulate_with_levers(
        seeds,
        policy,
        max_turns,
        content,
        ruleset,
        DifficultyLevers::NONE,
    )
}

/// [`simulate_with_ruleset`] with starting levers applied to every run.
pub fn simulate_with_levers(
    seeds: &[i128],
    policy: Policy,
    max_turns: usize,
    content: &ContentPack,
    ruleset: Ruleset,
    levers: DifficultyLevers,
) -> SimReport {
    let records: Vec<RunRecord> = seeds
        .iter()
        .enumerate()
        .map(|(idx, &seed)| {
            run_one_with_levers(seed, idx, policy, max_turns, content, ruleset, levers)
        })
        .collect();
    aggregate(
        policy.name(),
        max_turns as u64,
        &records,
        &content_event_ids(content),
        &content_choice_keys(content),
    )
}

/// Parsed `etv_simulate` options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimOptions {
    pub seeds: u64,
    pub start: i128,
    pub policies: Vec<Policy>,
    pub turns: usize,
    pub json: bool,
    /// Which rules the runs play by (`Legacy` unless `--ruleset v2`).
    pub ruleset: Ruleset,
    /// Starting levers (a difficulty mode's, or set one by one); V2 only.
    pub levers: DifficultyLevers,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimCommand {
    Help,
    Run(SimOptions),
}

/// Usage text for `etv_simulate --help`.
pub const USAGE: &str = "\
etv_simulate: balance simulation harness for Escape the Valley

USAGE:
    etv_simulate [--seeds <n>] [--start <seed>] [--policy <name>|all] [--turns <n>]
                 [--ruleset legacy|v2] [--difficulty <mode>]
                 [--start-food-pct <n>] [--start-water-pct <n>] [--json]

OPTIONS:
    --seeds <n>        number of seeds to run (default 100, at least 1)
    --start <seed>     first seed; seeds are start, start+1, ... (default 0)
    --policy <name>    first | last | cautious | reckless | rotating | all (default all)
    --turns <n>        turn cap per run (default 500, at least 1)
    --ruleset <name>   legacy (default: the frozen rules) | v2 (the rules the game will play)
    --difficulty <m>   hardcore | normal | easy: that mode's starting levers (v2 only)
    --start-food-pct <n>   start with n% of the standard food (v2 only; after --difficulty)
    --start-water-pct <n>  start with n% of the standard water (v2 only; after --difficulty)
    --json             print the reports as a JSON array instead of a table
    --help             print this help
";

fn parse_min_one<T: std::str::FromStr + PartialOrd + From<u8>>(
    flag: &str,
    value: &str,
) -> Result<T, String> {
    value
        .parse::<T>()
        .ok()
        .filter(|n| *n >= T::from(1u8))
        .ok_or_else(|| format!("invalid value for {flag}: '{value}' (need an integer >= 1)"))
}

/// Parse the command-line arguments (without the program name).
pub fn parse_args(args: &[String]) -> Result<SimCommand, String> {
    let mut opts = SimOptions {
        seeds: DEFAULT_SEEDS,
        start: 0,
        policies: Policy::ALL.to_vec(),
        turns: DEFAULT_MAX_TURNS,
        json: false,
        ruleset: Ruleset::Legacy,
        levers: DifficultyLevers::NONE,
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--help" | "-h" => return Ok(SimCommand::Help),
            "--json" => opts.json = true,
            "--difficulty" | "--start-food-pct" | "--start-water-pct" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    return Err(format!("flag '{flag}' needs a value"));
                };
                match flag {
                    "--difficulty" => {
                        opts.levers = Difficulty::parse(value)
                            .ok_or_else(|| {
                                format!("unknown difficulty '{value}' (use hardcore, normal or easy)")
                            })?
                            .levers();
                    }
                    _ => {
                        let pct = value
                            .parse::<i32>()
                            .ok()
                            .filter(|n| (1..=1000).contains(n))
                            .ok_or_else(|| {
                                format!("invalid value for {flag}: '{value}' (need 1 to 1000)")
                            })?;
                        if flag == "--start-food-pct" {
                            opts.levers.start_food_pct = pct;
                        } else {
                            opts.levers.start_water_pct = pct;
                        }
                    }
                }
            }
            "--seeds" | "--start" | "--policy" | "--turns" | "--ruleset" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    return Err(format!("flag '{flag}' needs a value"));
                };
                match flag {
                    "--seeds" => opts.seeds = parse_min_one::<u64>(flag, value)?,
                    "--turns" => {
                        let n = parse_min_one::<u64>(flag, value)?;
                        opts.turns = usize::try_from(n)
                            .map_err(|_| format!("invalid value for --turns: '{value}'"))?;
                    }
                    "--ruleset" => {
                        opts.ruleset = match value.as_str() {
                            "legacy" => Ruleset::Legacy,
                            "v2" => Ruleset::V2,
                            _ => {
                                return Err(format!("unknown ruleset '{value}' (use legacy or v2)"))
                            }
                        };
                    }
                    "--start" => {
                        opts.start = value.parse::<i128>().map_err(|_| {
                            format!("invalid value for --start: '{value}' (need an integer)")
                        })?;
                    }
                    _ => {
                        opts.policies = if value == "all" {
                            Policy::ALL.to_vec()
                        } else {
                            vec![Policy::parse(value).ok_or_else(|| {
                                format!(
                                    "unknown policy '{value}' (use first, last, cautious, reckless, rotating or all)"
                                )
                            })?]
                        };
                    }
                }
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
        i += 1;
    }
    if opts.levers != DifficultyLevers::NONE && opts.ruleset != Ruleset::V2 {
        return Err("difficulty levers need --ruleset v2 (the Legacy rules have no modes)".to_string());
    }
    Ok(SimCommand::Run(opts))
}

/// The seed list for `opts`: `start`, `start + 1`, ... (`seeds` of them).
pub fn seed_list(opts: &SimOptions) -> Vec<i128> {
    (0..opts.seeds)
        .map(|i| opts.start + i128::from(i))
        .collect()
}

fn join_pairs(pairs: &[(String, u64)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Plain-text rendering of one report.
pub fn format_text(r: &SimReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "policy {}: {} runs, turn cap {}\n",
        r.policy, r.runs, r.max_turns
    ));
    out.push_str(&format!(
        "  wins {}/{} ({}), died {}, turn cap {}\n",
        r.wins,
        r.runs,
        percent_string(r.wins, r.runs),
        r.deaths,
        r.turn_caps
    ));
    out.push_str(&format!(
        "  days: median {}, min {}, max {}\n",
        r.median_days, r.min_days, r.max_days
    ));
    let tiers: Vec<(String, u64)> = r
        .ending_tiers
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    out.push_str(&format!("  endings: {}\n", join_pairs(&tiers)));
    let mut causes: Vec<(String, u64)> = r
        .deaths_by_cause
        .iter()
        .map(|(k, v)| (k.clone(), (*v).max(0) as u64))
        .collect();
    causes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.push_str(&format!("  deaths by cause: {}\n", join_pairs(&causes)));
    out.push_str(&format!(
        "  back-to-back repeats: {} ({} distinct events)\n",
        r.repeats_total,
        r.repeats_by_event.len()
    ));
    out.push_str(&format!(
        "  events seen {} distinct, never seen {}; choices never taken {}\n",
        r.events_seen.len(),
        r.events_never_seen.len(),
        r.choices_never_taken.len()
    ));
    out.push_str(&format!("  most frequent: {}\n", join_pairs(&r.top_events)));
    out.push_str(&format!(
        "  least frequent: {}\n",
        join_pairs(&r.bottom_events)
    ));
    out
}
