//! `etv_simulate --ruleset`: the simulator can play the V2 rules, and Legacy is unchanged.

use etv_core::content::embedded_content_pack;
use etv_core::ruleset::Ruleset;
use etv_core::simulate::{
    parse_args, run_one, run_one_with_ruleset, simulate, simulate_with_ruleset, Policy, SimCommand,
};

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ruleset_flag_parses_and_defaults_to_legacy() {
    let SimCommand::Run(opts) = parse_args(&[]).unwrap() else {
        panic!("expected a run");
    };
    assert_eq!(opts.ruleset, Ruleset::Legacy);
    let SimCommand::Run(opts) = parse_args(&args(&["--ruleset", "v2"])).unwrap() else {
        panic!("expected a run");
    };
    assert_eq!(opts.ruleset, Ruleset::V2);
    let SimCommand::Run(opts) = parse_args(&args(&["--ruleset", "legacy"])).unwrap() else {
        panic!("expected a run");
    };
    assert_eq!(opts.ruleset, Ruleset::Legacy);
    assert_eq!(
        parse_args(&args(&["--ruleset", "v3"])).unwrap_err(),
        "unknown ruleset 'v3' (use legacy or v2)"
    );
    assert_eq!(
        parse_args(&args(&["--ruleset"])).unwrap_err(),
        "flag '--ruleset' needs a value"
    );
}

#[test]
fn legacy_ruleset_is_exactly_the_plain_simulator() {
    let content = embedded_content_pack();
    let seeds: Vec<i128> = (0..40).collect();
    for policy in Policy::ALL {
        let plain = serde_json::to_string(&simulate(&seeds, policy, 300, content)).unwrap();
        let legacy = serde_json::to_string(&simulate_with_ruleset(
            &seeds,
            policy,
            300,
            content,
            Ruleset::Legacy,
        ))
        .unwrap();
        assert_eq!(plain, legacy, "{}", policy.name());
    }
    let a = run_one(7, 0, Policy::Cautious, 300, content);
    let b = run_one_with_ruleset(7, 0, Policy::Cautious, 300, content, Ruleset::Legacy);
    assert_eq!(a.final_state_hash, b.final_state_hash);
}

/// The V2 rules change real runs: over seeds 0 to 199 the cautious script ends some runs in a
/// different state, and the two win counts are the measured ones.
#[test]
fn v2_ruleset_changes_runs_and_its_win_counts_are_pinned() {
    let content = embedded_content_pack();
    let seeds: Vec<i128> = (0..200).collect();
    let legacy = simulate(&seeds, Policy::Cautious, 500, content);
    let v2 = simulate_with_ruleset(&seeds, Policy::Cautious, 500, content, Ruleset::V2);
    assert_eq!(legacy.runs, 200);
    assert_eq!(v2.runs, 200);
    let differing = seeds
        .iter()
        .enumerate()
        .filter(|(idx, &seed)| {
            run_one(seed, *idx, Policy::Cautious, 500, content).final_state_hash
                != run_one_with_ruleset(seed, *idx, Policy::Cautious, 500, content, Ruleset::V2)
                    .final_state_hash
        })
        .count();
    assert!(differing > 0);
    assert_eq!(
        (legacy.wins, v2.wins, differing),
        (EXPECTED_LEGACY_WINS, EXPECTED_V2_WINS, EXPECTED_DIFFERING)
    );
}

// Measured when this test was written (seeds 0 to 199, cautious policy, turn cap 500). They move
// whenever a V2 rule changes, on purpose.
const EXPECTED_LEGACY_WINS: u64 = 20;
const EXPECTED_V2_WINS: u64 = 69;
const EXPECTED_DIFFERING: usize = 200;
