//! Ruleset switch and the first V2 rule: the river event (`river_crossing`) is resolved at
//! most once per run. Legacy stays byte-for-byte as it was.
//!
//! No second implementation checks V2, so every expected value here is a literal worked out
//! by hand or measured once and written down.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::events::{
    build_event_library, missing_supplies_for_choice, select_event, select_event_with_ruleset,
    V2_ONCE_PER_RUN_RIVER_EVENT_ID,
};
use etv_core::model::{Biome, GamePhase, JournalEntry, RunState};
use etv_core::physics::current_node;
use etv_core::rng::CpythonRng;
use etv_core::ruleset::Ruleset;
use etv_core::save::{state_to_value, value_to_state};
use etv_core::snapshot::{
    build_1000_seeds, run_tier2_mechanics_with_ruleset, run_tier2_seed_trace, tier2_mechanics_turn,
};
use etv_core::worldgen::{create_new_run, create_new_run_with_content, EventSkeleton};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const RIVER: &str = V2_ONCE_PER_RUN_RIVER_EVENT_ID;

fn river_resolved_count(state: &RunState) -> usize {
    state
        .journal
        .iter()
        .filter(|j| !j.choice_made.is_empty() && j.event_id == RIVER)
        .count()
}

/// Per-run counts of how often the river event was resolved, over `n` seeds x `turns`.
fn river_counts(ruleset: Ruleset, n: usize, turns: usize) -> Vec<usize> {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    seeds
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, &s)| {
            let run = run_tier2_mechanics_with_ruleset(s, i, pack, turns, ruleset);
            river_resolved_count(&run.state)
        })
        .collect()
}

// 1. Legacy untouched ---------------------------------------------------------------------

/// SHA-256 over the concatenated per-turn mechanics hashes of the first 200 seeds x 60 turns,
/// measured on the base commit before the ruleset change (so it pins today's behaviour).
const LEGACY_200X60_DIGEST: &str =
    "12ecb6ef49ac962988daa41083146d696da59f0b76a27bc497b234f6832a4af3";

#[test]
fn legacy_default_and_explicit_legacy_match_the_tier2_trace() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut digest = Sha256::new();
    for (idx, &seed) in seeds.iter().take(200).enumerate() {
        let trace: Vec<String> = run_tier2_seed_trace(seed, idx, pack, 60)
            .turns
            .into_iter()
            .map(|t| t.mechanics_state_hash)
            .collect();

        // Engine left at the default.
        let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        assert_eq!(engine.ruleset, Ruleset::Legacy, "default must be Legacy");
        let mut default_hashes = Vec::new();
        for turn in 1..=60 {
            if engine.phase == GamePhase::GameOver || engine.state.game_over {
                break;
            }
            default_hashes.push(tier2_mechanics_turn(&mut engine, idx, turn));
        }

        let explicit = run_tier2_mechanics_with_ruleset(seed, idx, pack, 60, Ruleset::Legacy);
        assert_eq!(default_hashes, trace, "default vs trace, seed {seed}");
        assert_eq!(
            explicit.hashes, trace,
            "explicit Legacy vs trace, seed {seed}"
        );
        for h in &trace {
            digest.update(h.as_bytes());
        }
    }
    assert_eq!(format!("{:x}", digest.finalize()), LEGACY_200X60_DIGEST);
}

// 2. The V2 rule --------------------------------------------------------------------------

#[test]
fn v2_resolves_the_river_event_at_most_once_per_run() {
    let v2 = river_counts(Ruleset::V2, 1000, 120);
    assert_eq!(*v2.iter().max().unwrap(), 1, "V2 max river events per run");
    let once = v2.iter().filter(|&&c| c == 1).count();
    // 400 before the affordability gate existed; 404 with it (the gate changes outcomes, so
    // later draws and so which events turn up).
    assert_eq!(once, 404, "runs with the river event exactly once under V2");

    let legacy = river_counts(Ruleset::Legacy, 1000, 120);
    let twice_plus = legacy.iter().filter(|&&c| c >= 2).count();
    assert_eq!(twice_plus, 81, "Legacy runs with 2+ river events");
    assert_eq!(
        *legacy.iter().max().unwrap(),
        4,
        "Legacy max river events per run"
    );
}

// 3. V2 equals Legacy up to the first river event ------------------------------------------

/// Under V2 the river event is excluded from the candidate list after it is resolved, and
/// that changes the weight total of every later draw. So V2 can differ from Legacy from the
/// first selection after the first river event, not only from the second river selection.
/// The affordability gate (`ruleset_v2_affordability.rs`) also changes V2 outcomes: from the
/// first choice taken while unaffordable. The invariant that must hold: every turn up to and
/// including the turn the first river event is resolved, and before the first unaffordable
/// pick or pick that would spend `tools` (V2 never spends durable supplies), is identical.
#[test]
fn v2_matches_legacy_through_the_first_river_event_then_may_diverge() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut diverged_after = 0usize;
    for (idx, &seed) in seeds.iter().take(50).enumerate() {
        let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        let mut first_river_turn: Option<usize> = None;
        let mut first_unaffordable_pick: Option<usize> = None;
        let mut turns_run = 0usize;
        for turn in 1..=120 {
            if engine.phase == GamePhase::GameOver || engine.state.game_over {
                break;
            }
            if first_unaffordable_pick.is_none() && engine.phase == GamePhase::Event {
                // The Tier 2 policy's pick, replayed: `(seed_idx * 31 + turn) % choices`.
                let ev = engine.pending_event().expect("pending event").clone();
                let n = engine.pending_event_choices().len();
                let id = engine.pending_event_choices()
                    [idx.wrapping_mul(31).wrapping_add(turn) % n]
                    .id
                    .clone();
                let spends_tools = ev.outcome_templates[&id]
                    .supplies_delta
                    .get("tools")
                    .is_some_and(|v| *v < 0);
                if spends_tools
                    || !missing_supplies_for_choice(&engine.state, &ev, &id, Ruleset::V2).is_empty()
                {
                    first_unaffordable_pick = Some(turn);
                }
            }
            let _ = tier2_mechanics_turn(&mut engine, idx, turn);
            turns_run = turn;
            if first_river_turn.is_none() && river_resolved_count(&engine.state) >= 1 {
                first_river_turn = Some(turn);
            }
        }
        let legacy = run_tier2_mechanics_with_ruleset(seed, idx, pack, 120, Ruleset::Legacy);
        let v2 = run_tier2_mechanics_with_ruleset(seed, idx, pack, 120, Ruleset::V2);
        let cut = first_river_turn
            .unwrap_or(turns_run)
            .min(first_unaffordable_pick.map_or(usize::MAX, |t| t - 1));
        assert_eq!(
            legacy.hashes[..cut],
            v2.hashes[..cut],
            "seed {seed}: turns up to and including the first river resolution must match"
        );
        if first_river_turn.is_some() && legacy.hashes != v2.hashes {
            diverged_after += 1;
        }
    }
    assert_eq!(
        diverged_after,
        21, // 17 before the affordability gate existed
        "seeds (of 50) that diverge after the first river event"
    );
}

// 4. Pool exhaustion ----------------------------------------------------------------------

fn lib_event(id: &str) -> EventSkeleton {
    let mut e = build_event_library()
        .into_iter()
        .find(|e| e.event_id == id)
        .unwrap_or_else(|| panic!("event {id} missing"));
    e.biome_filter = None;
    e.weather_filter = None;
    e.time_filter = None;
    e.costs_uncanny_token = false;
    e
}

fn state_with_river_resolved() -> RunState {
    let mut state = create_new_run(42);
    state.journal.push(JournalEntry {
        day: 1,
        location: "start".to_string(),
        event_id: RIVER.to_string(),
        scene_title: String::new(),
        narration: String::new(),
        choice_made: "A: Ford the river.".to_string(),
        outcome: String::new(),
        deltas: BTreeMap::new(),
        tags: vec![],
    });
    state
}

fn pick(state: &mut RunState, lib: &[EventSkeleton], rs: Ruleset) -> (Option<String>, u64) {
    let mut rng = CpythonRng::from_seed_and_counter(7, 0, None);
    let got = select_event_with_ruleset(state, &mut rng, lib, None, rs).map(|e| e.event_id);
    (got, rng.counter)
}

#[test]
fn v2_pool_exhaustion_cases() {
    let river = lib_event(RIVER);
    let other = lib_event("flash_flood");

    // River plus one other, river already resolved: V2 can only pick the other.
    let lib = vec![river.clone(), other.clone()];
    let mut st = state_with_river_resolved();
    assert_eq!(
        pick(&mut st, &lib, Ruleset::V2).0.as_deref(),
        Some("flash_flood")
    );

    // River alone, already resolved: V2 selects nothing and makes no RNG draw.
    let lib = vec![river.clone()];
    let mut st = state_with_river_resolved();
    assert_eq!(pick(&mut st, &lib, Ruleset::V2), (None, 0));

    // Same pool under Legacy still returns the river event (one weighted draw).
    let mut st = state_with_river_resolved();
    assert_eq!(
        pick(&mut st, &lib, Ruleset::Legacy),
        (Some(RIVER.to_string()), 1)
    );

    // River alone, not yet resolved: V2 still allows it.
    let mut st = create_new_run(42);
    assert_eq!(pick(&mut st, &lib, Ruleset::V2).0.as_deref(), Some(RIVER));

    // The other event is filtered out by biome, so no candidate passes; V2's fallback must not
    // hand the river event back (a legacy fallback could).
    let mut filtered = other.clone();
    let here = current_node(&create_new_run(42)).unwrap().biome;
    let elsewhere = Biome::ALL.into_iter().find(|b| *b != here).unwrap();
    filtered.biome_filter = Some(vec![elsewhere]);
    let lib = vec![river.clone(), filtered.clone()];
    let mut st = state_with_river_resolved();
    assert_eq!(
        pick(&mut st, &lib, Ruleset::V2).0.as_deref(),
        Some("flash_flood")
    );

    // The public Legacy entry point keeps its signature and behaviour.
    let mut st = create_new_run(42);
    let mut rng = CpythonRng::from_seed_and_counter(7, 0, None);
    let got = select_event(&mut st, &mut rng, &[river], None);
    assert_eq!(got.event_id, RIVER);
}

// 5. Save and resume under V2 -------------------------------------------------------------

#[test]
fn v2_resume_after_the_first_river_event_needs_no_new_saved_field() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut checked = 0usize;
    for (idx, &seed) in seeds.iter().enumerate() {
        if checked == 20 {
            break;
        }
        // Uninterrupted V2 run, recording the first camp turn after the first river event.
        let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        engine.set_ruleset(Ruleset::V2);
        let mut hashes = Vec::new();
        let mut save_after: Option<usize> = None;
        for turn in 1..=120 {
            if engine.phase == GamePhase::GameOver || engine.state.game_over {
                break;
            }
            hashes.push(tier2_mechanics_turn(&mut engine, idx, turn));
            if save_after.is_none()
                && river_resolved_count(&engine.state) == 1
                && engine.phase == GamePhase::Camp
            {
                save_after = Some(turn);
            }
        }
        let Some(cut) = save_after else { continue };
        if cut >= hashes.len() {
            continue;
        }

        // Replay to the cut, save in memory, rebuild, set V2, continue.
        let mut first = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        first.set_ruleset(Ruleset::V2);
        for turn in 1..=cut {
            let _ = tier2_mechanics_turn(&mut first, idx, turn);
        }
        let saved = state_to_value(&first.state);
        let restored = value_to_state(&saved).expect("restore");
        let mut resumed = StepEngine::with_content(restored, pack);
        resumed.set_ruleset(Ruleset::V2);
        let mut tail = Vec::new();
        for turn in (cut + 1)..=hashes.len() {
            if resumed.phase == GamePhase::GameOver || resumed.state.game_over {
                break;
            }
            tail.push(tier2_mechanics_turn(&mut resumed, idx, turn));
        }
        assert_eq!(
            tail,
            hashes[cut..],
            "seed {seed}: resumed run must equal uninterrupted run"
        );
        assert_eq!(
            river_resolved_count(&resumed.state),
            1,
            "seed {seed}: river recurred"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 20,
        "found 20 seeds with a river event followed by a camp turn"
    );
}
