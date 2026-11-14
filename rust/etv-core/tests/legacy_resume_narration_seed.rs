//! A resumed Legacy run never reuses a narration seed. The frozen v4 save cannot carry the step
//! counter, and some camp turns leave it exactly as it was (a pace change, a refused hunt or
//! repair), so the counter lives in `.trail/narration.json`, written before `run.json` on every
//! v4 save. These tests resume through the real v4 path: `save_game` on every engine step and
//! `load_game_checked_with_content_hash`, as `etv` does.

use std::collections::BTreeSet;
use std::path::Path;

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::{GMProfile, GamePhase, IntentAction, PlayerIntent, RunState};
use etv_core::narrator::{
    build_narration_context_with_phase, evaluate_turn_matters, narration_call_seed,
    NarrationContext, NarratorLength, PreTurnObservation,
};
use etv_core::ruleset::Ruleset;
use etv_core::save::{
    load_game_checked_with_content_hash, read_narration_sidecar, save_game, state_to_value,
    write_narration_sidecar, NARRATION_FILE, NARRATION_FILE_MAX_BYTES, SAVE_DIR, SAVE_FILE,
};
use etv_core::save_v5::{state_to_value_v5, value_to_saved_game};
use etv_core::simulate::{choose_intent, Policy};
use etv_core::worldgen::create_new_run_with_content;

/// A new Legacy run at seed `seed`, saved the way `etv new` saves it.
fn new_run(seed: i128, dir: &Path) -> RunState {
    let state = create_new_run_with_content(seed, embedded_content_pack());
    save_game(&state, dir).unwrap();
    state
}

/// An engine that autosaves under `dir`, as `etv` builds it (Legacy is the default ruleset).
fn engine_at(state: RunState, dir: &Path) -> StepEngine {
    let mut engine = StepEngine::with_options(state, true, Some(dir.to_path_buf()));
    engine.event_library = embedded_content_pack().all_events.clone();
    engine
}

/// Quit and resume: load the save from disk and build a fresh engine on it.
fn resume(dir: &Path) -> StepEngine {
    let content = embedded_content_pack();
    let state = load_game_checked_with_content_hash(dir, &content.content_hash_hex).unwrap();
    engine_at(state, dir)
}

/// One turn's narration seeds (scene and outcome) and its context.
struct Turn {
    ctx: NarrationContext,
    seeds: [u32; 2],
}

fn step(engine: &mut StepEngine, intent: &PlayerIntent) -> Turn {
    let pre = PreTurnObservation::capture(engine.phase, &engine.state);
    let msgs = engine.step(intent);
    let flags = evaluate_turn_matters(&pre, intent, &msgs, &engine.state);
    let ctx = build_narration_context_with_phase(
        &engine.state,
        engine.phase,
        "",
        &intent.choice_id,
        &msgs,
        flags,
    );
    let seed = |scene| narration_call_seed(&ctx, GMProfile::Fireside, NarratorLength::Brief, scene);
    let seeds = [seed(true), seed(false)];
    Turn { ctx, seeds }
}

fn pace(p: &str) -> PlayerIntent {
    let mut intent = PlayerIntent::new(IntentAction::ChangePace);
    intent.pace = p.to_string();
    intent
}

fn run_json(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join(SAVE_DIR).join(SAVE_FILE)).unwrap()
}

/// Every seed input of a turn except its index, as one string.
fn facts(ctx: &NarrationContext) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{:?}",
        ctx.run_seed(),
        ctx.day(),
        ctx.time_of_day().as_str(),
        ctx.phase().as_str(),
        ctx.action(),
        ctx.event_id(),
        ctx.choice_id(),
        ctx.place(),
        ctx.shown_deltas()
    )
}

/// The two turns have the same facts, and the resume still told them apart.
fn assert_fresh_across_a_resume(before: &Turn, after: &Turn) {
    assert_eq!(
        facts(&before.ctx),
        facts(&after.ctx),
        "the case needs two turns with the same facts"
    );
    assert_ne!(before.ctx.turn_index(), after.ctx.turn_index());
    assert_ne!(before.seeds[0], after.seeds[0], "scene seed repeated");
    assert_ne!(before.seeds[1], after.seeds[1], "outcome seed repeated");
}

#[test]
fn a_pace_change_after_a_resume_gets_a_fresh_seed() {
    // Seed 42, Day 1 morning at camp: hard, quit and resume, slow.
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_at(new_run(42, dir.path()), dir.path());
    assert_eq!(engine.phase, GamePhase::Camp);
    assert_eq!(engine.state.day, 1);
    let hard = step(&mut engine, &pace("hard"));
    drop(engine);

    let mut engine = resume(dir.path());
    assert_eq!(engine.state.turns_stepped, 1, "the count came back");
    let slow = step(&mut engine, &pace("slow"));
    assert_eq!(slow.ctx.turn_index(), 2);
    assert_fresh_across_a_resume(&hard, &slow);
}

#[test]
fn a_refused_hunt_after_a_resume_gets_a_fresh_seed() {
    // No ammunition: the hunt is refused and the save after it is the save before it.
    let dir = tempfile::tempdir().unwrap();
    let mut state = new_run(42, dir.path());
    state.supplies.set("ammo", 0);
    save_game(&state, dir.path()).unwrap();
    let mut engine = engine_at(state, dir.path());
    let hunt = PlayerIntent::new(IntentAction::Hunt);
    // The first refusal writes `last_action` (and the engine's RNG state); after it, nothing.
    let primer = step(&mut engine, &hunt);
    let saved = run_json(dir.path());
    let first = step(&mut engine, &hunt);
    assert_eq!(
        run_json(dir.path()),
        saved,
        "a refused hunt changes nothing saved"
    );
    drop(engine);

    let mut engine = resume(dir.path());
    let second = step(&mut engine, &hunt);
    assert_eq!(run_json(dir.path()), saved);
    assert_fresh_across_a_resume(&primer, &second);
    assert_fresh_across_a_resume(&first, &second);
}

#[test]
fn a_refused_repair_after_a_resume_gets_a_fresh_seed() {
    // A wagon at 90 or better needs no repair: refused, and nothing changes at all.
    let dir = tempfile::tempdir().unwrap();
    let mut state = new_run(42, dir.path());
    state.wagon.condition = 95;
    assert!(
        state.supplies.parts() > 0,
        "the refusal must be the condition's"
    );
    save_game(&state, dir.path()).unwrap();
    let mut engine = engine_at(state, dir.path());
    let repair = PlayerIntent::new(IntentAction::Repair);
    // The first engine save also records the RNG state; from then on nothing changes.
    let primer = step(&mut engine, &repair);
    let saved = run_json(dir.path());
    let first = step(&mut engine, &repair);
    assert_eq!(
        run_json(dir.path()),
        saved,
        "a refused repair changes nothing saved"
    );
    drop(engine);

    let mut engine = resume(dir.path());
    let second = step(&mut engine, &repair);
    assert_eq!(run_json(dir.path()), saved);
    assert_fresh_across_a_resume(&primer, &second);
    assert_fresh_across_a_resume(&first, &second);
}

#[test]
fn the_same_run_json_after_one_turn_or_three_still_resumes_fresh() {
    // The case that rules out deriving the count from run.json: after "hard" and after
    // "hard, slow, hard" the v4 save is the same file, byte for byte.
    let (one, three) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let start = new_run(42, one.path());
    save_game(&start, three.path()).unwrap();

    let mut a = engine_at(start.clone(), one.path());
    let a_turns = vec![step(&mut a, &pace("hard"))];
    let mut b = engine_at(start, three.path());
    let b_turns: Vec<Turn> = ["hard", "slow", "hard"]
        .iter()
        .map(|p| step(&mut b, &pace(p)))
        .collect();
    drop((a, b));
    assert_eq!(run_json(one.path()), run_json(three.path()));

    for (dir, before, count) in [(&one, &a_turns, 1), (&three, &b_turns, 3)] {
        let mut engine = resume(dir.path());
        assert_eq!(engine.state.turns_stepped, count);
        let after = step(&mut engine, &pace("slow"));
        for turn in before {
            assert_fresh_across_a_resume(turn, &after);
        }
    }
}

#[test]
fn resuming_after_every_step_never_repeats_a_seed() {
    // Quit and resume after every single step, over seeds, policies and weirdness levels: no
    // two turns of one run share an index, so none shares (turn index, facts).
    let mut turns_checked = 0usize;
    let mut same_facts_pairs = 0usize;
    for seed in [7i128, 42, 99] {
        for policy in [Policy::First, Policy::Cautious, Policy::Rotating] {
            for level in [0, 2] {
                let dir = tempfile::tempdir().unwrap();
                let mut state = create_new_run_with_content(seed, embedded_content_pack());
                state.weirdness_level = level;
                save_game(&state, dir.path()).unwrap();
                let mut indices = BTreeSet::new();
                let mut seeds = BTreeSet::new();
                let mut by_facts: std::collections::BTreeMap<String, usize> = Default::default();
                for turn in 1..=120usize {
                    let mut engine = resume(dir.path());
                    if engine.phase == GamePhase::GameOver {
                        break;
                    }
                    assert_eq!(engine.state.turns_stepped, turn as u64 - 1);
                    let (intent, _) = choose_intent(policy, &engine, seed as usize, turn);
                    let t = step(&mut engine, &intent);
                    assert_eq!(t.ctx.turn_index(), turn as u64);
                    assert!(indices.insert(t.ctx.turn_index()));
                    for s in t.seeds {
                        assert!(
                            seeds.insert(s),
                            "seed {seed} {policy:?} level {level} turn {turn}: seed repeated"
                        );
                    }
                    *by_facts.entry(facts(&t.ctx)).or_default() += 1;
                    turns_checked += 1;
                }
                same_facts_pairs += by_facts.values().filter(|&&n| n > 1).count();
            }
        }
    }
    assert!(turns_checked > 800, "only {turns_checked} turns");
    // The sweep must contain turns whose facts repeat, or it proves nothing about them.
    assert!(same_facts_pairs > 0, "no two turns shared their facts");
}

#[test]
fn a_crash_between_the_two_writes_still_resumes_fresh() {
    // The sidecar is written first. A crash after it and before run.json leaves the new count
    // beside the old save: the resumed run is ahead, never behind.
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_at(new_run(42, dir.path()), dir.path());
    let hard = step(&mut engine, &pace("hard"));
    let saved = run_json(dir.path());

    // The next turn, played without saving, then only the first of the two writes.
    let mut state = engine.state.clone();
    drop(engine);
    let mut unsaved = StepEngine::with_options(state.clone(), false, None);
    unsaved.event_library = embedded_content_pack().all_events.clone();
    let lost = step(&mut unsaved, &pace("slow"));
    state.turns_stepped = unsaved.state.turns_stepped;
    write_narration_sidecar(&dir.path().join(SAVE_DIR), &state).unwrap();
    assert_eq!(run_json(dir.path()), saved, "run.json was not written");

    let mut engine = resume(dir.path());
    assert_eq!(engine.state.turns_stepped, 2);
    let after = step(&mut engine, &pace("slow"));
    assert_fresh_across_a_resume(&hard, &after);
    assert_fresh_across_a_resume(&lost, &after);
}

#[test]
fn a_missing_or_bad_sidecar_reads_zero_and_changes_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_at(new_run(42, dir.path()), dir.path());
    for p in ["hard", "slow", "hard"] {
        step(&mut engine, &pace(p));
    }
    let run_id = engine.state.run_id.clone();
    drop(engine);
    let save_dir = dir.path().join(SAVE_DIR);
    let sidecar = save_dir.join(NARRATION_FILE);
    let good = std::fs::read(&sidecar).unwrap();
    let content = embedded_content_pack();
    let load = || load_game_checked_with_content_hash(dir.path(), &content.content_hash_hex);
    let with_good = load().unwrap();
    assert_eq!(with_good.turns_stepped, 3);
    assert_eq!(read_narration_sidecar(&save_dir, &run_id), 3);

    let other_run = serde_json::json!({"run_id": "another-run", "turns_stepped": 9}).to_string();
    let mut cases: Vec<(&str, Option<Vec<u8>>)> = vec![
        ("missing", None),
        ("not UTF-8", Some(vec![0xff, 0xfe, 0x00, 0x7b])),
        ("not JSON", Some(b"{\"run_id\": ".to_vec())),
        ("not an object", Some(b"[3]".to_vec())),
        ("another run", Some(other_run.into_bytes())),
    ];
    for bad in [
        serde_json::json!(-1),
        serde_json::json!("3"),
        serde_json::json!(1.5),
        serde_json::Value::Null,
    ] {
        let text = serde_json::json!({"run_id": run_id, "turns_stepped": bad}).to_string();
        cases.push(("a count that is not one", Some(text.into_bytes())));
    }
    for (label, bytes) in cases {
        match &bytes {
            None => std::fs::remove_file(&sidecar).unwrap(),
            Some(b) => std::fs::write(&sidecar, b).unwrap(),
        }
        let loaded = load().unwrap_or_else(|e| panic!("{label}: the load failed: {e:?}"));
        assert_eq!(loaded.turns_stepped, 0, "{label}");
        let mut same = loaded.clone();
        same.turns_stepped = with_good.turns_stepped;
        assert_eq!(same, with_good, "{label}: only the counter may differ");
        // The bad file is left where it is; no quarantine copy is made of it.
        let names: BTreeSet<String> = std::fs::read_dir(&save_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.contains("corrupt")),
            "{label}: {names:?}"
        );
        assert_eq!(sidecar.exists(), bytes.is_some(), "{label}");
        std::fs::write(&sidecar, &good).unwrap();
    }
    assert_eq!(load().unwrap().turns_stepped, 3);
}

#[test]
fn the_v4_run_json_is_unchanged_by_the_sidecar() {
    // run.json is exactly what the frozen v4 writer makes of the state; the counter is only in
    // the sidecar. The frozen Python vectors are checked against this writer in oracle_parity.
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_at(new_run(42, dir.path()), dir.path());
    for p in ["hard", "slow"] {
        step(&mut engine, &pace(p));
    }
    let text = String::from_utf8(run_json(dir.path())).unwrap();
    assert_eq!(
        text,
        serde_json::to_string_pretty(&state_to_value(&engine.state)).unwrap()
    );
    assert!(!text.contains("turns_stepped"));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["save_version"], serde_json::json!(4));
    let sidecar: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(SAVE_DIR).join(NARRATION_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        sidecar,
        serde_json::json!({"run_id": engine.state.run_id, "turns_stepped": 2})
    );
}

#[test]
fn a_failed_sidecar_write_leaves_run_json_untouched() {
    // The write order is what keeps the sidecar ahead of run.json. With the sidecar write made to
    // fail (a directory where the file goes), save_game must fail before it touches run.json.
    // Swapping the two writes in save_game turns this test red.
    let dir = tempfile::tempdir().unwrap();
    let mut state = new_run(42, dir.path());
    let before = run_json(dir.path());
    let sidecar = dir.path().join(SAVE_DIR).join(NARRATION_FILE);
    std::fs::remove_file(&sidecar).unwrap();
    std::fs::create_dir(&sidecar).unwrap();

    state.day += 1;
    state.turns_stepped = 5;
    assert!(
        save_game(&state, dir.path()).is_err(),
        "the sidecar write must fail"
    );
    assert_eq!(run_json(dir.path()), before, "run.json changed");

    // The engine's autosave takes the same path and ignores the error: run.json still holds.
    let mut engine = engine_at(state, dir.path());
    step(&mut engine, &pace("hard"));
    assert_eq!(
        run_json(dir.path()),
        before,
        "run.json changed on an engine step"
    );
}

#[test]
fn a_count_at_the_top_neither_panics_nor_wraps() {
    // A sidecar or a v5 save can be edited by hand to hold any count. The counter saturates: at
    // u64::MAX it stays there, in debug (no overflow panic) and in release (no wrap to 0). The
    // player sees nothing different: the run plays as it would, and only the narrator's turn
    // index stops counting.
    let content = embedded_content_pack();
    for start in [u64::MAX, u64::MAX - 1] {
        let dir = tempfile::tempdir().unwrap();
        let state = new_run(42, dir.path());
        let text = serde_json::json!({"run_id": state.run_id, "turns_stepped": start}).to_string();
        std::fs::write(dir.path().join(SAVE_DIR).join(NARRATION_FILE), text).unwrap();
        let mut engine = resume(dir.path());
        assert_eq!(engine.state.turns_stepped, start);
        let plain_dir = tempfile::tempdir().unwrap();
        let mut plain = engine_at(state.clone(), plain_dir.path());
        for _ in 0..3 {
            let t = step(&mut engine, &pace("hard"));
            assert_eq!(t.ctx.turn_index(), u64::MAX, "from {start}");
            step(&mut plain, &pace("hard"));
        }
        assert_eq!(engine.state.turns_stepped, u64::MAX);
        // Mechanics are the run's own: only the counter differs from a run that started at 0.
        let mut same = engine.state.clone();
        same.turns_stepped = plain.state.turns_stepped;
        assert_eq!(same, plain.state);
        drop(engine);
        assert_eq!(resume(dir.path()).state.turns_stepped, u64::MAX);
    }

    // A v5 save at the top.
    let (mut state, _) = etv_core::worldgen::create_new_run_with_rng(9, GMProfile::Fireside, 2);
    state.content_hash = content.content_hash_hex.clone();
    state.turns_stepped = u64::MAX;
    let value = state_to_value_v5(&state, None);
    assert_eq!(value["turns_stepped"].as_u64(), Some(u64::MAX));
    let loaded = value_to_saved_game(&value, content).unwrap();
    assert_eq!(loaded.state.turns_stepped, u64::MAX);
    let mut engine = StepEngine::with_content(loaded.state, content);
    engine.set_ruleset(Ruleset::V2);
    for turn in 1..=3 {
        let (intent, _) = choose_intent(Policy::Cautious, &engine, 9, turn);
        engine.step(&intent);
        assert_eq!(engine.state.turns_stepped, u64::MAX);
    }
}

#[test]
fn an_oversized_sidecar_reads_zero() {
    // The read stops after a fixed number of bytes; a larger file is not a sidecar.
    let dir = tempfile::tempdir().unwrap();
    let state = new_run(42, dir.path());
    let save_dir = dir.path().join(SAVE_DIR);
    let body = serde_json::json!({"run_id": state.run_id, "turns_stepped": 7}).to_string();
    let limit = NARRATION_FILE_MAX_BYTES as usize;

    // Exactly at the limit (padded with JSON whitespace): still read.
    let at_limit = format!("{body}{}", " ".repeat(limit - body.len()));
    assert_eq!(at_limit.len(), limit);
    std::fs::write(save_dir.join(NARRATION_FILE), &at_limit).unwrap();
    assert_eq!(read_narration_sidecar(&save_dir, &state.run_id), 7);

    // One byte over, and far over: read as no sidecar, and the run still loads.
    for size in [limit + 1, 8 * 1024 * 1024] {
        let big = format!("{body}{}", " ".repeat(size - body.len()));
        std::fs::write(save_dir.join(NARRATION_FILE), &big).unwrap();
        assert_eq!(
            read_narration_sidecar(&save_dir, &state.run_id),
            0,
            "{size}"
        );
        assert_eq!(resume(dir.path()).state.turns_stepped, 0, "{size}");
    }
}
