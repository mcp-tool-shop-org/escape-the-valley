//! Save schema v5: the save holds the pending event or fork; old saves are refused.
//!
//! Every expected value here is a literal written by hand or measured once and written down.

use etv_core::content::{embedded_content_pack, ContentPack};
use etv_core::engine::{EventChoiceInfo, RouteOption, StepEngine};
use etv_core::model::{GamePhase, IntentAction, PlayerIntent, RunState};
use etv_core::ruleset::Ruleset;
use etv_core::save::{
    load_game_result_with_content_hash, state_to_value, SaveLoadError, SAVE_DIR, SAVE_FILE,
};
use etv_core::save_v5::{
    load_saved_game, save_game_v5, state_to_value_v5, value_to_saved_game, PendingBlock,
};
use etv_core::snapshot::{
    build_1000_seeds, compute_mechanics_state_hash, run_tier2_mechanics_with_ruleset,
    tier2_mechanics_turn,
};
use etv_core::worldgen::create_new_run_with_content;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Default)]
struct Counts {
    steps: usize,
    reloads: usize,
    skipped: usize,
    in_event: usize,
    in_route: usize,
}

/// Play up to `turns` Tier 2 turns under V2, saving after EVERY step and resuming from the
/// save. With `files`, the engine autosaves to that directory and reloads from the real file.
fn interrupted_run(
    seed: i128,
    idx: usize,
    turns: usize,
    files: Option<&Path>,
) -> (Vec<String>, Counts) {
    let pack = embedded_content_pack();
    let state = create_new_run_with_content(seed, pack);
    let mut engine = match files {
        Some(base) => StepEngine::with_options(state, true, Some(base.to_path_buf())),
        None => StepEngine::with_content(state, pack),
    };
    engine.event_library = pack.all_events.clone();
    engine.set_ruleset(Ruleset::V2);
    let mut hashes = Vec::new();
    let mut c = Counts::default();
    for turn in 1..=turns {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        hashes.push(tier2_mechanics_turn(&mut engine, idx, turn));
        c.steps += 1;
        let phase = engine.phase;
        let saved = match files {
            Some(base) => load_saved_game(base, pack).expect("load from file"),
            None => {
                let pending = engine.pending_block();
                if matches!(phase, GamePhase::Event | GamePhase::Route) && pending.is_none() {
                    c.skipped += 1;
                    continue;
                }
                let value = state_to_value_v5(&engine.state, pending.as_ref());
                value_to_saved_game(&value, pack).expect("load from value")
            }
        };
        engine = StepEngine::resume(
            saved.state,
            saved.pending,
            pack,
            Ruleset::V2,
            files.is_some(),
            files.map(|b| b.to_path_buf()),
        )
        .expect("resume");
        assert_eq!(
            engine.phase, phase,
            "seed {seed} turn {turn}: phase must survive the save"
        );
        c.reloads += 1;
        match phase {
            GamePhase::Event => c.in_event += 1,
            GamePhase::Route => c.in_route += 1,
            _ => {}
        }
    }
    (hashes, c)
}

fn new_v2_engine(seed: i128, pack: &ContentPack) -> StepEngine {
    let mut engine = StepEngine::with_content(create_new_run_with_content(seed, pack), pack);
    engine.set_ruleset(Ruleset::V2);
    engine
}

/// Seed index 0 (seed 0) under V2, after `stop_turn` Tier 2 turns.
fn seed0_after_turn(pack: &ContentPack, stop_turn: usize) -> StepEngine {
    let mut engine = new_v2_engine(0, pack);
    for turn in 1..=stop_turn {
        tier2_mechanics_turn(&mut engine, 0, turn);
    }
    engine
}

fn hash_after_choice(engine: &mut StepEngine, choice: &str) -> String {
    let msgs = engine.step(&PlayerIntent::new(IntentAction::Choose).with_choice(choice));
    engine.state.rng_counter = engine.rng.counter;
    engine.state.rng_state = Some(engine.rng.getstate());
    compute_mechanics_state_hash(
        &engine.state,
        engine.phase,
        "",
        choice,
        &msgs.outcome_deltas,
    )
}

fn resume_v2(engine: &StepEngine, pack: &ContentPack) -> StepEngine {
    let value = state_to_value_v5(&engine.state, engine.pending_block().as_ref());
    let saved = value_to_saved_game(&value, pack).expect("load");
    StepEngine::resume(saved.state, saved.pending, pack, Ruleset::V2, false, None).expect("resume")
}

// a. Save every step ----------------------------------------------------------------------

#[test]
fn save_every_step_under_v2_matches_the_uninterrupted_run() {
    let pack = embedded_content_pack();
    let seeds = build_1000_seeds();
    let mut total = Counts::default();
    for (idx, &seed) in seeds.iter().take(100).enumerate() {
        let plain = run_tier2_mechanics_with_ruleset(seed, idx, pack, 60, Ruleset::V2);
        let (hashes, c) = interrupted_run(seed, idx, 60, None);
        assert_eq!(
            hashes, plain.hashes,
            "seed index {idx}: hash sequence diverged after a reload"
        );
        if idx < 5 {
            let dir = tempfile::tempdir().expect("tempdir");
            let (file_hashes, fc) = interrupted_run(seed, idx, 60, Some(dir.path()));
            assert_eq!(
                file_hashes, plain.hashes,
                "seed index {idx}: file reloads diverged"
            );
            assert_eq!(
                fc.reloads, fc.steps,
                "seed index {idx}: a file reload was skipped"
            );
        }
        total.steps += c.steps;
        total.reloads += c.reloads;
        total.skipped += c.skipped;
        total.in_event += c.in_event;
        total.in_route += c.in_route;
    }
    assert_eq!(total.skipped, 0, "no reload point may be skipped");
    assert_eq!(total.reloads, total.steps, "one reload per step");
    assert_eq!(total.steps, 5963);
    assert_eq!(
        total.in_event, 2032,
        "reloads taken while an event was on screen"
    );
    assert_eq!(
        total.in_route, 118,
        "reloads taken while a fork was on screen"
    );
}

// b. A resumed event shows the same thing ---------------------------------------------------

#[test]
fn resumed_event_shows_the_same_title_narration_and_choices() {
    let pack = embedded_content_pack();
    // Seed 0 under V2: after turn 16 the player is looking at "Fellow Travelers".
    let before = seed0_after_turn(pack, 16);
    assert_eq!(before.phase, GamePhase::Event);
    let resumed = resume_v2(&before, pack);
    for engine in [&before, &resumed] {
        assert_eq!(engine.phase, GamePhase::Event);
        assert_eq!(
            engine.pending_event().unwrap().event_id,
            "friendly_travelers"
        );
        assert_eq!(
            engine.pending_event_text(),
            (
                "Fellow Travelers",
                "Another party camps nearby. They share a fire and news of the road ahead."
            )
        );
        let got: Vec<(&str, &str, &str, &str)> = engine
            .pending_event_choices()
            .iter()
            .map(|c| {
                (
                    c.id.as_str(),
                    c.label.as_str(),
                    c.risk_hint.as_str(),
                    c.cost_hint.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "A",
                    "Share a meal and trade news.",
                    "Morale boost, food cost.",
                    "Food."
                ),
                (
                    "B",
                    "Trade supplies.",
                    "Exchange what you have.",
                    "Depends on trade."
                ),
                ("C", "Keep to yourselves.", "No interaction.", "Nothing."),
            ]
        );
    }
    assert_eq!(resumed.msgs.event_title, "Fellow Travelers");
    // Choosing gives the same state as without the save.
    let mut a = before;
    let mut b = resumed;
    assert_eq!(
        hash_after_choice(&mut a, "B"),
        hash_after_choice(&mut b, "B")
    );
}

// c. A resumed fork -------------------------------------------------------------------------

#[test]
fn resumed_fork_offers_the_same_routes_and_chooses_the_same() {
    let pack = embedded_content_pack();
    // Seed 0 under V2: after turn 14 the trail forks between The Gulch and Dunhollow.
    let before = seed0_after_turn(pack, 14);
    assert_eq!(before.phase, GamePhase::Route);
    let resumed = resume_v2(&before, pack);
    for engine in [&before, &resumed] {
        assert_eq!(engine.phase, GamePhase::Route);
        let got: Vec<(&str, &str, i32)> = engine
            .pending_routes()
            .iter()
            .map(|r| (r.node_id.as_str(), r.name.as_str(), r.distance))
            .collect();
        assert_eq!(
            got,
            vec![("node_03", "The Gulch", 15), ("node_04", "Dunhollow", 23)]
        );
    }
    let mut a = before;
    let mut b = resumed;
    assert_eq!(
        hash_after_choice(&mut a, "B"),
        hash_after_choice(&mut b, "B")
    );
    assert_eq!(a.state.destination_id, "node_04");
    assert_eq!(b.state.destination_id, "node_04");
}

// d. Legacy is unchanged --------------------------------------------------------------------

#[test]
fn legacy_saved_mid_event_comes_back_at_camp_with_no_pending_block() {
    let pack = embedded_content_pack();
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path();
    let mut engine = StepEngine::with_options(
        create_new_run_with_content(0, pack),
        true,
        Some(base.to_path_buf()),
    );
    engine.event_library = pack.all_events.clone();
    assert_eq!(engine.ruleset, Ruleset::Legacy);
    let mut turn = 0;
    while engine.phase != GamePhase::Event {
        turn += 1;
        assert!(turn <= 60, "Legacy seed 0 should show an event in 60 turns");
        tier2_mechanics_turn(&mut engine, 0, turn);
    }
    assert!(engine.pending_event().is_some());
    assert!(
        engine.pending_block().is_none(),
        "Legacy never offers a pending block"
    );

    let raw: Value = serde_json::from_str(
        &fs::read_to_string(base.join(SAVE_DIR).join(SAVE_FILE)).expect("read save"),
    )
    .expect("json");
    assert_eq!(
        raw["save_version"],
        json!(4),
        "Legacy keeps writing the frozen v4 save"
    );
    assert!(
        raw.get("pending").is_none(),
        "Legacy save has no pending block"
    );

    let loaded = load_game_result_with_content_hash(base, &pack.content_hash_hex);
    assert!(loaded.ok, "{}", loaded.reason);
    let reloaded = StepEngine::resume(
        loaded.state.unwrap(),
        None,
        pack,
        Ruleset::Legacy,
        false,
        None,
    )
    .expect("resume");
    assert_eq!(reloaded.phase, GamePhase::Camp);
    assert!(reloaded.pending_event().is_none());
    assert!(reloaded.pending_event_choices().is_empty());

    // Even handed a block, a Legacy engine does not restore it.
    let block = PendingBlock::route(vec![RouteOption {
        node_id: "node_03".to_string(),
        name: "The Gulch".to_string(),
        distance: 15,
    }]);
    let ignored = StepEngine::resume(
        create_new_run_with_content(0, pack),
        Some(block),
        pack,
        Ruleset::Legacy,
        false,
        None,
    )
    .expect("resume");
    assert_eq!(ignored.phase, GamePhase::Camp);
    assert!(ignored.pending_routes().is_empty());
}

// e. Refusals -------------------------------------------------------------------------------

/// Write `value` as the save, try to load it, and report the error and whether the file (and
/// its directory) are exactly as they were.
fn refuse(value: &Value) -> (SaveLoadError, bool) {
    let pack = embedded_content_pack();
    let dir = tempfile::tempdir().unwrap();
    let save_dir = dir.path().join(SAVE_DIR);
    fs::create_dir_all(&save_dir).unwrap();
    let path = save_dir.join(SAVE_FILE);
    fs::write(&path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let err = load_saved_game(dir.path(), pack).expect_err("must be refused");
    let after = fs::read(&path).unwrap();
    let entries = fs::read_dir(&save_dir).unwrap().count();
    (err, before == after && entries == 1)
}

#[test]
fn refusals_have_their_own_variant_message_and_leave_the_file_alone() {
    let pack = embedded_content_pack();
    let base = create_new_run_with_content(42, pack);

    let mut v3 = state_to_value(&base);
    v3["save_version"] = json!(3);
    let (err, untouched) = refuse(&v3);
    assert_eq!(err, SaveLoadError::PythonSaveRefused { save_version: 3 });
    assert_eq!(
        err.message(),
        "This save is from Escape the Valley 1.x (save format 3). Version 2 cannot continue it, because the two versions play events differently and a continued run would not be the same run. The 1.3 game still plays it: install it with `pip install \"escape-the-valley<2\"`, then run `trail play` (or `trail tui --continue` for the full-screen game). Your save file has not been changed."
    );
    assert!(untouched);

    let mut none = state_to_value(&base);
    none.as_object_mut().unwrap().remove("save_version");
    let (err, untouched) = refuse(&none);
    assert_eq!(err, SaveLoadError::PythonSaveRefused { save_version: 0 });
    assert_eq!(
        err.message(),
        "This save is from Escape the Valley 1.x (save format 0). Version 2 cannot continue it, because the two versions play events differently and a continued run would not be the same run. The 1.3 game still plays it: install it with `pip install \"escape-the-valley<2\"`, then run `trail play` (or `trail tui --continue` for the full-screen game). Your save file has not been changed."
    );
    assert!(untouched);

    let (err, untouched) = refuse(&state_to_value(&base));
    assert_eq!(
        err,
        SaveLoadError::DevelopmentSaveRefused { save_version: 4 }
    );
    assert_eq!(
        err.message(),
        "This save (save format 4) was made by an earlier development build of Escape the Valley 2 and cannot be continued. Please start a new run. Your save file has not been changed."
    );
    assert!(untouched);

    let mut v6 = state_to_value_v5(&base, None);
    v6["save_version"] = json!(6);
    let (err, untouched) = refuse(&v6);
    assert_eq!(err, SaveLoadError::SaveFormatTooNew { save_version: 6 });
    assert_eq!(
        err.message(),
        "This save (save format 6) was made by a newer version of Escape the Valley than this one, which reads save formats up to 5. Please update the game to continue it. Your save file has not been changed."
    );
    assert!(untouched);

    // A genuine v5 file is accepted by the same loader.
    let dir = tempfile::tempdir().unwrap();
    save_game_v5(&base, None, dir.path()).expect("save");
    let saved = load_saved_game(dir.path(), pack).expect("v5 loads");
    assert_eq!(saved.pending, None);
}

// f. Corrupt pending blocks -----------------------------------------------------------------

fn corrupt_message(pending: Value) -> String {
    let pack = embedded_content_pack();
    let mut v = state_to_value_v5(&create_new_run_with_content(42, pack), None);
    v["pending"] = pending;
    match value_to_saved_game(&v, pack) {
        Err(SaveLoadError::Corrupt(msg)) => msg,
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn corrupt_pending_blocks_are_named_errors_not_panics() {
    let choices = json!([{"id": "A", "label": "x", "risk_hint": "", "cost_hint": ""}]);
    assert_eq!(
        corrupt_message(json!({
            "phase": "event", "event_id": "no_such_event", "choices": choices,
            "title": "t", "narration": "n"
        })),
        "pending event 'no_such_event' is not in the event library"
    );
    assert_eq!(
        corrupt_message(json!({"phase": "event"})),
        "pending block with phase 'event' has no event_id"
    );
    assert_eq!(
        corrupt_message(json!({"phase": "sideways"})),
        "pending block has unknown phase 'sideways'"
    );
    assert_eq!(corrupt_message(json!({})), "pending block has no phase");
    assert_eq!(
        corrupt_message(json!({"phase": "route", "routes": []})),
        "pending block with phase 'route' has no routes"
    );

    // The engine itself also refuses an unknown id, without panicking.
    let pack = embedded_content_pack();
    let block = PendingBlock::event(
        "no_such_event",
        vec![EventChoiceInfo {
            id: "A".to_string(),
            label: "x".to_string(),
            ..Default::default()
        }],
        "t",
        "n",
    );
    let err = StepEngine::resume(
        create_new_run_with_content(42, pack),
        Some(block),
        pack,
        Ruleset::V2,
        false,
        None,
    )
    .err()
    .expect("unknown id must fail");
    assert_eq!(
        err,
        SaveLoadError::Corrupt("pending event 'no_such_event' is not in the event library".into())
    );
}

// g. The block never changes mechanics hashes ------------------------------------------------

#[test]
fn pending_block_never_changes_the_state_or_its_hash() {
    let pack = embedded_content_pack();
    let engine = seed0_after_turn(pack, 16);
    assert_eq!(engine.phase, GamePhase::Event);
    let block = engine.pending_block().expect("block");
    let with = value_to_saved_game(&state_to_value_v5(&engine.state, Some(&block)), pack).unwrap();
    let without = value_to_saved_game(&state_to_value_v5(&engine.state, None), pack).unwrap();
    assert!(with.pending.is_some());
    assert!(without.pending.is_none());
    assert_eq!(
        with.state, without.state,
        "the state is the same with or without the block"
    );
    let deltas = BTreeMap::new();
    let h = |s: &RunState| {
        compute_mechanics_state_hash(s, GamePhase::Event, "friendly_travelers", "", &deltas)
    };
    assert_eq!(h(&with.state), h(&without.state));
    // And the v5 value differs from the block-less value only by the block itself.
    let mut stripped = state_to_value_v5(&engine.state, Some(&block));
    stripped.as_object_mut().unwrap().remove("pending");
    assert_eq!(stripped, state_to_value_v5(&engine.state, None));
}
