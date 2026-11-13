//! A reloaded engine with an event on screen must carry the same recent-event cooldown list as
//! one that was never saved.
//!
//! The list is not in the save: it is rebuilt from the journal, and the journal holds an event
//! only once it is resolved. So a reload with an event pending used to leave that event out of
//! the list until it resolved. That never changed play while every pick resolved on the next
//! turn. Once the V2 rules can refuse a pick, a turn can end with the same event still pending,
//! and the two engines then disagree. `StepEngine::resume` now adds the pending event.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::GamePhase;
use etv_core::ruleset::Ruleset;
use etv_core::save_v5::{state_to_value_v5, value_to_saved_game};
use etv_core::snapshot::{build_1000_seeds, tier2_mechanics_turn};
use etv_core::worldgen::create_new_run_with_content;

const SEED_INDEX: usize = 1;
/// On this seed the Tier 2 policy asks, on turn 39, for a choice of event `f4_075` that the
/// party cannot afford, with an affordable one beside it. The engine refuses it.
const REFUSED_TURN: usize = 39;
const PENDING_EVENT: &str = "f4_075";

fn new_engine() -> StepEngine {
    let pack = embedded_content_pack();
    let seed = build_1000_seeds()[SEED_INDEX];
    let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
    engine.set_ruleset(Ruleset::V2);
    engine
}

fn reload(engine: &StepEngine) -> StepEngine {
    let pack = embedded_content_pack();
    let value = state_to_value_v5(&engine.state, engine.pending_block().as_ref());
    let saved = value_to_saved_game(&value, pack).expect("load");
    StepEngine::resume(saved.state, saved.pending, pack, Ruleset::V2, false, None).expect("resume")
}

fn recent_ids(engine: &StepEngine) -> Vec<String> {
    engine
        .state
        .recent_event_ids
        .0
        .clone()
        .expect("the cooldown list is set once an event has been selected or restored")
}

#[test]
fn a_reload_with_an_event_pending_keeps_it_in_the_cooldown_list() {
    let mut plain = new_engine();
    for turn in 1..REFUSED_TURN {
        tier2_mechanics_turn(&mut plain, SEED_INDEX, turn);
    }
    // The event is on screen at the end of turn 38, in both engines.
    assert_eq!(plain.phase, GamePhase::Event);
    assert_eq!(
        plain.pending_event().map(|e| e.event_id.as_str()),
        Some(PENDING_EVENT)
    );
    let reloaded = reload(&plain);
    assert_eq!(reloaded.phase, GamePhase::Event);

    let ids = recent_ids(&plain);
    assert_eq!(ids.last().map(String::as_str), Some(PENDING_EVENT));
    assert!(ids.len() <= 4);
    assert_eq!(recent_ids(&reloaded), ids);
}

#[test]
fn after_a_refused_pick_the_reloaded_run_matches_the_uninterrupted_one() {
    let mut plain = new_engine();
    let mut saved_every_step = new_engine();
    for turn in 1..=60 {
        if plain.phase == GamePhase::GameOver || plain.state.game_over {
            break;
        }
        let phase_before = plain.phase;
        let journal_before = plain.state.journal.len();
        let counter_before = plain.rng.counter;
        let a = tier2_mechanics_turn(&mut plain, SEED_INDEX, turn);
        let b = tier2_mechanics_turn(&mut saved_every_step, SEED_INDEX, turn);
        assert_eq!(a, b, "turn {turn}: the reloaded run diverged");
        assert_eq!(
            plain.rng.counter, saved_every_step.rng.counter,
            "turn {turn}"
        );
        if turn == REFUSED_TURN {
            // The pick was refused: same event still up, nothing resolved, no draw made.
            assert_eq!(phase_before, GamePhase::Event);
            assert_eq!(plain.phase, GamePhase::Event);
            assert_eq!(
                plain.pending_event().map(|e| e.event_id.as_str()),
                Some(PENDING_EVENT)
            );
            assert_eq!(plain.state.journal.len(), journal_before);
            assert_eq!(plain.rng.counter, counter_before);
        }
        saved_every_step = reload(&saved_every_step);
    }
}
