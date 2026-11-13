//! Tests for the balance simulation harness (`etv_core::simulate`).
//!
//! 1. determinism: same seeds and policy give byte-identical JSON
//! 2. non-interference: `rotating` reproduces the Tier 2 driver's mechanics hashes turn by turn
//!    and its final state hash; every policy's final hash matches a by-hand run
//! 3. a hand-worked aggregate from literal run records
//! 4. argument parsing errors
//! 5. a 200-seed smoke run per policy asserting structural facts only

use std::collections::BTreeMap;

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::GamePhase;
use etv_core::simulate::{
    aggregate, choose_intent, content_event_ids, parse_args, percent_string, run_one, seed_list,
    simulate, Outcome, Policy, RunRecord, SimCommand, DEFAULT_MAX_TURNS, DEFAULT_SEEDS,
};
use etv_core::snapshot::{
    build_1000_seeds, compute_mechanics_state_hash, run_tier2_seed_trace, Tier2SeedTrace,
};
use etv_core::worldgen::create_new_run_with_content;

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn determinism_same_seeds_same_json() {
    let content = embedded_content_pack();
    let seeds: Vec<i128> = (0..25).collect();
    for policy in Policy::ALL {
        let a = serde_json::to_string(&simulate(&seeds, policy, 300, content)).unwrap();
        let b = serde_json::to_string(&simulate(&seeds, policy, 300, content)).unwrap();
        assert_eq!(a, b, "policy {} not deterministic", policy.name());
    }
}

/// Re-run `policy` by hand on a fresh engine, using only public engine API, and return the
/// per-turn mechanics hashes in the same way the Tier 2 driver computes them.
fn by_hand_hashes(policy: Policy, seed: i128, seed_idx: usize, max_turns: usize) -> Vec<String> {
    let content = embedded_content_pack();
    let state = create_new_run_with_content(seed, content);
    let mut engine = StepEngine::with_content(state, content);
    let mut out = Vec::new();
    for turn in 1..=max_turns {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        let pre = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or_default();
        let (intent, cid) = choose_intent(policy, &engine, seed_idx, turn);
        let msgs = engine.step(&intent);
        let post = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or(pre);
        engine.state.rng_counter = engine.rng.counter;
        engine.state.rng_state = Some(engine.rng.getstate());
        let h = compute_mechanics_state_hash(
            &engine.state,
            engine.phase,
            &post,
            &cid,
            &msgs.outcome_deltas,
        );
        out.push(h);
    }
    out
}

#[test]
fn rotating_matches_tier2_driver_hashes() {
    let content = embedded_content_pack();
    let seeds = build_1000_seeds();
    for (idx, &seed) in seeds.iter().enumerate().take(50) {
        let trace: Tier2SeedTrace = run_tier2_seed_trace(seed, idx, content, 250);
        let mine = by_hand_hashes(Policy::Rotating, seed, idx, 250);
        assert_eq!(
            trace.turns.len(),
            mine.len(),
            "turn count, seed index {idx}"
        );
        for (t, h) in trace.turns.iter().zip(mine.iter()) {
            assert_eq!(
                &t.mechanics_state_hash, h,
                "seed index {idx}, turn {}",
                t.turn
            );
        }
        let rec = run_one(seed, idx, Policy::Rotating, 250, content);
        if let Some(last) = trace.turns.last() {
            assert_eq!(rec.final_state_hash, last.mechanics_state_hash);
        }
    }
}

#[test]
fn run_one_final_hash_matches_by_hand_run_for_every_policy() {
    let content = embedded_content_pack();
    for policy in Policy::ALL {
        for idx in 0..20usize {
            let seed = idx as i128;
            let rec = run_one(seed, idx, policy, 300, content);
            let hand = by_hand_hashes(policy, seed, idx, 300);
            let last = hand.last().expect("at least one turn").clone();
            assert_eq!(
                rec.final_state_hash,
                last,
                "policy {} seed {seed}",
                policy.name()
            );
            assert_eq!(rec.turns as usize, hand.len());
        }
    }
}

fn rec(
    seed: &str,
    outcome: Outcome,
    day: i64,
    deaths: &[(&str, i64)],
    events: &[(&str, u64)],
    repeats: &[(&str, u64)],
    choices: &[(&str, u64)],
) -> RunRecord {
    RunRecord {
        seed: seed.to_string(),
        outcome,
        turns: 0,
        day,
        distance_traveled: 0,
        total_distance: 0,
        party_size: 4,
        survivors: 0,
        ending_tier: match outcome {
            Outcome::Victory => "triumphant".to_string(),
            Outcome::Died => "lost".to_string(),
            Outcome::TurnCap => "unfinished".to_string(),
        },
        deaths_by_cause: deaths.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        events_seen: events.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        choices_taken: choices.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        repeats: repeats.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        final_supplies: BTreeMap::new(),
        final_state_hash: String::new(),
    }
}

#[test]
fn aggregate_hand_worked_case() {
    let records = vec![
        rec(
            "1",
            Outcome::Victory,
            50,
            &[("Disease", 1)],
            &[("a", 2), ("b", 1)],
            &[("a", 1)],
            &[("a:A", 2), ("b:B", 1)],
        ),
        rec(
            "2",
            Outcome::Died,
            10,
            &[("Starvation", 3), ("Disease", 1)],
            &[("a", 1), ("c", 1)],
            &[],
            &[("a:A", 1), ("c:A", 1)],
        ),
        rec(
            "3",
            Outcome::TurnCap,
            30,
            &[],
            &[("b", 4)],
            &[("b", 2)],
            &[("b:B", 4)],
        ),
    ];
    let r = aggregate(
        "test",
        100,
        &records,
        &strs(&["a", "b", "c", "d"]),
        &strs(&["a:A", "a:B", "b:B", "c:A"]),
    );
    assert_eq!((r.runs, r.wins, r.deaths, r.turn_caps), (3, 1, 1, 1));
    assert_eq!(percent_string(r.wins, r.runs), "33.3%");
    // Days sorted: 10, 30, 50 -> lower median 30.
    assert_eq!((r.median_days, r.min_days, r.max_days), (30, 10, 50));
    let causes: BTreeMap<String, i64> = [("Disease", 2), ("Starvation", 3)]
        .iter()
        .map(|(k, v)| (k.to_string(), *v))
        .collect();
    assert_eq!(r.deaths_by_cause, causes);
    // a: 2+1 = 3, b: 1+4 = 5, c: 1.
    assert_eq!(
        r.top_events,
        vec![
            ("b".to_string(), 5),
            ("a".to_string(), 3),
            ("c".to_string(), 1)
        ]
    );
    assert_eq!(
        r.bottom_events,
        vec![
            ("c".to_string(), 1),
            ("a".to_string(), 3),
            ("b".to_string(), 5)
        ]
    );
    assert_eq!(r.events_never_seen, strs(&["d"]));
    assert_eq!(r.repeats_total, 3);
    assert_eq!(r.repeats_by_event.get("a"), Some(&1));
    assert_eq!(r.repeats_by_event.get("b"), Some(&2));
    assert_eq!(r.choices_never_taken, strs(&["a:B"]));
    assert_eq!(r.ending_tiers.get("triumphant"), Some(&1));
    assert_eq!(r.ending_tiers.get("lost"), Some(&1));
    assert_eq!(r.ending_tiers.get("unfinished"), Some(&1));
}

#[test]
fn aggregate_of_nothing_is_zeroes() {
    let r = aggregate("none", 10, &[], &strs(&["a"]), &[]);
    assert_eq!(
        (r.runs, r.wins, r.median_days, r.min_days, r.max_days),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(percent_string(0, 0), "n/a");
    assert_eq!(r.events_never_seen, strs(&["a"]));
}

#[test]
fn percent_string_uses_integer_rounding() {
    assert_eq!(percent_string(1, 3), "33.3%");
    assert_eq!(percent_string(2, 3), "66.7%");
    assert_eq!(percent_string(1, 1), "100.0%");
    assert_eq!(percent_string(0, 7), "0.0%");
    assert_eq!(percent_string(1, 8), "12.5%");
}

#[test]
fn argument_parsing() {
    let p = |v: &[&str]| parse_args(&strs(v));
    match p(&[]).unwrap() {
        SimCommand::Run(o) => {
            assert_eq!(o.seeds, DEFAULT_SEEDS);
            assert_eq!(o.start, 0);
            assert_eq!(o.turns, DEFAULT_MAX_TURNS);
            assert_eq!(o.policies, Policy::ALL.to_vec());
            assert!(!o.json);
        }
        SimCommand::Help => panic!("no args must not mean help"),
    }
    match p(&[
        "--seeds", "7", "--start", "-5", "--policy", "cautious", "--turns", "9", "--json",
    ])
    .unwrap()
    {
        SimCommand::Run(o) => {
            assert_eq!((o.seeds, o.start, o.turns), (7, -5, 9));
            assert_eq!(o.policies, vec![Policy::Cautious]);
            assert!(o.json);
            assert_eq!(seed_list(&o), vec![-5, -4, -3, -2, -1, 0, 1]);
        }
        SimCommand::Help => panic!(),
    }
    assert_eq!(p(&["--help"]).unwrap(), SimCommand::Help);
    assert_eq!(
        p(&["--policy", "all"]).unwrap(),
        p(&[]).unwrap(),
        "--policy all is the default"
    );

    assert_eq!(p(&["--bogus"]).unwrap_err(), "unknown argument '--bogus'");
    assert_eq!(p(&["--seeds"]).unwrap_err(), "flag '--seeds' needs a value");
    assert_eq!(
        p(&["--seeds", "0"]).unwrap_err(),
        "invalid value for --seeds: '0' (need an integer >= 1)"
    );
    assert_eq!(
        p(&["--seeds", "abc"]).unwrap_err(),
        "invalid value for --seeds: 'abc' (need an integer >= 1)"
    );
    assert_eq!(
        p(&["--turns", "0"]).unwrap_err(),
        "invalid value for --turns: '0' (need an integer >= 1)"
    );
    assert_eq!(
        p(&["--start", "x"]).unwrap_err(),
        "invalid value for --start: 'x' (need an integer)"
    );
    assert_eq!(
        p(&["--policy", "nope"]).unwrap_err(),
        "unknown policy 'nope' (use first, last, cautious, reckless, rotating or all)"
    );
    assert_eq!(
        p(&["--policy"]).unwrap_err(),
        "flag '--policy' needs a value"
    );
}

#[test]
fn smoke_200_seeds_per_policy_structural_facts() {
    let content = embedded_content_pack();
    let seeds: Vec<i128> = (0..200).collect();
    let ids = content_event_ids(content);
    for policy in Policy::ALL {
        let r = simulate(&seeds, policy, 400, content);
        assert_eq!(r.runs, 200, "{}", policy.name());
        assert_eq!(r.wins + r.deaths + r.turn_caps, r.runs, "{}", policy.name());
        assert!(r.min_days <= r.median_days && r.median_days <= r.max_days);
        for id in r.events_seen.keys() {
            assert!(ids.contains(id), "unknown event id {id}");
        }
        for (k, _) in r.top_events.iter().chain(r.bottom_events.iter()) {
            assert!(ids.contains(k));
        }
        for id in &r.events_never_seen {
            assert!(ids.contains(id));
            assert!(!r.events_seen.contains_key(id));
        }
        assert_eq!(
            r.events_seen.len() + r.events_never_seen.len(),
            ids.len(),
            "seen plus never seen covers the pack"
        );
        let tier_total: u64 = r.ending_tiers.values().sum();
        assert_eq!(tier_total, r.runs);
        assert_eq!(r.repeats_total, r.repeats_by_event.values().sum::<u64>());
    }
}
