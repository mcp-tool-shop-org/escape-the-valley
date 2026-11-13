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

use std::collections::BTreeSet;

use crate::content::ContentPack;
use crate::engine::StepEngine;
use crate::measure::{TurnSet, TurnSpec, TURN_SET_SCHEMA};
use crate::model::{GMProfile, GamePhase, IntentAction, PlayerIntent};
use crate::narrator::{
    build_narration_context_with_phase, evaluate_turn_matters, resolve_fallback_line_with_mode,
    FallbackPhraseTable, PreTurnObservation,
};
use crate::worldgen::create_new_run_with_rng;

/// Turns per weirdness level in turn set v1: `(level, turns)`. 240 turns in all.
pub const TURN_SET_V1_QUOTAS: [(u8, usize); 4] = [(0, 40), (1, 40), (2, 120), (3, 40)];

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
            // The composed line, whatever the build profile, so debug and release agree.
            let fallback_line = resolve_fallback_line_with_mode(&ctx, Some(table), false);
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
                effects: ctx.shown_deltas().clone(),
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
