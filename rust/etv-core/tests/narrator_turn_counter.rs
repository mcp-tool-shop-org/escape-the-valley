//! The narrator's turn index is the engine's step counter (`RunState::turns_stepped`), not the
//! journal's length. The journal does not grow on every step (an event choice can leave it as it
//! was), so two turns could share an index under the old rule. The counter advances on every
//! engine step, choice and fork turns included, and v5 saves carry it.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::{GMProfile, GamePhase, IntentAction, PlayerIntent};
use etv_core::narrator::{
    build_narration_context_with_phase, evaluate_turn_matters, narration_call_seed,
    NarratorLength, PreTurnObservation,
};
use etv_core::ruleset::Ruleset;
use etv_core::save_v5::{state_to_value_v5, value_to_saved_game};
use etv_core::simulate::{choose_intent, Policy};
use etv_core::worldgen::create_new_run_with_rng;

/// What one run showed about the counter.
#[derive(Default)]
struct Tally {
    steps: u64,
    choice_steps: u64,
    fork_steps: u64,
    journal_unchanged_steps: u64,
    steps_after_the_end: u64,
}

/// Play one run step by step and check the counter after every step.
fn play(seed: i64, level: u8, policy: Policy, ruleset: Ruleset, steps: usize) -> Tally {
    let content = embedded_content_pack();
    let (mut state, _rng) =
        create_new_run_with_rng(i128::from(seed), GMProfile::Fireside, i32::from(level));
    state.content_hash = content.content_hash_hex.clone();
    assert_eq!(state.turns_stepped, 0, "a new run has stepped no turns");
    let mut engine = StepEngine::with_content(state, content);
    engine.set_ruleset(ruleset);
    let mut tally = Tally::default();
    let mut seeds = std::collections::BTreeSet::new();
    for turn in 1..=steps {
        let before = engine.state.turns_stepped;
        let journal_before = engine.state.journal.len();
        let was_over = engine.phase == GamePhase::GameOver;
        let phase_before = engine.phase;
        let pre = PreTurnObservation::capture(engine.phase, &engine.state);
        let (intent, _) = if was_over {
            (PlayerIntent::new(IntentAction::Rest), String::new())
        } else {
            choose_intent(policy, &engine, seed as usize, turn)
        };
        let msgs = engine.step(&intent);

        // Exactly one, whatever the step did.
        assert_eq!(
            engine.state.turns_stepped,
            before + 1,
            "seed {seed} level {level} {policy:?} {ruleset:?} turn {turn}: {phase_before:?} {:?}",
            intent.action
        );
        assert_eq!(engine.state.turns_stepped, turn as u64);

        // The narration context carries it as the turn index.
        let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
        let ctx = build_narration_context_with_phase(
            &engine.state,
            engine.phase,
            "",
            &intent.choice_id,
            &msgs,
            flags,
        );
        assert_eq!(ctx.turn_index(), engine.state.turns_stepped);
        seeds.insert(ctx.turn_index());

        tally.steps += 1;
        if was_over {
            tally.steps_after_the_end += 1;
        } else if phase_before == GamePhase::Event {
            tally.choice_steps += 1;
        } else if phase_before == GamePhase::Route {
            tally.fork_steps += 1;
        }
        if engine.state.journal.len() == journal_before {
            tally.journal_unchanged_steps += 1;
        }
    }
    // No two steps of one run share an index.
    assert_eq!(seeds.len() as u64, tally.steps);
    tally
}

#[test]
fn every_engine_step_advances_the_turn_counter_by_one() {
    let mut all = Tally::default();
    for ruleset in [Ruleset::Legacy, Ruleset::V2] {
        for policy in [Policy::First, Policy::Cautious, Policy::Rotating] {
            for seed in 1..=6 {
                for level in [0u8, 2, 3] {
                    // Long enough that most runs end, so the "run is over" step is covered too.
                    let t = play(seed, level, policy, ruleset, 260);
                    all.steps += t.steps;
                    all.choice_steps += t.choice_steps;
                    all.fork_steps += t.fork_steps;
                    all.journal_unchanged_steps += t.journal_unchanged_steps;
                    all.steps_after_the_end += t.steps_after_the_end;
                }
            }
        }
    }
    // The kinds of step the old index (the journal's length) could miss are all exercised.
    assert!(all.choice_steps > 0, "no event-choice step was played");
    assert!(all.fork_steps > 0, "no fork step was played");
    assert!(
        all.journal_unchanged_steps > 0,
        "no step left the journal unchanged, so the old index was never wrong here"
    );
    assert!(all.steps_after_the_end > 0, "no step after the run ended");
}

#[test]
fn two_steps_that_leave_the_journal_unchanged_get_different_seeds() {
    // Find two steps in one run that both leave the journal as it was: under the old rule they
    // would have carried the same index whenever the journal length matched.
    let content = embedded_content_pack();
    let (mut state, _rng) = create_new_run_with_rng(3, GMProfile::Fireside, 2);
    state.content_hash = content.content_hash_hex.clone();
    let mut engine = StepEngine::with_content(state, content);
    engine.set_ruleset(Ruleset::V2);
    let mut unchanged = Vec::new();
    for turn in 1..=200 {
        if engine.phase == GamePhase::GameOver {
            break;
        }
        let journal_before = engine.state.journal.len();
        let pre = PreTurnObservation::capture(engine.phase, &engine.state);
        let (intent, _) = choose_intent(Policy::First, &engine, 3, turn);
        let msgs = engine.step(&intent);
        if engine.state.journal.len() == journal_before {
            let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
            let ctx = build_narration_context_with_phase(
                &engine.state,
                engine.phase,
                "",
                &intent.choice_id,
                &msgs,
                flags,
            );
            unchanged.push((journal_before, ctx));
        }
    }
    assert!(unchanged.len() >= 2, "need two journal-unchanged steps");
    let (_, a) = &unchanged[0];
    let (_, b) = &unchanged[1];
    assert_ne!(a.turn_index(), b.turn_index());
    // Give them every other fact in common: only the index tells them apart, and it does.
    let same_facts_as_a_at_b = a.clone().with_turn_index(b.turn_index());
    for scene in [true, false] {
        let seed = |c| narration_call_seed(c, GMProfile::Fireside, NarratorLength::Brief, scene);
        assert_ne!(seed(a), seed(&same_facts_as_a_at_b));
        // A replay of the same step asks for the same seed.
        assert_eq!(seed(a), seed(&a.clone()));
    }
}

#[test]
fn a_v5_save_carries_the_counter_and_an_older_one_reads_zero() {
    let content = embedded_content_pack();
    let (mut state, _rng) = create_new_run_with_rng(9, GMProfile::Fireside, 2);
    state.content_hash = content.content_hash_hex.clone();
    let mut engine = StepEngine::with_content(state, content);
    engine.set_ruleset(Ruleset::V2);
    for turn in 1..=12 {
        let (intent, _) = choose_intent(Policy::Cautious, &engine, 9, turn);
        engine.step(&intent);
    }
    assert_eq!(engine.state.turns_stepped, 12);

    let value = state_to_value_v5(&engine.state, None);
    assert_eq!(value["turns_stepped"], serde_json::json!(12));
    let loaded = value_to_saved_game(&value, content).unwrap();
    assert_eq!(loaded.state.turns_stepped, 12);

    // A resumed run keeps counting from where the save left it.
    let mut resumed = StepEngine::with_content(loaded.state, content);
    resumed.set_ruleset(Ruleset::V2);
    resumed.step(&PlayerIntent::new(IntentAction::Rest));
    assert_eq!(resumed.state.turns_stepped, 13);

    // A v5 save written before the counter existed reads it as 0.
    let mut older = value.clone();
    older.as_object_mut().unwrap().remove("turns_stepped");
    assert_eq!(
        value_to_saved_game(&older, content)
            .unwrap()
            .state
            .turns_stepped,
        0
    );

    // A value that is not a count is a corrupt save, not a silent 0.
    for bad in [serde_json::json!(-1), serde_json::json!("12"), serde_json::json!(1.5)] {
        let mut v = value.clone();
        v["turns_stepped"] = bad.clone();
        assert!(
            value_to_saved_game(&v, content).is_err(),
            "{bad} was accepted"
        );
    }
}
