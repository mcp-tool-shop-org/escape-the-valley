//! Invariant sweeps over the Rust engine: save-every-step equivalence, no soft-lock, no silent death.
//!
//! Expected counts and sets below are literals measured from a run, never recomputed by the code
//! under test.
//!
//! Save/resume note: `GamePhase` is not persisted in `run.json` (see `oracle_parity.rs`, check 08:
//! a save taken at a route fork resumes in `Camp`), and the pending event / route options live in
//! private engine fields. So the save-every-step test only reloads when the engine is in
//! `GamePhase::Camp` after a step, which is the one phase the save format supports. Steps that
//! leave the engine in `Event` or `Route` continue on the same engine, and the test asserts how
//! many reload points were taken and how many were skipped for that reason.

use std::collections::{BTreeMap, BTreeSet};

use etv_core::content::{embedded_content_pack, ContentPack};
use etv_core::engine::StepEngine;
use etv_core::model::{GamePhase, IntentAction, PlayerIntent, RunState};
use etv_core::physics::HealthEffect;
use etv_core::save::{load_game_checked, save_game, state_to_value, value_to_state};
use etv_core::snapshot::{build_1000_seeds, compute_mechanics_state_hash};
use etv_core::worldgen::create_new_run_with_content;

const RETRY_LINE: &str = "That option isn't available";

fn new_engine(seed: i128, pack: &ContentPack) -> StepEngine {
    StepEngine::with_content(create_new_run_with_content(seed, pack), pack)
}

/// Mixed deterministic policy used by the save-equivalence and silent-death sweeps. It depends
/// only on `seed_idx`, `turn`, the phase, offered choice ids and visible state, never on text.
/// Returns the intent, the chosen choice id (empty outside events/routes) and an action label.
fn mixed_policy(
    e: &StepEngine,
    seed_idx: usize,
    turn: usize,
) -> (PlayerIntent, String, &'static str) {
    match e.phase {
        GamePhase::Event => {
            let c = e.pending_event_choices();
            if c.is_empty() {
                return (
                    PlayerIntent::new(IntentAction::Choose).with_choice("A"),
                    "A".into(),
                    "event",
                );
            }
            let id = c[(seed_idx.wrapping_mul(31).wrapping_add(turn)) % c.len()]
                .id
                .clone();
            (
                PlayerIntent::new(IntentAction::Choose).with_choice(&id),
                id,
                "event",
            )
        }
        GamePhase::Route => {
            let n = e.pending_routes().len().clamp(1, 4);
            let id = ["A", "B", "C", "D"][(seed_idx + turn) % n].to_string();
            (
                PlayerIntent::new(IntentAction::Choose).with_choice(&id),
                id,
                "route",
            )
        }
        GamePhase::Camp => {
            let s = &e.state;
            let sel = (seed_idx.wrapping_mul(17).wrapping_add(turn.wrapping_mul(5))) % 14;
            let (intent, label) = match sel {
                0 if turn == 1 => (
                    PlayerIntent::new(IntentAction::ChangePace).with_pace(
                        if seed_idx.is_multiple_of(2) {
                            "steady"
                        } else {
                            "hard"
                        },
                    ),
                    "pace",
                ),
                1 | 2 if s.supplies.ammo() > 0 => (PlayerIntent::new(IntentAction::Hunt), "hunt"),
                3 if s.wagon.condition < 75 && s.supplies.parts() > 0 => {
                    (PlayerIntent::new(IntentAction::Repair), "repair")
                }
                4 | 5 if s.party.morale < 70 => (PlayerIntent::new(IntentAction::Rest), "rest"),
                6 | 7 => (PlayerIntent::new(IntentAction::HardRation), "hard_ration"),
                8 | 9 => (
                    PlayerIntent::new(IntentAction::DesperateRepair),
                    "desperate_repair",
                ),
                _ => (PlayerIntent::new(IntentAction::Travel), "travel"),
            };
            (intent, String::new(), label)
        }
        GamePhase::GameOver => (
            PlayerIntent::new(IntentAction::Travel),
            String::new(),
            "travel",
        ),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Reload {
    Never,
    InMemory,
    OnDisk,
}

/// Plays up to `turns` turns and returns (per-turn hashes, reloads taken, reload points skipped
/// because the engine was in Event/Route after the step).
fn play_hashes(
    seed: i128,
    idx: usize,
    turns: usize,
    reload: Reload,
) -> (Vec<String>, usize, usize) {
    let pack = embedded_content_pack();
    let mut e = new_engine(seed, pack);
    let mut hashes = Vec::new();
    let (mut reloads, mut skipped) = (0usize, 0usize);
    let dir = tempfile::tempdir().expect("tempdir");
    for turn in 1..=turns {
        if e.phase == GamePhase::GameOver || e.state.game_over {
            break;
        }
        let pre_event = e
            .pending_event()
            .map(|x| x.event_id.clone())
            .unwrap_or_default();
        let (intent, cid, _) = mixed_policy(&e, idx, turn);
        let msgs = e.step(&intent);
        let post_event = e
            .pending_event()
            .map(|x| x.event_id.clone())
            .unwrap_or(pre_event);
        e.state.rng_counter = e.rng.counter;
        e.state.rng_state = Some(e.rng.getstate());
        let h = compute_mechanics_state_hash(
            &e.state,
            e.phase,
            &post_event,
            &cid,
            &msgs.outcome_deltas,
        );
        hashes.push(h.clone());

        if reload == Reload::Never {
            continue;
        }
        if e.phase != GamePhase::Camp {
            skipped += 1;
            continue;
        }
        let loaded: RunState = match reload {
            Reload::InMemory => value_to_state(&state_to_value(&e.state)).expect("value_to_state"),
            Reload::OnDisk => {
                save_game(&e.state, dir.path()).expect("save_game");
                load_game_checked(dir.path()).expect("load_game_checked")
            }
            Reload::Never => unreachable!(),
        };
        e = StepEngine::with_content(loaded, pack);
        assert_eq!(
            e.phase,
            GamePhase::Camp,
            "seed {seed} turn {turn}: reload must land in Camp"
        );
        // The reloaded state must hash identically to the state that was saved.
        let h2 = compute_mechanics_state_hash(
            &e.state,
            e.phase,
            &post_event,
            &cid,
            &msgs.outcome_deltas,
        );
        assert_eq!(
            h, h2,
            "seed {seed} turn {turn}: state hash changed across save/load"
        );
        reloads += 1;
    }
    (hashes, reloads, skipped)
}

#[test]
fn save_every_step_equals_straight_through_100_seeds() {
    let seeds: Vec<i128> = build_1000_seeds().into_iter().take(100).collect();
    let (mut total_reloads, mut total_skipped, mut total_turns) = (0usize, 0usize, 0usize);
    for (idx, &seed) in seeds.iter().enumerate() {
        let (straight, _, _) = play_hashes(seed, idx, 60, Reload::Never);
        let mode = if idx < 5 {
            Reload::OnDisk
        } else {
            Reload::InMemory
        };
        let (resumed, reloads, skipped) = play_hashes(seed, idx, 60, mode);
        assert_eq!(
            straight, resumed,
            "seed {seed}: save-every-step hash sequence diverged"
        );
        total_reloads += reloads;
        total_skipped += skipped;
        total_turns += straight.len();
    }
    eprintln!("save sweep: turns {total_turns} reloads {total_reloads} skipped {total_skipped}");
    assert_eq!(total_turns, EXPECTED_SAVE_TURNS);
    assert_eq!(total_reloads, EXPECTED_SAVE_RELOADS);
    assert_eq!(total_skipped, EXPECTED_SAVE_SKIPPED);
}

const EXPECTED_SAVE_TURNS: usize = 6000;
const EXPECTED_SAVE_RELOADS: usize = 4457;
const EXPECTED_SAVE_SKIPPED: usize = 1543;

// ---------------------------------------------------------------------------------------------
// 2. No soft-lock
// ---------------------------------------------------------------------------------------------

/// Returns (turns played, event turns checked, route turns checked).
fn soft_lock_run(seed: i128, last: bool) -> (usize, usize, usize) {
    let pack = embedded_content_pack();
    let mut e = new_engine(seed, pack);
    let (mut played, mut events, mut routes) = (0, 0, 0);
    for turn in 1..=120 {
        if e.phase == GamePhase::GameOver || e.state.game_over {
            break;
        }
        played += 1;
        let intent = match e.phase {
            GamePhase::Event => {
                events += 1;
                let offered: Vec<String> = e
                    .pending_event_choices()
                    .iter()
                    .map(|c| c.id.clone())
                    .collect();
                assert!(
                    !offered.is_empty(),
                    "seed {seed} turn {turn}: event offers no choices"
                );
                let pick = if last {
                    offered.last()
                } else {
                    offered.first()
                }
                .unwrap()
                .clone();
                let msgs = e.step(&PlayerIntent::new(IntentAction::Choose).with_choice(&pick));
                assert!(
                    !msgs.lines.iter().any(|l| l.contains(RETRY_LINE)),
                    "seed {seed} turn {turn}: offered choice {pick} of {offered:?} was rejected"
                );
                assert_eq!(
                    msgs.outcome_title, "Outcome",
                    "seed {seed} turn {turn}: choice {pick} produced no outcome"
                );
                assert_ne!(
                    e.phase,
                    GamePhase::Event,
                    "seed {seed} turn {turn}: still in Event"
                );
                continue;
            }
            GamePhase::Route => {
                routes += 1;
                assert!(
                    !e.pending_routes().is_empty(),
                    "seed {seed} turn {turn}: no route options"
                );
                let n = e.pending_routes().len().min(4);
                let id = ["A", "B", "C", "D"][if last { n - 1 } else { 0 }];
                let msgs = e.step(&PlayerIntent::new(IntentAction::Choose).with_choice(id));
                assert!(
                    !msgs.lines.iter().any(|l| l.contains(RETRY_LINE)),
                    "seed {seed} turn {turn}: offered route {id} was rejected"
                );
                assert_ne!(
                    e.phase,
                    GamePhase::Route,
                    "seed {seed} turn {turn}: still in Route"
                );
                continue;
            }
            _ => {
                // Spend aggressively: hunt/repair when possible, else travel.
                let s = &e.state;
                if last && s.supplies.ammo() > 0 && turn % 3 == 0 {
                    PlayerIntent::new(IntentAction::Hunt)
                } else if last && s.wagon.condition < 80 && s.supplies.parts() > 0 {
                    PlayerIntent::new(IntentAction::Repair)
                } else {
                    PlayerIntent::new(IntentAction::Travel)
                }
            }
        };
        e.step(&intent);
    }
    (played, events, routes)
}

#[test]
fn no_soft_lock_5000_seeds_two_policies() {
    let t0 = std::time::Instant::now();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 16))
        .unwrap_or(4);
    let seeds: Vec<i128> = (0..5000).collect();
    let chunk = seeds.len().div_ceil(workers);
    let totals: Vec<(usize, usize, usize)> = std::thread::scope(|s| {
        let hs: Vec<_> = seeds
            .chunks(chunk)
            .map(|c| {
                s.spawn(move || {
                    let mut t = (0, 0, 0);
                    for &seed in c {
                        for last in [true, false] {
                            let r = soft_lock_run(seed, last);
                            t = (t.0 + r.0, t.1 + r.1, t.2 + r.2);
                        }
                    }
                    t
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let played: usize = totals.iter().map(|t| t.0).sum();
    let events: usize = totals.iter().map(|t| t.1).sum();
    let routes: usize = totals.iter().map(|t| t.2).sum();
    eprintln!(
        "soft-lock sweep: {played} turns, {events} event turns, {routes} route turns, {:?}",
        t0.elapsed()
    );
    assert_eq!(played, EXPECTED_SOFTLOCK_TURNS);
    assert_eq!(events, EXPECTED_SOFTLOCK_EVENTS);
    assert_eq!(routes, EXPECTED_SOFTLOCK_ROUTES);
}

const EXPECTED_SOFTLOCK_TURNS: usize = 943135;
const EXPECTED_SOFTLOCK_EVENTS: usize = 298354;
const EXPECTED_SOFTLOCK_ROUTES: usize = 17186;

// ---------------------------------------------------------------------------------------------
// 3. No silent death
// ---------------------------------------------------------------------------------------------

type Silent = (i128, usize, &'static str);
type SilentRun = (usize, Vec<Silent>, BTreeMap<&'static str, usize>, usize);

fn alive_names(e: &StepEngine) -> Vec<String> {
    e.state
        .party
        .members
        .iter()
        .filter(|m| m.is_alive())
        .map(|m| m.name.clone())
        .collect()
}

/// Returns (deaths seen, silent deaths as (seed, turn, action), deaths by action label,
/// valve uses (hard ration + desperate repair that actually fired)).
fn silent_death_run(
    seed: i128,
    idx: usize,
) -> (usize, Vec<Silent>, BTreeMap<&'static str, usize>, usize) {
    let pack = embedded_content_pack();
    let mut e = new_engine(seed, pack);
    let (mut deaths, mut silent) = (0usize, Vec::new());
    let mut by_action: BTreeMap<&'static str, usize> = BTreeMap::new();
    for turn in 1..=120 {
        if e.phase == GamePhase::GameOver || e.state.game_over {
            break;
        }
        let before = alive_names(&e);
        let (intent, _, action) = mixed_policy(&e, idx, turn);
        let msgs = e.step(&intent);
        let after = alive_names(&e);
        for name in before.iter().filter(|n| !after.contains(n)) {
            deaths += 1;
            *by_action.entry(action).or_insert(0) += 1;
            let by_effect = msgs
                .effects
                .iter()
                .any(|f| matches!(f, HealthEffect::Died { member, .. } if member == name));
            let by_line = msgs.lines.iter().any(|l| l.contains(name.as_str()));
            if !(by_effect || by_line) {
                silent.push((seed, turn, action));
            }
        }
    }
    let valves = e
        .diagnostics
        .get("escape_valves_used")
        .copied()
        .unwrap_or(0) as usize;
    (deaths, silent, by_action, valves)
}

#[test]
fn no_silent_death_300_seeds() {
    let seeds: Vec<i128> = build_1000_seeds().into_iter().take(300).collect();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 16))
        .unwrap_or(4);
    let chunk = seeds.len().div_ceil(workers);
    let indexed: Vec<(usize, i128)> = seeds.into_iter().enumerate().collect();
    let results: Vec<SilentRun> = std::thread::scope(|s| {
        let hs: Vec<_> = indexed
            .chunks(chunk)
            .map(|c| {
                s.spawn(move || {
                    let mut d = 0;
                    let mut sil = Vec::new();
                    let mut ba: BTreeMap<&'static str, usize> = BTreeMap::new();
                    let mut v = 0;
                    for &(i, seed) in c {
                        let (a, b, m, valves) = silent_death_run(seed, i);
                        d += a;
                        sil.extend(b);
                        v += valves;
                        for (k, n) in m {
                            *ba.entry(k).or_insert(0) += n;
                        }
                    }
                    (d, sil, ba, v)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let deaths: usize = results.iter().map(|r| r.0).sum();
    let valves: usize = results.iter().map(|r| r.3).sum();
    let mut deaths_by_action: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &results {
        for (k, n) in &r.2 {
            *deaths_by_action.entry(k).or_insert(0) += n;
        }
    }
    let silent: Vec<Silent> = results.into_iter().flat_map(|r| r.1).collect();
    let actions: BTreeSet<&str> = silent.iter().map(|s| s.2).collect();
    eprintln!(
        "silent-death sweep: {deaths} deaths, {} silent, actions {actions:?}, by action {deaths_by_action:?}, valves {valves}",
        silent.len()
    );
    for s in &silent {
        eprintln!("  silent: {s:?}");
    }
    assert_eq!(deaths, EXPECTED_DEATHS);
    // Proves the policy really fired the escape valves (hard ration / desperate repair).
    assert_eq!(valves, EXPECTED_VALVES);
    let expected_by_action: BTreeMap<&str, usize> =
        EXPECTED_DEATHS_BY_ACTION.iter().copied().collect();
    assert_eq!(deaths_by_action, expected_by_action);
    // Issue #42: hard rationing, hunt injuries and failed desperate repairs can kill silently in
    // Python. Rust is a faithful port; if this set is non-empty it pins exactly those actions, and
    // the fix PR must flip the expected set to empty.
    let expected: BTreeSet<&str> = EXPECTED_SILENT_ACTIONS.iter().copied().collect();
    assert_eq!(actions, expected, "silent deaths by action changed");
    assert_eq!(silent.len(), EXPECTED_SILENT_COUNT);
    for ordinary in ["travel", "rest", "event", "route", "repair", "pace"] {
        assert!(
            !actions.contains(ordinary),
            "ordinary action {ordinary} killed silently"
        );
    }
}

const EXPECTED_DEATHS: usize = 421;
const EXPECTED_SILENT_COUNT: usize = 0;
const EXPECTED_SILENT_ACTIONS: [&str; 0] = [];
const EXPECTED_VALVES: usize = 174;
const EXPECTED_DEATHS_BY_ACTION: [(&str, usize); 4] = [
    ("event", 40),
    ("hard_ration", 7),
    ("hunt", 4),
    ("travel", 370),
];
