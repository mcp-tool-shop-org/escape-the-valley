//! The `cautious-aware` simulator policy: the cautious script, except that it never picks a
//! choice the party cannot afford when an affordable one is offered. `cautious` itself is left
//! exactly as it was, as the fixed reference.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::GamePhase;
use etv_core::ruleset::Ruleset;
use etv_core::simulate::{
    choose_intent, parse_args, run_one_with_ruleset, simulate_with_ruleset, Policy, SimCommand,
};
use etv_core::worldgen::create_new_run_with_content;

#[test]
fn the_policy_has_a_name_and_is_in_the_all_list_after_cautious() {
    assert_eq!(Policy::CautiousAware.name(), "cautious-aware");
    assert_eq!(Policy::parse("cautious-aware"), Some(Policy::CautiousAware));
    let names: Vec<&str> = Policy::ALL.iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        vec![
            "first",
            "last",
            "cautious",
            "cautious-aware",
            "reckless",
            "rotating"
        ]
    );
    assert_eq!(
        serde_json::to_string(&Policy::CautiousAware).unwrap(),
        "\"cautious-aware\""
    );
    let args: Vec<String> = ["--policy", "cautious-aware"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let SimCommand::Run(opts) = parse_args(&args).unwrap() else {
        panic!("expected a run");
    };
    assert_eq!(opts.policies, vec![Policy::CautiousAware]);
    let bad: Vec<String> = ["--policy", "careful"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parse_args(&bad).unwrap_err(),
        "unknown policy 'careful' (use first, last, cautious, cautious-aware, reckless, rotating or all)"
    );
}

/// Under the Legacy rules every choice is shown affordable, so the two scripts are one script.
#[test]
fn under_legacy_it_plays_exactly_as_cautious() {
    let content = embedded_content_pack();
    for (idx, seed) in (0..200i128).enumerate() {
        let a = run_one_with_ruleset(seed, idx, Policy::Cautious, 500, content, Ruleset::Legacy);
        let b = run_one_with_ruleset(
            seed,
            idx,
            Policy::CautiousAware,
            500,
            content,
            Ruleset::Legacy,
        );
        assert_eq!(a.final_state_hash, b.final_state_hash, "seed {seed}");
    }
}

/// Count, over a V2 run, the event picks made and the picks of a choice shown unaffordable while
/// an affordable one was on offer.
fn picks(policy: Policy, seed: i128, idx: usize) -> (usize, usize) {
    let content = embedded_content_pack();
    let mut engine = StepEngine::with_content(create_new_run_with_content(seed, content), content);
    engine.set_ruleset(Ruleset::V2);
    let (mut made, mut avoidable) = (0usize, 0usize);
    for turn in 1..=500 {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        let (intent, choice_id) = choose_intent(policy, &engine, idx, turn);
        if engine.phase == GamePhase::Event {
            let choices = engine.pending_event_choices();
            let picked = choices.iter().find(|c| c.id == choice_id).unwrap();
            made += 1;
            if !picked.affordable && choices.iter().any(|c| c.affordable) {
                avoidable += 1;
            }
        }
        engine.step(&intent);
    }
    (made, avoidable)
}

#[test]
fn under_v2_it_never_picks_an_unaffordable_choice_when_an_affordable_one_is_offered() {
    let (mut aware_made, mut aware_avoidable) = (0usize, 0usize);
    let (mut plain_made, mut plain_avoidable) = (0usize, 0usize);
    for (idx, seed) in (0..200i128).enumerate() {
        let (m, a) = picks(Policy::CautiousAware, seed, idx);
        aware_made += m;
        aware_avoidable += a;
        let (m, a) = picks(Policy::Cautious, seed, idx);
        plain_made += m;
        plain_avoidable += a;
    }
    assert!(aware_made > 0);
    assert_eq!(aware_avoidable, 0);
    // The plain cautious script does make such picks, which is the difference being measured.
    assert!(plain_avoidable > 0);
    assert_eq!(
        (aware_made, plain_made, plain_avoidable),
        (
            EXPECTED_AWARE_PICKS,
            EXPECTED_PLAIN_PICKS,
            EXPECTED_PLAIN_AVOIDABLE
        )
    );
}

/// Win counts over seeds 0 to 199 under V2, both scripts, as measured when this was written.
#[test]
fn v2_win_counts_for_both_cautious_scripts_are_pinned() {
    let content = embedded_content_pack();
    let seeds: Vec<i128> = (0..200).collect();
    let plain = simulate_with_ruleset(&seeds, Policy::Cautious, 500, content, Ruleset::V2);
    let aware = simulate_with_ruleset(&seeds, Policy::CautiousAware, 500, content, Ruleset::V2);
    assert_eq!(aware.policy, "cautious-aware");
    assert_eq!(
        (plain.wins, aware.wins),
        (EXPECTED_PLAIN_V2_WINS, EXPECTED_AWARE_V2_WINS)
    );
}

// Measured (seeds 0 to 199, turn cap 500, V2 rules). They move whenever a V2 rule changes.
const EXPECTED_AWARE_PICKS: usize = 10086;
const EXPECTED_PLAIN_PICKS: usize = 10053;
const EXPECTED_PLAIN_AVOIDABLE: usize = 1806;
const EXPECTED_PLAIN_V2_WINS: u64 = 10;
const EXPECTED_AWARE_V2_WINS: u64 = 11;
