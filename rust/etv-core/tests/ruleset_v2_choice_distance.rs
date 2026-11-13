//! What a choice's `distance` does, under each rule set.
//!
//! The content gives many choices a positive distance ("Push through the gale", "Cross the
//! chained span"). The Legacy rules add it to the miles still to go, as the Python game does, so
//! pushing on makes the road longer. Under V2 it is progress: subtracted from the miles still to
//! go and added to the miles travelled. Legacy is unchanged by this.
//!
//! Whatever the rule set, the narrator's `distance` figure means progress: miles covered are
//! positive ("travelled"), ground lost is negative ("lost ground").
//!
//! Every expected value here is a literal worked out by hand or measured once and written down.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::events::{apply_outcome, apply_outcome_with_ruleset};
use etv_core::model::{GamePhase, IntentAction, PlayerIntent, RunState};
use etv_core::narrator::{
    build_narration_context_with_phase, change_word, evaluate_turn_matters, PreTurnObservation,
};
use etv_core::ruleset::Ruleset;
use etv_core::worldgen::{create_new_run, create_new_run_with_content, EventOutcome};

fn state(remaining: i32, travelled: i32) -> RunState {
    let mut st = create_new_run(42);
    st.distance_remaining = remaining;
    st.distance_traveled = travelled;
    st
}

fn outcome(distance: i32) -> EventOutcome {
    EventOutcome {
        distance_delta: distance,
        ..EventOutcome::default()
    }
}

fn applied(remaining: i32, travelled: i32, distance: i32, ruleset: Ruleset) -> (i32, i32) {
    let mut st = state(remaining, travelled);
    apply_outcome_with_ruleset(&mut st, &outcome(distance), ruleset);
    (st.distance_remaining, st.distance_traveled)
}

#[test]
fn legacy_adds_a_choices_distance_to_the_miles_still_to_go() {
    // 20 miles to go, 30 travelled. A choice with distance +5 leaves 25 to go.
    assert_eq!(applied(20, 30, 5, Ruleset::Legacy), (25, 30));
    // A negative distance shortens the road under Legacy.
    assert_eq!(applied(20, 30, -5, Ruleset::Legacy), (15, 30));
    assert_eq!(applied(20, 30, 0, Ruleset::Legacy), (20, 30));
    // The two-argument function is the Legacy rule, as before.
    let mut st = state(20, 30);
    apply_outcome(&mut st, &outcome(5));
    assert_eq!((st.distance_remaining, st.distance_traveled), (25, 30));
}

#[test]
fn v2_counts_a_choices_distance_as_progress() {
    // 20 miles to go, 30 travelled. Distance +5: 15 to go, 35 travelled.
    assert_eq!(applied(20, 30, 5, Ruleset::V2), (15, 35));
    // Distance -5 is ground lost: 25 to go, and the miles travelled stay as they were.
    assert_eq!(applied(20, 30, -5, Ruleset::V2), (25, 30));
    assert_eq!(applied(20, 30, 0, Ruleset::V2), (20, 30));
    // Progress stops at the destination: 3 miles to go and distance +5 covers the 3.
    assert_eq!(applied(3, 30, 5, Ruleset::V2), (0, 33));
    assert_eq!(applied(0, 30, 5, Ruleset::V2), (0, 30));
}

/// Seed 42, first event: Persistent Rain. Choice A, "Continue despite the wet", has distance 4.
#[test]
fn the_same_choice_lengthens_the_road_under_legacy_and_covers_ground_under_v2() {
    let content = embedded_content_pack();
    for (ruleset, remaining_change, shown, word) in [
        (Ruleset::Legacy, 4, -4, "lost ground"),
        (Ruleset::V2, -4, 4, "travelled"),
    ] {
        let mut engine =
            StepEngine::with_content(create_new_run_with_content(42, content), content);
        engine.set_ruleset(ruleset);
        engine.step(&PlayerIntent::new(IntentAction::Travel));
        assert_eq!(engine.phase, GamePhase::Event);
        assert_eq!(engine.pending_event().unwrap().title, "Persistent Rain");
        let before = engine.state.distance_remaining;
        let travelled_before = engine.state.distance_traveled;

        let pre = PreTurnObservation::capture(engine.phase, &engine.state);
        let event_id = engine.pending_event().unwrap().event_id.clone();
        let intent = PlayerIntent::new(IntentAction::Choose).with_choice("A");
        let msgs = engine.step(&intent);
        assert_eq!(
            engine.state.distance_remaining - before,
            remaining_change,
            "{ruleset:?}"
        );
        // Only V2 counts the miles as travelled.
        assert_eq!(
            engine.state.distance_traveled - travelled_before,
            if ruleset == Ruleset::V2 { 4 } else { 0 }
        );

        // The narrator is told progress under both.
        let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
        let ctx = build_narration_context_with_phase(
            &engine.state,
            engine.phase,
            &event_id,
            "A",
            &msgs,
            flags,
        );
        assert_eq!(ctx.shown_deltas()["distance"], shown, "{ruleset:?}");
        assert_eq!(change_word("distance", shown), word);
    }
}

/// A camp travel step is told the same under both rule sets: miles covered.
#[test]
fn a_travel_step_reads_travelled_under_both_rule_sets() {
    let content = embedded_content_pack();
    for ruleset in [Ruleset::Legacy, Ruleset::V2] {
        let mut engine =
            StepEngine::with_content(create_new_run_with_content(42, content), content);
        engine.set_ruleset(ruleset);
        let pre = PreTurnObservation::capture(engine.phase, &engine.state);
        let intent = PlayerIntent::new(IntentAction::Travel);
        let before = engine.state.distance_remaining;
        let msgs = engine.step(&intent);
        let covered = before - engine.state.distance_remaining;
        assert!(covered > 0);
        let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
        let ctx =
            build_narration_context_with_phase(&engine.state, engine.phase, "", "", &msgs, flags);
        assert_eq!(ctx.shown_deltas()["distance"], covered);
        assert_eq!(change_word("distance", covered), "travelled");
    }
}

/// How much of the content this touches, counted from the embedded pack.
#[test]
fn the_content_gives_191_of_627_choices_a_positive_distance_and_none_a_negative_one() {
    let pack = embedded_content_pack();
    let mut positive = 0;
    let mut negative = 0;
    let mut choices = 0;
    for event in &pack.all_events {
        for template in event.outcome_templates.values() {
            choices += 1;
            if template.distance_delta > 0 {
                positive += 1;
            } else if template.distance_delta < 0 {
                negative += 1;
            }
        }
    }
    assert_eq!((choices, positive, negative), EXPECTED_CONTENT_COUNTS);
}

// Measured from the embedded content pack when this was written.
const EXPECTED_CONTENT_COUNTS: (usize, usize, usize) = (627, 191, 0);
