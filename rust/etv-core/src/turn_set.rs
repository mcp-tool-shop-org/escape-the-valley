//! Generator for the frozen narrator turn set (`etv-turn-set-1`) that the fallback-rate harness
//! in [`crate::measure`] reads.
//!
//! Every turn is drawn by the engine: a run is created from a seed, played with a fixed policy,
//! and each resolved event choice is recorded with the facts the game had on that turn. Nothing
//! here calls a model, and the generator has no randomness of its own, so the same quotas and
//! content always give the same bytes.
//!
//! ## How the turns are chosen
//!
//! - One stratum per engine weirdness level `0..=3`. Level 2 is the level a new run plays at, so
//!   it gets the largest share; the other levels are there so the band rules are measured too.
//! - Within a level, seeds are tried in order from 1. A run plays up to
//!   [`MAX_STEPS_PER_RUN`] steps: at camp it always travels; at a fork it takes the first road;
//!   at an event it takes the offered choice at index `(seed + events resolved so far) mod n`.
//! - A resolved choice is kept only if its `(event_id, choice_id)` pair is new for that level,
//!   and at most [`MAX_TURNS_PER_RUN`] turns are kept from one run, so the set spreads over many
//!   events and many seeds.
//! - The level is full when its quota is reached.
//! - Every run is played under the Legacy ruleset ([`TURN_SET_RULESET`]), pinned in code, so
//!   later changes to the V2 rules cannot move the set.

use std::collections::{BTreeMap, BTreeSet};

use crate::content::ContentPack;
use crate::engine::StepEngine;
use crate::measure::{
    TurnSet, TurnSetV2, TurnSpec, TurnSpecV2, TURN_SET_SCHEMA, TURN_SET_SCHEMA_V2,
};
use crate::model::{GMProfile, GamePhase, IntentAction, PlayerIntent};
use crate::narrator::{
    build_narration_context_with_phase, evaluate_turn_matters, ComposedFallbackComposer,
    FallbackPhraseTable, NarrationContext, PreTurnObservation,
};
use crate::ruleset::Ruleset;
use crate::simulate::{choose_intent, Policy};
use crate::worldgen::create_new_run_with_rng;

/// Turns per weirdness level in turn set v1: `(level, turns)`. 240 turns in all.
pub const TURN_SET_V1_QUOTAS: [(u8, usize); 4] = [(0, 40), (1, 40), (2, 120), (3, 40)];

/// The ruleset every turn of the set is drawn under.
pub const TURN_SET_RULESET: Ruleset = Ruleset::Legacy;

/// Steps played per run before moving to the next seed.
pub const MAX_STEPS_PER_RUN: usize = 150;

/// Most turns kept from one run.
pub const MAX_TURNS_PER_RUN: usize = 4;

/// Seeds tried per level before giving up on a quota.
pub const MAX_SEEDS_PER_LEVEL: i64 = 5_000;

/// Collect up to `quota` turns at `level`, appending them to `out`.
fn collect_level(
    level: u8,
    quota: usize,
    content: &ContentPack,
    table: &FallbackPhraseTable,
    out: &mut Vec<TurnSpec>,
) {
    let mut seen_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut taken = 0usize;
    for seed in 1..=MAX_SEEDS_PER_LEVEL {
        if taken >= quota {
            return;
        }
        let (mut state, _rng) =
            create_new_run_with_rng(i128::from(seed), GMProfile::Fireside, i32::from(level));
        state.content_hash = content.content_hash_hex.clone();
        let mut engine = StepEngine::with_content(state, content);
        // Pinned: the set is drawn under the Legacy rules, whatever rules the game plays later,
        // so a change to the V2 rules can never move this file or its digest.
        engine.set_ruleset(TURN_SET_RULESET);
        let mut resolved_in_run = 0usize;
        let mut taken_in_run = 0usize;

        for _ in 0..MAX_STEPS_PER_RUN {
            if taken >= quota || taken_in_run >= MAX_TURNS_PER_RUN {
                break;
            }
            let phase = engine.phase;
            let intent = match phase {
                GamePhase::GameOver => break,
                GamePhase::Camp => PlayerIntent::new(IntentAction::Travel),
                GamePhase::Route => PlayerIntent::new(IntentAction::Choose).with_choice("A"),
                GamePhase::Event => {
                    let choices = engine.pending_event_choices();
                    if choices.is_empty() {
                        break;
                    }
                    let idx = (seed as usize + resolved_in_run) % choices.len();
                    PlayerIntent::new(IntentAction::Choose).with_choice(&choices[idx].id)
                }
            };
            // Facts that are gone once the step resolves the event.
            let pending = if phase == GamePhase::Event {
                engine.pending_event().map(|event| {
                    let label = engine
                        .pending_event_choices()
                        .iter()
                        .find(|c| c.id == intent.choice_id)
                        .map(|c| c.label.clone())
                        .unwrap_or_default();
                    (
                        event.event_id.clone(),
                        event.fallback_narration.clone(),
                        label,
                    )
                })
            } else {
                None
            };

            let pre = PreTurnObservation::capture(phase, &engine.state);
            let msgs = engine.step(&intent);
            let Some((event_id, event_seed, choice_label)) = pending else {
                continue;
            };
            if msgs.outcome_title.is_empty() {
                continue;
            }
            resolved_in_run += 1;

            let pair = (event_id.clone(), intent.choice_id.clone());
            if seen_pairs.contains(&pair) {
                continue;
            }
            let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
            let ctx = build_narration_context_with_phase(
                &engine.state,
                engine.phase,
                &event_id,
                &intent.choice_id,
                &msgs,
                flags,
            );
            // Version 1 is frozen with the meaning it was drawn under: on a resolved choice its
            // `distance` is the change in miles still to go (positive = a longer road). The
            // narrator's own figure is now progress, the other way round, so it is turned back
            // here and the file stays byte for byte what it was.
            let mut v1_effects = ctx.shown_deltas().clone();
            if let Some(distance) = v1_effects.get_mut("distance") {
                *distance = -*distance;
            }
            // The composed line, whatever the build profile, so debug and release agree.
            let fallback_line =
                match ComposedFallbackComposer::try_start_with_mode(Some(table), false) {
                    Ok(composer) => composer.compose_from_delta_map(&v1_effects),
                    Err(_) => ctx.fallback_line().to_string(),
                };
            if fallback_line.trim().is_empty() {
                continue;
            }
            seen_pairs.insert(pair);
            taken += 1;
            taken_in_run += 1;
            out.push(TurnSpec {
                id: format!("w{level}-{taken:03}"),
                seed,
                event_id,
                choice_id: intent.choice_id.clone(),
                weirdness_level: level,
                uncanny_tokens: engine.state.uncanny_tokens,
                deltas: msgs
                    .outcome_deltas
                    .iter()
                    .filter(|(_, v)| **v != 0)
                    .map(|(k, v)| (k.clone(), *v))
                    .collect(),
                effects: v1_effects,
                event_seed,
                choice_label,
                fallback_line,
            });
        }
    }
}

/// Generate a turn set with `quotas` (`(weirdness level, turns)` pairs) from `content`.
///
/// A level whose quota cannot be met within [`MAX_SEEDS_PER_LEVEL`] seeds returns fewer turns;
/// the caller checks the counts.
pub fn generate_turn_set(quotas: &[(u8, usize)], content: &ContentPack) -> TurnSet {
    let table = FallbackPhraseTable::embedded();
    let mut turns = Vec::new();
    for &(level, quota) in quotas {
        collect_level(level, quota, content, &table, &mut turns);
    }
    TurnSet {
        schema: TURN_SET_SCHEMA.to_string(),
        turns,
    }
}

/// The exact bytes of a turn-set file: pretty JSON, LF line endings, one trailing newline.
pub fn turn_set_to_json(set: &TurnSet) -> String {
    let mut text = serde_json::to_string_pretty(set).expect("a turn set always serialises");
    text.push('\n');
    text
}

// ============================================================================
// Turn set v2: drawn mid-run, with the whole narrator context stored
// ============================================================================

/// Turns per weirdness level in turn set v2 (four levels, 360 turns in all).
pub const TURN_SET_V2_PER_LEVEL: usize = 90;

/// Seeds played per weirdness level and policy to build the candidate pool.
pub const TURN_SET_V2_SEEDS: i64 = 40;

/// Steps played per run when building the candidate pool.
pub const TURN_SET_V2_STEPS_PER_RUN: usize = 400;

/// The simulator policies that play the candidate runs, in order.
pub const TURN_SET_V2_POLICIES: [Policy; 3] = [Policy::Cautious, Policy::Rotating, Policy::First];

/// Most turns one run may contribute to the quota picks.
pub const TURN_SET_V2_QUOTA_PICKS_PER_RUN: usize = 4;

/// The quotas of turn set v2: `(tag, least number of turns carrying it)`, filled in this order
/// (rarest first). A turn may carry several tags and counts toward each.
pub const TURN_SET_V2_QUOTAS: [(&str, usize); 17] = [
    ("victory", 6),
    ("death", 30),
    ("ending", 12),
    ("low_health", 30),
    ("low_morale", 30),
    ("supply_zero", 30),
    ("injury", 30),
    ("night_at_camp", 30),
    ("landmark_arrival", 30),
    ("late_journey", 40),
    ("allowance:strong", 45),
    ("allowance:hint", 45),
    ("biome:alpine", 25),
    ("biome:swamp", 25),
    ("biome:desert", 25),
    ("biome:forest", 25),
    ("biome:plains", 25),
];

/// Tags that mark a turn as one of the rare cases. A turn with none of them is "ordinary".
pub const TURN_SET_V2_RARE_TAGS: [&str; 6] = [
    "death",
    "ending",
    "low_health",
    "low_morale",
    "supply_zero",
    "injury",
];

/// One turn of a candidate run, with everything the selection needs.
#[derive(Debug, Clone)]
pub struct V2Candidate {
    pub level: u8,
    pub policy: Policy,
    pub seed: i64,
    pub turn: u32,
    pub action: String,
    pub tags: Vec<String>,
    pub context: NarrationContext,
}

impl V2Candidate {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// A turn that is none of the rare cases: an everyday step of the journey.
    pub fn is_ordinary(&self) -> bool {
        !TURN_SET_V2_RARE_TAGS.iter().any(|t| self.has(t))
    }
}

fn action_name(intent: &PlayerIntent) -> &'static str {
    match intent.action {
        IntentAction::Travel => "travel",
        IntentAction::Rest => "rest",
        IntentAction::Hunt => "hunt",
        IntentAction::Repair => "repair",
        IntentAction::Choose => "choose",
        IntentAction::ChangePace => "pace",
        IntentAction::AbandonCargo => "abandon_cargo",
        IntentAction::DesperateRepair => "desperate_repair",
        IntentAction::HardRation => "hard_ration",
        _ => "other",
    }
}

/// The tags of one turn, sorted: the five turn-matters flags that are set, `ending` (and `victory`
/// when the run reached the valley), the journey
/// band, `low_health` (party health band critical), `low_morale` (morale band low or broken),
/// the biome, the weirdness level the run was created at, the weirdness allowance the engine
/// gave the turn (`none`, `hint` or `strong`), and what kind of turn it was.
fn tags_for(
    ctx: &NarrationContext,
    level: u8,
    intent: &PlayerIntent,
    resolved_choice: bool,
) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let f = ctx.flags();
    for (on, name) in [
        (f.death, "death"),
        (f.injury, "injury"),
        (f.supply_zero, "supply_zero"),
        (f.landmark_arrival, "landmark_arrival"),
        (f.night_at_camp, "night_at_camp"),
    ] {
        if on {
            tags.push(name.to_string());
        }
    }
    if ctx.phase() == GamePhase::GameOver {
        tags.push("ending".to_string());
        // The engine's own line for a run that reached the valley.
        if ctx
            .shown_lines()
            .iter()
            .any(|l| l == "You made it! Victory!")
        {
            tags.push("victory".to_string());
        }
    }
    tags.push(format!("{}_journey", ctx.band()));
    if ctx.party_status().health_band() == "critical" {
        tags.push("low_health".to_string());
    }
    if matches!(ctx.party_status().morale_band(), "low" | "broken") {
        tags.push("low_morale".to_string());
    }
    tags.push(format!("biome:{}", ctx.biome()));
    tags.push(format!("weirdness:{level}"));
    tags.push(format!("allowance:{}", ctx.weirdness_allowance()));
    tags.push(if resolved_choice {
        "event_choice".to_string()
    } else if ctx.phase() == GamePhase::Event {
        "event_presented".to_string()
    } else if intent.action == IntentAction::Choose {
        "fork_choice".to_string()
    } else {
        format!("camp:{}", action_name(intent))
    });
    tags.sort();
    tags
}

/// Play the candidate runs and return every shown turn with its context and tags, in the fixed
/// order (level, policy, seed, turn). Runs are played under [`TURN_SET_RULESET`], with the
/// simulator's policies, exactly as the game's play loop builds a turn's context.
pub fn collect_v2_candidates(content: &ContentPack) -> Vec<V2Candidate> {
    let mut out = Vec::new();
    for level in 0u8..=3 {
        for policy in TURN_SET_V2_POLICIES {
            for seed in 1..=TURN_SET_V2_SEEDS {
                let (mut state, _rng) = create_new_run_with_rng(
                    i128::from(seed),
                    GMProfile::Fireside,
                    i32::from(level),
                );
                state.content_hash = content.content_hash_hex.clone();
                let mut engine = StepEngine::with_content(state, content);
                engine.set_ruleset(TURN_SET_RULESET);
                let idx = (seed - 1) as usize;
                for turn in 1..=TURN_SET_V2_STEPS_PER_RUN {
                    if engine.phase == GamePhase::GameOver || engine.state.game_over {
                        break;
                    }
                    let pre = PreTurnObservation::capture(engine.phase, &engine.state);
                    let pre_event_id = engine
                        .pending_event()
                        .map(|e| e.event_id.clone())
                        .unwrap_or_default();
                    let (intent, _) = choose_intent(policy, &engine, idx, turn);
                    let msgs = engine.step(&intent);
                    let event_id = engine
                        .pending_event()
                        .map(|e| e.event_id.clone())
                        .unwrap_or(pre_event_id);
                    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
                    let context = build_narration_context_with_phase(
                        &engine.state,
                        engine.phase,
                        &event_id,
                        &intent.choice_id,
                        &msgs,
                        flags,
                    );
                    if context.fallback_line().trim().is_empty() {
                        continue;
                    }
                    let resolved_choice = !msgs.outcome_title.is_empty();
                    let tags = tags_for(&context, level, &intent, resolved_choice);
                    out.push(V2Candidate {
                        level,
                        policy,
                        seed,
                        turn: turn as u32,
                        action: action_name(&intent).to_string(),
                        tags,
                        context,
                    });
                }
            }
        }
    }
    out
}

/// Choose the turns of v2 from `candidates` (indices into it, ascending).
///
/// 1. Quotas, in the order of [`TURN_SET_V2_QUOTAS`]: candidates are tried seed by seed (so every
///    level and policy is visited before the next seed), a run gives at most
///    [`TURN_SET_V2_QUOTA_PICKS_PER_RUN`] quota picks, and a level takes at most
///    [`TURN_SET_V2_PER_LEVEL`] turns in all. A quota the pool cannot fill is left short.
/// 2. Each level is then filled up to [`TURN_SET_V2_PER_LEVEL`] with ordinary turns, taken at an
///    even stride through that level's ordinary candidates, so they keep the pool's own mix of
///    travel, rest, hunt, event and arrival turns.
pub fn select_v2(candidates: &[V2Candidate]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|&i| {
        let c = &candidates[i];
        (c.seed, c.level, c.policy, c.turn)
    });
    let mut chosen: BTreeSet<usize> = BTreeSet::new();
    let mut per_level = [0usize; 4];
    let mut per_run: BTreeMap<(u8, Policy, i64), usize> = BTreeMap::new();

    for (tag, quota) in TURN_SET_V2_QUOTAS {
        let mut have = chosen.iter().filter(|&&i| candidates[i].has(tag)).count();
        for &i in &order {
            if have >= quota {
                break;
            }
            let c = &candidates[i];
            if chosen.contains(&i) || !c.has(tag) {
                continue;
            }
            let run = (c.level, c.policy, c.seed);
            let used = per_run.get(&run).copied().unwrap_or(0);
            if used >= TURN_SET_V2_QUOTA_PICKS_PER_RUN
                || per_level[c.level as usize] >= TURN_SET_V2_PER_LEVEL
            {
                continue;
            }
            chosen.insert(i);
            per_run.insert(run, used + 1);
            per_level[c.level as usize] += 1;
            have += 1;
        }
    }

    for level in 0u8..=3 {
        let need = TURN_SET_V2_PER_LEVEL.saturating_sub(per_level[level as usize]);
        if need == 0 {
            continue;
        }
        let pool: Vec<usize> = (0..candidates.len())
            .filter(|&i| {
                candidates[i].level == level && candidates[i].is_ordinary() && !chosen.contains(&i)
            })
            .collect();
        if pool.is_empty() {
            continue;
        }
        let stride = (pool.len() / need).max(1);
        let mut taken = 0usize;
        let mut k = stride / 2;
        while taken < need && k < pool.len() {
            chosen.insert(pool[k]);
            taken += 1;
            k += stride;
        }
    }
    chosen.into_iter().collect()
}

/// Generate turn set v2 from `content`.
pub fn generate_turn_set_v2(content: &ContentPack) -> TurnSetV2 {
    let candidates = collect_v2_candidates(content);
    let picks = select_v2(&candidates);
    let mut counters = [0usize; 4];
    let turns = picks
        .into_iter()
        .map(|i| {
            let c = &candidates[i];
            counters[c.level as usize] += 1;
            TurnSpecV2 {
                id: format!("v2-w{}-{:03}", c.level, counters[c.level as usize]),
                seed: c.seed,
                weirdness_level: c.level,
                policy: c.policy.name().to_string(),
                turn: c.turn,
                action: c.action.clone(),
                strata: c.tags.clone(),
                context: c.context.clone(),
            }
        })
        .collect();
    TurnSetV2 {
        schema: TURN_SET_SCHEMA_V2.to_string(),
        turns,
    }
}

/// The exact bytes of a v2 turn-set file: one turn per line inside the `turns` array, LF line
/// endings, one trailing newline.
pub fn turn_set_v2_to_json(set: &TurnSetV2) -> String {
    let mut text = String::new();
    text.push_str(&format!(
        "{{\"schema\":{},\"turns\":[\n",
        serde_json::to_string(&set.schema).expect("schema serialises")
    ));
    for (i, turn) in set.turns.iter().enumerate() {
        text.push_str(&serde_json::to_string(turn).expect("a turn always serialises"));
        if i + 1 < set.turns.len() {
            text.push(',');
        }
        text.push('\n');
    }
    text.push_str("]}\n");
    text
}
