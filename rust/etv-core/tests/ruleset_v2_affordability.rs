//! V2 affordability gate: a choice whose outcome template costs a supply the party lacks is
//! shown as unaffordable before the player picks, and cannot be picked while an affordable
//! choice is offered: the engine refuses it and nothing changes. Legacy stays byte-for-byte as
//! it was (there a shortfall still resolves through the `insufficient_supplies` path).
//!
//! No second implementation checks V2, so every expected value here is a literal worked out by
//! hand or measured once and written down.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::events::{
    apply_outcome, build_event_library, missing_supplies_for_choice,
    raw_missing_supplies_for_choice, resolve_event, resolve_event_with_ruleset,
};
use etv_core::model::{GamePhase, IntentAction, PlayerIntent, RunState};
use etv_core::rng::CpythonRng;
use etv_core::ruleset::Ruleset;
use etv_core::save::{state_to_value, value_to_state};
use etv_core::snapshot::{
    build_1000_seeds, run_tier2_mechanics_with_ruleset, run_tier2_seed_trace, tier2_mechanics_turn,
};
use etv_core::worldgen::{
    create_new_run, create_new_run_with_content, ChoiceTemplate, EventCategory, EventOutcome,
    EventSkeleton,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn choice(id: &str) -> ChoiceTemplate {
    ChoiceTemplate {
        choice_id: id.to_string(),
        label: format!("Choice {id}"),
        action: "IMPROVISE".to_string(),
        style: "careful".to_string(),
        risk_hint: String::new(),
        cost_hint: String::new(),
        outcome_weights: BTreeMap::new(),
        outcome_sentence: None,
    }
}

fn outcome(supplies: &[(&str, i32)]) -> EventOutcome {
    let mut o = EventOutcome::default();
    for (k, v) in supplies {
        o.supplies_delta.insert((*k).to_string(), *v);
        o.supplies_delta_order.push((*k).to_string());
    }
    o
}

fn event(id: &str, choices: &[&str], templates: Vec<(&str, EventOutcome)>) -> EventSkeleton {
    EventSkeleton {
        event_id: id.to_string(),
        title: format!("Title {id}"),
        category: EventCategory::Survival,
        tags: vec![],
        severity: "low".to_string(),
        family: String::new(),
        biome_filter: None,
        weather_filter: None,
        time_filter: None,
        preconditions: vec![],
        base_weight: 1.0,
        folklore_type: None,
        costs_uncanny_token: false,
        oregon_nod: false,
        fallback_narration: "Something happens.".to_string(),
        fallback_choices: choices.iter().map(|c| choice(c)).collect(),
        outcome_templates: templates
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        gm_aside: String::new(),
    }
}

/// Choice A costs rope 3 (and a wagon hit, so its denial is visible), B costs food 2, C is free.
fn three_choice_event() -> EventSkeleton {
    let mut a = outcome(&[("rope", -3)]);
    a.wagon_delta = -4;
    a.morale_delta = 3;
    a.distance_delta = 10;
    a.special_flags = vec!["flag_a".to_string()];
    let b = outcome(&[("food", -2)]);
    let c = outcome(&[]);
    event(
        "afford_three",
        &["A", "B", "C"],
        vec![("A", a), ("B", b), ("C", c)],
    )
}

fn drive_to_event(engine: &mut StepEngine) {
    for _ in 0..400 {
        if engine.phase == GamePhase::Event {
            return;
        }
        assert_ne!(
            engine.phase,
            GamePhase::GameOver,
            "game ended before an event"
        );
        engine.step(&PlayerIntent::new(IntentAction::Travel));
    }
    panic!("no event after 400 travel steps");
}

fn engine_with(ev: &EventSkeleton, ruleset: Ruleset, supplies: &[(&str, i32)]) -> StepEngine {
    let mut state = create_new_run(42);
    for (k, v) in supplies {
        state.supplies.set(k, *v);
    }
    let mut engine = StepEngine::new(state);
    engine.event_library = vec![ev.clone()];
    engine.set_ruleset(ruleset);
    engine
}

fn shape(engine: &StepEngine) -> Vec<(bool, Vec<String>)> {
    engine
        .pending_event_choices()
        .iter()
        .map(|c| (c.affordable, c.missing.clone()))
        .collect()
}

fn keys(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// a. Rope / food / free ----------------------------------------------------------------------

#[test]
fn v2_denies_a_template_cost_the_party_cannot_pay_and_says_so_up_front() {
    let ev = three_choice_event();

    // Resolution, called directly. Party: rope 0, food 50.
    let mut st = create_new_run(42);
    st.supplies.set("rope", 0);
    st.supplies.set("food", 50);
    let mut rng = CpythonRng::from_seed(42);
    let counter = rng.counter;

    let denied = resolve_event_with_ruleset(&mut st, &ev, "A", &mut rng, Ruleset::V2);
    // The denied path (`insufficient_supplies_outcome`): no supply charged, wagon hit kept,
    // morale min(3, -2) = -2, time max(0, 1) = 1, distance kept, flag appended, no RNG draw.
    assert!(denied.supplies_delta.is_empty());
    assert_eq!(denied.health_delta, 0);
    assert_eq!(denied.wagon_delta, -4);
    assert_eq!(denied.animals_health_delta, 0);
    assert_eq!(denied.morale_delta, -2);
    assert_eq!(denied.time_cost, 1);
    assert_eq!(denied.distance_delta, 10);
    assert_eq!(
        denied.special_flags,
        keys(&["flag_a", "insufficient_supplies"])
    );
    assert_eq!(rng.counter, counter);
    let before = st.clone();
    apply_outcome(&mut st, &denied);
    assert_eq!(st.supplies.get("rope"), 0);
    assert_eq!(st.supplies.get("food"), 50);
    assert_eq!(st.distance_remaining, before.distance_remaining + 10);

    // B costs food 2 and the party has 50: it goes through, food 50 -> 48.
    let mut st = create_new_run(42);
    st.supplies.set("food", 50);
    let mut rng = CpythonRng::from_seed(42);
    let paid = resolve_event_with_ruleset(&mut st, &ev, "B", &mut rng, Ruleset::V2);
    assert!(!paid
        .special_flags
        .iter()
        .any(|f| f == "insufficient_supplies"));
    apply_outcome(&mut st, &paid);
    assert_eq!(st.supplies.get("food"), 48);

    // Legacy, same event and state: A goes through. Template untouched, rope clamps at 0.
    let mut st = create_new_run(42);
    st.supplies.set("rope", 0);
    let mut rng = CpythonRng::from_seed(42);
    let free_ride = resolve_event(&mut st, &ev, "A", &mut rng);
    assert_eq!(free_ride.supplies_delta.get("rope"), Some(&-3));
    assert_eq!(free_ride.wagon_delta, -4);
    assert_eq!(free_ride.morale_delta, 3);
    assert_eq!(free_ride.time_cost, 0);
    assert_eq!(free_ride.special_flags, keys(&["flag_a"]));
    apply_outcome(&mut st, &free_ride);
    assert_eq!(st.supplies.get("rope"), 0);
}

#[test]
fn v2_choice_list_carries_affordable_and_missing_and_refreshes_on_re_present() {
    let ev = three_choice_event();

    // Presented with whatever the new run holds, then supplies set and the event re-presented
    // (a non-choose intent): the list is recomputed from the state at that moment.
    let mut v2 = engine_with(&ev, Ruleset::V2, &[]);
    drive_to_event(&mut v2);
    v2.state.supplies.set("rope", 0);
    v2.state.supplies.set("food", 50);
    let msgs = v2.step(&PlayerIntent::new(IntentAction::Travel));
    assert_eq!(v2.phase, GamePhase::Event, "re-present keeps the event up");
    let expect = vec![(false, keys(&["rope"])), (true, vec![]), (true, vec![])];
    let from_msgs: Vec<(bool, Vec<String>)> = msgs
        .event_choices
        .iter()
        .map(|c| (c.affordable, c.missing.clone()))
        .collect();
    assert_eq!(from_msgs, expect);
    assert_eq!(shape(&v2), expect);

    // Giving the party rope and re-presenting flips A to affordable.
    v2.state.supplies.set("rope", 3);
    v2.step(&PlayerIntent::new(IntentAction::Travel));
    assert_eq!(
        shape(&v2),
        vec![(true, vec![]), (true, vec![]), (true, vec![])]
    );

    // Choosing A with rope 0 is refused: the event stays up and the line names the choices
    // that can be picked. (`a_refused_pick_changes_nothing` checks that nothing moved.)
    v2.state.supplies.set("rope", 0);
    let msgs = v2.step(&PlayerIntent::new(IntentAction::Choose).with_choice("A"));
    assert_eq!(v2.phase, GamePhase::Event);
    assert_eq!(
        msgs.lines,
        vec!["That option isn't available -- choose one of: B/C.".to_string()]
    );
    assert_eq!(shape(&v2)[0], (false, keys(&["rope"])));
    // B is affordable and resolves: food 50 -> 48, back to camp.
    v2.step(&PlayerIntent::new(IntentAction::Choose).with_choice("B"));
    assert_eq!(v2.phase, GamePhase::Camp);
    assert_eq!(v2.state.supplies.get("food"), 48);
    assert_eq!(v2.state.supplies.get("rope"), 0);

    // Legacy, same event and state: everything is shown affordable, even re-presented.
    let mut legacy = engine_with(&ev, Ruleset::Legacy, &[]);
    drive_to_event(&mut legacy);
    legacy.state.supplies.set("rope", 0);
    legacy.state.supplies.set("food", 50);
    legacy.step(&PlayerIntent::new(IntentAction::Travel));
    assert_eq!(
        shape(&legacy),
        vec![(true, vec![]), (true, vec![]), (true, vec![])]
    );
    legacy.step(&PlayerIntent::new(IntentAction::Choose).with_choice("A"));
    assert_eq!(legacy.phase, GamePhase::Camp);
    assert_eq!(legacy.state.supplies.get("rope"), 0);
}

// b. Durable key ------------------------------------------------------------------------------

#[test]
fn v2_durable_supply_is_checked_never_spent() {
    let ev = event(
        "afford_tools",
        &["A", "B"],
        vec![("A", outcome(&[("tools", -1)])), ("B", outcome(&[]))],
    );

    let mut st = create_new_run(42);
    st.supplies.set("tools", 1);
    assert!(missing_supplies_for_choice(&st, &ev, "A", Ruleset::V2).is_empty());
    let mut rng = CpythonRng::from_seed(42);
    let out = resolve_event_with_ruleset(&mut st, &ev, "A", &mut rng, Ruleset::V2);
    assert!(out.special_flags.is_empty());
    assert!(out.supplies_delta.is_empty());
    apply_outcome(&mut st, &out);
    assert_eq!(st.supplies.get("tools"), 1);

    let mut st = create_new_run(42);
    st.supplies.set("tools", 0);
    assert_eq!(
        missing_supplies_for_choice(&st, &ev, "A", Ruleset::V2),
        keys(&["tools"])
    );
    let mut rng = CpythonRng::from_seed(42);
    let out = resolve_event_with_ruleset(&mut st, &ev, "A", &mut rng, Ruleset::V2);
    assert_eq!(out.special_flags, keys(&["insufficient_supplies"]));
}

// c. No soft-lock: an event is never left with every choice unaffordable ------------------------

fn zero(state: &mut RunState, ks: &[&str]) {
    for k in ks {
        state.supplies.set(k, 0);
    }
}

#[test]
fn v2_keeps_the_smallest_shortfall_choice_open_when_every_choice_is_unaffordable() {
    // Party holds none of rope / parts / meds. Shortfalls: A rope 2 = 2; B parts 1 = 1;
    // C meds 1 + rope 1 = 2. B is the smallest, so B stays open.
    let ev = event(
        "afford_none",
        &["A", "B", "C"],
        vec![
            ("A", outcome(&[("rope", -2)])),
            ("B", outcome(&[("parts", -1)])),
            ("C", outcome(&[("meds", -1), ("rope", -1)])),
        ],
    );
    let mut engine = engine_with(&ev, Ruleset::V2, &[]);
    drive_to_event(&mut engine);
    zero(&mut engine.state, &["rope", "parts", "meds"]);
    engine.step(&PlayerIntent::new(IntentAction::Travel));
    assert_eq!(
        shape(&engine),
        vec![
            (false, keys(&["rope"])),
            (true, vec![]),
            (false, keys(&["meds", "rope"])),
        ]
    );

    // Directly: B resolves normally (its delta kept, clamped at zero on apply, no denial);
    // A and C are still denied.
    let mut st = create_new_run(42);
    zero(&mut st, &["rope", "parts", "meds"]);
    let mut rng = CpythonRng::from_seed(42);
    let b = resolve_event_with_ruleset(&mut st, &ev, "B", &mut rng, Ruleset::V2);
    assert_eq!(b.supplies_delta.get("parts"), Some(&-1));
    assert!(b.special_flags.is_empty());
    apply_outcome(&mut st, &b);
    assert_eq!(st.supplies.get("parts"), 0);
    for id in ["A", "C"] {
        let d = resolve_event_with_ruleset(&mut st, &ev, id, &mut rng, Ruleset::V2);
        assert_eq!(d.special_flags, keys(&["insufficient_supplies"]), "{id}");
        assert!(d.supplies_delta.is_empty(), "{id}");
    }

    // Engine: picking the open choice returns to camp.
    engine.step(&PlayerIntent::new(IntentAction::Choose).with_choice("B"));
    assert_eq!(engine.phase, GamePhase::Camp);
    for k in ["rope", "parts", "meds"] {
        assert_eq!(engine.state.supplies.get(k), 0, "{k}");
    }
}

#[test]
fn v2_keep_open_ties_go_to_the_first_offered_and_do_not_apply_when_one_is_affordable() {
    // A rope 1 and B parts 1 are equally short: the first offered (A) stays open.
    let tie = event(
        "afford_tie",
        &["A", "B"],
        vec![
            ("A", outcome(&[("rope", -1)])),
            ("B", outcome(&[("parts", -1)])),
        ],
    );
    let mut st = create_new_run(42);
    zero(&mut st, &["rope", "parts"]);
    assert!(missing_supplies_for_choice(&st, &tie, "A", Ruleset::V2).is_empty());
    assert_eq!(
        missing_supplies_for_choice(&st, &tie, "B", Ruleset::V2),
        keys(&["parts"])
    );
    // With parts in hand B is affordable, so nothing is kept open and A is denied.
    st.supplies.set("parts", 1);
    assert_eq!(
        missing_supplies_for_choice(&st, &tie, "A", Ruleset::V2),
        keys(&["rope"])
    );
    // Legacy never shows a shortfall.
    assert!(missing_supplies_for_choice(&st, &tie, "A", Ruleset::Legacy).is_empty());
}

#[test]
fn v2_gate_covers_every_supply_the_party_carries_and_the_oil_alias() {
    let ev = event(
        "afford_keys",
        &["A", "B", "C", "D"],
        vec![
            ("A", outcome(&[("water", -3)])),
            ("B", outcome(&[("oil", -2)])),
            ("C", outcome(&[("ammo", -1), ("cloth", -2)])),
            ("D", outcome(&[("not_a_supply", -5)])),
        ],
    );
    let mut st = create_new_run(42);
    zero(&mut st, &["water", "lantern_oil", "ammo", "cloth"]);
    st.supplies.set("cloth", 1);
    let m = |id| raw_missing_supplies_for_choice(&st, &ev, id, Ruleset::V2);
    assert_eq!(m("A"), keys(&["water"]));
    assert_eq!(m("B"), keys(&["lantern_oil"]));
    assert_eq!(m("C"), keys(&["ammo", "cloth"]));
    assert!(
        m("D").is_empty(),
        "a key that is not a supply is not a cost"
    );
}

// d. Legacy untouched --------------------------------------------------------------------------

/// SHA-256 over the concatenated per-turn mechanics hashes of the first 200 seeds x 60 turns,
/// the same value measured on the commit before the ruleset switch existed (pinned in
/// `ruleset_v2_river_once.rs`). Comparing the default engine with `run_tier2_seed_trace` alone
/// could not fail, since both would move together; this literal can.
const LEGACY_200X60_DIGEST: &str =
    "12ecb6ef49ac962988daa41083146d696da59f0b76a27bc497b234f6832a4af3";

#[test]
fn legacy_default_ruleset_still_equals_the_tier2_trace() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut digest = Sha256::new();
    for (idx, &seed) in seeds.iter().take(200).enumerate() {
        let trace: Vec<String> = run_tier2_seed_trace(seed, idx, pack, 60)
            .turns
            .into_iter()
            .map(|t| t.mechanics_state_hash)
            .collect();
        let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        assert_eq!(engine.ruleset, Ruleset::Legacy);
        let mut hashes = Vec::new();
        for turn in 1..=60 {
            if engine.phase == GamePhase::GameOver || engine.state.game_over {
                break;
            }
            hashes.push(tier2_mechanics_turn(&mut engine, idx, turn));
        }
        assert_eq!(hashes, trace, "seed {seed}");
        for h in &hashes {
            digest.update(h.as_bytes());
        }
    }
    assert_eq!(format!("{:x}", digest.finalize()), LEGACY_200X60_DIGEST);
}

// e. V2 changes something real -----------------------------------------------------------------

fn no_negative_supply(state: &RunState) -> bool {
    state.supplies.to_dict().values().all(|v| *v >= 0)
}

/// Replays the Tier 2 policy's event pick (`(seed_idx * 31 + turn) % choices`), so the choice
/// taken can be inspected before the turn is played.
fn tier2_event_pick(seed_idx: usize, turn: usize, len: usize) -> usize {
    seed_idx.wrapping_mul(31).wrapping_add(turn) % len
}

/// Everything that must not move when a pick is refused: the saved state, the phase, the RNG
/// position and the pending event.
fn frozen(engine: &StepEngine) -> (String, GamePhase, u64, Option<String>) {
    (
        state_to_value(&engine.state).to_string(),
        engine.phase,
        engine.rng.counter,
        engine.pending_event().map(|e| e.event_id.clone()),
    )
}

/// (picks refused because the choice was unaffordable, event choices resolved, presentations
/// where no choice was affordable) over `n` seeds x `turns` under `ruleset`, with the Tier 2
/// policy. A refused pick must leave everything as it was; a resolved pick must be the one the
/// journal records.
fn count_refused_picks(ruleset: Ruleset, n: usize, turns: usize) -> (usize, usize, usize) {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut refused = 0usize;
    let mut resolved = 0usize;
    let mut all_unaffordable = 0usize;
    for (idx, &seed) in seeds.iter().take(n).enumerate() {
        let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        engine.set_ruleset(ruleset);
        for turn in 1..=turns {
            if engine.phase == GamePhase::GameOver || engine.state.game_over {
                break;
            }
            let mut expect_refused = false;
            let mut chosen = String::new();
            if engine.phase == GamePhase::Event && !engine.pending_event_choices().is_empty() {
                let ev = engine.pending_event().expect("pending event");
                if ev.fallback_choices.iter().all(|c| {
                    !raw_missing_supplies_for_choice(&engine.state, ev, &c.choice_id, Ruleset::V2)
                        .is_empty()
                }) {
                    all_unaffordable += 1;
                }
                let cs = engine.pending_event_choices();
                let c = &cs[tier2_event_pick(idx, turn, cs.len())];
                chosen = c.id.clone();
                // An unaffordable choice is only ever shown beside an affordable one: when none
                // is affordable, the kept-open choice is shown affordable.
                expect_refused = !c.affordable;
                if expect_refused {
                    assert!(cs.iter().any(|o| o.affordable), "seed {seed} turn {turn}");
                }
            }
            let before = frozen(&engine);
            let _ = tier2_mechanics_turn(&mut engine, idx, turn);
            if expect_refused {
                refused += 1;
                assert_eq!(
                    frozen(&engine),
                    before,
                    "seed {seed} turn {turn}: a refused pick moved something"
                );
            } else if !chosen.is_empty() {
                resolved += 1;
                let last = engine
                    .state
                    .journal
                    .iter()
                    .rev()
                    .find(|j| !j.choice_made.is_empty())
                    .expect("journal entry with a choice");
                assert!(
                    last.choice_made.starts_with(&format!("{chosen}:")),
                    "seed {seed} turn {turn}: replayed pick {chosen} vs journal {}",
                    last.choice_made
                );
            }
            assert!(no_negative_supply(&engine.state), "seed {seed} turn {turn}");
        }
    }
    (refused, resolved, all_unaffordable)
}

#[test]
fn v2_refuses_unaffordable_picks_over_1000_seeds_and_no_supply_goes_negative() {
    let (refused_v2, resolved_v2, all_unaffordable) = count_refused_picks(Ruleset::V2, 1000, 120);
    let (refused_legacy, _, _) = count_refused_picks(Ruleset::Legacy, 1000, 120);
    assert_eq!(
        refused_legacy, 0,
        "Legacy never marks a choice unaffordable"
    );
    // Measured once, under the Tier 2 policy (it does not look at affordability, so it asks for
    // unaffordable choices and is refused; the same event then stays up for its next turn).
    assert_eq!(
        (resolved_v2, refused_v2, all_unaffordable),
        (
            EXPECTED_RESOLVED,
            EXPECTED_REFUSED,
            EXPECTED_ALL_UNAFFORDABLE
        )
    );
    assert!(refused_v2 > 0);
}

const EXPECTED_RESOLVED: usize = 31_913;
const EXPECTED_REFUSED: usize = 2_898;
const EXPECTED_ALL_UNAFFORDABLE: usize = 45;

/// The fix in one place: a refused pick changes no state, no RNG position and no phase; the
/// line names only what can be picked; and the same pick under Legacy still goes through.
#[test]
fn a_refused_pick_changes_nothing() {
    let ev = three_choice_event();
    let mut v2 = engine_with(&ev, Ruleset::V2, &[("rope", 0), ("food", 50)]);
    drive_to_event(&mut v2);
    // Re-present so the list reflects the supplies now held.
    v2.step(&PlayerIntent::new(IntentAction::Travel));
    assert_eq!(shape(&v2)[0], (false, keys(&["rope"])));

    let before = frozen(&v2);
    let hash_before = etv_core::snapshot::compute_mechanics_state_hash(
        &v2.state,
        v2.phase,
        "afford_three",
        "",
        &BTreeMap::new(),
    );
    for _ in 0..5 {
        let msgs = v2.step(&PlayerIntent::new(IntentAction::Choose).with_choice("A"));
        assert_eq!(
            msgs.lines,
            vec!["That option isn't available -- choose one of: B/C.".to_string()]
        );
        assert!(msgs.outcome_title.is_empty());
        assert!(msgs.outcome_deltas.is_empty());
        // The event is shown again with the same choices.
        assert_eq!(msgs.event_choices.len(), 3);
    }
    assert_eq!(frozen(&v2), before);
    let hash_after = etv_core::snapshot::compute_mechanics_state_hash(
        &v2.state,
        v2.phase,
        "afford_three",
        "",
        &BTreeMap::new(),
    );
    assert_eq!(hash_after, hash_before);
    assert_eq!(v2.state.journal.len(), 0);

    // Legacy, same event and supplies: the pick goes through (frozen behaviour).
    let mut legacy = engine_with(&ev, Ruleset::Legacy, &[("rope", 0), ("food", 50)]);
    drive_to_event(&mut legacy);
    legacy.step(&PlayerIntent::new(IntentAction::Choose).with_choice("A"));
    assert_eq!(legacy.phase, GamePhase::Camp);
}

/// What the player sees in line mode: the unaffordable choice carries a tag, picking it prints
/// the refusal and asks again in place, and an affordable pick then resolves.
#[test]
fn line_mode_shows_the_tag_and_the_refusal_and_asks_again() {
    use etv_core::narrator::NullNarrator;
    use etv_core::play::{choice_line, run_play, PlayExit};
    let ev = three_choice_event();
    let mut v2 = engine_with(&ev, Ruleset::V2, &[("rope", 0), ("food", 50)]);
    drive_to_event(&mut v2);
    v2.step(&PlayerIntent::new(IntentAction::Travel));
    assert_eq!(
        choice_line(&v2.pending_event_choices()[0]),
        "  A. Choice A [cannot afford: rope]"
    );
    assert_eq!(choice_line(&v2.pending_event_choices()[1]), "  B. Choice B");

    let food_before = v2.state.supplies.get("food");
    let mut out = Vec::new();
    let exit = run_play(
        &mut v2,
        &NullNarrator,
        None,
        "TEST",
        std::io::Cursor::new("a\nb\nq\n"),
        &mut out,
    )
    .unwrap();
    assert_eq!(exit, PlayExit::Quit);
    let text = String::from_utf8(out).unwrap();
    // The refusal, then the same event again with the same tagged choice, then B resolves.
    let refusal = "> That option isn't available -- choose one of: B/C.\n";
    let at = text.find(refusal).expect("refusal line");
    let after = &text[at + refusal.len()..];
    assert!(after.contains("  A. Choice A [cannot afford: rope]\n"));
    assert!(after.contains("> [Choice B] "));
    assert_eq!(text.matches("That option isn't available").count(), 1);
    assert_eq!(v2.state.supplies.get("food"), food_before - 2);
}

#[test]
fn win_counts_over_1000_seeds_legacy_vs_v2() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut wins = [0usize; 2];
    for (i, rs) in [Ruleset::Legacy, Ruleset::V2].into_iter().enumerate() {
        for (idx, &seed) in seeds.iter().enumerate() {
            let run = run_tier2_mechanics_with_ruleset(seed, idx, pack, 120, rs);
            if run.state.victory {
                wins[i] += 1;
            }
        }
    }
    // Measured, reported as measured: no explanation is claimed here.
    assert_eq!(
        wins,
        [2, 3],
        "wins over 1,000 seeds x 120 turns: [Legacy, V2]"
    );
}

// f. Save and resume ----------------------------------------------------------------------------

#[test]
fn v2_run_saved_and_reloaded_at_camp_continues_identically() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    for (idx, &seed) in seeds.iter().take(20).enumerate() {
        let mut full = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        full.set_ruleset(Ruleset::V2);
        let mut hashes = Vec::new();
        let mut cut: Option<usize> = None;
        for turn in 1..=60 {
            if full.phase == GamePhase::GameOver || full.state.game_over {
                break;
            }
            hashes.push(tier2_mechanics_turn(&mut full, idx, turn));
            if cut.is_none() && turn >= 20 && full.phase == GamePhase::Camp {
                cut = Some(turn);
            }
        }
        let cut = cut.unwrap_or_else(|| panic!("seed {seed}: no camp turn at or after turn 20"));
        assert!(
            cut < hashes.len(),
            "seed {seed}: nothing left after the cut"
        );

        let mut first = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
        first.set_ruleset(Ruleset::V2);
        for turn in 1..=cut {
            let _ = tier2_mechanics_turn(&mut first, idx, turn);
        }
        let restored = value_to_state(&state_to_value(&first.state)).expect("restore");
        let mut resumed = StepEngine::with_content(restored, pack);
        resumed.set_ruleset(Ruleset::V2);
        let mut tail = Vec::new();
        for turn in (cut + 1)..=hashes.len() {
            if resumed.phase == GamePhase::GameOver || resumed.state.game_over {
                break;
            }
            tail.push(tier2_mechanics_turn(&mut resumed, idx, turn));
        }
        assert_eq!(tail, hashes[cut..], "seed {seed}");
    }
}

// g. How many shipped choices the gate touches -----------------------------------------------------

#[test]
fn how_many_shipped_choices_the_gate_touches() {
    let lib = build_event_library();
    let empty = {
        let mut s = create_new_run(42);
        let all_keys: Vec<String> = s.supplies.to_dict().keys().cloned().collect();
        for k in all_keys {
            s.supplies.set(&k, 0);
        }
        s
    };
    let mut all_costed: Vec<String> = Vec::new();
    let (mut total, mut v2_gated, mut legacy_gated, mut events_all_gated) = (0, 0, 0, 0);
    let (mut json_total, mut json_v2, mut json_legacy, mut json_any_neg) = (0, 0, 0, 0);
    for (i, ev) in lib.iter().enumerate() {
        let mut all = !ev.fallback_choices.is_empty();
        for ch in &ev.fallback_choices {
            let v2 =
                !raw_missing_supplies_for_choice(&empty, ev, &ch.choice_id, Ruleset::V2).is_empty();
            let tmpl = ev.outcome_templates.get(&ch.choice_id).unwrap();
            let lg = !etv_core::events::advertised_inventory_cost(Some(ch), tmpl).is_empty();
            total += 1;
            v2_gated += v2 as usize;
            legacy_gated += lg as usize;
            if i >= 60 {
                json_any_neg += tmpl.supplies_delta.values().any(|v| *v < 0) as usize;
                json_total += 1;
                json_v2 += v2 as usize;
                json_legacy += lg as usize;
            }
            all &= v2;
        }
        events_all_gated += all as usize;
        if all {
            all_costed.push(ev.event_id.clone());
        }
    }
    // `empty` holds none of the 12 supplies, so "missing" lists exactly what each choice requires.
    assert_eq!(lib.len(), 260);
    assert_eq!(total, 627, "all choices");
    assert_eq!(v2_gated, 202, "choices that require a supply under V2");
    assert_eq!(
        legacy_gated, 17,
        "choices that require a supply under Legacy (hints only)"
    );
    assert_eq!(json_total, 464, "JSON-event choices");
    assert_eq!(json_v2, 158, "JSON-event choices gated under V2");
    assert_eq!(json_legacy, 0, "JSON-event choices gated under Legacy");
    assert_eq!(
        json_any_neg, 158,
        "JSON-event choices with a negative delta on any key"
    );
    assert_eq!(
        all_costed,
        keys(&["wolf_pack", "f1_036", "f2_036", "f3_035", "f4_074"]),
        "events where every choice requires a supply"
    );
    assert_eq!(events_all_gated, 5);
}
