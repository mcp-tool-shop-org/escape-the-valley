//! Phase 3: the line-mode play loop (`etv new`, `etv continue`).
//!
//! The golden transcript in `tests/fixtures/play_seed42_transcript.txt` is a regression pin of
//! what the player sees for one scripted run. When a change to the shown text is meant, write it
//! again with `ETV_WRITE_GOLDEN=1 cargo test --test phase3_play_line_mode golden` and read the
//! diff before committing it.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::process::{Command, Stdio};

use etv_core::content::{embedded_content_pack, parse_cli_args, CliCommand};
use etv_core::engine::{EventChoiceInfo, StepEngine};
use etv_core::model::{GamePhase, IntentAction, PlayerIntent};
use etv_core::narrator::{FallbackPhraseTable, NarrationContext, Narrator, NullNarrator};
use etv_core::play::{
    choice_line, deltas_line, parse_camp_input, parse_choice_input, play_turn, run_play,
    status_lines, CampCommand, PlayExit, CAMP_HINT,
};
use etv_core::save::{state_to_value, SAVE_DIR, SAVE_FILE};
use etv_core::worldgen::create_new_run_with_content;

fn new_engine(seed: i128) -> StepEngine {
    let content = embedded_content_pack();
    StepEngine::with_content(create_new_run_with_content(seed, content), content)
}

fn act(action: IntentAction) -> Option<CampCommand> {
    Some(CampCommand::Act(PlayerIntent::new(action)))
}

#[test]
fn camp_input_accepts_letters_and_action_names() {
    assert_eq!(parse_camp_input("t"), act(IntentAction::Travel));
    assert_eq!(parse_camp_input("  Travel \n"), act(IntentAction::Travel));
    assert_eq!(parse_camp_input("R"), act(IntentAction::Rest));
    assert_eq!(parse_camp_input("rest"), act(IntentAction::Rest));
    assert_eq!(parse_camp_input("h"), act(IntentAction::Hunt));
    assert_eq!(parse_camp_input("HUNT"), act(IntentAction::Hunt));
    assert_eq!(parse_camp_input("f"), act(IntentAction::Repair));
    assert_eq!(parse_camp_input("fix"), act(IntentAction::Repair));
    assert_eq!(parse_camp_input("repair"), act(IntentAction::Repair));
    assert_eq!(parse_camp_input("x"), act(IntentAction::AbandonCargo));
    assert_eq!(
        parse_camp_input("abandon cargo"),
        act(IntentAction::AbandonCargo)
    );
    assert_eq!(parse_camp_input("d"), act(IntentAction::DesperateRepair));
    assert_eq!(
        parse_camp_input("desperate repair"),
        act(IntentAction::DesperateRepair)
    );
    assert_eq!(parse_camp_input("n"), act(IntentAction::HardRation));
    assert_eq!(
        parse_camp_input("hard ration"),
        act(IntentAction::HardRation)
    );
    assert_eq!(parse_camp_input("q"), Some(CampCommand::Quit));
    assert_eq!(parse_camp_input("Quit"), Some(CampCommand::Quit));

    for pace in ["slow", "steady", "hard"] {
        let expected = Some(CampCommand::Act(
            PlayerIntent::new(IntentAction::ChangePace).with_pace(pace),
        ));
        assert_eq!(parse_camp_input(&format!("p {pace}")), expected);
        assert_eq!(parse_camp_input(&format!("PACE {pace}")), expected);
    }

    for bad in [
        "",
        "   ",
        "a",
        "1",
        "p",
        "pace",
        "pace fast",
        "pace hard now",
        "travel far",
        "hard",
        "rest rest",
    ] {
        assert_eq!(parse_camp_input(bad), None, "input {bad:?}");
    }
}

#[test]
fn choice_input_accepts_offered_letters_and_numbers_only() {
    let offered = vec!["A".to_string(), "B".to_string(), "C".to_string()];
    assert_eq!(parse_choice_input("A", &offered), Some("A".to_string()));
    assert_eq!(parse_choice_input(" b \n", &offered), Some("B".to_string()));
    assert_eq!(parse_choice_input("1", &offered), Some("A".to_string()));
    assert_eq!(parse_choice_input("3", &offered), Some("C".to_string()));
    for bad in ["", "D", "0", "4", "travel", "-1", "AB"] {
        assert_eq!(parse_choice_input(bad, &offered), None, "input {bad:?}");
    }
    assert_eq!(parse_choice_input("A", &[]), None);
}

#[test]
fn choice_line_joins_hints_without_doubling_punctuation() {
    let mut choice = EventChoiceInfo {
        id: "A".to_string(),
        label: "Wait it out".to_string(),
        risk_hint: String::new(),
        cost_hint: String::new(),
        ..Default::default()
    };
    assert_eq!(choice_line(&choice), "  A. Wait it out");
    choice.cost_hint = "A day.".to_string();
    assert_eq!(choice_line(&choice), "  A. Wait it out (A day)");
    choice.risk_hint = "Time cost, animal recovers.".to_string();
    assert_eq!(
        choice_line(&choice),
        "  A. Wait it out (A day; Time cost, animal recovers)"
    );
    choice.cost_hint = "  ".to_string();
    assert_eq!(
        choice_line(&choice),
        "  A. Wait it out (Time cost, animal recovers)"
    );
}

#[test]
fn deltas_line_shows_signed_numbers_in_key_order_and_skips_zeroes() {
    let mut deltas = BTreeMap::new();
    assert_eq!(deltas_line(&deltas), None);
    deltas.insert("water".to_string(), 3);
    deltas.insert("food".to_string(), -2);
    deltas.insert("ammo".to_string(), 0);
    assert_eq!(deltas_line(&deltas), Some("Food -2, Water +3".to_string()));
    deltas.clear();
    deltas.insert("ammo".to_string(), 0);
    assert_eq!(deltas_line(&deltas), None);
}

/// Every supply the party carries is on the status panel, tools and rope included.
#[test]
fn status_panel_for_seed_42_at_the_start() {
    let engine = new_engine(42);
    assert_eq!(
        status_lines(&engine.state),
        vec![
            "Day 1, morning. Coldspring (plains). 0 of 396 miles, 21 to the next stop.".to_string(),
            "Party: Ruth 100 (healthy), Josiah 94 (healthy), Elias 87 (healthy), Agnes 80 (healthy). Morale 70."
                .to_string(),
            "Wagon 80, animals 90, pace steady.".to_string(),
            "Supplies: Ammunition 20, Boots 2, Cloth 5, Firewood 15, Food 50, Lantern oil 6, Medicine 5, Parts 4, Rope 2, Salt 8, Tools 1, Water 50."
                .to_string(),
        ]
    );

    let mut state = engine.state.clone();
    state.party.members[1].health = 0;
    assert_eq!(
        status_lines(&state)[1],
        "Party: Ruth 100 (healthy), Josiah (dead), Elias 87 (healthy), Agnes 80 (healthy). Morale 70."
    );
}

/// A narrator that always answers, to tell the narrated view from the narrator-off view.
struct FixedNarrator;

impl Narrator for FixedNarrator {
    fn narrate(&self, _ctx: &NarrationContext) -> Option<String> {
        Some("FIXTURE NARRATION.".to_string())
    }
}

/// Travel until an event is pending, then return the first offered choice.
fn travel_to_first_event(engine: &mut StepEngine) -> EventChoiceInfo {
    for _ in 0..50 {
        if engine.phase == GamePhase::Event {
            return engine.pending_event_choices()[0].clone();
        }
        assert_eq!(engine.phase, GamePhase::Camp);
        engine.step(&PlayerIntent::new(IntentAction::Travel));
    }
    panic!("no event in 50 travel steps");
}

/// With the narrator off, the chosen option is shown beside its fallback line, once, followed by
/// the numbers. On seed 42 the first event is Persistent Rain and its first choice is
/// "Continue despite the wet".
#[test]
fn narrator_off_shows_the_chosen_option_beside_the_fallback_line_once() {
    let table = FallbackPhraseTable::embedded();
    let mut engine = new_engine(42);
    let choice = travel_to_first_event(&mut engine);
    assert_eq!(choice.label, "Continue despite the wet");
    let intent = PlayerIntent::new(IntentAction::Choose).with_choice(&choice.id);
    let lines = play_turn(
        &mut engine,
        &NullNarrator,
        Some(&table),
        &intent,
        Some(&choice.label),
    );
    assert_eq!(
        lines,
        vec![
            "[Continue despite the wet] The party damages the wagon, loses heart, falls back on the trail and loses time."
                .to_string(),
            "  Distance -4, Morale -2, Days +1, Wagon condition -2".to_string(),
        ]
    );
}

/// With a narrator that answers, its text replaces the bracketed fallback line; the numbers stay.
#[test]
fn narrated_turn_shows_the_narrator_text_and_the_numbers() {
    let table = FallbackPhraseTable::embedded();
    let mut engine = new_engine(42);
    let choice = travel_to_first_event(&mut engine);
    let intent = PlayerIntent::new(IntentAction::Choose).with_choice(&choice.id);
    let lines = play_turn(
        &mut engine,
        &FixedNarrator,
        Some(&table),
        &intent,
        Some(&choice.label),
    );
    assert_eq!(
        lines,
        vec![
            "FIXTURE NARRATION.".to_string(),
            "  Distance -4, Morale -2, Days +1, Wagon condition -2".to_string(),
        ]
    );
}

/// Without a phrase table the engine's own plain line is the fallback.
#[test]
fn missing_phrase_table_falls_back_to_the_engine_line() {
    let mut engine = new_engine(42);
    let choice = travel_to_first_event(&mut engine);
    let intent = PlayerIntent::new(IntentAction::Choose).with_choice(&choice.id);
    let lines = play_turn(
        &mut engine,
        &NullNarrator,
        None,
        &intent,
        Some(&choice.label),
    );
    assert_eq!(
        lines[0],
        "[Continue despite the wet] wagon -2, morale -2, 1 time lost"
    );
}

/// At camp `a` is refused and at an event `q` is refused, so the closing `a`, `q` always ends
/// the script at camp with a save-and-quit.
const SCRIPT_SEED_42: &str = "t\nzzz\na\nt\nt\nt\nhunt\npace hard\nt\n1\nt\nrest\na\nq\n";

fn scripted_transcript(narrator: &dyn Narrator) -> (PlayExit, String, StepEngine) {
    let table = FallbackPhraseTable::embedded();
    let mut engine = new_engine(42);
    let mut out = Vec::new();
    let exit = run_play(
        &mut engine,
        narrator,
        Some(&table),
        "TEST",
        Cursor::new(SCRIPT_SEED_42),
        &mut out,
    )
    .unwrap();
    (exit, String::from_utf8(out).unwrap(), engine)
}

#[test]
fn golden_transcript_seed_42() {
    let (exit, transcript, _) = scripted_transcript(&NullNarrator);
    assert_eq!(exit, PlayExit::Quit);
    assert!(!transcript.contains('\r'));
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/play_seed42_transcript.txt"
    );
    if std::env::var_os("ETV_WRITE_GOLDEN").is_some() {
        std::fs::write(path, &transcript).unwrap();
    }
    let golden = std::fs::read_to_string(path)
        .expect("golden transcript fixture")
        .replace("\r\n", "\n");
    assert_eq!(transcript, golden);

    // Facts about the transcript that do not depend on the fixture.
    assert!(transcript
        .starts_with("ESCAPE THE VALLEY\nversion TEST\n\nWhen a narrator model is running"));
    assert!(transcript.contains("> Choose one of: A, B.\n"));
    assert!(transcript.ends_with("> Saved. Come back with: etv continue\n"));
}

/// The play loop only presents: the same intents stepped straight on the engine give the same
/// state, with or without a narrator.
#[test]
fn play_loop_does_not_change_the_run() {
    let (_, _, played) = scripted_transcript(&NullNarrator);
    let (_, narrated_text, narrated) = scripted_transcript(&FixedNarrator);
    assert!(narrated_text.contains("FIXTURE NARRATION."));

    // The script, as the intents it resolves to: t, (zzz and a are refused or accepted by phase).
    let mut direct = new_engine(42);
    for line in SCRIPT_SEED_42.lines() {
        let intent = match direct.phase {
            GamePhase::Camp => match parse_camp_input(line) {
                Some(CampCommand::Act(intent)) => intent,
                _ => continue,
            },
            GamePhase::Event => {
                let offered: Vec<String> = direct
                    .pending_event_choices()
                    .iter()
                    .map(|c| c.id.clone())
                    .collect();
                match parse_choice_input(line, &offered) {
                    Some(id) => PlayerIntent::new(IntentAction::Choose).with_choice(&id),
                    None => continue,
                }
            }
            _ => panic!("the script reaches no fork or ending"),
        };
        direct.step(&intent);
    }
    // The run id is random per new run; everything else must match.
    let comparable = |engine: &StepEngine| {
        let mut value = state_to_value(&engine.state);
        value.as_object_mut().unwrap().remove("run_id");
        value
    };
    let direct_value = comparable(&direct);
    assert_eq!(comparable(&played), direct_value);
    assert_eq!(comparable(&narrated), direct_value);
    assert_eq!(played.rng.counter, direct.rng.counter);
    assert_eq!(narrated.rng.counter, direct.rng.counter);
}

/// A whole run plays to its ending on plain input, and ends with the Trail Ledger.
#[test]
fn a_full_run_reaches_an_ending_with_the_narrator_off() {
    let table = FallbackPhraseTable::embedded();
    for seed in [1i128, 42, 2026] {
        let mut engine = new_engine(seed);
        let mut out = Vec::new();
        // At camp "a" is refused and "t" travels; at an event or fork "t" is refused and "a"
        // chooses. So this input always moves the run on.
        let script = "t\na\n".repeat(3000);
        let exit = run_play(
            &mut engine,
            &NullNarrator,
            Some(&table),
            "TEST",
            Cursor::new(script),
            &mut out,
        )
        .unwrap();
        assert_eq!(exit, PlayExit::Finished, "seed {seed}");
        assert!(engine.state.game_over);
        let transcript = String::from_utf8(out).unwrap();
        let ledger = etv_core::trail_ledger(&engine.state).join("\n");
        assert!(!ledger.is_empty());
        assert!(transcript.ends_with(&format!("{ledger}\n")), "seed {seed}");
        // Every event shown is resolved once, and shows its bracketed option line once.
        let resolved = transcript.lines().filter(|l| l.starts_with("== ")).count();
        assert!(resolved > 0);
        let bracketed = transcript
            .lines()
            .filter(|l| l.trim_start_matches("> ").starts_with('['))
            .count();
        assert_eq!(bracketed, resolved, "seed {seed}");
    }
}

#[test]
fn input_ending_mid_run_stops_cleanly() {
    let table = FallbackPhraseTable::embedded();
    let mut engine = new_engine(42);
    let mut out = Vec::new();
    let exit = run_play(
        &mut engine,
        &NullNarrator,
        Some(&table),
        "TEST",
        Cursor::new("t\n"),
        &mut out,
    )
    .unwrap();
    assert_eq!(exit, PlayExit::InputEnded);

    // A line that is not understood at camp prints the hint, asks again, and plays no turn.
    let mut engine = new_engine(42);
    let mut out = Vec::new();
    let exit = run_play(
        &mut engine,
        &NullNarrator,
        Some(&table),
        "TEST",
        Cursor::new("zzz\nq\n"),
        &mut out,
    )
    .unwrap();
    assert_eq!(exit, PlayExit::Quit);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains(&format!("> {CAMP_HINT}\n> Saved.")));
    assert_eq!(engine.rng.counter, new_engine(42).rng.counter);
}

#[test]
fn cli_parses_new_and_continue() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        parse_cli_args(&args(&["new"])).unwrap().command,
        CliCommand::New {
            seed: None,
            force: false
        }
    );
    assert_eq!(
        parse_cli_args(&args(&["new", "--seed", "42", "--force"]))
            .unwrap()
            .command,
        CliCommand::New {
            seed: Some(42),
            force: true
        }
    );
    assert_eq!(
        parse_cli_args(&args(&["continue"])).unwrap().command,
        CliCommand::Continue
    );
    assert_eq!(
        parse_cli_args(&args(&["new", "--seed"])).unwrap_err(),
        "missing value for '--seed'"
    );
    assert!(parse_cli_args(&args(&["new", "--seed", "x"]))
        .unwrap_err()
        .starts_with("invalid '--seed' value 'x'"));
}

fn run_etv(
    dir: &std::path::Path,
    args: &[&str],
    stdin_text: &str,
) -> (Option<i32>, String, String) {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_etv"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run etv");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_text.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
    )
}

/// `etv new` then `etv continue` plays the same run as one uninterrupted session.
#[test]
fn etv_binary_new_quit_continue_matches_one_session() {
    let save = |dir: &std::path::Path| {
        let text = std::fs::read_to_string(dir.join(SAVE_DIR).join(SAVE_FILE)).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        // The run id is random per new run; everything else must match.
        value.as_object_mut().unwrap().remove("run_id");
        value
    };

    let split = tempfile::tempdir().unwrap();
    let (code, stdout, _) = run_etv(split.path(), &["new", "--seed", "42"], "t\na\nt\nq\n");
    assert_eq!(code, Some(0));
    assert!(stdout.starts_with("New run. Seed 42.\n"));
    assert!(stdout.contains("\nESCAPE THE VALLEY\n"));
    assert!(stdout.ends_with("> Saved. Come back with: etv continue\n"));
    let (code, stdout, _) = run_etv(split.path(), &["continue"], "t\nt\nhunt\nq\n");
    assert_eq!(code, Some(0));
    assert!(stdout.contains("Day 1, night."), "{stdout}");

    let whole = tempfile::tempdir().unwrap();
    let (code, _, _) = run_etv(
        whole.path(),
        &["new", "--seed", "42"],
        "t\na\nt\nt\nt\nhunt\nq\n",
    );
    assert_eq!(code, Some(0));
    assert_eq!(save(split.path()), save(whole.path()));
}

#[test]
fn etv_binary_refuses_to_overwrite_or_resume_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (code, _, stderr) = run_etv(dir.path(), &["continue"], "");
    assert_eq!(code, Some(1));
    assert_eq!(
        stderr,
        "error: no saved run here. Start one with 'etv new'.\n"
    );

    let (code, _, _) = run_etv(dir.path(), &["new", "--seed", "7"], "q\n");
    assert_eq!(code, Some(0));
    let before = std::fs::read_to_string(dir.path().join(SAVE_DIR).join(SAVE_FILE)).unwrap();

    let (code, stdout, stderr) = run_etv(dir.path(), &["new", "--seed", "8"], "q\n");
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "error: a saved run exists here. Resume it with 'etv continue', or replace it with 'etv new --force'.\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(SAVE_DIR).join(SAVE_FILE)).unwrap(),
        before
    );

    let (code, stdout, _) = run_etv(dir.path(), &["new", "--seed", "8", "--force"], "q\n");
    assert_eq!(code, Some(0));
    assert!(stdout.starts_with("New run. Seed 8.\n"));

    // A run with no seed given picks one and says which.
    let fresh = tempfile::tempdir().unwrap();
    let (code, stdout, _) = run_etv(fresh.path(), &["new"], "");
    assert_eq!(code, Some(0));
    assert!(stdout.starts_with("New run. Seed "));
}
